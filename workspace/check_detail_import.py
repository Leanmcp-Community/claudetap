"""Smoke-test that the modified DetailScreen module still imports.

Run with: python workspace/check_detail_import.py
"""

from __future__ import annotations

import sys
from pathlib import Path

# Make the project root importable so `packages.tui...` resolves the same
# way it does when launched via `python -m packages.tui`.
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))


def main() -> None:
    from packages.tui.screens.detail import (
        DetailScreen,
        BODY_COLLAPSE_THRESHOLD,
        STREAM_COLLAPSE_THRESHOLD,
    )

    print(f"  BODY_COLLAPSE_THRESHOLD   = {BODY_COLLAPSE_THRESHOLD}")
    print(f"  STREAM_COLLAPSE_THRESHOLD = {STREAM_COLLAPSE_THRESHOLD}")

    bound_keys = sorted({b.key if hasattr(b, "key") else b[0]
                         for b in DetailScreen.BINDINGS})
    print(f"  bindings: {bound_keys}")

    actions = [name for name in dir(DetailScreen) if name.startswith("action_")]
    print(f"  actions:  {sorted(actions)}")


if __name__ == "__main__":
    main()
