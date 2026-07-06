#!/usr/bin/env python3
"""Sonar UI — entry point.

Usage:
    python3 -m sonar_ui        # from the python/ directory
    python3 sonar_ui/main.py   # directly
"""
import sys
import os

# Allow running as `python3 main.py` from the sonar_ui package dir.
if __name__ == "__main__" and __package__ is None:
    parent = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    sys.path.insert(0, parent)
    __package__ = "sonar_ui"

from sonar_ui.app import SonarApp


def main():
    try:
        app = SonarApp()
    except FileNotFoundError:
        sys.exit(1)
    except Exception:
        sys.exit(1)
    app.mainloop()


if __name__ == "__main__":
    main()
