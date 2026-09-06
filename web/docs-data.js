/**
 * Sonar documentation content — rendered by the Docs tab and indexed by
 * search.js. Mirrors README.md (the canonical copy) in a structured form
 * the site can render and search without any build step or external
 * dependencies.
 */

export const DOCS = {
  title: "Sonar Documentation",
  version: "0.2.0-beta.1",
  sections: [
    {
      id: "overview",
      title: "What is Sonar?",
      keywords: ["sonar", "overview", "intro", "about", "engine", "battleship"],
      body: [
        "Sonar is a battleship AI engine written in pure Rust and compiled to WebAssembly for this app. It plays the classic 10×10 fleet game (ships of length 5, 4, 3, 3, 2 — 17 cells) and combines three techniques to play at state-of-the-art strength:",
        "1. PDF density targeting — for every cell, count the legal ship placements passing through it given all observations (misses, hits, sinks), then fire at the densest cell.",
        "2. Bayesian hypothesis filtering — maintain a sample of full fleet configurations consistent with every observation; discard inconsistent ones after each shot; blend the posterior with the PDF density using an adaptive weight (n/(n+K)).",
        "3. Constraint-dispersal placement — place your own fleet by random sampling with a penalty score (ship contact, edge/corner exposure, parity balance), drawing from a near-optimal ε-band so the placement cannot be fingerprinted.",
        "Sonar is fully self-contained: no network services, no telemetry, no external AI. Everything on this page runs locally in your browser — the engine, the docs search, the test suite and the mod runtime.",
      ],
    },
    {
      id: "beta",
      title: "The Beta Stability Contract",
      keywords: ["beta", "stability", "contract", "version", "guarantee"],
      body: [
        "Version 0.2.0-beta.1 formally ends the 0.1.0 'experimental — may contain bugs' era. In exchange, the engine commits to a written contract (see STABILITY.md in the repository):",
        "Correctness: no panics reachable through the public API or protocol — unwrap/expect are denied by lint in library code, and a hostile-input test battery feeds garbage into the protocol continuously.",
        "Determinism: with work-limited search (no wall-clock deadline), the same seed produces a bit-identical game on every platform. This is tested by replaying full games and comparing move logs.",
        "Adversarial robustness: targeting is a pure function of public information — no opponent modelling, no cross-game bias, no learned 'personality' an attacker could exploit. Placement is a bounded, documented mixed strategy.",
        "Protocol stability: the JSON wire format (protocol v1) is frozen; evolution is additive-only — new commands and fields may appear, existing ones never change meaning.",
      ],
    },
    {
      id: "play",
      title: "Playing Against Sonar",
      keywords: ["play", "game", "how to", "difficulty", "levels", "board"],
      body: [
        "Open the Play tab. Pick a difficulty — it maps directly to the engine's hypothesis budget: Ensign (16 hypotheses), Lieutenant (64), Commander (256), Captain (1024), Admiral (4096). Higher budgets think longer per move and play sharper.",
        "Place your fleet: Auto places a Sonar-grade fleet for you, or place ships manually — click a ship in the list, then click the grid and press R (or the Rotate button) to flip orientation. Ships may not touch, even diagonally.",
        "Click enemy-board cells to fire. Sonar answers on its turn. The move log records every shot; enable the heatmap to overlay Sonar's live density estimate of your fleet (its full reasoning, exposed — 100% observability).",
        "Team mode (the Team tab) is for playing a physical/paper opponent: you fire in the real world, enter the result here, and Sonar suggests the next shot with its reasoning.",
      ],
    },
    {
      id: "modding",
      title: "Modding — 100% Control Over Sonar",
      keywords: ["mod", "modding", "api", "chooseMove", "placeFleet", "custom", "javascript", "brain"],
      body: [
        "The Mods tab lets you write a complete replacement brain for Sonar in plain JavaScript, directly in the app. Mods run inside a Web Worker at full native speed — the UI never blocks.",
        "A mod is an object with up to four hooks: name (string), placeFleet(api) → ships or null, chooseMove(api) → {r, c} (required), onObserve(api, r, c, result), and onGameEnd(api, won, moves).",
        "The api object in chooseMove exposes everything the engine itself sees: api.shots / api.hits / api.sunk / api.activeHits (Uint8Array(100) masks of the fleet you are attacking), api.remaining (surviving enemy ship lengths), api.history (your shot log), api.density (Sonar's PDF density matrix, Float32Array(100)), api.probability (Sonar's Bayesian posterior), api.hypothesisCount, plus helpers: api.valid(r, c), api.argmax(matrix), api.rand().",
        "In placeFleet the api offers api.rand(), api.legalFleet(ships), api.randomFleet() and api.fleetLens. Return an array like [{r:0, c:0, len:5, horizontal:true}, ...] or null for a random legal fleet.",
        "Fair play is enforced: an illegal move (out of bounds, already fired, malformed return) is auto-corrected to a random legal cell and counted as a strike; three strikes forfeit the game. Exceptions surface in the match report.",
        "Use the arena to benchmark your mod against the engine at any difficulty: you get win rates with Wilson 95% confidence intervals, average game length, forfeit counts and error logs.",
      ],
    },
    {
      id: "protocol",
      title: "The JSON Protocol (v1)",
      keywords: ["protocol", "json", "ipc", "serve", "commands", "api", "wire"],
      body: [
        "Sonar speaks a line-delimited JSON protocol — identical over stdio (sonar serve), from the WebAssembly exports, and in tests. One request per line, one reply per line.",
        "Lifecycle: new_game {seed, config?} → place_random / place_smart / place_manual {ships} → loop { choose_move {deadline_secs} | suggest_move → receive_shot {r,c} → observe {r,c,result} } → snapshot / probability / density → record_game {won} → reset.",
        "Result strings: 'miss', 'hit', 'sunk' (with an additive len field on receive_shot replies — use 'sunk_<len>' when observing so the engine reconstructs the ship exactly), 'already', 'invalid'.",
        "Introspection: snapshot returns fleet/shot/hit/sunk masks (decimal strings), enemy_remaining, hypothesis_count, the probability and density matrices, and moves_fired. probability and density are also available as standalone commands.",
        "Benchmarks over the protocol: bench {games, opponent: random|pdf|self, soft_target, seed} runs a deterministic, seeded match series and returns Wilson-interval statistics.",
        "Hostile input policy: every malformed line receives a JSON error reply — the engine never panics, never hangs, and remains fully usable afterwards. This is tested with a continuous garbage battery.",
      ],
    },
    {
      id: "engine",
      title: "How the Engine Thinks",
      keywords: ["engine", "pdf", "density", "hypothesis", "bayes", "posterior", "blend", "algorithm"],
      body: [
        "Hunt phase: with no hits on the board, the PDF density peaks in the centre — the number of legal placements through central cells is highest. A parity (checkerboard) preference shaves expected shots further.",
        "Target phase: after a hit, only placements containing the active hits survive, so density collapses onto the neighbours — constraint propagation in one pass.",
        "Hypothesis filter: full-fleet samples consistent with all observations form a posterior over cells. It is theoretically sharper (it models ship mutual exclusion) but noisy at small budgets. The hybrid blends it with the PDF using weight n/(n+K) — measured, not assumed: at a 512-hypothesis budget the blend beats PDF-only by ~12 percentage points, while at 64 it safely degenerates toward the PDF.",
        "Placement: fleets are sampled and scored; the final fleet is drawn from candidates within ε of the optimal penalty (ε=4). This is the GHOST FLEET indistinguishability design — pure argmin placement would let an attacker learn 'Sonar never uses the border' (measured: 0.000 occupancy on all 36 border cells before the fix; 0.057–0.204 after).",
      ],
    },
    {
      id: "tests",
      title: "Testing Sonar (In This Page)",
      keywords: ["test", "tests", "suite", "verify", "quality", "determinism", "invariant"],
      body: [
        "The Test tab runs a browser-adapted version of the project's verification suite against the live WASM engine: the protocol contract, determinism replays, full self-play with no-repeat-shot invariants, strength gates with Wilson intervals, hostile-input robustness, and the mod runtime.",
        "The repository itself ships 122 Rust tests across six layers: unit, determinism (bit-identical replays), full-game invariants, property-based batteries, protocol/golden, statistical strength gates and placement-statistics (two-sample consistency, bounded bias, orientation balance).",
        "The same web suite runs in CI via Node (scripts/test-suite.mjs) — the browser and CI verify the identical checks, so 'green in CI' means 'green in your browser'.",
      ],
    },
    {
      id: "bench",
      title: "Benchmarks & Methodology",
      keywords: ["benchmark", "wilson", "confidence", "statistics", "strength", "measurement"],
      body: [
        "Every published win rate carries a Wilson score 95% confidence interval — correct for extreme proportions (100%/0%) and small samples, unlike naive normal-approximation error bars.",
        "Benchmark games are work-limited (a hypothesis budget), never wall-clock-limited: strength numbers measure the algorithm, not the machine. Timings are reported separately and never affect outcomes.",
        "All runs are seeded — the same seed replays the same games bit-for-bit, so any third party can reproduce a number exactly (see BENCHMARKS.md in the repository for the raw data and the re-run commands).",
        "E[shots] is reported with standard error, not just an average. The CLI adds handicap benchmarks: bench-2x (opponent gets 2× the time) and bench-half (opponent gets 2× the hypothesis budget) to demonstrate the algorithm — not just compute — carries the advantage.",
      ],
    },
    {
      id: "build",
      title: "Building From Source",
      keywords: ["build", "rust", "cargo", "wasm", "compile", "source", "github pages"],
      body: [
        "Requirements: stable Rust 1.85+ (developed and tested on 1.98.1) — no nightly toolchain needed since 0.2.0.",
        "Native engine + CLI: cargo build --release (the binary is target/release/sonar). Tests: cargo test --release. Benchmarks: cargo bench or sonar bench.",
        "WebAssembly: rustup target add wasm32-unknown-unknown, then cargo build --release --target wasm32-unknown-unknown -p sonar-wasm and copy target/wasm32-unknown-unknown/release/sonar_wasm.wasm to web/engine.wasm (scripts/build-wasm.sh does both).",
        "This site: serve the web/ directory statically — it is plain HTML/JS/CSS with no build step. GitHub Pages deploys it automatically via the repository workflow (build wasm → copy → publish).",
        "The default build is portable (no target-cpu=native). For a single-machine maximum-performance native build use RUSTFLAGS='-C target-cpu=native' cargo build --release.",
      ],
    },
    {
      id: "cli",
      title: "CLI Reference",
      keywords: ["cli", "command", "terminal", "sonar serve", "play", "bench", "usage"],
      body: [
        "sonar version — print version, channel, protocol, license.",
        "sonar play — play against the engine in the terminal (set $SONAR_MOVE_SECS to change Sonar's time budget).",
        "sonar serve — run the JSON IPC server on stdin/stdout; the integration point for any language.",
        "sonar bench [N] / bench-fast / bench-big — self-play benchmarks with Wilson CIs (Hybrid vs Random, vs PdfOnly, vs itself).",
        "sonar bench-ref [N] — N games against published reference bots (HuntTarget, BurnsPdf, MonteCarlo-256/512).",
        "sonar bench-2x [N] — time-handicap: the opponent gets twice Sonar's move time. sonar bench-half [N] — compute-handicap: the opponent gets twice the hypothesis budget.",
        "sonar learning — show the passive game statistics database (records never influence play).",
      ],
    },
    {
      id: "security",
      title: "Adversarial Robustness",
      keywords: ["security", "adversarial", "exploit", "ai", "attack", "robustness", "fair"],
      body: [
        "Threat model: an attacker collects thousands of games against Sonar and trains a model (RL agent, classifier) to exploit statistical regularities in its behaviour. Two attack surfaces exist: predicting where Sonar shoots (targeting) and predicting where Sonar's own ships are (placement).",
        "Targeting is exploitable only if it deviates from the Bayes-optimal posterior. Sonar's targeting is a pure function of the public observation sequence — no opponent identity, no history, no learned bias. An attacker cannot use anything Sonar's algorithm does not already use. Verified by tests: reset() must restore byte-identical first-move behaviour; two fresh engines with the same observation sequence produce the same distributions.",
        "Placement is a genuine policy choice and can leak a fingerprint. The 0.1.0 argmin placement had a catastrophic one (all 36 border cells at 0.000 occupancy). The 0.2.0 ε-band mixed strategy bounds every cell into a measured band with no predictable-empty and no predictable-occupied cells, while keeping ships strictly non-touching.",
        "The old 'micro-learning' feature (biasing decisions from past games) was removed in 0.2.0: it was a direct cross-game state leak. The statistics database is now passive-only — records never influence play.",
      ],
    },
    {
      id: "license",
      title: "License & Credits",
      keywords: ["license", "apache", "credits", "authors", "contribute"],
      body: [
        "Sonar is Apache-2.0 licensed. See the LICENSE file in the repository for the full text.",
        "Reference bots included for benchmarking: classic HuntTarget (textbook), BurnsPdf (Ethan Burns, Dartmouth battleship research), and Monte Carlo sampling (à la mitchelljy/battleships_ai).",
        "Contributions welcome — the project CI runs the full 122-test suite plus the web verification on every commit, and no release ships with a red gate.",
      ],
    },
  ],
};

/** Flat text chunks for the search index. */
export function docsChunks() {
  const chunks = [];
  for (const sec of DOCS.sections) {
    chunks.push({
      id: sec.id,
      title: sec.title,
      keywords: sec.keywords || [],
      text: sec.body.join(" "),
      section: sec,
    });
  }
  return chunks;
}
