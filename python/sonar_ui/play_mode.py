"""Play vs Bot mode — you play against Sonar.

Sonar places its own fleet. You also get a fleet placed by the engine
(so you don't need to place it manually). You fire at Sonar's board;
Sonar fires at yours. First to sink all enemy ships wins.
"""
from __future__ import annotations

import random
import threading
import tkinter as tk
from typing import TYPE_CHECKING

from .board_view import BoardView, UNKNOWN, MISS, HIT, SUNK, SHIP, EMPTY, MISSED_SHOT, HIT_SHIP, SUNK_SHIP

if TYPE_CHECKING:
    from .app import SonarApp


def generate_player_fleet(board_size, ship_lengths, contact_rule):
    ships = []
    board = [[0] * board_size for _ in range(board_size)]
    
    for length in ship_lengths:
        placed = False
        for attempt in range(1000):
            horiz = random.choice([True, False])
            if horiz:
                r = random.randint(0, board_size - 1)
                c = random.randint(0, board_size - length)
                cells = [(r, c + i) for i in range(length)]
            else:
                r = random.randint(0, board_size - length)
                c = random.randint(0, board_size - 1)
                cells = [(r + i, c) for i in range(length)]
            
            ok = True
            for cr, cc in cells:
                if board[cr][cc] != 0:
                    ok = False
                    break
                
                # Check neighbors
                if contact_rule == "NoContact":
                    for dr in [-1, 0, 1]:
                        for dc in [-1, 0, 1]:
                            if dr == 0 and dc == 0:
                                continue
                            nr, nc = cr + dr, cc + dc
                            if 0 <= nr < board_size and 0 <= nc < board_size:
                                if board[nr][nc] != 0:
                                    ok = False
                                    break
                        if not ok:
                            break
                elif contact_rule == "AllowCornerContact":
                    for dr, dc in [(-1, 0), (1, 0), (0, -1), (0, 1)]:
                        nr, nc = cr + dr, cc + dc
                        if 0 <= nr < board_size and 0 <= nc < board_size:
                            if board[nr][nc] != 0:
                                ok = False
                                break
                if not ok:
                    break
            
            if ok:
                for cr, cc in cells:
                    board[cr][cc] = length
                ships.append({
                    "len": length,
                    "cells": cells,
                    "hits": set()
                })
                placed = True
                break
        if not placed:
            raise ValueError("Failed to place player fleet")
    return ships


class PlayMode(tk.Frame):
    """Play vs Sonar frame."""

    def __init__(self, parent: tk.Misc, app: "SonarApp"):
        super().__init__(parent, bg="#1a1a2e")
        self.app = app
        self.client = app.client
        self.thinking = False
        self.game_over = False
        self.moves = 0
        self.player_ships = []

        self._build_ui()

    def _build_ui(self):
        # Top bar
        top = tk.Frame(self, bg="#1a1a2e")
        top.pack(fill=tk.X, padx=8, pady=4)

        tk.Label(top, text="Play vs Sonar",
                 bg="#1a1a2e", fg="#00ff88", font=("Arial", 13, "bold")).pack(side=tk.LEFT)

        self.status_label = tk.Label(
            top, text="Click Sonar's board to fire.",
            bg="#1a1a2e", fg="#e0e0e0", font=("Arial", 10),
        )
        self.status_label.pack(side=tk.RIGHT)

        # Two boards side by side
        boards = tk.Frame(self, bg="#1a1a2e")
        boards.pack(fill=tk.BOTH, expand=True, padx=8, pady=4)

        self.enemy_board = BoardView(
            boards, size=self.app.board_size,
            title="Sonar's fleet (click to fire)",
            on_click=self._on_enemy_click,
        )
        self.enemy_board.pack(side=tk.LEFT, fill=tk.BOTH, expand=True, padx=4)

        self.own_board = BoardView(
            boards, size=self.app.board_size,
            title="Your fleet",
            show_ships=True,
        )
        self.own_board.pack(side=tk.RIGHT, fill=tk.BOTH, expand=True, padx=4)

        # Bottom
        bottom = tk.Frame(self, bg="#1a1a2e")
        bottom.pack(fill=tk.X, padx=8, pady=8)

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
        except Exception as e:
            self.player_ships = []

        self.enemy_board.clear()
        self.own_board.clear()
        self._show_own_fleet()
        self.thinking = False
        self.game_over = False
        self.moves = 0
        self.status_label.config(text="Click Sonar's board to fire.",
                                  fg="#e0e0e0")
        self.moves_label.config(text="Moves: 0")

    def _show_own_fleet(self):
        """Display the player's fleet on the own board."""
        for ship in self.player_ships:
            for r, c in ship["cells"]:
                self.own_board.set_cell(r, c, SHIP)

    def _on_enemy_click(self, r: int, c: int):
        if self.game_over or self.thinking:
            return
        # Ask the engine what happens when we fire at (r, c).
        result = self.client.receive_shot(r, c)
        self._mark_enemy(r, c, result)
        self.moves += 1

        coord = chr(ord("A") + c) + str(r + 1)
        if result == "miss":
            self.status_label.config(text=f"Your shot {coord}: MISS", fg="#888")
        elif result == "hit":
            self.status_label.config(text=f"Your shot {coord}: HIT!", fg="#ffaa44")
        elif result == "sunk":
            self.status_label.config(text=f"Your shot {coord}: SUNK!", fg="#ff4444")
            self._sync_enemy_sunk()
        else:
            self.status_label.config(text=f"Already fired at {coord}", fg="#888")
            self.moves_label.config(text=f"Moves: {self.moves}")
            return

        # Check if Sonar is defeated (we won).
        snap = self.client.snapshot()
        fleet_str = snap.get("our_fleet_mask", "0")
        sunk_str = snap.get("our_sunk_mask", "0")
        try:
            fleet = int(fleet_str)
            sunk = int(sunk_str)
        except (ValueError, TypeError):
            fleet, sunk = 0, 0
        if fleet != 0 and (sunk & fleet) == fleet:
            self.game_over = True
            self.status_label.config(text="YOU WIN!", fg="#00ff88")
            self.client.record_game(True)
            return

        # Sonar fires back in a background thread.
        self.status_label.config(text=f"Sonar is thinking ({self.app.move_secs}s)...",
                                  fg="#ffaa44")
        self.update_idletasks()
        self.thinking = True
        threading.Thread(target=self._sonar_fire, daemon=True).start()
        self.moves_label.config(text=f"Moves: {self.moves}")

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

    def _sonar_fire(self):
        """Sonar chooses a move and fires at the player's fleet."""
        br, bc = self.client.choose_move(self.app.move_secs)
        
        # Check hit/miss/sunk on player's ships
        hit_ship = None
        for ship in self.player_ships:
            if (br, bc) in ship["cells"]:
                hit_ship = ship
                break
        
        if hit_ship:
            hit_ship["hits"].add((br, bc))
            if len(hit_ship["hits"]) == hit_ship["len"]:
                result = f"sunk_{hit_ship['len']}"
                display_result = "sunk"
                # Mark all cells of this ship as sunk!
                for cr, cc in hit_ship["cells"]:
                    self.after(0, lambda cr=cr, cc=cc: self.own_board.set_cell(cr, cc, SUNK_SHIP))
            else:
                result = "hit"
                display_result = "hit"
                self.after(0, lambda: self._mark_own(br, bc, display_result))
        else:
            result = "miss"
            display_result = "miss"
            self.after(0, lambda: self._mark_own(br, bc, display_result))
            
        # Tell the engine about the result
        self.client.observe(br, bc, result)
        self.after(0, lambda: self._post_sonar_fire(br, bc, display_result))

    def _post_sonar_fire(self, br: int, bc: int, result: str):
        coord = chr(ord("A") + bc) + str(br + 1)
        self.moves += 1
        self.thinking = False
        
        # Check if player lost
        player_lost = all(len(s["hits"]) == s["len"] for s in self.player_ships)
        if player_lost:
            self.game_over = True
            self.status_label.config(text="SONAR WINS!", fg="#ff4444")
            self.client.record_game(False)
            return
            
        self.status_label.config(text=f"Sonar fired at {coord} ({result.upper()}). Your turn.",
                                  fg="#e0e0e0")
        self.moves_label.config(text=f"Moves: {self.moves}")

    def _mark_enemy(self, r: int, c: int, result: str):
        if result == "miss":
            self.enemy_board.set_cell(r, c, MISS)
        elif result == "hit":
            self.enemy_board.set_cell(r, c, HIT)
        elif result == "sunk":
            self.enemy_board.set_cell(r, c, SUNK)

    def _mark_own(self, r: int, c: int, result: str):
        if result == "miss":
            self.own_board.set_cell(r, c, MISSED_SHOT)
        elif result == "hit":
            self.own_board.set_cell(r, c, HIT_SHIP)
        elif result == "sunk":
            self.own_board.set_cell(r, c, SUNK_SHIP)
