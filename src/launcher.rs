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

/// Resolve the Windsurf binary path: explicit override, then `which windsurf`,
/// then platform default install locations.
pub fn resolve_windsurf(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        if p.exists() {
            return Ok(p.to_path_buf());
        }
        return Err(anyhow!("--windsurf-bin {} does not exist", p.display()));
    }

    // On macOS, prefer the real Electron binary inside Windsurf.app over
    // whatever `which windsurf` finds — the latter is typically the
    // ~/.codeium/windsurf/bin/windsurf CLI shim, which uses `open` to launch
    // the GUI as a detached process. That detaches the child from claudetap,
    // so the supervisor sees an immediate exit(0) and tears down the proxy
    // before any real traffic flows.
    #[cfg(target_os = "macos")]
    {
        for c in [
            "/Applications/Windsurf.app/Contents/MacOS/Electron",
            "/Applications/Windsurf.app/Contents/MacOS/Windsurf",
        ] {
            let p = PathBuf::from(c);
            if p.is_file() {
                return Ok(p);
            }
        }
    }

    if let Some(p) = which("windsurf") {
        return Ok(p);
    }

    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/Applications/Windsurf.app/Contents/MacOS/Electron",
            "/Applications/Windsurf.app/Contents/MacOS/Windsurf",
        ]
    } else if cfg!(target_os = "linux") {
        &[
            "/usr/share/windsurf/windsurf",
            "/opt/windsurf/windsurf",
            "/usr/bin/windsurf",
        ]
    } else {
        &[]
    };
    for c in candidates {
        let p = PathBuf::from(c);
        if p.is_file() {
            return Ok(p);
        }
    }

    Err(anyhow!(
        "could not locate Windsurf; pass --windsurf-bin /path/to/windsurf"
    ))
}

/// Best-effort: check whether Windsurf is already running. If it is, a second
/// launch will silently attach to the existing process and our env is dropped.
pub fn is_windsurf_running() -> bool {
    !running_windsurf_pids().is_empty()
}

/// Return PIDs of every running Windsurf-related process (main + helpers).
/// Empty if none. Best-effort, cross-platform.
pub fn running_windsurf_pids() -> Vec<u32> {
    use std::process::Command;
    let mut pids: Vec<u32> = Vec::new();
    let push_pgrep = |args: &[&str], out: &mut Vec<u32>| {
        let bin = if cfg!(target_os = "macos") {
            "/usr/bin/pgrep"
        } else {
            "pgrep"
        };
        if let Ok(o) = Command::new(bin).args(args).output() {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    if let Ok(p) = line.trim().parse::<u32>() {
                        if !out.contains(&p) {
                            out.push(p);
                        }
                    }
                }
            }
        }
    };

    #[cfg(target_os = "macos")]
    {
        // Main app binary, Electron renderer, and helper processes all live
        // under Windsurf.app. `-f` matches against the full argv path so we
        // catch every Helper (Renderer/GPU/Plugin) too.
        push_pgrep(&["-f", "Windsurf.app/Contents/"], &mut pids);
        push_pgrep(&["-i", "-f", "Windsurf Helper"], &mut pids);
        push_pgrep(&["-x", "Windsurf"], &mut pids);
    }
    #[cfg(target_os = "linux")]
    {
        push_pgrep(&["-x", "windsurf"], &mut pids);
        push_pgrep(&["-f", "windsurf"], &mut pids);
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = push_pgrep;
    }

    // Never list ourselves — pgrep -f might match the claudetap process if
    // "windsurf" appears in our own argv (it does: `claudetap windsurf`).
    let me = std::process::id();
    pids.retain(|&p| p != me);
    pids
}

/// Terminate any running Windsurf processes. Sends SIGTERM first, waits up to
/// `grace`, then SIGKILLs anything still alive. Returns the number of PIDs we
/// signalled. Best-effort — Windsurf may also be relaunched by `launchd` or a
/// user; we only do one pass.
pub async fn kill_running_windsurf(grace: std::time::Duration) -> usize {
    let pids = running_windsurf_pids();
    if pids.is_empty() {
        return 0;
    }

    #[cfg(unix)]
    {
        // If claudetap was launched from Windsurf's own integrated terminal,
        // killing Windsurf will close our controlling tty and deliver SIGHUP
        // to us. Ignore it so we survive long enough to relaunch Windsurf.
        // SIGPIPE for the same reason — writes to the dead stdout would
        // otherwise terminate us.
        unsafe {
            libc::signal(libc::SIGHUP, libc::SIG_IGN);
            libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        }
        for &pid in &pids {
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }

    // Poll for exit; bail out early once everything is gone.
    let start = std::time::Instant::now();
    let poll = std::time::Duration::from_millis(150);
    while start.elapsed() < grace {
        if running_windsurf_pids().is_empty() {
            return pids.len();
        }
        tokio::time::sleep(poll).await;
    }

    // Anything still alive gets SIGKILLed.
    #[cfg(unix)]
    {
        for pid in running_windsurf_pids() {
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }

    // Give the OS a moment to reap.
    let kill_deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(1500);
    while std::time::Instant::now() < kill_deadline {
        if running_windsurf_pids().is_empty() {
            break;
        }
        tokio::time::sleep(poll).await;
    }

    pids.len()
}

pub async fn spawn_windsurf(spec: LaunchSpec, user_data_dir: Option<&Path>) -> Result<Child> {
    let ca_cert = paths::ca_cert_path()?;
    let ca_cert_str = ca_cert
        .to_str()
        .ok_or_else(|| anyhow!("CA cert path is not utf-8: {}", ca_cert.display()))?
        .to_string();

    let mut cmd = Command::new(&spec.claude_path);

    // Chromium switches go BEFORE user-supplied args.
    cmd.arg(format!("--proxy-server={}", spec.proxy_url));
    cmd.arg("--proxy-bypass-list=<-loopback>;localhost;127.0.0.1;::1");
    if let Some(d) = user_data_dir {
        cmd.arg(format!("--user-data-dir={}", d.display()));
    }
    cmd.args(&spec.args);

    cmd.env("HTTPS_PROXY", &spec.proxy_url);
    cmd.env("https_proxy", &spec.proxy_url);
    cmd.env("HTTP_PROXY", &spec.proxy_url);
    cmd.env("http_proxy", &spec.proxy_url);
    cmd.env("ALL_PROXY", &spec.proxy_url);
    cmd.env("all_proxy", &spec.proxy_url);
    cmd.env("NO_PROXY", "localhost,127.0.0.1,::1");
    cmd.env("no_proxy", "localhost,127.0.0.1,::1");

    cmd.env("NODE_EXTRA_CA_CERTS", &ca_cert_str);
    cmd.env("SSL_CERT_FILE", &ca_cert_str);
    cmd.env("REQUESTS_CA_BUNDLE", &ca_cert_str);
    cmd.env("GRPC_DEFAULT_SSL_ROOTS_FILE_PATH", &ca_cert_str);

    cmd.env("CLAUDETAP_SESSION", &spec.session_id);

    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    // New process group so we can SIGKILL the whole Electron tree (zygote +
    // GPU + utility + renderers) on force-quit, not just the parent PID.
    #[cfg(unix)]
    {
        #[allow(unused_imports)]
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    cmd.spawn()
        .with_context(|| format!("spawning {}", spec.claude_path.display()))
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
