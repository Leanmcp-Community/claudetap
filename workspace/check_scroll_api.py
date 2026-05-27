"""Quick check that the VerticalScroll API we use exists in the installed
Textual version.

Run with: python workspace/check_scroll_api.py
"""

from __future__ import annotations

from textual.containers import VerticalScroll


def main() -> None:
    methods = [
        "scroll_home",
        "scroll_end",
        "scroll_up",
        "scroll_down",
        "scroll_page_up",
        "scroll_page_down",
        "focus",
    ]
    for m in methods:
        present = hasattr(VerticalScroll, m)
        print(f"  {m:20s} -> {'OK' if present else 'MISSING'}")


if __name__ == "__main__":
    main()
