"""Settings dialog — move time limit (5..60s) and game rules."""
from __future__ import annotations

import tkinter as tk
from tkinter import ttk


class SettingsDialog(tk.Toplevel):
    """Modal dialog for adjusting engine settings."""

    def __init__(self, parent: tk.Misc, move_secs: int, rules: dict):
        super().__init__(parent)
        self.title("Settings")
        self.transient(parent)
        self.grab_set()

        self.move_secs = move_secs
        self.rules = rules
        self.result: dict | None = None

        self._build_ui()
        self.protocol("WM_DELETE_WINDOW", self._cancel)

    def _build_ui(self):
        self.configure(bg="#1a1a2e")
        pad = 12

        # ── Move time ────────────────────────────────────────────────
        tk.Label(self, text="Move time limit (seconds):",
                 bg="#1a1a2e", fg="#e0e0e0",
                 font=("Arial", 11, "bold")).grid(row=0, column=0, columnspan=2,
                                                  padx=pad, pady=(pad, 4), sticky="w")

        self.time_var = tk.IntVar(value=self.move_secs)
        self.time_scale = tk.Scale(
            self, from_=5, to=60, orient=tk.HORIZONTAL,
            variable=self.time_var, length=300,
            bg="#1a1a2e", fg="#e0e0e0", highlightthickness=0,
            troughcolor="#333", font=("Arial", 9),
        )
        self.time_scale.grid(row=1, column=0, columnspan=2,
                             padx=pad, pady=4, sticky="ew")

        self.time_label = tk.Label(self, text=f"{self.move_secs}s",
                                   bg="#1a1a2e", fg="#00ff88",
                                   font=("Arial", 12, "bold"))
        self.time_label.grid(row=1, column=2, padx=pad, pady=4)
        self.time_var.trace_add("write", self._update_time_label)

        # ── Buttons ──────────────────────────────────────────────────
        btn_frame = tk.Frame(self, bg="#1a1a2e")
        btn_frame.grid(row=2, column=0, columnspan=3, padx=pad, pady=pad)

        tk.Button(btn_frame, text="OK", command=self._ok,
                  bg="#00884e", fg="white", font=("Arial", 10, "bold"),
                  padx=20, pady=4).pack(side=tk.LEFT, padx=4)
        tk.Button(btn_frame, text="Cancel", command=self._cancel,
                  bg="#555", fg="white", font=("Arial", 10),
                  padx=20, pady=4).pack(side=tk.LEFT, padx=4)

    def _update_time_label(self, *_args):
        self.time_label.config(text=f"{self.time_var.get()}s")

    def _ok(self):
        self.result = {
            "move_secs": self.time_var.get(),
            "rules": self.rules,
        }
        self.destroy()

    def _cancel(self):
        self.result = None
        self.destroy()
