#!/usr/bin/env bash
# claudetap installer
# Builds and installs the `claudetap` binary into ~/.cargo/bin via `cargo install`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo not found. Install Rust from https://rustup.rs/ first." >&2
    exit 1
fi

echo ">>> Installing claudetap from ${SCRIPT_DIR}"
cargo install --path "${SCRIPT_DIR}" --color never "$@"

BIN_PATH="${CARGO_HOME:-$HOME/.cargo}/bin/claudetap"
if [[ -x "${BIN_PATH}" ]]; then
    echo ">>> Installed: ${BIN_PATH}"
    case ":${PATH}:" in
        *":${CARGO_HOME:-$HOME/.cargo}/bin:"*) ;;
        *)
            echo ">>> Note: ${CARGO_HOME:-$HOME/.cargo}/bin is not in your PATH."
            echo "    Add this line to your shell profile:"
            echo "        export PATH=\"\$HOME/.cargo/bin:\$PATH\""
            ;;
    esac
else
    echo "warning: expected binary not found at ${BIN_PATH}" >&2
fi
