/**
 * Sonar in-browser test suite — the "Tests" feature.
 *
 * Environment-agnostic: given an engine factory (see `worker.js` for the
 * browser wiring and `scripts/test-suite.mjs` for Node), it runs the same
 * verification battery the project ships in CI, adapted to the browser:
 *
 *   1. protocol & version contract
 *   2. determinism (same seed → same moves)
 *   3. no-repeat-shot invariant over full self-play games
 *   4. strength gates (Wilson CIs)
 *   5. hostile-input robustness
 *   6. documentation examples (every runnable example executes + asserts)
 *   7. generalised variants (0.4): presets, a full protocol game,
 *      snapshots, the feasibility solver and hostile input
 *
 * Every test reports {name, ok, detail} through `onProgress`.
 */

import { verifyDocsExamples } from "./docs-verify.js";

/**
 * @param {() => Promise<object>} makeEngine — async factory producing a
 *   fresh engine instance (each with its own state).
 * @param {(result: object) => void} onProgress — called after every test.
 */
export async function runTestSuite(makeEngine, onProgress = () => {}) {
  const results = [];
  const t = (name, ok, detail = "") => {
    const entry = { name, ok: !!ok, detail };
    results.push(entry);
    onProgress(entry);
    return entry;
  };

  // ── 1. protocol & version ───────────────────────────────────────────────
  const e = await makeEngine();
  const hello = e.cmd("version");
  t(
    "version contract (channel=beta, protocol=1)",
    hello.channel === "beta" && hello.protocol === 1,
    `v${hello.version} protocol ${hello.protocol} · simd: ${hello.features ? "declared" : "n/a"}`
  );

  const snap = (e.cmd("place_smart"), e.cmd("snapshot"));
  const bits = bitCount(BigInt(snap.our_fleet_mask));
  t("smart placement covers exactly 17 cells", bits === 17, `${bits} cells`);

  // The hypothesis filter is lazy: it materialises on the first
  // choose_move. Ask for a move first, then inspect.
  e.cmd("choose_move", { deadline_secs: 0 });
  const prob = e.cmd("probability");
  t(
    "probability matrix has 100 entries + hypotheses",
    Array.isArray(prob.matrix) && prob.matrix.length === 100 && prob.hypothesis_count > 0,
    `hyp=${prob.hypothesis_count}`
  );

  const dens = e.cmd("density");
  t(
    "density matrix has 100 entries, non-negative",
    Array.isArray(dens.matrix) &&
      dens.matrix.length === 100 &&
      dens.matrix.every((v) => v >= 0)
  );

  // ── 2. determinism ─────────────────────────────────────────────────────
  const a = await makeEngine();
  const b = await makeEngine();
  for (const eng of [a, b]) {
    eng.cmd("new_game", {
      seed: 31337,
      config: { use_learning: false, hypothesis_soft_target: 64, default_deadline_secs: 0 },
    });
    eng.cmd("place_smart");
  }
  let same = true;
  const seqA = [];
  const seqB = [];
  // Fixed observation feedback pattern (deterministic pseudo-board).
  for (let i = 0; i < 10; i++) {
    const ma = a.cmd("choose_move", { deadline_secs: 0 });
    const mb = b.cmd("choose_move", { deadline_secs: 0 });
    seqA.push([ma.row, ma.col]);
    seqB.push([mb.row, mb.col]);
    if (ma.row !== mb.row || ma.col !== mb.col) same = false;
    // Feed a deterministic result (pattern-based).
    const res = (ma.row * 3 + ma.col * 7 + i) % 4 === 0 ? "hit" : "miss";
    a.cmd("observe", { r: ma.row, c: ma.col, result: res });
    b.cmd("observe", { r: mb.row, c: mb.col, result: res });
  }
  t(
    "same seed → identical 10-move sequence",
    same,
    same ? `first move (${seqA[0][0]},${seqA[0][1]})` : `${fmt(seqA)} vs ${fmt(seqB)}`
  );

  // ── 3. full self-play through the protocol + no-repeat invariant ────────
  const p1 = await makeEngine();
  const p2 = await makeEngine();
  for (const [eng, seed] of [
    [p1, 11],
    [p2, 22],
  ]) {
    eng.cmd("new_game", {
      seed,
      config: { use_learning: false, hypothesis_soft_target: 64, default_deadline_secs: 0 },
    });
    eng.cmd("place_smart");
  }
  const firedByP1 = new Set();
  const firedByP2 = new Set();
  let noRepeat = true;
  let over = false;
  let winnerName = null;
  let movesTotal = 0;
  for (let guard = 0; guard < 300 && !over; guard++) {
    const mv = p1.cmd("choose_move", { deadline_secs: 0 });
    const idx = mv.row * 10 + mv.col;
    if (firedByP1.has(idx)) {
      noRepeat = false;
      break;
    }
    firedByP1.add(idx);
    const res = p2.cmd("receive_shot", { r: mv.row, c: mv.col });
    p1.cmd("observe", {
      r: mv.row,
      c: mv.col,
      result: res.result === "sunk" ? `sunk_${res.len}` : res.result,
    });
    movesTotal++;
    if (res.result === "sunk") {
      const s = p2.cmd("snapshot");
      const fleet = BigInt(s.our_fleet_mask);
      const sunkM = BigInt(s.our_sunk_mask);
      if (fleet !== 0n && (fleet & sunkM) === fleet) {
        over = true;
        winnerName = "p1";
        break;
      }
    }
    const mv2 = p2.cmd("choose_move", { deadline_secs: 0 });
    const idx2 = mv2.row * 10 + mv2.col;
    if (firedByP2.has(idx2)) {
      noRepeat = false;
      break;
    }
    firedByP2.add(idx2);
    const res2 = p1.cmd("receive_shot", { r: mv2.row, c: mv2.col });
    p2.cmd("observe", {
      r: mv2.row,
      c: mv2.col,
      result: res2.result === "sunk" ? `sunk_${res2.len}` : res2.result,
    });
    movesTotal++;
    if (res2.result === "sunk") {
      const s = p1.cmd("snapshot");
      const fleet = BigInt(s.our_fleet_mask);
      const sunkM = BigInt(s.our_sunk_mask);
      if (fleet !== 0n && (fleet & sunkM) === fleet) {
        over = true;
        winnerName = "p2";
        break;
      }
    }
  }
  t(
    "self-play game terminates with a winner",
    over && winnerName !== null,
    `winner=${winnerName} in ${movesTotal} moves`
  );
  t("no side ever fires at the same cell twice", noRepeat);

  // ── 4. strength gates ───────────────────────────────────────────────────
  const bench = e.cmd("bench", { games: 20, opponent: "random", seed: 777, soft_target: 64 });
  t(
    "sonar dominates random (Wilson lower bound ≥ 70%)",
    bench.sonar.wilson95[0] >= 70,
    `${bench.sonar.wins}/${bench.games} wins, CI [${bench.sonar.wilson95[0].toFixed(1)}, ${bench.sonar.wilson95[1].toFixed(1)}]`
  );

  const bench2 = e.cmd("bench", { games: 30, opponent: "pdf", seed: 888, soft_target: 512 });
  t(
    "sonar ≥ pdf-only component (point estimate ≥ 50%)",
    bench2.sonar.win_rate >= 50,
    `${bench2.sonar.wins}/${bench2.games} vs PdfOnly (CI [${bench2.sonar.wilson95[0].toFixed(1)}, ${bench2.sonar.wilson95[1].toFixed(1)}])`
  );

  // ── 5. hostile inputs ───────────────────────────────────────────────────
  const hostile = [
    "null",
    "[]",
    "garbage {",
    '{"cmd":null}',
    '{"cmd":"observe","r":-5,"result":"zzz"}',
    '{"cmd":"place_manual","ships":[[0,0,"x",1]]}',
    '{"cmd":"unknown"}',
    "\u{1F980}",
  ];
  let hostileOk = true;
  for (const h of hostile) {
    try {
      const rep = e.request(h);
      if (!rep || typeof rep !== "object") hostileOk = false;
    } catch {
      hostileOk = false;
    }
  }
  t("hostile inputs never crash the engine", hostileOk);

  // ── 6. documentation examples (verified, not just shown) ────────────────
  const docs = await verifyDocsExamples(makeEngine);
  t(
    "every runnable docs example executes + asserts",
    docs.summary.allOk,
    `${docs.summary.passed}/${docs.summary.total} examples` +
      (docs.summary.allOk ? "" : " — first failure: " + firstFail(docs.results))
  );

  // ── 7. generalised variants (0.4) ───────────────────────────────────────
  const v = await makeEngine();
  const vlist = v.cmd("variant_list");
  t(
    "variant_list exposes the presets",
    Array.isArray(vlist.presets) &&
      ["classic", "micro", "big16", "huge30", "torus8", "archipelago12", "poly10"].every(
        (n) => vlist.presets.some((p) => p.name === n)
      ),
    `${vlist.presets.length} presets`
  );

  const vn = v.cmd("variant_new", { preset: "micro", seed: 42 });
  t(
    "variant_new(micro) starts a 7x7 game",
    vn.ok === true && vn.width === 7 && vn.height === 7 && vn.ships === 3,
    `${vn.width}x${vn.height}, ${vn.ships} ships, simd=${vn.simd_backend}`
  );

  // Play a full variant game through the protocol.
  let vMoves = 0;
  let vDone = false;
  let vErr = "";
  try {
    for (; vMoves < 200; vMoves++) {
      const m = v.cmd("variant_move");
      if (!m.ok) throw new Error(m.error || "no move");
      const f = v.cmd("variant_fire", { r: m.row, c: m.col });
      if (!f.ok) throw new Error(f.error || "fire failed");
      if (f.all_sunk) {
        vDone = true;
        break;
      }
    }
  } catch (err) {
    vErr = String(err.message || err);
  }
  t(
    "variant game completes via variant_move/variant_fire",
    vDone && vMoves < 200,
    vErr || `${vMoves + 1} moves to sink the fleet`
  );

  const vf = v.cmd("variant_state");
  t(
    "variant_state reports a complete snapshot",
    vf.ok !== false &&
      vf.width === 7 &&
      typeof vf.moves_fired === "number" &&
      Array.isArray(vf.density) &&
      vf.density.length === 49 &&
      typeof vf.feasibility === "object" &&
      vf.feasibility.feasible === true,
    `moves=${vf.moves_fired}, simd=${vf.simd_backend}`
  );

  // A second preset: the torus.
  const vt = v.cmd("variant_new", { preset: "torus8", seed: 7 });
  const vtp = v.cmd("variant_play", { seed: 11 });
  t(
    "torus variant self-play finishes",
    vt.ok === true && vt.torus === true && vtp.ok === true && vtp.shots >= 14 && vtp.shots <= 64,
    vt.ok && vtp.ok ? `${vtp.shots} shots on the donut` : `${JSON.stringify(vt)} ${JSON.stringify(vtp)}`
  );

  // Hostile variant input: bad preset, out-of-bounds fire.
  const vbad = v.cmd("variant_new", { preset: "nonsense" });
  const vfire = v.cmd("variant_fire", { r: 999, c: 999 });
  t(
    "hostile variant input gets JSON errors, no crashes",
    vbad.ok === false && vfire.ok === false,
    `${vbad.error} | ${vfire.error}`
  );

  // ── summary ─────────────────────────────────────────────────────────────
  const passed = results.filter((r) => r.ok).length;
  const summary = { passed, total: results.length, allOk: passed === results.length };
  onProgress({ name: "__summary__", summary });
  return { results, summary };
}

// ── helpers ────────────────────────────────────────────────────────────────

function firstFail(results) {
  const f = results.find((r) => !r.ok);
  return f ? `${f.section}: ${f.detail}` : "";
}

function bitCount(v) {
  let n = 0;
  let x = v;
  while (x) {
    x &= x - 1n;
    n++;
  }
  return n;
}

function fmt(seq) {
  return seq
    .slice(0, 3)
    .map(([r, c]) => `(${r},${c})`)
    .join(" ");
}
