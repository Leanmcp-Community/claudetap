# Claudetap

Local HTTPS MITM proxy that taps Claude Code traffic into `~/.claudetap`.

## Installation

Claudetap is written in Rust. Install from source today using Cargo on Linux or
macOS. This repository includes Linux binary release and npm publishing tools,
but those tools do not mean that an npm package or release has been published.

### From source (Linux and macOS)

Install a current stable Rust toolchain from [rustup](https://rustup.rs/). On
Ubuntu/Debian, install the native build prerequisites:

```bash
sudo apt-get update
sudo apt-get install -y build-essential cmake pkg-config ca-certificates
```

Then build and install:

```bash
git clone https://github.com/Leanmcp-Community/claudetap.git
cd claudetap
cargo install --path . --locked
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
claudetap --version
```

For an existing checkout, just run `cargo install --path . --locked` again.
`./install.sh` performs the same Cargo installation and also records an install
telemetry event. `--locked` uses the dependency versions in `Cargo.lock`.

Install Claude Code separately, then run `claudetap` to launch it through the
proxy. Linux CI is configured to build and test on x86_64 and ARM64.

### One-line Linux installer (requires publishing this script)

Once `scripts/install-linux.sh` is merged into the repository's `main` branch:

```bash
curl -fsSL https://raw.githubusercontent.com/Leanmcp-Community/claudetap/main/scripts/install-linux.sh | bash
```

This installs to `~/.local/bin` without sudo. It downloads the latest Linux x64
or ARM64 release and verifies its SHA-256 checksum. Prebuilt binaries target
glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+); they do not require Rust or Node.js.
If no binary has been published, it builds from source instead, installing Rust
if needed. Source builds require the native build prerequisites above.

To build from source explicitly, choose a release, or change the install folder:

```bash
bash scripts/install-linux.sh --source
bash scripts/install-linux.sh --version v0.2.0 --prefix "$HOME/.local/bin"
```

A version must exist as a Git tag or release. Re-run the installer to update;
remove `~/.local/bin/claudetap` to uninstall the default binary installation.

### Homebrew (macOS)

```bash
brew tap leanmcp-community/claudetap https://github.com/Leanmcp-Community/claudetap.git
brew install --HEAD leanmcp-community/claudetap/claudetap
```

This builds from source; Homebrew installs Rust as a build dependency.
See [Homebrew setup](docs/HOMEBREW.md) for cloud configuration and background services.

### npm, apt, Snap, and Windows

**No npm installation is advertised yet.** Maintainers can use the
[publishing guide](docs/PUBLISHING.md) to publish Linux x64 and ARM64 binaries
and an npm launcher. npm is a distribution channel: the application stays Rust,
and the small JavaScript launcher runs the packaged executable. npm installation
will require Node.js, while the standalone binary will not.

There is no configured apt repository or Snap package. A Windows `.exe` requires
a Windows build and platform validation; Linux binaries cannot be renamed to
`.exe`. Windows release packaging is not configured. See the publishing guide
for the release process and the distinction between npm packages and executables.

## Usage

Claudetap mints a local root CA, stands up an HTTPS MITM proxy, then launches a
target with its traffic routed through that proxy and logged to
`~/.claudetap/sessions/<id>/`.

```bash
# Tap the Claude Code CLI (default; env-var proxy)
claudetap                       # launches `claude` under the proxy
claudetap -- --help             # args after `--` are forwarded to claude

# Tap Windsurf (Electron GUI; Chromium --proxy-server flags)
claudetap windsurf

# Tap the Antigravity CLI (`agy`, Google's agent CLI)
claudetap antigravity           # or the alias: claudetap agy
claudetap agy --agy-bin /path/to/agy

# Tap the OpenAI Codex CLI
claudetap codex
claudetap codex -- --help

# Just run the proxy and wire up your own client
claudetap proxy
```

### Trusting the CA (required for Antigravity & Windsurf)

`agy` is a Go binary and Windsurf is Chromium-based; both validate TLS against
the **OS trust store**, not env-var CA bundles. Install the claudetap root CA
once:

```bash
claudetap ca trust       # add root CA to the login keychain (macOS)
claudetap ca untrust     # remove it again
```

The Claude Code launcher sets `NODE_EXTRA_CA_CERTS` for the child process.
On Linux, `claudetap ca trust` prints manual certificate setup instructions; it
does not automatically modify certificate stores.

### What gets tapped

Each target ships with a default host allow-list (everything else is
blind-tunneled, not decrypted). Override with `--hosts a.com,*.b.com`.

- **claude** → `api.anthropic.com`, `*.anthropic.com`
- **codex** → `api.openai.com`, `*.openai.com`, `chatgpt.com`
- **antigravity** → `cloudcode-pa.googleapis.com`, `aiplatform.googleapis.com`,
  `*.googleapis.com`, Google auth hosts, and `api.anthropic.com` (Antigravity
  serves Claude models through Google's Cloud Code backend).

> **Note:** the proxy decrypts HTTP/1.1 only. Google's Cloud Code REST/SSE
> endpoints work over HTTP/1.1, but any pure-gRPC (HTTP/2) calls are tunneled
> without decryption. Likewise, the Electron app `agy` can launch over CDP
> inherits the proxy env vars but Chromium ignores `HTTPS_PROXY` for its own
> network stack — the core LLM traffic from the `agy` process itself is what
> gets captured.

## Optional cloud uploads

See [cloud setup and deployment instructions](README_CLOUD.md) for running your
own server locally or remotely, configuring uploads, and installing the background service.
