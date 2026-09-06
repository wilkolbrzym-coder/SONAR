# Sonar

**The world's strongest battleship AI engine.**

[![CI](https://github.com/wilkolbrzym-coder/SONAR/actions/workflows/ci.yml/badge.svg)](https://github.com/wilkolbrzym-coder/SONAR/actions/workflows/ci.yml)
[![Pages](https://github.com/wilkolbrzym-coder/SONAR/actions/workflows/pages.yml/badge.svg)](https://github.com/wilkolbrzym-coder/SONAR/actions/workflows/pages.yml)

Pure Rust · Apache-2.0 · PDF density + Bayesian hypotheses · exact-CENSUS
endgame · generalised boards (polyomino / torus / holes) · multi-ISA SIMD ·
WebAssembly

**[▶ Play it in your browser](https://wilkolbrzym-coder.github.io/SONAR/)** —
the full engine, test suite, live benchmarks and searchable docs run locally
in the page. No install, no servers, no telemetry.

| | |
|---|---|
| Version | **0.5.0-beta.1** — beta, see the [stability contract](STABILITY.md) |
| Rust | stable, MSRV 1.89 (developed and tested on 1.98.1), edition 2024 |
| Platforms | Linux / macOS / Windows (native) + every modern browser (WASM) |
| Verification | **212 Rust tests** + 17-check web suite + 7 verified doc examples + red-team gates, all green in CI |
| SIMD | AVX2 / AVX-512F (runtime-detected) · NEON · wasm128 — differential-tested against scalar |
| Adversarial ledger | [EXPLOITABILITY.md](EXPLOITABILITY.md) (red-team harness, out-of-tree, never shipped) |

---

## Why it is the strongest

Sonar combines four measured techniques — no hype components, each one
justified by a benchmark:

1. **PDF density targeting** — for every cell, count the legal ship
   placements passing through it given all observations; fire at the
   densest cell. Pure constraint propagation, zero sampling noise.
2. **Bayesian hypothesis filtering** — sample full fleet configurations
   consistent with every observation, discard inconsistent ones after each
   shot, and blend the posterior with the PDF using an adaptive weight
   (n/(n+K)) — measured to beat PDF-only by ~12 pp at a 512-hypothesis
   budget.
3. **Exact-CENSUS endgame solver (0.3)** — with 1–2 ships left, enumerate
   *every* legal configuration exactly and run memoised expectimax over the
   observation tree. Touched submarines die in 3–5 shots: the theoretical
   optimum.
4. **Constraint-dispersal placement** — near-optimal placements drawn from
   an ε-band mixed strategy, so an attacker collecting thousands of games
   cannot fingerprint where your fleet hides (the 0.1.0 argmin placement
   leaked: 0.000 occupancy on all 36 border cells; now bounded at
   0.057–0.204).

And since 0.4/0.5 the same theory scales beyond 10×10: boards from 5×5 to
30×30, polyomino ships, torus wrap, island holes, an exact feasibility
solver, and multi-ISA SIMD kernels underneath it all.

## Quick start

```bash
git clone https://github.com/wilkolbrzym-coder/SONAR.git
cd SONAR
cargo build --release

# play against the engine in your terminal
./target/release/sonar play

# or drive it from any language over line-delimited JSON on stdio
./target/release/sonar serve
```

One full shot cycle over the protocol (identical over stdio, WASM and
tests — this exact exchange is executed in CI):

```json
{"cmd":"new_game","seed":42,"config":{"hypothesis_soft_target":64,"default_deadline_secs":0}}
{"cmd":"place_smart"}
{"cmd":"suggest_move","deadline_secs":0}
{"cmd":"receive_shot","r":3,"c":5}
{"cmd":"observe","r":3,"c":5,"result":"miss"}
{"cmd":"snapshot"}
```

As a Rust library:

```rust
use sonar::time_limit::Deadline;
use sonar::{Engine, EngineConfig, ShotResult};

let mut engine = Engine::new(EngineConfig {
    hypothesis_soft_target: 512,
    default_deadline_secs: 0,
    use_learning: false,
    ..EngineConfig::default()
});
engine.reseed(0x5EED_0000_0000_0001);
engine.place_fleet_smart();

let (r, c) = engine.choose_move(Deadline::none());   // the engine's shot
engine.observe_result(r, c, ShotResult::Miss);        // you referee
let posterior = engine.probability_matrix();          // full observability
```

The complete runnable version of this example ships as
`crates/sonar-core/examples/basic.rs` and is compiled on every CI run —
the docs never show unverified code.

## The web app

The `web/` directory is a zero-dependency static site (Material Design 3)
published to GitHub Pages by the repository workflow:

- **Play** — a duel against the engine on the classic board, six solo
  *hunt* modes on generalised boards (7×7, torus, 16×16, archipelago,
  polyomino fleet…), and a paper-game advisor mode.
- **Benchmarks** — strength gates with Wilson 95% CIs, per-move latency
  and self-play throughput, measured live in your browser against the
  same WASM engine, plus the release reference tables.
- **Tests** — the same verification battery CI runs, in the page.
- **Docs** — the full manual with client-side search; every runnable code
  example is executed and asserted in CI (`scripts/test-docs.mjs`).

## Strength (the short version)

Every number is seeded and reproducible — full data, commands and
methodology in [BENCHMARKS.md](BENCHMARKS.md) and the site's Benchmarks
tab.

| Matchup (1024 hypotheses) | Win rate | Wilson 95% |
|---|---|---|
| Sonar vs Random | **100.0%** | [96.3, 100.0] |
| Sonar vs HuntTarget (textbook) | **90.0%** | 60 games |
| Sonar vs BurnsPdf (Dartmouth-style) | **56.7%** | 60 games |
| Sonar vs MonteCarlo-512 | **100.0%** | 60 games |
| Sonar vs Sonar (fairness check) | 48/52 | CI contains 50% |

Handicap runs (opponent gets 2× the time or 2× the hypotheses) show the
algorithm — not compute — carries the advantage. `sonar verify-release`
prints the seed-verified release numbers with a hash-chain digest:
re-run it anywhere, compare bit-for-bit.

## The beta stability contract

0.5.0-beta.1 keeps the written contract introduced in 0.2.0
([STABILITY.md](STABILITY.md)):

- **No reachable panics** — `unwrap`/`expect` denied by lint in library
  code; 2,420 hostile protocol inputs tested with zero panics; malformed
  input gets JSON errors and never corrupts engine state.
- **Bit-identical determinism** — same seed, same game, every platform;
  verified by full-game replay tests.
- **No adversarial fingerprint** — targeting is a pure function of public
  observations; placement is a bounded mixed strategy; no cross-game
  learning (removed in 0.2.0).
- **Protocol v1 frozen** — additive-only evolution.

## Repository layout

```
crates/sonar-core     engine library + `sonar` CLI (lib + bin + examples)
crates/sonar-simd     multi-ISA kernels (the only crate with unsafe code)
crates/sonar-wasm     cdylib for the browser app
web/                  static MD3 web app (GitHub Pages, no build step)
scripts/              build-wasm.sh + Node verification (web, suite, docs)
redteam/              adversarial harness — out-of-tree, never shipped
BENCHMARKS.md         measured performance + reproduction commands
STABILITY.md          the beta contract
EXPLOITABILITY.md     the adversarial ledger
```

## Building & verifying

```bash
cargo build --release                    # native engine + CLI
cargo test --release                     # 212 tests (incl. examples)
cargo clippy --release --all-targets -- -D warnings   # zero warnings
cargo bench                              # criterion micro-benchmarks

./scripts/build-wasm.sh                  # wasm32 + simd128 → web/engine.wasm
node scripts/test-web.mjs                # engine E2E (12 checks)
node scripts/test-suite.mjs              # the browser suite, in Node (17)
node scripts/test-docs.mjs               # every runnable doc example (7)
cargo test --release --manifest-path redteam/Cargo.toml   # red-team gates
```

CI runs all of the above on every push, plus a `release-checked` pass
with overflow checks enabled. No release ships with a red gate.

## License

Apache-2.0 — see [LICENSE](LICENSE). Reference bots included for
benchmarking: HuntTarget (textbook), BurnsPdf (Ethan Burns, Dartmouth),
Monte Carlo sampling.
