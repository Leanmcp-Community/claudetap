"""Smoke test for packages/tui/decoder.py content-encoding handling.

Run from the repo root:
    python workspace/test_decoder.py
"""
from __future__ import annotations

import base64
import gzip
import os
import sys
import zlib

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO_ROOT, "packages", "tui"))

import decoder  # noqa: E402


def case(title: str, raw: bytes, content_type: str, content_encoding: str) -> None:
    body_inline = {"encoding": "base64", "data": base64.b64encode(raw).decode()}
    txt, label, dec = decoder.decode_body(body_inline, None, content_type, content_encoding)
    print(f"--- {title} ---")
    print(f"  label={label!r}  decoded={dec}")
    preview = txt if len(txt) < 200 else txt[:200] + "…"
    print(f"  text={preview!r}")
    print()


def main() -> int:
    payload = b'{"hello":"world","n":42}'

    # gzip
    case("gzip via content-encoding", gzip.compress(payload), "application/json", "gzip")
    case("gzip via magic-byte fallback (no header)", gzip.compress(payload), "application/json", "")

    # deflate (both flavors)
    co = zlib.compressobj(wbits=-zlib.MAX_WBITS)
    raw_deflate = co.compress(payload) + co.flush()
    case("deflate (raw)", raw_deflate, "application/json", "deflate")
    case("deflate (zlib-wrapped)", zlib.compress(payload), "application/json", "deflate")

    # br — depends on whether `brotli` is installed
    try:
        import brotli  # type: ignore

        case("brotli (br) with `brotli` installed", brotli.compress(payload), "application/json", "br")
    except ImportError:
        # Use a few random bytes; we only care that the error path triggers.
        case("brotli (br) WITHOUT `brotli` installed", b"\x0b\xd5g\x00", "application/json", "br")

    # zstd
    try:
        import zstandard  # type: ignore

        zc = zstandard.ZstdCompressor().compress(payload)
        case("zstd with `zstandard` installed", zc, "application/json", "zstd")
    except ImportError:
        case("zstd WITHOUT `zstandard` installed", b"\x28\xb5\x2f\xfd\x00", "application/json", "zstd")

    # Identity / unknown / multi-coding
    case("identity passthrough", payload, "application/json", "identity")
    case("unknown coding", b"hello", "text/plain", "snappy")
    case(
        "stacked: gzip wrapped then declared as gzip",
        gzip.compress(payload),
        "application/json",
        "gzip",
    )

    return 0


if __name__ == "__main__":
    sys.exit(main())
