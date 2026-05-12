"""Entry point: python -m packages.tui [session-id]"""

import sys
from .app import ClaudetapApp


def main():
    session_id = sys.argv[1] if len(sys.argv) > 1 else None
    app = ClaudetapApp(session_id=session_id)
    app.run()


if __name__ == "__main__":
    main()
