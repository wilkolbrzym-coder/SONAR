/**
 * Node verification of the in-browser test suite (web/tests.js).
 *
 * Runs exactly the suite the "Test Sonar" tab runs in the browser —
 * same code, same gates — against the real WASM engine.
 *
 * Usage: node scripts/test-suite.mjs
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createEngine } from "../web/engine.js";
import { runTestSuite } from "../web/tests.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const wasmPath = path.join(here, "..", "target/wasm32-unknown-unknown/release/sonar_wasm.wasm");

const bytes = new Uint8Array(readFileSync(wasmPath));
const makeEngine = () => createEngine(bytes);

let failures = 0;
const { summary } = await runTestSuite(makeEngine, (entry) => {
  if (entry.name === "__summary__") return;
  const tag = entry.ok ? "PASS" : "FAIL";
  if (!entry.ok) failures++;
  console.log(`[${tag}] ${entry.name}${entry.detail ? " — " + entry.detail : ""}`);
});

console.log("");
console.log(`web test suite: ${summary.passed}/${summary.total} passed`);
if (failures > 0 || !summary.allOk) {
  process.exit(1);
}
