#!/usr/bin/env bash
# Install a Linux release, or build from source before binaries are published.
set -euo pipefail

main() {
    local repo="Leanmcp-Community/claudetap"
    local version="latest" source_only=false
    local prefix="${CLAUDETAP_INSTALL_DIR:-$HOME/.local/bin}"
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --source) source_only=true; shift ;;
            --version|--prefix)
                if [ "$#" -lt 2 ] || [ -z "$2" ]; then
                    echo "error: $1 requires a value" >&2; return 1
                fi
                case "$1" in
                    --version) version="$2" ;;
                    --prefix) prefix="$2" ;;
                esac
                shift 2 ;;
            --help|-h)
                echo "Usage: bash install-linux.sh [--version vX.Y.Z] [--prefix DIR] [--source]"
                echo "Installs to ~/.local/bin by default. --source builds with Cargo."
                return ;;
            *) echo "error: unknown option: $1" >&2; return 1 ;;
        esac
    done
    if [ "$(uname -s)" != Linux ]; then
        echo "error: this installer supports Linux; see README.md for other platforms" >&2
        return 1
    fi
    local arch
    case "$(uname -m)" in
        x86_64|amd64) arch=x86_64 ;;
        aarch64|arm64) arch=aarch64 ;;
        *) echo "error: supported Linux architectures are x86_64 and ARM64" >&2; return 1 ;;
    esac
    if [[ "$version" != latest && ! "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][a-zA-Z0-9.-]+)?$ ]]; then
        echo "error: version must be latest or a release tag such as v0.2.1" >&2
        return 1
    fi
    local command
    for command in curl tar sha256sum install mktemp; do
        command -v "$command" >/dev/null || { echo "error: $command is required" >&2; return 1; }
    done
    local work
    work=$(mktemp -d)
    trap "$(printf 'rm -rf -- %q' "$work")" EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    local asset="claudetap-${arch}-unknown-linux-gnu.tar.gz"
    local base="https://github.com/$repo/releases"
    if [ "$version" = latest ]; then
        base="$base/latest/download"
    else
        base="$base/download/$version"
    fi
    if [ "$source_only" = false ]; then
        echo ">>> Downloading claudetap ($version, $arch)"
        local status
        status=$(curl --proto '=https' --tlsv1.2 -sSL --retry 3 \
            -w '%{http_code}' -o "$work/$asset" "$base/$asset")
        if [ "$status" = 200 ]; then
            curl --proto '=https' --tlsv1.2 -fsSL --retry 3 \
                "$base/$asset.sha256" -o "$work/$asset.sha256"
            (cd "$work" && sha256sum --check "$asset.sha256")
            tar -xzf "$work/$asset" -C "$work" claudetap
            if ! "$work/claudetap" --version; then
                echo "error: this binary needs a compatible glibc Linux system; retry with --source" >&2
                return 1
            fi
        elif [ "$status" = 404 ]; then
            echo ">>> No release binary found; building from source instead."
            source_only=true
        else
            echo "error: release download returned HTTP $status" >&2
            return 1
        fi
    fi
    if [ "$source_only" = true ]; then
        if ! command -v cc >/dev/null || ! command -v cmake >/dev/null; then
            echo "error: source builds require a C compiler and CMake." >&2
            echo "Ubuntu/Debian: sudo apt-get install -y build-essential cmake pkg-config ca-certificates" >&2
            return 1
        fi
        if ! command -v cargo >/dev/null; then
            if [ -x "${CARGO_HOME:-$HOME/.cargo}/bin/cargo" ]; then
                export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
            else
                echo ">>> Installing the stable Rust toolchain for the source build."
                curl --proto '=https' --tlsv1.2 -fsSL --retry 3 \
                    https://sh.rustup.rs -o "$work/rustup.sh"
                sh "$work/rustup.sh" -y --profile minimal --default-toolchain stable --no-modify-path
                export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
            fi
        fi
        local ref="refs/heads/main"
        [ "$version" = latest ] || ref="refs/tags/$version"
        curl --proto '=https' --tlsv1.2 -fsSL --retry 3 \
            "https://github.com/$repo/archive/$ref.tar.gz" -o "$work/source.tar.gz"
        mkdir "$work/source"
        tar -xzf "$work/source.tar.gz" -C "$work/source" --strip-components=1
        cargo install --path "$work/source" --locked --root "$work/built"
        cp "$work/built/bin/claudetap" "$work/claudetap"
    fi
    mkdir -p "$prefix"
    install -m 755 "$work/claudetap" "$prefix/claudetap"
    echo ">>> Installed: $prefix/claudetap"
    "$prefix/claudetap" --version
    case ":$PATH:" in
        *":$prefix:"*) ;;
        *) printf 'Add this to your shell profile, then restart your shell:\n  export PATH=%q:"$PATH"\n' "$prefix" ;;
    esac
}

main "$@"
