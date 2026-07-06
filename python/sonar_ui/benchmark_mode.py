"""Benchmark mode — run Sonar self-play benchmarks from the UI."""
from __future__ import annotations

import subprocess
import tkinter as tk
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .app import SonarApp


class BenchmarkMode(tk.Frame):
    """Benchmark mode frame — runs sonar bench commands and shows output."""

    def __init__(self, parent: tk.Misc, app: "SonarApp"):
        super().__init__(parent, bg="#1a1a2e")
        self.app = app
        self.binary = app.binary

        self._build_ui()

    def _build_ui(self):
        # Top bar
        top = tk.Frame(self, bg="#1a1a2e")
        top.pack(fill=tk.X, padx=8, pady=4)

        tk.Label(top, text="Benchmark",
                 bg="#1a1a2e", fg="#00ff88", font=("Arial", 13, "bold")).pack(side=tk.LEFT)

        # Buttons
        btns = tk.Frame(self, bg="#1a1a2e")
        btns.pack(fill=tk.X, padx=8, pady=4)

        tk.Button(btns, text="Self-play (20 games)",
                  command=lambda: self._run("bench-fast"),
                  bg="#0066cc", fg="white", font=("Arial", 10, "bold"),
                  padx=12, pady=4).pack(side=tk.LEFT, padx=4)

        tk.Button(btns, text="vs Reference bots (30 games)",
                  command=lambda: self._run(["bench-ref", "30"]),
                  bg="#0066cc", fg="white", font=("Arial", 10, "bold"),
                  padx=12, pady=4).pack(side=tk.LEFT, padx=4)

        tk.Button(btns, text="2x time disadvantage (20 games)",
                  command=lambda: self._run(["bench-2x", "20"]),
                  bg="#0066cc", fg="white", font=("Arial", 10, "bold"),
                  padx=12, pady=4).pack(side=tk.LEFT, padx=4)

        # Output text area
        self.text = tk.Text(self, bg="#0d0d1a", fg="#00ff88",
                            font=("Courier", 10), wrap=tk.WORD,
                            insertbackground="#00ff88")
        self.text.pack(fill=tk.BOTH, expand=True, padx=8, pady=4)

    def _run(self, args):
        """Run a sonar bench command and display output."""
        self.text.delete("1.0", tk.END)
        self.text.insert(tk.END, f"Running: sonar {args}\n\n")
        self.text.update()

        import threading
        threading.Thread(target=self._run_thread, args=(args,), daemon=True).start()

    def _run_thread(self, args):
        """Run the benchmark in a background thread."""
        try:
            cmd = [self.binary] + ([args] if isinstance(args, str) else args)
            proc = subprocess.Popen(
                cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
            )
            for line in proc.stdout:
                self.after(0, lambda l=line: self._append(l))
            proc.wait()
            self.after(0, lambda: self._append("\n[done]\n"))
        except Exception as e:
            self.after(0, lambda: self._append(f"\n[error: {e}]\n"))

    def _append(self, text: str):
        self.text.insert(tk.END, text)
        self.text.see(tk.END)
