"""Sonar UI — a Python overlay for the Sonar battleship engine.

This is a thin GUI layer that talks to the Sonar Rust engine via the
JSON IPC protocol (`sonar serve`). No game logic lives here — every
decision is made by the engine.

Modules:
    client       — JSON IPC client (SonarClient)
    app          — main application window (fullscreen, adaptive)
    board_view   — board rendering widget
    menus        — menu bar with time selection and modes
    play_mode    — play vs bot mode (bot places ships for player)
    team_mode    — team mode (Sonar advises, user reports results)
    benchmark    — benchmark mode
    settings_dlg — settings dialog (time limit 5..60s)
"""
