"""Decode base64/gzip HTTP bodies into human-readable text."""

from __future__ import annotations

import base64
import gzip
import json
import shutil
import subprocess
from datetime import datetime
from pathlib import Path


def _try_protobuf_decode(raw: bytes) -> str | None:
    """
    Attempt schema-less protobuf decoding via `protoc --decode_raw`.
    Returns decoded text on success, None on failure.
    """
    protoc = shutil.which("protoc")
    if not protoc:
        return None
    try:
        result = subprocess.run(
            [protoc, "--decode_raw"],
            input=raw,
            capture_output=True,
            timeout=5,
        )
        if result.returncode == 0 and result.stdout:
            return result.stdout.decode("utf-8", errors="replace")
    except Exception:
        pass
    return None


def decode_body(
    body_inline: dict | None,
    body_path: str | None,
    content_type: str,
) -> tuple[str, str, bool]:
    """
    Decode a request/response body.

    Returns (decoded_text, label, decoded) where:
      - label is one of: "json", "text", "proto", "binary", "empty", "error", "missing"
      - decoded is True if the body was successfully decoded into human-readable form,
        False if it's still raw/hex/binary
    """
    if body_inline is None and not body_path:
        return "", "empty", False

    if body_path:
        p = Path(body_path)
        if p.exists():
            raw = p.read_bytes()
        else:
            return f"(body file not found: {body_path})", "missing", False
    else:
        enc = body_inline.get("encoding", "")
        data_str = body_inline.get("data", "")
        if not data_str:
            return "", "empty", False
        if enc == "base64":
            # Fix padding
            missing = len(data_str) % 4
            if missing:
                data_str += "=" * (4 - missing)
            try:
                raw = base64.b64decode(data_str)
            except Exception as e:
                return f"(base64 decode error: {e})", "error", False
        elif enc == "utf-8":
            raw = data_str.encode()
        else:
            raw = data_str.encode()

    # Decompress gzip
    if raw[:2] == b"\x1f\x8b":
        try:
            raw = gzip.decompress(raw)
        except Exception as e:
            return f"(gzip error: {e})", "error", False

    ct = content_type.lower()

    # JSON
    if "json" in ct or ct == "":
        try:
            obj = json.loads(raw)
            return json.dumps(obj, indent=2, ensure_ascii=False), "json", True
        except Exception:
            pass

    # Plaintext
    if any(t in ct for t in ("text/", "xml", "html", "svg", "csv")):
        try:
            return raw.decode("utf-8", errors="replace"), "text", True
        except Exception:
            pass

    # Try JSON anyway (some gRPC-JSON comes as application/proto)
    try:
        obj = json.loads(raw)
        return json.dumps(obj, indent=2, ensure_ascii=False), "json", True
    except Exception:
        pass

    # Protobuf — try schema-less decode via protoc
    if "proto" in ct or "connect+proto" in ct or "grpc" in ct:
        decoded = _try_protobuf_decode(raw)
        if decoded:
            return decoded, "proto", True
        # protoc not available or failed — fall through to hex dump

    # Binary with possible proto content (heuristic: starts with common proto field tags)
    if "octet" in ct or "binary" in ct:
        # Try proto decode as heuristic even without proto content-type
        decoded = _try_protobuf_decode(raw)
        if decoded:
            return decoded, "proto", True

    # Hex dump for truly un-decodable binary
    if "proto" in ct or "octet" in ct or "binary" in ct or "git" in ct:
        label = "proto" if "proto" in ct else "binary"
    else:
        label = "binary"

    hex_lines = []
    for i in range(0, min(len(raw), 512), 16):
        chunk = raw[i : i + 16]
        hex_part = " ".join(f"{b:02x}" for b in chunk)
        text_part = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
        hex_lines.append(f"{i:04x}  {hex_part:<48}  {text_part}")
    if len(raw) > 512:
        hex_lines.append(f"... ({len(raw)} bytes total, showing first 512)")
    return "\n".join(hex_lines), label, False


def get_header(headers: list, name: str) -> str:
    """Case-insensitive header lookup."""
    for h, v in headers:
        if h.lower() == name.lower():
            return v
    return ""


def duration_ms(ts_start: str, ts_end: str) -> str:
    """Human-readable duration between two ISO timestamps."""
    try:
        fmt = "%Y-%m-%dT%H:%M:%S.%fZ"
        a = datetime.strptime(ts_start, fmt)
        b = datetime.strptime(ts_end, fmt)
        ms = (b - a).total_seconds() * 1000
        if ms < 1000:
            return f"{ms:.0f}ms"
        return f"{ms / 1000:.1f}s"
    except Exception:
        return "?"
