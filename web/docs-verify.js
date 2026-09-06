/**
 * Documentation example verifier — runs every runnable doc example
 * against a real Sonar engine instance.
 *
 * Shared by the browser Test tab (web/tests.js) and CI
 * (scripts/test-docs.mjs): the same code verifies the documentation in
 * both places, so "verified example" means the same thing everywhere.
 *
 * Each runnable example in DOCS.sections[].examples[] carries a `run`
 * array. Steps run in order against a FRESH engine (no state leaks
 * between examples). A step is either a protocol line (string) or:
 *
 *   { line: '{"cmd":"…"}', expect: [ rule, … ] }
 *
 * Rules (all optional fields, checked against the parsed reply):
 *   { path: "ok",              equals: true }       — deep value equality
 *   { path: "row",             min: 0, max: 9 }     — numeric range (inclusive)
 *   { path: "matrix",          length: 100 }        — array length
 *   { path: "matrix",          minAll: 0 }          — every element >= min
 *
 * `path` uses dot notation ("sonar.wins"). Malformed replies, thrown
 * errors, and failed rules all fail the example.
 */

import { DOCS } from "./docs-data.js";

/**
 * Verify a single expectation rule against a reply.
 * @returns {string|null} failure description, or null on success.
 */
function checkRule(reply, rule) {
  const value = getPath(reply, rule.path);

  if (rule.equals !== undefined) {
    if (value !== rule.equals) {
      return `${rule.path} = ${JSON.stringify(value)}, expected ${JSON.stringify(rule.equals)}`;
    }
  }
  if (rule.inArray !== undefined) {
    if (!rule.inArray.includes(value)) {
      return `${rule.path} = ${JSON.stringify(value)}, expected one of ${JSON.stringify(rule.inArray)}`;
    }
  }
  if (rule.min !== undefined || rule.max !== undefined) {
    if (typeof value !== "number" || !Number.isFinite(value)) {
      return `${rule.path} = ${JSON.stringify(value)}, expected a number`;
    }
    if (rule.min !== undefined && value < rule.min) {
      return `${rule.path} = ${value} < min ${rule.min}`;
    }
    if (rule.max !== undefined && value > rule.max) {
      return `${rule.path} = ${value} > max ${rule.max}`;
    }
  }
  if (rule.length !== undefined) {
    if (!Array.isArray(value) || value.length !== rule.length) {
      return `${rule.path} should be an array of length ${rule.length}`;
    }
  }
  if (rule.minAll !== undefined) {
    if (!Array.isArray(value) || !value.every((v) => typeof v === "number" && v >= rule.minAll)) {
      return `${rule.path} should be an array of numbers all >= ${rule.minAll}`;
    }
  }
  return null;
}

/** Resolve a dot-notation path inside an object. */
function getPath(obj, path) {
  if (!path) return undefined;
  let cur = obj;
  for (const part of path.split(".")) {
    if (cur === null || cur === undefined) return undefined;
    cur = cur[part];
  }
  return cur;
}

/**
 * Run every runnable example in the docs.
 *
 * @param {() => Promise<object>} makeEngine — factory producing a fresh
 *   engine instance (one per example).
 * @returns {Promise<{results: Array, summary: object}>}
 *   results: [{ section, title, ok, detail }] — one entry per runnable
 *   example; summary: { passed, total, allOk }.
 */
export async function verifyDocsExamples(makeEngine) {
  const results = [];

  for (const sec of DOCS.sections) {
    for (const ex of sec.examples || []) {
      if (!Array.isArray(ex.run) || ex.run.length === 0) continue;
      let ok = true;
      let detail = "";
      try {
        const engine = await makeEngine();
        for (const step of ex.run) {
          const line = typeof step === "string" ? step : step.line;
          const rules = typeof step === "string" ? [] : step.expect || [];
          const reply = engine.request(line);
          if (!reply || typeof reply !== "object") {
            ok = false;
            detail = `non-object reply to ${line.slice(0, 60)}`;
            break;
          }
          for (const rule of rules) {
            const failure = checkRule(reply, rule);
            if (failure) {
              ok = false;
              detail = `${line.slice(0, 48)}…: ${failure}`;
              break;
            }
          }
          if (!ok) break;
        }
        if (ok) {
          const steps = ex.run.length;
          detail = `${steps} protocol step${steps === 1 ? "" : "s"} executed, all assertions passed`;
        }
      } catch (e) {
        ok = false;
        detail = String((e && e.message) || e);
      }
      results.push({ section: sec.id, title: ex.title, ok, detail });
    }
  }

  const passed = results.filter((r) => r.ok).length;
  return {
    results,
    summary: { passed, total: results.length, allOk: passed === results.length },
  };
}
