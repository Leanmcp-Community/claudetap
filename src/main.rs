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
        .row("traffic", banner::abbrev_path(&session_dir.join("traffic.jsonl")))
        .row("ca", banner::abbrev_path(&ca_path))
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
        claude_argv: cli
            .claude_args
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect(),
        claude_path: claude_path.display().to_string(),
        claude_pid: pid,
        proxy_addr: local.to_string(),
        redact: !cli.no_redact,
        host_filter: hosts,
        claudetap_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    logger.write_meta(&meta).await?;

    // Forward common termination signals to the child so Ctrl-C in the
    // terminal stops claude rather than orphaning it.
    install_signal_forwarder(pid);

    let status = child.wait().await.context("waiting on claude child")?;
    let code = status.code();
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

    // Write session meta now (no claude child to wait on).
    let meta = log::SessionMeta {
        session_id: session_id.clone(),
        started_at: OffsetDateTime::now_utc(),
        claude_argv: Vec::new(),
        claude_path: String::new(),
        claude_pid: None,
        proxy_addr: local.to_string(),
        redact: !cli.no_redact,
        host_filter: hosts.clone(),
        claudetap_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    logger.write_meta(&meta).await?;

    let session_dir = paths::session_dir(&session_id)?;
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

    eprintln!(
        "\nclaudetap: proxy stopped — session {}\n           logs at {}",
        session_id,
        session_dir.display()
    );
    proxy_task.abort();
    let _ = proxy_task.await;
    Ok(())
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

#[cfg(unix)]
fn install_signal_forwarder(pid: Option<u32>) {
    use tokio::signal::unix::{signal, SignalKind};
    let Some(pid) = pid else { return };
    tokio::spawn(async move {
        let mut sigint = match signal(SignalKind::interrupt()) {
            Ok(s) => s,
            Err(_) => return,
        };
        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(s) => s,
            Err(_) => return,
        };
        loop {
            let sig = tokio::select! {
                _ = sigint.recv() => libc::SIGINT,
                _ = sigterm.recv() => libc::SIGTERM,
            };
            unsafe {
                libc::kill(pid as i32, sig);
            }
        }
    });
}

#[cfg(not(unix))]
fn install_signal_forwarder(_pid: Option<u32>) {}
