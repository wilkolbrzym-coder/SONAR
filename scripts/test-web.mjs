/**
 * End-to-end verification of the WebAssembly engine in Node.js.
 *
 * Loads the compiled sonar_wasm.wasm, drives the full JSON protocol
 * (complete games included), and prints a pass/fail summary. This is the
 * same engine + protocol the browser app uses — so passing here means the
 * web app's core is verified outside any browser.
 *
 * Usage: node scripts/test-web.mjs
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createEngine, cryptoSeed } from "../web/engine.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const wasmPath = path.join(here, "..", "target/wasm32-unknown-unknown/release/sonar_wasm.wasm");

const results = [];
function check(name, cond, detail = "") {
  results.push({ name, ok: !!cond, detail });
  const tag = cond ? "PASS" : "FAIL";
  console.log(`[${tag}] ${name}${detail ? " — " + detail : ""}`);
}

// ── Load ─────────────────────────────────────────────────────────────────────

const bytes = readFileSync(wasmPath);
const engine = await createEngine(new Uint8Array(bytes));

check("wasm loads", true, `version ${engine.version.major}.${engine.version.minor}.${engine.version.patch}`);
check(
  "version command reports beta",
  engine.hello.channel === "beta" && engine.hello.protocol === 1,
  `channel=${engine.hello.channel} protocol=${engine.hello.protocol}`
);

// ── Protocol basics ──────────────────────────────────────────────────────────

const seed = 12345;
let r = engine.cmd("new_game", { seed, config: { use_learning: false, hypothesis_soft_target: 64, default_deadline_secs: 0 } });
check("new_game", r.ok === true);

r = engine.cmd("place_smart");
check("place_smart", r.ok === true);

r = engine.cmd("snapshot");
const fleetCells = BigInt(r.our_fleet_mask);
check(
  "snapshot fleet has 17 cells",
  countBits(fleetCells) === 17,
  `mask bits = ${countBits(fleetCells)}`
);
check("snapshot has density matrix", Array.isArray(r.density_matrix) && r.density_matrix.length === 100);

// Deterministic replay: same seed → same first move.
const first1 = engine.cmd("choose_move", { deadline_secs: 0 });
engine.cmd("new_game", { seed, config: { use_learning: false, hypothesis_soft_target: 64, default_deadline_secs: 0 } });
engine.cmd("place_smart");
const first2 = engine.cmd("choose_move", { deadline_secs: 0 });
check(
  "deterministic first move per seed",
  first1.row === first2.row && first1.col === first2.col,
  `(${first1.row},${first1.col}) vs (${first2.row},${first2.col})`
);

// ── A full game driven through the protocol ────────────────────────────────

// The engine plays Sonar's side: it targets "our" fleet (tracked here in
// JS) and receives our shots against its own fleet.
engine.cmd("new_game", { seed: 987, config: { use_learning: false, hypothesis_soft_target: 64, default_deadline_secs: 0 } });
engine.cmd("place_smart");

// Our (human) fleet, mirrored as a JS board.
const ourShips = [
  { r: 0, c: 0, len: 5, horizontal: true },
  { r: 2, c: 0, len: 4, horizontal: true },
  { r: 4, c: 0, len: 3, horizontal: true },
  { r: 6, c: 0, len: 3, horizontal: true },
  { r: 8, c: 0, len: 2, horizontal: true },
];
const shipCells = [];
for (const s of ourShips) {
  const cells = new Set();
  for (let i = 0; i < s.len; i++) {
    cells.add(s.horizontal ? s.r * 10 + s.c + i : (s.r + i) * 10 + s.c);
  }
  shipCells.push({ cells, hits: 0, len: s.len, sunk: false });
}
const ourShotsAt = new Set();

function resolveSonarShot(r, c) {
  const idx = r * 10 + c;
  if (ourShotsAt.has(idx)) return "already";
  ourShotsAt.add(idx);
  for (const ship of shipCells) {
    if (ship.cells.has(idx)) {
      ship.hits += 1;
      if (ship.hits === ship.len) {
        ship.sunk = true;
        return "sunk";
      }
      return "hit";
    }
  }
  return "miss";
}
function allOurShipsSunk() {
  return shipCells.every((s) => s.sunk);
}

let ourMoves = 0;
let sonarMoves = 0;
let winner = null;
let guard = 0;
while (winner === null && guard++ < 300) {
  const sug = engine.cmd("suggest_move", { deadline_secs: 0 });
  const res = engine.cmd("receive_shot", { r: sug.row, c: sug.col });
  engine.cmd("observe", {
    r: sug.row,
    c: sug.col,
    result: res.result === "sunk" ? `sunk_${res.len}` : res.result,
  });
  ourMoves++;
  if (res.result === "sunk") {
    const snap = engine.cmd("snapshot");
    const fleet = BigInt(snap.our_fleet_mask);
    const sunkM = BigInt(snap.our_sunk_mask);
    if (fleet !== 0n && (fleet & sunkM) === fleet) {
      winner = "human";
      break;
    }
  }

  const mv = engine.cmd("choose_move", { deadline_secs: 0 });
  const outcome = resolveSonarShot(mv.row, mv.col);
  sonarMoves++;
  if (allOurShipsSunk()) {
    winner = "sonar";
    break;
  }
}
check(
  "full protocol game terminates",
  winner !== null,
  `winner=${winner} after ${ourMoves}+${sonarMoves} moves`
);

// ── Bench command ───────────────────────────────────────────────────────────

const t0 = Date.now();
const bench = engine.cmd("bench", { games: 10, opponent: "random", seed: 4242 });
const dt = Date.now() - t0;
check(
  "bench command runs in wasm",
  bench.ok === true && bench.games === 10,
  `${bench.games} games in ${dt}ms, sonar won ${bench.sonar.wins}`
);

// ── Hostile inputs ──────────────────────────────────────────────────────────

const hostile = [
  "null",
  "[]",
  "garbage",
  '{"cmd":null}',
  '{"cmd":"place_manual","ships":"nope"}',
  '{"cmd":"observe","r":-1,"result":"zzz"}',
  '{"cmd":"bench","opponent":"geoffrey"}',
  '{"cmd":"unknown"}',
  "\u{1F980}",
];
let hostileOk = true;
for (const h of hostile) {
  try {
    const reply = engine.request(h);
    if (typeof reply !== "object" || reply === null) hostileOk = false;
  } catch (e) {
    hostileOk = false;
    console.error("   hostile input threw:", h, e.message);
  }
}
check("hostile inputs never crash", hostileOk);

const emptyReply = engine.request("");
check("empty request answered gracefully", typeof emptyReply === "object");

// ── crypto seed sanity ──────────────────────────────────────────────────────

const s1 = cryptoSeed();
const s2 = cryptoSeed();
check("crypto seeds differ", s1 !== s2, `${s1} vs ${s2}`);

// ── Summary ────────────────────────────────────────────────────────────────

const failed = results.filter((r) => !r.ok);
console.log("");
console.log(`web engine E2E: ${results.length - failed.length}/${results.length} passed`);
if (failed.length > 0) {
  console.error("FAILED:", failed.map((f) => f.name).join(", "));
  process.exit(1);
}

function countBits(v) {
  let n = 0n;
  let x = v;
  while (x) {
    x &= x - 1n;
    n++;
  }
  return Number(n);
}
