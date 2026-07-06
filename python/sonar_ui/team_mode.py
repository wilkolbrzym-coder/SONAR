"""Team mode — you and Sonar play against a paper player.

Sonar suggests a move; you fire at the paper player's board and report
the result (miss / hit / sunk). Sonar waits for your feedback before
suggesting the next move.
"""
from __future__ import annotations

import random
import tkinter as tk
from tkinter import ttk
from typing import TYPE_CHECKING

from .board_view import BoardView, UNKNOWN, MISS, HIT, SUNK, SHIP, EMPTY, MISSED_SHOT, HIT_SHIP, SUNK_SHIP
from .play_mode import generate_player_fleet

if TYPE_CHECKING:
    from .app import SonarApp


class TeamMode(tk.Frame):
    """Team mode frame."""

    def __init__(self, parent: tk.Misc, app: "SonarApp"):
        super().__init__(parent, bg="#1a1a2e")
        self.app = app
        self.client = app.client
        self.last_suggestion: tuple[int, int] | None = None
        self.moves = 0
        self.player_ships = []

        self._build_ui()

    def _build_ui(self):
        # Top bar
        top = tk.Frame(self, bg="#1a1a2e")
        top.pack(fill=tk.X, padx=8, pady=4)

        self.title_label = tk.Label(
            top, text="Team Mode — Sonar advises, you fire at the paper board",
            bg="#1a1a2e", fg="#00ff88", font=("Arial", 13, "bold"),
        )
        self.title_label.pack(side=tk.LEFT)

        self.status_label = tk.Label(
            top, text="Ready. Click 'Suggest' to ask Sonar.",
            bg="#1a1a2e", fg="#e0e0e0", font=("Arial", 10),
        )
        self.status_label.pack(side=tk.RIGHT)

        # Two boards side by side
        boards = tk.Frame(self, bg="#1a1a2e")
        boards.pack(fill=tk.BOTH, expand=True, padx=8, pady=4)

        self.enemy_board = BoardView(
            boards, size=self.app.board_size,
            title="Paper player's board (your shots)",
            on_click=self._on_board_click,
        )
        self.enemy_board.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=4)

        self.own_board = BoardView(
            boards, size=self.app.board_size,
            title="Your fleet (click where paper player shoots)",
            show_ships=True,
            on_click=self._on_own_click,
        )
        self.own_board.pack(side=tk.RIGHT, fill=tk.BOTH, expand=True, padx=4)

        # Bottom controls
        bottom = tk.Frame(self, bg="#1a1a2e")
        bottom.pack(fill=tk.X, padx=8, pady=8)

        tk.Button(bottom, text="Suggest move", command=self._suggest,
                  bg="#0066cc", fg="white", font=("Arial", 11, "bold"),
                  padx=16, pady=6).pack(side=tk.LEFT, padx=4)

        tk.Button(bottom, text="Miss", command=lambda: self._report("miss"),
                  bg="#555", fg="white", font=("Arial", 11, "bold"),
                  padx=16, pady=6).pack(side=tk.LEFT, padx=4)

        tk.Button(bottom, text="Hit", command=lambda: self._report("hit"),
                  bg="#cc6600", fg="white", font=("Arial", 11, "bold"),
                  padx=16, pady=6).pack(side=tk.LEFT, padx=4)

        tk.Button(bottom, text="Sunk", command=self._report_sunk,
                  bg="#cc0000", fg="white", font=("Arial", 11, "bold"),
                  padx=16, pady=6).pack(side=tk.LEFT, padx=4)

        self.moves_label = tk.Label(bottom, text="Moves: 0",
                                    bg="#1a1a2e", fg="#888", font=("Arial", 10))
        self.moves_label.pack(side=tk.RIGHT, padx=8)

    def on_enter(self):
        """Called when this mode becomes active."""
        self.client.reset()
        self.client.place_smart()

        # Generate player fleet
        try:
            self.player_ships = generate_player_fleet(
                self.app.board_size,
                self.app.rules["ship_lengths"],
                self.app.rules["contact_rule"]
            )
        except Exception:
            self.player_ships = []

        self.enemy_board.clear()
        self.own_board.clear()
        self._show_own_fleet()
        self.moves = 0
        self.last_suggestion = None
        self.status_label.config(text="Ready. Click 'Suggest' to ask Sonar.",
                                  fg="#e0e0e0")
        self.moves_label.config(text="Moves: 0")

    def _show_own_fleet(self):
        """Display the player's fleet on the own board."""
        for ship in self.player_ships:
            for r, c in ship["cells"]:
                self.own_board.set_cell(r, c, SHIP)

    def _suggest(self):
        """Ask Sonar for a move suggestion."""
        self.status_label.config(text=f"Sonar is thinking ({self.app.move_secs}s)...",
                                  fg="#ffaa44")
        self.update_idletasks()

        r, c = self.client.choose_move(self.app.move_secs)
        self.last_suggestion = (r, c)
        coord = chr(ord("A") + c) + str(r + 1)
        self.status_label.config(
            text=f"Sonar suggests {coord}. Fire there, then report the result.",
            fg="#00ff88",
        )
        self.enemy_board.set_highlight((r, c))

    def _on_board_click(self, r: int, c: int):
        """Allow the user to click a cell instead of using Suggest."""
        self.last_suggestion = (r, c)
        coord = chr(ord("A") + c) + str(r + 1)
        self.status_label.config(
            text=f"Selected {coord}. Fire there, then report the result.",
            fg="#00ff88",
        )
        self.enemy_board.set_highlight((r, c))

    def _on_own_click(self, r: int, c: int):
        # The paper player shot at our board at (r, c)
        hit_ship = None
        for ship in self.player_ships:
            if (r, c) in ship["cells"]:
                hit_ship = ship
                break
        
        coord = chr(ord("A") + c) + str(r + 1)
        if hit_ship:
            hit_ship["hits"].add((r, c))
            if len(hit_ship["hits"]) == hit_ship["len"]:
                # Sunk! Turn all cells red
                for cr, cc in hit_ship["cells"]:
                    self.own_board.set_cell(cr, cc, SUNK_SHIP)
                self.status_label.config(text=f"Paper player shot {coord}: SUNK ({hit_ship['len']})", fg="#ff4444")
            else:
                self.own_board.set_cell(r, c, HIT_SHIP)
                self.status_label.config(text=f"Paper player shot {coord}: HIT", fg="#ffaa44")
        else:
            self.own_board.set_cell(r, c, MISSED_SHOT)
            self.status_label.config(text=f"Paper player shot {coord}: MISS", fg="#888")
            
        # Check if player lost
        player_lost = all(len(s["hits"]) == s["len"] for s in self.player_ships)
        if player_lost:
            self.status_label.config(text="GAME OVER — YOU LOST!", fg="#ff4444")

    def _sync_enemy_sunk(self):
        snap = self.client.snapshot()
        try:
            sunk_mask = int(snap.get("our_sunk_mask", "0"))
            shots_mask = int(snap.get("our_shots_mask", "0"))
            hits_mask = int(snap.get("our_hits_mask", "0"))
        except (ValueError, TypeError):
            sunk_mask, shots_mask, hits_mask = 0, 0, 0
        for r in range(self.app.board_size):
            for c in range(self.app.board_size):
                bit = 1 << (r * 10 + c)
                if sunk_mask & bit:
                    self.enemy_board.set_cell(r, c, SUNK)
                elif hits_mask & bit:
                    self.enemy_board.set_cell(r, c, HIT)
                elif shots_mask & bit:
                    self.enemy_board.set_cell(r, c, MISS)

    def _report(self, result: str):
        """Report the result of the last fired shot."""
        if self.last_suggestion is None:
            self.status_label.config(text="No shot selected. Click 'Suggest' first.",
                                      fg="#ff4444")
            return
        r, c = self.last_suggestion

        # Tell the engine about the result.
        self.client.observe(r, c, result)

        # Update the board display.
        if result == "miss":
            self.enemy_board.set_cell(r, c, MISS)
        elif result == "hit":
            self.enemy_board.set_cell(r, c, HIT)
        elif result.startswith("sunk"):
            self.enemy_board.set_cell(r, c, SUNK)
            self._sync_enemy_sunk()

        self.moves += 1
        self.moves_label.config(text=f"Moves: {self.moves}")
        self.last_suggestion = None
        self.enemy_board.set_highlight(None)

        self.status_label.config(
            text=f"Recorded {result.upper()} at {chr(ord('A')+c)}{r+1}. Click 'Suggest' for next move.",
            fg="#00ff88",
        )

    def _report_sunk(self):
        """Report a sunk ship — ask for the ship length."""
        dlg = tk.Toplevel(self)
        dlg.title("Sunk ship length")
        dlg.transient(self)
        dlg.grab_set()
        dlg.configure(bg="#1a1a2e")

        tk.Label(dlg, text="What length was the sunk ship?",
                 bg="#1a1a2e", fg="#e0e0e0",
                 font=("Arial", 11)).pack(padx=16, pady=8)

        var = tk.IntVar(value=2)
        box = ttk.Combobox(dlg, textvariable=var,
                           values=[2, 3, 4, 5], width=5, state="readonly")
        box.pack(padx=16, pady=4)

        def _ok():
            dlg.destroy()
            self._report(f"sunk_{var.get()}")

        tk.Button(dlg, text="OK", command=_ok,
                  bg="#cc0000", fg="white", font=("Arial", 10, "bold"),
                  padx=16, pady=4).pack(pady=8)
