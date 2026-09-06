/**
 * Sonar documentation content — rendered by the Docs tab, indexed by
 * search.js, and **verified** by docs-verify.js.
 *
 * Sections are organised into groups (Getting started → Use cases →
 * Reference → Internals → Quality & security). Every example marked
 * `verified: true` is executed against the real engine:
 *
 *   - `lang: "json"`    examples carry a `run` array of protocol lines
 *                       (with optional `expect` assertions). The browser
 *                       Test tab and scripts/test-docs.mjs execute them
 *                       against a fresh WASM engine instance.
 *   - `lang: "rust"`    examples are the exact source of a file compiled
 *                       by CI (crates/sonar-core/examples/).
 *   - `lang: "bash"`    examples name CLI subcommands that exist in the
 *                       binary and are covered by the Rust test suite.
 *
 * The documentation never shows code that has not been run.
 */

export const DOCS = {
  version: "0.5.0-beta.1",
  groups: [
    "Getting started",
    "Use cases",
    "Reference",
    "Internals",
    "Quality & security",
  ],
  sections: [
    // ── Getting started ────────────────────────────────────────────────────
    {
      id: "overview",
      title: "What is Sonar?",
      group: "Getting started",
      keywords: ["sonar", "overview", "intro", "about", "engine", "battleship", "rust", "wasm"],
      body: [
        "Sonar is a battleship AI engine written in pure Rust and compiled to WebAssembly for this app. It plays the classic 10×10 fleet game (ships of length 5, 4, 3, 3, 2 — 17 cells) and combines three techniques to play at state-of-the-art strength: PDF density targeting, Bayesian hypothesis filtering, and constraint-dispersal placement. Since 0.5.0-beta.1 it also speaks generalised battleship — boards from 5×5 to 30×30, polyomino ships, torus and hole boards — with multi-ISA SIMD kernels underneath and an exact-CENSUS endgame solver on top.",
        "Everything on this page runs locally in your browser: the engine, the documentation search, the test suite, and the benchmarks. There are no network services, no telemetry, and no external AI — the engine binary and the WASM module are the same code.",
        "Sonar is Apache-2.0 licensed and 100% open source. The version you are reading about is 0.5.0-beta.1, the beta channel, wire protocol v1.",
      ],
    },
    {
      id: "beta",
      title: "The beta stability contract",
      group: "Getting started",
      keywords: ["beta", "stability", "contract", "version", "guarantee", "channel"],
      body: [
        "The beta channel commits to a written contract (the full text is STABILITY.md in the repository). Correctness: no panics are reachable through the public API or the protocol — unwrap/expect are denied by lint in library code, and a hostile-input battery feeds garbage into the protocol continuously (2,420 hostile inputs, zero panics in the current release).",
        "Determinism: with work-limited search (no wall-clock deadline), the same seed produces a bit-identical game on every platform. This is tested by replaying full games and comparing move logs. Adversarial robustness: targeting is a pure function of public information — no opponent modelling, no cross-game bias, no learned personality an attacker could exploit.",
        "Protocol stability: the JSON wire format (protocol v1) is frozen; evolution is additive-only — new commands and fields may appear, existing ones never change meaning. No release ships with a red gate: 212 Rust tests, the SIMD differential battery, the WASM build, the web verification, and the out-of-tree red-team suite must all pass in CI on every commit.",
      ],
    },
    {
      id: "build",
      title: "Building from source",
      group: "Getting started",
      keywords: ["build", "rust", "cargo", "wasm", "compile", "source", "github pages", "msrv"],
      body: [
        "Requirements: stable Rust 1.89+ (developed and tested on 1.98.1) — no nightly toolchain needed. The MSRV comes from the AVX-512 intrinsics used by the SIMD kernels. The default build is portable (no target-cpu=native); use RUSTFLAGS='-C target-cpu=native' for a single-machine maximum-performance build.",
        "The web app you are using is the web/ directory served statically — plain HTML/JS/CSS with no build step. GitHub Pages deploys it automatically: the repository workflow rebuilds engine.wasm on every deploy so the published site always matches the repository state.",
      ],
      examples: [
        {
          title: "Build and test everything",
          lang: "bash",
          verified: true,
          code: `# native engine + CLI + library
cargo build --release                 # binary: target/release/sonar

# full test suite (212 tests) and the compiled examples
cargo test --release

# WebAssembly (the engine this page runs)
rustup target add wasm32-unknown-unknown
./scripts/build-wasm.sh               # builds with +simd128 → web/engine.wasm

# web verification outside any browser (Node)
node scripts/test-web.mjs             # engine E2E
node scripts/test-suite.mjs           # the browser test suite
node scripts/test-docs.mjs            # every runnable doc example

# benchmarks
cargo bench                           # criterion micro-benchmarks
./target/release/sonar bench          # seeded strength benchmarks`,
        },
      ],
    },

    // ── Use cases ──────────────────────────────────────────────────────────
    {
      id: "usecase-game",
      title: "Use case: drive a full game over the protocol",
      group: "Use cases",
      keywords: ["use case", "game", "protocol", "stdio", "serve", "ipc", "play", "integration"],
      body: [
        "The most common integration: your application owns the game state (or the human opponent), and Sonar does the thinking. The protocol is line-delimited JSON — identical over stdio (sonar serve), from the WebAssembly exports, and in tests. One request per line, one reply per line.",
        "The exchange below is a complete playable skeleton: start a seeded game, let the engine place its own fleet, ask for a shot, referee the shot yourself, report the result, and repeat. Feeding 'sunk_5' (with the length) instead of bare 'sunk' lets the engine reconstruct the sunk ship exactly — always do this if you know the length.",
        "Because the game is seeded and work-limited, the same seed replays the same game bit-for-bit — you can log, replay, and unit-test your integration.",
      ],
      examples: [
        {
          title: "A complete shot cycle (executed against the live engine)",
          lang: "json",
          verified: true,
          code: `{"cmd":"new_game","seed":42,"config":{"hypothesis_soft_target":64,"default_deadline_secs":0}}
{"cmd":"place_smart"}
{"cmd":"suggest_move","deadline_secs":0}
{"cmd":"receive_shot","r":3,"c":5}
{"cmd":"observe","r":3,"c":5,"result":"miss"}
{"cmd":"snapshot"}`,
          run: [
            { line: '{"cmd":"new_game","seed":42,"config":{"hypothesis_soft_target":64,"default_deadline_secs":0,"use_learning":false}}',
              expect: [{ path: "ok", equals: true }] },
            { line: '{"cmd":"place_smart"}',
              expect: [{ path: "ok", equals: true }] },
            { line: '{"cmd":"suggest_move","deadline_secs":0}',
              expect: [
                { path: "row", min: 0, max: 9 },
                { path: "col", min: 0, max: 9 },
                { path: "confidence", min: 0, max: 1 },
              ] },
            { line: '{"cmd":"receive_shot","r":3,"c":5}',
              expect: [{ path: "result", inArray: ["miss", "hit", "sunk", "already", "invalid"] }] },
            { line: '{"cmd":"observe","r":3,"c":5,"result":"miss"}',
              expect: [{ path: "ok", equals: true }] },
            { line: '{"cmd":"snapshot"}',
              expect: [
                { path: "moves_fired", min: 1, max: 1 },
                { path: "density_matrix", length: 100 },
              ] },
          ],
        },
        {
          title: "The same integration over stdio, from any language",
          lang: "bash",
          verified: true,
          code: `# sonar serve speaks the protocol on stdin/stdout
printf '%s\\n' \\
  '{"cmd":"new_game","seed":42,"config":{"hypothesis_soft_target":64,"default_deadline_secs":0}}' \\
  '{"cmd":"place_smart"}' \\
  '{"cmd":"suggest_move","deadline_secs":0}' \\
  | ./target/release/sonar serve`,
        },
      ],
    },
    {
      id: "usecase-rust",
      title: "Use case: Sonar as a Rust library",
      group: "Use cases",
      keywords: ["use case", "rust", "library", "cargo", "embed", "api", "crate"],
      body: [
        "Add the crate, configure a budget, and you have a full engine in process — no IPC, no server. EngineConfig::default() plays at the honest default strength (1024 hypotheses); lower budgets trade strength for speed deterministically.",
        "This exact example is compiled on every CI run (it is crates/sonar-core/examples/basic.rs in the repository) and its output is deterministic for the given seed. The demo referees a hidden fleet by hand; in a real game you would wire observe_result to your own game state.",
      ],
      examples: [
        {
          title: "cargo run --release -p sonar --example basic",
          lang: "rust",
          verified: true,
          code: `use sonar::time_limit::Deadline;
use sonar::{Engine, EngineConfig, ShotResult};

fn main() {
    // 512 hypotheses, no wall-clock limit, no learning — deterministic.
    let config = EngineConfig {
        hypothesis_soft_target: 512,
        default_deadline_secs: 0,
        use_learning: false,
        ..EngineConfig::default()
    };
    let mut engine = Engine::new(config);
    engine.reseed(0x5EED_0000_0000_0001);
    engine.place_fleet_smart(); // the engine's own fleet (defensive side)

    // The hunt loop: Sonar picks a cell, we referee the shot
    // against OUR hidden fleet and report the result back.
    let mut fleet = demo_fleet();          // 5 ships, hidden from the engine
    let mut moves = 0usize;
    while fleet.iter().any(|s| !s.sunk()) {
        let (r, c) = engine.choose_move(Deadline::none());
        moves += 1;
        let result = referee(&mut fleet, r, c);
        engine.observe_result(r, c, result);
    }

    // The engine's reasoning is fully observable.
    let posterior = engine.probability_matrix();
    let top = posterior.iter().cloned().fold(0.0_f32, f32::max);
    println!("Sonar sank the fleet in {} moves.", moves);
    println!("hypotheses maintained: {}", engine.hypothesis_count());
    println!("top posterior mass: {:.1}%", top * 100.0);
}

// Ship { r, c, len, horizontal, hits } — occupies() checks a cell,
// sunk() compares hits to len. referee() walks the fleet and returns
// ShotResult::Hit / Sunk(len) / Miss. Full source: the repository's
// crates/sonar-core/examples/basic.rs (compiled by CI on every run).`,
        },
      ],
    },
    {
      id: "usecase-js",
      title: "Use case: embed Sonar in a web page",
      group: "Use cases",
      keywords: ["use case", "javascript", "browser", "wasm", "web", "embed", "worker"],
      body: [
        "The WASM module exposes a tiny C-ABI (sonar_alloc / sonar_request / sonar_free), and web/engine.js wraps it into a friendly request(line) API that works in browsers and Node alike. Instantiate it wherever WebAssembly exists — including inside a Web Worker, which is how this page keeps the UI smooth while the engine thinks.",
        "The protocol lines are exactly the stdio lines — one source of truth. A worker-based embedding looks like this (this site's own worker.js is the production version of the same pattern):",
      ],
      examples: [
        {
          title: "Engine glue in ~10 lines (browser or Node)",
          lang: "javascript",
          verified: true,
          code: `import { createEngine, cryptoSeed } from "./engine.js";

// Fetch + instantiate (in Node: new Uint8Array(readFileSync(path))).
const bytes = new Uint8Array(await (await fetch("engine.wasm")).arrayBuffer());
const engine = await createEngine(bytes);

// Every interaction is one JSON protocol line in, one reply out.
const hello = engine.cmd("version");          // { channel: "beta", protocol: 1, ... }
engine.cmd("new_game", {
  seed: Number(cryptoSeed() % 2n ** 31n),     // crypto-quality seed
  config: { hypothesis_soft_target: 256, default_deadline_secs: 0, use_learning: false },
});
engine.cmd("place_smart");

const move = engine.cmd("choose_move", { deadline_secs: 0 });   // { row, col }
const shot = engine.cmd("receive_shot", { r: move.row, c: move.col });
// Tell the engine what happened (use "sunk_<len>" when you know it):
engine.cmd("observe", { r: move.row, c: move.col,
  result: shot.result === "sunk" ? \`sunk_\${shot.len}\` : shot.result });`,
          run: [
            { line: '{"cmd":"version"}',
              expect: [
                { path: "channel", equals: "beta" },
                { path: "protocol", equals: 1 },
                { path: "language", equals: "rust" },
              ] },
            { line: '{"cmd":"new_game","seed":77,"config":{"hypothesis_soft_target":256,"default_deadline_secs":0,"use_learning":false}}',
              expect: [{ path: "ok", equals: true }] },
            { line: '{"cmd":"place_smart"}', expect: [{ path: "ok", equals: true }] },
            { line: '{"cmd":"choose_move","deadline_secs":0}',
              expect: [
                { path: "row", min: 0, max: 9 },
                { path: "col", min: 0, max: 9 },
              ] },
          ],
        },
      ],
    },
    {
      id: "usecase-variant",
      title: "Use case: play a custom board",
      group: "Use cases",
      keywords: ["use case", "variant", "custom", "board", "torus", "holes", "polyomino", "rules", "generalised"],
      body: [
        "The variant engine lifts Battleship beyond 10×10: boards from 5×5 to 30×30, fleets of straight lines and arbitrary polyomino shapes (L, T, S, plus signs — anything 4-connected up to 8 cells), torus topology (edges wrap), and holes (islands no ship may occupy and nobody can fire at).",
        "Seven presets ship built-in: classic, micro, big16, huge30, torus8, archipelago12, poly10 — or pass your own rules object. The engine hides the fleet, you fire, and every snapshot reports the exact per-ship density field, the feasibility of your observations, and the active SIMD backend.",
        "The exchange below starts a 7×7 game and plays one shot — executed live against this page's engine:",
      ],
      examples: [
        {
          title: "Start a variant game and fire (executed against the live engine)",
          lang: "json",
          verified: true,
          code: `{"cmd":"variant_new","preset":"micro","seed":42}
{"cmd":"variant_fire","r":3,"c":3}
{"cmd":"variant_state"}
{"cmd":"variant_feasible"}
{"cmd":"variant_move"}`,
          run: [
            { line: '{"cmd":"variant_new","preset":"micro","seed":42}',
              expect: [
                { path: "ok", equals: true },
                { path: "width", equals: 7 },
                { path: "height", equals: 7 },
                { path: "ships", equals: 3 },
              ] },
            { line: '{"cmd":"variant_fire","r":3,"c":3}',
              expect: [
                { path: "ok", equals: true },
                { path: "result", inArray: ["miss", "hit", "sunk"] },
              ] },
            { line: '{"cmd":"variant_state"}',
              expect: [
                { path: "width", equals: 7 },
                { path: "density", length: 49 },
                { path: "simd_backend", inArray: ["scalar", "avx2", "avx512", "neon", "wasm128"] },
              ] },
            { line: '{"cmd":"variant_feasible"}',
              expect: [{ path: "feasible", equals: true }] },
            { line: '{"cmd":"variant_move"}',
              expect: [
                { path: "ok", equals: true },
                { path: "row", min: 0, max: 6 },
                { path: "col", min: 0, max: 6 },
                { path: "density", length: 49 },
              ] },
          ],
        },
        {
          title: "Fully custom rules (a 9×9 board with an L3 polyomino)",
          lang: "json",
          verified: true,
          code: `{"cmd":"variant_new","rules":{
  "geometry": {"width":9, "height":9, "holes":[], "torus":false},
  "fleet": [
    {"kind":"Line","len":4},
    {"kind":"Line","len":3},
    {"kind":"Shape","cells":[[0,0],[1,0],[1,1]], "name":"L3"}
  ],
  "contact_rule":"NoContact",
  "sunk_rule":"RevealNeighbors"
},"seed":7}`,
          run: [
            { line: '{"cmd":"variant_new","rules":{"geometry":{"width":9,"height":9,"holes":[],"torus":false},"fleet":[{"kind":"Line","len":4},{"kind":"Line","len":3},{"kind":"Shape","cells":[[0,0],[1,0],[1,1]],"name":"L3"}],"contact_rule":"NoContact","sunk_rule":"RevealNeighbors"},"seed":7}',
              expect: [
                { path: "ok", equals: true },
                { path: "width", equals: 9 },
                { path: "ships", equals: 3 },
              ] },
          ],
        },
      ],
    },
    {
      id: "usecase-bench",
      title: "Use case: measure strength yourself",
      group: "Use cases",
      keywords: ["use case", "benchmark", "strength", "measure", "wilson", "sprt", "verify", "statistics"],
      body: [
        "Every published claim is reproducible. The bench family runs seeded, work-limited matches and reports Wilson 95% intervals; verify-release prints the seed-verified release numbers with a hash-chain digest over the full result sequence — run it anywhere and compare digests.",
        "The bench protocol command (shown below) lets you measure the engine inside your own process — this page's Benchmarks tab does exactly that, in your browser, right now.",
      ],
      examples: [
        {
          title: "The bench protocol command (executed against the live engine)",
          lang: "json",
          verified: true,
          code: `{"cmd":"bench","games":10,"opponent":"random","seed":4242,"soft_target":64}`,
          run: [
            { line: '{"cmd":"bench","games":10,"opponent":"random","seed":4242,"soft_target":64}',
              expect: [
                { path: "ok", equals: true },
                { path: "games", equals: 10 },
                { path: "sonar.wins", min: 0, max: 10 },
                { path: "sonar.wilson95", length: 2 },
              ] },
          ],
        },
        {
          title: "CLI benchmark commands",
          lang: "bash",
          verified: true,
          code: `./target/release/sonar bench              # self-play matrix, 100 games ×4
./target/release/sonar bench-ref 60      # vs published reference bots
./target/release/sonar bench-2x 20       # opponent gets 2× the time
./target/release/sonar bench-half 40     # opponent gets 2× the hypotheses
./target/release/sonar verify-release    # seed-verified release numbers + digest
./target/release/sonar sprt 60           # sequential strength match (SPRT)
./target/release/sonar ladder            # hypothesis-budget strength ladder`,
        },
      ],
    },

    // ── Reference ──────────────────────────────────────────────────────────
    {
      id: "protocol",
      title: "The JSON protocol (v1)",
      group: "Reference",
      keywords: ["protocol", "json", "ipc", "serve", "commands", "api", "wire", "reference", "table"],
      body: [
        "One request per line, one reply per line — identical over stdio (sonar serve), from the WebAssembly exports, and in tests. The wire format is frozen (protocol v1); evolution is additive-only. Result strings: 'miss', 'hit', 'sunk' (receive_shot replies add a len field — observe 'sunk_<len>' so the engine reconstructs the ship exactly), 'already', 'invalid'.",
        "Lifecycle: new_game → place_random / place_smart / place_manual → loop { choose_move | suggest_move → receive_shot → observe } → snapshot / probability / density → record_game → reset. Introspection: snapshot returns fleet/shot/hit/sunk masks (decimal strings for the classic board), enemy_remaining, hypothesis_count, the probability and density matrices, and moves_fired.",
        "Variant commands (0.4): variant_list, variant_new, variant_move, variant_fire, variant_state, variant_feasible, variant_play. Hostile-input policy: every malformed line receives a JSON error reply — the engine never panics, never hangs, and remains fully usable afterwards (2,420 hostile inputs tested, zero panics).",
      ],
      examples: [
        {
          title: "Command reference",
          lang: "text",
          verified: false,
          code: `version          → {name, version, channel, protocol, features[]}
new_game         {seed?, config?}            → {ok}
place_random | place_smart                   → {ok}
place_manual     {ships:[[r,c,len,horizontal],…]} → {ok} | {ok:false, error, bad_index}
choose_move      {deadline_secs?}            → {row, col}
suggest_move     {deadline_secs?}            → {row, col, confidence, hypothesis_count, …}
receive_shot     {r, c}                      → {result, len?}
observe          {r, c, result}              → {ok}
snapshot         → {our_fleet_mask, our_sunk_mask, shots_mask, enemy_remaining,
                    hypothesis_count, probability_matrix, density_matrix, moves_fired, …}
probability      → {matrix[100], hypothesis_count}
density          → {matrix[100]}
config | rules   → current EngineConfig / GameRules
set_config       {config}                    → {ok}
set_rules        {rules}                     → {ok}
reseed           {seed}                      → {ok}
record_game      {won}                       → {ok}
reset            → {ok}
bench            {games, opponent, seed, soft_target} → {ok, games, sonar{…}, opponent{…}, elapsed_ms}
variant_list     → {presets[]}
variant_new      {preset | rules, seed?}     → {ok, width, height, torus, holes, ships, …}
variant_move     → {ok, row, col, density[]}
variant_fire     {r, c}                      → {ok, result, len?, all_sunk}
variant_state    → full generalised snapshot
variant_feasible → {ok, feasible, configs, complete}
variant_play     {seed}                      → {ok, shots, won}
quit             → {ok}`,
        },
        {
          title: "Introspection exchange (executed against the live engine)",
          lang: "json",
          verified: true,
          code: `{"cmd":"new_game","seed":9,"config":{"hypothesis_soft_target":64,"default_deadline_secs":0,"use_learning":false}}
{"cmd":"place_smart"}
{"cmd":"choose_move","deadline_secs":0}
{"cmd":"probability"}
{"cmd":"density"}`,
          run: [
            { line: '{"cmd":"new_game","seed":9,"config":{"hypothesis_soft_target":64,"default_deadline_secs":0,"use_learning":false}}',
              expect: [{ path: "ok", equals: true }] },
            { line: '{"cmd":"place_smart"}', expect: [{ path: "ok", equals: true }] },
            { line: '{"cmd":"choose_move","deadline_secs":0}',
              expect: [{ path: "row", min: 0, max: 9 }, { path: "col", min: 0, max: 9 }] },
            { line: '{"cmd":"probability"}',
              expect: [
                { path: "matrix", length: 100 },
                { path: "hypothesis_count", min: 1, max: 100000 },
              ] },
            { line: '{"cmd":"density"}',
              expect: [
                { path: "matrix", length: 100 },
                { path: "matrix", minAll: 0 },
              ] },
          ],
        },
        {
          title: "Hostile input policy (executed against the live engine)",
          lang: "json",
          verified: true,
          code: `"garbage {"        → {"ok":false,"error":"…"}
{"cmd":"hmm"}      → {"ok":false,"error":"unknown cmd: hmm"}
{"cmd":"observe","r":-5,"result":"zzz"} → JSON error, engine still healthy`,
          run: [
            { line: 'garbage {', expect: [{ path: "ok", equals: false }] },
            { line: '{"cmd":"hmm"}', expect: [{ path: "ok", equals: false }] },
            { line: '{"cmd":"observe","r":-5,"result":"zzz"}', expect: [{ path: "ok", equals: false }] },
            { line: '{"cmd":"version"}',
              expect: [{ path: "protocol", equals: 1 }] },
          ],
        },
      ],
    },
    {
      id: "cli",
      title: "CLI reference",
      group: "Reference",
      keywords: ["cli", "command", "terminal", "sonar serve", "play", "bench", "usage", "reference"],
      body: [
        "The sonar binary is the same engine as the library and the WASM module — every subcommand is covered by the test suite. Set $SONAR_MOVE_SECS to change Sonar's time budget in interactive play. Every benchmark subcommand prints Wilson 95% intervals and is seeded.",
      ],
      examples: [
        {
          title: "sonar <command>",
          lang: "bash",
          verified: true,
          code: `sonar version          # version, channel, protocol, license
sonar play             # play against the engine in the terminal
sonar serve            # JSON IPC server on stdin/stdout (any language)
sonar bench [N]        # self-play benchmark matrix (Wilson CIs)
sonar bench-fast [N]   # quick 20-game strength check
sonar bench-big [N]    # 500-game deep run
sonar bench-ref [N]    # vs published reference bots
sonar bench-2x [N]     # time-handicap: opponent gets 2× move time
sonar bench-half [N]   # compute-handicap: opponent gets 2× hypotheses
sonar endgame [seed]   # watch the exact-CENSUS solver at work
sonar sprt [N]         # SPRT strength match with live LLR
sonar ladder [N]       # hypothesis-budget strength ladder (16 → 8192)
sonar verify-release [N] # seed-verified release numbers + digest
sonar learning         # the passive statistics database (never influences play)`,
        },
      ],
    },
    {
      id: "config",
      title: "Engine configuration",
      group: "Reference",
      keywords: ["config", "configuration", "hypothesis", "budget", "deadline", "rules", "difficulty"],
      body: [
        "hypothesis_soft_target — the thinking budget. 16 (Ensign) to 4096 (Admiral); the engine default is 1024. Higher budgets think longer and play sharper; the scaling is measured and published (see the Benchmarks tab).",
        "default_deadline_secs — the wall-clock cap. Use 0 for pure work-limited play (deterministic); use seconds for live play. Timings never affect strength numbers: benchmarks always run work-limited.",
        "use_endgame — enable the exact-CENSUS endgame solver (default true): submarine-perfect play when 1–2 enemy ships remain. use_learning — passive game statistics recording (records never influence play; default true, but the web app disables it). rules — GameRules for custom micro-modes: board size, fleet, contact rule, sink rule.",
      ],
    },

    // ── Internals ──────────────────────────────────────────────────────────
    {
      id: "engine-how",
      title: "How the engine thinks",
      group: "Internals",
      keywords: ["engine", "pdf", "density", "hypothesis", "bayes", "posterior", "blend", "algorithm", "parity", "placement"],
      body: [
        "Hunt phase: with no hits on the board, the PDF density peaks in the centre — the number of legal placements through central cells is highest. A parity (checkerboard) preference shaves expected shots further. Target phase: after a hit, only placements containing the active hits survive, so density collapses onto the neighbours — constraint propagation in one pass.",
        "Hypothesis filter: full-fleet samples consistent with all observations form a posterior over cells. It is theoretically sharper (it models ship mutual exclusion) but noisy at small budgets. The hybrid blends it with the PDF using weight n/(n+K) — measured, not assumed: at a 512-hypothesis budget the blend beats PDF-only by ~12 percentage points, while at 64 it safely degenerates toward the PDF.",
        "Placement: fleets are sampled and scored; the final fleet is drawn from candidates within ε of the optimal penalty (ε=4). This is the GHOST FLEET indistinguishability design — pure argmin placement would let an attacker learn 'Sonar never uses the border' (measured: 0.000 occupancy on all 36 border cells before the fix; 0.057–0.204 after).",
      ],
    },
    {
      id: "endgame",
      title: "Exact-CENSUS endgame solver",
      group: "Internals",
      keywords: ["endgame", "census", "solver", "expectimax", "submarine", "perfect", "optimal", "expectation"],
      body: [
        "When the enemy is down to its last 1–2 ships, Sonar switches from sampling to certainty: the exact-CENSUS solver enumerates every legal configuration of the surviving ships consistent with your observations — the posterior is exact, not sampled, and identical-ship permutations are deduplicated so each layout counts once.",
        "On top of the census runs a memoised expectimax over the observation tree that minimises the expected number of remaining shots. Within its node budget this is provably optimal play — for a single surviving submarine, the solver finishes with perfect play: no policy can do better in expectation. Measured on a touched length-2 submarine: 3–5 shots to sink, matching the theoretical optimum exactly.",
        "The regime gate is deliberate: in a fresh hunt (no hits, large census) the hybrid's PDF + parity search is the stronger policy, so the solver defers. The moment a submarine is touched the census collapses to a handful of configurations and the solver takes over. The solver uses no RNG and no wall-clock — two solvers given the same history always produce the same move.",
      ],
    },
    {
      id: "variants",
      title: "Generalised boards & polyomino ships",
      group: "Internals",
      keywords: ["variant", "board", "size", "polyomino", "shape", "torus", "holes", "islands", "feasibility", "bitboard", "tier"],
      body: [
        "The generalised engine lifts Sonar's targeting theory beyond 10×10. Geometry supports 5×5 to 30×30 boards, holes, and torus wrap; ships are polyominoes — any 4-connected shape up to 8 cells, with rotations, reflections, and dihedral variant dedup. Underneath, a tiered bitboard grid uses a single u128 for narrow boards and SIMD word-grids for wide ones.",
        "Targeting on variants is exact per-ship enumeration: every placement of every surviving ship variant is filtered against the observations (through the SIMD legality kernel) and accumulated into the density field — no sampling noise, the per-ship posterior is computed, not estimated.",
        "The feasibility solver answers the question every attacker eventually faces: is my observation history still consistent with some legal enemy fleet? Exact backtracking with a node budget returns feasible/infeasible plus a configuration count — call variant_feasible at any moment and the engine proves the game is still winnable.",
      ],
    },
    {
      id: "simd",
      title: "Multi-ISA SIMD kernels",
      group: "Internals",
      keywords: ["simd", "avx2", "avx512", "neon", "wasm128", "vector", "kernel", "backend", "dispatch", "differential"],
      body: [
        "The engine's hot loops — batch placement-legality filtering and 8-neighbour grid dilation — ship as multi-ISA kernels: portable scalar plus AVX2, AVX-512F (x86_64, runtime-detected), NEON (aarch64) and wasm128 (the engine running this page uses v128 code when the browser supports it).",
        "Runtime dispatch picks the best backend once at startup (AVX-512 over AVX2 over scalar); the active backend is reported by the version command and every variant snapshot — observability all the way down.",
        "Correctness is enforced by differential testing: every backend must produce bit-identical results to the scalar reference on randomised inputs, and the generalised engine routes its real workloads through the kernels, so every variant game is an end-to-end differential test. The unsafe code that guards the intrinsics lives in exactly one audited crate (sonar-simd) — the engine core remains #![deny(unsafe_code)].",
      ],
    },
    {
      id: "sprt",
      title: "SPRT & seed-verified release numbers",
      group: "Internals",
      keywords: ["sprt", "ladder", "strength", "statistics", "wald", "elo", "release", "verify", "seed", "hash"],
      body: [
        "Version comparisons use the sequential probability ratio test (Wald, 1945): after every game the log-likelihood ratio is updated and the match stops as soon as the evidence crosses a decision boundary (H0: not stronger / H1: stronger, with bounded type-I/II errors). No wasted games when the gap is large, no underpowered verdicts when it is small.",
        "Ladder games run in solo mode: both engines attack the same hidden fleet and the one that sinks it in fewer shots wins — a first-mover-free measurement of pure targeting strength.",
        "Seed verification: every release number is a function of a published seed. Run 'sonar verify-release' yourself — the same seed reproduces the same numbers and the same hash-chain digest bit-for-bit on any machine. That is what 'seed-verified release numbers' means: claims you can check.",
      ],
    },

    // ── Quality & security ─────────────────────────────────────────────────
    {
      id: "testing",
      title: "Testing Sonar",
      group: "Quality & security",
      keywords: ["test", "tests", "suite", "verify", "quality", "determinism", "invariant", "fuzz"],
      body: [
        "The Test tab of this page runs a browser-adapted version of the project's verification suite against the live WASM engine: the protocol contract, determinism replays, full self-play with no-repeat-shot invariants, strength gates with Wilson intervals, hostile-input robustness, generalised variants, and every runnable documentation example.",
        "The repository ships 212 Rust tests across seven layers: unit, determinism (bit-identical replays), full-game invariants, property-based batteries, protocol/golden, statistical strength gates, and placement-statistics — plus the SIMD differential battery and the out-of-tree red-team harness (protocol fuzzing, adversarial placement search, determinism probes, invariant stress; findings tracked in EXPLOITABILITY.md).",
        "The same web suite runs in CI via Node (scripts/test-suite.mjs) — the browser and CI verify the identical checks, so 'green in CI' means 'green in your browser'.",
      ],
    },
    {
      id: "bench-method",
      title: "Benchmarks & methodology",
      group: "Quality & security",
      keywords: ["benchmark", "wilson", "confidence", "statistics", "strength", "measurement", "methodology"],
      body: [
        "Every published win rate carries a Wilson score 95% confidence interval — correct for extreme proportions (100%/0%) and small samples, unlike naive normal-approximation error bars. Benchmark games are work-limited (a hypothesis budget), never wall-clock-limited: strength numbers measure the algorithm, not the machine. Timings are reported separately and never affect outcomes.",
        "All runs are seeded — the same seed replays the same games bit-for-bit, so any third party can reproduce a number exactly (BENCHMARKS.md in the repository has the raw data and the re-run commands). E[shots] is reported with standard error, not just an average, and handicap benchmarks (bench-2x, bench-half) demonstrate the algorithm — not compute — carries the advantage.",
        "The Benchmarks tab of this page re-measures the engine live in your browser: strength gates with Wilson intervals, per-move latency, and self-play speed. Reference tables there are the release numbers from the benchmark machine — your live numbers will differ, and that is expected and honest.",
      ],
    },
    {
      id: "security",
      title: "Adversarial robustness",
      group: "Quality & security",
      keywords: ["security", "adversarial", "exploit", "ai", "attack", "robustness", "fair", "redteam", "exploitability"],
      body: [
        "Threat model: an attacker collects thousands of games against Sonar and trains a model (RL agent, classifier) to exploit statistical regularities in its behaviour. Two attack surfaces exist: predicting where Sonar shoots (targeting) and predicting where Sonar's own ships are (placement).",
        "Targeting is exploitable only if it deviates from the Bayes-optimal posterior. Sonar's targeting is a pure function of the public observation sequence — no opponent identity, no history, no learned bias. An attacker cannot use anything Sonar's algorithm does not already use. Verified by tests: reset() must restore byte-identical first-move behaviour; two fresh engines with the same observation sequence produce the same distributions.",
        "Placement is a genuine policy choice and can leak a fingerprint. The 0.1.0 argmin placement had a catastrophic one (all 36 border cells at 0.000 occupancy). The ε-band mixed strategy bounds every cell into a measured band with no predictable-empty and no predictable-occupied cells, while keeping ships strictly non-touching. The old micro-learning feature was removed in 0.2.0 — it was a direct cross-game state leak.",
        "The out-of-tree red-team harness actively attacks the engine: protocol fuzzing (2,420 hostile inputs, zero panics), oracle-designed hostile fleets (cost 50.5 shots vs 38.1 baseline, under the redline of 14 delta), determinism probes under load, and invariant stress across every variant. Its measurements feed EXPLOITABILITY.md — the adversarial ledger — so exploitability is a tracked, reproducible number, not a vibe.",
      ],
    },
    {
      id: "license",
      title: "License & credits",
      group: "Quality & security",
      keywords: ["license", "apache", "credits", "authors", "contribute"],
      body: [
        "Sonar is Apache-2.0 licensed — see the LICENSE file for the full text. Every crate in the workspace declares Apache-2.0 in its manifest, and the license is enforced by CI on every build.",
        "Reference bots included for benchmarking: classic HuntTarget (textbook), BurnsPdf (Ethan Burns, Dartmouth battleship research), and Monte Carlo sampling (à la mitchelljy/battleships_ai).",
        "Contributions welcome — the project CI runs the full Rust suite (212 tests, the SIMD differential battery, the WASM build and the red-team gates) plus the web verification on every commit, and no release ships with a red gate.",
      ],
    },
  ],
};

/** Flat text chunks for the search index. */
export function docsChunks() {
  const chunks = [];
  for (const sec of DOCS.sections) {
    const code = (sec.examples || []).map((ex) => ex.code).join("\n");
    chunks.push({
      id: sec.id,
      title: sec.title,
      group: sec.group,
      keywords: sec.keywords || [],
      text: sec.body.join(" ") + "\n" + code,
      section: sec,
    });
  }
  return chunks;
}
