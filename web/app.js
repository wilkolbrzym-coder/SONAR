/**
 * Sonar web app — main-thread UI logic.
 *
 * All engine work happens in the Web Worker (worker.js); this file only
 * renders state and forwards user actions. Tabs: Play, Team, Mods, Test,
 * Docs.
 */

import { EXAMPLE_MODS } from "./mods.js";
import { DOCS } from "./docs-data.js";
import { searchDocs } from "./search.js";
import { cryptoSeed } from "./engine.js";

// ── Worker wiring (promise-based RPC) ───────────────────────────────────────

const worker = new Worker("worker.js", { type: "module" });
let rpcId = 1;
const pending = new Map();
const progressHandlers = new Map();

worker.onmessage = (ev) => {
  const msg = ev.data;
  if (!msg || typeof msg !== "object") return;
  // Progress events carry the same id but no final reply flag.
  if (msg.type === "progress" && progressHandlers.has(msg.id)) {
    progressHandlers.get(msg.id)(msg);
    return;
  }
  if (msg.type === "test-progress" && progressHandlers.has(msg.id)) {
    progressHandlers.get(msg.id)(msg);
    return;
  }
  const cb = pending.get(msg.id);
  if (cb) {
    pending.delete(msg.id);
    if (msg.id && progressHandlers.has(msg.id)) progressHandlers.delete(msg.id);
    cb(msg);
  }
};

function call(type, extra = {}, onProgress = null) {
  return new Promise((resolve) => {
    const id = rpcId++;
    if (onProgress) progressHandlers.set(id, onProgress);
    pending.set(id, resolve);
    worker.postMessage({ id, type, ...extra });
  });
}

/** Send one JSON protocol line to the engine; returns the parsed reply. */
async function cmd(name, extra = {}) {
  const res = await call("cmd", { line: JSON.stringify({ cmd: name, ...extra }) });
  if (!res.ok) throw new Error(res.error);
  return JSON.parse(res.reply);
}

// ── Boot ────────────────────────────────────────────────────────────────────

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => Array.from(document.querySelectorAll(sel));

const state = {
  version: null,
  // Play mode.
  play: {
    phase: "setup", // setup | playing | over
    difficulty: 256,
    myShips: [],
    palette: [],
    rotate: true,
    selectedLen: null,
    myBoard: null, // {ships:[{cells,hits,len,sunk}], shotsAt:Set}
    enemyDead: false,
    moveNo: 0,
    heatOn: false,
  },
  team: {
    started: false,
    difficulty: 256,
    log: [],
  },
};

async function boot() {
  try {
    const hello = await call("hello");
    state.version = hello.hello;
    $("#brandVersion").textContent = hello.hello.version;
    $("#footerVersion").textContent = hello.hello.version;
    const es = $("#engineState");
    es.textContent = `engine: ${hello.hello.version} ready (wasm)`;
    es.classList.add("ok");
  } catch (e) {
    const es = $("#engineState");
    es.textContent = "engine: failed to load";
    es.classList.add("err");
    console.error(e);
  }
  initTabs();
  initPlay();
  initTeam();
  initMods();
  initTests();
  initDocs();
}
boot();

// ── Tabs ────────────────────────────────────────────────────────────────────

function initTabs() {
  $$(".tab").forEach((btn) => {
    btn.addEventListener("click", () => activateTab(btn.dataset.tab));
  });
  activateTab("play");
}

function activateTab(name) {
  $$(".tab").forEach((b) => b.classList.toggle("active", b.dataset.tab === name));
  $$(".panel").forEach((p) => (p.hidden = p.id !== `tab-${name}`));
  if (name === "docs" && !docState.rendered) renderDocs();
}

// ── Shared board rendering ──────────────────────────────────────────────────

const COLS = "ABCDEFGHIJ";

function makeBoard(el, { attack = false, onCell } = {}) {
  el.innerHTML = "";
  const cells = [];
  // Corner + column headers.
  el.appendChild(div("cell coord", ""));
  for (let c = 0; c < 10; c++) el.appendChild(div("cell coord", COLS[c]));
  for (let r = 0; r < 10; r++) {
    el.appendChild(div("cell coord", String(r + 1)));
    for (let c = 0; c < 10; c++) {
      const cell = div("cell", "");
      cell.dataset.r = r;
      cell.dataset.c = c;
      if (attack) {
        cell.addEventListener("click", () => onCell && onCell(r, c, cell));
      }
      cells.push(cell);
      el.appendChild(cell);
    }
  }
  return cells;
}

function div(cls, text) {
  const d = document.createElement("div");
  d.className = cls;
  d.textContent = text;
  return d;
}

function cellAt(cells, r, c) {
  return cells[r * 10 + c];
}

function setCell(cells, r, c, cls) {
  const cell = cellAt(cells, r, c);
  cell.className = `cell ${cls}`;
}

// ── PLAY: fleet placement editor ────────────────────────────────────────────

const PALETTE_SHIPS = [
  { len: 5, label: "Carrier" },
  { len: 4, label: "Battleship" },
  { len: 3, label: "Submarine" },
  { len: 3, label: "Cruiser" },
  { len: 2, label: "Destroyer" },
];

let setupCells = [];

function initPlay() {
  setupCells = makeBoard($("#boardMy"), {
    onCell: (r, c) => handleSetupClick(r, c),
  });
  // Hover preview.
  $("#boardMy").addEventListener("mouseover", (ev) => {
    const cell = ev.target.closest(".cell:not(.coord)");
    if (!cell || state.play.phase !== "setup") return;
    updatePreview(Number(cell.dataset.r), Number(cell.dataset.c));
  });
  $("#boardMy").addEventListener("mouseleave", clearPreview);

  $$(".ship-chip").forEach((chip) => {
    chip.addEventListener("click", () => {
      const len = Number(chip.dataset.len);
      const already = state.play.palette.some((p) => p.len === len && p.used);
      if (already) return;
      selectChip(len);
    });
  });

  $("#btnRotate").addEventListener("click", () => {
    state.play.rotate = !state.play.rotate;
    clearPreview();
  });
  document.addEventListener("keydown", (ev) => {
    if (ev.key === "r" || ev.key === "R") {
      if (!$("#tab-play").hidden && state.play.phase === "setup") {
        state.play.rotate = !state.play.rotate;
        clearPreview();
      }
    }
  });
  $("#btnAutoPlace").addEventListener("click", () => autoPlace(true));
  $("#btnRandomPlace").addEventListener("click", () => autoPlace(false));
  $("#btnClearFleet").addEventListener("click", clearPlacement);
  $("#btnStartGame").addEventListener("click", startBattle);
  $("#btnNewGame").addEventListener("click", () => {
    // Full reset.
    state.play.phase = "setup";
    state.play.myShips = [];
    state.play.palette = [];
    $("#playSetup").hidden = false;
    $("#playGame").hidden = true;
    $("#moveLog").innerHTML = "";
    $("#sonarInfo").innerHTML = "";
    $("#gameStatus").textContent = "Your move";
    refreshSetupBoard();
    updateSetupStatus();
  });
  $("#playDifficulty").addEventListener("change", (ev) => {
    state.play.difficulty = Number(ev.target.value);
  });
  $("#playHeatmap").addEventListener("change", (ev) => {
    state.play.heatOn = ev.target.checked;
    if (gameCells.my) renderMyGameBoard();
  });

  selectChip(5);
  updateSetupStatus();
}

function selectChip(len) {
  state.play.selectedLen = len;
  $$(".ship-chip").forEach((chip) => {
    chip.classList.toggle("selected", Number(chip.dataset.len) === len);
  });
  const used = state.play.myShips.filter((s) => s.len === len).length;
  const total = PALETTE_SHIPS.filter((s) => s.len === len).length;
  $$(".ship-chip").forEach((chip) => {
    if (Number(chip.dataset.len) !== len) return;
    const u = state.play.myShips.filter((s) => s.len === len).length;
    chip.classList.toggle("placed", u >= total);
  });
}

function shipCellsAt(r, c, len, horizontal) {
  const cells = [];
  for (let i = 0; i < len; i++) {
    cells.push(horizontal ? r * 10 + c + i : (r + i) * 10 + c);
  }
  return cells;
}

function placementLegal(cells) {
  const occupied = new Set();
  for (const s of state.play.myShips) for (const idx of s.cells) occupied.add(idx);
  for (const idx of cells) {
    if (idx < 0 || idx > 99) return false;
    const r = Math.floor(idx / 10);
    const c = idx % 10;
    if (horizontalCheck(cells, idx) === false) return false;
    // No overlap and no 8-neighbour contact with existing ships.
    for (let dr = -1; dr <= 1; dr++) {
      for (let dc = -1; dc <= 1; dc++) {
        const nr = r + dr;
        const nc = c + dc;
        if (nr < 0 || nr > 9 || nc < 0 || nc > 9) continue;
        if (occupied.has(nr * 10 + nc)) return false;
      }
    }
  }
  return true;
}

function horizontalCheck(cells, idx) {
  return true; // bounds already verified via idx range
}

function handleSetupClick(r, c) {
  const len = state.play.selectedLen;
  if (!len) return;
  const cells = shipCellsAt(r, c, len, state.play.rotate);
  const fits = cells.every((i) => i >= 0 && i <= 99) &&
    (state.play.rotate ? c + len <= 10 : r + len <= 10);
  if (!fits) return flashStatus("Ship does not fit there.");
  if (!placementLegal(cells)) return flashStatus("Ships may not touch each other.");

  state.play.myShips.push({ r, c, len, horizontal: state.play.rotate, cells });
  refreshSetupBoard();
  updateSetupStatus();

  // Auto-advance to the next unplaced ship type.
  const remaining = PALETTE_SHIPS.filter((ps) => {
    const used = state.play.myShips.filter((s) => s.len === ps.len).length;
    const total = PALETTE_SHIPS.filter((s) => s.len === ps.len).length;
    return used < total;
  });
  if (remaining.length > 0) {
    selectChip(remaining[0].len);
  } else {
    state.play.selectedLen = null;
    $$(".ship-chip").forEach((chip) => chip.classList.remove("selected"));
  }
}

function refreshSetupBoard() {
  for (const cell of setupCells) cell.className = "cell";
  for (const s of state.play.myShips) {
    for (const idx of s.cells) {
      const r = Math.floor(idx / 10);
      const c = idx % 10;
      setCell(setupCells, r, c, "ship");
    }
  }
  // Mark placed chips.
  $$(".ship-chip").forEach((chip) => {
    const len = Number(chip.dataset.len);
    const used = state.play.myShips.filter((s) => s.len === len).length;
    const total = PALETTE_SHIPS.filter((s) => s.len === len).length;
    chip.classList.toggle("placed", used >= total);
  });
}

function updatePreview(r, c) {
  clearPreview();
  const len = state.play.selectedLen;
  if (!len) return;
  const cells = shipCellsAt(r, c, len, state.play.rotate);
  const fits = cells.every((i) => i >= 0 && i <= 99) &&
    (state.play.rotate ? c + len <= 10 : r + len <= 10);
  const legal = fits && placementLegal(cells);
  for (const idx of cells) {
    if (idx < 0 || idx > 99) continue;
    const cell = cellAt(setupCells, Math.floor(idx / 10), idx % 10);
    cell.classList.add(legal ? "preview" : "preview bad");
  }
}

function clearPreview() {
  $$("#boardMy .cell.preview").forEach((c) => c.classList.remove("preview", "bad"));
}

function updateSetupStatus() {
  const total = 5;
  const placed = state.play.myShips.length;
  $("#btnStartGame").disabled = placed !== total;
  $("#setupStatus").textContent =
    placed === total ? "Fleet ready — start the battle!" : `${placed}/${total} ships placed.`;
}

function flashStatus(msg) {
  const el = $("#setupStatus");
  el.textContent = msg;
  el.style.color = "var(--hit)";
  setTimeout(() => {
    el.style.color = "";
    updateSetupStatus();
  }, 1400);
}

async function autoPlace(smart) {
  // Ask the engine for a placement (smart = penalty-minimising, still
  // ε-band randomised) and mirror it in JS.
  const seed = Number(cryptoSeed() % 2n ** 31n);
  await cmd("new_game", {
    seed,
    config: {
      use_learning: false,
      hypothesis_soft_target: state.play.difficulty,
      default_deadline_secs: 0,
    },
  });
  await cmd(smart ? "place_smart" : "place_random");
  const snap = await cmd("snapshot");
  const fleetMask = BigInt(snap.our_fleet_mask);
  // Reconstruct ships from the engine's manual API: ask the engine to
  // verify our reading by placing manually next; simpler — reconstruct
  // greedily from the mask using standard lengths.
  state.play.myShips = reconstructFleet(fleetMask);
  refreshSetupBoard();
  updateSetupStatus();
}

/** Reconstruct a legal 5-ship fleet from a 100-bit occupancy mask. */
function reconstructFleet(mask) {
  const lens = [5, 4, 3, 3, 2];
  const occupied = new Set();
  for (let i = 0; i < 100; i++) if ((mask >> BigInt(i)) & 1n) occupied.add(i);
  const ships = [];
  const remaining = [...lens];
  while (remaining.length > 0) {
    // Find the largest remaining ship that fits at the first free corner.
    let placed = false;
    for (let li = 0; li < remaining.length && !placed; li++) {
      const len = remaining[li];
      for (let idx = 0; idx < 100 && !placed; idx++) {
        if (!occupied.has(idx)) continue;
        const r = Math.floor(idx / 10);
        const c = idx % 10;
        for (const horizontal of [true, false]) {
          const cells = shipCellsAt(r, c, len, horizontal);
          if (cells.some((i) => !occupied.has(i))) continue;
          const key = cells.join(",");
          if (ships.some((s) => s.cells.join(",") === key)) continue;
          ships.push({ r, c, len, horizontal, cells });
          remaining.splice(li, 1);
          placed = true;
          break;
        }
      }
    }
    if (!placed) break;
  }
  return ships.slice(0, 5);
}

function clearPlacement() {
  state.play.myShips = [];
  state.play.selectedLen = 5;
  selectChip(5);
  refreshSetupBoard();
  updateSetupStatus();
}

// ── PLAY: the battle ────────────────────────────────────────────────────────

const gameCells = { enemy: null, my: null };

async function startBattle() {
  if (state.play.myShips.length !== 5) return;
  const seed = Number(cryptoSeed() % 2n ** 31n);
  // Fresh engine game.
  await cmd("new_game", {
    seed,
    config: {
      use_learning: false,
      hypothesis_soft_target: state.play.difficulty,
      default_deadline_secs: 0,
    },
  });
  // Sonar places its fleet (hidden from us).
  await cmd("place_smart");

  state.play.phase = "playing";
  state.play.moveNo = 0;
  state.play.myBoard = {
    ships: state.play.myShips.map((s) => ({ ...s, hits: 0, sunk: false })),
    shotsAtMe: new Set(),
  };
  $("#playSetup").hidden = true;
  $("#playGame").hidden = false;
  gameCells.enemy = makeBoard($("#boardEnemy"), {
    attack: true,
    onCell: (r, c) => playerFire(r, c),
  });
  gameCells.my = makeBoard($("#boardMyGame"));
  renderMyGameBoard();
  $("#moveLog").innerHTML = "";
  $("#gameStatus").textContent = "Your move — fire at enemy waters.";
  $("#turnMarker").textContent = "◀ your turn";
  await updateSonarInfo();
}

async function playerFire(r, c) {
  if (state.play.phase !== "playing") return;
  if (cellAt(gameCells.enemy, r, c).classList.contains("miss")) return;
  if (cellAt(gameCells.enemy, r, c).classList.contains("hit")) return;
  if (cellAt(gameCells.enemy, r, c).classList.contains("sunk")) return;

  const res = await cmd("receive_shot", { r, c });
  const resultStr = res.result;
  state.play.moveNo++;

  logMove("me", r, c, resultStr);
  if (resultStr === "miss") {
    setCell(gameCells.enemy, r, c, "miss");
  } else if (resultStr === "hit") {
    setCell(gameCells.enemy, r, c, "hit");
  } else if (resultStr === "sunk") {
    setCell(gameCells.enemy, r, c, "sunk");
    // The engine auto-reveals the sunk neighbourhood as misses.
    refreshEnemyMisses();
  } else {
    return; // already / invalid
  }

  // Game over for the engine?
  const snap = await cmd("snapshot");
  const fleet = BigInt(snap.our_fleet_mask);
  const sunkM = BigInt(snap.our_sunk_mask);
  if (fleet !== 0n && (fleet & sunkM) === fleet) {
    endGame("win");
    return;
  }

  // Sonar's turn.
  $("#turnMarker").textContent = "sonar thinking…";
  $("#gameStatus").textContent = "Sonar is thinking…";
  await sonarTurn();
}

async function sonarTurn() {
  const mv = await cmd("choose_move", { deadline_secs: 0 });
  const { row, col } = mv;
  const idx = row * 10 + col;
  const my = state.play.myBoard;
  if (my.shotsAtMe.has(idx)) {
    // Should never happen (invariant-tested); guard anyway.
    return;
  }
  my.shotsAtMe.add(idx);
  state.play.moveNo++;

  let result = "miss";
  let sunkLen = 0;
  for (const ship of my.ships) {
    if (ship.cells.includes(idx)) {
      ship.hits++;
      if (ship.hits === ship.len) {
        ship.sunk = true;
        result = "sunk";
        sunkLen = ship.len;
      } else {
        result = "hit";
      }
      break;
    }
  }
  // Feed the precise result back (sunk length matters for reconstruction).
  await cmd("observe", {
    r: row,
    c: col,
    result: result === "sunk" ? `sunk_${sunkLen}` : result,
  });

  logMove("sonar", row, col, result);
  renderMyGameBoard();

  if (my.ships.every((s) => s.sunk)) {
    endGame("loss");
    return;
  }
  $("#gameStatus").textContent = "Your move — fire at enemy waters.";
  $("#turnMarker").textContent = "◀ your turn";
  await updateSonarInfo();
}

function renderMyGameBoard() {
  if (!gameCells.my) return;
  const my = state.play.myBoard;
  const heat = state.play.heatOn ? sonarHeatCache : null;
  for (const cell of gameCells.my) cell.className = "cell";
  for (let r = 0; r < 10; r++) {
    for (let c = 0; c < 10; c++) {
      const idx = r * 10 + c;
      const isShip = my.ships.some((s) => s.cells.includes(idx));
      const shot = my.shotsAtMe.has(idx);
      const ship = my.ships.find((s) => s.cells.includes(idx));
      if (shot && ship && ship.sunk) setCell(gameCells.my, r, c, "sunk");
      else if (shot && ship) setCell(gameCells.my, r, c, "hit");
      else if (shot) setCell(gameCells.my, r, c, "miss");
      else if (isShip) setCell(gameCells.my, r, c, "ship");
    }
  }
  // Heatmap overlay: sonar's live estimate of YOUR fleet.
  if (heat) {
    let max = 0;
    for (const v of heat) if (v > max) max = v;
    if (max > 0) {
      for (let i = 0; i < 100; i++) {
        const cell = gameCells.my[i];
        if (cell.classList.contains("miss")) continue;
        let overlay = cell.querySelector(".heat");
        if (!overlay) {
          overlay = document.createElement("div");
          overlay.className = "heat";
          cell.appendChild(overlay);
        }
        overlay.style.setProperty("--heat", String(heat[i] / max));
      }
    }
  } else {
    $$("#boardMyGame .heat").forEach((h) => h.remove());
  }
}

let sonarHeatCache = null;

async function updateSonarInfo() {
  try {
    const [snap, prob] = await Promise.all([cmd("snapshot"), cmd("probability")]);
    sonarHeatCache = Float32Array.from(prob.matrix);
    const d = snap.density_matrix || [];
    let dmax = 0;
    for (const v of d) if (v > dmax) dmax = v;
    const top = dmax > 0 ? Math.max(...d.map((v, i) => (snap.our_shots_mask && false ? 0 : v))) : 0;
    $("#sonarInfo").innerHTML = `
      <div class="row"><span>Sonar's hypotheses</span><b>${snap.hypothesis_count}</b></div>
      <div class="row"><span>Enemy ships left</span><b>${snap.enemy_remaining.join(", ") || "—"}</b></div>
      <div class="row"><span>Moves fired</span><b>${snap.moves_fired}</b></div>
      <div class="row"><span>Posterior mass (top cell)</span><b>${(Math.max(...prob.matrix) * 100).toFixed(1)}%</b></div>
    `;
    if (state.play.heatOn && gameCells.my) renderMyGameBoard();
  } catch {
    // Info is best-effort.
  }
}

function refreshEnemyMisses() {
  // The engine auto-marks sunk neighbourhoods; pull its view of our shots
  // through the next snapshot? The enemy board is OUR view — we mark
  // neighbours ourselves (standard rule).
  // Nothing extra needed: misses are marked on fire.
}

function logMove(who, r, c, result) {
  const li = document.createElement("li");
  li.className = `${result} ${who === "me" ? "me" : ""}`;
  const coord = `${COLS[c]}${r + 1}`;
  const label = who === "me" ? "you" : "sonar";
  li.innerHTML = `<span class="who">${label}</span> fired ${coord} → ${result}`;
  const log = $("#moveLog");
  log.prepend(li);
}

function endGame(outcome) {
  state.play.phase = "over";
  $("#turnMarker").textContent = "";
  const h = $("#gameStatus");
  if (outcome === "win") {
    h.textContent = `🏆 You defeated Sonar in ${Math.ceil(state.play.moveNo / 2)} of your shots!`;
  } else {
    h.textContent = `Sonar sank your fleet in ${state.play.moveNo} shots. New game?`;
  }
  updateSonarInfo();
}

// ── TEAM MODE ───────────────────────────────────────────────────────────────

let teamCells = null;

function initTeam() {
  teamCells = makeBoard($("#boardTeam"), {});
  $("#teamDifficulty").addEventListener("change", (ev) => {
    state.team.difficulty = Number(ev.target.value);
  });
  $("#btnTeamNew").addEventListener("click", teamNew);
  $$(".result-btn").forEach((btn) => {
    btn.addEventListener("click", () => teamReportResult(btn.dataset.result));
  });
  // Board clicks act as "I fired here" (recording the shot).
  $("#boardTeam").addEventListener("click", (ev) => {
    const cell = ev.target.closest(".cell:not(.coord)");
    if (!cell || !state.team.started) return;
    const r = Number(cell.dataset.r);
    const c = Number(cell.dataset.c);
    if (cell.classList.contains("miss") || cell.classList.contains("hit") || cell.classList.contains("sunk")) return;
    teamFire(r, c);
  });
}

async function teamNew() {
  const seed = Number(cryptoSeed() % 2n ** 31n);
  await cmd("new_game", {
    seed,
    config: {
      use_learning: false,
      hypothesis_soft_target: state.team.difficulty,
      default_deadline_secs: 0,
    },
  });
  state.team.started = true;
  state.team.log = [];
  $("#teamLog").innerHTML = "";
  for (const cell of teamCells) cell.className = "cell";
  await teamSuggest();
  $("#teamInfo").innerHTML = "";
}

async function teamFire(r, c) {
  // The human fired at a real opponent; the result is entered separately.
  // Record the pending shot.
  state.team.pending = { r, c };
  $$(".result-btn").forEach((b) => (b.style.outline = ""));
  $("#teamSuggest").innerHTML = `Shot recorded at <span class="coord">${COLS[c]}${r + 1}</span><small>now enter the real-world result →</small>`;
}

async function teamReportResult(result) {
  const p = state.team.pending;
  if (!p) return;
  const res = result === "sunk" ? `sunk_${$("#teamSunkLen").value || 2}` : result;
  await cmd("observe", { r: p.r, c: p.c, result: res });
  setCell(teamCells, p.r, p.c, result === "miss" ? "miss" : result);
  if (result === "sunk") {
    // Mark the neighbourhood as revealed (standard rule).
    for (let dr = -1; dr <= 1; dr++) {
      for (let dc = -1; dc <= 1; dc++) {
        const nr = p.r + dr;
        const nc = p.c + dc;
        if (nr < 0 || nr > 9 || nc < 0 || nc > 9) continue;
        const cell = cellAt(teamCells, nr, nc);
        if (cell.className === "cell ") cell.className = "cell";
        if (!cell.classList.contains("hit") && !cell.classList.contains("sunk") && !cell.classList.contains("miss")) {
          // Only mark as miss-like hint.
          cell.classList.add("miss");
        }
      }
    }
  }
  const li = document.createElement("li");
  li.className = result;
  li.innerHTML = `<span class="who">you</span> ${COLS[p.c]}${p.r + 1} → ${result}`;
  $("#teamLog").prepend(li);
  state.team.pending = null;
  await teamSuggest();
}

async function teamSuggest() {
  const t0 = performance.now();
  const sug = await cmd("suggest_move", { deadline_secs: 0 });
  const dt = (performance.now() - t0).toFixed(1);
  $("#teamSuggest").innerHTML = `Fire at <span class="coord">${COLS[sug.col]}${sug.row + 1}</span>
    <small>confidence ${(sug.confidence * 100).toFixed(1)}% · ${sug.hypothesis_count} hypotheses · ${sug.elapsed_us} μs</small>`;
  // Highlight the suggested cell.
  for (const cell of teamCells) cell.classList.remove("preview");
  const sc = cellAt(teamCells, sug.row, sug.col);
  if (!sc.classList.contains("miss") && !sc.classList.contains("hit")) {
    sc.classList.add("preview");
  }
  await updateTeamInfo(sug);
}

async function updateTeamInfo(_sug) {
  const prob = await cmd("probability");
  const snap = await cmd("snapshot");
  $("#teamInfo").innerHTML = `
    <div class="row"><span>Hypotheses</span><b>${snap.hypothesis_count}</b></div>
    <div class="row"><span>Enemy ships left</span><b>${snap.enemy_remaining.join(", ") || "—"}</b></div>
    <div class="row"><span>Moves fired</span><b>${snap.moves_fired}</b></div>
    <div class="row"><span>Top posterior</span><b>${(Math.max(...prob.matrix) * 100).toFixed(1)}%</b></div>
  `;
}

// ── MODS ────────────────────────────────────────────────────────────────────

function initMods() {
  const sel = $("#modExamples");
  for (const [key, ex] of Object.entries(EXAMPLE_MODS)) {
    const opt = document.createElement("option");
    opt.value = key;
    opt.textContent = `${ex.title} — ${ex.description.slice(0, 60)}…`;
    sel.appendChild(opt);
  }
  sel.addEventListener("change", () => {
    const ex = EXAMPLE_MODS[sel.value];
    if (ex) {
      $("#modEditor").value = ex.source;
    }
  });
  $("#modEditor").value = EXAMPLE_MODS.parityHunter.source;
  $("#btnRunMod").addEventListener("click", runModArenaUi);
}

async function runModArenaUi() {
  const code = $("#modEditor").value;
  const games = Math.max(1, Math.min(200, Number($("#modGames").value) || 20));
  const soft = Number($("#modDifficulty").value);
  const btn = $("#btnRunMod");
  btn.disabled = true;
  btn.textContent = "Running…";
  const bar = $("#modProgress");
  bar.hidden = false;
  bar.firstElementChild.style.width = "0%";
  $("#modReport").innerHTML = "Starting arena…";

  const res = await call(
    "modmatch",
    { code, games, softTarget: soft },
    (p) => {
      bar.firstElementChild.style.width = `${(p.done / p.total) * 100}%`;
    }
  );
  btn.disabled = false;
  btn.textContent = "Run arena";

  if (!res.ok) {
    $("#modReport").innerHTML = `<div class="err">✖ ${escapeHtml(res.error)}</div>`;
    return;
  }
  const rep = res.report;
  const ci = rep.wilson95;
  $("#modReport").innerHTML = `
    <div class="big">${rep.modWins}<span style="color:var(--muted);font-size:15px"> / ${rep.total} wins</span></div>
    <div>Win rate <b>${rep.modWinRate.toFixed(1)}%</b> <span class="muted">(Wilson 95%: [${ci[0].toFixed(1)}%, ${ci[1].toFixed(1)}%])</span></div>
    <div>Avg moves · wins <b>${rep.total - rep.modWins > 0 ? (rep.movesInModWins / rep.modWins).toFixed(1) : "—"}</b> · losses <b>${rep.total - rep.modWins > 0 ? (rep.movesInEngineWins / (rep.total - rep.modWins)).toFixed(1) : "—"}</b></div>
    ${rep.forfeits ? `<div class="err">${rep.forfeits} forfeited (illegal moves)</div>` : ""}
    ${rep.errors.length ? `<div class="err">${rep.errors.slice(0, 5).map(escapeHtml).join("<br/>")}</div>` : ""}
    <table><tr><th>game</th><th>winner</th><th>moves</th></tr>
      ${rep.gamesList.slice(0, 12).map((g, i) => `<tr><td>${i + 1}</td><td class="${g.winner === "mod" ? "win" : "loss"}">${g.winner}${g.forfeited ? " (forfeit)" : ""}</td><td>${g.moves}</td></tr>`).join("")}
    </table>
  `;
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[c]);
}

// ── TEST TAB ────────────────────────────────────────────────────────────────

function initTests() {
  $("#btnRunTests").addEventListener("click", runTests);
}

async function runTests() {
  const btn = $("#btnRunTests");
  btn.disabled = true;
  btn.textContent = "Running…";
  const box = $("#testResults");
  box.innerHTML = "";
  const bar = $("#testProgress");
  bar.hidden = false;
  bar.firstElementChild.style.width = "0%";

  const res = await call("testsuite", {}, (p) => {
    if (p.entry && p.entry.name === "__summary__") return;
    if (p.entry) {
      box.appendChild(testRow(p.entry));
      box.scrollTop = box.scrollHeight;
    }
  });
  btn.disabled = false;
  btn.textContent = "Run test suite";

  if (!res.ok) {
    box.innerHTML = `<div class="test-row fail"><span class="status">FAIL</span><span class="name">suite crashed</span><span class="detail">${escapeHtml(res.error)}</span></div>`;
    return;
  }
  const s = res.summary;
  bar.firstElementChild.style.width = "100%";
  const row = document.createElement("div");
  row.className = `test-row summary ${s.allOk ? "" : "fail"}`;
  row.innerHTML = `<span class="status">${s.allOk ? "✓ ALL" : "✖ FAIL"}</span>
    <span class="name">${s.passed}/${s.total} checks passed</span>`;
  box.appendChild(row);
}

function testRow(entry) {
  const row = document.createElement("div");
  row.className = `test-row ${entry.ok ? "pass" : "fail"}`;
  row.innerHTML = `<span class="status">${entry.ok ? "✓ PASS" : "✖ FAIL"}</span>
    <span class="name">${escapeHtml(entry.name)}</span>
    <span class="detail">${escapeHtml(entry.detail || "")}</span>`;
  return row;
}

// ── DOCS TAB ────────────────────────────────────────────────────────────────

const docState = { rendered: false, activeSection: null };

function initDocs() {
  $("#docSearch").addEventListener("input", (ev) => {
    const q = ev.target.value.trim();
    if (q.length < 2) {
      $("#docSearchResults").hidden = true;
      $("#docBody").hidden = false;
      $("#docNav").hidden = false;
      return;
    }
    renderSearchResults(q);
  });
}

function renderDocs() {
  docState.rendered = true;
  const nav = $("#docNav");
  nav.innerHTML = "";
  const body = $("#docBody");
  body.innerHTML = "";
  for (const sec of DOCS.sections) {
    const a = document.createElement("a");
    a.href = `#doc-${sec.id}`;
    a.textContent = sec.title;
    a.addEventListener("click", (ev) => {
      ev.preventDefault();
      showSection(sec.id);
    });
    nav.appendChild(a);

    const section = document.createElement("section");
    section.className = "doc-section";
    section.id = `doc-${sec.id}`;
    const h = document.createElement("h2");
    h.textContent = sec.title;
    section.appendChild(h);
    for (const p of sec.body) {
      const para = document.createElement("p");
      para.textContent = p;
      section.appendChild(para);
    }
    body.appendChild(section);
  }
  showSection(DOCS.sections[0].id);
}

function showSection(id) {
  const sec = DOCS.sections.find((s) => s.id === id);
  if (!sec) return;
  docState.activeSection = id;
  $$("#docNav a").forEach((a) =>
    a.classList.toggle("active", a.getAttribute("href") === `#doc-${id}`)
  );
  document.getElementById(`doc-${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  $("#docSearchResults").hidden = true;
  $("#docBody").hidden = false;
  $("#docNav").hidden = false;
}

function renderSearchResults(q) {
  const results = searchDocs(q);
  const box = $("#docSearchResults");
  box.innerHTML = "";
  if (results.length === 0) {
    box.innerHTML = `<div class="search-result"><div class="title">No matches for “${escapeHtml(q)}”</div>
      <div class="snippet">Try: modding, protocol, probability, wasm, wilson, determinism…</div></div>`;
  } else {
    for (const r of results) {
      const el = document.createElement("div");
      el.className = "search-result";
      const snippet = highlightTerms(r.snippet, r.terms);
      el.innerHTML = `<span class="score">${r.score.toFixed(2)}</span>
        <div class="title">${escapeHtml(r.title)}</div>
        <div class="snippet">${snippet}</div>`;
      el.addEventListener("click", () => showSection(r.id));
      box.appendChild(el);
    }
  }
  box.hidden = false;
  $("#docBody").hidden = true;
  $("#docNav").hidden = true;
}

function highlightTerms(text, terms) {
  let html = escapeHtml(text);
  for (const t of terms) {
    if (t.length < 2) continue;
    const re = new RegExp(`(${escapeRegExp(t)}[a-z0-9_]*)`, "gi");
    html = html.replace(re, "<mark>$1</mark>");
  }
  return html;
}

function escapeRegExp(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
