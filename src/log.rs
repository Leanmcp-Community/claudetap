//! Append-only session logger.
//!
//! IMPORTANT: this module never deletes, rotates, truncates, or otherwise
//! removes any captured data on disk. Once a byte lands here it stays.
//! There is no `--max-session-bytes`, no expiry, no cleanup task.
//!
//! Layout under `~/.claudetap/sessions/<ulid>/`:
//!
//! ```text
//! meta.json                       # session metadata (claude argv, pid, start ts)
//! traffic.jsonl                   # one JSON record per request/response pair
//! bodies/<req-id>.req.bin         # request body if larger than inline threshold
//! bodies/<req-id>.res.bin         # response body if non-streaming
//! stream/<req-id>.sse.jsonl       # per-event log for SSE responses
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::paths;

/// Headers that should be hashed instead of written verbatim when redaction
/// is on (the default).
const REDACT_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "cookie",
    "set-cookie",
    "proxy-authorization",
];

/// Bodies <= this many bytes are inlined into `traffic.jsonl`. Larger bodies
/// spill to `bodies/<id>.{req,res}.bin`. The threshold only affects layout,
/// never retention.
const INLINE_BODY_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    pub claude_argv: Vec<String>,
    pub claude_path: String,
    pub claude_pid: Option<u32>,
    pub proxy_addr: String,
    pub redact: bool,
    pub host_filter: Vec<String>,
    pub claudetap_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrafficRecord {
    pub id: String,
    #[serde(with = "time::serde::rfc3339")]
    pub ts_start: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub ts_end: OffsetDateTime,
    pub method: String,
    pub url: String,
    pub http_version: String,
    pub upstream_addr: Option<String>,
    pub request: SidePayload,
    pub response: ResponsePayload,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SidePayload {
    pub headers: Vec<[String; 2]>,
    pub body_size: usize,
    pub body_inline: Option<InlineBody>,
    pub body_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponsePayload {
    pub status: u16,
    pub headers: Vec<[String; 2]>,
    pub body_size: usize,
    pub body_inline: Option<InlineBody>,
    pub body_path: Option<String>,
    pub is_stream: bool,
    pub stream_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "encoding", content = "data")]
pub enum InlineBody {
    #[serde(rename = "utf8")]
    Utf8(String),
    #[serde(rename = "base64")]
    Base64(String),
}

impl InlineBody {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        match std::str::from_utf8(bytes) {
            Ok(s) => InlineBody::Utf8(s.to_string()),
            Err(_) => InlineBody::Base64(b64::encode(bytes)),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SseEvent {
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    pub event: Option<String>,
    pub id: Option<String>,
    pub retry: Option<u64>,
    pub data: String,
}

/// Owns the session directory and the locked traffic.jsonl writer.
///
/// Append-only by design. The file is opened with `create(true).append(true)`
/// and never truncated. Crashes leave a valid prefix.
pub struct SessionLogger {
    dir: PathBuf,
    bodies_dir: PathBuf,
    stream_dir: PathBuf,
    traffic: Mutex<tokio::fs::File>,
    redact: bool,
    #[allow(dead_code)]
    pub session_id: String,
}

impl SessionLogger {
    pub async fn create(session_id: String, redact: bool) -> Result<Arc<Self>> {
        let dir = paths::session_dir(&session_id)?;
        paths::ensure_dir(&dir)?;
        let bodies_dir = dir.join("bodies");
        paths::ensure_dir(&bodies_dir)?;
        let stream_dir = dir.join("stream");
        paths::ensure_dir(&stream_dir)?;

        let traffic_path = dir.join("traffic.jsonl");
        let traffic = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&traffic_path)
            .await
            .with_context(|| format!("opening {}", traffic_path.display()))?;

        Ok(Arc::new(Self {
            dir,
            bodies_dir,
            stream_dir,
            traffic: Mutex::new(traffic),
            redact,
            session_id,
        }))
    }

    pub async fn write_meta(&self, meta: &SessionMeta) -> Result<()> {
        let path = self.dir.join("meta.json");
        let bytes = serde_json::to_vec_pretty(meta)?;
        tokio::fs::write(&path, &bytes)
            .await
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn redact(&self) -> bool {
        self.redact
    }

    #[allow(dead_code)]
    pub fn session_dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn redact_headers(&self, headers: &[(String, Vec<u8>)]) -> Vec<[String; 2]> {
        headers
            .iter()
            .map(|(k, v)| {
                let val = if self.redact && REDACT_HEADERS.iter().any(|r| r.eq_ignore_ascii_case(k))
                {
                    let mut h = Sha256::new();
                    h.update(v);
                    let digest = hex::encode(h.finalize());
                    format!("<redacted:sha256:{}>", &digest[..16])
                } else {
                    String::from_utf8_lossy(v).to_string()
                };
                [k.clone(), val]
            })
            .collect()
    }

    /// Persist a request body. Returns (size, optional path, optional inline payload).
    pub async fn store_request_body(
        &self,
        req_id: &str,
        body: &[u8],
    ) -> Result<(usize, Option<String>, Option<InlineBody>)> {
        if body.is_empty() {
            return Ok((0, None, None));
        }
        if body.len() <= INLINE_BODY_LIMIT {
            return Ok((body.len(), None, Some(InlineBody::from_bytes(body))));
        }
        let rel = format!("bodies/{req_id}.req.bin");
        let path = self.bodies_dir.join(format!("{req_id}.req.bin"));
        write_append(&path, body).await?;
        Ok((body.len(), Some(rel), None))
    }

    pub async fn store_response_body(
        &self,
        req_id: &str,
        body: &[u8],
    ) -> Result<(usize, Option<String>, Option<InlineBody>)> {
        if body.is_empty() {
            return Ok((0, None, None));
        }
        if body.len() <= INLINE_BODY_LIMIT {
            return Ok((body.len(), None, Some(InlineBody::from_bytes(body))));
        }
        let rel = format!("bodies/{req_id}.res.bin");
        let path = self.bodies_dir.join(format!("{req_id}.res.bin"));
        write_append(&path, body).await?;
        Ok((body.len(), Some(rel), None))
    }

    /// Open an append-only handle for streaming response chunks. Returns the
    /// path on disk plus the file handle for caller-driven appends.
    pub async fn open_stream_log(&self, req_id: &str) -> Result<(String, tokio::fs::File)> {
        let rel = format!("stream/{req_id}.sse.jsonl");
        let path = self.stream_dir.join(format!("{req_id}.sse.jsonl"));
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("opening {}", path.display()))?;
        Ok((rel, file))
    }

    /// Append one SSE event, JSON-encoded, to the per-request stream log.
    pub async fn append_sse_event(file: &mut tokio::fs::File, event: &SseEvent) -> Result<()> {
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        file.write_all(&line).await?;
        Ok(())
    }

    /// Append a raw response chunk to a per-request `.bin` file. Used for
    /// non-SSE streaming responses where we want the bytes preserved 1:1.
    #[allow(dead_code)]
    pub async fn append_response_chunk(
        &self,
        req_id: &str,
        chunk: &[u8],
    ) -> Result<String> {
        let rel = format!("bodies/{req_id}.res.bin");
        let path = self.bodies_dir.join(format!("{req_id}.res.bin"));
        write_append(&path, chunk).await?;
        Ok(rel)
    }

    pub async fn append_traffic(&self, record: &TrafficRecord) -> Result<()> {
        let mut line = serde_json::to_vec(record)?;
        line.push(b'\n');
        let mut f = self.traffic.lock().await;
        f.write_all(&line).await?;
        Ok(())
    }
}

async fn write_append(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let mut f = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .with_context(|| format!("opening {}", path.display()))?;
    f.write_all(bytes).await?;
    Ok(())
}

mod b64 {
    const CHARS: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(input: &[u8]) -> String {
        let mut out = String::with_capacity(((input.len() + 2) / 3) * 4);
        let mut i = 0;
        while i + 3 <= input.len() {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
            out.push(CHARS[((n >> 18) & 0x3f) as usize] as char);
            out.push(CHARS[((n >> 12) & 0x3f) as usize] as char);
            out.push(CHARS[((n >> 6) & 0x3f) as usize] as char);
            out.push(CHARS[(n & 0x3f) as usize] as char);
            i += 3;
        }
        let rem = input.len() - i;
        if rem == 1 {
            let n = (input[i] as u32) << 16;
            out.push(CHARS[((n >> 18) & 0x3f) as usize] as char);
            out.push(CHARS[((n >> 12) & 0x3f) as usize] as char);
            out.push('=');
            out.push('=');
        } else if rem == 2 {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
            out.push(CHARS[((n >> 18) & 0x3f) as usize] as char);
            out.push(CHARS[((n >> 12) & 0x3f) as usize] as char);
            out.push(CHARS[((n >> 6) & 0x3f) as usize] as char);
            out.push('=');
        }
        out
    }
}
