/**
 * Sonar mod runtime — write custom brains for Sonar directly in the app.
 *
 * A mod is a JavaScript object with hooks. It runs inside the Web Worker
 * (full performance, no main-thread blocking) and receives *100% of
 * Sonar's internal state* through the `api` object: the full board view,
 * the PDF density matrix, the Bayesian hypothesis posterior — everything
 * the engine itself sees. See the docs tab ("Modding") for the full API.
 *
 * ## Mod template
 *
 * ```js
 * const mod = {
 *   name: "My Mod",
 *   version: "1.0",
 *   // Optional: place YOUR fleet. Return [{r, c, len, horizontal}, ...]
 *   // or null to keep a random legal placement.
 *   placeFleet(api) { return null; },
 *   // Required: pick the next shot at the enemy fleet.
 *   chooseMove(api) { return { r: 5, c: 6 }; },
 *   // Optional: called after the enemy fires at YOUR fleet.
 *   onObserve(api, r, c, result) {},
 *   // Optional: called when the game ends.
 *   onGameEnd(api, won, moves) {},
 * };
 * ```
 *
 * Illegal moves (out of bounds / already fired / bad return shape) are
 * auto-corrected to a random legal cell and counted; three strikes in a
 * game forfeit the match — mods must play fair to win.
 */

// ── Seeded RNG for mods (xoshiro-style, deterministic) ─────────────────────

export function makeRng(seed = Date.now() ^ 0x9e3779b9) {
  let s = BigInt(seed) & 0xffffffffn;
  if (s === 0n) s = 0x1234567n;
  let x = s;
  return function rand() {
    // xorshift32 — small, fast, good enough for game mods.
    x ^= x << 13n;
    x &= 0xffffffffn;
    x ^= x >> 17n;
    x ^= x << 5n;
    x &= 0xffffffffn;
    return Number(x) / 4294967296;
  };
}

// ── Board helpers (pure JS, shared by the runner and mods) ────────────────

export function shipsToCells(ships) {
  const cells = [];
  for (const s of ships) {
    for (let i = 0; i < s.len; i++) {
      cells.push(s.horizontal ? s.r * 10 + s.c + i : (s.r + i) * 10 + s.c);
    }
  }
  return cells;
}

export function legalFleet(ships) {
  if (!Array.isArray(ships) || ships.length === 0) return false;
  const lens = ships.map((s) => Number(s.len)).sort((a, b) => a - b);
  const std = [2, 3, 3, 4, 5];
  if (lens.length !== std.length || lens.some((l, i) => l !== std[i])) return false;
  const occupied = new Set();
  const all = new Set();
  for (const s of ships) {
    const r = Number(s.r);
    const c = Number(s.c);
    const len = Number(s.len);
    const h = !!s.horizontal;
    if (!Number.isInteger(r) || !Number.isInteger(c)) return false;
    if (r < 0 || r > 9 || c < 0 || c > 9) return false;
    if (len < 2 || len > 5) return false;
    if (h ? c + len > 10 : r + len > 10) return false;
    for (let i = 0; i < len; i++) {
      const idx = h ? r * 10 + c + i : (r + i) * 10 + c;
      if (occupied.has(idx)) return false;
      all.add(idx);
    }
  }
  // No 8-directional contact between different ships.
  for (const idx of all) {
    const r = Math.floor(idx / 10);
    const c = idx % 10;
    for (let dr = -1; dr <= 1; dr++) {
      for (let dc = -1; dc <= 1; dc++) {
        if (dr === 0 && dc === 0) continue;
        const nr = r + dr;
        const nc = c + dc;
        if (nr < 0 || nr > 9 || nc < 0 || nc > 9) continue;
        const nIdx = nr * 10 + nc;
        if (all.has(nIdx)) {
          // Same ship cells are contiguous along one axis; diagonal
          // neighbours are ALWAYS different ships; orthogonal neighbours
          // across a gap cannot happen within one linear ship.
          if (dr !== 0 && dc !== 0) return false;
          // Orthogonal: same ship only if along its axis and contiguous.
          const sameShip = ships.some((s) => {
            const cells = shipsToCells([s]);
            return cells.includes(idx) && cells.includes(nIdx);
          });
          if (!sameShip) return false;
        }
      }
    }
  }
  return true;
}

export function randomFleetJs(rand) {
  // Uniform-ish random legal fleet (rejection sampling, like the engine's).
  for (let attempt = 0; attempt < 2000; attempt++) {
    const lens = [5, 4, 3, 3, 2];
    const ships = [];
    const occupied = new Set();
    let ok = true;
    for (const len of lens) {
      let placed = false;
      for (let t = 0; t < 300; t++) {
        const h = rand() < 0.5;
        const r = Math.floor(rand() * (h ? 10 : 11 - len));
        const c = Math.floor(rand() * (h ? 11 - len : 10));
        const cells = [];
        for (let i = 0; i < len; i++) {
          cells.push(h ? r * 10 + c + i : (r + i) * 10 + c);
        }
        let clash = false;
        for (const idx of cells) {
          const rr = Math.floor(idx / 10);
          const cc = idx % 10;
          for (let dr = -1; dr <= 1 && !clash; dr++) {
            for (let dc = -1; dc <= 1; dc++) {
              const nr = rr + dr;
              const nc = cc + dc;
              if (nr >= 0 && nr < 10 && nc >= 0 && nc < 10) {
                if (occupied.has(nr * 10 + nc)) {
                  clash = true;
                  break;
                }
              }
            }
          }
          if (clash) break;
        }
        if (!clash) {
          for (const idx of cells) occupied.add(idx);
          ships.push({ r, c, len, horizontal: h });
          placed = true;
          break;
        }
      }
      if (!placed) {
        ok = false;
        break;
      }
    }
    if (ok) return ships;
  }
  // Deterministic fallback (same shape as the engine's fallback fleet).
  return [
    { r: 0, c: 0, len: 5, horizontal: true },
    { r: 2, c: 0, len: 4, horizontal: true },
    { r: 4, c: 0, len: 3, horizontal: true },
    { r: 6, c: 0, len: 3, horizontal: true },
    { r: 8, c: 0, len: 2, horizontal: true },
  ];
}

// ── Mod sandboxing ─────────────────────────────────────────────────────────

/**
 * Evaluate mod source code and return the mod object.
 * The code must define a global `mod` (or export-like `return mod`).
 * Runs inside the worker — no DOM, no fetch (deny by convention: the
 * Function constructor only sees what we pass in).
 */
export function evalMod(source) {
  const factory = new Function("rand", `"use strict";
${source}
return (typeof mod !== "undefined" && mod) || (typeof Mod !== "undefined" && Mod) || null;`);
  const mod = factory();
  if (!mod || typeof mod !== "object") {
    throw new Error("Mod source must define a global object named `mod`.");
  }
  if (typeof mod.name !== "string" || !mod.name) {
    throw new Error("Mod must have a `name` string.");
  }
  if (typeof mod.chooseMove !== "function") {
    throw new Error("Mod must implement chooseMove(api).");
  }
  return mod;
}

// ── The mod api (this is the "100% control" surface) ───────────────────────

/**
 * Build the api object handed to mod hooks.
 *
 * @param {object} view  — {shots, hits, sunk, remaining, history} of the
 *   board the mod is ATTACKING (its own perspective).
 * @param {object} engineB — a Sonar engine instance whose enemy view
 *   mirrors the mod's view; its density/probability matrices are
 *   sonar's full analysis of that very board.
 */
export function buildModApi(view, engineB, rand) {
  const shots = new Uint8Array(100);
  const hits = new Uint8Array(100);
  const sunk = new Uint8Array(100);
  for (const idx of view.shots) shots[idx] = 1;
  for (const idx of view.hits) hits[idx] = 1;
  for (const idx of view.sunk) sunk[idx] = 1;

  // Ask the shadow engine for sonar's analysis of this exact view.
  let density = null;
  let probability = null;
  let hypothesisCount = 0;
  if (engineB) {
    try {
      const d = engineB.cmd("density");
      density = Float32Array.from(d.matrix);
      const p = engineB.cmd("probability");
      probability = Float32Array.from(p.matrix);
      hypothesisCount = p.hypothesis_count;
    } catch {
      // Analysis unavailable — mods must handle nulls gracefully.
    }
  }

  const valid = (r, c) =>
    Number.isInteger(r) &&
    Number.isInteger(c) &&
    r >= 0 &&
    r < 10 &&
    c >= 0 &&
    c < 10 &&
    shots[r * 10 + c] === 0;

  const argmax = (matrix) => {
    let best = -Infinity;
    let bestIdx = -1;
    for (let i = 0; i < 100; i++) {
      if (shots[i]) continue;
      const v = matrix ? matrix[i] : 0;
      if (v > best) {
        best = v;
        bestIdx = i;
      }
    }
    return bestIdx;
  };

  const rand2 = rand || makeRng(Date.now());

  return {
    // Board state (the fleet you are attacking, as you observed it).
    shots,
    hits,
    sunk,
    get activeHits() {
      const a = new Uint8Array(100);
      for (let i = 0; i < 100; i++) a[i] = hits[i] && !sunk[i] ? 1 : 0;
      return a;
    },
    remaining: view.remaining.slice(),
    moveNumber: view.history.length + 1,
    history: view.history.slice(),
    // Sonar's own analysis of the same board — 100% of the engine's data.
    density,
    probability,
    hypothesisCount,
    // Helpers.
    valid,
    argmax,
    rand: rand2,
    boardSize: 10,
    fleetLens: [5, 4, 3, 3, 2],
  };
}

// ── Match runner: mod vs engine ────────────────────────────────────────────

/**
 * Play one game: `mod` (JS brain) vs the engine (default Sonar brain).
 *
 * @param {object} mod        — evaluated mod object
 * @param {object} engineA    — plays Sonar's side (opponent)
 * @param {object|null} engineB — shadow engine feeding the mod's api
 *   (sonar's analysis of the board the mod attacks). Null = no analysis.
 * @param {object} opts       — {seed, softTarget, modFirst}
 * @returns game report {winner: 'mod'|'engine', moves, modErrors, forfeited}
 */
export function playModGame(mod, engineA, engineB, opts = {}) {
  const seed = opts.seed ?? Math.floor(Math.random() * 2 ** 31);
  const softTarget = opts.softTarget ?? 256;
  const rand = makeRng(seed);
  const history = []; // {r, c, result}
  const view = {
    shots: new Set(),
    hits: new Set(),
    sunk: new Set(),
    remaining: [5, 4, 3, 3, 2],
    history,
  };

  // ── Setup: the engine's side ────────────────────────────────────────────
  engineA.cmd("new_game", {
    seed,
    config: {
      use_learning: false,
      hypothesis_soft_target: softTarget,
      default_deadline_secs: 0,
    },
  });
  engineA.cmd("place_smart");

  // ── Setup: the mod's fleet ──────────────────────────────────────────────
  let modShips = null;
  try {
    if (typeof mod.placeFleet === "function") {
      const api = {
        rand,
        boardSize: 10,
        fleetLens: [5, 4, 3, 3, 2],
        legalFleet,
        randomFleet: () => randomFleetJs(rand),
      };
      const proposed = mod.placeFleet(api);
      if (proposed !== null && proposed !== undefined) {
        if (!legalFleet(proposed)) {
          throw new Error("placeFleet returned an illegal fleet");
        }
        modShips = proposed;
      }
    }
  } catch (e) {
    return {
      winner: "engine",
      moves: 0,
      modErrors: [String(e.message || e)],
      forfeited: true,
      history,
    };
  }
  if (!modShips) modShips = randomFleetJs(rand);

  const modShipState = modShips.map((s) => {
    const cells = shipsToCells([s]);
    return { cells, len: s.len, hits: 0, sunk: false };
  });

  // Shadow engine mirrors the mod's view for analysis.
  if (engineB) {
    engineB.cmd("new_game", {
      seed: seed ^ 0x5EED,
      config: {
        use_learning: false,
        hypothesis_soft_target: softTarget,
        default_deadline_secs: 0,
      },
    });
  }

  // ── Game loop ───────────────────────────────────────────────────────────
  const shotsAtMod = new Set();
  let modStrikes = 0;
  const modErrors = [];
  let moves = 0;
  let winner = null;
  let forfeited = false;
  const maxPairs = 150;

  for (let turn = 0; turn < maxPairs && !winner; turn++) {
    // ── The mod fires at the engine's fleet ───────────────────────────────
    // Refresh sonar's analysis first (this also updates engineB's matrices).
    let chosen = null;
    try {
      if (engineB) {
        // Regenerate hypotheses for the current view; the move itself is
        // discarded — the mod decides.
        engineB.cmd("choose_move", { deadline_secs: 0 });
      }
      const api = buildModApi(view, engineB, rand);
      const mv = mod.chooseMove(api);
      if (mv && Number.isInteger(mv.r) && Number.isInteger(mv.c)) {
        chosen = { r: mv.r, c: mv.c };
      }
    } catch (e) {
      modErrors.push(String(e.message || e));
      if (modErrors.length >= 3) {
        winner = "engine";
        forfeited = true;
        break;
      }
    }

    let illegal = false;
    if (!chosen || chosen.r < 0 || chosen.r > 9 || chosen.c < 0 || chosen.c > 9) {
      illegal = true;
    } else {
      const idx = chosen.r * 10 + chosen.c;
      if (view.shots.has(idx)) illegal = true;
    }
    if (illegal) {
      modStrikes++;
      modErrors.push(`illegal move #${modStrikes}: ${JSON.stringify(chosen)}`);
      // Auto-correct: pick a random unfired cell.
      const free = [];
      for (let i = 0; i < 100; i++) if (!view.shots.has(i)) free.push(i);
      if (free.length === 0) break;
      const pick = free[Math.floor(rand() * free.length)];
      chosen = { r: Math.floor(pick / 10), c: pick % 10 };
      if (modStrikes >= 3) {
        winner = "engine";
        forfeited = true;
        break;
      }
    }

    const resA = engineA.cmd("receive_shot", { r: chosen.r, c: chosen.c });
    // "miss" | "hit" | "sunk" (+ additive `len` for sunks).
    const resultStr = resA.result;
    const sunkLen = resultStr === "sunk" ? Number(resA.len) : 0;
    const idx = chosen.r * 10 + chosen.c;
    view.shots.add(idx);
    if (resultStr === "hit") view.hits.add(idx);
    if (resultStr === "sunk") {
      view.hits.add(idx);
      // Mark the full sunk ship using the true length from the protocol.
      markSunkFrom(view, chosen.r, chosen.c, sunkLen);
    }
    history.push({ r: chosen.r, c: chosen.c, result: resultStr, len: sunkLen });
    moves++;
    if (engineB) {
      engineB.cmd("observe", {
        r: chosen.r,
        c: chosen.c,
        result: resultStr === "sunk" ? `sunk_${sunkLen}` : resultStr,
      });
    }

    // Game over? (engine fleet fully sunk)
    const snapA = engineA.cmd("snapshot");
    const fleet = BigInt(snapA.our_fleet_mask);
    const sunkM = BigInt(snapA.our_sunk_mask);
    if (fleet !== 0n && (fleet & sunkM) === fleet) {
      winner = "mod";
      break;
    }

    // ── The engine fires at the mod's fleet ───────────────────────────────
    const mv = engineA.cmd("choose_move", { deadline_secs: 0 });
    const mIdx = mv.row * 10 + mv.col;
    if (!shotsAtMod.has(mIdx)) {
      shotsAtMod.add(mIdx);
      let resM = "miss";
      let sunkLenM = 0;
      for (const ship of modShipState) {
        if (ship.cells.includes(mIdx)) {
          ship.hits++;
          if (ship.hits === ship.len) {
            ship.sunk = true;
            resM = "sunk";
            sunkLenM = ship.len;
          } else {
            resM = "hit";
          }
          break;
        }
      }
      moves++;
      // Feed the engine with the precise sunk length.
      engineA.cmd("observe", {
        r: mv.row,
        c: mv.col,
        result: resM === "sunk" ? `sunk_${sunkLenM}` : resM,
      });
      try {
        if (typeof mod.onObserve === "function") {
          mod.onObserve(
            {
              remaining: modShipState.filter((s) => !s.sunk).map((s) => s.len),
              shipsSunk: modShipState.filter((s) => s.sunk).length,
            },
            mv.row,
            mv.col,
            resM
          );
        }
      } catch (e) {
        modErrors.push(String(e.message || e));
      }
      if (modShipState.every((s) => s.sunk)) {
        winner = "engine";
        break;
      }
    }
  }

  if (winner === null) winner = moves % 2 === 0 ? "engine" : "mod";

  try {
    if (typeof mod.onGameEnd === "function") {
      mod.onGameEnd({}, winner === "mod", moves);
    }
  } catch {
    // onGameEnd errors never affect the result.
  }

  return { winner, moves, modErrors, forfeited, history, modShips };
}

/**
 * Extend the sunk set along the contiguous run of hits through (r, c),
 * bounded to the ship's true `len` from the protocol. Ships are linear,
 * so the run (horizontal or vertical, whichever matches the length) is
 * the sunk ship.
 */
function markSunkFrom(view, r, c, len) {
  const run = (dr, dc) => {
    const cells = [];
    let rr = r;
    let cc = c;
    // Extend backwards.
    while (true) {
      rr -= dr;
      cc -= dc;
      if (rr < 0 || rr > 9 || cc < 0 || cc > 9) break;
      const idx = rr * 10 + cc;
      if (!view.hits.has(idx)) break;
      cells.unshift(idx);
    }
    cells.push(r * 10 + c);
    // Extend forwards.
    rr = r;
    cc = c;
    while (cells.length < Math.max(len, 1) + 16) {
      rr += dr;
      cc += dc;
      if (rr < 0 || rr > 9 || cc < 0 || cc > 9) break;
      const idx = rr * 10 + cc;
      if (!view.hits.has(idx)) break;
      cells.push(idx);
    }
    return cells;
  };

  const h = run(0, 1);
  const v = run(1, 0);
  // Pick the run whose length matches the ship, or the longer one.
  let cells;
  if (h.length === len && v.length !== len) cells = h;
  else if (v.length === len && h.length !== len) cells = v;
  else cells = h.length >= v.length ? h : v;
  // Bound to len when we have an exact length.
  if (len >= 1 && cells.length > len) {
    // Keep the window containing (r, c).
    const pos = cells.indexOf(r * 10 + c);
    let start = Math.max(0, Math.min(cells.length - len, pos - (len - 1)));
    cells = cells.slice(start, start + len);
  }
  for (const idx of cells) view.sunk.add(idx);
  // Remove the ship length from `remaining`.
  if (len >= 1) {
    const i = view.remaining.indexOf(len);
    if (i >= 0) view.remaining.splice(i, 1);
    else if (view.remaining.length > 0) view.remaining.splice(0, 1);
  }
}

// ── Arena: N games with aggregate stats ────────────────────────────────────

/**
 * Run a full mod-vs-engine arena.
 *
 * @param {object} mod
 * @param {object} engineA
 * @param {object|null} engineB
 * @param {object} opts {games, softTarget, seed, onProgress}
 */
export function runModArena(mod, engineA, engineB, opts = {}) {
  const games = opts.games ?? 20;
  const baseSeed = opts.seed ?? 1_000_003;
  const results = {
    modWins: 0,
    engineWins: 0,
    movesInModWins: 0,
    movesInEngineWins: 0,
    forfeits: 0,
    errors: [],
    gamesList: [],
  };
  for (let g = 0; g < games; g++) {
    const rep = playModGame(mod, engineA, engineB, {
      seed: baseSeed + g * 7919,
      softTarget: opts.softTarget,
    });
    if (rep.winner === "mod") {
      results.modWins++;
      results.movesInModWins += rep.moves;
    } else {
      results.engineWins++;
      results.movesInEngineWins += rep.moves;
    }
    if (rep.forfeited) results.forfeits++;
    for (const e of rep.modErrors.slice(0, 3)) {
      if (results.errors.length < 20) results.errors.push(`game ${g + 1}: ${e}`);
    }
    results.gamesList.push({ winner: rep.winner, moves: rep.moves, forfeited: rep.forfeited });
    if (opts.onProgress) opts.onProgress(g + 1, games, rep);
  }
  results.total = games;
  results.modWinRate = games ? (results.modWins / games) * 100 : 0;
  // Wilson 95% on the mod's win rate.
  const p = results.modWins / Math.max(games, 1);
  const z = 1.96;
  const n = games;
  const denom = 1 + (z * z) / n;
  const centre = p + (z * z) / (2 * n);
  const margin = z * Math.sqrt((p * (1 - p)) / n + (z * z) / (4 * n * n));
  results.wilson95 = [
    Math.max(0, ((centre - margin) / denom) * 100),
    Math.min(100, ((centre + margin) / denom) * 100),
  ];
  return results;
}

// ── Example mods (also used as the docs' live templates) ──────────────────

export const EXAMPLE_MODS = {
  parityHunter: {
    title: "Parity Hunter",
    description:
      "Classic checkerboard hunt with neighbour targeting. A solid, simple baseline — beats random easily, loses to Sonar.",
    source: `// Parity Hunter — a classic battleship brain.
const mod = {
  name: "Parity Hunter",
  version: "1.0",

  chooseMove(api) {
    // Target mode: fire next to active hits.
    const active = api.activeHits;
    for (let i = 0; i < 100; i++) {
      if (!active[i]) continue;
      const r = Math.floor(i / 10), c = i % 10;
      for (const [dr, dc] of [[-1,0],[1,0],[0,-1],[0,1]]) {
        const nr = r + dr, nc = c + dc;
        if (api.valid(nr, nc)) return { r: nr, c: nc };
      }
    }
    // Hunt mode: checkerboard cells, prefer sonar's hot cells as a tiebreak.
    let best = null, bestScore = -1;
    for (let i = 0; i < 100; i++) {
      if (!api.valid(Math.floor(i / 10), i % 10)) continue;
      const r = Math.floor(i / 10), c = i % 10;
      if ((r + c) % 2 !== 0) continue;
      const d = api.density ? api.density[i] : 0;
      if (d > bestScore) { bestScore = d; best = { r, c }; }
    }
    if (best) return best;
    // Any unfired cell.
    const i = api.argmax(api.density);
    return { r: Math.floor(i / 10), c: i % 10 };
  },
};`,
  },

  densityRider: {
    title: "Density Rider",
    description:
      "Rides Sonar's PDF density matrix directly (api.density) with a small centre bias — demonstrates using the engine's own analysis data.",
    source: `// Density Rider — let Sonar's density matrix do the aiming.
const mod = {
  name: "Density Rider",
  version: "1.0",

  chooseMove(api) {
    const d = api.density;
    if (!d) {
      const i = api.argmax(null);
      return { r: Math.floor(i / 10), c: i % 10 };
    }
    let best = -Infinity, bestIdx = -1;
    for (let i = 0; i < 100; i++) {
      if (api.shots[i]) continue;
      const r = Math.floor(i / 10), c = i % 10;
      // Light centre bias on top of the density.
      const centre = 5.5 - (Math.abs(r - 4.5) + Math.abs(c - 4.5)) / 9;
      const score = d[i] + centre * 2;
      if (score > best) { best = score; bestIdx = i; }
    }
    return { r: Math.floor(bestIdx / 10), c: bestIdx % 10 };
  },
};`,
  },

  posteriorSniper: {
    title: "Posterior Sniper",
    description:
      "Blends Sonar's Bayesian posterior (api.probability) with its density — the strongest example mod; close to the engine's own hybrid.",
    source: `// Posterior Sniper — blend sonar's posterior with its density.
const mod = {
  name: "Posterior Sniper",
  version: "1.0",

  chooseMove(api) {
    const p = api.probability, d = api.density;
    let best = -Infinity, bestIdx = api.argmax(d);
    if (p && d) {
      const dmax = Math.max(...Array.from(d), 1e-9);
      for (let i = 0; i < 100; i++) {
        if (api.shots[i]) continue;
        const score = p[i] + (d[i] / dmax) * 0.5;
        if (score > best) { best = score; bestIdx = i; }
      }
    } else if (p) {
      bestIdx = api.argmax(p);
    }
    return { r: Math.floor(bestIdx / 10), c: bestIdx % 10 };
  },
};`,
  },

  edgeGhost: {
    title: "Edge Ghost",
    description:
      "Custom placement near the border (where PDF hunters expect ships least after sonar's centre-heavy density) plus parity targeting.",
    source: `// Edge Ghost — sneaky placement + checkerboard hunt.
const mod = {
  name: "Edge Ghost",
  version: "1.0",

  placeFleet(api) {
    // A legal fleet hugging structure the engine rarely expects.
    const fleet = [
      { r: 0, c: 0, len: 5, horizontal: true },
      { r: 9, c: 5, len: 4, horizontal: true },
      { r: 2, c: 9, len: 3, horizontal: false },
      { r: 5, c: 2, len: 3, horizontal: true },
      { r: 7, c: 7, len: 2, horizontal: true },
    ];
    return api.legalFleet(fleet) ? fleet : api.randomFleet();
  },

  chooseMove(api) {
    const active = api.activeHits;
    for (let i = 0; i < 100; i++) {
      if (!active[i]) continue;
      const r = Math.floor(i / 10), c = i % 10;
      for (const [dr, dc] of [[-1,0],[1,0],[0,-1],[0,1]]) {
        const nr = r + dr, nc = c + dc;
        if (api.valid(nr, nc)) return { r: nr, c: nc };
      }
    }
    let best = null, bestScore = -1;
    for (let i = 0; i < 100; i++) {
      const r = Math.floor(i / 10), c = i % 10;
      if (!api.valid(r, c) || (r + c) % 2 !== 0) continue;
      const score = api.density ? api.density[i] : 0;
      if (score > bestScore) { bestScore = score; best = { r, c }; }
    }
    return best || { r: 0, c: 0 };
  },
};`,
  },
};
