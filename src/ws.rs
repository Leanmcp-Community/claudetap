use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use time::OffsetDateTime;
use anyhow::Result;
use serde::Serialize;


#[derive(Serialize)]
pub struct WsFrameLog {
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    pub dir: &'static str,
    pub op: &'static str,
    pub fin: bool,
    pub len: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rsv1: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rsv2: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rsv3: Option<bool>,
}

fn opcode_str(op: u8) -> &'static str {
    match op {
        0 => "cont",
        1 => "text",
        2 => "binary",
        8 => "close",
        9 => "ping",
        10 => "pong",
        _ => "unknown",
    }
}

pub async fn proxy_ws_stream<R, W>(
    mut read: R,
    mut write: W,
    dir: &'static str,
    log_file: Arc<Mutex<tokio::fs::File>>,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    loop {
        let mut b = [0u8; 2];
        let n = read.read(&mut b).await?;
        if n == 0 { break; } // EOF
        if n == 1 { 
            read.read_exact(&mut b[1..2]).await?;
        }

        write.write_all(&b).await?;

        let fin = (b[0] & 0x80) != 0;
        let rsv1 = (b[0] & 0x40) != 0;
        let rsv2 = (b[0] & 0x20) != 0;
        let rsv3 = (b[0] & 0x10) != 0;
        let opcode = b[0] & 0x0F;
        let masked = (b[1] & 0x80) != 0;
        let mut payload_len = (b[1] & 0x7F) as u64;

        if payload_len == 126 {
            let mut ext = [0u8; 2];
            read.read_exact(&mut ext).await?;
            write.write_all(&ext).await?;
            payload_len = u16::from_be_bytes(ext) as u64;
        } else if payload_len == 127 {
            let mut ext = [0u8; 8];
            read.read_exact(&mut ext).await?;
            write.write_all(&ext).await?;
            payload_len = u64::from_be_bytes(ext);
        }

        let mut mask_key = [0u8; 4];
        if masked {
            read.read_exact(&mut mask_key).await?;
            write.write_all(&mask_key).await?;
        }

        let mut payload = vec![0u8; payload_len as usize];
        if payload_len > 0 {
            read.read_exact(&mut payload).await?;
            write.write_all(&payload).await?;
        }

        // Unmask for logging only
        if masked {
            for i in 0..payload_len as usize {
                payload[i] ^= mask_key[i % 4];
            }
        }

        let op_str = opcode_str(opcode);
        let mut log = WsFrameLog {
            ts: OffsetDateTime::now_utc(),
            dir,
            op: op_str,
            fin,
            len: payload_len,
            payload: None,
            payload_b64: None,
            code: None,
            reason: None,
            rsv1: if rsv1 { Some(true) } else { None },
            rsv2: if rsv2 { Some(true) } else { None },
            rsv3: if rsv3 { Some(true) } else { None },
        };

        if opcode == 1 { // text
            if let Ok(s) = std::str::from_utf8(&payload) {
                log.payload = Some(s.to_string());
            } else {
                log.payload_b64 = Some(crate::log::InlineBody::from_bytes(&payload).to_b64());
            }
        } else if opcode == 8 { // close
            if payload.len() >= 2 {
                log.code = Some(u16::from_be_bytes([payload[0], payload[1]]));
                if let Ok(s) = std::str::from_utf8(&payload[2..]) {
                    log.reason = Some(s.to_string());
                }
            }
        } else if payload_len > 0 {
            log.payload_b64 = Some(crate::log::InlineBody::from_bytes(&payload).to_b64());
        }

        let mut line = serde_json::to_vec(&log)?;
        line.push(b'\n');
        
        let mut f = log_file.lock().await;
        f.write_all(&line).await?;
        // Flush every frame so `tail -f`, the Python `inspect_ws.py`, and the
        // TUI detail view see frames the moment they arrive — instead of
        // waiting for the connection to close (which is what an unflushed
        // tokio::fs::File can effectively do once the OS page cache is
        // involved). This is per-frame work but each frame is already a
        // small JSON line so the syscall overhead is negligible compared
        // to the TLS+forward path we already pay.
        f.flush().await?;
    }
    Ok(())
}
