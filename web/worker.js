/**
 * Sonar Web Worker — hosts the WASM engine, mods, tests, and benchmarks.
 *
 * The main thread does rendering and message passing only; every
 * computation happens here, so the UI never blocks (even with the engine
 * thinking at full speed — "full performance on your own machine").
 *
 * Message protocol (main → worker):
 *   {id, type: "hello"}                       → {id, ok, version, hello}
 *   {id, type: "cmd", line: string}           → {id, ok, reply}   // JSON protocol passthrough
 *   {id, type: "bench", games, opponent, softTarget} → {id, ok, report}
 *   {id, type: "modmatch", code, games, softTarget}
 *        → {id, type: "progress", done, total, last}*
 *        → {id, ok, report} | {id, ok: false, error}
 *   {id, type: "testsuite"}
 *        → {id, type: "test-progress", entry}*
 *        → {id, ok, results, summary}
 */

import { createEngine, cryptoSeed } from "./engine.js";
import { evalMod, runModArena } from "./mods.js";
import { runTestSuite } from "./tests.js";

const WASM_URL = new URL("engine.wasm", import.meta.url).href;

// ── Engine pool ─────────────────────────────────────────────────────────────
// One engine instance per concurrent use. The main game uses engines[0];
// mods and tests create their own (the wasm Module is shared, each
// Instance gets fresh state).

let wasmBytes = null;
const engines = [];

async function loadBytes() {
  if (wasmBytes) return wasmBytes;
  const res = await fetch(WASM_URL);
  if (!res.ok) {
    throw new Error(`failed to load engine.wasm (HTTP ${res.status})`);
  }
  wasmBytes = new Uint8Array(await res.arrayBuffer());
  return wasmBytes;
}

async function getEngine(i = 0) {
  const bytes = await loadBytes();
  while (engines.length <= i) {
    engines.push(null);
  }
  if (!engines[i]) {
    engines[i] = await createEngine(bytes);
  }
  return engines[i];
}

async function makeFreshEngine() {
  const bytes = await loadBytes();
  return createEngine(bytes);
}

// ── Message handling ────────────────────────────────────────────────────────

self.onmessage = async (ev) => {
  const msg = ev.data;
  if (!msg || typeof msg !== "object" || !msg.type) return;
  const { id } = msg;
  try {
    switch (msg.type) {
      case "hello": {
        const e = await getEngine(0);
        post({ id, ok: true, version: e.version, hello: e.hello });
        break;
      }

      case "cmd": {
        const e = await getEngine(0);
        const reply = e.requestRaw(String(msg.line || ""));
        post({ id, ok: true, reply });
        break;
      }

      case "bench": {
        const e = await getEngine(0);
        const report = e.cmd("bench", {
          games: clampInt(msg.games, 1, 5000, 20),
          opponent: ["random", "pdf", "self"].includes(msg.opponent)
            ? msg.opponent
            : "random",
          soft_target: clampInt(msg.softTarget, 8, 4096, 256),
          seed: clampInt(msg.seed, 0, Number.MAX_SAFE_INTEGER, 0x5eed),
        });
        post({ id, ok: true, report });
        break;
      }

      case "modmatch": {
        const mod = evalMod(String(msg.code || ""));
        const engineA = await makeFreshEngine();
        const engineB = await makeFreshEngine();
        const report = runModArena(mod, engineA, engineB, {
          games: clampInt(msg.games, 1, 200, 20),
          softTarget: clampInt(msg.softTarget, 8, 4096, 256),
          seed: clampInt(msg.seed, 0, Number.MAX_SAFE_INTEGER, 20260906),
          onProgress: (done, total, last) => {
            post({
              id,
              type: "progress",
              done,
              total,
              last: {
                winner: last.winner,
                moves: last.moves,
                forfeited: last.forfeited,
              },
            });
          },
        });
        post({ id, ok: true, report, modName: mod.name });
        break;
      }

      case "testsuite": {
        const { results, summary } = await runTestSuite(makeFreshEngine, (entry) => {
          post({ id, type: "test-progress", entry });
        });
        post({ id, ok: true, results, summary });
        break;
      }

      default:
        post({ id, ok: false, error: `unknown message type: ${msg.type}` });
    }
  } catch (e) {
    post({ id, ok: false, error: String(e && e.message ? e.message : e) });
  }
};

function post(obj) {
  self.postMessage(obj);
}

function clampInt(v, lo, hi, dflt) {
  const n = Number(v);
  if (!Number.isFinite(n)) return dflt;
  return Math.max(lo, Math.min(hi, Math.round(n)));
}

// Expose the crypto seed helper for tools that want it.
export { cryptoSeed };
