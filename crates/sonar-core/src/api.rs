//! The Sonar `Engine` — a single struct that exposes 100% of the engine's
//! capabilities.
//!
//! The `Engine` is the stable, ergonomic public API. Internally it composes
//! a [`Board`] (our fleet), an [`EnemyView`] (what we know about the
//! opponent), a [`TargetingStrategy`] (default: [`HybridTargeting`]), a PRNG
//! and a configurable move deadline.
//!
//! ## Design principles
//!
//! - **One struct, full control.** Everything Sonar can do is a method on
//!   `Engine`. No hidden globals, no implicit state.
//! - **Time-limited, not count-limited.** The only knob for search depth
//!   is a `Deadline`. Set it from 1 s to 60 s.
//! - **Pluggable strategy.** The default [`HybridTargeting`] can be swapped
//!   for any implementation of [`TargetingStrategy`] via
//!   [`Engine::with_strategy`].
//! - **Observable.** [`Engine::snapshot`] returns a serialisable view of
//!   the internal state — density matrix, hypothesis count, probability
//!   matrix — for debugging, UIs, and mods.
//! - **Pure-function targeting.** The engine carries no learned bias and
//!   no cross-game state that influences decisions: targeting is a pure
//!   function of the public observation sequence (adversarial-robustness
//!   contract, see `targeting` module docs). Game records are *statistics
//!   only* — they never influence play.

use crate::board::{Board, ShotResult};
use crate::learning::{self, GameRecord, LearningDB};
use crate::placement::{place_best_fleet, place_random_fleet, PlacementConfig};
use crate::rng::Xoshiro256;
use crate::rules::GameRules;
use crate::targeting::{EnemyView, HybridTargeting, TargetingStrategy};
use crate::time_limit::Deadline;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─────────────────────────────────────────────────────────────────────────────
// EngineConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Knobs that control an [`Engine`]'s behaviour.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Soft cap on the number of hypotheses (the generator stops early
    /// once it has this many). Set to `usize::MAX` for pure time-limited.
    pub hypothesis_soft_target: usize,
    /// Use intelligent (penalty-minimising) fleet placement.
    pub smart_placement: bool,
    /// Penalty configuration for fleet placement.
    pub placement: PlacementConfig,
    /// Default move deadline (used by [`Engine::choose_move`] when the
    /// caller passes `Deadline::none()`).
    pub default_deadline_secs: u32,
    /// Enable passive game recording (statistics only — recorded games
    /// NEVER influence decisions; see the module docs). The records go to
    /// `~/.local/share/sonar/learning.json` or `$SONAR_LEARNING_PATH`.
    pub use_learning: bool,
    /// Optional explicit path to the learning JSON file. If `None`,
    /// Sonar uses `~/.local/share/sonar/learning.json` or
    /// `$SONAR_LEARNING_PATH`.
    pub learning_path: Option<PathBuf>,
    /// Game rules — board size, fleet, contact/sink rules.
    /// Allows custom micro-modes beyond standard 10×10 Battleship.
    pub rules: GameRules,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            hypothesis_soft_target: 1024,
            smart_placement: true,
            placement: PlacementConfig::default(),
            default_deadline_secs: 20,
            use_learning: true,
            learning_path: None,
            rules: GameRules::default(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MoveSuggestion
// ─────────────────────────────────────────────────────────────────────────────

/// A move suggestion returned by [`Engine::suggest_move`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MoveSuggestion {
    /// Recommended row `[0, 10)`.
    pub row: usize,
    /// Recommended column `[0, 10)`.
    pub col: usize,
    /// Human-readable coordinate, e.g. `"E5"`.
    pub coordinate: String,
    /// Sonar's confidence in this move `[0.0, 1.0]` — derived from the
    /// probability gap between the top cell and the runner-up.
    pub confidence: f32,
    /// Number of surviving hypotheses used to make this decision.
    pub hypothesis_count: usize,
    /// Time spent deliberating, in microseconds.
    pub elapsed_us: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// EngineSnapshot
// ─────────────────────────────────────────────────────────────────────────────

/// A serialisable snapshot of the engine's internal state. Useful for
/// debugging, UIs, and saving/resuming games.
///
/// `u128` masks are serialised as strings because `serde_json` does not
/// support `u128` natively.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineSnapshot {
    /// Our fleet mask (100 bits packed as `u128`), serialised as a decimal string.
    pub our_fleet_mask: String,
    /// Our shots mask, serialised as a decimal string.
    pub our_shots_mask: String,
    /// Our hits mask, serialised as a decimal string.
    pub our_hits_mask: String,
    /// Our sunk mask, serialised as a decimal string.
    pub our_sunk_mask: String,
    /// Remaining enemy ship lengths.
    pub enemy_remaining: Vec<u8>,
    /// Surviving hypothesis count.
    pub hypothesis_count: usize,
    /// Latest 100-cell probability matrix (from hypotheses), if available.
    pub probability_matrix: Option<Vec<f32>>,
    /// Latest 100-cell PDF density matrix.
    pub density_matrix: Vec<f32>,
    /// Number of moves we have fired so far.
    pub moves_fired: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Engine
// ─────────────────────────────────────────────────────────────────────────────

/// The Sonar engine. Owns all state for a single player.
///
/// Create one with [`Engine::new`], place your fleet (or let Sonar do it
/// with [`Engine::place_fleet_random`] / [`Engine::place_fleet_smart`]),
/// then alternate between [`Engine::choose_move`] and
/// [`Engine::observe_result`].
pub struct Engine {
    config: EngineConfig,
    our_board: Board,
    enemy_view: EnemyView,
    strategy: Box<dyn TargetingStrategy>,
    rng: Xoshiro256,
    /// History of our shots with hit/miss — used when recording a game.
    my_shots: Vec<(u8, bool)>,
    moves_fired: u32,
    learning: Option<LearningDB>,
}

impl Engine {
    /// Construct a new engine with the given configuration.
    pub fn new(config: EngineConfig) -> Self {
        let learning = if config.use_learning {
            let path = config
                .learning_path
                .clone()
                .or_else(learning::default_path_opt);
            Some(match path {
                Some(p) => LearningDB::load(&p),
                None => LearningDB::new(),
            })
        } else {
            None
        };

        Self {
            strategy: Box::new(
                HybridTargeting::new().with_soft_target(config.hypothesis_soft_target),
            ),
            our_board: Board::new(),
            enemy_view: EnemyView::new(),
            rng: Xoshiro256::from_seed(crate::rng::random_u64()),
            config,
            my_shots: Vec::with_capacity(100),
            moves_fired: 0,
            learning,
        }
    }

    /// Replace the targeting strategy. The previous strategy is dropped.
    pub fn with_strategy(mut self, strategy: Box<dyn TargetingStrategy>) -> Self {
        self.strategy = strategy;
        self
    }

    /// Reference the current configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Mutable reference to the configuration. Caller is responsible for
    /// calling [`Engine::apply_config`] if changes should propagate to
    /// already-constructed subcomponents.
    pub fn config_mut(&mut self) -> &mut EngineConfig {
        &mut self.config
    }

    /// Re-apply the configuration (e.g. after mutating
    /// `EngineConfig::hypothesis_soft_target`).
    pub fn apply_config(&mut self) {
        // We can't mutate the boxed strategy directly without downcasting;
        // the simplest correct behaviour is to rebuild it.
        self.strategy = Box::new(
            HybridTargeting::new().with_soft_target(self.config.hypothesis_soft_target),
        );
    }

    /// Borrow the current game rules.
    pub fn rules(&self) -> &GameRules {
        &self.config.rules
    }

    /// Mutable borrow of the game rules. Call [`Engine::apply_rules`]
    /// afterwards if the engine should immediately reflect the change.
    pub fn rules_mut(&mut self) -> &mut GameRules {
        &mut self.config.rules
    }

    /// Apply rule changes (validates and resets internal state if the
    /// board size or fleet changed).
    pub fn apply_rules(&mut self) -> Result<(), String> {
        self.config.rules.validate()?;
        self.reset();
        Ok(())
    }

    // ─── Fleet placement ───────────────────────────────────────────────────

    /// Place our fleet using Sonar's intelligent penalty-minimising
    /// placement. Returns `true` on success.
    pub fn place_fleet_smart(&mut self) -> bool {
        self.our_board = place_best_fleet(&mut self.rng, &self.config.placement);
        true
    }

    /// Place our fleet uniformly at random (used for benchmarks and
    /// ablations). Returns `true` on success.
    pub fn place_fleet_random(&mut self) -> bool {
        self.our_board = place_random_fleet(&mut self.rng);
        true
    }

    /// Place our fleet manually. `ships` is a list of `(row, col, length,
    /// horizontal)`. Returns `Err` with the index of the offending ship
    /// if any placement is illegal.
    pub fn place_fleet_manual(
        &mut self,
        ships: &[(usize, usize, u8, bool)],
    ) -> Result<(), usize> {
        self.our_board.clear();
        for (i, &(r, c, len, h)) in ships.iter().enumerate() {
            match crate::board::Ship::new(r, c, len, h) {
                Some(s) => {
                    if !self.our_board.place_ship(s) {
                        return Err(i);
                    }
                }
                None => return Err(i),
            }
        }
        Ok(())
    }

    /// Borrow our board (read-only).
    pub fn our_board(&self) -> &Board {
        &self.our_board
    }

    /// Borrow our board (mutable).
    pub fn our_board_mut(&mut self) -> &mut Board {
        &mut self.our_board
    }

    /// Borrow the enemy view (read-only).
    pub fn enemy_view(&self) -> &EnemyView {
        &self.enemy_view
    }

    // ─── Moves ─────────────────────────────────────────────────────────────

    /// Ask Sonar for its next move. If `deadline` has no limit, the
    /// engine's `default_deadline_secs` is used.
    pub fn choose_move(&mut self, deadline: Deadline) -> (usize, usize) {
        let d = if deadline.limit.is_some() {
            deadline
        } else if self.config.default_deadline_secs > 0 {
            Deadline::from_secs(self.config.default_deadline_secs as u64)
        } else {
            Deadline::none()
        };
        self.strategy.choose(&self.enemy_view, &mut self.rng, d)
    }

    /// Like [`Engine::choose_move`] but returns a rich [`MoveSuggestion`]
    /// with confidence, hypothesis count, and timing.
    pub fn suggest_move(&mut self, deadline: Deadline) -> MoveSuggestion {
        let start = crate::clock::now_us();
        let (r, c) = self.choose_move(deadline);
        let elapsed_us = crate::clock::now_us().saturating_sub(start);

        let hypothesis_count = self.hypothesis_count();
        let prob = self.probability_matrix();
        let top = prob[r * 10 + c];
        let runner_up = prob
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != r * 10 + c)
            .map(|(_, &v)| v)
            .fold(0.0f32, f32::max);
        let confidence = if top > 0.0 { (top - runner_up).clamp(0.0, 1.0) } else { 0.0 };

        MoveSuggestion {
            row: r,
            col: c,
            coordinate: crate::helpers::format_coordinate(r, c),
            confidence,
            hypothesis_count,
            elapsed_us,
        }
    }

    /// Feed back the result of our shot at `(r, c)`.
    pub fn observe_result(&mut self, r: usize, c: usize, result: ShotResult) {
        let hit = matches!(result, ShotResult::Hit | ShotResult::Sunk(_));
        self.my_shots.push(((r * 10 + c) as u8, hit));
        self.enemy_view.observe(r, c, result);
        self.strategy.observe(r, c, result);
        self.moves_fired += 1;
    }

    /// Apply an incoming enemy shot to our board. Returns the result
    /// the enemy would observe.
    pub fn receive_shot(&mut self, r: usize, c: usize) -> ShotResult {
        self.our_board.shoot(r, c)
    }

    /// Have we lost all our ships?
    pub fn is_defeated(&self) -> bool {
        self.our_board.all_sunk()
    }

    // ─── Inspection ───────────────────────────────────────────────────────

    /// Number of surviving hypotheses in the Bayesian filter (0 for
    /// strategies that do not use hypotheses).
    ///
    /// Real data — forwarded from the active strategy via
    /// [`TargetingStrategy::stats`].
    pub fn hypothesis_count(&self) -> usize {
        self.strategy.stats().hypothesis_count
    }

    /// Latest 100-cell probability matrix from the hypothesis filter.
    /// Returns zeroes if the strategy does not compute one.
    ///
    /// Real data — forwarded from the active strategy via
    /// [`TargetingStrategy::stats`].
    pub fn probability_matrix(&self) -> [f32; 100] {
        self.strategy
            .stats()
            .probability_matrix
            .unwrap_or([0.0; 100])
    }

    /// Latest 100-cell PDF density matrix. Recomputed from the current
    /// enemy view.
    pub fn density_matrix(&self) -> [f32; 100] {
        let pdf = crate::targeting::PdfTargeting::new(crate::targeting::PdfConfig::default());
        pdf.compute_density(&self.enemy_view)
    }

    /// Build a serialisable snapshot of the engine's internal state.
    pub fn snapshot(&self) -> EngineSnapshot {
        let density = self.density_matrix();
        let stats = self.strategy.stats();
        EngineSnapshot {
            our_fleet_mask: self.our_board.ships.0.to_string(),
            our_shots_mask: self.our_board.shots.0.to_string(),
            our_hits_mask: self.our_board.hits.0.to_string(),
            our_sunk_mask: self.our_board.sunk.0.to_string(),
            enemy_remaining: self.enemy_view.remaining.clone(),
            hypothesis_count: self.hypothesis_count(),
            probability_matrix: stats.probability_matrix.map(|m| m.to_vec()),
            density_matrix: density.to_vec(),
            moves_fired: self.moves_fired,
        }
    }

    // ─── Learning ─────────────────────────────────────────────────────────

    /// Record the current game into the statistics database. Called when
    /// the game ends. Records are **passive statistics** — they are never
    /// used to bias future decisions.
    pub fn record_game(&mut self, won: bool) {
        if !self.config.use_learning {
            return;
        }
        let timestamp = crate::clock::unix_secs();
        let fleet_lens: Vec<u8> = self.our_board.ship_list.iter().map(|s| s.len).collect();
        let rec = GameRecord {
            my_fleet_mask: self.our_board.ships.0.to_string(),
            my_shots: std::mem::take(&mut self.my_shots),
            won,
            moves: self.moves_fired,
            fleet_lengths: fleet_lens,
            timestamp,
        };
        // Record into the global DB (which persists to disk).
        learning::record_game(rec);
    }

    /// Borrow the in-memory learning database (or `None` if learning is
    /// disabled).
    pub fn learning(&self) -> Option<&LearningDB> {
        self.learning.as_ref()
    }

    /// Force-save the learning database to disk.
    pub fn save_learning(&self) -> std::io::Result<()> {
        learning::save_global()
    }

    // ─── Lifecycle ────────────────────────────────────────────────────────

    /// Reset the engine to a fresh state (new game). Keeps the same
    /// configuration, PRNG seed lineage, and learning DB.
    pub fn reset(&mut self) {
        self.our_board.clear();
        self.enemy_view = EnemyView::new();
        self.strategy.reset();
        self.my_shots.clear();
        self.moves_fired = 0;
    }

    /// Replace the PRNG seed (useful for deterministic testing).
    pub fn reseed(&mut self, seed: u64) {
        self.rng = Xoshiro256::from_seed(seed);
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(EngineConfig::default())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_plays_a_game() {
        let mut sonar = Engine::new(EngineConfig {
            use_learning: false,
            hypothesis_soft_target: 8,
            ..Default::default()
        });
        let mut enemy = Engine::new(EngineConfig {
            use_learning: false,
            hypothesis_soft_target: 8,
            ..Default::default()
        });
        sonar.reseed(1234);
        enemy.reseed(5678);
        sonar.place_fleet_smart();
        enemy.place_fleet_smart();

        // Use a short deadline (10 ms) to keep the test fast.
        let dl = Deadline::from_duration(std::time::Duration::from_millis(10));
        for _ in 0..200 {
            let (r, c) = sonar.choose_move(dl);
            let res = enemy.receive_shot(r, c);
            sonar.observe_result(r, c, res);
            if enemy.is_defeated() {
                break;
            }
            let (r, c) = enemy.choose_move(dl);
            let res = sonar.receive_shot(r, c);
            enemy.observe_result(r, c, res);
            if sonar.is_defeated() {
                break;
            }
        }
        assert!(sonar.is_defeated() || enemy.is_defeated());
    }

    #[test]
    fn test_hypothesis_metadata_is_real() {
        // Regression test for the 0.1.0 stubs where hypothesis_count()
        // and probability_matrix() always returned zeros.
        let mut e = Engine::new(EngineConfig {
            use_learning: false,
            hypothesis_soft_target: 64,
            // Zero = no default time budget: moves are work-limited, which
            // keeps this test fast and exercises the fast path.
            default_deadline_secs: 0,
            ..Default::default()
        });
        e.reseed(99);
        e.place_fleet_smart();
        let _ = e.choose_move(Deadline::none());
        assert!(e.hypothesis_count() > 0, "hypothesis_count must report real data");
        let prob = e.probability_matrix();
        assert!(prob.iter().any(|&p| p > 0.0), "probability matrix must be non-zero");
        let snap = e.snapshot();
        assert!(snap.probability_matrix.is_some(), "snapshot must include the probability matrix");
    }

    #[test]
    fn test_manual_placement_ok() {
        let mut e = Engine::new(EngineConfig {
            use_learning: false,
            ..Default::default()
        });
        let ships = vec![
            (0, 0, 5, true),
            (2, 0, 4, true),
            (4, 0, 3, true),
            (6, 0, 3, true),
            (8, 0, 2, true),
        ];
        assert!(e.place_fleet_manual(&ships).is_ok());
        assert!(e.our_board().is_complete());
    }

    #[test]
    fn test_manual_placement_illegal() {
        let mut e = Engine::new(EngineConfig {
            use_learning: false,
            ..Default::default()
        });
        // Two ships touching.
        let ships = vec![
            (0, 0, 3, true),
            (1, 0, 3, true),
        ];
        assert!(e.place_fleet_manual(&ships).is_err());
    }

    #[test]
    fn test_suggest_move_returns_metadata() {
        let mut e = Engine::new(EngineConfig {
            use_learning: false,
            hypothesis_soft_target: 32,
            ..Default::default()
        });
        e.place_fleet_smart();
        let s = e.suggest_move(Deadline::from_duration(std::time::Duration::from_millis(200)));
        assert!(s.row < 10);
        assert!(s.col < 10);
        assert!(!s.coordinate.is_empty());
    }

    #[test]
    fn test_snapshot_serializes() {
        let mut e = Engine::new(EngineConfig {
            use_learning: false,
            ..Default::default()
        });
        e.place_fleet_smart();
        let snap = e.snapshot();
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("our_fleet_mask"));
        assert!(json.contains("density_matrix"));
    }

    #[test]
    fn test_reset_clears_state() {
        let mut e = Engine::new(EngineConfig {
            use_learning: false,
            hypothesis_soft_target: 8,
            ..Default::default()
        });
        e.place_fleet_smart();
        e.choose_move(Deadline::from_duration(std::time::Duration::from_millis(10)));
        e.reset();
        assert_eq!(e.our_board().ship_list.len(), 0);
    }
}
