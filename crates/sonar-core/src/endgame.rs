//! Exact-CENSUS endgame solver — submarine-perfect play in the last 1–2 ships.
//!
//! When the number of surviving enemy ships drops to 1–2, the full space of
//! legal fleet configurations becomes small enough to enumerate **exactly**.
//! The solver then:
//!
//! 1. **Census** — enumerates every legal joint configuration of the
//!    remaining ships that is consistent with all observations. The
//!    resulting posterior `P(ship at cell)` is *exact* (not sampled, unlike
//!    the Bayesian filter, which draws hypotheses at random).
//! 2. **Expectimax** — plays the move that minimises the *expected number
//!    of remaining shots* to sink the fleet, computed by exact game-tree
//!    search over the observation tree with memoisation. Within the node
//!    budget this is **provably optimal play** — for a single surviving
//!    ship ("the submarine") the solver plays perfectly: no policy can
//!    finish in fewer expected shots.
//!
//! When the position is too large for the budget (early 2-ship endgames
//! with heavy branching), the solver **defers** to the hybrid strategy
//! instead of playing a myopic greedy move — the hybrid's PDF + target
//! bonus is the stronger approximation in that regime. The solver only
//! takes control when it can play *exactly*; `stats()` then reports the
//! census size so callers can see which regime produced each move.
//!
//! ## Why a census and not more sampling?
//!
//! The Bayesian hypothesis filter samples configurations uniformly, so its
//! posterior carries sampling noise of order `1/sqrt(n)`. In the endgame
//! the difference between a 0.51 and a 0.49 posterior is often the
//! difference between winning and losing a tempo race. The census removes
//! that noise entirely: every legal configuration is counted exactly once
//! (ships of equal length are deduplicated by canonical ordering).
//!
//! ## Determinism
//!
//! The solver is fully deterministic — no RNG, no wall-clock dependence.
//! Two solvers given the same observation history always produce the same
//! move. This is a hard requirement of the adversarial-robustness contract
//! (see `targeting` module docs).

use crate::bitboard::BitBoard;
use crate::fleet::placements;
use crate::targeting::EnemyView;
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Tuning knobs for the exact-CENSUS endgame solver.
#[derive(Clone, Copy, Debug)]
pub struct EndgameConfig {
    /// Solve endgames with at most this many surviving ships.
    pub max_ships: usize,
    /// Hard cap on expectimax tree nodes. When exceeded, the solver falls
    /// back to greedy argmax of the exact posterior.
    pub node_budget: u64,
    /// Hard cap on enumerated configurations. When exceeded, the solver
    /// reports "no census" and the caller keeps the hybrid strategy.
    pub config_cap: usize,
    /// A no-hit census at or below this size is solved exactly. Above it,
    /// the fresh-hunt regime defers to the hybrid strategy.
    pub tractable_census: usize,
    /// With active hits on the board, attempt the exact solve up to this
    /// census size (hit-pinned censuses solve fast).
    pub hit_census_cap: usize,
    /// Master switch.
    pub enabled: bool,
}

impl Default for EndgameConfig {
    fn default() -> Self {
        Self {
            max_ships: 2,
            node_budget: 60_000,
            config_cap: 250_000,
            tractable_census: 64,
            hit_census_cap: 4_096,
            enabled: true,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Census — exact enumeration of consistent configurations
// ─────────────────────────────────────────────────────────────────────────────

/// The exact set of legal configurations of the remaining ships.
///
/// Each configuration is `[mask_a, mask_b]` — the cell masks of the (at
/// most two) surviving ships. With one ship remaining `mask_b == 0`.
#[derive(Clone, Debug)]
pub struct Census {
    /// One entry per distinct legal configuration.
    pub configs: Vec<[u128; 2]>,
    /// `coverage[i]` = number of configurations whose union covers cell `i`.
    pub coverage: [u32; 100],
    /// Number of ships in each configuration (1 or 2).
    pub ships: usize,
}

impl Default for Census {
    fn default() -> Self {
        Self {
            configs: Vec::new(),
            coverage: [0; 100],
            ships: 0,
        }
    }
}

impl Census {
    /// Total number of distinct configurations.
    pub fn total(&self) -> u32 {
        self.configs.len() as u32
    }

    /// Exact posterior `P(ship occupies cell i)`; `None` if the census is
    /// empty (an infeasible observation history).
    pub fn posterior(&self) -> Option<[f32; 100]> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        let mut m = [0.0f32; 100];
        for (mi, &count) in m.iter_mut().zip(self.coverage.iter()) {
            *mi = count as f32 / total as f32;
        }
        Some(m)
    }
}

/// Enumerate **all** legal configurations of the surviving ships that are
/// consistent with the observations in `view`.
///
/// Returns `None` when the position is outside the solver's regime
/// (more than 2 surviving ships, ship lengths outside 1..=5, no legal
/// configuration, or the config cap was exceeded).
///
/// Consistency rules (the classic 10×10 engine's rules, mirrored exactly):
/// - no ship cell on a known miss or a sunk cell,
/// - every active hit is covered by some ship,
/// - ships do not overlap and do not touch (8-neighbourhood),
/// - configurations are canonicalised: ships of equal length are emitted
///   in strictly increasing mask order, so each distinct layout is
///   counted exactly once.
pub fn enumerate_census(view: &EnemyView, cap: usize) -> Option<Census> {
    let mut remaining = view.remaining.clone();
    if remaining.is_empty() || remaining.len() > 2 {
        return None;
    }
    // Descending order: place the big ships first (better pruning), and
    // equal lengths end up adjacent for the canonical-ordering rule.
    remaining.sort_unstable_by(|a, b| b.cmp(a));

    for &len in &remaining {
        if !(1..=5).contains(&len) {
            return None; // outside the classic placement table
        }
    }

    let forbidden = view.miss_mask().0 | view.sunk.0;
    let hits_needed = view.hits.0 & !view.sunk.0; // active hits

    let mut configs: Vec<[u128; 2]> = Vec::new();
    let mut masks: Vec<u128> = Vec::with_capacity(2);
    enumerate_rec(
        &remaining,
        0,
        &mut masks,
        forbidden,
        hits_needed,
        &mut configs,
        cap,
    );

    if configs.is_empty() || configs.len() > cap {
        return None;
    }

    let mut coverage = [0u32; 100];
    for cfg in &configs {
        let union = cfg[0] | cfg[1];
        let mut m = union;
        while m != 0 {
            coverage[m.trailing_zeros() as usize] += 1;
            m &= m - 1;
        }
    }

    Some(Census {
        configs,
        coverage,
        ships: remaining.len(),
    })
}

/// Recursive backtracking over the surviving ships.
fn enumerate_rec(
    remaining: &[u8],
    idx: usize,
    masks: &mut Vec<u128>,
    forbidden: u128,
    hits_needed: u128,
    out: &mut Vec<[u128; 2]>,
    cap: usize,
) {
    if out.len() > cap {
        return; // budget exceeded — caller sees len > cap and bails
    }
    if idx == remaining.len() {
        // All ships placed: verify every active hit is covered.
        let union: u128 = masks.iter().copied().fold(0, |a, b| a | b);
        if (hits_needed & !union) == 0 {
            out.push([masks[0], *masks.get(1).unwrap_or(&0)]);
        }
        return;
    }

    let len = remaining[idx] as usize;
    let table = placements();
    if len >= table.len() {
        return;
    }
    // Canonical ordering: equal-length ships must be placed in strictly
    // increasing mask order so each layout is counted once.
    let same_len_prev = idx > 0 && remaining[idx - 1] == remaining[idx];
    let lower_bound = if same_len_prev {
        *masks.last().unwrap_or(&0)
    } else {
        0
    };

    let placed_union: u128 = masks.iter().copied().fold(0, |a, b| a | b);
    let placed_dilated = BitBoard(placed_union).dilate8().0;

    for &(_r, _c, _h, mask) in &table[len] {
        if same_len_prev && mask <= lower_bound {
            continue; // duplicate of an already-counted permutation
        }
        if (mask & forbidden) != 0 {
            continue; // overlaps a miss or sunk cell
        }
        if (mask & placed_union) != 0 || (mask & placed_dilated) != 0 {
            continue; // overlaps or touches an already-placed ship
        }
        // A placement that covers an active hit cell is impossible only if
        // the hit cell is outside every legal remaining placement — the
        // final check at idx == len handles coverage. But we can prune:
        // if this is the LAST ship and uncovered hits exist that this
        // placement does not cover, skip early.
        if idx + 1 == remaining.len() {
            let covered = placed_union | mask;
            if (hits_needed & !covered) != 0 {
                continue;
            }
        }
        masks.push(mask);
        enumerate_rec(remaining, idx + 1, masks, forbidden, hits_needed, out, cap);
        masks.pop();
        if out.len() > cap {
            return;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// EndgameSolver — expectimax over the exact census
// ─────────────────────────────────────────────────────────────────────────────

/// A solved endgame move. `EndgameMove` is only ever produced by the
/// **exact** expectimax — when the budget is exceeded the solver defers
/// to the caller's default strategy instead (`best_move` → `None`).
#[derive(Clone, Copy, Debug)]
pub struct EndgameMove {
    pub row: usize,
    pub col: usize,
    /// Expected number of remaining shots under optimal play.
    pub expected_shots: f64,
    /// Always `true` for moves returned by `best_move` (exact regime).
    pub exact: bool,
    /// Number of distinct legal configurations in the census.
    pub configs: u32,
}

/// Memoisation key: the full observation state. Two states with identical
/// masks and remaining fleets have identical subtrees.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ViewKey {
    shots: u128,
    hits: u128,
    sunk: u128,
    remaining: [u8; 3],
}

impl ViewKey {
    fn of(view: &EnemyView) -> Self {
        let mut rem = [0u8; 3];
        for (i, &l) in view.remaining.iter().take(3).enumerate() {
            rem[i] = l;
        }
        // Canonical: sorted descending, independent of removal order.
        rem.sort_unstable_by(|a, b| b.cmp(a));
        Self {
            shots: view.shots.0,
            hits: view.hits.0,
            sunk: view.sunk.0,
            remaining: rem,
        }
    }
}

/// The exact-CENSUS endgame solver.
///
/// `best_move` returns the expected-shots-optimal cell to fire at, or
/// `None` when the position is outside the solver's regime (in which case
/// the caller should keep its default strategy).
pub struct EndgameSolver {
    pub config: EndgameConfig,
    memo: HashMap<ViewKey, f64>,
    nodes: u64,
    /// Flip the budget trip-wire when the tree search is aborted.
    aborted: bool,
}

impl EndgameSolver {
    pub fn new(config: EndgameConfig) -> Self {
        Self {
            config,
            memo: HashMap::new(),
            nodes: 0,
            aborted: false,
        }
    }

    /// Drop all memoised subtrees (called on strategy reset).
    pub fn clear_memo(&mut self) {
        self.memo.clear();
    }

    /// Solve the position. Returns `None` if the census regime does not
    /// apply (too many ships / no legal configuration / the position is
    /// outside the exact-solve budget). `None` is a *deferral*: the caller
    /// keeps its default strategy, which is the stronger policy in the
    /// deferred regimes — the solver never plays a move it cannot prove
    /// is optimal.
    pub fn best_move(&mut self, view: &EnemyView) -> Option<EndgameMove> {
        if !self.config.enabled || view.remaining.is_empty() {
            return None;
        }
        if view.remaining.len() > self.config.max_ships {
            return None;
        }
        self.memo.clear();
        self.nodes = 0;
        self.aborted = false;

        let census = enumerate_census(view, self.config.config_cap)?;

        // ── Regime gate ─────────────────────────────────────────────
        // The expectimax is only worth its cost when the position is
        // information-rich: active hits pin the census to a handful of
        // configurations, and a small census solves exactly. In the
        // "fresh hunt" regime (no hits, large census) the hybrid's
        // PDF + parity search is the stronger policy, so the solver
        // defers to it.
        let active_hits = view.active_hits().0;
        let tractable = census.total() as usize <= self.config.tractable_census
            || (active_hits != 0 && census.total() as usize <= self.config.hit_census_cap);
        if !tractable {
            return None;
        }

        // Exact or defer: if the expectimax exceeds the node budget the
        // solver returns None and the hybrid keeps control. A myopic
        // greedy-posterior move measurably *loses* tempo in 2-ship
        // midgames (it prefers probing the un-wounded ship's likely
        // cells over finishing the wounded one), so it is not an
        // acceptable fallback.
        let e = self.expected_shots(view, &census)?;
        let (r, c) = self.argmin_expected(view, &census)?;
        Some(EndgameMove {
            row: r,
            col: c,
            expected_shots: e,
            exact: true,
            configs: census.total(),
        })
    }

    /// Exact expected remaining shots via memoised expectimax.
    ///
    /// `None` = budget exceeded (or a sub-census infeasible) — the caller
    /// defers to its default strategy.
    fn expected_shots(&mut self, view: &EnemyView, census: &Census) -> Option<f64> {
        if view.remaining.is_empty() {
            return Some(0.0);
        }
        let key = ViewKey::of(view);
        if let Some(&v) = self.memo.get(&key) {
            return Some(v);
        }
        self.nodes += 1;
        if self.nodes > self.config.node_budget || self.aborted {
            self.aborted = true;
            return None;
        }

        let mut best = f64::INFINITY;
        let total = census.total() as f64;

        for (r, c) in view.unknown().iter_cells() {
            let i = r * 10 + c;
            if census.coverage[i] == 0 {
                continue; // a certain miss carries no information — skip
            }
            let bit = 1u128 << i;

            // Group the census by outcome.
            let mut miss = 0u32;
            let mut hit = 0u32;
            let mut sinks: HashMap<u128, u32> = HashMap::new(); // sunk ship mask → count
            for cfg in &census.configs {
                if (cfg[0] & bit) == 0 && (cfg[1] & bit) == 0 {
                    miss += 1;
                } else {
                    // The ship that contains this cell.
                    let ship = if (cfg[0] & bit) != 0 { cfg[0] } else { cfg[1] };
                    let rest = ship & !bit;
                    if (rest & !view.hits.0) == 0 {
                        *sinks.entry(ship).or_insert(0) += 1;
                    } else {
                        hit += 1;
                    }
                }
            }

            let mut value = 1.0f64;

            if miss > 0 {
                let mut v2 = view.clone();
                v2.observe(r, c, crate::board::ShotResult::Miss);
                value += (miss as f64 / total) * self.recurse(&v2)?;
            }
            if hit > 0 {
                let mut v2 = view.clone();
                v2.observe(r, c, crate::board::ShotResult::Hit);
                value += (hit as f64 / total) * self.recurse(&v2)?;
            }
            for (&ship_mask, &n) in &sinks {
                let len = ship_mask.count_ones() as u8;
                let mut v2 = view.clone();
                v2.observe(r, c, crate::board::ShotResult::Sunk(len));
                value += (n as f64 / total) * self.recurse(&v2)?;
            }

            if value < best {
                best = value;
            }
        }

        if best == f64::INFINITY {
            // No candidate with coverage — every cell is a certain miss.
            // The position is degenerate; treat as one wasted shot.
            best = 1.0;
        }
        self.memo.insert(key, best);
        Some(best)
    }

    /// Recurse into a child state (re-enumerating its census).
    fn recurse(&mut self, view: &EnemyView) -> Option<f64> {
        if view.remaining.is_empty() {
            return Some(0.0);
        }
        let census = enumerate_census(view, self.config.config_cap)?;
        self.expected_shots(view, &census)
    }

    /// The argmin move over completed expectations. Only called after a
    /// successful `expected_shots`, so every child value resolves through
    /// the memo table (values are recomputed identically — memo hits).
    fn argmin_expected(&mut self, view: &EnemyView, census: &Census) -> Option<(usize, usize)> {
        let total = census.total() as f64;
        let mut best = f64::INFINITY;
        let mut best_cell = (0usize, 0usize);
        for (r, c) in view.unknown().iter_cells() {
            let i = r * 10 + c;
            if census.coverage[i] == 0 {
                continue;
            }
            let bit = 1u128 << i;
            let mut miss = 0u32;
            let mut hit = 0u32;
            let mut sinks: HashMap<u128, u32> = HashMap::new();
            for cfg in &census.configs {
                if (cfg[0] & bit) == 0 && (cfg[1] & bit) == 0 {
                    miss += 1;
                } else {
                    let ship = if (cfg[0] & bit) != 0 { cfg[0] } else { cfg[1] };
                    let rest = ship & !bit;
                    if (rest & !view.hits.0) == 0 {
                        *sinks.entry(ship).or_insert(0) += 1;
                    } else {
                        hit += 1;
                    }
                }
            }
            let mut value = 1.0f64;
            if miss > 0 {
                let mut v2 = view.clone();
                v2.observe(r, c, crate::board::ShotResult::Miss);
                value += (miss as f64 / total) * self.recurse(&v2)?;
            }
            if hit > 0 {
                let mut v2 = view.clone();
                v2.observe(r, c, crate::board::ShotResult::Hit);
                value += (hit as f64 / total) * self.recurse(&v2)?;
            }
            for (&ship_mask, &n) in &sinks {
                let len = ship_mask.count_ones() as u8;
                let mut v2 = view.clone();
                v2.observe(r, c, crate::board::ShotResult::Sunk(len));
                value += (n as f64 / total) * self.recurse(&v2)?;
            }
            if value < best {
                best = value;
                best_cell = (r, c);
            }
        }
        Some(best_cell)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::ShotResult;

    fn view_with_remaining(remaining: &[u8]) -> EnemyView {
        let mut v = EnemyView::new();
        v.remaining = remaining.to_vec();
        v
    }

    #[test]
    fn test_census_single_ship_empty_board() {
        let v = view_with_remaining(&[2]);
        let census = enumerate_census(&v, 250_000).unwrap();
        // 180 legal placements of a length-2 ship on an empty 10×10.
        assert_eq!(census.total(), 180);
        // The exact posterior sums to the ship length (2).
        let post = census.posterior().unwrap();
        let sum: f32 = post.iter().sum();
        assert!((sum - 2.0).abs() < 1e-3, "posterior sum {}", sum);
        // Every configuration has exactly 2 cells.
        for cfg in &census.configs {
            assert_eq!((cfg[0] | cfg[1]).count_ones(), 2);
            assert_eq!(cfg[1], 0);
        }
    }

    #[test]
    fn test_census_respects_miss() {
        let mut v = view_with_remaining(&[3]);
        v.observe(5, 5, ShotResult::Miss);
        let census = enumerate_census(&v, 250_000).unwrap();
        for cfg in &census.configs {
            assert_eq!(cfg[0] & (1u128 << 55), 0, "ship on a missed cell");
        }
        // A length-3 ship has 160 placements; those covering (5,5) start
        // at row/col 3, 4 or 5 in that line: 3 horizontal + 3 vertical = 6.
        assert_eq!(census.total(), 160 - 6);
    }

    #[test]
    fn test_census_identical_ships_deduplicated() {
        // Two length-3 ships: the census must count *distinct layouts*, not
        // ship permutations. Brute-force reference count:
        let table = placements();
        let p3 = &table[3];
        let mut reference = 0u32;
        for (i, &(_, _, _, m1)) in p3.iter().enumerate() {
            for &(_, _, _, m2) in p3.iter().skip(i + 1) {
                let d1 = BitBoard(m1).dilate8().0;
                if (m1 & m2) == 0 && (d1 & m2) == 0 {
                    reference += 1;
                }
            }
        }
        let v = view_with_remaining(&[3, 3]);
        let census = enumerate_census(&v, 250_000).unwrap();
        assert_eq!(
            census.total(),
            reference,
            "census must deduplicate ship permutations"
        );
        // Every entry is canonical: mask[0] < mask[1].
        for cfg in &census.configs {
            assert!(cfg[0] < cfg[1] || cfg[1] == 0);
        }
    }

    #[test]
    fn test_census_requires_hits_covered() {
        let mut v = view_with_remaining(&[2]);
        v.observe(0, 0, ShotResult::Hit);
        let census = enumerate_census(&v, 250_000).unwrap();
        assert!(census.total() > 0);
        for cfg in &census.configs {
            assert_ne!(cfg[0] & 1u128, 0, "active hit must be covered");
        }
    }

    #[test]
    fn test_census_infeasible_returns_none() {
        // Hits in two far-apart cells cannot both belong to one length-2 ship.
        let mut v = view_with_remaining(&[2]);
        v.observe(0, 0, ShotResult::Hit);
        v.observe(9, 9, ShotResult::Hit);
        assert!(enumerate_census(&v, 250_000).is_none());
    }

    #[test]
    fn test_census_rejects_out_of_regime() {
        // Three surviving ships: outside the solver regime.
        let v = view_with_remaining(&[5, 4, 3]);
        assert!(enumerate_census(&v, 250_000).is_none());
        // Zero surviving ships.
        let v = view_with_remaining(&[]);
        assert!(enumerate_census(&v, 250_000).is_none());
    }

    #[test]
    fn test_solver_perfect_after_first_hit() {
        // Submarine-perfect play: the moment a submarine is touched, the
        // census collapses (≤ 12 configs) and the expectimax completes —
        // the solver then finishes the ship with provably optimal play.
        let mut v = view_with_remaining(&[2]);
        v.observe(5, 5, ShotResult::Hit);
        let mut solver = EndgameSolver::new(EndgameConfig::default());
        let mv = solver.best_move(&v).unwrap();
        assert!(
            mv.exact,
            "hit-pinned census must solve exactly ({} configs)",
            mv.configs
        );
        assert!(mv.expected_shots.is_finite());
        assert!(mv.expected_shots >= 1.0);
        // The optimal continuation fires along the submarine's line.
        let next = mv.row * 10 + mv.col;
        assert!(
            (mv.row == 5 && (mv.col == 4 || mv.col == 6))
                || (mv.col == 5 && (mv.row == 4 || mv.row == 6))
        );
        let _ = next;
    }

    #[test]
    fn test_solver_defers_on_fresh_hunt() {
        // A fresh hunt (no hits, 180-config census) is NOT the solver's
        // regime — it must return None so the hybrid's PDF + parity
        // search keeps control.
        let v = view_with_remaining(&[2]);
        let mut solver = EndgameSolver::new(EndgameConfig::default());
        assert!(solver.best_move(&v).is_none());
    }

    #[test]
    fn test_solver_never_fires_shot_cell() {
        let mut v = view_with_remaining(&[3]);
        // A scattering of misses, then a touch — the solver regime.
        for r in 0..10 {
            v.observe(r, r, ShotResult::Miss);
        }
        v.observe(5, 4, ShotResult::Hit);
        let mut solver = EndgameSolver::new(EndgameConfig::default());
        let mv = solver.best_move(&v).unwrap();
        assert!(!v.shots.test(mv.row, mv.col));
    }

    #[test]
    fn test_solver_finishes_touched_submarine() {
        // Play out single-ship endgames: the hybrid hunts, and the moment
        // it lands a hit the solver takes over and finishes optimally.
        // Submarine-perfect bound: a touched 2-ship must sink within a few
        // shots of the touch.
        let mut rng = crate::rng::Xoshiro256::from_seed(2024);
        for trial in 0..10 {
            let mut board = crate::board::Board::new();
            let ship = loop {
                let r = rng.gen_range(10) as usize;
                let c = rng.gen_range(10) as usize;
                let h = rng.gen_range(2) == 0;
                if let Some(s) = crate::board::Ship::new(r, c, 2, h)
                    && board.place_ship(s)
                {
                    break s;
                }
            };

            // The hunt phase: fire the touching shot at the board first —
            // the view and the board must stay consistent.
            let cells = ship.cells();
            let (hr, hc) = cells[0];
            let mut view = view_with_remaining(&[2]);
            let touch = board.shoot(hr, hc);
            view.observe(hr, hc, touch);

            // The solver finishes from here.
            let mut solver = EndgameSolver::new(EndgameConfig::default());
            let mut shots = 1; // the touching shot
            while !view.remaining.is_empty() && shots < 100 {
                let mv = solver.best_move(&view).expect("hit-pinned regime holds");
                let res = board.shoot(mv.row, mv.col);
                view.observe(mv.row, mv.col, res);
                shots += 1;
            }
            assert!(
                view.remaining.is_empty(),
                "solver must sink the touched ship (trial {})",
                trial
            );
            // Perfect finish bound: touching an END cell of a length-2
            // ship leaves 4 mutually-exclusive candidate cells; each probe
            // tests exactly one, so the worst case is 3 misses + the
            // sinking hit — 5 shots total including the touch.
            assert!(
                shots <= 5,
                "trial {}: took {} shots to finish a touched submarine",
                trial,
                shots
            );
        }
    }

    #[test]
    fn test_solver_deterministic() {
        let mut v = view_with_remaining(&[4]);
        v.observe(3, 3, ShotResult::Miss);
        v.observe(7, 7, ShotResult::Miss);
        v.observe(5, 2, ShotResult::Hit);
        let mut s1 = EndgameSolver::new(EndgameConfig::default());
        let mut s2 = EndgameSolver::new(EndgameConfig::default());
        let a = s1.best_move(&v).unwrap();
        let b = s2.best_move(&v).unwrap();
        assert_eq!((a.row, a.col), (b.row, b.col));
        assert_eq!(a.exact, b.exact);
    }

    #[test]
    fn test_budget_exceeded_defers() {
        // A node budget of 0 makes the exact search impossible: the solver
        // must DEFER (return None) rather than play a greedy move.
        let mut solver = EndgameSolver::new(EndgameConfig {
            node_budget: 0,
            ..Default::default()
        });
        let mut v = view_with_remaining(&[2]);
        v.observe(5, 5, ShotResult::Hit);
        assert!(
            solver.best_move(&v).is_none(),
            "budget exhaustion must defer to the hybrid strategy"
        );
    }
}
