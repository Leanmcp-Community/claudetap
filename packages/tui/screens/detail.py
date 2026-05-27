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


# Max lines to show before truncating a body (when collapsed).
BODY_COLLAPSE_THRESHOLD = 50
# Max lines to show for streamed SSE / WebSocket transcripts (when collapsed).
STREAM_COLLAPSE_THRESHOLD = 80


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
        ("x", "toggle_expand", "Expand body"),
        # Vim-style scrolling fallbacks. The inner VerticalScroll already
        # handles up/down/pgup/pgdn/home/end when focused, but j/k/d/u are
        # nicer for terminal users and work even if focus drifts.
        Binding("j", "scroll_down", "Scroll ↓", show=False),
        Binding("k", "scroll_up", "Scroll ↑", show=False),
        Binding("ctrl+d", "page_down", "Page ↓", show=False),
        Binding("ctrl+u", "page_up", "Page ↑", show=False),
    ]

    def __init__(
        self,
        entry: TrafficEntry,
        entries: list[TrafficEntry],
        index: int,
        session_dir: Path | None = None,
    ) -> None:
        super().__init__()
        self.entry = entry
        self.entries = entries
        self.index = index
        self.session_dir = session_dir
        # When True, request/response bodies and SSE/WS transcripts are
        # rendered in full instead of being collapsed to the threshold.
        self._show_full_bodies = False

    def compose(self) -> ComposeResult:
        yield Header()
        with VerticalScroll(id="detail-scroll"):
            yield Static(id="detail-content")
        yield Footer()

    def on_mount(self) -> None:
        self._render_entry()

    def _render_entry(self, *, scroll_to_top: bool = True) -> None:
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
                e.req_content_encoding,
                session_dir=self.session_dir,
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
                if (
                    not self._show_full_bodies
                    and len(body_lines) > BODY_COLLAPSE_THRESHOLD
                ):
                    for bl in body_lines[:BODY_COLLAPSE_THRESHOLD]:
                        lines.append(f"    {_escape(bl)}")
                    remaining = len(body_lines) - BODY_COLLAPSE_THRESHOLD
                    total = len(body_lines)
                    lines.append(
                        f"    [dim]… {remaining} more lines "
                        f"(total {total}) — press [bold]x[/bold] "
                        f"to expand[/dim]"
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
        is_websocket = e.response.get("is_websocket", False)
        ws_path = e.response.get("ws_path")
        resp_size = e.resp_body_size

        if is_websocket and ws_path:
            lines.append("")
            lines.append(f"  [dim]WebSocket UPGRADED → {_escape(ws_path)}[/dim]")
            p = Path(ws_path)
            if not p.is_absolute() and self.session_dir is not None:
                p = self.session_dir / p
            if p.exists():
                import json
                ws_lines = p.read_text(errors="replace").splitlines()
                if self._show_full_bodies:
                    show = ws_lines
                else:
                    show = ws_lines[:STREAM_COLLAPSE_THRESHOLD]
                for sl in show:
                    try:
                        frame = json.loads(sl)
                        arr = "→" if frame.get("dir") == "c2s" else "←"
                        col = "green" if frame.get("dir") == "c2s" else "cyan"
                        op = frame.get("op", "?")
                        ln = frame.get("len", 0)
                        lines.append(f"  [{col}]{arr}[/{col}] [bold]{op}[/bold] [dim]len={ln}[/dim]")
                        if frame.get("payload"):
                            lines.append(f"      {_escape(frame['payload'])}")
                        elif frame.get("payload_b64"):
                            lines.append(f"      [dim]b64: {_escape(frame['payload_b64'])}[/dim]")
                    except:
                        lines.append(f"    {_escape(sl)}")
                if (
                    not self._show_full_bodies
                    and len(ws_lines) > STREAM_COLLAPSE_THRESHOLD
                ):
                    remaining = len(ws_lines) - STREAM_COLLAPSE_THRESHOLD
                    lines.append(
                        f"    [dim]… {remaining} more lines — "
                        f"press [bold]x[/bold] to expand[/dim]"
                    )
        elif is_stream and stream_path:
            lines.append("")
            lines.append(f"  [dim]streaming SSE → {_escape(stream_path)}[/dim]")
            p = Path(stream_path)
            if not p.is_absolute() and self.session_dir is not None:
                p = self.session_dir / p
            if p.exists():
                sse_lines = p.read_text(errors="replace").splitlines()
                if self._show_full_bodies:
                    show = sse_lines
                else:
                    show = sse_lines[:STREAM_COLLAPSE_THRESHOLD]
                for sl in show:
                    lines.append(f"    {_escape(sl)}")
                if (
                    not self._show_full_bodies
                    and len(sse_lines) > STREAM_COLLAPSE_THRESHOLD
                ):
                    remaining = len(sse_lines) - STREAM_COLLAPSE_THRESHOLD
                    lines.append(
                        f"    [dim]… {remaining} more lines — "
                        f"press [bold]x[/bold] to expand[/dim]"
                    )
        elif resp_size > 0:
            body_txt, label, decoded = decode_body(
                e.response.get("body_inline"),
                e.response.get("body_path"),
                e.resp_content_type,
                e.resp_content_encoding,
                session_dir=self.session_dir,
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
                if (
                    not self._show_full_bodies
                    and len(body_lines) > BODY_COLLAPSE_THRESHOLD
                ):
                    for bl in body_lines[:BODY_COLLAPSE_THRESHOLD]:
                        lines.append(f"    {_escape(bl)}")
                    remaining = len(body_lines) - BODY_COLLAPSE_THRESHOLD
                    total = len(body_lines)
                    lines.append(
                        f"    [dim]… {remaining} more lines "
                        f"(total {total}) — press [bold]x[/bold] "
                        f"to expand[/dim]"
                    )
                else:
                    for bl in body_lines:
                        lines.append(f"    {_escape(bl)}")
        else:
            lines.append("")
            lines.append("  [dim](no response body)[/dim]")

        content = self.query_one("#detail-content", Static)
        content.update("\n".join(lines))

        scroll = self.query_one("#detail-scroll", VerticalScroll)
        if scroll_to_top:
            scroll.scroll_home(animate=False)
        # Make sure the scroll container is the focused widget so up/down,
        # PgUp/PgDn, Home/End keys go to it (and not get swallowed by the
        # screen's other bindings).
        scroll.focus()

    def action_go_back(self) -> None:
        self.app.pop_screen()

    def action_quit(self) -> None:
        self.app.exit()

    def action_next_entry(self) -> None:
        if self.index + 1 < len(self.entries):
            self.index += 1
            self.entry = self.entries[self.index]
            # New entry → reset expand state so we don't accidentally
            # render a giant body for an unrelated request.
            self._show_full_bodies = False
            self._render_entry()

    def action_prev_entry(self) -> None:
        if self.index > 0:
            self.index -= 1
            self.entry = self.entries[self.index]
            self._show_full_bodies = False
            self._render_entry()

    def action_toggle_expand(self) -> None:
        """Toggle full-body / collapsed body rendering for the current entry.

        When expanding, the truncated tail (potentially thousands of lines)
        is rendered in full so the user can scroll through it. We preserve
        the scroll position so the toggle feels in-place.
        """
        self._show_full_bodies = not self._show_full_bodies
        self._render_entry(scroll_to_top=False)
        msg = "Body expanded" if self._show_full_bodies else "Body collapsed"
        self.notify(msg, severity="information", timeout=1)

    def action_scroll_down(self) -> None:
        self.query_one("#detail-scroll", VerticalScroll).scroll_down(animate=False)

    def action_scroll_up(self) -> None:
        self.query_one("#detail-scroll", VerticalScroll).scroll_up(animate=False)

    def action_page_down(self) -> None:
        self.query_one("#detail-scroll", VerticalScroll).scroll_page_down(animate=False)

    def action_page_up(self) -> None:
        self.query_one("#detail-scroll", VerticalScroll).scroll_page_up(animate=False)
