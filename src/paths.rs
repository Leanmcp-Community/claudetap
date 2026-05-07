//! Filesystem layout for `~/.claudetap`.
//!
//! Logs are write-only and append-only. Nothing in this module ever deletes,
//! truncates, or rotates anything on disk. Sessions accumulate forever.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

/// Root directory: `$CLAUDETAP_HOME` or `~/.claudetap`.
pub fn root() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("CLAUDETAP_HOME") {
        return Ok(PathBuf::from(p));
    }
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not resolve home directory"))?;
    Ok(home.join(".claudetap"))
}

pub fn ca_dir() -> Result<PathBuf> {
    Ok(root()?.join("ca"))
}

pub fn ca_cert_path() -> Result<PathBuf> {
    Ok(ca_dir()?.join("root.crt"))
}

pub fn ca_key_path() -> Result<PathBuf> {
    Ok(ca_dir()?.join("root.key"))
}

pub fn sessions_dir() -> Result<PathBuf> {
    Ok(root()?.join("sessions"))
}

pub fn session_dir(session_id: &str) -> Result<PathBuf> {
    Ok(sessions_dir()?.join(session_id))
}

/// Create a directory tree, idempotently. Never deletes anything.
pub fn ensure_dir(p: &Path) -> Result<()> {
    std::fs::create_dir_all(p).with_context(|| format!("creating {}", p.display()))
}
