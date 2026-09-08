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

### 2. Homebrew (macOS / Linux)

*Note: To distribute via Homebrew without requiring users to compile from source, you will need to publish compiled binaries (e.g., via GitHub Releases) and create a Homebrew Tap (a repository named `homebrew-claudetap`).*

Once you have set up a Homebrew tap, users can install it like this:

```bash
brew tap ddod/claudetap
brew install claudetap
```

<details>
<summary><b>How to set up the Homebrew Formula</b></summary>

In your `homebrew-claudetap` repository, you would create a file called `claudetap.rb`:

```ruby
class Claudetap < Formula
  desc "Local HTTPS MITM proxy for Claude Code traffic"
  homepage "https://github.com/ddod/claudetap"
  
  # Option 1: Build from source
  url "https://github.com/ddod/claudetap/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "REPLACE_WITH_SHA256_OF_TARBALL"
  license "MIT"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    system "#{bin}/claudetap", "--version"
  end
end
```
</details>

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

Basic cloud sync is implemented and tested. You can run your own server on your
laptop or a remote machine. Local capture continues independently of cloud
availability. Browser login, a web dashboard, managed storage, and marketplace
publishing remain follow-on work.

### Run your own server locally

Run these commands from the repository root. You need Rust/Cargo, Python 3, and
Docker Desktop (or Docker Engine with the Compose plugin).

1. Start Docker Desktop, then build and install the client:

   ```bash
   cargo install --path .
   ```

2. Create your organization upload key:

   ```bash
   mkdir -p deploy/secrets
   python3 server/create_key.py my-organization \
     --file deploy/secrets/keys.json
   ```

   Save the key printed by this command; it is shown once. The server key file
   contains its hash. The container runs as UID 10001, so that user must be able
   to read the file. On Linux, you can set ownership with
   `sudo chown 10001:10001 deploy/secrets/keys.json`.

3. Start the local server and check its health:

   ```bash
   docker compose -f deploy/compose.yaml up -d --build server
   curl http://127.0.0.1:8080/health
   ```

   The health check should return `{"status":"ok"}`. Captured data persists in a
   Docker volume. This setup exposes the server only on your own computer.

4. Connect the client:

   ```bash
   export CLAUDETAP_UPLOAD_KEY='paste-your-key-here'
   claudetap cloud configure --endpoint http://127.0.0.1:8080
   unset CLAUDETAP_UPLOAD_KEY
   ```

   This enables uploads for **new sessions**. To include existing captures, add
   `--backfill` to the configure command. Uploads include captured prompts,
   responses, and machine metadata. Structured credentials are filtered, but
   arbitrary content and binary bodies can still contain secrets.

5. Run the uploader in a separate terminal:

   ```bash
   claudetap cloud sync
   ```

   Continue using `claudetap` normally in another terminal. Check upload status
   with:

   ```bash
   claudetap cloud status
   ```

   For a single upload pass instead of a continuous worker, use
   `claudetap cloud sync --once`.

6. Optionally install the uploader as a background service:

   Stop the manually running uploader with Ctrl-C first, then run:

   ```bash
   python3 deploy/client-service.py install
   ```

   This installs a per-user macOS LaunchAgent or Linux systemd service. Remove
   it with:

   ```bash
   python3 deploy/client-service.py uninstall
   ```

### Stop uploads or the local server

Disable uploads while keeping local capture working:

```bash
claudetap cloud disable
```

An in-flight upload pass can finish. Stop the worker or uninstall its service
for immediate shutdown. Run `cloud configure` again to re-enable uploads.

Stop the local server without deleting its stored data:

```bash
docker compose -f deploy/compose.yaml stop server
```

### Deploy a remote server

Use `deploy/aws.sh` or `deploy/gcp.sh` to provision a VM. These scripts require
cloud credentials and the environment variables listed at the top of each
script; running them creates billable resources. They do not publish a
marketplace listing.

Follow [cloud setup and deployment](docs/CLOUD.md) to install Docker on the VM,
create its organization key file, configure DNS and firewall access, and start
it with HTTPS:

```bash
DOMAIN=capture.example.com ./deploy/start.sh
```

The same container setup works on other Linux Docker hosts. Configure clients
with the server's HTTPS URL and their upload key; clients do not need AWS or GCP
credentials. See the deployment guide for persistence, backup, retrieval API,
and operational limitations, and the [seven-part roadmap](docs/CLOUD_PLAN.md)
for remaining work.

### Validation status

The Rust build/test and four server and client/server integration tests passed.
Shell syntax and Docker Compose configuration were checked. Container runtime
validation was not completed because Docker's daemon was stopped during
implementation. AWS/GCP deployment scripts have not been exercised against a
cloud account.

Run `claudetap cloud --help` for the available uploader commands.
