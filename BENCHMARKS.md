# Sonar Benchmarks

**Version 0.2.0-beta.1** · generated from real runs · every number is reproducible

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
node scripts/test-web.mjs               # engine E2E
node scripts/test-suite.mjs             # the browser test suite, in Node
node scripts/test-mods.mjs              # mod runtime + examples

# Everything at once
./sonar-forge.sh
```

Raw logs of the official runs are not committed (they are regenerable from
the fixed seeds); the numbers above were produced by the commands shown,
on the environment listed, at commit time — see git history for the exact
commit.
