"""Screen 3 — Request detail view."""

from __future__ import annotations

from pathlib import Path

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import VerticalScroll
from textual.screen import Screen
from textual.widgets import Footer, Header, Static

from ..decoder import decode_body, duration_ms
from ..models import TrafficEntry


# Max lines to show before truncating a body
BODY_COLLAPSE_THRESHOLD = 50


def _method_colour(m: str) -> str:
    colours = {"GET": "green", "POST": "yellow", "PUT": "cyan",
               "DELETE": "red", "PATCH": "magenta", "OPTIONS": "dim", "HEAD": "dim"}
    return colours.get(m.upper(), "white")


def _status_colour(code: int | None) -> str:
    if code is None:
        return "dim"
    if code < 300:
        return "green"
    if code < 400:
        return "cyan"
    if code < 500:
        return "yellow"
    return "red"


def _escape(s: str) -> str:
    """Escape Rich markup characters in user content."""
    return s.replace("[", "\\[").replace("]", "\\]")


class DetailScreen(Screen):
    """Show full detail for one request/response."""

    BINDINGS = [
        ("escape", "go_back", "Back"),
        Binding("backspace", "go_back", "Back", show=False),
        ("q", "quit", "Quit"),
        ("n", "next_entry", "Next"),
        ("p", "prev_entry", "Prev"),
        Binding("right", "next_entry", "Next →", show=False),
        Binding("left", "prev_entry", "← Prev", show=False),
    ]

    def __init__(
        self,
        entry: TrafficEntry,
        entries: list[TrafficEntry],
        index: int,
    ) -> None:
        super().__init__()
        self.entry = entry
        self.entries = entries
        self.index = index

    def compose(self) -> ComposeResult:
        yield Header()
        with VerticalScroll(id="detail-scroll"):
            yield Static(id="detail-content")
        yield Footer()

    def on_mount(self) -> None:
        self._render_entry()

    def _render_entry(self) -> None:
        e = self.entry
        mc = _method_colour(e.method)
        sc = _status_colour(e.status)
        dur = duration_ms(e.ts_start, e.ts_end)
        pos = f"{self.index + 1}/{len(self.entries)}"

        lines: list[str] = []

        # ── Summary ──
        lines.append(
            f"[dim]\\[{pos}][/dim]  "
            f"[{mc} bold]{e.method}[/{mc} bold]  "
            f"[{sc} bold]{e.status or '???'}[/{sc} bold]  "
            f"[dim]{dur}[/dim]  "
            f"[dim]{e.ts_start[:19].replace('T', ' ')}[/dim]"
        )
        lines.append(f"[bold]{_escape(e.url)}[/bold]")
        lines.append(f"[dim]id={e.id}[/dim]")
        if e.error:
            lines.append(f"[red bold]ERROR: {_escape(e.error)}[/red bold]")

        lines.append("")

        # ── Request ──
        lines.append("[cyan bold]━━━ REQUEST ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━[/cyan bold]")
        lines.append("")

        # Headers
        req_headers = e.request.get("headers", [])
        if req_headers:
            for h, v in req_headers:
                lines.append(f"  [bold cyan]{_escape(h)}[/bold cyan]: {_escape(v)}")
        else:
            lines.append("  [dim](no headers)[/dim]")

        # Body
        req_size = e.req_body_size
        if req_size > 0:
            body_txt, label, decoded = decode_body(
                e.request.get("body_inline"),
                e.request.get("body_path"),
                e.req_content_type,
            )
            label_display = label.upper()
            decoded_badge = (
                "[green bold]✓ DECODED[/green bold]"
                if decoded
                else "[red bold]✗ RAW[/red bold]"
            )
            lines.append("")
            lines.append(
                f"  [dim]body ({req_size:,} bytes, {label_display})[/dim]  {decoded_badge}"
            )
            lines.append("")
            if body_txt:
                body_lines = body_txt.splitlines()
                if len(body_lines) > BODY_COLLAPSE_THRESHOLD:
                    for bl in body_lines[:BODY_COLLAPSE_THRESHOLD]:
                        lines.append(f"    {_escape(bl)}")
                    remaining = len(body_lines) - BODY_COLLAPSE_THRESHOLD
                    lines.append(
                        f"    [dim]… {remaining} more lines (total {len(body_lines)})[/dim]"
                    )
                else:
                    for bl in body_lines:
                        lines.append(f"    {_escape(bl)}")
        else:
            lines.append("")
            lines.append("  [dim](no request body)[/dim]")

        lines.append("")

        # ── Response ──
        status_str = str(e.status or "???")
        lines.append(f"[green bold]━━━ RESPONSE {status_str} ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━[/green bold]")
        lines.append("")

        # Headers
        resp_headers = e.response.get("headers", [])
        if resp_headers:
            for h, v in resp_headers:
                lines.append(f"  [bold cyan]{_escape(h)}[/bold cyan]: {_escape(v)}")
        else:
            lines.append("  [dim](no headers)[/dim]")

        # Body / Stream
        is_stream = e.response.get("is_stream", False)
        stream_path = e.response.get("stream_path")
        resp_size = e.resp_body_size

        if is_stream and stream_path:
            lines.append("")
            lines.append(f"  [dim]streaming SSE → {_escape(stream_path)}[/dim]")
            p = Path(stream_path)
            if p.exists():
                sse_lines = p.read_text(errors="replace").splitlines()
                show = sse_lines[:80]
                for sl in show:
                    lines.append(f"    {_escape(sl)}")
                if len(sse_lines) > 80:
                    lines.append(
                        f"    [dim]… {len(sse_lines) - 80} more lines[/dim]"
                    )
        elif resp_size > 0:
            body_txt, label, decoded = decode_body(
                e.response.get("body_inline"),
                e.response.get("body_path"),
                e.resp_content_type,
            )
            label_display = label.upper()
            decoded_badge = (
                "[green bold]✓ DECODED[/green bold]"
                if decoded
                else "[red bold]✗ RAW[/red bold]"
            )
            lines.append("")
            lines.append(
                f"  [dim]body ({resp_size:,} bytes, {label_display})[/dim]  {decoded_badge}"
            )
            lines.append("")
            if body_txt:
                body_lines = body_txt.splitlines()
                if len(body_lines) > BODY_COLLAPSE_THRESHOLD:
                    for bl in body_lines[:BODY_COLLAPSE_THRESHOLD]:
                        lines.append(f"    {_escape(bl)}")
                    remaining = len(body_lines) - BODY_COLLAPSE_THRESHOLD
                    lines.append(
                        f"    [dim]… {remaining} more lines (total {len(body_lines)})[/dim]"
                    )
                else:
                    for bl in body_lines:
                        lines.append(f"    {_escape(bl)}")
        else:
            lines.append("")
            lines.append("  [dim](no response body)[/dim]")

        content = self.query_one("#detail-content", Static)
        content.update("\n".join(lines))

        # Scroll to top
        scroll = self.query_one("#detail-scroll", VerticalScroll)
        scroll.scroll_home(animate=False)

    def action_go_back(self) -> None:
        self.app.pop_screen()

    def action_quit(self) -> None:
        self.app.exit()

    def action_next_entry(self) -> None:
        if self.index + 1 < len(self.entries):
            self.index += 1
            self.entry = self.entries[self.index]
            self._render_entry()

    def action_prev_entry(self) -> None:
        if self.index > 0:
            self.index -= 1
            self.entry = self.entries[self.index]
            self._render_entry()
