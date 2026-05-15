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

*Add documentation on how to use `claudetap` here.*
