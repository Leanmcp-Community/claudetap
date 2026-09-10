# Publishing Claudetap

Claudetap is a Rust executable. GitHub Releases distributes that executable
directly; npm distributes the same executable through platform-specific packages
and a small JavaScript launcher. No package is published by building this repo.

## GitHub Releases and the one-line installer

The `release-linux.yml` workflow builds and tests on native Ubuntu 22.04 x64 and
ARM64 runners. It publishes these files when a `v*` version tag is pushed:

- `claudetap-x86_64-unknown-linux-gnu.tar.gz` and its `.sha256` checksum
- `claudetap-aarch64-unknown-linux-gnu.tar.gz` and its `.sha256` checksum

These builds require glibc 2.35 or newer. Alpine/musl, Windows, and macOS release
binaries are not included. GitHub Actions needs permission to write releases.

Before releasing, update `Cargo.toml` and `Cargo.lock` to the intended version,
commit the changes, and merge the installer and workflows. Then, using a version
that matches Cargo and has not already been released:

```bash
# Example only: choose the actual next version first.
git tag v0.2.1
git push origin v0.2.1
```

Pushing the tag publishes a GitHub Release after both builds pass. Pre-release
tags should use a hyphen, such as `v0.3.0-beta.1`; these are marked as prereleases.
Once the script is on `main`, users can install with:

```bash
curl -fsSL https://raw.githubusercontent.com/Leanmcp-Community/claudetap/main/scripts/install-linux.sh | bash
```

The installer prefers a checksummed prebuilt binary and falls back to building
the selected tag (or `main` for `latest`) if the asset returns HTTP 404. A corrupt
checksum, failed download, or incompatible binary fails without installing it.
Source fallback installs Rust if missing, but requires C build tools and CMake
already installed. It does not install system packages or change shell profiles.

## npm packaging and publishing

Maintainer prerequisites: Python 3.11+, Node.js 18+, npm, and the two Linux
release archives with checksums. Use artifacts from the same version and commit
as the checkout's `Cargo.toml`. You need an npm account with publishing access to
all three chosen names. Name availability and ownership have not been established.

Download the release assets (after publishing the matching GitHub release):

```bash
mkdir -p dist
gh release download v0.2.1 --repo Leanmcp-Community/claudetap \
  --pattern 'claudetap-*-unknown-linux-gnu.tar.gz*' --dir dist
```

Generate packages. Prefer an npm scope you control; substitute your real scope:

```bash
python3 scripts/prepare-npm.py --artifacts dist --name @YOUR_SCOPE/claudetap
```

Without `--name`, the names default to `claudetap`, `claudetap-linux-x64`, and
`claudetap-linux-arm64`. With a scope, all three names use that scope. The script
verifies checksums, release versions, and ELF architectures and writes `dist/npm/`. It refuses to
overwrite an existing output directory; use `--output` for a fresh build.

The launcher has exact-version optional dependencies on both native packages.
npm selects the package matching the user's Linux CPU architecture. The launcher
forwards arguments, terminal input/output, and the native process's exit status.
No Rust compiler or installation-time binary download is needed by npm users.
Optional dependencies must be enabled. See npm's
[package manifest documentation](https://docs.npmjs.com/cli/v11/configuring-npm/package-json/).

Inspect the packages without uploading anything:

```bash
node scripts/publish-npm.mjs dist/npm
```

Once the names, version, and package contents are correct:

```bash
npm login
node scripts/publish-npm.mjs dist/npm --publish
```

The script dry-runs every package first, then publishes the two native packages
before the launcher. Prerelease versions use the npm `next` tag; others use
`latest`. npm may require two-factor authentication. Publication is not atomic:
if it stops after a native package is published, inspect registry state and
publish the remaining prepared packages manually using `npm publish <directory>
--access public` with the appropriate `--tag`. npm does not allow reusing an
already-published name/version. See [npm publish](https://docs.npmjs.com/cli/v11/commands/npm-publish/).

Only after publication, advertise commands using the actual package name:

```bash
npm install -g @YOUR_SCOPE/claudetap
# Or run without a global install:
npx @YOUR_SCOPE/claudetap --help
```

## apt, Snap, and Windows executables

`apt install claudetap` requires publishing Debian packages in an apt repository
and configuring that repository on users' machines. Snap requires a snap package
and a Snap Store release. Those publishing channels are not configured here.
The standalone Linux installer is the current distribution workflow provided by
this repository for users without Rust or Node.js, once binary releases exist.

`npm pack` produces a `.tgz` package archive, not an `.exe`. For Claudetap,
compile the Rust application for Windows to produce `claudetap.exe`, test its
Windows behavior, then distribute it directly or include it in a Windows npm
package. A Windows CI/release job and launcher support would be needed first.

Claude Code's native Windows installation similarly provides `claude.exe`.
Its npm distribution installs the same native executable through platform
packages, according to the [Claude Code installation documentation](https://code.claude.com/docs/en/installation#install-with-npm).
JavaScript applications can also bundle a runtime into an executable; this is
unnecessary for Claudetap because Rust already compiles to native machine code.
