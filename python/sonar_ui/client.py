"""SonarClient — JSON IPC client for the Sonar engine.

Talks to the `sonar serve` subprocess over newline-delimited JSON.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
from typing import Optional


import threading


class SonarError(Exception):
    """Raised when the engine returns an error or is not reachable."""


class SonarClient:
    """JSON IPC client for the Sonar engine."""

    def __init__(self, binary: str):
        self.binary = binary
        self.proc: Optional[subprocess.Popen] = None
        self.lock = threading.Lock()

    # ── lifecycle ─────────────────────────────────────────────────────

    def start(self):
        """Start the `sonar serve` subprocess."""
        self.proc = subprocess.Popen(
            [self.binary, "serve"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            bufsize=1,
            universal_newlines=True,
        )

    def stop(self):
        """Stop the subprocess."""
        if self.proc:
            try:
                self._send({"cmd": "quit"})
            except Exception:
                pass
            try:
                self.proc.terminate()
                self.proc.wait(timeout=2)
            except Exception:
                try:
                    self.proc.kill()
                except Exception:
                    pass
            self.proc = None

    # ── low-level send/recv ──────────────────────────────────────────

    def _send(self, obj: dict) -> Optional[dict]:
        if not self.proc or not self.proc.stdin or not self.proc.stdout:
            raise SonarError("engine not started")
        line = json.dumps(obj) + "\n"
        with self.lock:
            self.proc.stdin.write(line)
            self.proc.stdin.flush()
            reply = self.proc.stdout.readline()
        if not reply:
            raise SonarError("engine closed stdout")
        try:
            return json.loads(reply)
        except json.JSONDecodeError as e:
            raise SonarError(f"bad JSON from engine: {e}")

    # ── high-level API ────────────────────────────────────────────────

    def place_smart(self) -> dict:
        return self._send({"cmd": "place_smart"}) or {}

    def place_random(self) -> dict:
        return self._send({"cmd": "place_random"}) or {}

    def choose_move(self, deadline_secs: int = 20) -> tuple[int, int]:
        r = self._send({"cmd": "choose_move", "deadline_secs": deadline_secs}) or {}
        return int(r.get("row", 0)), int(r.get("col", 0))

    def suggest_move(self, deadline_secs: int = 20) -> dict:
        return self._send({"cmd": "suggest_move", "deadline_secs": deadline_secs}) or {}

    def observe(self, r: int, c: int, result: str) -> dict:
        return self._send({"cmd": "observe", "r": r, "c": c, "result": result}) or {}

    def receive_shot(self, r: int, c: int) -> str:
        r_ = self._send({"cmd": "receive_shot", "r": r, "c": c}) or {}
        return r_.get("result", "miss")

    def snapshot(self) -> dict:
        return self._send({"cmd": "snapshot"}) or {}

    def config(self) -> dict:
        return self._send({"cmd": "config"}) or {}

    def rules(self) -> dict:
        return self._send({"cmd": "rules"}) or {}

    def set_rules(self, rules: dict) -> dict:
        return self._send({"cmd": "set_rules", "rules": rules}) or {}

    def reset(self) -> dict:
        return self._send({"cmd": "reset"}) or {}

    def record_game(self, won: bool) -> dict:
        return self._send({"cmd": "record_game", "won": won}) or {}

    def learning(self) -> dict:
        return self._send({"cmd": "learning"}) or {}


# ── binary discovery ──────────────────────────────────────────────────

def find_binary() -> str:
    """Find the `sonar` binary on the system."""
    env = os.environ.get("SONAR_BIN")
    if env and os.path.isfile(env):
        return env
    found = shutil.which("sonar")
    if found:
        return found
    here = os.path.dirname(os.path.abspath(__file__))
    candidates = [
        os.path.join(here, "..", "..", "target", "release", "sonar"),
        os.path.join(here, "..", "..", "target", "debug", "sonar"),
        "/home/z/my-project/sonar/target/release/sonar",
    ]
    for c in candidates:
        if os.path.isfile(c):
            return os.path.abspath(c)
    raise FileNotFoundError(
        "sonar binary not found. Set $SONAR_BIN or build with cargo build --release"
    )
