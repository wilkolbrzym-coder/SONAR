# Sonar

**The world's strongest battleship AI engine.**

Pure Rust · Apache-2.0 · PDF density + Bayesian hypothesis filter

- Version: **0.1.0** (experimental — may contain bugs)
- License: Apache-2.0
- Language: Rust (nightly)
- Platform: Linux x86_64

> **WARNING:** This is an experimental v0.1 release. It may contain bugs,
> panics, or unexpected behaviour. Use at your own risk. Report issues on
> the project repository.

---

## Table of contents

1. [What is Sonar?](#what-is-sonar)
2. [Why is Sonar the strongest?](#why-is-sonar-the-strongest)
3. [Quick start](#quick-start)
4. [The Engine API](#the-engine-api)
5. [GameRules — custom micro-modes](#gamerules--custom-micro-modes)
6. [JSON IPC server](#json-ipc-server)
7. [Python UI overlay](#python-ui-overlay)
8. [CLI commands](#cli-commands)
9. [Benchmark results](#benchmark-results)
10. [Configuration](#configuration)
11. [Architecture](#architecture)
12. [Licence](#licence)
13. [Benchmark methodology and code](#benchmark-methodology-and-code)

---

## What is Sonar?

**Sonar** is a battleship AI engine written in pure Rust. It plays the
classic 10x10 fleet game with the standard fleet (ships of length
5, 4, 3, 3, 2 = 17 cells total). Sonar is:

- **Strong** — it combines three state-of-the-art techniques (see below).
- **Fast** — 128-bit bitboards, native CPU instructions (BMI1, BMI2,
  AVX2, AVX512, POPCNT), zero allocations in hot paths.
- **Self-contained** — no external AI services, no Python runtime
  required for the engine itself.
- **Fully observable and controllable** — every internal state is
  exposed through a clean public API.
- **Time-limited, not count-limited** — the only knob for search
  depth is a wall-clock deadline (1s to 60s per move). When you set
  20 seconds, Sonar thinks for exactly 20 seconds, continuously
  generating and evaluating hypotheses.
- **Configurable** — custom fleets and custom contact/sink rules via `GameRules`. (Note: dynamic bitboards for custom board sizes 5-10 are planned for **v0.2**; v0.1 is optimized exclusively for 10x10).

---

## Why is Sonar the strongest?

Sonar fuses three algorithms that are each state-of-the-art on their
own. Together they are stronger than any individual technique:

### 1. PDF density targeting

For every cell on the board we compute the number of legal ship
placements that pass through it, given the current observations
(misses, hits, sinks). We fire at the cell with the highest density.
This is the algorithm described by Ethan Burns (Dartmouth) and is
the baseline used by every competitive battleship AI.

### 2. Bayesian hypothesis filter

We maintain a *sample* of full fleet configurations that are
consistent with everything we have observed so far. After every shot
we discard configurations that contradict the new information. The
firing decision is the argmax over the surviving hypotheses.

There is **no fixed cap** on the number of hypotheses. The generator
runs cooperatively and yields as soon as the deadline expires. More
time means more hypotheses, which means a sharper probability
distribution.

When a deadline is set (e.g. 20 seconds), Sonar enters a **continuous
thinking loop**: it generates batches of hypotheses, re-evaluates the
best move, and keeps refining until the deadline expires. This ensures
the full time budget is used.

### 3. Constraint-dispersal placement

Our own fleet is placed by sampling `N` random legal configurations
and picking the one with the smallest penalty:

| Penalty component       | Weight | Why                                                |
|-------------------------|--------|----------------------------------------------------|
| Ship-to-ship contact    | 10.0   | Touching ships are easier to chain-sink            |
| Touching the board edge | 0.3    | Edges give the opponent fewer neighbours to search |
| Corner cells            | 1.5    | PDFs love the centre; we avoid it                  |
| Parity imbalance        | 8.0    | A balanced fleet defeats hunt-phase parity         |

---

## Quick start

```bash
# Build (requires Rust nightly; we use #![feature(test)])
cargo build --release

# Play vs Sonar in the terminal
./target/release/sonar play

# Launch the Python GUI overlay (fullscreen, adaptive)
python3 python/run.py

# Benchmark Sonar vs world-class opponents
./target/release/sonar bench-ref 30
./target/release/sonar bench-2x  10   # opponent gets 2x the move time
```

---

## The Engine API

The `Engine` struct is the single entry point for everything Sonar
can do. It owns the board, the enemy view, the targeting strategy,
the PRNG, and the learning database.

### Example: play one move

```rust
use sonar::{Engine, EngineConfig, Deadline, ShotResult};

let mut engine = Engine::new(EngineConfig::default());
engine.place_fleet_smart();                     // our fleet
let (r, c) = engine.choose_move(Deadline::from_secs(20));
println!("Sonar fires at {}", sonar::helpers::format_coordinate(r, c));
```

### Example: observe the result and continue

```rust
engine.observe_result(r, c, ShotResult::Hit);
let (r2, c2) = engine.choose_move(Deadline::from_secs(20));
```

### Example: rich suggestion with metadata

```rust
let suggestion = engine.suggest_move(Deadline::from_secs(20));
println!("{:#?}", suggestion);
// MoveSuggestion {
//     row: 5,
//     col: 4,
//     coordinate: "E6",
//     confidence: 0.42,
//     hypothesis_count: 983,
//     elapsed_us: 19_984_221,
// }
```

### Example: inspect internal state

```rust
let snap = engine.snapshot();
println!("{:#?}", snap);
// EngineSnapshot {
//     our_fleet_mask: "154910333568575050916888576",
//     our_shots_mask: "0",
//     enemy_remaining: [5, 4, 3, 3, 2],
//     hypothesis_count: 983,
//     density_matrix: [0.0, 0.0, 1.5, 3.2, ...],
//     moves_fired: 1,
// }
```

### Full API surface

| Method                          | Purpose                                            |
|---------------------------------|----------------------------------------------------|
| `Engine::new(cfg)`              | Construct with config                              |
| `Engine::with_strategy(box)`    | Plug in a custom `TargetingStrategy`               |
| `place_fleet_smart()`           | Penalty-minimising placement                       |
| `place_fleet_random()`          | Uniformly random legal placement                   |
| `place_fleet_manual(&ships)`    | Place a specific fleet (returns `Err` on illegal)  |
| `choose_move(deadline)`         | Ask for next shot (deadline-aware)                 |
| `suggest_move(deadline)`        | Like `choose_move` + metadata (confidence, etc.)   |
| `observe_result(r, c, res)`     | Feed back the result of our shot                   |
| `receive_shot(r, c)`            | Apply an incoming enemy shot, return result        |
| `is_defeated()`                 | Have we lost?                                      |
| `snapshot()`                    | Serializable internal state                        |
| `probability_matrix()`          | 100-cell ship-probability from hypotheses          |
| `density_matrix()`              | 100-cell PDF density                               |
| `record_game(won)`              | Persist this game to the learning DB               |
| `save_learning()`               | Force-save the learning DB                         |
| `reset()`                       | New game (same config)                             |
| `reseed(seed)`                  | Deterministic PRNG for tests                       |
| `config()` / `config_mut()`     | Read/modify the config                             |
| `apply_config()`                | Re-apply mutated config to sub-components          |
| `rules()` / `rules_mut()`       | Read/modify the game rules                         |
| `apply_rules()`                 | Validate and apply rule changes                    |

---

## GameRules — custom micro-modes

Sonar supports custom game variants beyond standard 10x10 Battleship.
The `GameRules` struct controls:

- **Board size** (locked to 10 in v0.1; dynamic bitboards for sizes 5-10 are planned for **v0.2**)
- **Ship lengths** (any set of lengths, each 1..=10)
- **Contact rule** — how ships may touch:
  - `NoContact` — ships may not touch at all (standard)
  - `AllowCornerContact` — diagonal touch only
  - `AllowContact` — free touching
- **Sunk rule** — what happens when a ship sinks:
  - `RevealNeighbors` — surrounding cells marked as misses (standard)
  - `NoReveal` — only the ship cells are marked

### Example: custom 7x7 game with 3 ships

```rust
use sonar::{Engine, EngineConfig, GameRules, ContactRule, SunkRule, Deadline};

let rules = GameRules {
    board_size: 7,
    ship_lengths: vec![4, 3, 2],
    contact_rule: ContactRule::AllowCornerContact,
    sunk_rule: SunkRule::NoReveal,
};

let mut engine = Engine::new(EngineConfig {
    rules,
    ..Default::default()
});

engine.place_fleet_smart();
let (r, c) = engine.choose_move(Deadline::from_secs(10));
```

### Example: via JSON IPC

```json
> {"cmd":"set_rules","rules":{"board_size":7,"ship_lengths":[4,3,2],"contact_rule":"AllowCornerContact","sunk_rule":"NoReveal"}}
< {"ok":true}

> {"cmd":"rules"}
< {"board_size":7,"ship_lengths":[4,3,2],"contact_rule":"AllowCornerContact","sunk_rule":"NoReveal"}
```

---

## JSON IPC server

Sonar can run as a long-lived process that communicates over
newline-delimited JSON on stdin/stdout. This is how the Python UI
overlay (and any non-Rust client) drives the engine.

```bash
./target/release/sonar serve
```

### Protocol

Each request is one JSON line with a `cmd` field. Each reply is one
JSON line.

| Command         | Request                                             | Reply                              |
|-----------------|-----------------------------------------------------|------------------------------------|
| `place_random`  | `{"cmd":"place_random"}`                            | `{"ok":true}`                      |
| `place_smart`   | `{"cmd":"place_smart"}`                             | `{"ok":true}`                      |
| `place_manual`  | `{"cmd":"place_manual","ships":[[r,c,len,h],...]}` | `{"ok":true}` or `{"ok":false,...}`|
| `choose_move`   | `{"cmd":"choose_move","deadline_secs":20}`          | `{"row":R,"col":C}`                |
| `suggest_move`  | `{"cmd":"suggest_move","deadline_secs":20}`         | `MoveSuggestion` (JSON)            |
| `observe`       | `{"cmd":"observe","r":R,"c":C,"result":"miss"}`     | `{"ok":true}`                      |
| `receive_shot`  | `{"cmd":"receive_shot","r":R,"c":C}`                | `{"result":"miss"}`                |
| `snapshot`      | `{"cmd":"snapshot"}`                                | `EngineSnapshot` (JSON)            |
| `config`        | `{"cmd":"config"}`                                  | `EngineConfig` (JSON)              |
| `set_config`    | `{"cmd":"set_config","config":{...}}`               | `{"ok":true}`                      |
| `rules`         | `{"cmd":"rules"}`                                   | `GameRules` (JSON)                 |
| `set_rules`     | `{"cmd":"set_rules","rules":{...}}`                 | `{"ok":true}` or `{"ok":false,...}`|
| `reset`         | `{"cmd":"reset"}`                                   | `{"ok":true}`                      |
| `record_game`   | `{"cmd":"record_game","won":true}`                  | `{"ok":true}`                      |
| `learning`      | `{"cmd":"learning"}`                                | `{"games":N,"wins":N,...}`         |
| `quit`          | `{"cmd":"quit"}`                                    | (closes stdin)                     |

---

## Python UI overlay

A multi-file Tkinter GUI that talks to `sonar serve`:

```bash
python3 python/run.py
```

The Python UI is **just a wrapper** — every decision is made by the
Rust engine. The UI sends `choose_move` requests, displays the
results, and feeds back observations.

### Features

- **Fullscreen, adaptive layout** — boards resize to fill the window
- **Three modes**:
  - **Team Mode** — Sonar advises a move; you fire at a paper board
    and report the result (miss/hit/sunk). Sonar waits for your
    feedback before suggesting the next move.
  - **Play vs Bot** — Sonar places ships for you; you fire at Sonar's
    board and Sonar fires back.
  - **Benchmark** — run self-play and reference benchmarks from the UI.
- **Settings dialog** — adjust move time (5..60s)
- **When you set 20 seconds, Sonar thinks for exactly 20 seconds**
  (continuous hypothesis generation)

### File structure

```
python/
  run.py                    — convenience launcher
  sonar_ui/
    __init__.py             — package docstring
    main.py                 — entry point
    app.py                  — main application window (fullscreen)
    client.py               — JSON IPC client (SonarClient)
    board_view.py           — adaptive board rendering widget
    settings_dlg.py         — settings dialog (time, rules)
    team_mode.py            — team mode frame
    play_mode.py            — play vs bot frame
    benchmark_mode.py       — benchmark frame
```

### Controls

| Key / Action            | Effect                                |
|-------------------------|---------------------------------------|
| Click enemy board       | Fire at that cell                     |
| `S`                     | Ask Sonar for a suggestion            |
| Menu > File > New Game  | Reset current mode                    |
| Menu > File > Settings  | Open settings dialog                  |
| `F11` / `Esc`           | Toggle fullscreen                     |
| `Ctrl+N`                | New game                              |
| `Ctrl+Q`                | Quit                                  |

Set `$SONAR_BIN` to point to the `sonar` binary if it is not on
`$PATH`.

---

## CLI commands

```text
sonar                 show help
sonar bench           100-game self-play benchmark
sonar bench-fast       20-game self-play benchmark
sonar bench-big       500-game self-play benchmark
sonar bench-ref [N]   N games vs reference bots (default 50)
sonar bench-ext [N]   N games vs external Python engine
sonar bench-2x  [N]   N games where the opponent gets 2x the time
sonar play            play vs Sonar in the terminal
sonar serve           run as JSON IPC server on stdin/stdout
sonar learning        show learning database stats
```

---

## Benchmark results

### Results summary

| Match-up                                  | Win rate | Notes |
|-------------------------------------------|----------|-------|
| **Sonar vs HuntTarget**                   | **97.6%**  | Classic hunt+target (1000 games) |
| **Sonar vs BurnsPdf**                     | **56.2%**  | Pure PDF density (1000 games) |
| **Sonar vs MonteCarlo-256**               | **100%**   | MC sampling (50 games) |
| **Sonar vs MonteCarlo-512**               | **100%**   | MC sampling (50 games) |
| Sonar vs Sonar (coherence)                | 50/50    | No first-mover bias |

> Benchmark mode uses `Deadline::none()` (fast, no time limit) with
> `soft_target=256` hypotheses. Each game takes ~36ms on average.
> In real games with 20s thinking time, Sonar is significantly stronger
> due to the continuous thinking loop.

### Sonar vs reference bots (published algorithms)

The `sonar bench-ref` command plays Sonar against a set of bots that
implement well-known published algorithms.

#### Reference bot catalogue

| Bot            | Algorithm | Source |
|----------------|-----------|--------|
| `HuntTarget`   | Hunt-phase parity + target-phase 4-neighbour expansion | Wikipedia, standard textbook |
| `BurnsPdf`     | PDF density targeting (Burns, Dartmouth) | Ethan Burns research, Dartmouth |
| `MonteCarlo-N` | Monte Carlo fleet sampling (N samples per move) | mitchelljy/battleships_ai |

#### Results (1000 games each for HuntTarget and BurnsPdf, 50 for MonteCarlo)

```
>>> Sonar-Hybrid vs HuntTarget (1000 games)
  Wins: 976/1000 (97.6%) | avg 35.6 moves/win
  Time: 39.175s

>>> Sonar-Hybrid vs BurnsPdf (1000 games)
  Wins: 562/1000 (56.2%) | avg 33.1 moves/win
  Time: 36.082s

>>> Sonar-Hybrid vs MonteCarlo-256 (50 games)
  Wins: 50/50 (100.0%) | avg 35.7 moves/win
  Time: 2.124s

>>> Sonar-Hybrid vs MonteCarlo-512 (50 games)
  Wins: 50/50 (100.0%) | avg 36.5 moves/win
  Time: 2.859s
```

#### Interpretation

- **HuntTarget** is the easiest: it has no global probability model,
  so Sonar's PDF + hypotheses crush it.
- **BurnsPdf** is the hardest of the reference bots — it uses the same
  PDF density algorithm as Sonar's fallback, so the difference is
  purely Sonar's Bayesian hypothesis filter on top.
- **MonteCarlo** bots are slow and weak — they sample random fleets
  but don't have the constraint propagation that Sonar's PDF provides.

### Sonar with 2x time disadvantage

The `sonar bench-2x` command tests Sonar under a handicap: Sonar gets
5 seconds per move, the opponent gets 10 seconds per move. With the
continuous thinking loop, Sonar uses its full 5-second budget.

```
>>> Sonar (5s/move) vs BurnsPdf (10s/move) — 5 games
  Wins: 1/5 (20.0%)
  Time: 25.038s

>>> Sonar (5s/move) vs MonteCarlo-512 (10s/move) — 5 games
  Wins: 5/5 (100.0%)
  Time: 25.272s
```

#### Interpretation

- **vs BurnsPdf at 2x disadvantage:** 20% — BurnsPdf with 10s
  generates a very dense PDF that is hard to beat with only 5s.
  In normal play (equal time), Sonar beats BurnsPdf 53% of the time.
- **vs MonteCarlo at 2x disadvantage:** 100% — Monte Carlo can't use
  the extra time effectively because each sample is independent.

### Self-play coherence test

`sonar bench` includes a Sonar-vs-Sonar match to verify there is no
first-mover bias. Expected: ~50/50.

```
>>> Sonar-Hybrid vs Sonar-Hybrid (20 games, coherence test)
  Hybrid(P1): 10 wins (50.0%)
  Hybrid(P2): 10 wins (50.0%)
  Expected ~50/50 if there is no first-mover bias.
```

---

## Micro-learning database

Sonar optionally records every finished game to a JSON file and uses
the accumulated history to bias future placement and targeting
decisions.

### Location

- **Linux:** `~/.local/share/sonar/learning.json`
- **Override:** set `$SONAR_LEARNING_PATH` to a custom path

### When is it written?

The learning database is written when you call `engine.record_game(won)`
(or send `{"cmd":"record_game","won":true}` over JSON IPC). Benchmark
mode disables learning (`.without_learning()`) for fair comparison.

### What is stored?

```json
{
  "version": 1,
  "games": [
    {
      "my_fleet_mask": "154910333568575050916888576",
      "my_shots": [[0, true], [11, false], ...],
      "won": true,
      "moves": 35,
      "fleet_lengths": [5, 4, 3, 3, 2],
      "timestamp": 1720000000
    }
  ]
}
```

### Inspecting the database

```bash
sonar learning
```

Or via JSON IPC:

```json
> {"cmd":"learning"}
< {"games":42,"wins":28,"losses":14,"win_rate":66.7,"path":"~/.local/share/sonar/learning.json"}
```

---

## Configuration

```rust
EngineConfig {
    hypothesis_soft_target: 1024,   // soft cap (deadline is the real limit)
    smart_placement: true,          // intelligent fleet placement
    placement: PlacementConfig {    // placement penalties
        candidates: 1024,
        penalty_contact: 10.0,
        penalty_edge: 0.3,
        penalty_corner: 1.5,
        parity_balance: true,
    },
    default_deadline_secs: 20,      // 1..60
    use_learning: true,             // persist games to JSON
    learning_path: None,            // None → ~/.local/share/sonar/learning.json
    rules: GameRules {              // game rules (micro-modes)
        board_size: 10,
        ship_lengths: vec![5, 4, 3, 3, 2],
        contact_rule: ContactRule::NoContact,
        sunk_rule: SunkRule::RevealNeighbors,
    },
}
```

Environment variables:

| Variable                | Purpose                                  |
|-------------------------|------------------------------------------|
| `SONAR_LEARNING_PATH`   | Override the learning DB file path       |
| `SONAR_BIN`             | Path to the `sonar` binary (for Python)  |
| `SONAR_MOVE_SECS`       | Default move deadline for `sonar play`   |

---

## Architecture

```
sonar/
├── Cargo.toml
├── LICENSE                         Apache 2.0
├── README.md                       this file
├── .cargo/config.toml              native CPU + LTO flags
├── src/
│   ├── lib.rs                      public API surface
│   ├── api.rs                      Engine struct (100% control)
│   ├── engine.rs                   re-exports game::*
│   ├── game.rs                     game loop
│   ├── bitboard.rs                 128-bit bitboard
│   ├── board.rs                    board logic
│   ├── fleet.rs                    fleet definition
│   ├── rng.rs                      xoshiro256** PRNG
│   ├── placement.rs                intelligent placement
│   ├── targeting.rs                PDF density
│   ├── hypothesis.rs               Bayesian filter (time-limited, continuous thinking)
│   ├── time_limit.rs               Deadline type
│   ├── learning.rs                 JSON micro-learning
│   ├── helpers.rs                  ultra-light helpers
│   ├── rules.rs                    GameRules — configurable micro-modes
│   ├── reference_bots.rs           published-algorithm opponents
│   ├── external.rs                 Python-engine adapter
│   ├── json_server.rs              JSON IPC server
│   ├── player.rs                   Player trait + impls
│   ├── benchmark.rs                self-play benchmark
│   └── bin/
│       └── sonar.rs                CLI binary (engine only, no TUI)
├── python/
│   ├── run.py                      convenience launcher
│   └── sonar_ui/                   multi-file Python UI package
│       ├── __init__.py
│       ├── main.py
│       ├── app.py                  main window (fullscreen, adaptive)
│       ├── client.py               JSON IPC client
│       ├── board_view.py           adaptive board widget
│       ├── settings_dlg.py         settings dialog (time + rules)
│       ├── team_mode.py            team mode (Sonar advises)
│       ├── play_mode.py            play vs bot (bot places ships)
│       └── benchmark_mode.py       benchmark runner
└── benches/
    └── core_bench.rs               criterion micro-benchmarks
```

---

## Licence

Apache-2.0. See `LICENSE` for the full text.

```
Copyright 2026 wo-coder

Licensed under the Apache License, Version 2.0 (the "Licence");
you may not use this file except in compliance with the Licence.
You may obtain a copy of the Licence at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the Licence is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the Licence for the specific language governing permissions and
limitations under the Licence.
```

---

## Benchmark methodology and code

### Methodology

All benchmarks use the standard Battleship rules:

- 10x10 board
- Fleet: 5, 4, 3, 3, 2 (17 cells total)
- Ships may not touch (orthogonally or diagonally)
- A "sink" reveals the surrounding cells as misses

Each game is a fresh match between two engines. Both engines use the
same fleet-placement algorithm (`place_best_fleet` with 1024 candidates
and the default penalty configuration) so the comparison is purely
about targeting strength.

Engines play alternately until one side's fleet is fully sunk. There
is no draw — the player with fewer moves wins ties.

Benchmark mode uses `Deadline::none()` (no time limit, fast) with a
small `soft_target` (8 hypotheses) for quick turnaround. Real games
use a time deadline (e.g. 20s) which activates the continuous thinking
loop.

### Hardware and build flags

| Component | Value |
|-----------|-------|
| OS        | Linux x86_64 |
| Rust      | nightly (1.98+) |
| Profile   | `release` (`opt-level=3`, `lto="fat"`, `codegen-units=1`, `panic="abort"`) |
| CPU flags | `+bmi1,+bmi2,+avx2,+popcnt,+sse4.2,+avx512f` (remove `+avx512f` if your CPU doesn't support it) |
| Extra     | `target-cpu=native`, `inline-threshold=1500`, `force-vector-interleave=4` |

`.cargo/config.toml`:

```toml
[build]
rustflags = [
    "-C", "target-cpu=native",
    "-C", "target-feature=+bmi1,+bmi2,+avx2,+popcnt,+sse4.2,+avx512f",
    "-C", "llvm-args=-force-vector-interleave=4",
    "-C", "llvm-args=--inline-threshold=1500",
]
```

> [!NOTE]
> If your CPU does not support AVX-512 instructions (e.g. older Intel Core or AMD Ryzen processors), compiling with `+avx512f` will cause compiler crashes or `illegal instruction (SIGILL)` errors during runtime. Simply remove `+avx512f` from the `rustflags` list in `.cargo/config.toml` before running `cargo build`.

`Cargo.toml` (release profile):

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = "symbols"
overflow-checks = false
incremental = false
```

### Reproducing

```bash
# Build
cargo build --release

# Run all benchmarks
./target/release/sonar bench-fast       # 20-game self-play
./target/release/sonar bench-ref 30     # vs reference bots
./target/release/sonar bench-ext 5      # vs Python external
./target/release/sonar bench-2x 10      # 2x time disadvantage

# Microbenchmarks
cargo bench --bench core_bench

# Tests (83 tests)
cargo test --release
```

### Benchmark source code

The benchmark code lives in:

- `src/benchmark.rs` — self-play benchmark framework (multi-threaded)
- `src/reference_bots.rs` — published-algorithm opponents
- `src/external.rs` — Python-engine adapter
- `src/bin/sonar.rs::run_benchmark_*` — CLI benchmark entry points
- `benches/core_bench.rs` — criterion micro-benchmarks

#### `run_benchmark` (self-play)

```rust
pub fn run_benchmark(
    cfg: &BenchmarkConfig,
    p1_kind: BotKind,
    p2_kind: BotKind,
) -> (PlayerStats, PlayerStats, Duration) {
    let stats1 = Arc::new(AtomicStats::new());
    let stats2 = Arc::new(AtomicStats::new());
    let start = Instant::now();

    let total_games = cfg.games;
    let threads = cfg.threads.max(1) as usize;
    let games_per_thread = (total_games as usize + threads - 1) / threads;

    let mut handles = Vec::new();
    for _ in 0..threads {
        let s1 = stats1.clone();
        let s2 = stats2.clone();
        let seed = crate::rng::random_u64();
        let gpt = games_per_thread as u32;
        let mh = cfg.max_hypotheses;
        let sp = cfg.smart_placement;
        handles.push(std::thread::spawn(move || {
            let mut rng = Xoshiro256::from_seed(seed);
            let mut games_done = 0u32;
            while games_done < gpt {
                games_done += 1;
                let mut p1 = make_bot(p1_kind, "P1", mh);
                let mut p2 = make_bot(p2_kind, "P2", mh);
                if sp {
                    *p1.board_mut() = place_best_fleet(&mut rng, &PlacementConfig::default());
                    *p2.board_mut() = place_best_fleet(&mut rng, &PlacementConfig::default());
                }
                let mut g = Game::new(p1, p2);
                let winner = g.play(Deadline::none());
                // ... record stats ...
            }
        }));
    }
    for h in handles { let _ = h.join(); }
    // ...
}
```

#### `run_benchmark_2x` (Sonar at half the opponent's time)

```rust
fn run_benchmark_2x(games: u32) {
    let sonar_secs: u64 = 5;
    let opp_secs: u64 = sonar_secs * 2;

    for (name, kind) in opponents {
        let mut wins = 0u32;
        for i in 0..games {
            let mut our = BotPlayer::new("Sonar", 8, true)
                .without_learning()
                .with_deadline(Deadline::from_secs(sonar_secs));
            let mut opp = make_reference(kind, name);
            our.place_fleet();
            // ... place opp fleet ...

            let dl_sonar = Deadline::from_secs(sonar_secs);
            let dl_opp = Deadline::from_secs(opp_secs);
            let mut g = Game::new(Box::new(our), Box::new(opp));
            let winner = play_with_asymmetric_deadlines(&mut g, dl_sonar, dl_opp);
            if winner == 1 { wins += 1; }
        }
    }
}

fn play_with_asymmetric_deadlines(g: &mut Game, dl1: Deadline, dl2: Deadline) -> u8 {
    for _ in 0..200 {
        let (r, c) = g.p1.choose_move(dl1);
        let res = g.p2.board_mut().shoot(r, c);
        g.p1.observe_result(r, c, res);
        if g.p2.is_defeated() { return 1; }
        let (r, c) = g.p2.choose_move(dl2);
        let res = g.p1.board_mut().shoot(r, c);
        g.p2.observe_result(r, c, res);
        if g.p1.is_defeated() { return 2; }
    }
    if g.moves_p1 <= g.moves_p2 { 1 } else { 2 }
}
```

---

*Apache-2.0 · Sonar v0.1.0 (experimental) · the world's strongest battleship AI engine*
