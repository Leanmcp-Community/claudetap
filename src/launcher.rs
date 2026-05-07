//! Spawn `claude` (or any command) with the env vars that route its HTTPS
//! traffic through our local MITM proxy.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use tokio::process::{Child, Command};

use crate::paths;

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub claude_path: PathBuf,
    pub args: Vec<OsString>,
    pub proxy_url: String,
    pub session_id: String,
}

/// Resolve the `claude` binary path: explicit override, then `$PATH`, then
/// the `.exe` shim that npm's @anthropic-ai/claude-code installs on macOS.
pub fn resolve_claude(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        if p.exists() {
            return Ok(p.to_path_buf());
        }
        return Err(anyhow!("--claude-bin {} does not exist", p.display()));
    }

    if let Some(p) = which("claude") {
        return Ok(p);
    }
    if let Some(p) = which("claude.exe") {
        return Ok(p);
    }
    Err(anyhow!(
        "could not find `claude` on $PATH; pass --claude-bin /path/to/claude"
    ))
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub async fn spawn(spec: LaunchSpec) -> Result<Child> {
    let ca_cert = paths::ca_cert_path()?;
    let ca_cert_str = ca_cert
        .to_str()
        .ok_or_else(|| anyhow!("CA cert path is not utf-8: {}", ca_cert.display()))?
        .to_string();

    let mut cmd = Command::new(&spec.claude_path);
    cmd.args(&spec.args);

    // HTTPS / HTTP proxy routing.
    cmd.env("HTTPS_PROXY", &spec.proxy_url);
    cmd.env("https_proxy", &spec.proxy_url);
    cmd.env("HTTP_PROXY", &spec.proxy_url);
    cmd.env("http_proxy", &spec.proxy_url);
    cmd.env("ALL_PROXY", &spec.proxy_url);
    cmd.env("all_proxy", &spec.proxy_url);
    // Don't proxy ourselves recursively.
    cmd.env("NO_PROXY", "localhost,127.0.0.1,::1");
    cmd.env("no_proxy", "localhost,127.0.0.1,::1");

    // Trust our CA. Cover the three common runtimes Claude Code might use.
    cmd.env("NODE_EXTRA_CA_CERTS", &ca_cert_str);
    cmd.env("SSL_CERT_FILE", &ca_cert_str);
    cmd.env("BUN_CA_BUNDLE", &ca_cert_str);
    cmd.env("REQUESTS_CA_BUNDLE", &ca_cert_str);

    cmd.env("CLAUDETAP_SESSION", &spec.session_id);

    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    cmd.spawn()
        .with_context(|| format!("spawning {}", spec.claude_path.display()))
}
