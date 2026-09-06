//! Reference bots — implementations of well-known published algorithms.
//!
//! These implementations exist for comparative benchmarking. Each one
//! implements a classic algorithm from the literature / open source:
//!
//! 1. `HuntTargetBot` — the classic hunt+target pattern (textbook baseline)
//!    2. `BurnsPdfBot` — PDF density per Ethan Burns (Dartmouth research)
//!    3. `MonteCarloBot` — Monte Carlo sampling (à la mitchelljy/battleships_ai)

use crate::board::{Board, ShotResult};
use crate::placement::{PlacementConfig, place_random_fleet};
use crate::player::Player;
use crate::rng::Xoshiro256;
use crate::targeting::{EnemyView, TargetingStrategy, placements};
use crate::time_limit::Deadline;

// ============================================================
// 1. HuntTargetBot — the classic hunt+target strategy (simplest).
// ============================================================
// Strategy:
// - Hunt mode: fire at every other cell (checkerboard) until a hit.
// - Target mode: after a hit, fire at the 4 neighbours.
// - After a sink, return to hunt mode.
pub struct HuntTargetBot {
    pub view: EnemyView,
    pub rng: Xoshiro256,
    pub last_hits: Vec<(usize, usize)>,
    pub hunt_parity: bool,
}

impl Default for HuntTargetBot {
    fn default() -> Self {
        Self::new()
    }
}

impl HuntTargetBot {
    pub fn new() -> Self {
        Self {
            view: EnemyView::new(),
            rng: Xoshiro256::from_seed(crate::rng::random_u64()),
            last_hits: Vec::new(),
            hunt_parity: true,
        }
    }

    fn choose_hunt(&mut self) -> (usize, usize) {
        // Checkerboard (parity) search.
        let un = self.view.unknown();
        let mut best: Option<(usize, usize)> = None;
        for (r, c) in un.iter_cells() {
            if (!self.hunt_parity || (r + c) % 2 == 0)
                && (best.is_none() || self.rng.next_u64() & 3 == 0)
            {
                best = Some((r, c));
            }
        }
        best.unwrap_or_else(|| {
            // Fallback: anything unfired.
            let cells: Vec<_> = un.iter_cells().collect();
            cells[self.rng.gen_range(cells.len() as u64) as usize]
        })
    }

    fn choose_target(&mut self) -> Option<(usize, usize)> {
        // For every active hit, check its 4 neighbours.
        let active = self.view.active_hits();
        for (r, c) in active.iter_cells() {
            for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let nr = (r as i32 + dr) as usize;
                let nc = (c as i32 + dc) as usize;
                if nr < 10 && nc < 10 && !self.view.shots.test(nr, nc) {
                    return Some((nr, nc));
                }
            }
        }
        None
    }
}

impl TargetingStrategy for HuntTargetBot {
    fn choose(
        &mut self,
        view: &EnemyView,
        _rng: &mut Xoshiro256,
        _deadline: Deadline,
    ) -> (usize, usize) {
        self.view = view.clone();
        if let Some(mv) = self.choose_target() {
            return mv;
        }
        self.choose_hunt()
    }
    fn observe(&mut self, _r: usize, _c: usize, _result: ShotResult) {}
    fn reset(&mut self) {
        self.view = EnemyView::new();
        self.last_hits.clear();
    }
}

/// ============================================================
/// 2. BurnsPdfBot — PDF density (Ethan Burns, Dartmouth).
/// ============================================================
/// Essentially our PdfBot — PDF is a well-known algorithm from the
/// literature. Kept as a separate class for benchmark clarity.
pub struct BurnsPdfBot {
    pub view: EnemyView,
    pub rng: Xoshiro256,
}

impl Default for BurnsPdfBot {
    fn default() -> Self {
        Self::new()
    }
}

impl BurnsPdfBot {
    pub fn new() -> Self {
        Self {
            view: EnemyView::new(),
            rng: Xoshiro256::from_seed(crate::rng::random_u64()),
        }
    }
}

impl TargetingStrategy for BurnsPdfBot {
    fn choose(
        &mut self,
        view: &EnemyView,
        rng: &mut Xoshiro256,
        _deadline: Deadline,
    ) -> (usize, usize) {
        // Classic PDF: density = the number of ship placements through each cell.
        let mut density = [0.0f32; 100];
        let miss = view.miss_mask().0;
        let active = view.active_hits().0;
        let known_no = miss | view.sunk.0;
        let placements = placements();

        for &len in &view.remaining {
            let li = len as usize;
            if li >= placements.len() {
                continue;
            }
            for &(_, _, _, mask) in &placements[li] {
                if (mask & known_no) != 0 {
                    continue;
                }
                let has_active = active != 0;
                let contains = (mask & active) != 0;
                if has_active && !contains {
                    continue;
                }
                let w = if contains { 50.0 } else { 1.0 };
                let mut m = mask;
                while m != 0 {
                    let i = m.trailing_zeros() as usize;
                    density[i] += w;
                    m &= m - 1;
                }
            }
        }

        let mut best_val = f32::MIN;
        let mut best_cells: [usize; 16] = [0; 16];
        let mut best_count = 0usize;
        for (i, &v) in density.iter().enumerate() {
            if (view.shots.0 & (1u128 << i)) != 0 {
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
            let cells: Vec<_> = view.unknown().iter_cells().collect();
            return cells[rng.gen_range(cells.len() as u64) as usize];
        }
        let pick = if best_count == 1 {
            0
        } else {
            rng.gen_range(best_count as u64) as usize
        };
        let i = best_cells[pick];
        (i / 10, i % 10)
    }
    fn observe(&mut self, _r: usize, _c: usize, _result: ShotResult) {}
    fn reset(&mut self) {
        self.view = EnemyView::new();
    }
}

/// ============================================================
/// 3. MonteCarloBot — Monte Carlo simulation (à la mitchelljy/battleships_ai).
/// ============================================================
/// Algorithm: generate N random fleet configurations consistent with the
/// observations; for every cell compute the fraction of configurations
/// with a ship there; fire at the argmax. Essentially our HypothesisFilter
/// with more samples and no PDF fallback.
pub struct MonteCarloBot {
    pub view: EnemyView,
    pub rng: Xoshiro256,
    pub samples: usize,
}

impl MonteCarloBot {
    pub fn new(samples: usize) -> Self {
        Self {
            view: EnemyView::new(),
            rng: Xoshiro256::from_seed(crate::rng::random_u64()),
            samples,
        }
    }
}

impl TargetingStrategy for MonteCarloBot {
    fn choose(
        &mut self,
        view: &EnemyView,
        rng: &mut Xoshiro256,
        deadline: Deadline,
    ) -> (usize, usize) {
        use crate::placement::random_fleet;
        let miss = view.miss_mask().0;
        let hits = view.hits.0;
        let active = view.active_hits().0;
        let known_no = miss;

        let mut counts = [0u32; 100];
        let mut total = 0u32;
        let mut attempts = 0u32;
        while (total as usize) < self.samples && attempts < self.samples as u32 * 10 {
            attempts += 1;
            if deadline.check_expired(attempts) {
                break;
            }
            if let Some(cfg) = random_fleet(rng) {
                if (cfg.mask & known_no) != 0 {
                    continue;
                }
                if (active & !cfg.mask) != 0 {
                    continue;
                }
                if (hits & !cfg.mask) != 0 {
                    continue;
                }
                // Length-subset check.
                let mut fleet_lens: Vec<u8> = cfg.ships.iter().map(|s| s.len).collect();
                fleet_lens.sort();
                let mut remaining = view.remaining.clone();
                remaining.sort();
                let mut counts_l = [0i8; 8];
                for &v in &fleet_lens {
                    if (v as usize) < 8 {
                        counts_l[v as usize] += 1;
                    }
                }
                let mut ok = true;
                for &v in &remaining {
                    if (v as usize) >= 8 {
                        ok = false;
                        break;
                    }
                    counts_l[v as usize] -= 1;
                    if counts_l[v as usize] < 0 {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }
                let mut m = cfg.mask;
                while m != 0 {
                    let i = m.trailing_zeros() as usize;
                    counts[i] += 1;
                    m &= m - 1;
                }
                total += 1;
            }
        }

        // Argmax with a random tie-break over unfired cells.
        let mut best_val = 0u32;
        let mut best_cells: [usize; 16] = [0; 16];
        let mut best_count = 0usize;
        for (i, &v) in counts.iter().enumerate() {
            if (view.shots.0 & (1u128 << i)) != 0 {
                continue;
            }
            if v > best_val {
                best_val = v;
                best_count = 0;
                best_cells[best_count] = i;
                best_count += 1;
            } else if v == best_val && best_count < 16 {
                best_cells[best_count] = i;
                best_count += 1;
            }
        }
        if best_count == 0 || best_val == 0 {
            // Fallback — any unfired cell.
            let cells: Vec<_> = view.unknown().iter_cells().collect();
            if cells.is_empty() {
                return (0, 0);
            }
            return cells[rng.gen_range(cells.len() as u64) as usize];
        }
        let pick = if best_count == 1 {
            0
        } else {
            rng.gen_range(best_count as u64) as usize
        };
        let i = best_cells[pick];
        (i / 10, i % 10)
    }
    fn observe(&mut self, _r: usize, _c: usize, _result: ShotResult) {}
    fn reset(&mut self) {
        self.view = EnemyView::new();
    }
}

/// Wraps a reference bot as a `Player`.
pub struct ReferencePlayer {
    pub name: String,
    pub board: Board,
    pub view: EnemyView,
    pub strategy: Box<dyn TargetingStrategy>,
    pub rng: Xoshiro256,
}

impl ReferencePlayer {
    pub fn new(name: impl Into<String>, strategy: Box<dyn TargetingStrategy>) -> Self {
        let mut rng = Xoshiro256::from_seed(crate::rng::random_u64());
        Self {
            name: name.into(),
            board: place_random_fleet(&mut rng),
            view: EnemyView::new(),
            strategy,
            rng,
        }
    }
}

impl Player for ReferencePlayer {
    fn name(&self) -> &str {
        &self.name
    }
    fn board(&self) -> &Board {
        &self.board
    }
    fn board_mut(&mut self) -> &mut Board {
        &mut self.board
    }
    fn choose_move(&mut self, deadline: Deadline) -> (usize, usize) {
        self.strategy.choose(&self.view, &mut self.rng, deadline)
    }
    fn observe_result(&mut self, r: usize, c: usize, result: ShotResult) {
        self.view.observe(r, c, result);
        self.strategy.observe(r, c, result);
    }
    fn reset(&mut self) {
        self.board = place_random_fleet(&mut self.rng);
        self.view = EnemyView::new();
        self.strategy.reset();
    }

    fn reseed(&mut self, seed: u64) {
        self.rng = Xoshiro256::from_seed(seed);
    }
}

/// Create a reference player of the given kind.
pub fn make_reference(kind: ReferenceKind, name: &str) -> ReferencePlayer {
    let strat: Box<dyn TargetingStrategy> = match kind {
        ReferenceKind::HuntTarget => Box::new(HuntTargetBot::new()),
        ReferenceKind::BurnsPdf => Box::new(BurnsPdfBot::new()),
        ReferenceKind::MonteCarlo(n) => Box::new(MonteCarloBot::new(n)),
    };
    let mut p = ReferencePlayer::new(name, strat);
    // Use smart placement for fairness (same conditions as Sonar).
    let mut rng = Xoshiro256::from_seed(crate::rng::random_u64());
    p.board = crate::placement::place_best_fleet(&mut rng, &PlacementConfig::default());
    p
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReferenceKind {
    HuntTarget,
    BurnsPdf,
    MonteCarlo(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hunt_target_plays_game() {
        let mut ht = HuntTargetBot::new();
        let mut v = EnemyView::new();
        let mut rng = Xoshiro256::from_seed(42);
        for _ in 0..30 {
            let (r, c) = ht.choose(&v, &mut rng, Deadline::none());
            assert!(!v.shots.test(r, c));
            v.observe(
                r,
                c,
                if (r + c) % 3 == 0 {
                    ShotResult::Hit
                } else {
                    ShotResult::Miss
                },
            );
        }
    }

    #[test]
    fn test_burns_pdf_plays_game() {
        let mut b = BurnsPdfBot::new();
        let mut v = EnemyView::new();
        let mut rng = Xoshiro256::from_seed(42);
        for _ in 0..30 {
            let (r, c) = b.choose(&v, &mut rng, Deadline::none());
            assert!(!v.shots.test(r, c));
            v.observe(
                r,
                c,
                if (r + c) % 3 == 0 {
                    ShotResult::Hit
                } else {
                    ShotResult::Miss
                },
            );
        }
    }

    #[test]
    fn test_montecarlo_plays_game() {
        let mut mc = MonteCarloBot::new(64);
        let mut v = EnemyView::new();
        let mut rng = Xoshiro256::from_seed(42);
        for _ in 0..20 {
            let (r, c) = mc.choose(&v, &mut rng, Deadline::none());
            assert!(!v.shots.test(r, c));
            v.observe(
                r,
                c,
                if (r + c) % 3 == 0 {
                    ShotResult::Hit
                } else {
                    ShotResult::Miss
                },
            );
        }
    }
}
