/**
 * Sonar Web Worker — hosts the WASM engine, tests, and live benchmarks.
 *
 * The main thread does rendering and message passing only; every
 * computation happens here, so the UI never blocks — even with the
 * engine thinking at full speed.
 *
 * Message protocol (main → worker):
 *   {id, type: "hello"}                       → {id, ok, version, hello}
 *   {id, type: "cmd", line: string}           → {id, ok, reply}   // JSON protocol passthrough
 *   {id, type: "livebench"}
 *        → {id, type: "bench-progress", label}*
 *        → {id, ok, report} | {id, ok: false, error}
 *   {id, type: "testsuite"}
 *        → {id, type: "test-progress", entry}*
 *        → {id, ok, results, summary}
 */

import { createEngine, cryptoSeed } from "./engine.js";
import { runTestSuite } from "./tests.js";
import { runLiveBench } from "./bench-live.js";

const WASM_URL = new URL("engine.wasm", import.meta.url).href;

// ── Engine pool ─────────────────────────────────────────────────────────────
// One engine instance per concurrent use. The main game uses engines[0];
// tests and benchmarks create their own (the wasm Module is shared, each
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
        // `slot` selects the engine instance: 0 = duel/advisor (classic),
        // 1 = solo hunt (variant state persists across tab switches).
        const e = await getEngine(Number(msg.slot) || 0);
        const reply = e.requestRaw(String(msg.line || ""));
        post({ id, ok: true, reply });
        break;
      }

      case "livebench": {
        const report = await runLiveBench(makeFreshEngine, (p) => {
          post({ id, type: "bench-progress", label: p.label, stage: p.stage });
        });
        post({ id, ok: true, report });
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

// Expose the crypto seed helper for tools that want it.
export { cryptoSeed };
