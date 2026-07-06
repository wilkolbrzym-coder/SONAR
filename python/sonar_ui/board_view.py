"""BoardView — a Tkinter canvas widget that renders a battleship board.

Adaptive sizing: the board fills its allocated space while keeping
square cells.
"""
from __future__ import annotations

import tkinter as tk
from typing import Callable, Optional


# Cell states (match the Rust Cell enum).
UNKNOWN = 0
MISS = 1
HIT = 2
SUNK = 3

SHIP = 10
EMPTY = 11
MISSED_SHOT = 12
HIT_SHIP = 13
SUNK_SHIP = 14

# Colors for each state.
COLORS = {
    UNKNOWN: "#b0c4de",
    MISS: "#6a7b8a",
    HIT: "#ffaa44",
    SUNK: "#ff4444",
    SHIP: "#4477aa",
    EMPTY: "#b0c4de",
    MISSED_SHOT: "#6a7b8a",
    HIT_SHIP: "#ffaa44",
    SUNK_SHIP: "#ff4444",
}

SYMBOLS = {
    UNKNOWN: "",
    MISS: "o",
    HIT: "x",
    SUNK: "X",
    SHIP: "#",
    EMPTY: "",
    MISSED_SHOT: "o",
    HIT_SHIP: "x",
    SUNK_SHIP: "X",
}


class BoardView(tk.Canvas):
    """A square battleship board that adapts to its container size.

    Args:
        parent:   Tk parent widget.
        size:     Board dimension (e.g. 10 for a 10x10 board).
        title:    Title displayed above the board.
        on_click: Callback ``(row, col)`` called on left-click.
        show_ships: If True, render ship cells (for the player's own board).
    """

    def __init__(
        self,
        parent: tk.Misc,
        size: int = 10,
        title: str = "",
        on_click: Optional[Callable[[int, int], None]] = None,
        show_ships: bool = False,
        **kwargs,
    ):
        super().__init__(parent, bg="#1a1a2e", highlightthickness=0, **kwargs)
        self.board_size = size
        self.title = title
        self.on_click = on_click
        self.show_ships = show_ships
        self.cells: list[list[int]] = [[UNKNOWN] * size for _ in range(size)]
        self.highlight: Optional[tuple[int, int]] = None
        self._cell_px = 30  # updated on resize

        self.bind("<Button-1>", self._handle_click)
        self.bind("<Configure>", self._handle_resize)

    # ── public API ────────────────────────────────────────────────────

    def set_cell(self, r: int, c: int, state: int):
        """Update a single cell's state and redraw."""
        if 0 <= r < self.board_size and 0 <= c < self.board_size:
            self.cells[r][c] = state
            self._draw()

    def set_all(self, states: list[list[int]]):
        """Set the entire board state at once."""
        self.cells = states
        self._draw()

    def set_highlight(self, cell: Optional[tuple[int, int]]):
        """Highlight a cell (e.g. Sonar's suggestion)."""
        self.highlight = cell
        self._draw()

    def clear(self):
        """Reset all cells to unknown."""
        self.cells = [[UNKNOWN] * self.board_size for _ in range(self.board_size)]
        self.highlight = None
        self._draw()

    def set_board_size(self, size: int):
        """Change the board dimension."""
        self.board_size = size
        self.clear()

    # ── event handlers ────────────────────────────────────────────────

    def _handle_click(self, event: tk.Event):
        if self.on_click is None:
            return
        pad = self._pad()
        th = self._title_h()
        offset_x = 24
        offset_y = 20
        cx = (event.x - pad - offset_x) // self._cell_px
        cy = (event.y - pad - th - offset_y) // self._cell_px
        if 0 <= cx < self.board_size and 0 <= cy < self.board_size:
            self.on_click(cy, cx)

    def _handle_resize(self, _event: tk.Event):
        self._draw()

    # ── rendering ─────────────────────────────────────────────────────

    def _pad(self) -> int:
        return 4

    def _title_h(self) -> int:
        return 24 if self.title else 4

    def _draw(self):
        self.delete("all")
        w = self.winfo_width()
        h = self.winfo_height()
        if w < 10 or h < 10:
            return

        pad = self._pad()
        th = self._title_h()
        offset_x = 24
        offset_y = 20

        avail_w = w - 2 * pad - offset_x
        avail_h = h - 2 * pad - th - offset_y
        cell = max(8, min(avail_w, avail_h) // self.board_size)
        self._cell_px = cell

        # Title
        if self.title:
            self.create_text(
                w // 2, th // 2 + 2,
                text=self.title,
                fill="#e0e0e0",
                font=("Arial", 11, "bold"),
            )

        # Column letters and row numbers
        for c in range(self.board_size):
            x = pad + offset_x + c * cell + cell // 2
            y = pad + th + offset_y // 2
            self.create_text(x, y, text=chr(ord("A") + c),
                             fill="#00ff88", font=("Arial", 10, "bold"))
        for r in range(self.board_size):
            x = pad + offset_x // 2
            y = pad + th + offset_y + r * cell + cell // 2
            self.create_text(x, y, text=str(r + 1),
                             fill="#00ff88", font=("Arial", 10, "bold"))

        # Cells
        for r in range(self.board_size):
            for c in range(self.board_size):
                x = pad + offset_x + c * cell
                y = pad + th + offset_y + r * cell
                state = self.cells[r][c]
                color = COLORS.get(state, "#888")
                self.create_rectangle(x, y, x + cell, y + cell,
                                      fill=color, outline="#333")
                sym = SYMBOLS.get(state, "")
                if sym:
                    self.create_text(x + cell // 2, y + cell // 2,
                                     text=sym, font=("Arial", max(8, cell // 2), "bold"),
                                     fill="#1a1a2e")

        # Highlight
        if self.highlight:
            hr, hc = self.highlight
            x = pad + offset_x + hc * cell
            y = pad + th + offset_y + hr * cell
            self.create_rectangle(x, y, x + cell, y + cell,
                                  outline="#00ff00", width=3)
