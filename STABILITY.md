# Sonar Stability Contract

**Version 0.2.0-beta.1 (beta channel)**

Sonar 0.1.0 shipped with the disclaimer *"experimental — may contain bugs
or panics"*. The 0.2.0-beta channel replaces that disclaimer with a
written, testable contract. This document is that contract.

## The guarantees

| # | Guarantee | Statement | Enforced by |
|---|---|---|---|
| 1 | **No reachable panics** | No public API or protocol input can crash the engine. `unwrap`/`expect`/`panic!` are lint-denied in library code (`clippy::unwrap_used`); `#![forbid]`-level policy on `unsafe` (the WASM FFI boundary is the single audited exception). | CI clippy gate + hostile-input test batteries (Rust + web) |
| 2 | **Determinism** | With work-limited search (`Deadline::none()`), the same seed produces a **bit-identical game** on every platform and ISA. Wall-clock deadlines are explicitly excluded — they trade reproducibility for extra thinking time. | `tests/determinism.rs`: full-game replay comparison, benchmark seed equality, `reset() == fresh engine` |
| 3 | **Pure-function targeting** | Targeting decisions are a pure function of the public observation sequence. No opponent modelling, no cross-game state, no learned bias. An adversary collecting unbounded games cannot extract a targeting "personality", because none exists. | `test_reset_equals_fresh_engine`, targeting purity unit tests, code review of the `targeting` module (no `learning` import) |
| 4 | **Bounded placement bias** | Fleet placement is a documented mixed strategy (ε-band sampling). Measured per-cell occupancy stays within a published band; there are no predictable-empty and no predictable-occupied cells. | `tests/placement_statistics.rs`: per-cell bands, two-sample stability, orientation balance |
| 5 | **Protocol stability (v1)** | The JSON wire protocol is frozen: existing commands and fields never change meaning; evolution is additive-only (new commands, new fields). | `tests/protocol.rs` golden-shape tests incl. an additive-field regression (`receive_shot` gains `len` without breaking `"sunk"`) |
| 6 | **Honest measurement** | Every published benchmark number ships with methodology, confidence intervals (Wilson 95%), and a re-run command. Strength numbers are work-limited (machine-independent). | `BENCHMARKS.md` (generated from real runs) + the benchmark suite itself |
| 7 | **Test gate** | No release ships with a red gate: 122 Rust tests + web verification (engine E2E, browser suite, mod runtime) must pass in CI on every commit. | `.github/workflows/ci.yml` |

## What "beta" means (and does not mean)

Beta means: the above contract is now **in force and continuously verified**,
but the public API is not yet frozen for semver. Concretely:

* `sonar-core`'s public API may still see additive changes (new methods,
  new config fields) within the 0.x series. Breaking changes will be
  avoided where possible and always called out in the changelog with a
  migration note.
* The JSON protocol v1 is frozen *now* (stronger than the API contract).
* File formats (`learning.json`) are versioned and forward-compatible.

## Known limitations (documented, not hidden)

* Board generalisation: the 0.2.x series plays the canonical 10×10 board.
  Larger boards (up to 30×30), polyomino ships, and torus wrap are on the
  roadmap to 0.5 — see the README roadmap. The `GameRules` validator
  currently accepts 5×5–10×10.
* The hypothesis filter's strength gain is budget-dependent (measured in
  `BENCHMARKS.md`): below ~256 hypotheses the engine behaves close to
  pure PDF; the blend is calibrated so it is never *worse*.
* Wall-clock pondering (`Deadline::from_secs`) is intentionally
  non-reproducible (more time = sharper posterior). Use work-limited
  search for replays and audits.

## Reporting a violation

Any panic reachable from the public API or the protocol, any
reproducibility failure with `Deadline::none()`, or any protocol
incompatibility with a documented v1 client is a **contract violation**
and treated as a release blocker. Open an issue with the input sequence
and seed; the test suite grows a regression case from it.
