//! Bayesian hypothesis filter — **time-limited, not count-limited**.
//!
//! Sonar maintains a set of *plausible fleet configurations* that are
//! consistent with the current observations. After every shot we discard
//! configurations that contradict the new information. The decision is the
//! argmax over the surviving hypotheses.
//!
//! There is no fixed cap on the number of hypotheses. The generator runs
//! cooperatively and yields as soon as the [`Deadline`] expires. The more
//! time you give it, the more hypotheses it collects, and the sharper the
//! probability distribution becomes.

use crate::board::ShotResult;
use crate::endgame::{EndgameConfig, EndgameSolver};
use crate::placement::{FleetConfig, random_fleet};
use crate::rng::Xoshiro256;
use crate::targeting::{EnemyView, PdfConfig, PdfTargeting, StrategyStats, TargetingStrategy};
use crate::time_limit::Deadline;

// ─────────────────────────────────────────────────────────────────────────────
// HypothesisFilter
// ─────────────────────────────────────────────────────────────────────────────

/// Bayesian filter over plausible fleet configurations.
///
/// Hypotheses are full fleet layouts (5 ships: 5, 4, 3, 3, 2) that are
/// consistent with everything observed so far. There is no upper bound on
/// the number of hypotheses — the generator runs until the deadline
/// expires or the soft target is reached.
pub struct HypothesisFilter {
    /// Surviving hypotheses. Grows monotonically during a single
    /// `regenerate` call, then is replaced wholesale.
    pub hypotheses: Vec<FleetConfig>,
    /// Soft target — once we have this many, we stop early (still
    /// respecting the deadline). Set to `usize::MAX` for pure time-limited.
    pub soft_target: usize,
    /// PDF fallback used when no hypothesis is consistent.
    pub pdf: PdfTargeting,
}

impl HypothesisFilter {
    /// Create a new filter. `soft_target` is a soft upper bound; once we
    /// have that many hypotheses we stop early. Set to `usize::MAX` for
    /// pure time-limited behaviour.
    pub fn new(soft_target: usize) -> Self {
        Self {
            hypotheses: Vec::new(),
            soft_target,
            pdf: PdfTargeting::new(PdfConfig::default()),
        }
    }

    /// Regenerate the hypothesis set under the given deadline.
    ///
    /// The generator runs until either:
    ///   - the deadline expires, or
    ///   - we have collected `soft_target` hypotheses, or
    ///   - we have made `soft_target * 20` random attempts.
    pub fn regenerate(&mut self, view: &EnemyView, rng: &mut Xoshiro256, deadline: Deadline) {
        let known_no_ship = view.miss_mask().0 | view.sunk.0;
        let active_hits = view.active_hits().0;
        let hits = view.hits.0;

        let mut new_hyps = Vec::new();
        let max_attempts: u32 = (self.soft_target.saturating_mul(20)) as u32;
        let mut attempts = 0u32;

        // Pre-sort the surviving ship lengths — used by the subset check.
        let mut remaining = view.remaining.clone();
        remaining.sort_unstable();

        while new_hyps.len() < self.soft_target && attempts < max_attempts {
            attempts += 1;
            // Cooperative deadline check — every `check_interval` iterations.
            if deadline.check_expired(attempts) {
                break;
            }
            let Some(cfg) = random_fleet(rng) else {
                continue;
            };

            if is_consistent(&cfg, known_no_ship, active_hits, hits, &remaining) {
                new_hyps.push(cfg);
            }
        }

        self.hypotheses = new_hyps;
    }

    /// Append more hypotheses to the existing set (for the "keep thinking"
    /// loop). Runs until the deadline expires or `batch_size` new
    /// hypotheses are found.
    pub fn regenerate_more(&mut self, view: &EnemyView, rng: &mut Xoshiro256, deadline: Deadline) {
        let known_no_ship = view.miss_mask().0 | view.sunk.0;
        let active_hits = view.active_hits().0;
        let hits = view.hits.0;

        let batch_size = 64usize; // hypotheses per batch
        let max_attempts: u32 = (batch_size.saturating_mul(20)) as u32;
        let mut attempts = 0u32;
        let mut found = 0usize;

        let mut remaining = view.remaining.clone();
        remaining.sort_unstable();

        while found < batch_size && attempts < max_attempts {
            attempts += 1;
            if deadline.check_expired(attempts) {
                break;
            }
            let Some(cfg) = random_fleet(rng) else {
                continue;
            };
            if is_consistent(&cfg, known_no_ship, active_hits, hits, &remaining) {
                self.hypotheses.push(cfg);
                found += 1;
            }
        }
    }

    /// Probability that a ship occupies cell `(r, c)` — the fraction of
    /// hypotheses that confirm it.
    #[inline]
    pub fn ship_probability(&self, r: usize, c: usize) -> f32 {
        if self.hypotheses.is_empty() {
            return 0.0;
        }
        let bit = 1u128 << (r * 10 + c);
        let n = self.hypotheses.len() as f32;
        let count = self
            .hypotheses
            .iter()
            .filter(|h| (h.mask & bit) != 0)
            .count();
        count as f32 / n
    }

    /// Full 100-cell probability matrix.
    #[inline]
    pub fn probability_matrix(&self) -> [f32; 100] {
        let mut m = [0.0f32; 100];
        if self.hypotheses.is_empty() {
            return m;
        }
        let n = self.hypotheses.len() as f32;
        for h in &self.hypotheses {
            let mut mask = h.mask;
            while mask != 0 {
                let idx = mask.trailing_zeros() as usize;
                m[idx] += 1.0 / n;
                mask &= mask - 1;
            }
        }
        m
    }

    /// Pick a move using the hypothesis set. Falls back to PDF if no
    /// hypothesis is consistent with the observations.
    pub fn choose_move(
        &mut self,
        view: &EnemyView,
        rng: &mut Xoshiro256,
        deadline: Deadline,
    ) -> (usize, usize) {
        if self.hypotheses.is_empty() {
            return self.pdf.choose(view, rng, deadline);
        }
        let prob = self.probability_matrix();
        let shots = view.shots.0;

        let mut best_val = f32::MIN;
        let mut best_cells: [usize; 16] = [0; 16];
        let mut best_count = 0usize;

        for (i, &v) in prob.iter().enumerate() {
            // Skip cells already fired at.
            if (shots & (1u128 << i)) != 0 {
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
            return self.pdf.choose(view, rng, deadline);
        }
        let pick = if best_count == 1 {
            0
        } else {
            rng.gen_range(best_count as u64) as usize
        };
        let i = best_cells[pick];
        (i / 10, i % 10)
    }
}

/// `true` iff the fleet configuration is consistent with the observations:
/// no ship cell on a known-miss or sunk cell, every hit covered by a ship,
/// and the fleet contains at least the surviving ship lengths.
#[inline]
fn is_consistent(
    cfg: &FleetConfig,
    known_no_ship: u128,
    active_hits: u128,
    hits: u128,
    remaining: &[u8],
) -> bool {
    if (cfg.mask & known_no_ship) != 0 {
        return false;
    }
    if (active_hits & !cfg.mask) != 0 {
        return false;
    }
    if (hits & !cfg.mask) != 0 {
        return false;
    }
    is_length_superset(remaining, &cfg.ships)
}

/// `true` iff `ships` contains at least the lengths in `remaining`
/// (with multiplicity). Uses a tiny count array — no allocations.
#[inline]
fn is_length_superset(remaining: &[u8], ships: &[crate::board::Ship]) -> bool {
    let mut counts = [0i8; 8];
    for s in ships {
        let l = s.len as usize;
        if l < 8 {
            counts[l] += 1;
        }
    }
    for &l in remaining {
        let l = l as usize;
        if l >= 8 {
            return false;
        }
        counts[l] -= 1;
        if counts[l] < 0 {
            return false;
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// HybridTargeting — the default strategy used by the Sonar engine.
// ─────────────────────────────────────────────────────────────────────────────

/// Hybrid targeting strategy: PDF density + Bayesian hypothesis filter.
///
/// This is the strategy Sonar uses by default. You can plug in any
/// [`TargetingStrategy`] implementation via `Engine::with_strategy`.
pub struct HybridTargeting {
    /// PDF fallback used when no hypothesis is consistent.
    pub pdf: PdfTargeting,
    /// Bayesian hypothesis filter.
    pub hypotheses: HypothesisFilter,
    /// Regenerate hypotheses every `regen_every` moves (default 1).
    pub regen_every: u32,
    /// Moves since the last regeneration.
    pub moves_since_regen: u32,
    /// Master switch for the hypothesis path.
    pub use_hypotheses: bool,
    /// Exact-CENSUS endgame solver (0.3): when 1–2 enemy ships remain,
    /// Sonar switches from sampled hypotheses to exhaustive enumeration
    /// and optimal expectimax play.
    pub endgame: EndgameSolver,
    /// The last endgame move solved (if any) — exposed via `stats()`.
    pub last_endgame: Option<crate::endgame::EndgameMove>,
    /// Bayesian blend constant K: the move score mixes the hypothesis
    /// posterior with the (exact-model) PDF density using weight
    /// `n / (n + K)` where `n` is the current hypothesis count.
    /// Small samples lean on the PDF; large samples lean on the posterior.
    /// Empirically calibrated (see `tests/strength.rs`
    /// `test_sonar_outperforms_pdf_component`).
    pub blend_k: f32,
}

impl HybridTargeting {
    /// Create a new hybrid strategy with a soft target of 4096 hypotheses
    /// and the default PDF config.
    pub fn new() -> Self {
        Self {
            pdf: PdfTargeting::new(PdfConfig::default()),
            hypotheses: HypothesisFilter::new(4096),
            regen_every: 1,
            moves_since_regen: 0,
            use_hypotheses: true,
            endgame: EndgameSolver::new(EndgameConfig::default()),
            last_endgame: None,
            blend_k: 64.0,
        }
    }

    /// Configure the soft target (max hypotheses before stopping early).
    pub fn with_soft_target(mut self, n: usize) -> Self {
        self.hypotheses.soft_target = n;
        self
    }

    /// Enable / disable the exact endgame solver.
    pub fn with_endgame(mut self, enabled: bool) -> Self {
        self.endgame.config.enabled = enabled;
        self
    }

    /// Disable the hypothesis path entirely (PDF only).
    pub fn pdf_only(mut self) -> Self {
        self.use_hypotheses = false;
        self
    }

    /// Move score for `(r, c)` under the blended posterior:
    /// `w · P(ship at cell | hypotheses) + (1 − w) · normalised PDF density`
    /// with `w = n / (n + K)`.
    fn blended_score(&self, view: &EnemyView, r: usize, c: usize) -> f32 {
        let n = self.hypotheses.hypotheses.len() as f32;
        let w = n / (n + self.blend_k);
        let density = self.pdf.compute_density(view);
        let dmax = density.iter().copied().fold(0.0f32, f32::max).max(1e-9);
        let idx = r * 10 + c;
        let post = self.hypotheses.ship_probability(r, c);
        let dens = density[idx] / dmax;
        w * post + (1.0 - w) * dens
    }

    /// Argmax (with random tie-break) over the blended score, skipping
    /// already-fired cells.
    fn choose_blended(&mut self, view: &EnemyView, rng: &mut Xoshiro256) -> (usize, usize) {
        let n = self.hypotheses.hypotheses.len() as f32;
        let w = n / (n + self.blend_k);
        let density = self.pdf.compute_density(view);
        let dmax = density.iter().copied().fold(0.0f32, f32::max).max(1e-9);
        let prob = self.hypotheses.probability_matrix();
        let shots = view.shots.0;

        let mut best_val = f32::MIN;
        let mut best_cells: [usize; 16] = [0; 16];
        let mut best_count = 0usize;
        for i in 0..100 {
            if (shots & (1u128 << i)) != 0 {
                continue;
            }
            let v = w * prob[i] + (1.0 - w) * (density[i] / dmax);
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
            // Every cell exhausted — any coordinate is moot.
            return (0, 0);
        }
        let pick = if best_count == 1 {
            0
        } else {
            rng.gen_range(best_count as u64) as usize
        };
        let i = best_cells[pick];
        (i / 10, i % 10)
    }
}

impl Default for HybridTargeting {
    fn default() -> Self {
        Self::new()
    }
}

impl TargetingStrategy for HybridTargeting {
    fn choose(
        &mut self,
        view: &EnemyView,
        rng: &mut Xoshiro256,
        deadline: Deadline,
    ) -> (usize, usize) {
        // ── Phase 0: exact-CENSUS endgame (0.3). ─────────────────────────
        // When 1–2 enemy ships remain, enumerate every legal configuration
        // exactly and play the expected-shots-optimal move. This overrides
        // the sampled hypothesis path entirely — the census is strictly
        // sharper, and the solver is deterministic (no RNG, no clock).
        if self.endgame.config.enabled && !view.remaining.is_empty() {
            if view.remaining.len() <= self.endgame.config.max_ships
                && let Some(mv) = self.endgame.best_move(view)
            {
                self.last_endgame = Some(mv);
                return (mv.row, mv.col);
            }
            self.last_endgame = None;
        }

        // ── Phase 1: generate an initial batch of hypotheses. ──────────
        // The `+ 1 >=` comparison makes the *first* move of a game use the
        // hypothesis filter too (0.1.0 quirk fixed in 0.2.0: the first move
        // used to fall back to PDF because `0 >= 1` is false).
        if self.use_hypotheses && self.moves_since_regen + 1 >= self.regen_every {
            self.hypotheses.regenerate(view, rng, deadline);
            self.moves_since_regen = 0;
        }
        self.moves_since_regen += 1;

        // Pick the best move. With hypotheses available we do NOT discard
        // the PDF — we *blend* them: the hypothesis posterior is the
        // theoretically right signal but noisy at small counts, while the
        // PDF density is an exact computation over an approximate
        // (per-ship independent) model. The adaptive weight `n/(n+K)`
        // uses each source where it is strongest. (Pure posterior was
        // measured at ~53% vs PDF-only at soft target 128 — the blend is
        // what makes the hybrid actually stronger than its parts.)
        let mut best = if self.use_hypotheses && !self.hypotheses.hypotheses.is_empty() {
            self.choose_blended(view, rng)
        } else {
            self.pdf.choose(view, rng, deadline)
        };

        // ── Phase 2: with a real deadline, keep thinking until it ──────
        // ── expires. More hypotheses = sharper distribution. ───────────
        // When the deadline is `None` (tests, benchmarks, WASM) we return
        // immediately — the soft target already bounds the work.
        if deadline.limit.is_none() {
            return best;
        }

        // Loop: keep accumulating hypotheses and re-evaluating the best
        // move until the deadline expires. `regenerate_more` respects the
        // deadline internally.
        let mut idle_batches = 0u32;
        loop {
            if deadline.expired() {
                break;
            }

            // Generate more hypotheses with the remaining time.
            let before = self.hypotheses.hypotheses.len();
            self.hypotheses.regenerate_more(view, rng, deadline);
            let after = self.hypotheses.hypotheses.len();

            // If no new hypotheses appeared in this batch, count it as
            // idle. After 3 consecutive idle batches, sleep briefly to
            // avoid busy-spinning.
            if after == before {
                idle_batches += 1;
                if idle_batches >= 3 {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            } else {
                idle_batches = 0;
            }

            // Re-evaluate the best move with the enriched set.
            if !self.hypotheses.hypotheses.is_empty() {
                let candidate = self.choose_blended(view, rng);
                // Only update if the candidate is strictly better (avoids
                // flapping due to tie-break randomness).
                let prob = self.blended_score(view, candidate.0, candidate.1);
                let best_prob = self.blended_score(view, best.0, best.1);
                if prob > best_prob {
                    best = candidate;
                }
            }

            // Safety valve: bound memory growth.
            if self.hypotheses.hypotheses.len() >= 500_000 {
                break;
            }
        }

        best
    }

    fn observe(&mut self, _r: usize, _c: usize, _result: ShotResult) {
        // Hypotheses are regenerated wholesale on the next `choose`.
    }

    fn reset(&mut self) {
        self.pdf.reset();
        self.hypotheses.hypotheses.clear();
        self.moves_since_regen = 0;
        self.endgame.clear_memo();
        self.last_endgame = None;
    }

    fn stats(&self) -> StrategyStats {
        StrategyStats {
            hypothesis_count: self.hypotheses.hypotheses.len(),
            probability_matrix: Some(self.hypotheses.probability_matrix()),
            density_matrix: Some(self.pdf.last_density),
            endgame: self.last_endgame,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_length_superset() {
        use crate::board::Ship;
        let ships = [
            Ship::new(0, 0, 5, true).unwrap(),
            Ship::new(2, 0, 4, true).unwrap(),
            Ship::new(4, 0, 3, true).unwrap(),
            Ship::new(6, 0, 3, true).unwrap(),
            Ship::new(8, 0, 2, true).unwrap(),
        ];
        assert!(is_length_superset(&[2, 3, 3, 4, 5], &ships));
        assert!(is_length_superset(&[2, 3], &ships));
        assert!(!is_length_superset(&[2, 2], &ships)); // only one ship of length 2
        assert!(!is_length_superset(&[6], &ships)); // no ship of length 6
    }

    #[test]
    fn test_filter_consistent_after_miss() {
        let mut rng = Xoshiro256::from_seed(42);
        let mut v = EnemyView::new();
        v.observe(5, 5, ShotResult::Miss);

        let mut hf = HypothesisFilter::new(64);
        hf.regenerate(&v, &mut rng, Deadline::none());

        for h in &hf.hypotheses {
            assert_eq!(h.mask & (1u128 << 55), 0, "ship on a miss cell");
        }
    }

    #[test]
    fn test_filter_consistent_after_hit() {
        let mut rng = Xoshiro256::from_seed(42);
        let mut v = EnemyView::new();
        v.observe(5, 5, ShotResult::Hit);

        let mut hf = HypothesisFilter::new(64);
        hf.regenerate(&v, &mut rng, Deadline::none());

        for h in &hf.hypotheses {
            assert_ne!(h.mask & (1u128 << 55), 0, "no ship on a hit cell");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_filter_respects_deadline() {
        let mut rng = Xoshiro256::from_seed(42);
        let v = EnemyView::new();

        let mut hf = HypothesisFilter::new(usize::MAX);
        // 50 ms deadline — should collect far fewer than usize::MAX hypotheses.
        hf.regenerate(
            &v,
            &mut rng,
            Deadline::from_duration(std::time::Duration::from_millis(50)),
        );
        assert!(
            !hf.hypotheses.is_empty(),
            "should have collected some hypotheses"
        );
        // The key assertion is that we didn't run forever.
        assert!(
            hf.hypotheses.len() < 1_000_000,
            "deadline should have stopped it"
        );
    }

    #[test]
    fn test_hybrid_never_reshoots() {
        let mut rng = Xoshiro256::from_seed(7);
        let mut v = EnemyView::new();
        let mut ht = HybridTargeting::new().with_soft_target(64);

        for _ in 0..30 {
            let (r, c) = ht.choose(&v, &mut rng, Deadline::none());
            assert!(!v.shots.test(r, c), "reshoot at ({},{})", r, c);
            let res = if (r + c) % 3 == 0 {
                ShotResult::Hit
            } else {
                ShotResult::Miss
            };
            v.observe(r, c, res);
        }
    }

    #[test]
    fn test_filter_after_sunk() {
        let mut rng = Xoshiro256::from_seed(7);
        let mut v = EnemyView::new();
        v.observe(0, 0, ShotResult::Hit);
        v.observe(0, 1, ShotResult::Sunk(2));

        let mut hf = HypothesisFilter::new(64);
        hf.regenerate(&v, &mut rng, Deadline::none());

        let mask01 = (1u128 << 0) | (1u128 << 1);
        for h in &hf.hypotheses {
            // The sunk cells must be a ship.
            assert_eq!(h.mask & mask01, mask01, "sunk cells must be a ship");
            // The fleet must still contain all 5 original ship lengths.
            let mut lens: Vec<u8> = h.ships.iter().map(|s| s.len).collect();
            lens.sort_unstable();
            assert_eq!(lens, vec![2, 3, 3, 4, 5]);
        }
    }

    #[test]
    fn test_stats_reports_hypothesis_count() {
        let mut rng = Xoshiro256::from_seed(11);
        let v = EnemyView::new();
        let mut ht = HybridTargeting::new().with_soft_target(64);
        let _ = ht.choose(&v, &mut rng, Deadline::none());
        let s = ht.stats();
        assert!(s.hypothesis_count > 0, "hybrid should have hypotheses");
        let m = s.probability_matrix.unwrap_or([0.0; 100]);
        // Every hypothesis covers 17 ship cells and the matrix is
        // normalised per hypothesis, so the total probability mass is ~17.
        let sum: f32 = m.iter().sum();
        assert!(
            (sum - 17.0).abs() < 0.5,
            "probability mass should be ~17 cells, got {}",
            sum
        );
    }
}
