/**
 * Node verification of the mod runtime (web/mods.js) and example mods.
 *
 * Runs the example mods against the real WASM engine (two instances:
 * opponent + analysis) and asserts the arena completes cleanly with
 * sane statistics. This is the same code path the browser "Mods" tab uses.
 *
 * Usage: node scripts/test-mods.mjs
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createEngine } from "../web/engine.js";
import { evalMod, runModArena, playModGame, EXAMPLE_MODS } from "../web/mods.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const wasmPath = path.join(here, "..", "target/wasm32-unknown-unknown/release/sonar_wasm.wasm");

const bytes = new Uint8Array(readFileSync(wasmPath));
const makeEngine = () => createEngine(bytes);

let failures = 0;
function check(name, cond, detail = "") {
  const tag = cond ? "PASS" : "FAIL";
  if (!cond) failures++;
  console.log(`[${tag}] ${name}${detail ? " — " + detail : ""}`);
}

// ── evalMod ─────────────────────────────────────────────────────────────────

check("evalMod rejects empty source", (() => {
  try {
    evalMod("// nothing here");
    return false;
  } catch {
    return true;
  }
})());

for (const [key, ex] of Object.entries(EXAMPLE_MODS)) {
  let mod = null;
  try {
    mod = evalMod(ex.source);
  } catch (e) {
    check(`evalMod ${key}`, false, String(e.message || e));
    continue;
  }
  check(
    `evalMod ${key}`,
    mod && typeof mod.chooseMove === "function",
    `name=${mod.name}`
  );

  // One game, verbose error surfacing.
  const engineA = await makeEngine();
  const engineB = await makeEngine();
  const rep = playModGame(mod, engineA, engineB, { seed: 555, softTarget: 64 });
  check(
    `game completes: ${key}`,
    rep.winner === "mod" || rep.winner === "engine",
    `winner=${rep.winner} moves=${rep.moves} forfeited=${rep.forfeited} errors=${rep.modErrors.length}`
  );
  check(
    `no illegal-move forfeits: ${key}`,
    !rep.forfeited,
    rep.modErrors.join("; ")
  );
}

// ── Arena: parity hunter vs engine (sanity stats) ──────────────────────────

const mod = evalMod(EXAMPLE_MODS.parityHunter.source);
const arena = runModArena(mod, await makeEngine(), await makeEngine(), {
  games: 10,
  softTarget: 64,
  seed: 9001,
});
check(
  "arena totals add up",
  arena.total === 10 && arena.modWins + arena.engineWins === 10,
  `mod ${arena.modWins}/10, CI [${arena.wilson95[0].toFixed(1)}, ${arena.wilson95[1].toFixed(1)}]`
);
check("arena records zero errors", arena.errors.length === 0, arena.errors.slice(0, 2).join("; "));

// ── Posterior Sniper should be the strongest example (uses sonar data) ────

const sniper = evalMod(EXAMPLE_MODS.posteriorSniper.source);
const arenaSniper = runModArena(sniper, await makeEngine(), await makeEngine(), {
  games: 10,
  softTarget: 64,
  seed: 9002,
});
console.log(
  `        sniper vs sonar(64): ${arenaSniper.modWins}/10 wins (CI [${arenaSniper.wilson95[0].toFixed(1)}, ${arenaSniper.wilson95[1].toFixed(1)}])`
);

// ── Summary ────────────────────────────────────────────────────────────────

console.log("");
if (failures > 0) {
  console.error(`mods runtime: FAILED (${failures} checks)`);
  process.exit(1);
}
console.log("mods runtime: all checks passed");
