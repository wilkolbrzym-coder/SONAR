#!/usr/bin/env python3
"""Sonar UI — convenience launcher.

Run from the python/ directory:
    python3 run.py
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

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
