"""SonarApp — main application window.

Fullscreen, adaptive layout. Menu bar with mode selection and time
limit. Three modes: Play vs Bot, Team Mode, Benchmark.
"""
from __future__ import annotations

import tkinter as tk
from tkinter import messagebox

from .client import SonarClient, find_binary
from .settings_dlg import SettingsDialog
from .team_mode import TeamMode
from .play_mode import PlayMode
from .benchmark_mode import BenchmarkMode


class SonarApp(tk.Tk):
    """Main application window for the Sonar UI."""

    def __init__(self):
        super().__init__()
        self.title("Sonar — Battleship AI")
        self.configure(bg="#1a1a2e")

        # ── Engine ────────────────────────────────────────────────────
        try:
            self.binary = find_binary()
        except FileNotFoundError as e:
            messagebox.showerror("Sonar", str(e))
            self.destroy()
            raise

        self.client = SonarClient(self.binary)
        try:
            self.client.start()
        except Exception as e:
            messagebox.showerror("Sonar", f"Failed to start engine: {e}")
            self.destroy()
            raise

        # ── Settings ──────────────────────────────────────────────────
        self.move_secs = 20
        self.board_size = 10
        self.rules = {
            "board_size": 10,
            "ship_lengths": [5, 4, 3, 3, 2],
            "contact_rule": "NoContact",
            "sunk_rule": "RevealNeighbors",
        }

        # ── UI ────────────────────────────────────────────────────────
        self._build_menu()
        self._build_layout()
        self._go_fullscreen()

        # Default mode
        self._show_mode("team")

    def _build_menu(self):
        menubar = tk.Menu(self, bg="#1a1a2e", fg="#e0e0e0",
                          activebackground="#0066cc", activeforeground="white",
                          borderwidth=0)

        # File menu
        file_menu = tk.Menu(menubar, tearoff=0, bg="#1a1a2e", fg="#e0e0e0",
                           activebackground="#0066cc", activeforeground="white")
        file_menu.add_command(label="New Game", command=self._new_game,
                              accelerator="Ctrl+N")
        file_menu.add_command(label="Settings...", command=self._open_settings)
        file_menu.add_separator()
        file_menu.add_command(label="Quit", command=self._quit, accelerator="Ctrl+Q")
        menubar.add_cascade(label="File", menu=file_menu)

        # Mode menu
        mode_menu = tk.Menu(menubar, tearoff=0, bg="#1a1a2e", fg="#e0e0e0",
                           activebackground="#0066cc", activeforeground="white")
        mode_menu.add_command(label="Team Mode", command=lambda: self._show_mode("team"))
        mode_menu.add_command(label="Play vs Bot", command=lambda: self._show_mode("play"))
        mode_menu.add_command(label="Benchmark", command=lambda: self._show_mode("bench"))
        menubar.add_cascade(label="Mode", menu=mode_menu)

        # Help menu
        help_menu = tk.Menu(menubar, tearoff=0, bg="#1a1a2e", fg="#e0e0e0",
                           activebackground="#0066cc", activeforeground="white")
        help_menu.add_command(label="About", command=self._about)
        menubar.add_cascade(label="Help", menu=help_menu)

        self.config(menu=menubar)

        # Keyboard shortcuts
        self.bind("<Control-n>", lambda e: self._new_game())
        self.bind("<Control-q>", lambda e: self._quit())
        self.bind("<Escape>", lambda e: self._toggle_fullscreen())
        self.bind("<F11>", lambda e: self._toggle_fullscreen())

    def _build_layout(self):
        """Build the main layout — a container frame that swaps modes."""
        self.container = tk.Frame(self, bg="#1a1a2e")
        self.container.pack(fill=tk.BOTH, expand=True)

        self.modes: dict[str, tk.Frame] = {}
        self.current_mode: tk.Frame | None = None

    def _show_mode(self, name: str):
        """Switch to a mode (team / play / bench)."""
        if self.current_mode is not None:
            self.current_mode.pack_forget()

        if name not in self.modes:
            if name == "team":
                self.modes[name] = TeamMode(self.container, self)
            elif name == "play":
                self.modes[name] = PlayMode(self.container, self)
            elif name == "bench":
                self.modes[name] = BenchmarkMode(self.container, self)
            else:
                return

        self.current_mode = self.modes[name]
        self.current_mode.pack(fill=tk.BOTH, expand=True)

        # Call on_enter if available.
        if hasattr(self.current_mode, "on_enter"):
            self.current_mode.on_enter()

    def _new_game(self):
        """Start a new game in the current mode."""
        if self.current_mode and hasattr(self.current_mode, "on_enter"):
            self.current_mode.on_enter()

    def _open_settings(self):
        """Open the settings dialog."""
        dlg = SettingsDialog(self, self.move_secs, self.rules)
        self.wait_window(dlg)
        if dlg.result:
            self.move_secs = dlg.result["move_secs"]
            new_rules = dlg.result["rules"]
            if new_rules != self.rules:
                self.rules = new_rules
                self.board_size = new_rules["board_size"]
                # Apply rules to the engine.
                resp = self.client.set_rules(new_rules)
                if resp.get("ok"):
                    # Update board views in all modes.
                    for mode in self.modes.values():
                        if hasattr(mode, "enemy_board"):
                            mode.enemy_board.set_board_size(self.board_size)
                        if hasattr(mode, "own_board"):
                            mode.own_board.set_board_size(self.board_size)
                    # Restart current mode.
                    if self.current_mode and hasattr(self.current_mode, "on_enter"):
                        self.current_mode.on_enter()
                else:
                    err = resp.get("error", "unknown error")
                    messagebox.showerror("Sonar", f"Invalid rules: {err}")

    def _about(self):
        messagebox.showinfo(
            "About Sonar",
            "Sonar v0.1.0 (experimental)\n\n"
            "The world's strongest battleship AI engine.\n"
            "Pure Rust · Apache-2.0\n\n"
            "WARNING: This is an experimental version and may contain bugs.\n"
            "Use at your own risk.",
        )

    def _quit(self):
        self.client.stop()
        self.quit()

    def _go_fullscreen(self):
        """Start in fullscreen mode."""
        self.attributes("-fullscreen", True)
        self.bind("<Escape>", lambda e: self._toggle_fullscreen())

    def _toggle_fullscreen(self):
        """Toggle between fullscreen and windowed."""
        is_fs = self.attributes("-fullscreen")
        self.attributes("-fullscreen", not is_fs)
