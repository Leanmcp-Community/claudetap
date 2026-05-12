"""Data classes for claudetap session data."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class SessionMeta:
    session_id: str
    target: str
    started_at: str
    host_filter: list[str]
    entry_count: int
    session_dir: Path
    proxy_addr: str = ""
    claudetap_version: str = ""
    claude_pid: int = 0
    hostname: str = ""
    os_name: str = ""
    arch: str = ""

    @classmethod
    def from_dir(cls, d: Path) -> SessionMeta:
        meta: dict = {}
        meta_path = d / "meta.json"
        if meta_path.exists():
            try:
                meta = json.loads(meta_path.read_text())
            except Exception:
                pass

        entry_count = 0
        traffic = d / "traffic.jsonl"
        if traffic.exists():
            try:
                with traffic.open() as f:
                    entry_count = sum(1 for line in f if line.strip())
            except Exception:
                pass

        return cls(
            session_id=meta.get("session_id", d.name),
            target=meta.get("target", "?"),
            started_at=meta.get("started_at", ""),
            host_filter=meta.get("host_filter", []),
            entry_count=entry_count,
            session_dir=d,
            proxy_addr=meta.get("proxy_addr", ""),
            claudetap_version=meta.get("claudetap_version", ""),
            claude_pid=meta.get("claude_pid", 0),
            hostname=meta.get("hostname", ""),
            os_name=meta.get("os", ""),
            arch=meta.get("arch", ""),
        )

    @property
    def started_short(self) -> str:
        """e.g. '2026-05-11 10:37:37'"""
        return self.started_at[:19].replace("T", " ") if self.started_at else ""

    @property
    def hosts_short(self) -> str:
        if not self.host_filter:
            return ""
        s = ",".join(self.host_filter[:2])
        if len(self.host_filter) > 2:
            s += f"+{len(self.host_filter) - 2}"
        return s


@dataclass
class TrafficEntry:
    id: str
    ts_start: str
    ts_end: str
    method: str
    url: str
    http_version: str
    upstream_addr: str
    request: dict
    response: dict
    error: str | None

    @classmethod
    def from_dict(cls, d: dict) -> TrafficEntry:
        return cls(
            id=d.get("id", ""),
            ts_start=d.get("ts_start", ""),
            ts_end=d.get("ts_end", ""),
            method=d.get("method", "?"),
            url=d.get("url", ""),
            http_version=d.get("http_version", ""),
            upstream_addr=d.get("upstream_addr", ""),
            request=d.get("request", {}),
            response=d.get("response", {}),
            error=d.get("error"),
        )

    @property
    def status(self) -> int | None:
        return self.response.get("status")

    @property
    def time_short(self) -> str:
        """HH:MM:SS.mmm"""
        ts = self.ts_start
        return ts[11:23] if len(ts) >= 23 else ts

    @property
    def url_short(self) -> str:
        """Strip query params for display."""
        u = self.url
        q = u.find("?")
        if q > 0:
            return u[:q]
        return u

    @property
    def host(self) -> str:
        from urllib.parse import urlparse
        try:
            return urlparse(self.url).hostname or ""
        except Exception:
            return ""

    @property
    def req_content_type(self) -> str:
        return _get_header(self.request.get("headers", []), "content-type")

    @property
    def resp_content_type(self) -> str:
        return _get_header(self.response.get("headers", []), "content-type")

    @property
    def req_body_size(self) -> int:
        return self.request.get("body_size", 0) or 0

    @property
    def resp_body_size(self) -> int:
        return self.response.get("body_size", 0) or 0


def _get_header(headers: list, name: str) -> str:
    for h, v in headers:
        if h.lower() == name.lower():
            return v
    return ""


def load_entries(session_dir: Path) -> list[TrafficEntry]:
    """Load all traffic entries from a session directory, time-ascending."""
    traffic = session_dir / "traffic.jsonl"
    if not traffic.exists():
        return []
    entries = []
    with traffic.open() as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                entries.append(TrafficEntry.from_dict(json.loads(line)))
            except Exception:
                continue
    return entries
