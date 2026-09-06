# Sonar Benchmarks

**Version 0.5.0-beta.1** · generated from real runs · every number is reproducible

> This file documents measured performance and strength. Re-run everything
> yourself with the commands in [Reproducing this file](#reproducing-this-file).
> The raw output of the runs behind these tables is reproducible byte-for-byte
> for the seeded self-play benchmarks.

## Methodology

* **Win rates carry Wilson score 95% confidence intervals** — the correct
  interval for proportions, well-behaved at 0%/100% and small samples,
  unlike the naive normal approximation.
* **Strength is work-limited, not time-limited**: benchmark games run with
  `Deadline::none()` and a fixed hypothesis budget, so results measure the
  algorithm and are independent of machine speed. Wall-clock timings are
  reported separately and never affect outcomes.
* **Determinism**: every self-play run is seeded (`sonar bench` uses a fixed
  master seed); identical seeds replay identical games, verified by the
  `determinism` test suite. The `bench-ref` runs vs reference bots use
  per-game derived seeds for both sides.
* **E[shots] is reported with standard error**, not just the mean.
* Sonar plays at its **honest default strength** (1024 hypotheses, the
  `EngineConfig` default) unless a table row says otherwise.
* Hypothesis-budget scaling is reported explicitly — the engine's strength
  is a function of its compute budget, and we say so.

## Environment

| Item | Value |
|---|---|
| CPU | Intel Xeon (2 vCPU) |
| rustc | 1.98.1 (48a229cea 2026-09-01), stable channel |
| Profile | `release` (opt-level 3, LTO fat, codegen-units 1) |
| Portability | default build — no `target-cpu=native` |

## Strength: self-play matrix (100 games each, 1024 hypotheses)

`sonar bench` — seeded, deterministic.

| Matchup | Win rate | Wilson 95% CI | E[shots] ± SE (winner) |
|---|---|---|---|
| Sonar-Hybrid vs Random | **100.0%** | [96.3%, 100.0%] | 39.6 ± 0.6 |
| Sonar-Hybrid vs PdfOnly | **56.0%** | [46.2%, 65.3%] | 37.0 ± 0.5 |
| PdfOnly vs Random | **100.0%** | [96.3%, 100.0%] | 40.0 ± 0.6 |
| Sonar-Hybrid vs Sonar-Hybrid | 48.0% / 52.0% | [38.5%, 57.7%] | 36.1 ± 0.5 |

The self-play row is the first-mover fairness check: identical bots split
~50/50 (CI contains 50%), confirming no game-loop asymmetry.

## Strength: vs published reference bots (60 games each, 1024 hypotheses)

`sonar bench-ref 60`

| Opponent | Sonar win rate | avg moves/win |
|---|---|---|
| HuntTarget (classic textbook) | **90.0%** (54/60) | 38.4 |
| BurnsPdf (Dartmouth-style PDF) | **56.7%** (34/60) | 36.7 |
| MonteCarlo-256 | **100.0%** (60/60) | 40.1 |
| MonteCarlo-512 | **100.0%** (60/60) | 39.2 |

## Strength: hypothesis-budget scaling (vs BurnsPdf, 100 games each)

The hybrid's advantage over a strong PDF opponent grows with the sample
budget — the Bayesian posterior needs samples to sharpen (this is the
measured justification for the adaptive blend weight `n/(n+K)`).

| Hypothesis budget | Win rate vs BurnsPdf |
|---|---|
| 256 | 51.0% |
| 512 | 57.0% |
| 1024 | 60.0% |
| 2048 | 56.7% (60 games) |
| 4096 | 65.0% (60 games) |

## Strength: handicap benchmarks

The point: the **algorithm**, not raw compute, carries the advantage.

| Handicap | Result |
|---|---|
| Opponent gets **2× the time** (Sonar 5 s/move vs BurnsPdf 10 s/move, 20 games) | Sonar **55.0%** |
| Opponent gets **2× the time** (vs MonteCarlo-512, 20 games) | Sonar **100.0%** |
| Opponent gets **2× the hypotheses** (Sonar-128 vs Sonar-256, 40 games) | Sonar-128 **60.0%** — doubling the budget is worth ~10 pp, so time/compute alone does not explain Sonar's strength |

## Throughput: core micro-benchmarks (criterion)

`cargo bench` — per-operation latency on the benchmark machine.

| Operation | Time |
|---|---|
| `bitboard` set+test sweep (100 cells) | ~14 ns/cell-pair |
| `bitboard` popcount (100 bits) | ~288 ps |
| `bitboard` iterate (50 bits) | ~285 ps/cell |
| `bitboard` dilate8 (50 bits) | ~706 ps |
| `random_fleet` (one legal fleet) | ~256 ns |
| `best_fleet` 64 candidates | ~19.4 µs |
| `best_fleet` 1024 candidates | ~310 µs |
| `fleet_penalty` | ~44 ns |
| `pdf_density` (initial board) | ~5.1 µs |
| `pdf_density` (10 misses) | ~3.7 µs |
| `pdf_density` (mid-game) | ~2.7 µs |
| `pdf_choose_move` (initial) | ~10.7 µs |
| `hypothesis` regenerate 256 (initial) | ~72.7 µs |
| `hypothesis` regenerate 256 (mid-game) | ~1.24 ms |
| `hybrid_choose_move` (soft 64) | ~29 µs |
| full game: Hybrid vs Random | ~44 ms |
| full game: Hybrid vs Hybrid | ~82 ms |

## Endgame statistics

Across the self-play corpus (400 games):

* Average winning shots: **~37–40** (vs the theoretical optimum of ~17 for
  a perfect posterior; the gap is the price of hidden information, not
  weak play — no algorithm can do better than the posterior it infers).
* Every game terminated within the 200-move-pair guard; the invariant suite
  verifies zero illegal shots across full-game replays.

## 0.3: exact-CENSUS endgame solver

Measured on the benchmark machine (release build, AVX-512 backend):

| Measurement | Value |
|---|---|
| Solver latency, hit-pinned regime | **0.01 ms/move** (132 moves in 1 ms, 100% exact) |
| Touched length-2 submarine (optimal bound: 5) | finishes in **3–5 shots**, matching the theoretical optimum |
| Full-game effect (30 seeded solo games, 1024 hypotheses) | **39.43 vs 39.93 avg shots** — the endgame solver never hurts and claws back ~0.5 shots/game |
| Census counts | single 2-ship, empty board: 180 configs; touched: ≤ 4; two 3-ships: exact dedup (verified against brute force) |

The regime gate is honest engineering: in fresh hunts the solver defers to
the hybrid (PDF + parity is the stronger policy there — a greedy-posterior
fallback was measured to *lose* tempo in 2-ship midgames, so it is not
shipped). `sonar endgame` prints the census size and E[remaining] per move.

## 0.3: SPRT ladders & seed-verified numbers

* SPRT matches decide sequentially (±25 Elo, 5%/5%): hybrid-512 vs
  hybrid-64 accepts "A stronger" with live LLR — run `sonar sprt 60`.
* Release numbers (`sonar verify-release`, seed 0x5EED_CAFE, 8
  games/matchup): vs random **100%** (38.0 vs 87.4 shots), vs PDF-only
  **75%** (40.1 vs 43.0), endgame-on vs off 68.8% (40.6 vs 40.9). The
  hash-chain digest commits to the full result sequence — same seed,
  same digest, bit-for-bit, on any machine.

## 0.4: generalised boards (self-play, 8 games/preset, seeded)

| Preset | Cells | Ship cells | Avg shots/game | Time/game |
|---|---|---|---|---|
| classic (10×10) | 100 | 17 | 46.4 | < 1 ms |
| micro (7×7) | 49 | 9 | 21.9 | < 1 ms |
| big16 (16×16) | 256 | 32 | 109.9 | ~5 ms |
| **huge30 (30×30)** | 900 | 43 | 352.1 | ~225 ms |
| torus8 (8×8, wrap) | 64 | 14 | 32.1 | < 1 ms |
| archipelago12 (holes) | 144 | 17 | 58.1 | ~3 ms |
| poly10 (polyomino fleet) | 100 | 18 | 52.6 | ~2 ms |

Notes: the generalised engine uses exact per-ship density (no hypothesis
blend — classic-hybrid parity is a 0.6 roadmap item; the classic engine
averages ~40 shots on the same 10×10 game). Every preset terminates with
zero invariant violations in the red-team stress (12 games across presets,
including hole maps).

## 0.5: SIMD kernels

Throughput on the benchmark machine (AVX-512 backend, 10,000 placements ×
30-row words — a `huge30` fleet filtering task):

| Kernel | Throughput |
|---|---|
| `legal_filter` | 10,000 placements × 30 words in **124.5 µs** ≈ **2.4 G words/s** |
| backend selection | AVX-512 > AVX2 > scalar (runtime-detected); wasm128 in the browser build |

Every backend is differential-tested against the scalar reference
(bit-identical, 200-case fuzz batteries + gated per-backend tests); the
generalised engine routes its real workloads through the kernels, so all
numbers above are end-to-end differential-verified.

## 0.5: red-team measurements (the adversarial ledger)

Full runs with `cargo run --manifest-path redteam/Cargo.toml`; tracked
per-release in `EXPLOITABILITY.md`:

| Attack | Result (0.5.0-beta.1) |
|---|---|
| protocol fuzz (mutation + hostile corpus) | 420 quick / 2000 full inputs — **0 panics** |
| adversarial placement search | hostile fleet costs Sonar **50.5 shots** vs 38.1 random baseline (delta **12.38**, redline 14) |
| determinism under load | 6 seeds × interleaved busy-workload replays — **0 mismatches** |
| invariant stress | 12 games (classic + all variants + hole maps) — **0 violations** |

## WebAssembly parity

The identical engine runs in the browser (see the **Test** tab of the web
app, or `node scripts/test-suite.mjs`): same protocol, same determinism
checks, same strength gates. Benchmark command over the WASM protocol
(`bench`, 20 games vs random): **20/20 wins**, ~70 ms per game on a laptop
CPU — the browser engine is the same code, compiled to wasm32.

## Reproducing this file

```bash
# Strength (deterministic, seeded)
./target/release/sonar bench            # self-play matrix, 100 games ×4
./target/release/sonar bench-ref 60     # vs reference bots
./target/release/sonar bench-2x 20      # time-handicap
./target/release/sonar bench-half 40    # hypothesis-handicap

# Throughput (criterion)
cargo bench

# WebAssembly parity
./scripts/build-wasm.sh
node scripts/test-web.mjs               # engine E2E (12 checks)
node scripts/test-suite.mjs             # the browser test suite, in Node
node scripts/test-docs.mjs              # every runnable doc example
```

Raw logs of the official runs are not committed (they are regenerable from
the fixed seeds); the numbers above were produced by the commands shown,
on the environment listed, at commit time — see git history for the exact
commit.
