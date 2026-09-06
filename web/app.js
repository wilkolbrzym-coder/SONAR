/**
 * Sonar web app — main-thread UI logic.
 *
 * Material Design 3 (Expressive) interface. All engine work happens in
 * the Web Worker (worker.js); this file renders state and forwards user
 * actions. Tabs: Play (game modes), Benchmarks, Tests, Docs.
 *
 * Engine slots (worker instances):
 *   0 — the classic duel + advisor (shared; entering a mode starts fresh)
 *   1 — the solo hunt (variant engine state persists across tab switches)
 */

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
  if (msg.type === "progress" && progressHandlers.has(msg.id)) {
    progressHandlers.get(msg.id)(msg);
    return;
  }
  if (msg.type === "test-progress" && progressHandlers.has(msg.id)) {
    progressHandlers.get(msg.id)(msg);
    return;
  }
  if (msg.type === "bench-progress" && progressHandlers.has(msg.id)) {
    progressHandlers.get(msg.id)(msg);
    return;
  }
  const cb = pending.get(msg.id);
  if (cb) {
    pending.delete(msg.id);
    if (progressHandlers.has(msg.id)) progressHandlers.delete(msg.id);
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

/** Send one JSON protocol line to an engine slot; returns the parsed reply. */
async function cmd(name, extra = {}, slot = 0) {
  const res = await call("cmd", { line: JSON.stringify({ cmd: name, ...extra }), slot });
  if (!res.ok) throw new Error(res.error);
  return JSON.parse(res.reply);
}

// ── Boot ────────────────────────────────────────────────────────────────────

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => Array.from(document.querySelectorAll(sel));

const state = {
  version: null,
  // Duel (classic 10×10 vs the engine).
  play: {
    phase: "setup", // setup | playing | over
    difficulty: 256,
    myShips: [],
    rotate: true,
    selectedLen: null,
    myBoard: null,
    moveNo: 0,
    heatOn: false,
  },
  // Solo hunt (variant engine, slot 1).
  hunt: {
    preset: null,
    w: 0,
    h: 0,
    ships: 0,
    started: false,
    over: false,
    hints: false,
    heat: false,
    holes: new Set(),
    lastDensity: null,
    moves: 0,
  },
  // Advisor (paper game).
  team: {
    started: false,
    difficulty: 256,
    log: [],
  },
};

async function boot() {
  initTheme();
  try {
    const hello = await call("hello");
    state.version = hello.hello;
    $("#brandVersion").textContent = hello.hello.version;
    $("#footerVersion").textContent = hello.hello.version;
    const es = $("#engineState");
    es.textContent = `engine ${hello.hello.version} · wasm ready`;
    es.classList.add("ok");
  } catch (e) {
    const es = $("#engineState");
    es.textContent = "engine failed to load";
    es.classList.add("err");
    console.error(e);
  }
  initTabs();
  initPlay();
  initBench();
  initTests();
  initDocs();
}
boot();

// ── Theme (MD3 dark default, light optional) ────────────────────────────────

function initTheme() {
  const saved = localStorage.getItem("sonar-theme");
  const preferLight = window.matchMedia?.("(prefers-color-scheme: light)").matches;
  setTheme(saved || (preferLight ? "light" : "dark"));
  $("#btnTheme").addEventListener("click", () => {
    const cur = document.documentElement.dataset.theme || "dark";
    setTheme(cur === "dark" ? "light" : "dark");
  });
}

function setTheme(theme) {
  document.documentElement.dataset.theme = theme;
  localStorage.setItem("sonar-theme", theme);
}

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

// ── Play: mode routing ──────────────────────────────────────────────────────

const HUNT_MODES = {
  hunt_micro: { preset: "micro", title: "Skirmish 7×7" },
  hunt_classic: { preset: "classic", title: "Classic Hunt" },
  hunt_torus: { preset: "torus8", title: "Torus 8×8" },
  hunt_big16: { preset: "big16", title: "Big Ocean 16×16" },
  hunt_archipelago: { preset: "archipelago12", title: "Archipelago 12×12" },
  hunt_poly: { preset: "poly10", title: "Polyomino Fleet" },
};

const PLAY_SCREENS = ["playHome", "playSetup", "playGame", "huntGame", "advisorGame"];

function showPlayScreen(name) {
  for (const s of PLAY_SCREENS) {
    $("#" + s).hidden = s !== name;
  }
}

function initPlay() {
  // Mode cards.
  $$(".mode-card").forEach((card) => {
    card.addEventListener("click", () => {
      const mode = card.dataset.mode;
      if (mode === "duel") {
        enterDuelSetup();
      } else if (mode === "advisor") {
        enterAdvisor();
      } else if (HUNT_MODES[mode]) {
        startHunt(mode);
      }
    });
  });
  for (const id of ["btnBackHome1", "btnBackHome2", "btnBackHome3", "btnBackHome4"]) {
    $("#" + id).addEventListener("click", () => showPlayScreen("playHome"));
  }

  initDuel();
  initHunt();
}

// ── Shared board rendering (any size) ───────────────────────────────────────

function makeBoard(el, n, { attack = false, onCell } = {}) {
  el.innerHTML = "";
  el.style.setProperty("--n", n + 1);
  sizeCells(el, n);
  const cells = [];
  el.appendChild(div("cell coord", ""));
  for (let c = 0; c < n; c++) el.appendChild(div("cell coord", colName(c)));
  for (let r = 0; r < n; r++) {
    el.appendChild(div("cell coord", String(r + 1)));
    for (let c = 0; c < n; c++) {
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

/** Column names A..Z, AA.. for wide boards. */
function colName(c) {
  let name = "";
  c++;
  while (c > 0) {
    const m = (c - 1) % 26;
    name = String.fromCharCode(65 + m) + name;
    c = Math.floor((c - 1) / 26);
  }
  return name;
}

/** Pick a readable cell size so any board fits the viewport width. */
function sizeCells(el, n) {
  const avail = Math.min(window.innerWidth - 56, 900);
  const coords = 34; // coordinate column + padding allowance
  const px = Math.max(14, Math.min(40, Math.floor((avail - coords) / n) - 3));
  el.style.setProperty("--cell", px + "px");
}

function div(cls, text) {
  const d = document.createElement("div");
  d.className = cls;
  d.textContent = text;
  return d;
}

function cellAt(cells, r, c, n) {
  return cells[r * n + c];
}

function setCellClass(cells, r, c, n, cls) {
  cellAt(cells, r, c, n).className = `cell ${cls}`;
}

function addCellClass(cells, r, c, n, cls) {
  cellAt(cells, r, c, n).classList.add(cls);
}

// ── DUEL: fleet placement ───────────────────────────────────────────────────

const PALETTE_SHIPS = [
  { len: 5, label: "Carrier" },
  { len: 4, label: "Battleship" },
  { len: 3, label: "Submarine" },
  { len: 3, label: "Cruiser" },
  { len: 2, label: "Destroyer" },
];

let setupCells = [];

function initDuel() {
  setupCells = makeBoard($("#boardMy"), 10, {
    onCell: (r, c) => handleSetupClick(r, c),
  });
  $("#boardMy").addEventListener("mouseover", (ev) => {
    const cell = ev.target.closest(".cell:not(.coord)");
    if (!cell || state.play.phase !== "setup" || $("#playSetup").hidden) return;
    updatePreview(Number(cell.dataset.r), Number(cell.dataset.c));
  });
  $("#boardMy").addEventListener("mouseleave", clearPreview);

  $$(".ship-chip").forEach((chip) => {
    chip.addEventListener("click", () => {
      const len = Number(chip.dataset.len);
      const total = PALETTE_SHIPS.filter((s) => s.len === len).length;
      const used = state.play.myShips.filter((s) => s.len === len).length;
      if (used >= total) return;
      selectChip(len);
    });
  });

  $("#btnRotate").addEventListener("click", () => {
    state.play.rotate = !state.play.rotate;
    clearPreview();
  });
  document.addEventListener("keydown", (ev) => {
    if ((ev.key === "r" || ev.key === "R") && !$("#playSetup").hidden && state.play.phase === "setup") {
      state.play.rotate = !state.play.rotate;
      clearPreview();
    }
  });
  $("#btnAutoPlace").addEventListener("click", () => autoPlace(true));
  $("#btnRandomPlace").addEventListener("click", () => autoPlace(false));
  $("#btnClearFleet").addEventListener("click", clearPlacement);
  $("#btnStartGame").addEventListener("click", startBattle);
  $("#btnNewGame").addEventListener("click", () => enterDuelSetup());
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

function enterDuelSetup() {
  state.play.phase = "setup";
  state.play.myShips = [];
  state.play.selectedLen = null;
  showPlayScreen("playSetup");
  $("#moveLog").innerHTML = "";
  $("#sonarInfo").innerHTML = "";
  $("#gameStatus").textContent = "Your move";
  refreshSetupBoard();
  updateSetupStatus();
  selectChip(5);
}

function selectChip(len) {
  state.play.selectedLen = len;
  $$(".ship-chip").forEach((chip) => {
    chip.classList.toggle("selected", Number(chip.dataset.len) === len);
  });
  refreshChipPlaced();
}

function refreshChipPlaced() {
  $$(".ship-chip").forEach((chip) => {
    const len = Number(chip.dataset.len);
    const used = state.play.myShips.filter((s) => s.len === len).length;
    const total = PALETTE_SHIPS.filter((s) => s.len === len).length;
    chip.classList.toggle("placed", used >= total);
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
    const r = Math.floor(idx / 10);
    const c = idx % 10;
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

function handleSetupClick(r, c) {
  const len = state.play.selectedLen;
  if (!len) return;
  const cells = shipCellsAt(r, c, len, state.play.rotate);
  const fits =
    cells.every((i) => i >= 0 && i <= 99) &&
    (state.play.rotate ? c + len <= 10 : r + len <= 10);
  if (!fits) return flashStatus("Ship does not fit there.");
  if (!placementLegal(cells)) return flashStatus("Ships may not touch each other.");

  state.play.myShips.push({ r, c, len, horizontal: state.play.rotate, cells });
  refreshSetupBoard();
  updateSetupStatus();

  // Auto-advance to the next unplaced ship type.
  const next = PALETTE_SHIPS.find((ps) => {
    const used = state.play.myShips.filter((s) => s.len === ps.len).length;
    const total = PALETTE_SHIPS.filter((s) => s.len === ps.len).length;
    return used < total;
  });
  if (next) {
    selectChip(next.len);
  } else {
    state.play.selectedLen = null;
    $$(".ship-chip").forEach((chip) => chip.classList.remove("selected"));
  }
}

function refreshSetupBoard() {
  for (const cell of setupCells) cell.className = "cell";
  for (const s of state.play.myShips) {
    for (const idx of s.cells) {
      setCellClass(setupCells, Math.floor(idx / 10), idx % 10, 10, "ship");
    }
  }
  refreshChipPlaced();
}

function updatePreview(r, c) {
  clearPreview();
  const len = state.play.selectedLen;
  if (!len) return;
  const cells = shipCellsAt(r, c, len, state.play.rotate);
  const fits =
    cells.every((i) => i >= 0 && i <= 99) &&
    (state.play.rotate ? c + len <= 10 : r + len <= 10);
  const legal = fits && placementLegal(cells);
  for (const idx of cells) {
    if (idx < 0 || idx > 99) continue;
    const cell = cellAt(setupCells, Math.floor(idx / 10), idx % 10, 10);
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
  el.style.color = "var(--sunk)";
  setTimeout(() => {
    el.style.color = "";
    updateSetupStatus();
  }, 1400);
}

async function autoPlace(smart) {
  // Ask the engine for a placement and reconstruct the ships from the mask.
  const seed = Number(cryptoSeed() % 2n ** 31n);
  await cmd("new_game", {
    seed,
    config: duelConfig(),
  });
  await cmd(smart ? "place_smart" : "place_random");
  const snap = await cmd("snapshot");
  state.play.myShips = reconstructFleet(BigInt(snap.our_fleet_mask));
  refreshSetupBoard();
  updateSetupStatus();
}

function duelConfig() {
  return {
    use_learning: false,
    hypothesis_soft_target: state.play.difficulty,
    default_deadline_secs: 0,
  };
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
  selectChip(5);
  refreshSetupBoard();
  updateSetupStatus();
}

// ── DUEL: the battle ────────────────────────────────────────────────────────

const gameCells = { enemy: null, my: null };

async function startBattle() {
  if (state.play.myShips.length !== 5) return;
  const seed = Number(cryptoSeed() % 2n ** 31n);
  await cmd("new_game", { seed, config: duelConfig() });
  // Sonar places its fleet (hidden from us).
  await cmd("place_smart");

  state.play.phase = "playing";
  state.play.moveNo = 0;
  state.play.myBoard = {
    ships: state.play.myShips.map((s) => ({ ...s, hits: 0, sunk: false })),
    shotsAtMe: new Set(),
  };
  showPlayScreen("playGame");
  gameCells.enemy = makeBoard($("#boardEnemy"), 10, {
    attack: true,
    onCell: (r, c) => playerFire(r, c),
  });
  gameCells.my = makeBoard($("#boardMyGame"), 10);
  renderMyGameBoard();
  $("#moveLog").innerHTML = "";
  $("#gameStatus").textContent = "Your move — fire at enemy waters.";
  $("#turnMarker").textContent = "◀ your turn";
  await updateSonarInfo();
}

async function playerFire(r, c) {
  if (state.play.phase !== "playing") return;
  const cell = cellAt(gameCells.enemy, r, c, 10);
  if (cell.classList.contains("miss") || cell.classList.contains("hit") || cell.classList.contains("sunk")) return;

  const res = await cmd("receive_shot", { r, c });
  const resultStr = res.result;
  state.play.moveNo++;

  logMove("me", r, c, resultStr);
  if (resultStr === "miss") {
    setCellClass(gameCells.enemy, r, c, 10, "miss");
  } else if (resultStr === "hit") {
    setCellClass(gameCells.enemy, r, c, 10, "hit");
  } else if (resultStr === "sunk") {
    setCellClass(gameCells.enemy, r, c, 10, "sunk");
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
      const ship = my.ships.find((s) => s.cells.includes(idx));
      const shot = my.shotsAtMe.has(idx);
      if (shot && ship && ship.sunk) setCellClass(gameCells.my, r, c, 10, "sunk");
      else if (shot && ship) setCellClass(gameCells.my, r, c, 10, "hit");
      else if (shot) setCellClass(gameCells.my, r, c, 10, "miss");
      else if (ship) setCellClass(gameCells.my, r, c, 10, "ship");
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
        overlay.style.setProperty("--heat-o", String(heat[i] / max));
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

function logMove(who, r, c, result) {
  const li = document.createElement("li");
  li.className = `${result} ${who === "me" ? "me" : ""}`;
  const coord = `${colName(c)}${r + 1}`;
  const label = who === "me" ? "you" : "sonar";
  li.innerHTML = `<span class="who">${label}</span> fired ${coord} → ${result}`;
  $("#moveLog").prepend(li);
}

function endGame(outcome) {
  state.play.phase = "over";
  $("#turnMarker").textContent = "";
  const h = $("#gameStatus");
  if (outcome === "win") {
    h.textContent = `You defeated Sonar in ${Math.ceil(state.play.moveNo / 2)} of your shots!`;
  } else {
    h.textContent = `Sonar sank your fleet in ${state.play.moveNo} shots. New battle?`;
  }
  updateSonarInfo();
}

// ── SOLO HUNT (variant engine, engine slot 1) ───────────────────────────────

let huntCells = null;

function initHunt() {
  $("#btnHuntNew").addEventListener("click", () => startHunt(state.hunt.modeKey || "hunt_classic"));
  $("#huntHints").addEventListener("change", (ev) => {
    state.hunt.hints = ev.target.checked;
    if (!state.hunt.hints) clearHintHighlight();
    renderHuntSuggest();
    renderHuntHeat();
  });
  $("#huntHeat").addEventListener("change", (ev) => {
    state.hunt.heat = ev.target.checked;
    renderHuntHeat();
  });
}

async function startHunt(modeKey) {
  const mode = HUNT_MODES[modeKey];
  if (!mode) return;
  const seed = Number(cryptoSeed() % 2n ** 31n);
  const info = await cmd("variant_new", { preset: mode.preset, seed }, 1);

  state.hunt = {
    modeKey,
    preset: mode.preset,
    title: mode.title,
    w: info.width,
    h: info.height,
    ships: info.ships,
    totalCells: info.width * info.height,
    shipCells: info.total_ship_cells,
    started: true,
    over: false,
    hints: $("#huntHints").checked,
    heat: $("#huntHeat").checked,
    holes: new Set((info.holes || []).map(([r, c]) => r * info.width + c)),
    lastDensity: null,
    moves: 0,
    backend: info.simd_backend,
  };

  $("#huntTitle").textContent = mode.title;
  $("#huntBoardLabel").textContent =
    `Hidden fleet — ${info.width}×${info.height}, ${info.ships} ships, ${info.total_ship_cells} ship cells`;
  $("#huntLog").innerHTML = "";
  $("#huntBanner").hidden = true;
  showPlayScreen("huntGame");

  huntCells = makeBoard($("#boardHunt"), info.width, {
    attack: true,
    onCell: (r, c) => huntFire(r, c),
  });
  // Holes are static — render once.
  for (const idx of state.hunt.holes) {
    setCellClass(huntCells, Math.floor(idx / state.hunt.w), idx % state.hunt.w, state.hunt.w, "hole");
  }
  await refreshHuntInfo();
  renderHuntSuggest();
}

async function huntFire(r, c) {
  const h = state.hunt;
  if (!h.started || h.over) return;
  const cell = cellAt(huntCells, r, c, h.w);
  if (cell.classList.contains("miss") || cell.classList.contains("hit") || cell.classList.contains("sunk")) return;
  if (cell.classList.contains("hole")) return;

  const res = await cmd("variant_fire", { r, c }, 1);
  if (res.ok !== true) return;
  h.moves++;

  const idx = r * h.w + c;
  if (res.result === "miss") {
    setCellClass(huntCells, r, c, h.w, "miss");
  } else if (res.result === "hit") {
    setCellClass(huntCells, r, c, h.w, "hit");
  } else if (res.result === "sunk") {
    setCellClass(huntCells, r, c, h.w, "sunk");
    // The engine reconstructs the full sunk ship — pull it from the state.
    await revealSunkShip();
  }

  huntLogMove(r, c, res.result, res.len);

  if (res.all_sunk) {
    h.over = true;
    await huntVictory();
    return;
  }
  await refreshHuntInfo();
  if (h.hints) await renderHuntSuggest();
  renderHuntHeat();
}

/** The variant snapshot's sunk mask holds the exact reconstructed ships. */
async function revealSunkShip() {
  const st = await cmd("variant_state", {}, 1);
  const sunkMask = BigInt("0x" + st.sunk);
  const w = st.width;
  for (let i = 0; i < w * st.height; i++) {
    if ((sunkMask >> BigInt(i)) & 1n) {
      const r = Math.floor(i / w);
      const c = i % w;
      const cell = cellAt(huntCells, r, c, w);
      if (!cell.classList.contains("hole")) {
        cell.className = "cell sunk";
      }
    }
  }
}

async function refreshHuntInfo() {
  const h = state.hunt;
  const st = await cmd("variant_state", {}, 1);
  h.lastDensity = st.density;
  const remaining = (st.remaining || []).map(describeShip);
  $("#huntInfo").innerHTML = `
    <div class="row"><span>Shots fired</span><b>${st.moves_fired}</b></div>
    <div class="row"><span>Ships remaining</span><b>${st.remaining.length}</b></div>
    <div class="row"><span>Fleet</span><b>${remaining.join(" · ") || "—"}</b></div>
    <div class="row"><span>Hunt still winnable</span><b>${st.feasibility.feasible ? "yes" : "IMPOSSIBLE"}</b></div>
    <div class="row"><span>SIMD backend</span><b>${st.simd_backend}</b></div>
  `;
}

function describeShip(spec) {
  if (spec.kind === "Line") return `${spec.len}`;
  if (spec.name) return spec.name;
  return "shape";
}

async function renderHuntSuggest() {
  const h = state.hunt;
  if (!h.hints || !h.started || h.over) {
    $("#huntSuggest").textContent = h.hints
      ? "Game over."
      : "Enable “Sonar hint” to see the engine's own next shot.";
    clearHintHighlight();
    return;
  }
  const m = await cmd("variant_move", {}, 1);
  if (m.ok !== true) {
    $("#huntSuggest").textContent = "No suggestions left.";
    return;
  }
  h.lastDensity = m.density;
  clearHintHighlight();
  addCellClass(huntCells, m.row, m.col, h.w, "hint");
  const counts = (m.legal_counts || []).join(" · ");
  $("#huntSuggest").innerHTML = `
    <div class="row"><span>Sonar would fire at</span><b>${colName(m.col)}${m.row + 1}</b></div>
    <div class="row"><span>Legal placements per ship</span><b>${counts || "—"}</b></div>
    <div class="row"><span>Engine</span><b>exact per-ship density</b></div>
  `;
}

function clearHintHighlight() {
  if (huntCells) $$("#boardHunt .cell.hint").forEach((c) => c.classList.remove("hint"));
}

function renderHuntHeat() {
  const h = state.hunt;
  if (!huntCells) return;
  $$("#boardHunt .heat").forEach((x) => x.remove());
  if (!h.heat || !h.lastDensity) return;
  let max = 0;
  for (const v of h.lastDensity) if (v > max) max = v;
  if (max <= 0) return;
  for (let i = 0; i < h.totalCells; i++) {
    const cell = huntCells[i];
    if (cell.classList.contains("hole")) continue;
    if (cell.classList.contains("miss") || cell.classList.contains("hit") || cell.classList.contains("sunk")) continue;
    const overlay = document.createElement("div");
    overlay.className = "heat";
    overlay.style.setProperty("--heat-o", String(h.lastDensity[i] / max));
    cell.appendChild(overlay);
  }
}

function huntLogMove(r, c, result, len) {
  const li = document.createElement("li");
  li.className = result === "sunk" ? "sunk" : result;
  const coord = `${colName(c)}${r + 1}`;
  const suffix = result === "sunk" && len ? ` (len ${len})` : "";
  li.innerHTML = `<span class="who">you</span> ${coord} → ${result}${suffix}`;
  $("#huntLog").prepend(li);
}

async function huntVictory() {
  const h = state.hunt;
  // Reference: the engine's own self-play average on this preset.
  let sonarShots = null;
  try {
    const r = await cmd("variant_play", { seed: 9000 }, 1);
    if (r.ok) sonarShots = r.shots;
  } catch {
    /* best effort */
  }
  const verdict =
    sonarShots === null || h.moves <= sonarShots
      ? "You matched or beat the engine's own hunt."
      : "The engine found it faster — hunt the density, not the fish.";
  $("#huntBanner").hidden = false;
  $("#huntBanner").innerHTML = `
    Fleet destroyed in ${h.moves} shots.
    <span class="sub">${verdict}${sonarShots !== null ? ` (Sonar self-play: ${sonarShots} shots, fresh fleet, same rules)` : ""}</span>
  `;
  await refreshHuntInfo();
  clearHintHighlight();
  renderHuntHeat();
}

// ── ADVISOR (paper game) ────────────────────────────────────────────────────

let teamCells = null;
let teamInit = false;

function enterAdvisor() {
  showPlayScreen("advisorGame");
  if (!teamInit) {
    teamInit = true;
    teamCells = makeBoard($("#boardTeam"), 10, {});
    initTeam();
  }
}

function initTeam() {
  $("#teamDifficulty").addEventListener("change", (ev) => {
    state.team.difficulty = Number(ev.target.value);
  });
  $("#btnTeamNew").addEventListener("click", teamNew);
  $$(".seg-btn").forEach((btn) => {
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
  state.team.pending = { r, c };
  $("#teamSuggest").innerHTML = `Shot recorded at <span class="coord">${colName(c)}${r + 1}</span><small>now enter the real-world result →</small>`;
}

async function teamReportResult(result) {
  const p = state.team.pending;
  if (!p) return;
  const res = result === "sunk" ? `sunk_${$("#teamSunkLen").value || 2}` : result;
  await cmd("observe", { r: p.r, c: p.c, result: res });
  setCellClass(teamCells, p.r, p.c, 10, result);
  if (result === "sunk") {
    // Mark the neighbourhood as revealed (standard rule).
    for (let dr = -1; dr <= 1; dr++) {
      for (let dc = -1; dc <= 1; dc++) {
        const nr = p.r + dr;
        const nc = p.c + dc;
        if (nr < 0 || nr > 9 || nc < 0 || nc > 9) continue;
        const cell = cellAt(teamCells, nr, nc, 10);
        if (!cell.classList.contains("hit") && !cell.classList.contains("sunk") && !cell.classList.contains("miss")) {
          cell.classList.add("miss");
        }
      }
    }
  }
  const li = document.createElement("li");
  li.className = result;
  li.innerHTML = `<span class="who">you</span> ${colName(p.c)}${p.r + 1} → ${result}`;
  $("#teamLog").prepend(li);
  state.team.pending = null;
  await teamSuggest();
}

async function teamSuggest() {
  const t0 = performance.now();
  const sug = await cmd("suggest_move", { deadline_secs: 0 });
  const dt = (performance.now() - t0).toFixed(1);
  $("#teamSuggest").innerHTML = `Fire at <span class="coord">${colName(sug.col)}${sug.row + 1}</span>
    <small>confidence ${(sug.confidence * 100).toFixed(1)}% · ${sug.hypothesis_count} hypotheses · ${sug.elapsed_us} μs</small>`;
  // Highlight the suggested cell.
  for (const cell of teamCells) cell.classList.remove("preview");
  const sc = cellAt(teamCells, sug.row, sug.col, 10);
  if (!sc.classList.contains("miss") && !sc.classList.contains("hit")) {
    sc.classList.add("preview");
  }
  await updateTeamInfo();
}

async function updateTeamInfo() {
  const prob = await cmd("probability");
  const snap = await cmd("snapshot");
  $("#teamInfo").innerHTML = `
    <div class="row"><span>Hypotheses</span><b>${snap.hypothesis_count}</b></div>
    <div class="row"><span>Enemy ships left</span><b>${snap.enemy_remaining.join(", ") || "—"}</b></div>
    <div class="row"><span>Moves fired</span><b>${snap.moves_fired}</b></div>
    <div class="row"><span>Top posterior</span><b>${(Math.max(...prob.matrix) * 100).toFixed(1)}%</b></div>
  `;
}

// ── BENCHMARKS TAB ──────────────────────────────────────────────────────────

function initBench() {
  $("#btnRunBench").addEventListener("click", runLiveBenchUi);
}

async function runLiveBenchUi() {
  const btn = $("#btnRunBench");
  btn.disabled = true;
  btn.textContent = "Running…";
  const box = $("#liveBenchResults");
  box.innerHTML = "";
  const bar = $("#benchProgress");
  bar.hidden = false;
  bar.firstElementChild.style.width = "0%";

  let stageCount = 0;
  const stages = 9;
  const res = await call("livebench", {}, (p) => {
    stageCount++;
    bar.firstElementChild.style.width = `${Math.min(100, (stageCount / stages) * 100)}%`;
    const line = document.createElement("div");
    line.className = "muted";
    line.textContent = p.label;
    box.appendChild(line);
  });
  btn.disabled = false;
  btn.textContent = "Run live benchmarks";
  bar.hidden = true;

  if (!res.ok) {
    box.innerHTML = `<div class="bench-card error-card"><h3>Live benchmark failed</h3><div class="sub">${escapeHtml(res.error)}</div></div>`;
    return;
  }
  bar.firstElementChild.style.width = "100%";
  renderLiveBench(res.report);
}

function renderLiveBench(r) {
  const box = $("#liveBenchResults");
  box.innerHTML = "";

  const mk = (html) => {
    const d = document.createElement("div");
    d.className = "bench-card";
    d.innerHTML = html;
    box.appendChild(d);
  };

  if (r.latency) {
    mk(`
      <h3>Per-move latency <span class="tag">work-limited · 256 hypotheses</span></h3>
      <div class="big">${fmtMs(r.latency.meanMs)} <small>mean</small></div>
      <div class="grid2">
        <div><div class="big" style="font-size:20px">${fmtMs(r.latency.p95Ms)}</div><div class="sub">p95</div></div>
        <div><div class="big" style="font-size:20px">${fmtMs(r.latency.maxMs)}</div><div class="sub">max</div></div>
        <div><div class="big" style="font-size:20px">${fmtMs(r.latency.minMs)}</div><div class="sub">min</div></div>
      </div>
      <div class="sub">${r.latency.samples} sampled moves (choose_move over the live protocol, after warm-up)</div>
    `);
  }

  const strength = (title, s, opp) => {
    if (!s) return;
    mk(`
      <h3>${title} <span class="tag">seeded · ${s.games} games</span></h3>
      <div class="big">${s.wins}<small>/${s.games} wins</small></div>
      <div class="sub">win rate <b>${s.winRate.toFixed(1)}%</b> · Wilson 95%: [${s.wilson[0].toFixed(1)}%, ${s.wilson[1].toFixed(1)}%]</div>
      <div class="sub">avg moves to win <b>${s.avgMoves}</b> · engine time ${(s.elapsedMs / 1000).toFixed(2)} s</div>
    `);
  };
  strength("Strength gate — vs Random", r.vsRandom);
  strength("Strength gate — vs PdfOnly", r.vsPdf);

  if (r.selfPlay) {
    mk(`
      <h3>Self-play throughput <span class="tag">hybrid vs hybrid · ${r.selfPlay.games} games</span></h3>
      <div class="big">${r.selfPlay.gamesPerSec ?? "—"} <small>games/s</small></div>
      <div class="sub">${r.selfPlay.msPerGame} ms/game · winner averages <b>${r.selfPlay.avgMovesWinner}</b> shots</div>
    `);
  }

  if (r.variants && r.variants.length) {
    const rows = r.variants
      .map(
        (v) => `
        <tr>
          <td>${v.preset}</td>
          <td>${v.ok ? v.avgShots : "—"}</td>
          <td>${v.ok ? v.msPerGame + " ms" : "error"}</td>
          <td>${v.ok ? v.gamesPerSec : "—"}</td>
        </tr>`
      )
      .join("");
    mk(`
      <h3>Generalised variants — self-play speed <span class="tag">${r.variants[0].games} games / preset</span></h3>
      <table class="data-table">
        <thead><tr><th>Preset</th><th>Avg shots</th><th>Time / game</th><th>Games / s</th></tr></thead>
        <tbody>${rows}</tbody>
      </table>
    `);
  }

  if (r.memory) {
    mk(`
      <h3>Footprint</h3>
      <div class="big">${r.memory.heapMB} <small>MB wasm heap</small></div>
      <div class="sub">SIMD backend: <b>${r.memory.backend || "scalar"}</b> · measured in this browser, on this machine</div>
    `);
  }
}

function fmtMs(x) {
  if (typeof x !== "number") return "—";
  return x >= 10 ? `${x.toFixed(1)} ms` : `${(x * 1000).toFixed(0)} µs`;
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

  let seen = 0;
  const res = await call("testsuite", {}, (p) => {
    if (p.entry && p.entry.name === "__summary__") {
      bar.firstElementChild.style.width = "100%";
      return;
    }
    if (p.entry) {
      seen++;
      bar.firstElementChild.style.width = `${Math.min(95, (seen / 19) * 95)}%`;
      box.appendChild(testRow(p.entry));
      box.scrollTop = box.scrollHeight;
    }
  });
  btn.disabled = false;
  btn.textContent = "Run test suite";

  if (!res.ok) {
    box.innerHTML = `<div class="test-row fail"><span class="status">✖ FAIL</span><span class="name">suite crashed</span><span class="detail">${escapeHtml(res.error)}</span></div>`;
    return;
  }
  const s = res.summary;
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
  const body = $("#docBody");
  nav.innerHTML = "";
  body.innerHTML = "";

  let lastGroup = null;
  for (const sec of DOCS.sections) {
    if (sec.group !== lastGroup) {
      lastGroup = sec.group;
      const g = document.createElement("div");
      g.className = "nav-group";
      g.textContent = sec.group;
      nav.appendChild(g);
    }
    const a = document.createElement("a");
    a.href = `#doc-${sec.id}`;
    a.textContent = sec.title;
    a.dataset.section = sec.id;
    a.addEventListener("click", (ev) => {
      ev.preventDefault();
      showSection(sec.id);
    });
    nav.appendChild(a);

    const section = document.createElement("section");
    section.className = "doc-section";
    section.id = `doc-${sec.id}`;

    const groupLabel = document.createElement("div");
    groupLabel.className = "nav-group-label";
    groupLabel.textContent = sec.group;
    section.appendChild(groupLabel);

    const h = document.createElement("h2");
    h.textContent = sec.title;
    section.appendChild(h);

    for (const p of sec.body) {
      const para = document.createElement("p");
      para.textContent = p;
      section.appendChild(para);
    }

    for (const ex of sec.examples || []) {
      section.appendChild(renderExample(ex));
    }

    body.appendChild(section);
  }
  showSection(DOCS.sections[0].id);
}

/** Render one documentation code example as an MD3 code block. */
function renderExample(ex) {
  const wrap = document.createElement("div");
  wrap.className = "doc-code";

  const head = document.createElement("div");
  head.className = "doc-code-head";
  head.textContent = ex.title;
  if (ex.verified) {
    const badge = document.createElement("span");
    badge.className = "verified";
    badge.textContent = Array.isArray(ex.run) ? "✓ executed in CI" : "✓ verified";
    head.appendChild(badge);
  }
  wrap.appendChild(head);

  const pre = document.createElement("pre");
  pre.textContent = ex.code;
  wrap.appendChild(pre);

  wrap.addEventListener("click", (ev) => {
    if (ev.target.classList?.contains("verified")) return;
  });

  return wrap;
}

function showSection(id) {
  const sec = DOCS.sections.find((s) => s.id === id);
  if (!sec) return;
  docState.activeSection = id;
  $$("#docNav a").forEach((a) =>
    a.classList.toggle("active", a.dataset.section === id)
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
      <div class="snippet">Try: protocol, density, torus, wasm, wilson, endgame, variant, license…</div></div>`;
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

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[c]);
}

function escapeRegExp(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
