# Sonar Stability Contract

**Version 0.5.0-beta.1 (beta channel)**

Sonar 0.1.0 shipped with the disclaimer *"experimental — may contain bugs
or panics"*. The 0.2.0-beta channel replaces that disclaimer with a
written, testable contract. This document is that contract.

## The guarantees

| # | Guarantee | Statement | Enforced by |
|---|---|---|---|
| 1 | **No reachable panics** | No public API or protocol input can crash the engine — including the `variant_*` generalised-game commands (0.4). `unwrap`/`expect`/`panic!` are lint-denied in library code (`clippy::unwrap_used`); `unsafe` exists only in the audited `sonar-simd` kernel crate and the WASM FFI boundary (0.5 — see its SAFETY CHARTER). | CI clippy gate + hostile-input test batteries (Rust + web) + the red-team protocol fuzzer (2000-case mutation battery) |
| 2 | **Determinism** | With work-limited search (`Deadline::none()`), the same seed produces a **bit-identical game** on every platform and ISA. This includes the exact-CENSUS endgame solver (no RNG, no clock — 0.3) and the generalised engine (deterministic density, lowest-index tie-break — 0.4). Wall-clock deadlines are explicitly excluded — they trade reproducibility for extra thinking time. | `tests/determinism.rs` + endgame/variant determinism tests + the red-team determinism probe (replays under machine-load noise) |
| 3 | **Pure-function targeting** | Targeting decisions are a pure function of the public observation sequence. No opponent modelling, no cross-game state, no learned bias. An adversary collecting unbounded games cannot extract a targeting "personality", because none exists. | `test_reset_equals_fresh_engine`, targeting purity unit tests, code review of the `targeting` module (no `learning` import) |
| 4 | **Bounded placement bias** | Fleet placement is a documented mixed strategy (ε-band sampling). Measured per-cell occupancy stays within a published band; there are no predictable-empty and no predictable-occupied cells. | `tests/placement_statistics.rs`: per-cell bands, two-sample stability, orientation balance |
| 5 | **Protocol stability (v1)** | The JSON wire protocol is frozen: existing commands and fields never change meaning; evolution is additive-only (new commands, new fields). The 0.4 `variant_*` family is an *addition* under this policy. | `tests/protocol.rs` golden-shape tests incl. an additive-field regression (`receive_shot` gains `len` without breaking `"sunk"`) + variant protocol round-trip tests |
| 6 | **Honest measurement** | Every published benchmark number ships with methodology, confidence intervals (Wilson 95%), and a re-run command. Strength numbers are work-limited (machine-independent). | `BENCHMARKS.md` (generated from real runs) + the benchmark suite itself |
| 7 | **Test gate** | No release ships with a red gate: 212 Rust tests (incl. the SIMD differential battery) + web verification (engine E2E, 17-check browser suite, 7 verified documentation examples) + the out-of-tree red-team gates must pass in CI on every commit. | `.github/workflows/ci.yml` |
| 8 | **SIMD bit-identity** (0.5) | Every SIMD backend (AVX2/AVX-512F/NEON/wasm128) produces **bit-identical** results to the scalar reference. The active backend is a performance choice, never a behavioural one. | `sonar-simd` differential tests (200-case fuzz per kernel + gated per-backend tests) + end-to-end generalised-engine tests |
| 9 | **Tracked exploitability** (0.5) | The engine's adversarial exposure is a *measured, published number* — not a claim. Hard invariants (no panics, no determinism leaks, no invariant violations) are gated; the adversarial placement delta is tracked with a redline in the ledger. | `redteam/` harness gates in CI + `EXPLOITABILITY.md` (the adversarial ledger) |

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

* The **classic engine** (the strongest path) plays the canonical 10×10
  straight-ship game; generalised boards (5×5–30×30, polyominoes, torus,
  holes) run on the 0.4 generalised engine, which uses exact per-ship
  density targeting without the Bayesian hypothesis blend (the classic
  hybrid and the exact endgame solver are 0.6 roadmap items for variants).
* The hypothesis filter's strength gain is budget-dependent (measured in
  `BENCHMARKS.md`): below ~256 hypotheses the engine behaves close to
  pure PDF; the blend is calibrated so it is never *worse*.
* The exact endgame solver defers (rather than plays) in fresh-hunt
  regimes — by design; see the regime-gate discussion in `endgame.rs`.
* The measured adversarial placement delta (oracle-designed hostile
  fleets cost Sonar ~12 extra shots over random baselines) is tracked,
  redlined and documented in `EXPLOITABILITY.md` — an honest number with
  a mitigation plan, not a hidden weakness.
* Wall-clock pondering (`Deadline::from_secs`) is intentionally
  non-reproducible (more time = sharper posterior). Use work-limited
  search for replays and audits.
* MSRV is 1.89 (raised from 1.85 in 0.5.0 for the AVX-512 intrinsics).

## Reporting a violation

Any panic reachable from the public API or the protocol, any
reproducibility failure with `Deadline::none()`, or any protocol
incompatibility with a documented v1 client is a **contract violation**
and treated as a release blocker. Open an issue with the input sequence
and seed; the test suite grows a regression case from it.
