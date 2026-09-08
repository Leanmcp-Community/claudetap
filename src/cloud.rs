//! Optional file-backed cloud uploader. Never modifies captured sessions.
use anyhow::{bail, Context, Result};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Enable uploads of future sessions. Key is read from CLAUDETAP_UPLOAD_KEY.
    Configure {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        backfill: bool,
    },
    Status,
    Disable,
    /// Run continuously, or make one pass with --once.
    Sync {
        #[arg(long)]
        once: bool,
    },
}
#[derive(Serialize, Deserialize)]
struct Config {
    endpoint: String,
    key: String,
    device: String,
    since: String,
    enabled: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct State {
    offsets: BTreeMap<String, u64>,
    metadata: BTreeMap<String, String>,
    last_success: Option<String>,
}
fn file(name: &str) -> Result<PathBuf> {
    Ok(crate::paths::root()?.join(name))
}
fn save<T: Serialize>(path: &Path, data: &T) -> Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension(format!("{}.tmp", ulid::Ulid::new()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(data)?)?;
    f.sync_all()?;
    std::fs::rename(tmp, path)?;
    Ok(())
}
fn config() -> Result<Config> {
    Ok(serde_json::from_slice(
        &std::fs::read(file("cloud.json")?).context("Run claudetap cloud configure first")?,
    )?)
}
fn state() -> Result<State> {
    let p = file("cloud-state.json")?;
    if p.exists() {
        Ok(serde_json::from_slice(&std::fs::read(p)?)?)
    } else {
        Ok(State::default())
    }
}
fn digest(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}
fn sensitive(k: &str) -> bool {
    matches!(
        k.to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "x-api-key"
            | "api_key"
            | "apikey"
            | "access_token"
            | "refresh_token"
            | "cookie"
            | "set-cookie"
    )
}
fn sanitize(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                if sensitive(k) {
                    *v = Value::String("<redacted>".into());
                } else {
                    sanitize(v);
                }
            }
        }
        Value::Array(a) => {
            if a.len() == 2 && a[0].as_str().is_some_and(sensitive) {
                a[1] = Value::String("<redacted>".into());
            } else {
                for v in a {
                    sanitize(v);
                }
            }
        }
        _ => {}
    }
}
pub async fn run(cmd: Command) -> Result<()> {
    match cmd {
        Command::Configure { endpoint, backfill } => {
            let u = reqwest::Url::parse(&endpoint)?;
            if u.scheme() != "https"
                && !(u.scheme() == "http"
                    && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
            {
                bail!("Remote endpoints require HTTPS");
            }
            if !u.username().is_empty()
                || u.password().is_some()
                || u.query().is_some()
                || u.fragment().is_some()
                || u.path() != "/"
            {
                bail!("Use an origin URL without credentials, path, query or fragment");
            }
            let key = std::env::var("CLAUDETAP_UPLOAD_KEY")
                .context("Set CLAUDETAP_UPLOAD_KEY to your organization upload key")?;
            if key.len() < 32 {
                bail!("Upload key must contain at least 32 characters");
            }
            let old = if file("cloud.json")?.exists() {
                Some(config()?)
            } else {
                None
            };
            if old
                .as_ref()
                .is_some_and(|c| c.endpoint != endpoint.trim_end_matches('/'))
            {
                bail!("Changing servers requires a separate CLAUDETAP_HOME to keep checkpoints isolated");
            }
            let c = Config {
                endpoint: endpoint.trim_end_matches('/').into(),
                key,
                device: old
                    .as_ref()
                    .map(|c| c.device.clone())
                    .unwrap_or_else(|| ulid::Ulid::new().to_string()),
                since: if backfill {
                    String::new()
                } else {
                    old.map(|c| c.since)
                        .unwrap_or_else(|| ulid::Ulid::new().to_string())
                },
                enabled: true,
            };
            save(&file("cloud.json")?, &c)?;
            println!("Cloud enabled. Captured prompts, bodies and machine metadata will be uploaded. Structured credentials are filtered; arbitrary content may contain secrets. Run claudetap cloud sync.");
        }
        Command::Disable => {
            let mut c = config()?;
            c.enabled = false;
            save(&file("cloud.json")?, &c)?;
            println!("Cloud disabled; local capture continues.");
        }
        Command::Status => {
            let c = config()?;
            let s = state()?;
            println!(
                "{}\nEnabled: {}\nDevice: {}\nTracked files: {}\nLast successful pass: {}",
                c.endpoint,
                c.enabled,
                c.device,
                s.offsets.len() + s.metadata.len(),
                s.last_success.as_deref().unwrap_or("never")
            );
        }
        Command::Sync { once } => {
            // Advisory lock automatically releases after crashes.
            let lock_path = file("cloud-sync.lock")?;
            std::fs::create_dir_all(lock_path.parent().unwrap())?;
            let lock = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(lock_path)?;
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                    bail!("Another uploader is running");
                }
            }
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(30))
                .build()?;
            let mut delay = 2;
            loop {
                let c = config()?;
                if !c.enabled {
                    println!("Cloud disabled");
                    break;
                }
                let result = sync(&client, &c).await;
                if once {
                    return result;
                }
                match result {
                    Ok(()) => delay = 2,
                    Err(e) => {
                        eprintln!("Cloud sync paused: {e:#}. Retrying.");
                        delay = (delay * 2).min(60);
                    }
                }
                tokio::select! { _=tokio::signal::ctrl_c()=>break, _=tokio::time::sleep(Duration::from_secs(delay))=>{} }
            }
        }
    }
    Ok(())
}
async fn sync(client: &reqwest::Client, c: &Config) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut s = state()?;
    let root = crate::paths::sessions_dir()?;
    if !root.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let session = entry.file_name().to_string_lossy().to_string();
        if session.parse::<ulid::Ulid>().is_err() || session < c.since {
            continue;
        }
        let mut files = vec![
            entry.path().join("meta.json"),
            entry.path().join("traffic.jsonl"),
        ];
        for folder in ["bodies", "stream"] {
            let p = entry.path().join(folder);
            if p.symlink_metadata()
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
            {
                for f in std::fs::read_dir(p)? {
                    files.push(f?.path());
                }
            }
        }
        for p in files {
            if !p
                .symlink_metadata()
                .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            {
                continue;
            }
            let rel = p.strip_prefix(entry.path())?.to_string_lossy().to_string();
            if !(rel == "meta.json" || rel.ends_with(".jsonl") || rel.ends_with(".bin")) {
                continue;
            }
            let id = format!("{session}/{rel}");
            let meta = rel == "meta.json";
            let offset = if meta {
                0
            } else {
                *s.offsets.get(&id).unwrap_or(&0)
            };
            let mut f = std::fs::File::open(&p)?;
            f.seek(SeekFrom::Start(offset))?;
            let mut bytes = Vec::new();
            f.take(4 * 1024 * 1024).read_to_end(&mut bytes)?;
            if bytes.is_empty() {
                continue;
            }
            if rel.ends_with(".jsonl") {
                if let Some(n) = bytes.iter().rposition(|b| *b == b'\n') {
                    bytes.truncate(n + 1)
                } else {
                    if bytes.len() == 4 * 1024 * 1024 {
                        bail!("Log line exceeds 4 MiB in {id}");
                    }
                    continue;
                }
            }
            let consumed = bytes.len() as u64;
            if meta {
                let Ok(mut v) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                sanitize(&mut v);
                bytes = serde_json::to_vec(&v)?;
            } else if rel.ends_with(".jsonl") {
                let mut out = Vec::new();
                for line in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
                    let mut v: Value = serde_json::from_slice(line)
                        .with_context(|| format!("Invalid JSON in {id}"))?;
                    sanitize(&mut v);
                    serde_json::to_writer(&mut out, &v)?;
                    out.push(b'\n');
                }
                bytes = out;
            }
            let checksum = digest(&bytes);
            if meta && s.metadata.get(&id) == Some(&checksum) {
                continue;
            }
            let response = client
                .post(format!("{}/v1/chunks", c.endpoint))
                .bearer_auth(&c.key)
                .header("X-Device", &c.device)
                .header("X-Session", &session)
                .header("X-Path", &rel)
                .header("X-Offset", offset)
                .header("X-Source-Length", consumed)
                .header("X-SHA256", &checksum)
                .header("X-Hostname", crate::hostname().unwrap_or_default())
                .body(bytes)
                .send()
                .await
                .context("Upload connection failed")?;
            if !response.status().is_success() {
                bail!("Server returned {} for {id}", response.status())
            }
            if meta {
                s.metadata.insert(id, checksum);
            } else {
                s.offsets.insert(id, offset + consumed);
            }
            save(&file("cloud-state.json")?, &s)?;
        }
    }
    s.last_success = Some(time::OffsetDateTime::now_utc().to_string());
    save(&file("cloud-state.json")?, &s)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_nested_credentials() {
        let mut v = serde_json::json!({"headers":[["Authorization","secret"],["Accept","ok"]],"nested":{"api_key":"secret"}});
        sanitize(&mut v);
        assert_eq!(v["headers"][0][1], "<redacted>");
        assert_eq!(v["headers"][1][1], "ok");
        assert!(!v.to_string().contains("secret"));
    }
}
