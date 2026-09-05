//! Targeting algorithms — the heart of the bot's strength.
//!
//! A hybrid of three mechanisms:
//!  1. **PDF (Probability Density Function)** — for every cell we count the
//!     number of ways the surviving ships can legally pass through it,
//!     consistent with the observations (misses/hits/sinks).
//!  2. **Hunt/Target** — after a hit, constraint propagation narrows the
//!     possible orientations.
//!  3. **Fleet hypotheses** — a Bayesian filter that discards inconsistent
//!     full-fleet configurations.
//!
//! The decision is the argmax over the PDF/probability field, with a
//! preference for cells adjacent to active hits.
//!
//! ## Adversarial robustness contract
//!
//! Targeting here is a **pure function of public information**: the exact
//! sequence of misses/hits/sinks on the attacked board. There is
//! deliberately no opponent modelling, no learned bias, and no hidden
//! state that persists between games — an adversary that collects
//! thousands of games cannot extract a targeting "personality" to
//! exploit, because there is none. This property is enforced by tests in
//! `tests/` (determinism, indistinguishability, no-reshoot invariants).

use crate::bitboard::{BitBoard, MASK_100};
use crate::board::Board;
use crate::fleet::FLEET;
use crate::rng::Xoshiro256;
use crate::time_limit::Deadline;

// Re-export from `hypothesis` for convenience.
pub use crate::hypothesis::HybridTargeting;

// Re-export the fleet placements function for convenience.
pub use crate::fleet::placements;

/// Search statistics exposed by a targeting strategy. Used by the engine
/// API, the JSON protocol, and web mods to inspect what the strategy
/// actually computed — 100% observability by design.
#[derive(Clone, Debug, Default)]
pub struct StrategyStats {
    /// Number of surviving fleet hypotheses (0 if the strategy does not
    /// use hypotheses).
    pub hypothesis_count: usize,
    /// Latest 100-cell probability matrix derived from hypotheses
    /// (fraction of hypotheses covering each cell). `None` if unavailable.
    pub probability_matrix: Option<[f32; 100]>,
    /// Latest 100-cell PDF density matrix, if the strategy computes one.
    pub density_matrix: Option<[f32; 100]>,
}

/// The public targeting interface — produces move decisions.
pub trait TargetingStrategy: Send + Sync {
    /// Choose the next move given the current view of the enemy board.
    /// `deadline` bounds the deliberation time.
    fn choose(&mut self, view: &EnemyView, rng: &mut Xoshiro256, deadline: Deadline) -> (usize, usize);

    /// Update internal state after a shot.
    fn observe(&mut self, r: usize, c: usize, result: crate::board::ShotResult);

    /// Reset to a fresh game.
    fn reset(&mut self);

    /// Expose computed statistics (hypothesis count, matrices).
    /// Strategies that compute nothing meaningful return the default.
    fn stats(&self) -> StrategyStats {
        StrategyStats::default()
    }
}

/// The enemy board as seen from the bot's perspective.
#[derive(Clone, Debug, Default)]
pub struct EnemyView {
    /// Cells fired at (misses + hits + sinks).
    pub shots: BitBoard,
    /// Cells hit (including sunk cells).
    pub hits: BitBoard,
    /// Cells belonging to sunk ships.
    pub sunk: BitBoard,
    /// Lengths of the enemy's surviving ships.
    pub remaining: Vec<u8>,
}

impl EnemyView {
    pub fn new() -> Self {
        Self {
            shots: BitBoard::new(),
            hits: BitBoard::new(),
            sunk: BitBoard::new(),
            remaining: FLEET.to_vec(),
        }
    }

    /// Cells that certainly hold no ship (misses).
    #[inline]
    pub fn miss_mask(&self) -> BitBoard {
        BitBoard(self.shots.0 & !self.hits.0)
    }

    /// Cells that certainly hold a ship (hits + sinks).
    #[inline]
    pub fn hit_mask(&self) -> BitBoard {
        self.hits
    }

    /// Cells not yet fired at — the move candidates.
    #[inline]
    pub fn unknown(&self) -> BitBoard {
        BitBoard(MASK_100 & !self.shots.0)
    }

    /// Active hits — cells of a ship that is hit but not yet sunk.
    #[inline]
    pub fn active_hits(&self) -> BitBoard {
        BitBoard(self.hits.0 & !self.sunk.0)
    }

    /// Update the state with the result of a shot.
    ///
    /// NOTE: the bot never sees the enemy's ship mask, so on `Sunk(len)` it
    /// must reconstruct the sunk ship's cells from the active hits. This
    /// works because ships are linear.
    pub fn observe(&mut self, r: usize, c: usize, result: crate::board::ShotResult) {
        use crate::board::ShotResult::*;
        let bit = 1u128 << (r * 10 + c);
        self.shots.0 |= bit;
        match result {
            Miss => {}
            Hit => {
                self.hits.0 |= bit;
            }
            Sunk(len) => {
                self.hits.0 |= bit;
                // Reconstruct the sunk ship's mask: find the contiguous
                // run of `len` cells containing (r, c) among active hits.
                let sunk_mask = reconstruct_sunk_ship(self.hits.0 & !self.sunk.0, r, c, len);
                self.sunk.0 |= sunk_mask;
                // After a sink, the ship's neighbourhood is all misses
                // (standard Battleship rule).
                let dilated = BitBoard(sunk_mask).dilate8().0;
                let extra = dilated & !sunk_mask & MASK_100;
                self.shots.0 |= extra;
                // Remove the length from the surviving list.
                if let Some(idx) = self.remaining.iter().position(|&l| l == len) {
                    self.remaining.swap_remove(idx);
                }
            }
            _ => {}
        }
    }

    /// Construct from an actual board (for tests and analysis tools).
    pub fn from_board(board: &Board) -> Self {
        let mut v = Self::new();
        v.shots = board.shots;
        v.hits = board.hits;
        v.sunk = board.sunk;
        v.remaining = board.remaining_ship_lengths();
        v
    }
}

/// Reconstruct the mask of a sunk ship from active hits.
/// Ships are linear (horizontal or vertical), so we look for a run of
/// `len` cells.
fn reconstruct_sunk_ship(active_hits: u128, r: usize, c: usize, len: u8) -> u128 {
    // Try horizontal: extend left and right from (r, c).
    let mut mask_h = 1u128 << (r * 10 + c);
    let mut cc = c;
    while cc > 0 {
        cc -= 1;
        let bit = 1u128 << (r * 10 + cc);
        if (active_hits & bit) != 0 {
            mask_h |= bit;
        } else {
            break;
        }
    }
    let mut cc = c;
    while cc < 9 {
        cc += 1;
        let bit = 1u128 << (r * 10 + cc);
        if (active_hits & bit) != 0 {
            mask_h |= bit;
        } else {
            break;
        }
    }
    if mask_h.count_ones() == len as u32 {
        return mask_h;
    }

    // Try vertical.
    let mut mask_v = 1u128 << (r * 10 + c);
    let mut rr = r;
    while rr > 0 {
        rr -= 1;
        let bit = 1u128 << (rr * 10 + c);
        if (active_hits & bit) != 0 {
            mask_v |= bit;
        } else {
            break;
        }
    }
    let mut rr = r;
    while rr < 9 {
        rr += 1;
        let bit = 1u128 << (rr * 10 + c);
        if (active_hits & bit) != 0 {
            mask_v |= bit;
        } else {
            break;
        }
    }
    if mask_v.count_ones() == len as u32 {
        return mask_v;
    }

    // Fallback: return the horizontal mask (most likely the whole ship).
    mask_h
}

/// ============================================================
/// PDF Targeting — state of the art single-pass density targeting.
/// ============================================================

/// PDF strategy configuration.
#[derive(Clone, Copy, Debug)]
pub struct PdfConfig {
    /// Weight for cells adjacent to hits (target-mode bonus).
    pub target_bonus: f32,
    /// Prefer even-parity cells in hunt mode.
    pub use_parity: bool,
    /// Once any hit exists, focus fully on target mode.
    pub hard_target: bool,
}

impl Default for PdfConfig {
    fn default() -> Self {
        Self {
            target_bonus: 100.0,
            use_parity: true,
            hard_target: true,
        }
    }
}

/// PDF density targeting.
///
/// NOTE: deliberately carries **no** learned bias — see the module-level
/// adversarial-robustness contract.
pub struct PdfTargeting {
    pub config: PdfConfig,
    /// Density matrix from the last `choose` call (for inspection).
    pub last_density: [f32; 100],
}

impl PdfTargeting {
    pub fn new(config: PdfConfig) -> Self {
        Self {
            config,
            last_density: [0.0; 100],
        }
    }

    /// Compute the probability-density matrix for every cell.
    /// `density[i]` = the number of legal placements of surviving ships
    /// passing through cell `i`, consistent with the observations.
    pub fn compute_density(&self, view: &EnemyView) -> [f32; 100] {
        let mut density = [0.0f32; 100];
        let miss = view.miss_mask().0;
        let sunk = view.sunk.0;
        let active_hits = view.active_hits().0;
        let known_no_ship = miss | sunk; // cells that certainly hold no ship

        let placements = placements();

        // For every surviving ship length...
        for &len in &view.remaining {
            let len_idx = len as usize;
            if len_idx >= placements.len() {
                continue;
            }
            // ...check every legal placement of that ship.
            for &(_, _, _, mask) in &placements[len_idx] {
                // Does the placement overlap a cell that holds no ship?
                if (mask & known_no_ship) != 0 {
                    continue;
                }
                // With active hits on the board, prefer placements that
                // contain them.
                let contains_active = (mask & active_hits) != 0;
                let has_active = active_hits != 0;
                if has_active && !contains_active {
                    continue;
                }
                // Weight — placements containing active hits get a bonus.
                let weight = if contains_active {
                    self.config.target_bonus
                } else {
                    1.0
                };
                // Add the weight to every cell of the placement.
                let mut m = mask;
                while m != 0 {
                    let idx = m.trailing_zeros() as usize;
                    density[idx] += weight;
                    m &= m - 1;
                }
            }
        }

        // Parity preference in hunt mode (no active hits).
        if self.config.use_parity && active_hits == 0 {
            let min_len = view.remaining.iter().copied().min().unwrap_or(2) as usize;
            if min_len >= 2 {
                for r in 0..10 {
                    for c in 0..10 {
                        if (r + c) % 2 == 0 {
                            density[r * 10 + c] *= 1.05;
                        }
                    }
                }
            }
        }

        // Zero out cells already fired at.
        let mut s = view.shots.0;
        while s != 0 {
            let idx = s.trailing_zeros() as usize;
            density[idx] = 0.0;
            s &= s - 1;
        }

        density
    }

    /// Pick the best move (deadline-aware, but PDF is fast — ~1 ms).
    pub fn choose_move(&self, view: &EnemyView, rng: &mut Xoshiro256, _deadline: Deadline) -> (usize, usize) {
        let density = self.compute_density(view);
        let shots_mask = view.shots.0;
        // Find the max with a small random tie-break; already-shot cells
        // are never selected.
        let mut best_val = f32::MIN;
        let mut best_cells: [usize; 16] = [0; 16];
        let mut best_count: usize = 0;
        for (i, &v) in density.iter().enumerate() {
            // Skip cells already fired at.
            if (shots_mask & (1u128 << i)) != 0 {
                continue;
            }
            if v > best_val {
                best_val = v;
                best_count = 0;
                best_cells[best_count] = i;
                best_count += 1;
            } else if (v - best_val).abs() < 1e-6 && best_count < 16 {
                best_cells[best_count] = i;
                best_count += 1;
            }
        }
        if best_count == 0 {
            // Emergency fallback — a random unfired cell (in case every
            // density entry was zero).
            let un = view.unknown();
            let cells: Vec<_> = un.iter_cells().collect();
            if cells.is_empty() {
                return (0, 0);
            }
            let i = rng.gen_range(cells.len() as u64) as usize;
            return cells[i];
        }
        let pick = if best_count == 1 { 0 } else { rng.gen_range(best_count as u64) as usize };
        let i = best_cells[pick];
        (i / 10, i % 10)
    }
}

impl TargetingStrategy for PdfTargeting {
    fn choose(&mut self, view: &EnemyView, rng: &mut Xoshiro256, deadline: Deadline) -> (usize, usize) {
        let mv = self.choose_move(view, rng, deadline);
        self.last_density = self.compute_density(view);
        mv
    }

    fn observe(&mut self, _r: usize, _c: usize, _result: crate::board::ShotResult) {
        // The state lives in the game loop's EnemyView — nothing to do.
    }

    fn reset(&mut self) {
        self.last_density = [0.0; 100];
    }

    fn stats(&self) -> StrategyStats {
        StrategyStats {
            hypothesis_count: 0,
            probability_matrix: None,
            density_matrix: Some(self.last_density),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Board, Ship, ShotResult};

    #[test]
    fn test_density_initial() {
        let v = EnemyView::new();
        let pdf = PdfTargeting::new(PdfConfig::default());
        let d = pdf.compute_density(&v);
        // On an empty board the centre should have the highest density
        // (the most ship placements pass through it).
        let center = d[5 * 10 + 5];
        let corner = d[0];
        assert!(center > corner, "Center ({}) should be > corner ({})", center, corner);
        for &v in d.iter() {
            assert!(v >= 0.0);
        }
    }

    #[test]
    fn test_density_after_miss() {
        let mut v = EnemyView::new();
        v.observe(5, 5, ShotResult::Miss);
        let pdf = PdfTargeting::new(PdfConfig::default());
        let d = pdf.compute_density(&v);
        // Cell (5,5) must have density 0.
        assert_eq!(d[5 * 10 + 5], 0.0);
    }

    #[test]
    fn test_target_mode_after_hit() {
        let mut v = EnemyView::new();
        v.observe(5, 5, ShotResult::Hit);
        let pdf = PdfTargeting::new(PdfConfig::default());
        let d = pdf.compute_density(&v);
        // Cells adjacent to (5,5) should outrank distant cells.
        let adjacent = d[5 * 10 + 6];
        let far = d[0];
        assert!(adjacent > far, "Adjacent ({}) should be > far ({})", adjacent, far);
    }

    #[test]
    fn test_choose_never_shoots_same_cell() {
        let mut v = EnemyView::new();
        let mut pdf = PdfTargeting::new(PdfConfig::default());
        let mut rng = Xoshiro256::from_seed(42);

        for _ in 0..30 {
            let (r, c) = pdf.choose(&v, &mut rng, Deadline::none());
            assert!(!v.shots.test(r, c), "Bot shot at already-shot cell ({},{})", r, c);
            let res = if (r + c) % 3 == 0 { ShotResult::Hit } else { ShotResult::Miss };
            v.observe(r, c, res);
        }
    }

    #[test]
    fn test_density_respects_remaining() {
        let mut v = EnemyView::new();
        v.remaining = vec![2]; // only the length-2 ship survives
        let pdf = PdfTargeting::new(PdfConfig::default());
        let d = pdf.compute_density(&v);
        let sum: f32 = d.iter().sum();
        assert!(sum > 0.0, "Density should be > 0");
        assert!(sum < 5000.0, "Density should be bounded");
    }

    #[test]
    fn test_sunk_removes_from_remaining() {
        let mut v = EnemyView::new();
        assert_eq!(v.remaining.len(), 5);
        v.observe(0, 0, ShotResult::Sunk(2));
        assert_eq!(v.remaining.len(), 4);
        assert!(!v.remaining.contains(&2));
    }

    #[test]
    fn test_stats_exposes_density() {
        let mut pdf = PdfTargeting::new(PdfConfig::default());
        let mut rng = Xoshiro256::from_seed(1);
        let mut v = EnemyView::new();
        let _ = pdf.choose(&v, &mut rng, Deadline::none());
        let s = pdf.stats();
        assert!(s.density_matrix.is_some());
        let d = s.density_matrix.unwrap_or([0.0; 100]);
        assert!(d.iter().any(|&x| x > 0.0));
    }

    #[test]
    fn test_targeting_is_pure_function_of_view() {
        // Adversarial-robustness property: two fresh strategies given the
        // same observation sequence produce the same density field.
        let mut v = EnemyView::new();
        v.observe(4, 4, ShotResult::Miss);
        v.observe(5, 5, ShotResult::Hit);
        v.observe(5, 6, ShotResult::Sunk(2));

        let a = PdfTargeting::new(PdfConfig::default());
        let b = PdfTargeting::new(PdfConfig::default());
        let da = a.compute_density(&v);
        let db = b.compute_density(&v);
        for i in 0..100 {
            assert_eq!(da[i], db[i], "density differs at {}", i);
        }
    }
}
