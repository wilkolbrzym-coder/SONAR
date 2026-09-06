/**
 * Live benchmark runner — measures the real engine, in the browser.
 *
 * Runs on demand from the Benchmarks tab (via worker.js). Every
 * measurement uses a FRESH engine instance so the running games in the
 * Play tab are never touched, and every strength run is seeded (the
 * same seed → the same games).
 *
 * Measurements:
 *   1. move latency      — μs per suggest_move, mean + p95 (work-limited)
 *   2. strength vs random — 20 games, Wilson 95% CI
 *   3. strength vs pdf    — 20 games, Wilson 95% CI
 *   4. self-play speed    — 10 hybrid-vs-hybrid games, games/s + shots
 *   5. variant speed      — self-play on 4 generalised presets
 *   6. memory             — WASM heap size after the runs
 */

/**
 * @param {() => Promise<object>} makeEngine — fresh-engine factory.
 * @param {(msg: object) => void} onProgress — live progress reporter.
 * @returns {Promise<object>} structured report (see the Benchmarks tab
 *   renderer for the shape).
 */
export async function runLiveBench(makeEngine, onProgress = () => {}) {
  const report = {};

  // ── 1. move latency (work-limited, so it is deterministic strength) ──────
  onProgress({ stage: "latency", label: "Measuring per-move latency…" });
  {
    const e = await makeEngine();
    e.cmd("new_game", {
      seed: 20260907,
      config: { use_learning: false, hypothesis_soft_target: 256, default_deadline_secs: 0 },
    });
    e.cmd("place_smart");
    // Warm-up (first move materialises the hypothesis filter).
    const warm = 3;
    for (let i = 0; i < warm; i++) {
      const m = e.cmd("choose_move", { deadline_secs: 0 });
      e.cmd("observe", { r: m.row, c: m.col, result: "miss" });
    }
    const N = 40;
    const times = [];
    for (let i = 0; i < N; i++) {
      const t0 = performance.now();
      const m = e.cmd("choose_move", { deadline_secs: 0 });
      times.push(performance.now() - t0);
      e.cmd("observe", { r: m.row, c: m.col, result: "miss" });
    }
    times.sort((x, y) => x - y);
    const mean = times.reduce((s, v) => s + v, 0) / times.length;
    // p95 with a safe clamped index (N=40 → index 37).
    const p95Idx = Math.min(times.length - 1, Math.max(0, Math.floor(N * 0.95) - 1));
    report.latency = {
      samples: N,
      meanMs: round(mean, 3),
      p95Ms: round(times[p95Idx], 3),
      minMs: round(times[0], 3),
      maxMs: round(times[times.length - 1], 3),
    };
  }

  // ── 2–3. strength gates (seeded, work-limited) ──────────────────────────
  onProgress({ stage: "strength", label: "Strength gate: 20 games vs Random…" });
  {
    const e = await makeEngine();
    const b = e.cmd("bench", { games: 20, opponent: "random", seed: 4242, soft_target: 256 });
    report.vsRandom = {
      games: b.games,
      wins: b.sonar.wins,
      winRate: b.sonar.win_rate,
      wilson: b.sonar.wilson95.map((x) => round(x, 1)),
      avgMoves: round(b.sonar.avg_moves, 1),
      elapsedMs: b.elapsed_ms,
    };
  }
  onProgress({ stage: "strength2", label: "Strength gate: 20 games vs PdfOnly…" });
  {
    const e = await makeEngine();
    const b = e.cmd("bench", { games: 20, opponent: "pdf", seed: 8888, soft_target: 256 });
    report.vsPdf = {
      games: b.games,
      wins: b.sonar.wins,
      winRate: b.sonar.win_rate,
      wilson: b.sonar.wilson95.map((x) => round(x, 1)),
      avgMoves: round(b.sonar.avg_moves, 1),
      elapsedMs: b.elapsed_ms,
    };
  }

  // ── 4. self-play throughput (hybrid vs hybrid) ─────────────────────────
  onProgress({ stage: "selfplay", label: "Self-play throughput: 10 games…" });
  {
    const e = await makeEngine();
    const b = e.cmd("bench", { games: 10, opponent: "self", seed: 6060, soft_target: 256 });
    report.selfPlay = {
      games: b.games,
      elapsedMs: b.elapsed_ms,
      gamesPerSec: b.elapsed_ms > 0 ? round((b.games / b.elapsed_ms) * 1000, 2) : null,
      msPerGame: round(b.elapsed_ms / b.games, 1),
      avgMovesWinner: round(b.sonar.avg_moves, 1),
    };
  }

  // ── 5. generalised-variant self-play speed ─────────────────────────────
  report.variants = [];
  for (const preset of ["micro", "classic", "big16", "torus8"]) {
    onProgress({ stage: "variant", label: `Variant self-play: ${preset}…` });
    const e = await makeEngine();
    e.cmd("variant_new", { preset, seed: 77 });
    const games = 8;
    const t0 = performance.now();
    let shots = 0;
    let ok = true;
    try {
      for (let i = 0; i < games; i++) {
        e.cmd("variant_new", { preset, seed: 1000 + i });
        const r = e.cmd("variant_play", { seed: 2000 + i });
        if (r.ok !== true) throw new Error(r.error || "variant_play failed");
        shots += r.shots;
      }
    } catch (err) {
      ok = false;
      shots = 0;
    }
    const dt = performance.now() - t0;
    report.variants.push({
      preset,
      games,
      ok,
      avgShots: ok ? round(shots / games, 1) : null,
      msPerGame: ok && dt > 0 ? round(dt / games, 1) : null,
      gamesPerSec: ok && dt > 0 ? round((games / dt) * 1000, 2) : null,
    });
  }

  // ── 6. memory + active SIMD backend ────────────────────────────────────
  {
    const e = await makeEngine();
    report.memory = {
      heapMB: round(e.memory.buffer.byteLength / (1024 * 1024), 2),
      backend: null,
    };
    try {
      // variant_new reports the runtime-detected SIMD backend.
      const vn = e.cmd("variant_new", { preset: "micro", seed: 1 });
      report.memory.backend = vn.simd_backend || null;
    } catch {
      /* best effort */
    }
  }

  return report;
}

function round(x, digits) {
  if (typeof x !== "number" || !Number.isFinite(x)) return x;
  const f = 10 ** digits;
  return Math.round(x * f) / f;
}
