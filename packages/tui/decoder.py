"""Decode base64/gzip HTTP bodies into human-readable text."""

from __future__ import annotations

import base64
import gzip
import json
from datetime import datetime
from pathlib import Path


def decode_body(
    body_inline: dict | None,
    body_path: str | None,
    content_type: str,
) -> tuple[str, str]:
    """
    Decode a request/response body.

    Returns (decoded_text, label) where label is one of:
      "json", "text", "proto", "binary", "empty", "error", "missing"
    """
    if body_inline is None and not body_path:
        return "", "empty"

    if body_path:
        p = Path(body_path)
        if p.exists():
            raw = p.read_bytes()
        else:
            return f"(body file not found: {body_path})", "missing"
    else:
        enc = body_inline.get("encoding", "")
        data_str = body_inline.get("data", "")
        if not data_str:
            return "", "empty"
        if enc == "base64":
            # Fix padding
            missing = len(data_str) % 4
            if missing:
                data_str += "=" * (4 - missing)
            try:
                raw = base64.b64decode(data_str)
            except Exception as e:
                return f"(base64 decode error: {e})", "error"
        elif enc == "utf-8":
            raw = data_str.encode()
        else:
            raw = data_str.encode()

    # Decompress gzip
    if raw[:2] == b"\x1f\x8b":
        try:
            raw = gzip.decompress(raw)
        except Exception as e:
            return f"(gzip error: {e})", "error"

    ct = content_type.lower()

    # JSON
    if "json" in ct or ct == "":
        try:
            obj = json.loads(raw)
            return json.dumps(obj, indent=2, ensure_ascii=False), "json"
        except Exception:
            pass

    # Plaintext
    if any(t in ct for t in ("text/", "xml", "html", "svg", "csv")):
        try:
            return raw.decode("utf-8", errors="replace"), "text"
        except Exception:
            pass

    # Try JSON anyway (some gRPC-JSON comes as application/proto)
    try:
        obj = json.loads(raw)
        return json.dumps(obj, indent=2, ensure_ascii=False), "json"
    except Exception:
        pass

    # Hex dump for binary/proto
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
    return "\n".join(hex_lines), label


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
