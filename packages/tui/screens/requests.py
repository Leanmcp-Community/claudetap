"""Screen 2 — Request list for a single session."""

from __future__ import annotations

import subprocess

from rich.text import Text
from textual import on, work
from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import Screen
from textual.widgets import DataTable, Footer, Header, Input, Static

from ..models import SessionMeta, TrafficEntry, load_entries


def _method_style(method: str) -> str:
    """Return a Rich markup colour tag for the HTTP method."""
    m = method.upper()
    colours = {
        "GET": "green",
        "POST": "yellow",
        "PUT": "cyan",
        "DELETE": "red",
        "PATCH": "magenta",
        "OPTIONS": "dim",
        "HEAD": "dim",
    }
    c = colours.get(m, "white")
    return f"[{c}]{m:<7}[/{c}]"


def _status_style(code: int | None) -> str:
    if code is None:
        return "[dim]???[/dim]"
    s = str(code)
    if code < 300:
        return f"[green]{s}[/green]"
    if code < 400:
        return f"[cyan]{s}[/cyan]"
    if code < 500:
        return f"[yellow]{s}[/yellow]"
    return f"[red]{s}[/red]"


class RequestListScreen(Screen):
    """Browse all requests in a session."""

    BINDINGS = [
        ("escape", "go_back", "Back"),
        Binding("backspace", "go_back", "Back", show=False),
        ("q", "quit", "Quit"),
        ("slash", "toggle_filter", "Filter"),
        ("c", "copy_url", "Copy URL"),
    ]

    def __init__(self, session: SessionMeta) -> None:
        super().__init__()
        self.session = session
        self._all_entries: list[TrafficEntry] = []
        self._filtered_entries: list[TrafficEntry] = []
        self._filter_text = ""

    def compose(self) -> ComposeResult:
        yield Header()
        target = self.session.target
        started = self.session.started_short
        sid_short = self.session.session_id[:12] + "…"
        yield Static(
            f" 📋 {sid_short}  [bold]{target}[/bold]  {started}",
            id="request-header",
        )
        yield DataTable(id="request-table", cursor_type="row", zebra_stripes=True)
        yield Input(
            placeholder="Filter by URL (type and press Enter, Esc to clear)…",
            id="filter-input",
        )
        yield Footer()

    def on_mount(self) -> None:
        # Hide filter input initially
        self.query_one("#filter-input", Input).display = False
        self._load_entries()

    @work(thread=True)
    def _load_entries(self) -> None:
        """Load entries in a background thread to avoid blocking."""
        self._all_entries = load_entries(self.session.session_dir)
        self._filtered_entries = list(self._all_entries)
        self.app.call_from_thread(self._populate_table)

    def _populate_table(self) -> None:
        table = self.query_one("#request-table", DataTable)
        table.clear(columns=True)
        table.add_columns("#", "TIME", "METHOD", "STATUS", "URL")

        for i, e in enumerate(self._filtered_entries, 1):
            status_str = str(e.status or "?")
            status_text = Text(status_str)
            if e.status is not None:
                if e.status >= 500:
                    status_text.stylize("bold red")
                elif e.status >= 400:
                    status_text.stylize("bold yellow")
                elif e.status >= 300:
                    status_text.stylize("cyan")
                else:
                    status_text.stylize("green")
            else:
                status_text.stylize("dim")

            table.add_row(
                str(i),
                e.time_short,
                e.method,
                status_text,
                e.url_short,
                key=str(i - 1),  # index into _filtered_entries
            )

        # Update header with count
        target = self.session.target
        started = self.session.started_short
        sid_short = self.session.session_id[:12] + "…"
        count = len(self._filtered_entries)
        total = len(self._all_entries)
        count_str = f"{count}" if count == total else f"{count}/{total}"
        hdr = self.query_one("#request-header", Static)
        hdr.update(
            f" 📋 {sid_short}  [bold]{target}[/bold]  {started}  —  {count_str} requests"
        )

    @on(DataTable.RowSelected, "#request-table")
    def on_request_selected(self, event: DataTable.RowSelected) -> None:
        idx = int(event.row_key.value)
        if 0 <= idx < len(self._filtered_entries):
            entry = self._filtered_entries[idx]
            from .detail import DetailScreen
            self.app.push_screen(
                DetailScreen(
                    entry,
                    self._filtered_entries,
                    idx,
                    session_dir=self.session.session_dir,
                )
            )

    def action_go_back(self) -> None:
        # If filter is open, close it first
        inp = self.query_one("#filter-input", Input)
        if inp.display:
            inp.display = False
            self._filter_text = ""
            self._filtered_entries = list(self._all_entries)
            self._populate_table()
        else:
            self.app.pop_screen()

    def action_quit(self) -> None:
        self.app.exit()

    def action_copy_url(self) -> None:
        """Copy the selected row's URL to the system clipboard."""
        table = self.query_one("#request-table", DataTable)
        if table.cursor_row is not None and table.cursor_row < len(self._filtered_entries):
            entry = self._filtered_entries[table.cursor_row]
            try:
                subprocess.run(
                    ["pbcopy"], input=entry.url.encode(), check=True,
                )
                self.notify(f"Copied URL", severity="information", timeout=2)
            except Exception:
                self.notify("Copy failed", severity="error", timeout=2)

    def action_toggle_filter(self) -> None:
        inp = self.query_one("#filter-input", Input)
        inp.display = not inp.display
        if inp.display:
            inp.focus()
        else:
            self._filter_text = ""
            self._filtered_entries = list(self._all_entries)
            self._populate_table()

    @on(Input.Submitted, "#filter-input")
    def on_filter_submitted(self, event: Input.Submitted) -> None:
        self._filter_text = event.value.strip().lower()
        if self._filter_text:
            self._filtered_entries = [
                e for e in self._all_entries
                if self._filter_text in e.url.lower()
                or self._filter_text in e.method.lower()
                or self._filter_text in str(e.status or "").lower()
            ]
        else:
            self._filtered_entries = list(self._all_entries)
        self._populate_table()
        # Re-focus the table
        self.query_one("#request-table", DataTable).focus()
