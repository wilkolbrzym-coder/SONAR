/**
 * Sonar in-browser test suite — the "Test Sonar" feature.
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
 *   6. mod-runtime sanity (example mod plays games without errors)
 *
 * Every test reports {name, ok, detail} through `onProgress`.
 */

import { evalMod, runModArena, EXAMPLE_MODS } from "./mods.js";

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
    `v${hello.version} protocol ${hello.protocol}`
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

  // ── 6. mod runtime ──────────────────────────────────────────────────────
  const modSource = EXAMPLE_MODS.parityHunter.source;
  let modOk = false;
  let modDetail = "";
  try {
    const mod = evalMod(modSource);
    const arena = runModArena(mod, await makeEngine(), await makeEngine(), {
      games: 4,
      softTarget: 64,
      seed: 4242,
    });
    modOk = arena.total === 4 && arena.modWins + arena.engineWins === 4;
    modDetail = `mod won ${arena.modWins}/4`;
  } catch (err) {
    modDetail = String(err.message || err);
  }
  t("mod runtime plays games without errors", modOk, modDetail);

  // ── summary ─────────────────────────────────────────────────────────────
  const passed = results.filter((r) => r.ok).length;
  const summary = { passed, total: results.length, allOk: passed === results.length };
  onProgress({ name: "__summary__", summary });
  return { results, summary };
}

// ── helpers ────────────────────────────────────────────────────────────────

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
