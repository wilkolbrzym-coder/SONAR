/**
 * CI verification of the documentation examples (web/docs-data.js).
 *
 * Executes every runnable example against the real WASM engine and
 * checks all documented assertions — the same verifier the browser
 * Test tab uses (web/docs-verify.js). The docs never show code that
 * has not been run.
 *
 * Usage: node scripts/test-docs.mjs
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createEngine } from "../web/engine.js";
import { verifyDocsExamples } from "../web/docs-verify.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const wasmPath = path.join(here, "..", "target/wasm32-unknown-unknown/release/sonar_wasm.wasm");

const bytes = new Uint8Array(readFileSync(wasmPath));
const makeEngine = () => createEngine(bytes);

const { results, summary } = await verifyDocsExamples(makeEngine);

let failures = 0;
for (const r of results) {
  const tag = r.ok ? "PASS" : "FAIL";
  if (!r.ok) failures++;
  console.log(`[${tag}] docs:${r.section} — ${r.title}${r.detail ? " — " + r.detail : ""}`);
}

console.log("");
console.log(`docs examples: ${summary.passed}/${summary.total} passed`);
if (failures > 0 || !summary.allOk) {
  process.exit(1);
}
