# Sonar

**The world's strongest battleship AI engine.**

[![CI](https://github.com/wilkolbrzym-coder/SONAR/actions/workflows/ci.yml/badge.svg)](https://github.com/wilkolbrzym-coder/SONAR/actions/workflows/ci.yml)
[![Pages](https://github.com/wilkolbrzym-coder/SONAR/actions/workflows/pages.yml/badge.svg)](https://github.com/wilkolbrzym-coder/SONAR/actions/workflows/pages.yml)

Pure Rust · Apache-2.0 · PDF density + Bayesian hypothesis filter · WebAssembly

- Version: **0.2.0-beta.1** (beta — see the [stability contract](STABILITY.md))
- Language: Rust (stable — 1.85+ required, developed and tested on 1.98.1)
- Platform: Linux/macOS/Windows (native) + every modern browser (WebAssembly)
- Tests: **122 Rust tests** + web verification suite, all green in CI

> **0.2.0-beta ends the experimental era.** The 0.1.0 "may contain bugs or
> panics" disclaimer is replaced by a written, continuously-tested
> [stability contract](STABILITY.md): no reachable panics, bit-identical
> deterministic replays, adversarially robust by construction, protocol v1
> frozen.

---

## Table of contents

1. [What is Sonar?](#what-is-sonar)
2. [Why is Sonar the strongest?](#why-is-sonar-the-strongest)
3. [Quick start](#quick-start)
4. [The web app (GitHub Pages)](#the-web-app-github-pages)
5. [Modding — 100% control from your browser](#modding--100-control-from-your-browser)
6. [The Engine API tour](#the-engine-api-tour)
7. [GameRules — custom micro-modes](#gamerules--custom-micro-modes)
8. [The JSON protocol (v1)](#the-json-protocol-v1)
9. [CLI reference](#cli-reference)
10. [Benchmarks](#benchmarks)
11. [Testing](#testing)
12. [Adversarial robustness](#adversarial-robustness)
13. [Architecture](#architecture)
14. [Repository layout](#repository-layout)
15. [Building from source](#building-from-source)
16. [What changed from 0.1.0](#what-changed-from-010)
17. [Roadmap to 1.0](#roadmap-to-10)
18. [License](#license)

---

## What is Sonar?

**Sonar** is a battleship AI engine written in pure Rust. It plays the
classic 10×10 fleet game with the standard fleet (ships of length
5, 4, 3, 3, 2 — 17 cells total), and it plays it at state-of-the-art
strength by combining three techniques:

1. **PDF density targeting** — for every cell on the board, Sonar computes
   the number of legal ship placements that pass through it given *all*
   observations so far (misses, hits, sinks). It fires at the densest cell.
   This is the exact-in-model computation: one pass over the placement
   table, no sampling error.

2. **Bayesian hypothesis filtering** — Sonar maintains a sample of complete
   fleet configurations consistent with every observation. Each shot
   discards inconsistent hypotheses; the surviving set forms a posterior
   probability per cell. Theoretically sharper than the PDF (it models the
   mutual exclusion between ships) but noisy at small budgets — so the
   hybrid **blends** the two with an adaptive weight `n/(n+K)` (measured:
   never worse than PDF at low budgets, +6 to +12 percentage points at
   512–1024+ hypotheses).

3. **Constraint-dispersal placement** — Sonar's own fleet is placed by
   random sampling with a penalty score (ship-to-ship contact, edge/corner
   exposure, parity balance). The final fleet is drawn **uniformly from a
   near-optimal ε-band** rather than taking the argmin — a mixed strategy
   that keeps the defensive quality while making the placement
   statistically hard to fingerprint.

Sonar is fully self-contained: no external AI services, no Python runtime,
no network calls, no telemetry. The same Rust code compiles natively
(CLI + library) and to WebAssembly (the browser app below).

## Why is Sonar the strongest?

Because every claim is **measured, seeded, and reproducible**:

| Claim | Evidence (see [BENCHMARKS.md](BENCHMARKS.md)) |
|---|---|
| Beats the classic algorithms overwhelmingly | vs HuntTarget **90.0%** (60 games), vs MonteCarlo-256/512 **100%** |
| The hypothesis filter adds real strength | +6–12 pp over pure PDF at 512–1024 hypotheses, scaling with budget |
| Beats a strong PDF opponent | 56.7–65% vs BurnsPdf-style reference (budget-dependent, documented) |
| The algorithm — not compute — carries the advantage | Opponent with **2× the time**: Sonar still wins; opponent with **2× the hypotheses**: +10 pp, not a blowout |
| Fair game loop | Identical bots split 48/52 (CI contains 50%) — no first-mover structural bias |
| Invincible in your browser | 20/20 vs random in the WASM engine; the browser test tab re-verifies the engine live |

And because it is **adversarially robust by construction** — an opponent
that collects a million games and trains a model on them cannot find a
behavioral fingerprint to exploit, because the code has none (details in
[Adversarial robustness](#adversarial-robustness)).

## Quick start

```bash
# Build (stable Rust, no nightly needed since 0.2.0)
cargo build --release

# Play against the engine in the terminal
./target/release/sonar play

# Run the full test suite (122 tests)
cargo test --release

# Benchmarks with Wilson 95% confidence intervals
./target/release/sonar bench
```

Or in ~30 seconds without any Rust toolchain — in your browser: the
[web app](#the-web-app-github-pages) below runs the identical engine
compiled to WebAssembly.

### Drive the engine from any language

```bash
./target/release/sonar serve
```

then speak newline-delimited JSON on stdin/stdout:

```json
{"cmd":"new_game","seed":42}
{"cmd":"place_smart"}
{"cmd":"choose_move","deadline_secs":20}
{"cmd":"receive_shot","r":5,"c":6}
{"cmd":"observe","r":5,"c":6,"result":"sunk_3"}
{"cmd":"snapshot"}
```

Full command reference: [the JSON protocol](#the-json-protocol-v1).

## The web app (GitHub Pages)

The `web/` directory is a complete, dependency-free single-page
application — plain HTML/CSS/JS, no build step — that runs the full
Sonar engine in your browser via WebAssembly. **Everything happens
locally on your machine**: the engine, the docs search, the test suite,
the mod runtime. No servers, no accounts, no telemetry.

| Tab | What it does |
|---|---|
| **Play** | Full battleship vs Sonar: 5 difficulty levels (= hypothesis budgets), manual ship placement editor with live legality preview, auto-place, move log, and a live **heatmap of how Sonar sees your fleet** (its full Bayesian posterior, exposed). |
| **Team** | Advisor mode for playing a real-world/paper opponent: enter the results of your physical shots, Sonar suggests the next move with confidence, hypothesis count and timing. |
| **Mods** | A complete modding environment: JavaScript code editor, example mods, and an **arena** that plays your mod against the engine with win rates, Wilson CIs and error logs. |
| **Test** | Runs the verification suite **live in your browser** against the WASM engine: protocol contract, determinism replays, self-play invariants, strength gates, hostile-input robustness. The same suite runs in CI via Node. |
| **Docs** | The full documentation with a client-side search engine (TF-scored, prefix-matching, highlighted snippets) — zero network requests. |

### Deploying to GitHub Pages

The repository ships a ready workflow (`.github/workflows/pages.yml`):
on every push to `main` it rebuilds the WASM engine from source, runs the
web verification suite, and publishes `web/` to GitHub Pages. Enable Pages
(Repo → Settings → Pages → Source: **GitHub Actions**) once — after that
every push deploys automatically.

To run locally:

```bash
./scripts/build-wasm.sh           # builds wasm → web/engine.wasm
cd web && python3 -m http.server 8123
# open http://localhost:8123
```

The app is also a PWA (installable, works offline — the engine and docs
are cached by the service worker).

## Modding — 100% control from your browser

Sonar's mod API exposes **everything the engine itself sees**. A mod is a
JavaScript object with up to four hooks, written directly in the app's
Mods tab and executed inside a Web Worker (full performance, the UI never
blocks):

```js
const mod = {
  name: "Posterior Sniper",
  version: "1.0",

  // Optional: place YOUR fleet. Return [{r, c, len, horizontal}, ...]
  // or null for a random legal fleet.
  placeFleet(api) {
    return api.legalFleet(myFleet) ? myFleet : api.randomFleet();
  },

  // Required: pick the next shot at the enemy fleet.
  chooseMove(api) {
    // api.shots / api.hits / api.sunk / api.activeHits — Uint8Array(100)
    //   masks of the fleet you are attacking (your own observations).
    // api.remaining — surviving enemy ship lengths.
    // api.density — Sonar's PDF density matrix (Float32Array(100)).
    // api.probability — Sonar's Bayesian posterior (Float32Array(100)).
    // api.hypothesisCount, api.history, api.moveNumber,
    // api.valid(r,c), api.argmax(matrix), api.rand().
    let best = 0;
    for (let i = 0; i < 100; i++) {
      if (!api.shots[i] && api.probability[i] > api.probability[best]) best = i;
    }
    return { r: Math.floor(best / 10), c: best % 10 };
  },

  // Optional: the enemy fired at YOUR fleet at (r, c).
  onObserve(api, r, c, result) {},
  onGameEnd(api, won, moves) {},
};
```

**Fair play is enforced**: an illegal move (out of bounds, already fired,
malformed return) is auto-corrected to a random legal cell and counted as
a strike; three strikes forfeit the game. Exceptions surface in the arena
report. The arena plays N games of your mod vs the engine at a chosen
difficulty and reports win rates with Wilson 95% intervals.

The repo ships four example mods (`web/mods.js`): *Parity Hunter*
(classic baseline), *Density Rider* (rides the PDF matrix), *Posterior
Sniper* (blends posterior + density — the strongest example), and *Edge
Ghost* (sneaky border placement + checkerboard hunt).

## The Engine API tour

```rust
use sonar::{Engine, EngineConfig, Deadline, ShotResult};

let mut engine = Engine::new(EngineConfig {
    use_learning: false,             // passive statistics only
    hypothesis_soft_target: 1024,    // the strength knob
    default_deadline_secs: 20,       // competitive pondering time
    ..Default::default()
});

// Place our fleet — Sonar's ε-band mixed strategy (or place manually).
engine.place_fleet_smart();

// Ask for a move (deadline-limited), fire, and feed the result back.
let (r, c) = engine.choose_move(Deadline::from_secs(20));
let result: ShotResult = opponent_board.shoot(r, c);
engine.observe_result(r, c, result);

// Full observability — 100% of the engine's internal state:
let snap = engine.snapshot();        // masks, matrices, hypothesis count
let prob = engine.probability_matrix(); // Bayesian posterior per cell
let dens = engine.density_matrix();     // PDF density per cell
let hyps = engine.hypothesis_count();   // live filter size

// Determinism: same seed + work-limited search ⇒ bit-identical replay.
engine.reseed(42);
let (r, c) = engine.choose_move(Deadline::none());
```

Highlights:

* **Pluggable strategy**: implement `TargetingStrategy` (`choose`,
  `observe`, `reset`, `stats`) and install it with `with_strategy`.
* **Custom rules**: `EngineConfig.rules` (see below).
* **No hidden state**: `reset()` restores byte-identical first-move
  behaviour (tested); recorded games are passive statistics that never
  influence play.

## GameRules — custom micro-modes

```rust
use sonar::{GameRules, ContactRule, SunkRule};

let rules = GameRules {
    board_size: 7,                    // 5..=10
    ship_lengths: vec![4, 3, 2],      // any composition (validated)
    contact_rule: ContactRule::AllowCornerContact,
    sunk_rule: SunkRule::NoReveal,    // neighbours not auto-revealed
};
```

`ContactRule`: `NoContact` (standard — ships never touch), `AllowCornerContact`
(diagonal touching allowed), `AllowContact` (free touching).
`SunkRule`: `RevealNeighbors` (standard — the sink reveals surrounding
water) or `NoReveal` (harder — no free information on a sink).
Every combination is validated (`GameRules::validate`) and exercised by
generated rule-matrix tests.

## The JSON protocol (v1)

One request per line, one reply per line. **Frozen**: existing commands
never change; evolution is additive-only.

| Command | Request | Reply |
|---|---|---|
| `version` | `{"cmd":"version"}` | `{"name","version","channel","protocol","language","features"}` |
| `new_game` | `{"cmd":"new_game","seed":S,"config":{…}?}` | `{"ok":true,"seed":S}` |
| `place_random` | `{"cmd":"place_random"}` | `{"ok":true}` |
| `place_smart` | `{"cmd":"place_smart"}` | `{"ok":true}` |
| `place_manual` | `{"cmd":"place_manual","ships":[[r,c,len,h],…]}` | `{"ok":true}` or `{"ok":false,"error","bad_index"}` |
| `choose_move` | `{"cmd":"choose_move","deadline_secs":N}` | `{"row":R,"col":C}` |
| `suggest_move` | `{"cmd":"suggest_move","deadline_secs":N}` | `MoveSuggestion` (row, col, coordinate, confidence, hypothesis_count, elapsed_us) |
| `receive_shot` | `{"cmd":"receive_shot","r":R,"c":C}` | `{"result":"miss"\|"hit"\|"sunk","len":L?}` |
| `observe` | `{"cmd":"observe","r":R,"c":C,"result":"miss"\|"hit"\|"sunk_L"}` | `{"ok":true}` |
| `snapshot` | `{"cmd":"snapshot"}` | `EngineSnapshot` (masks, remaining, matrices, counts) |
| `probability` | `{"cmd":"probability"}` | `{"matrix":[100 floats],"hypothesis_count":N}` |
| `density` | `{"cmd":"density"}` | `{"matrix":[100 floats]}` |
| `config` / `set_config` | `{"cmd":"set_config","config":{…}}` | config echo / `{"ok":true}` |
| `rules` / `set_rules` | `{"cmd":"set_rules","rules":{…}}` | rules echo / `{"ok":true}` |
| `reseed` | `{"cmd":"reseed","seed":S}` | `{"ok":true}` |
| `reset` | `{"cmd":"reset"}` | `{"ok":true}` |
| `record_game` | `{"cmd":"record_game","won":B}` | `{"ok":true}` |
| `learning` | `{"cmd":"learning"}` | statistics summary |
| `bench` | `{"cmd":"bench","games":N,"opponent":"random"\|"pdf"\|"self","soft_target":N,"seed":S}` | benchmark report with Wilson CIs |
| `quit` | `{"cmd":"quit"}` | closes the stream (stdio transport) |

Notes:

* The `len` field on `receive_shot` replies is the additive v1.1 extension
  (0.1.0 lost the sunk length on the wire, which made wire-driven games
  strictly weaker; clients should observe `"sunk_<len>"`).
* `deadline_secs: 0` selects the fast work-limited path (used by the
  browser, where wall-clock deadlines do not exist).
* **Hostile input policy**: every malformed line receives a JSON error
  reply; the engine never panics and remains fully usable afterwards
  (continuously tested with a garbage battery).

## CLI reference

```
sonar                        show help
sonar version                version, channel, protocol, license
sonar play                   play vs Sonar in the terminal ($SONAR_MOVE_SECS)
sonar serve                  JSON IPC server on stdin/stdout
sonar learning               game statistics database (passive)
sonar bench            [N]   self-play benchmark, Wilson CIs (default 100)
sonar bench-fast       [N]   quick 20-game benchmark
sonar bench-big        [N]   500-game benchmark
sonar bench-ref        [N]   vs published reference bots (default 50)
sonar bench-2x         [N]   time-handicap: opponent gets 2× the move time
sonar bench-half       [N]   compute-handicap: opponent gets 2× the hypotheses
```

## Benchmarks

Full methodology, environment, raw tables and re-run commands:
**[BENCHMARKS.md](BENCHMARKS.md)**. Headlines (1024 hypotheses, 100/60-game
runs, Wilson 95% CIs):

* vs Random: **100.0%** [96.3, 100.0]
* vs HuntTarget: **90.0%** · vs MonteCarlo-256/512: **100%**
* vs strong PDF reference: **56.7–65%** (budget-scaling documented)
* Full game (hybrid vs random): **~44 ms** · PDF density: **~5 µs** ·
  hypothesis regen 256: **~73 µs** (criterion micro-benchmarks)
* Deterministic seeds: the same seed replays the same games bit-for-bit.

## Testing

Sonar is verified by 122 Rust tests plus a browser/Node web suite —
six layers, all green in CI:

| Layer | Files | What it proves |
|---|---|---|
| Unit | inline `#[cfg(test)]` in every module | each component in isolation |
| Determinism | `tests/determinism.rs` | same seed ⇒ bit-identical games; `reset() == fresh engine`; benchmark seed purity |
| Invariants | `tests/invariants.rs` | 5 invariants checked after *every move* of 30 full games: no repeat shots, no shots at known misses, no shots at sunk neighbourhoods, games always terminate, board bookkeeping consistent |
| Property | `tests/property.rs` | 8 randomised properties × 500 cases (custom harness, no deps): fleet legality, ship-mask geometry, view consistency, rules validation soundness, bitboard closure |
| Protocol | `tests/protocol.rs` | full games over the wire, golden response shapes, a hostile garbage battery that must never crash the engine |
| Statistics | `tests/strength.rs`, `tests/placement_statistics.rs` | strength gates with Wilson CIs (regression alarms), placement bias bounds, two-sample distribution stability |
| Web | `scripts/test-web.mjs`, `scripts/test-suite.mjs`, `scripts/test-mods.mjs` | the WASM engine end-to-end, the browser test suite in Node, and the mod runtime — the same code the site runs |

Run everything: `./sonar-forge.sh` (build → test → bench → wasm → web).

## Adversarial robustness

Threat model: an attacker collects thousands of games and trains a model
(RL agent / classifier) to exploit statistical regularities.

* **Targeting is exploitable only if it deviates from the Bayes-optimal
  posterior.** Sonar's targeting is a pure function of the public
  observation sequence — no opponent identity, no history, no learned
  bias. There is nothing to reverse-engineer. *Tested*: `reset()` must
  restore first-move behaviour byte-for-byte; fresh engines with equal
  observations produce equal distributions.
* **Placement is a genuine policy and can leak a fingerprint — so it is a
  bounded mixed strategy.** The 0.1.0 argmin placement was catastrophically
  fingerprintable: all 36 border cells at **0.000 occupancy** (a free
  "never shoot here" map for the attacker). The 0.2.0 ε-band draw keeps
  every cell inside a measured band (0.057–0.204 at default ε=4) with
  strictly non-touching ships. *Tested*: per-cell occupancy gates,
  orientation balance 45–55%, two-sample stability.
* **No cross-game state leaks.** The 0.1.0 "micro-learning" bias (past
  games nudging future decisions) was **removed** — it was a textbook
  exploit surface. The statistics database is passive: records never
  influence play.

## Architecture

```
┌───────────────────────────────────────────────────────────────┐
│                      sonar (Engine API)                        │
│  ┌──────────┐  ┌───────────────┐  ┌───────────────────────┐  │
│  │  Board   │  │ TargetingStrat │  │    Placement          │  │
│  │ u128     │  │ ┌───────────┐ │  │  ε-band sampling      │  │
│  │ bitboards│  │ │ PDF       │ │  │  (GHOST FLEET v2)     │  │
│  │          │  │ │ posterior │ │  └───────────────────────┘  │
│  │          │  │ │ blend     │ │  ┌───────────────────────┐  │
│  │          │  │ └───────────┘ │  │  Xoshiro256** PRNG    │  │
│  │          │  └───────────────┘  └───────────────────────┘  │
├───────────────────────────────────────────────────────────────┤
│            json_server (protocol v1 — one truth)              │
├──────────────────────┬────────────────────────────────────────┤
│   CLI (play/serve/   │   sonar-wasm (C-ABI → WebAssembly)     │
│   bench/…)           │   → web app: game, mods, tests, docs   │
└──────────────────────┴────────────────────────────────────────┘
```

Key design decisions:

* **One protocol, every surface.** The CLI stdio server, the WASM exports,
  and the integration tests all funnel through the same
  `json_server::handle_line` — a protocol bug fixed once is fixed
  everywhere.
* **Work-limited by default in the browser.** WASM has no trustworthy wall
  clock; there `Deadline` degenerates to the hypothesis budget, keeping
  moves fast, deterministic, and platform-independent.
* **Zero dependencies in the hot path.** serde/serde_json for the
  protocol; no rand, no rayon — the PRNG, bitboards, and statistics are
  hand-rolled and property-tested.
* **Panic-free by lint.** `clippy::unwrap_used`/`expect_used` denied;
  `unsafe` only at the audited WASM FFI boundary.

## Repository layout

```
sonar/
├── crates/
│   ├── sonar-core/           # the engine library + `sonar` CLI
│   │   ├── src/              # api, targeting, hypothesis, placement,
│   │   │                     # board, bitboard, benchmark, json_server, …
│   │   ├── benches/          # criterion micro-benchmarks
│   │   └── tests/            # determinism, invariants, property,
│   │                         # protocol, strength, placement statistics
│   └── sonar-wasm/           # C-ABI cdylib → sonar_wasm.wasm
├── web/                      # the GitHub Pages app (no build step)
│   ├── index.html, style.css, app.js
│   ├── worker.js             # engine host (Web Worker)
│   ├── engine.js             # WASM glue (browser + Node)
│   ├── mods.js               # mod runtime + example mods
│   ├── tests.js              # the in-browser test suite
│   ├── docs-data.js          # documentation content
│   ├── search.js             # client-side search engine
│   ├── sw.js, manifest.json  # PWA / offline
│   └── engine.wasm           # committed build (CI rebuilds on deploy)
├── scripts/                  # build-wasm.sh, test-web/test-suite/test-mods.mjs
├── .github/workflows/        # ci.yml (gate), pages.yml (deploy)
├── sonar-forge.sh            # one-command build+test+bench orchestrator
├── BENCHMARKS.md             # measured performance (reproducible)
└── STABILITY.md              # the beta stability contract
```

## Building from source

Requirements: stable Rust ≥ 1.85 (tested on 1.98.1). No nightly.

```bash
cargo build --release            # native library + CLI
cargo test --release             # 122 tests
cargo bench                      # criterion micro-benchmarks
./target/release/sonar bench     # strength benchmarks (Wilson CIs)

rustup target add wasm32-unknown-unknown
./scripts/build-wasm.sh          # wasm → web/engine.wasm
node scripts/test-web.mjs        # WASM E2E (needs Node ≥ 18)

./sonar-forge.sh                 # everything above in one command
```

The default build is **portable** (no `target-cpu=native` — removed in
0.2.0). For a single-machine maximum build:
`RUSTFLAGS="-C target-cpu=native" cargo build --release`.

A `release-checked` profile (overflow checks + unwinding) is provided for
auditing and fuzzing: `cargo test --release-checked`.

## What changed from 0.1.0

**Removed**

* The entire Python UI (`python/`) — the web app replaces it and runs
  everywhere with zero install.
* The external Python engine adapter (`bench-ext`) — benchmark opponents
  are now built-in reference bots.
* The "micro-learning" decision bias — adversarial-robustness contract
  (records are passive statistics).
* Nightly Rust requirement (`#![feature(test)]`) and the non-portable
  `target-cpu=native` default build.

**Fixed**

* `hypothesis_count()` / `probability_matrix()` were stubs returning
  zeros — now real data via `TargetingStrategy::stats`.
* The hypothesis filter never regenerated on the first move of a game
  (`0 >= 1` comparison bug).
* Sunk-ship length was lost on the wire (`receive_shot` reply) — now an
  additive `len` field.
* `best_fleet` panics replaced with a deterministic fallback.
* Placement fingerprint: border cells 0.000 occupancy → ε-band mixed
  strategy (measured in `tests/placement_statistics.rs`).

**Added**

* The blended hybrid scoring (`n/(n+K)`) with measured calibration.
* Cargo workspace (`sonar-core` + `sonar-wasm`), 122-test suite, Wilson-CI
  benchmarking, hostile-input batteries, determinism guarantees.
* The full web app: play, team mode, modding with arena, in-browser test
  suite, docs with client-side search. Deployable to GitHub Pages from
  the repo as-is.
* CI (test/clippy/fmt/wasm/web gates) and the Pages deploy workflow.
* The stability contract (`STABILITY.md`) and reproducible benchmark
  reports (`BENCHMARKS.md`).

## Roadmap to 1.0

Milestone-driven; each ships only with its tests green.

* **0.3** — exact-CENSUS endgame solver (submarine-perfect play in the
  last 1–2 ships), SPRT-driven strength ladders, seed-verified release
  numbers.
* **0.4** — generalised boards (5×5 up to 30×30, tiered bitboards),
  polyomino ships, torus/holes rule variants, feasibility solver.
* **0.5** — multi-ISA SIMD kernels (AVX2/AVX-512/NEON/wasm128 with runtime
  dispatch + differential testing), adversarial red-team harness
  (out-of-tree, never shipped), exploitability tracking file.
* **1.0** — API freeze; the beta contract becomes the 1.0 contract.

## License

Apache-2.0. See [LICENSE](LICENSE).
