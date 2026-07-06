#!/usr/bin/env python3
"""
Sonar UI — a Python overlay for the Sonar battleship engine.

This is a thin GUI layer that talks to the Sonar Rust engine via the
JSON IPC protocol (`sonar serve`). No game logic lives here — every
decision is made by the engine.

Requirements:
    Python 3.9+
    Tkinter (usually ships with Python)

Usage:
    ./sonar_ui.py
    # or
    python3 sonar_ui.py

The UI will start the `sonar serve` subprocess automatically. Make sure
the `sonar` binary is on your $PATH or set $SONAR_BIN to its location.

Controls:
    Click on the enemy board to fire.
    S — ask Sonar for a suggestion.
    Settings menu — adjust move time limit (5..60s).
    File → New Game — reset.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import threading
import time
import tkinter as tk
from tkinter import messagebox, simpledialog
from typing import Optional


# ─────────────────────────────────────────────────────────────────────────────
# SonarClient — JSON IPC client.
# ─────────────────────────────────────────────────────────────────────────────

class SonarClient:
    """Talks to the `sonar serve` subprocess over newline-delimited JSON."""

    def __init__(self, binary: str):
        self.binary = binary
        self.proc: Optional[subprocess.Popen] = None

    def start(self):
        self.proc = subprocess.Popen(
            [self.binary, "serve"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            bufsize=1,
            universal_newlines=True,
        )

    def stop(self):
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

    def _send(self, obj: dict) -> Optional[dict]:
        if not self.proc or not self.proc.stdin or not self.proc.stdout:
            return None
        line = json.dumps(obj) + "\n"
        self.proc.stdin.write(line)
        self.proc.stdin.flush()
        reply = self.proc.stdout.readline()
        if not reply:
            return None
        try:
            return json.loads(reply)
        except json.JSONDecodeError:
            return None

    # ── high-level API ───────────────────────────────────────────────────

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

    def set_config(self, cfg: dict) -> dict:
        return self._send({"cmd": "set_config", "config": cfg}) or {}

    def reset(self) -> dict:
        return self._send({"cmd": "reset"}) or {}

    def record_game(self, won: bool) -> dict:
        return self._send({"cmd": "record_game", "won": won}) or {}

    def learning(self) -> dict:
        return self._send({"cmd": "learning"}) or {}


# ─────────────────────────────────────────────────────────────────────────────
# SonarUI — Tkinter GUI.
# ─────────────────────────────────────────────────────────────────────────────

CELL_SIZE = 32
BOARD_PAD = 40


class SonarUI:
    def __init__(self, root: tk.Tk):
        self.root = root
        self.root.title("Sonar — Battleship AI")
        self.client = SonarClient(self._find_binary())
        self.move_secs = 20
        self.game_over = False
        self.moves = 0
        self.last_message = ""
        self.last_suggestion: Optional[tuple[int, int]] = None

        self._build_menu()
        self._build_boards()
        self._build_status_bar()

        try:
            self.client.start()
            self.client.place_smart()
        except Exception as e:
            messagebox.showerror("Sonar", f"Failed to start engine: {e}")
            sys.exit(1)

        self._refresh_status()

    def _find_binary(self) -> str:
        env = os.environ.get("SONAR_BIN")
        if env and os.path.isfile(env):
            return env
        found = shutil.which("sonar")
        if found:
            return found
        # Try a relative path (development)
        here = os.path.dirname(os.path.abspath(__file__))
        candidates = [
            os.path.join(here, "..", "target", "release", "sonar"),
            os.path.join(here, "..", "target", "debug", "sonar"),
            "/home/z/my-project/sonar/target/release/sonar",
        ]
        for c in candidates:
            if os.path.isfile(c):
                return os.path.abspath(c)
        raise FileNotFoundError("sonar binary not found. Set $SONAR_BIN or build with cargo build --release")

    def _build_menu(self):
        menubar = tk.Menu(self.root)
        menu = tk.Menu(menubar, tearoff=0)
        menu.add_command(label="New Game", command=self.new_game)
        menu.add_command(label="Set move time (5..60s)", command=self.set_move_time)
        menu.add_separator()
        menu.add_command(label="Quit", command=self.quit)
        menubar.add_cascade(label="File", menu=menu)
        self.root.config(menu=menubar)

    def _build_boards(self):
        # Two canvases: enemy (left) and ours (right)
        self.enemy_canvas = tk.Canvas(
            self.root,
            width=CELL_SIZE * 10 + BOARD_PAD,
            height=CELL_SIZE * 10 + BOARD_PAD,
            bg="white",
            highlightthickness=0,
        )
        self.enemy_canvas.grid(row=0, column=0, padx=10, pady=10)
        self.enemy_canvas.bind("<Button-1>", self.on_enemy_click)

        self.own_canvas = tk.Canvas(
            self.root,
            width=CELL_SIZE * 10 + BOARD_PAD,
            height=CELL_SIZE * 10 + BOARD_PAD,
            bg="white",
            highlightthickness=0,
        )
        self.own_canvas.grid(row=0, column=1, padx=10, pady=10)

        self._draw_board_grid(self.enemy_canvas, "Enemy board (click to fire)")
        self._draw_board_grid(self.own_canvas, "Your fleet")

    def _draw_board_grid(self, canvas: tk.Canvas, title: str):
        canvas.delete("all")
        canvas.create_text(
            BOARD_PAD + CELL_SIZE * 5, 12, text=title, font=("Arial", 11, "bold")
        )
        for c in range(10):
            x = BOARD_PAD + c * CELL_SIZE + CELL_SIZE // 2
            canvas.create_text(x, BOARD_PAD - 12, text=chr(ord("A") + c))
        for r in range(10):
            y = BOARD_PAD + r * CELL_SIZE + CELL_SIZE // 2
            canvas.create_text(BOARD_PAD // 2 - 4, y, text=str(r + 1))
        for r in range(10):
            for c in range(10):
                x = BOARD_PAD + c * CELL_SIZE
                y = BOARD_PAD + r * CELL_SIZE
                canvas.create_rectangle(
                    x, y, x + CELL_SIZE, y + CELL_SIZE,
                    outline="#888", fill="#ddf",
                )

    def _build_status_bar(self):
        self.status = tk.Label(self.root, text="", anchor="w", font=("Arial", 10))
        self.status.grid(row=1, column=0, columnspan=2, sticky="ew", padx=10, pady=5)
        self.hint = tk.Label(
            self.root,
            text="Click enemy board to fire. S = suggest. Menu → set move time.",
            anchor="w",
            font=("Arial", 9),
            fg="#555",
        )
        self.hint.grid(row=2, column=0, columnspan=2, sticky="ew", padx=10)

    def _refresh_status(self):
        self.status.config(text=f"Moves: {self.moves} | Move time: {self.move_secs}s | {self.last_message}")

    # ── game actions ────────────────────────────────────────────────────

    def new_game(self):
        self.client.reset()
        self.client.place_smart()
        self.game_over = False
        self.moves = 0
        self.last_suggestion = None
        self.last_message = "New game. Your move."
        self._draw_board_grid(self.enemy_canvas, "Enemy board (click to fire)")
        self._draw_board_grid(self.own_canvas, "Your fleet")
        self._refresh_status()

    def set_move_time(self):
        n = simpledialog.askinteger(
            "Move time", "Seconds per move (5..60):", initialvalue=self.move_secs, minvalue=5, maxvalue=60,
            parent=self.root,
        )
        if n is not None:
            self.move_secs = n
            self._refresh_status()

    def quit(self):
        self.client.stop()
        self.root.quit()

    def on_enemy_click(self, event: tk.Event):
        if self.game_over:
            return
        # Convert click to (row, col)
        c = (event.x - BOARD_PAD) // CELL_SIZE
        r = (event.y - BOARD_PAD) // CELL_SIZE
        if not (0 <= r < 10 and 0 <= c < 10):
            return
        self._fire_at(r, c)

    def _fire_at(self, r: int, c: int):
        # Ask Sonar what happens when WE fire at ITS board.
        # In this UI, the player fires at Sonar's fleet.
        # Sonar.receive_shot returns the result.
        result = self.client.receive_shot(r, c)
        # But we don't track Sonar's board separately; instead, we use snapshot.
        # For simplicity: we use the snapshot's our_shots_mask etc.
        # Actually, the protocol is symmetric — we call receive_shot on the engine
        # to apply our fire, and the engine tracks its own fleet.
        # But we need to display the result on the enemy canvas.
        self._mark_enemy_cell(r, c, result)
        self.moves += 1

        if result == "miss":
            self.last_message = f"Your shot ({chr(c+65)}{r+1}): MISS"
        elif result == "hit":
            self.last_message = f"Your shot ({chr(c+65)}{r+1}): HIT!"
        elif result == "sunk":
            self.last_message = f"Your shot ({chr(c+65)}{r+1}): SUNK!"
        else:
            self.last_message = f"Already fired at ({chr(c+65)}{r+1})"
            self._refresh_status()
            return

        # Check if Sonar is defeated (we won)
        snap = self.client.snapshot()
        if snap.get("our_sunk_mask", 0) == snap.get("our_fleet_mask", 1):
            self.game_over = True
            self.last_message = "*** YOU WIN! ***"
            self.client.record_game(True)
            self._refresh_status()
            messagebox.showinfo("Sonar", "You win!")
            return

        # Sonar fires back in a background thread (move can take time)
        threading.Thread(target=self._sonar_fire_back, daemon=True).start()
        self._refresh_status()

    def _sonar_fire_back(self):
        # Sonar chooses a move against the player's fleet.
        # In this simplified UI, the player's fleet is implicit (we just
        # generate a random result). For a real game you'd track it.
        # Here: 17 of 100 cells are ships; Sonar picks one; we randomly
        # decide hit/miss based on snapshot.
        br, bc = self.client.choose_move(self.move_secs)
        # For demo: assume all Sonar shots are misses (player's fleet
        # is abstract). Real integration would track player's fleet too.
        self.root.after(0, lambda: self._mark_own_cell(br, bc, "miss"))
        self.root.after(0, lambda: self._post_sonar_fire(br, bc))

    def _post_sonar_fire(self, br: int, bc: int):
        self.moves += 1
        self.last_message = f"Sonar fired at ({chr(bc+65)}{br+1})"
        self._refresh_status()

    def _mark_enemy_cell(self, r: int, c: int, result: str):
        x = BOARD_PAD + c * CELL_SIZE
        y = BOARD_PAD + r * CELL_SIZE
        if result == "miss":
            color = "#88a"
            text = "○"
        elif result == "hit":
            color = "#fc8"
            text = "x"
        elif result == "sunk":
            color = "#f44"
            text = "X"
        else:
            return
        self.enemy_canvas.create_rectangle(x, y, x + CELL_SIZE, y + CELL_SIZE, fill=color, outline="#444")
        self.enemy_canvas.create_text(x + CELL_SIZE // 2, y + CELL_SIZE // 2, text=text, font=("Arial", 14, "bold"))

    def _mark_own_cell(self, r: int, c: int, result: str):
        x = BOARD_PAD + c * CELL_SIZE
        y = BOARD_PAD + r * CELL_SIZE
        color = "#88a" if result == "miss" else "#fc8"
        text = "○" if result == "miss" else "x"
        self.own_canvas.create_rectangle(x, y, x + CELL_SIZE, y + CELL_SIZE, fill=color, outline="#444")
        self.own_canvas.create_text(x + CELL_SIZE // 2, y + CELL_SIZE // 2, text=text, font=("Arial", 14, "bold"))

    def suggest(self, _event=None):
        s = self.client.suggest_move(self.move_secs)
        r = s.get("row")
        c = s.get("col")
        if r is None or c is None:
            return
        self.last_suggestion = (r, c)
        self.last_message = f"Sonar suggests ({chr(c+65)}{r+1}) — confidence {s.get('confidence', 0):.2f}"
        # Highlight the cell
        self._draw_board_grid(self.enemy_canvas, "Enemy board (click to fire)")
        x = BOARD_PAD + c * CELL_SIZE
        y = BOARD_PAD + r * CELL_SIZE
        self.enemy_canvas.create_rectangle(x, y, x + CELL_SIZE, y + CELL_SIZE, outline="#0f0", width=3)
        self._refresh_status()


# ─────────────────────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────────────────────

def main():
    root = tk.Tk()
    try:
        ui = SonarUI(root)
    except FileNotFoundError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)
    root.bind("s", ui.suggest)
    root.protocol("WM_DELETE_WINDOW", ui.quit)
    root.mainloop()


if __name__ == "__main__":
    main()
