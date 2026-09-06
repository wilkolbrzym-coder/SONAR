//! Game logic — the match loop, play modes, and team mode.

use crate::board::{Board, ShotResult};
use crate::clock::now_us;
use crate::player::{BotPlayer, Player};
use crate::rng::Xoshiro256;
use crate::time_limit::Deadline;

/// A single game between two players.
pub struct Game {
    pub p1: Box<dyn Player>,
    pub p2: Box<dyn Player>,
    pub moves_p1: u32,
    pub moves_p2: u32,
    /// Monotonic timestamp (microseconds) of game start, if started.
    pub started: Option<u64>,
    /// Monotonic timestamp (microseconds) of game end, if finished.
    pub finished: Option<u64>,
}

impl Game {
    pub fn new(p1: Box<dyn Player>, p2: Box<dyn Player>) -> Self {
        Self {
            p1,
            p2,
            moves_p1: 0,
            moves_p2: 0,
            started: None,
            finished: None,
        }
    }

    /// Play the game to completion and return the winner (1 or 2).
    /// `deadline` is the per-move time limit (shared by both players —
    /// use [`Game::play_asymmetric`] for unequal budgets).
    pub fn play(&mut self, deadline: Deadline) -> u8 {
        self.play_asymmetric(deadline, deadline)
    }

    /// Play with different per-move deadlines for each player (used by the
    /// time-handicap benchmarks: `bench-2x`, `bench-half`).
    pub fn play_asymmetric(&mut self, dl1: Deadline, dl2: Deadline) -> u8 {
        self.started = Some(now_us());
        let max_moves = 200;
        for _ in 0..max_moves {
            // P1 fires at P2.
            let (r, c) = self.p1.choose_move(dl1);
            let res = self.p2.board_mut().shoot(r, c);
            self.p1.observe_result(r, c, res);
            self.p2.observe_incoming(r, c, res);
            self.moves_p1 += 1;
            if self.p2.is_defeated() {
                self.finished = Some(now_us());
                return 1;
            }
            // P2 fires at P1.
            let (r, c) = self.p2.choose_move(dl2);
            let res = self.p1.board_mut().shoot(r, c);
            self.p2.observe_result(r, c, res);
            self.p1.observe_incoming(r, c, res);
            self.moves_p2 += 1;
            if self.p1.is_defeated() {
                self.finished = Some(now_us());
                return 2;
            }
        }
        // Draw fallback — the player who fired fewer shots wins.
        self.finished = Some(now_us());
        if self.moves_p1 <= self.moves_p2 { 1 } else { 2 }
    }

    /// Total game duration.
    pub fn duration(&self) -> std::time::Duration {
        match (self.started, self.finished) {
            (Some(s), Some(f)) => std::time::Duration::from_micros(f.saturating_sub(s)),
            (Some(s), None) => std::time::Duration::from_micros(now_us().saturating_sub(s)),
            _ => std::time::Duration::default(),
        }
    }
}

/// Team mode: a human plays against a real-world ("paper") opponent while
/// the bot acts as an advisor. The bot *suggests* moves, but the human may
/// fire anywhere.
pub struct TeamGame {
    /// The enemy board as seen through the human's reported observations.
    pub enemy_view: crate::targeting::EnemyView,
    /// The advising bot.
    pub advisor: BotPlayer,
    /// Our team's board (shared with the advisor).
    pub our_board: Board,
    /// RNG.
    pub rng: Xoshiro256,
    pub moves: u32,
}

impl TeamGame {
    pub fn new() -> Self {
        let mut advisor = BotPlayer::new("Advisor", 2048, true);
        advisor.place_fleet();
        let our_board = advisor.board.clone();
        Self {
            enemy_view: crate::targeting::EnemyView::new(),
            advisor,
            our_board,
            rng: Xoshiro256::from_seed(crate::rng::random_u64()),
            moves: 0,
        }
    }

    /// Suggest the next move (uses the engine's default deadline).
    /// For tests, use [`TeamGame::suggest_fast`] (no deadline).
    pub fn suggest(&mut self) -> (usize, usize) {
        self.advisor.view = self.enemy_view.clone();
        self.advisor.choose_move(Deadline::default_20s())
    }

    /// Suggest the next move with no deadline (fast — for tests).
    pub fn suggest_fast(&mut self) -> (usize, usize) {
        self.advisor.view = self.enemy_view.clone();
        self.advisor.choose_move(Deadline::none())
    }

    /// The human fired at `(r, c)`; the real-world opponent reported `result`.
    pub fn observe(&mut self, r: usize, c: usize, result: ShotResult) {
        self.enemy_view.observe(r, c, result);
        self.advisor.view = self.enemy_view.clone();
        self.moves += 1;
    }

    /// The real-world opponent fired at our board.
    pub fn enemy_shoot(&mut self, r: usize, c: usize) -> ShotResult {
        self.our_board.shoot(r, c)
    }

    /// Has our team lost?
    pub fn is_defeated(&self) -> bool {
        self.our_board.all_sunk()
    }
}

impl Default for TeamGame {
    fn default() -> Self {
        Self::new()
    }
}

/// Suggest-only mode: the bot advises a human who plays a physical board.
pub struct SuggestMode {
    pub advisor: BotPlayer,
    pub rng: Xoshiro256,
}

impl SuggestMode {
    pub fn new() -> Self {
        let advisor = BotPlayer::new("Advisor", 2048, true);
        Self {
            advisor,
            rng: Xoshiro256::from_seed(crate::rng::random_u64()),
        }
    }

    pub fn suggest(&mut self) -> (usize, usize) {
        self.advisor.choose_move(Deadline::default_20s())
    }

    /// Suggest with no deadline (fast — for tests).
    pub fn suggest_fast(&mut self) -> (usize, usize) {
        self.advisor.choose_move(Deadline::none())
    }

    pub fn observe(&mut self, r: usize, c: usize, result: ShotResult) {
        self.advisor.observe_result(r, c, result);
    }

    pub fn reset(&mut self) {
        self.advisor.reset();
    }
}

impl Default for SuggestMode {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_game_completes() {
        let mut b1 = BotPlayer::new("B1", 32, true).without_learning();
        let mut b2 = BotPlayer::new("B2", 32, true).without_learning();
        b1.place_fleet();
        b2.place_fleet();
        let mut g = Game::new(Box::new(b1), Box::new(b2));
        let dl = Deadline::from_duration(std::time::Duration::from_millis(10));
        let winner = g.play(dl);
        assert!(winner == 1 || winner == 2);
        assert!(g.duration().as_millis() < 60000);
    }

    #[test]
    fn test_team_game_suggestion() {
        let mut tg = TeamGame::new();
        let (r, c) = tg.suggest_fast();
        assert!(r < 10 && c < 10);
        tg.observe(r, c, ShotResult::Miss);
        let (r2, c2) = tg.suggest_fast();
        assert_ne!((r2, c2), (r, c));
    }

    #[test]
    fn test_asymmetric_deadlines_end_game() {
        let mut b1 = BotPlayer::new("B1", 32, true).without_learning();
        let mut b2 = BotPlayer::new("B2", 32, true).without_learning();
        b1.place_fleet();
        b2.place_fleet();
        let mut g = Game::new(Box::new(b1), Box::new(b2));
        let winner = g.play_asymmetric(Deadline::none(), Deadline::none());
        assert!(winner == 1 || winner == 2);
        assert!(g.started.is_some() && g.finished.is_some());
        assert!(g.finished.unwrap() >= g.started.unwrap());
    }
}
