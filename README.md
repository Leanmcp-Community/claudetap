# Claudetap

Local HTTPS MITM proxy that taps Claude Code traffic into `~/.claudetap`.

## Installation

There are several ways to install Claudetap depending on your preference:

### 1. Cargo (Recommended for Rust users)

If you already have Rust and Cargo installed, you can build and install Claudetap directly from source:

```bash
# Clone the repository
git clone https://github.com/ddod/claudetap.git
cd claudetap

# Run the install script (which runs `cargo install`)
./install.sh
```

Ensure that `~/.cargo/bin` is in your `$PATH`.

### 2. Homebrew (macOS)

```bash
brew tap leanmcp-community/claudetap https://github.com/Leanmcp-Community/claudetap.git
brew install --HEAD leanmcp-community/claudetap/claudetap
```

This builds the current source version; Homebrew installs Rust as a build dependency.
See [Homebrew setup](docs/HOMEBREW.md) for cloud configuration, background services,
and migration from a Cargo installation.

### 3. NPX / NPM (For JS/TS developers)

*Note: Since Claudetap is written in Rust, distributing it via `npm` requires publishing pre-compiled binaries for each architecture to the npm registry.*

If published to NPM, you can run Claudetap instantly without installation using:

```bash
npx claudetap [args...]
```

Alternatively, install it globally:

```bash
npm install -g claudetap
```

<details>
<summary><b>How to set up NPM distribution</b></summary>

To support `npx`, you have two main options:
1. **Binary download wrapper:** Create a simple `package.json` with a `postinstall` script (using tools like `binary-install` or a custom JS script) that fetches the compiled binary from GitHub Releases for the user's OS and CPU architecture.
2. **Platform-specific optional dependencies:** Use a GitHub Action to cross-compile the binary to platforms like `darwin-arm64`, `linux-x64`, etc. Publish each as its own npm package (`@claudetap/core-darwin-arm64`), and have a main `claudetap` npm package that depends on the right binary via `optionalDependencies`. (This is how tools like `esbuild` and `turbo` distribute their binaries).
</details>

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

The Claude Code CLI (Node) honors `NODE_EXTRA_CA_CERTS`, so it works without
OS trust.

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
