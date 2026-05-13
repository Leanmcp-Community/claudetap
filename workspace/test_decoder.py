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

    # Git smart-HTTP — real bytes from GitHub's info/refs?service=git-upload-pack
    git_advert = (
        b"001e# service=git-upload-pack\n"
        b"0000"
        b"000eversion 2\n"
        b"0028agent=git/github-f8bdfd365d97-Linux\n"
        b"0013ls-refs=unborn\n"
        b"0027fetch=shallow wait-for-done filter\n"
        b"0012server-option\n"
        b"0017object-format=sha1\n"
        b"0000"
    )
    case(
        "git smart-HTTP info/refs advertisement",
        git_advert,
        "application/x-git-upload-pack-advertisement",
        "",
    )

    # Junk that looks like 4 hex chars but isn't valid pkt-line framing
    case(
        "git content-type but junk bytes (should fall through to hex)",
        b"\x00\x01\x02\x03\xff\xfe",
        "application/x-git-upload-pack-result",
        "",
    )

    # NDJSON labelled as application/json (1DS Web SDK telemetry pattern)
    ndjson = (
        b'{"name":"monacoworkbench/extHostDeprecatedApiUsage",'
        b'"time":"2026-05-13T10:13:41.196Z","ver":"4.0"}\n'
        b'{"name":"editor/autoSave","time":"2026-05-13T10:13:42.001Z"}\n'
        b'{"name":"workbench/heartbeat","time":"2026-05-13T10:13:43.500Z"}\n'
    )
    case(
        "NDJSON as application/json (1DS telemetry style)",
        ndjson,
        "application/json",
        "",
    )

    # application/json that's neither single JSON nor valid NDJSON — should
    # still come back as readable text instead of a hex dump.
    case(
        "garbled JSON-typed body falls back to text",
        b'{"truncated":',
        "application/json",
        "",
    )

    # body_path resolved against session_dir
    import tempfile
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = os.path.join(tmp, "bodies")
        os.makedirs(tmp_path)
        body_file = os.path.join(tmp_path, "01ABC.req.bin")
        big_payload = b'{"events":[' + b'{"k":"v"},' * 1000 + b'{"k":"v"}]}'
        with open(body_file, "wb") as f:
            f.write(big_payload)

        # Relative path + session_dir provided → should resolve & decode
        from pathlib import Path
        txt, label, dec = decoder.decode_body(
            None,
            "bodies/01ABC.req.bin",
            "application/json",
            "",
            session_dir=Path(tmp),
        )
        print("--- body_path relative + session_dir provided ---")
        print(f"  label={label!r}  decoded={dec}")
        print(f"  text={txt[:80]!r}…")
        print()

        # Relative path + NO session_dir → should be MISSING (regression: this is
        # the bug the telemetry payload hit)
        txt, label, dec = decoder.decode_body(
            None,
            "bodies/01ABC.req.bin",
            "application/json",
            "",
        )
        print("--- body_path relative + no session_dir (the bug we just fixed) ---")
        print(f"  label={label!r}  decoded={dec}")
        print(f"  text={txt!r}")
        print()

    return 0


if __name__ == "__main__":
    sys.exit(main())
