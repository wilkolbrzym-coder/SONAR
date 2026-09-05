//! Player trait and implementations: bot, human (CLI), suggest bot.

use crate::board::{Board, ShotResult};
use crate::clock::unix_secs;
use crate::placement::{place_best_fleet, place_random_fleet, PlacementConfig};
use crate::rng::{thread_rng, Xoshiro256};
use crate::targeting::{EnemyView, HybridTargeting, PdfTargeting, TargetingStrategy};
use crate::time_limit::Deadline;
use crate::learning;

/// A game participant.
pub trait Player: Send {
    /// The player's name.
    fn name(&self) -> &str;

    /// The player's board (with their ships).
    fn board(&self) -> &Board;
    fn board_mut(&mut self) -> &mut Board;

    /// Choose a move against the enemy (with a hard deadline).
    fn choose_move(&mut self, deadline: Deadline) -> (usize, usize);

    /// Observe the result of our own shot.
    fn observe_result(&mut self, r: usize, c: usize, result: ShotResult);

    /// Observe an enemy shot at our board (already resolved — for UI/debug).
    fn observe_incoming(&mut self, _r: usize, _c: usize, _result: ShotResult) {}

    /// Has the player lost (all ships sunk)?
    fn is_defeated(&self) -> bool {
        self.board().all_sunk()
    }

    /// Reset the player for a new game.
    fn reset(&mut self);

    /// Seed the player's RNG — makes full games deterministic
    /// (used by benchmarks and determinism tests). Default: no-op for
    /// players without an RNG.
    fn reseed(&mut self, _seed: u64) {}
}

/// A bot — the hybrid PDF + hypothesis strategy.
///
/// The bot deliberately keeps **no** cross-game state that could influence
/// decisions: `use_learning` only gates *passive statistics recording*
/// (see the `targeting` module's adversarial-robustness contract).
pub struct BotPlayer {
    pub name: String,
    pub board: Board,
    pub view: EnemyView,
    pub strategy: Box<dyn TargetingStrategy>,
    pub rng: Xoshiro256,
    pub placement: PlacementConfig,
    pub use_smart_placement: bool,
    /// Our shot history with results (for game records).
    pub my_shots: Vec<(u8, bool)>,
    /// Enable passive game recording (statistics only).
    pub use_learning: bool,
    /// Default per-move deadline.
    pub move_deadline: Deadline,
}

impl BotPlayer {
    pub fn new(name: impl Into<String>, max_hyp: usize, use_smart_placement: bool) -> Self {
        Self {
            name: name.into(),
            board: Board::new(),
            view: EnemyView::new(),
            strategy: Box::new(HybridTargeting::new().with_soft_target(max_hyp)),
            rng: thread_rng(),
            placement: PlacementConfig::default(),
            use_smart_placement,
            my_shots: Vec::with_capacity(100),
            use_learning: true,
            move_deadline: Deadline::default_20s(),
        }
    }

    /// Seed the bot's RNG (deterministic testing/benchmarking).
    pub fn reseed(&mut self, seed: u64) {
        self.rng = Xoshiro256::from_seed(seed);
    }

    /// Set the per-move deadline.
    pub fn with_deadline(mut self, deadline: Deadline) -> Self {
        self.move_deadline = deadline;
        self
    }

    /// Disable passive game recording (for benchmarks).
    pub fn without_learning(mut self) -> Self {
        self.use_learning = false;
        self
    }

    /// Place the fleet.
    pub fn place_fleet(&mut self) {
        if self.use_smart_placement {
            self.board = place_best_fleet(&mut self.rng, &self.placement);
        } else {
            self.board = place_random_fleet(&mut self.rng);
        }
    }

    /// Record the finished game into the passive statistics DB.
    pub fn record_game(&mut self, won: bool, moves: u32) {
        if !self.use_learning { return; }
        let fleet_lens: Vec<u8> = self.board.ship_list.iter().map(|s| s.len).collect();
        let rec = learning::GameRecord {
            my_fleet_mask: self.board.ships.0.to_string(),
            my_shots: std::mem::take(&mut self.my_shots),
            won,
            moves,
            fleet_lengths: fleet_lens,
            timestamp: unix_secs(),
        };
        learning::record_game(rec);
    }
}

impl Player for BotPlayer {
    fn name(&self) -> &str { &self.name }
    fn board(&self) -> &Board { &self.board }
    fn board_mut(&mut self) -> &mut Board { &mut self.board }

    fn choose_move(&mut self, deadline: Deadline) -> (usize, usize) {
        // If the caller passes a real deadline, use it. Otherwise fall back
        // to the bot's own default. If that is also None (benchmarks),
        // pass None through so the strategy returns immediately.
        let d = if deadline.limit.is_some() {
            deadline
        } else if self.move_deadline.limit.is_some() {
            self.move_deadline
        } else {
            Deadline::none()
        };
        self.strategy.choose(&self.view, &mut self.rng, d)
    }

    fn observe_result(&mut self, r: usize, c: usize, result: ShotResult) {
        let hit = matches!(result, ShotResult::Hit | ShotResult::Sunk(_));
        self.my_shots.push(((r * 10 + c) as u8, hit));
        self.view.observe(r, c, result);
        self.strategy.observe(r, c, result);
    }

    fn reset(&mut self) {
        self.board.clear();
        self.view = EnemyView::new();
        self.strategy.reset();
        self.my_shots.clear();
    }

    fn reseed(&mut self, seed: u64) {
        self.rng = Xoshiro256::from_seed(seed);
    }
}

/// A weaker bot — PDF only (for comparative benchmarks).
pub struct PdfBot {
    pub name: String,
    pub board: Board,
    pub view: EnemyView,
    pub pdf: PdfTargeting,
    pub rng: Xoshiro256,
}

impl PdfBot {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            board: Board::new(),
            view: EnemyView::new(),
            pdf: PdfTargeting::new(crate::targeting::PdfConfig::default()),
            rng: thread_rng(),
        }
    }
}

impl Player for PdfBot {
    fn name(&self) -> &str { &self.name }
    fn board(&self) -> &Board { &self.board }
    fn board_mut(&mut self) -> &mut Board { &mut self.board }

    fn choose_move(&mut self, deadline: Deadline) -> (usize, usize) {
        self.pdf.choose(&self.view, &mut self.rng, deadline)
    }

    fn observe_result(&mut self, r: usize, c: usize, result: ShotResult) {
        self.view.observe(r, c, result);
        self.pdf.observe(r, c, result);
    }

    fn reset(&mut self) {
        self.board.clear();
        self.view = EnemyView::new();
        self.pdf.reset();
    }

    fn reseed(&mut self, seed: u64) {
        self.rng = Xoshiro256::from_seed(seed);
    }
}

/// A random bot (the baseline).
pub struct RandomBot {
    pub name: String,
    pub board: Board,
    pub view: EnemyView,
    pub rng: Xoshiro256,
}

impl RandomBot {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            board: Board::new(),
            view: EnemyView::new(),
            rng: thread_rng(),
        }
    }
}

impl Player for RandomBot {
    fn name(&self) -> &str { &self.name }
    fn board(&self) -> &Board { &self.board }
    fn board_mut(&mut self) -> &mut Board { &mut self.board }

    fn choose_move(&mut self, _deadline: Deadline) -> (usize, usize) {
        // A random unfired cell.
        let un = self.view.unknown();
        let cells: Vec<_> = un.iter_cells().collect();
        if cells.is_empty() {
            return (0, 0);
        }
        let i = self.rng.gen_range(cells.len() as u64) as usize;
        cells[i]
    }

    fn observe_result(&mut self, r: usize, c: usize, result: ShotResult) {
        self.view.observe(r, c, result);
    }

    fn reset(&mut self) {
        self.board.clear();
        self.view = EnemyView::new();
    }

    fn reseed(&mut self, seed: u64) {
        self.rng = Xoshiro256::from_seed(seed);
    }
}

/// A human — moves come from a callback (stdin or another implementation).
pub struct HumanPlayer {
    pub name: String,
    pub board: Board,
    pub view: EnemyView,
    /// The move-source callback: returns `(r, c)`.
    pub input_fn: Box<dyn FnMut() -> (usize, usize) + Send>,
}

impl HumanPlayer {
    pub fn new(name: impl Into<String>, input_fn: impl FnMut() -> (usize, usize) + Send + 'static) -> Self {
        Self {
            name: name.into(),
            board: Board::new(),
            view: EnemyView::new(),
            input_fn: Box::new(input_fn),
        }
    }
}

impl Player for HumanPlayer {
    fn name(&self) -> &str { &self.name }
    fn board(&self) -> &Board { &self.board }
    fn board_mut(&mut self) -> &mut Board { &mut self.board }

    fn choose_move(&mut self, _deadline: Deadline) -> (usize, usize) {
        (self.input_fn)()
    }

    fn observe_result(&mut self, r: usize, c: usize, result: ShotResult) {
        self.view.observe(r, c, result);
    }

    fn reset(&mut self) {
        self.board.clear();
        self.view = EnemyView::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::place_best_fleet;

    #[test]
    fn test_bot_plays_full_game() {
        let mut bot1 = BotPlayer::new("Bot1", 32, true).without_learning();
        let mut bot2 = BotPlayer::new("Bot2", 32, true).without_learning();
        bot1.place_fleet();
        bot2.place_fleet();

        let dl = Deadline::from_duration(std::time::Duration::from_millis(10));
        for _ in 0..200 {
            let (r, c) = bot1.choose_move(dl);
            let res = bot2.board_mut().shoot(r, c);
            bot1.observe_result(r, c, res);
            if bot2.is_defeated() { break; }

            let (r, c) = bot2.choose_move(dl);
            let res = bot1.board_mut().shoot(r, c);
            bot2.observe_result(r, c, res);
            if bot1.is_defeated() { break; }
        }
        assert!(bot1.is_defeated() || bot2.is_defeated());
    }

    #[test]
    fn test_pdf_bot_beats_random() {
        let mut pdf_bot = PdfBot::new("PDF");
        let mut rnd_bot = RandomBot::new("RND");
        let mut rng = thread_rng();
        pdf_bot.board = place_best_fleet(&mut rng, &PlacementConfig::default());
        rnd_bot.board = place_best_fleet(&mut rng, &PlacementConfig::default());

        let dl = Deadline::none();
        let mut pdf_wins = 0;
        for _ in 0..10 {
            pdf_bot.reset();
            rnd_bot.reset();
            pdf_bot.board = place_best_fleet(&mut rng, &PlacementConfig::default());
            rnd_bot.board = place_best_fleet(&mut rng, &PlacementConfig::default());
            let mut winner = 0;
            for _ in 0..200 {
                let (r, c) = pdf_bot.choose_move(dl);
                let res = rnd_bot.board_mut().shoot(r, c);
                pdf_bot.observe_result(r, c, res);
                if rnd_bot.is_defeated() { winner = 1; break; }
                let (r, c) = rnd_bot.choose_move(dl);
                let res = pdf_bot.board_mut().shoot(r, c);
                rnd_bot.observe_result(r, c, res);
                if pdf_bot.is_defeated() { winner = 2; break; }
            }
            if winner == 1 { pdf_wins += 1; }
        }
        assert!(pdf_wins >= 8, "PDF won only {}/10 vs random", pdf_wins);
    }
}
