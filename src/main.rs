//! claudetap entry point: CLI parsing + orchestration.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use time::OffsetDateTime;
use tracing::warn;
use tracing_subscriber::EnvFilter;
use ulid::Ulid;

mod banner;
mod ca;
mod launcher;
mod log;
mod paths;
mod proxy;
mod sse;

const DEFAULT_HOSTS: &[&str] = &[
    "api.anthropic.com",
    "*.anthropic.com",
    "statsig.anthropic.com",
    "console.anthropic.com",
];

#[derive(Parser, Debug)]
#[command(
    name = "claudetap",
    version,
    about = "Tap Claude Code's HTTPS traffic into ~/.claudetap (logs are kept forever).",
    disable_help_subcommand = true
)]
struct Cli {
    /// Subcommand. With no subcommand we launch `claude` under the proxy.
    #[command(subcommand)]
    command: Option<Cmd>,

    /// Path to the `claude` binary. Defaults to `which claude`.
    #[arg(long, value_name = "PATH", global = true)]
    claude_bin: Option<PathBuf>,

    /// Local proxy port. 0 = pick a free one (default).
    #[arg(long, default_value_t = 0u16, global = true)]
    port: u16,

    /// Hostnames or `*.suffix` patterns to MITM. Repeatable, comma-separated.
    #[arg(long, value_delimiter = ',', global = true)]
    hosts: Vec<String>,

    /// Refuse non-tapped hosts entirely instead of blind-tunneling them.
    #[arg(long, default_value_t = false, global = true)]
    passthrough_only_logged: bool,

    /// Disable header redaction (default: redact auth/cookie/api-key).
    #[arg(long, default_value_t = false, global = true)]
    no_redact: bool,

    /// Forwarded to `claude` after `--`.
    #[arg(last = true)]
    claude_args: Vec<OsString>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// CA management.
    Ca {
        #[command(subcommand)]
        sub: CaCmd,
    },
    /// Print the resolved `~/.claudetap` root and exit.
    Where,
    /// Run only the proxy. Doesn't launch claude. Stays up until Ctrl-C.
    ///
    /// Prints the env vars you need to wire any other client (curl, your
    /// own claude invocation, etc.) up to it.
    Proxy,
}

#[derive(Subcommand, Debug)]
enum CaCmd {
    /// Print the path to the CA certificate.
    Path,
    /// Print the CA certificate to stdout.
    Print,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing();
    install_default_crypto_provider()?;

    match cli.command {
        Some(Cmd::Where) => {
            println!("{}", paths::root()?.display());
            Ok(())
        }
        Some(Cmd::Ca { sub: CaCmd::Path }) => {
            let _ = ca::Ca::load_or_generate()?;
            println!("{}", paths::ca_cert_path()?.display());
            Ok(())
        }
        Some(Cmd::Ca { sub: CaCmd::Print }) => {
            let ca = ca::Ca::load_or_generate()?;
            print!("{}", ca.cert_pem);
            Ok(())
        }
        Some(Cmd::Proxy) => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("building tokio runtime")?;
            rt.block_on(run_proxy_only(cli))
        }
        None => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("building tokio runtime")?;
            rt.block_on(run_proxy_and_launch(cli))
        }
    }
}

fn init_tracing() {
    // Default to warnings only so the banner is the primary signal at
    // startup. Bump with `CLAUDETAP_LOG=claudetap=info` (or =debug) when you
    // want per-connection traces.
    let filter = EnvFilter::try_from_env("CLAUDETAP_LOG")
        .unwrap_or_else(|_| EnvFilter::new("claudetap=warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .with_writer(std::io::stderr)
        .try_init();
}

fn install_default_crypto_provider() -> Result<()> {
    // rustls 0.23 requires a process-wide default crypto provider before any
    // ServerConfig / ClientConfig can be built without an explicit provider.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .map_err(|_| anyhow::anyhow!("failed to install rustls ring crypto provider"))?;
    }
    Ok(())
}

async fn run_proxy_and_launch(cli: Cli) -> Result<()> {
    paths::ensure_dir(&paths::root()?)?;
    paths::ensure_dir(&paths::sessions_dir()?)?;

    let ca = Arc::new(ca::Ca::load_or_generate()?);

    let session_id = Ulid::new().to_string();
    let logger = log::SessionLogger::create(session_id.clone(), !cli.no_redact).await?;

    let hosts = if cli.hosts.is_empty() {
        DEFAULT_HOSTS.iter().map(|s| s.to_string()).collect()
    } else {
        cli.hosts.clone()
    };
    let host_filter = proxy::HostFilter::new(hosts.clone());

    let listener = proxy::bind_listener(cli.port).await?;
    let local = listener.local_addr()?;
    let proxy_url = format!("http://{}", local);

    let state =
        proxy::build_state(ca.clone(), logger.clone(), host_filter, cli.passthrough_only_logged)
            .await?;

    let claude_path = launcher::resolve_claude(cli.claude_bin.as_deref())?;
    let spec = launcher::LaunchSpec {
        claude_path: claude_path.clone(),
        args: cli.claude_args.clone(),
        proxy_url: proxy_url.clone(),
        session_id: session_id.clone(),
    };

    // Start the proxy task.
    let proxy_state = state.clone();
    let proxy_task = tokio::spawn(async move {
        if let Err(e) = proxy::run(listener, proxy_state).await {
            warn!(error = %e, "proxy loop exited");
        }
    });

    // Print the splash banner BEFORE spawning claude, so it lands at the top
    // of scrollback even if claude paints over the visible area.
    let session_dir = paths::session_dir(&session_id)?;
    let ca_path = paths::ca_cert_path()?;
    let banner_text = banner::Banner::new("claudetap · launching")
        .row("session", session_id.clone())
        .row("proxy", proxy_url.clone())
        .row("logs", banner::abbrev_path(&session_dir))
        .row("hosts", hosts.join(", "))
        .row("claude", banner::abbrev_path(&claude_path))
        .render();
    print!("{}", banner_text);
    use std::io::Write;
    let _ = std::io::stdout().flush();

    write_last_session_pointer(&session_id, &session_dir, &proxy_url)?;

    // Spawn claude.
    let mut child = launcher::spawn(spec).await?;
    let pid = child.id();

    // Now that we know the child PID, write session metadata.
    let meta = log::SessionMeta {
        session_id: session_id.clone(),
        started_at: OffsetDateTime::now_utc(),
        ended_at: None,
        claude_argv: cli
            .claude_args
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect(),
        claude_path: claude_path.display().to_string(),
        claude_pid: pid,
        claude_exit_code: None,
        claude_session_id: None,
        claude_version: detect_claude_version(&claude_path),
        proxy_addr: local.to_string(),
        redact: !cli.no_redact,
        host_filter: hosts,
        claudetap_version: env!("CARGO_PKG_VERSION").to_string(),
        session_dir: Some(session_dir.display().to_string()),
        ca_cert_path: Some(ca_path.display().to_string()),
        cwd: std::env::current_dir().ok().map(|p| p.display().to_string()),
        user: std::env::var("USER").ok().or_else(|| std::env::var("USERNAME").ok()),
        hostname: hostname(),
        os: Some(std::env::consts::OS.to_string()),
        arch: Some(std::env::consts::ARCH.to_string()),
        shell: std::env::var("SHELL").ok(),
        term: std::env::var("TERM").ok(),
        system: Some(collect_system_info()),
    };
    logger.write_meta(&meta).await?;

    // Supervise the child with an escalating signal handler so Ctrl-C
    // ALWAYS gets us out, even when claude's TUI is hung waiting on a
    // dead network connection and ignoring SIGINT.
    let code = supervise_child(&mut child, pid).await;
    if let Err(e) = logger
        .update_meta(|m| {
            m.ended_at = Some(OffsetDateTime::now_utc());
            m.claude_exit_code = code;
        })
        .await
    {
        warn!(error = %e, "updating meta.json on shutdown");
    }
    eprintln!(
        "\nclaudetap: claude exited (code={}) — session {}\n           logs at {}",
        code.map(|c| c.to_string()).unwrap_or_else(|| "signal".to_string()),
        session_id,
        session_dir.display()
    );

    // Give the proxy a moment to finalize any in-flight SSE finalization
    // tasks, then drop it. We deliberately do NOT call any "flush all logs"
    // routine here — logs are append-only and persisted as they are written.
    proxy_task.abort();
    let _ = proxy_task.await;

    std::process::exit(code.unwrap_or(0));
}

/// `claudetap proxy`: stand the proxy up and stay alive until Ctrl-C.
///
/// We deliberately don't fork a child here so the proxy lives as long as
/// the user wants. Useful for manual testing with curl, or for users who
/// want to source the env vars and start `claude` themselves in another
/// terminal.
async fn run_proxy_only(cli: Cli) -> Result<()> {
    paths::ensure_dir(&paths::root()?)?;
    paths::ensure_dir(&paths::sessions_dir()?)?;

    let ca = Arc::new(ca::Ca::load_or_generate()?);

    let session_id = Ulid::new().to_string();
    let logger = log::SessionLogger::create(session_id.clone(), !cli.no_redact).await?;

    let hosts = if cli.hosts.is_empty() {
        DEFAULT_HOSTS.iter().map(|s| s.to_string()).collect()
    } else {
        cli.hosts.clone()
    };
    let host_filter = proxy::HostFilter::new(hosts.clone());

    let listener = proxy::bind_listener(cli.port).await?;
    let local = listener.local_addr()?;
    let proxy_url = format!("http://{}", local);

    let state = proxy::build_state(
        ca.clone(),
        logger.clone(),
        host_filter,
        cli.passthrough_only_logged,
    )
    .await?;

    let ca_path = paths::ca_cert_path()?;

    let session_dir = paths::session_dir(&session_id)?;

    // Write session meta now (no claude child to wait on).
    let meta = log::SessionMeta {
        session_id: session_id.clone(),
        started_at: OffsetDateTime::now_utc(),
        ended_at: None,
        claude_argv: Vec::new(),
        claude_path: String::new(),
        claude_pid: None,
        claude_exit_code: None,
        claude_session_id: None,
        claude_version: None,
        proxy_addr: local.to_string(),
        redact: !cli.no_redact,
        host_filter: hosts.clone(),
        claudetap_version: env!("CARGO_PKG_VERSION").to_string(),
        session_dir: Some(session_dir.display().to_string()),
        ca_cert_path: Some(ca_path.display().to_string()),
        cwd: std::env::current_dir().ok().map(|p| p.display().to_string()),
        user: std::env::var("USER").ok().or_else(|| std::env::var("USERNAME").ok()),
        hostname: hostname(),
        os: Some(std::env::consts::OS.to_string()),
        arch: Some(std::env::consts::ARCH.to_string()),
        shell: std::env::var("SHELL").ok(),
        term: std::env::var("TERM").ok(),
        system: Some(collect_system_info()),
    };
    logger.write_meta(&meta).await?;
    let banner_text = banner::Banner::new("claudetap · proxy (no child)")
        .row("session", session_id.clone())
        .row("proxy", proxy_url.clone())
        .row("logs", banner::abbrev_path(&session_dir))
        .row("traffic", banner::abbrev_path(&session_dir.join("traffic.jsonl")))
        .row("ca", banner::abbrev_path(&ca_path))
        .row("hosts", hosts.join(", "))
        .render();
    print!("{}", banner_text);

    // Copy-pasteable env block under the box.
    println!("# Source these to send another shell's traffic through claudetap:");
    println!("export HTTPS_PROXY={proxy_url}");
    println!("export HTTP_PROXY={proxy_url}");
    println!("export ALL_PROXY={proxy_url}");
    println!("export NO_PROXY=localhost,127.0.0.1,::1");
    println!("export NODE_EXTRA_CA_CERTS={}", ca_path.display());
    println!("export SSL_CERT_FILE={}", ca_path.display());
    println!("export BUN_CA_BUNDLE={}", ca_path.display());
    println!("# Quick smoke test:");
    println!(
        "# curl --proxy {proxy_url} --cacert {} https://api.anthropic.com/",
        ca_path.display()
    );
    println!("# Ctrl-C to stop.");
    use std::io::Write;
    let _ = std::io::stdout().flush();

    write_last_session_pointer(&session_id, &session_dir, &proxy_url)?;

    // Run the proxy loop until interrupted.
    let proxy_state = state.clone();
    let proxy_task = tokio::spawn(async move {
        if let Err(e) = proxy::run(listener, proxy_state).await {
            warn!(error = %e, "proxy loop exited");
        }
    });

    wait_for_shutdown().await;

    if let Err(e) = logger
        .update_meta(|m| m.ended_at = Some(OffsetDateTime::now_utc()))
        .await
    {
        warn!(error = %e, "updating meta.json on shutdown");
    }
    eprintln!(
        "\nclaudetap: proxy stopped — session {}\n           logs at {}",
        session_id,
        session_dir.display()
    );
    proxy_task.abort();
    let _ = proxy_task.await;
    Ok(())
}

/// Gather static, non-privileged system facts: CPU counts, RAM, model name,
/// kernel/OS version. Anything we can't read becomes `None`.
fn collect_system_info() -> log::SystemInfo {
    let cpu_logical = std::thread::available_parallelism()
        .ok()
        .map(|n| n.get() as u64);

    let mut info = log::SystemInfo {
        cpu_logical,
        cpu_physical: None,
        cpu_brand: None,
        machine_model: None,
        total_memory_bytes: None,
        page_size_bytes: None,
        kernel_version: None,
        os_release: None,
    };

    #[cfg(target_os = "macos")]
    {
        info.total_memory_bytes = sysctl_str("hw.memsize").and_then(|s| s.parse().ok());
        info.cpu_physical = sysctl_str("hw.physicalcpu").and_then(|s| s.parse().ok());
        if info.cpu_logical.is_none() {
            info.cpu_logical = sysctl_str("hw.logicalcpu").and_then(|s| s.parse().ok());
        }
        info.cpu_brand = sysctl_str("machdep.cpu.brand_string");
        info.machine_model = sysctl_str("hw.model");
        info.page_size_bytes = sysctl_str("hw.pagesize").and_then(|s| s.parse().ok());
        info.kernel_version = sysctl_str("kern.osrelease");
        info.os_release = sysctl_str("kern.osproductversion")
            .or_else(|| sysctl_str("kern.osversion"));
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    let kb: Option<u64> = rest
                        .trim()
                        .split_whitespace()
                        .next()
                        .and_then(|n| n.parse().ok());
                    info.total_memory_bytes = kb.map(|k| k * 1024);
                    break;
                }
            }
        }
        if let Ok(s) = std::fs::read_to_string("/proc/cpuinfo") {
            let mut physical_ids = std::collections::HashSet::new();
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("model name") {
                    if info.cpu_brand.is_none() {
                        if let Some((_, v)) = rest.split_once(':') {
                            info.cpu_brand = Some(v.trim().to_string());
                        }
                    }
                } else if let Some(rest) = line.strip_prefix("physical id") {
                    if let Some((_, v)) = rest.split_once(':') {
                        physical_ids.insert(v.trim().to_string());
                    }
                }
            }
            if !physical_ids.is_empty() {
                info.cpu_physical = Some(physical_ids.len() as u64);
            }
        }
        info.kernel_version = uname_r();
        if let Ok(s) = std::fs::read_to_string("/etc/os-release") {
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                    info.os_release = Some(rest.trim_matches('"').to_string());
                    break;
                }
            }
        }
        info.page_size_bytes = page_size_bytes();
    }

    info
}

#[cfg(target_os = "macos")]
fn sysctl_str(key: &str) -> Option<String> {
    let out = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", key])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(target_os = "linux")]
fn uname_r() -> Option<String> {
    let out = std::process::Command::new("uname").arg("-r").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(target_os = "linux")]
fn page_size_bytes() -> Option<u64> {
    let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if n > 0 { Some(n as u64) } else { None }
}

/// Best-effort hostname lookup. Tries libc on unix, falls back to env vars.
fn hostname() -> Option<String> {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if rc == 0 {
            let nul = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            if let Ok(s) = std::str::from_utf8(&buf[..nul]) {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
}

/// Best-effort: run `<claude_path> --version` with a short timeout and capture
/// stdout. Returns `None` on any failure (so we never block startup on it).
fn detect_claude_version(claude_path: &std::path::Path) -> Option<String> {
    use std::process::{Command, Stdio};
    use std::time::Duration;
    let mut child = Command::new(claude_path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Poll for up to ~1.5s.
    let deadline = std::time::Instant::now() + Duration::from_millis(1500);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// Drop a small text file at `~/.claudetap/last-session` so users can find
/// the most recent session without scrolling. Append-only? No — this is a
/// pointer, intentionally overwritten each launch. The session data itself
/// (traffic.jsonl, bodies, stream) remains on disk forever.
fn write_last_session_pointer(
    session_id: &str,
    session_dir: &std::path::Path,
    proxy_url: &str,
) -> Result<()> {
    let path = paths::root()?.join("last-session");
    let content = format!(
        "{session_id}\n{}\n{proxy_url}\n",
        session_dir.display()
    );
    std::fs::write(&path, content)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(_) => return,
    };
    tokio::select! {
        _ = sigint.recv() => {},
        _ = sigterm.recv() => {},
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

/// Wait for the claude child to exit, but with an escalating signal
/// supervisor so the user can always escape:
///
///   1st Ctrl-C: terminal already delivered SIGINT to the foreground
///               process group; we just print a hint and keep waiting.
///   2nd Ctrl-C: send SIGTERM to claude.
///   3rd Ctrl-C: SIGKILL claude and force-exit claudetap (in case the
///               child can't be reaped, e.g. it's stuck in uninterruptible
///               I/O against a broken FUSE/network mount).
///
/// Returns the child's exit code if we collected it cleanly, or 130
/// ("killed by SIGINT") if we had to force-quit.
#[cfg(unix)]
async fn supervise_child(child: &mut tokio::process::Child, pid: Option<u32>) -> Option<i32> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigint = signal(SignalKind::interrupt()).ok();
    let mut sigterm = signal(SignalKind::terminate()).ok();
    let mut sigquit = signal(SignalKind::quit()).ok();
    let mut count: u32 = 0;

    loop {
        tokio::select! {
            wait_res = child.wait() => {
                return match wait_res {
                    Ok(status) => status.code(),
                    Err(e) => {
                        warn!(error = %e, "waiting on claude child");
                        None
                    }
                };
            }
            Some(_) = async {
                match sigint.as_mut() { Some(s) => s.recv().await, None => None }
            } => { count += 1; on_signal(count, pid).await; if count >= 3 { return Some(130); } }
            Some(_) = async {
                match sigterm.as_mut() { Some(s) => s.recv().await, None => None }
            } => { count += 1; on_signal(count, pid).await; if count >= 3 { return Some(143); } }
            Some(_) = async {
                match sigquit.as_mut() { Some(s) => s.recv().await, None => None }
            } => {
                // Ctrl-\: skip straight to SIGKILL+exit; this is the
                // documented "get me out NOW" path.
                eprintln!("\nclaudetap: SIGQUIT — killing claude and exiting.");
                if let Some(p) = pid {
                    unsafe { libc::kill(p as i32, libc::SIGKILL); }
                }
                return Some(131);
            }
        }
    }
}

#[cfg(unix)]
async fn on_signal(count: u32, pid: Option<u32>) {
    match count {
        1 => {
            // The shell already delivered SIGINT to the whole foreground
            // process group, so claude has it. We just inform the user
            // about the escape hatch.
            eprintln!(
                "\nclaudetap: caught Ctrl-C. Press again to SIGTERM claude, a third time to force-quit."
            );
        }
        2 => {
            eprintln!("\nclaudetap: sending SIGTERM to claude. One more Ctrl-C will SIGKILL.");
            if let Some(p) = pid {
                unsafe { libc::kill(p as i32, libc::SIGTERM); }
            }
        }
        _ => {
            eprintln!("\nclaudetap: SIGKILLing claude and force-exiting.");
            if let Some(p) = pid {
                unsafe { libc::kill(p as i32, libc::SIGKILL); }
            }
            // Give the kernel a moment to deliver, then bail. We bypass
            // the normal cleanup because by definition we got here only
            // because the normal path was stuck.
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
}

#[cfg(not(unix))]
async fn supervise_child(child: &mut tokio::process::Child, _pid: Option<u32>) -> Option<i32> {
    // Windows: rely on tokio's ctrl_c handler to kill the child.
    let mut count: u32 = 0;
    loop {
        tokio::select! {
            wait_res = child.wait() => {
                return match wait_res { Ok(s) => s.code(), Err(_) => None };
            }
            _ = tokio::signal::ctrl_c() => {
                count += 1;
                if count == 1 {
                    eprintln!("\nclaudetap: caught Ctrl-C. Press again to kill claude.");
                } else {
                    eprintln!("\nclaudetap: killing claude.");
                    let _ = child.start_kill();
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(1), child.wait()
                    ).await;
                    return Some(130);
                }
            }
        }
    }
}
