//! Logika gry - pętla, tryby,team mode.

use crate::board::{Board, ShotResult};
use crate::player::{BotPlayer, Player};
use crate::rng::Xoshiro256;
use crate::time_limit::Deadline;
use std::time::Instant;

/// Pojedyncza gra między dwoma graczami
pub struct Game {
    pub p1: Box<dyn Player>,
    pub p2: Box<dyn Player>,
    pub moves_p1: u32,
    pub moves_p2: u32,
    pub started: Option<Instant>,
    pub finished: Option<Instant>,
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

    /// Uruchom grę do końca, zwróć zwycięzcę (1 lub 2).
    /// `deadline` określa limit czasu na pojedynczy ruch.
    pub fn play(&mut self, deadline: Deadline) -> u8 {
        self.started = Some(Instant::now());
        let max_moves = 200;
        for _ in 0..max_moves {
            // P1 strzela w P2
            let (r, c) = self.p1.choose_move(deadline);
            let res = self.p2.board_mut().shoot(r, c);
            self.p1.observe_result(r, c, res);
            self.p2.observe_incoming(r, c, res);
            self.moves_p1 += 1;
            if self.p2.is_defeated() {
                self.finished = Some(Instant::now());
                return 1;
            }
            // P2 strzela w P1
            let (r, c) = self.p2.choose_move(deadline);
            let res = self.p1.board_mut().shoot(r, c);
            self.p2.observe_result(r, c, res);
            self.p1.observe_incoming(r, c, res);
            self.moves_p2 += 1;
            if self.p1.is_defeated() {
                self.finished = Some(Instant::now());
                return 2;
            }
        }
        // Remis - decyduje mniej ruchów
        self.finished = Some(Instant::now());
        if self.moves_p1 <= self.moves_p2 { 1 } else { 2 }
    }

    /// Czas gry
    pub fn duration(&self) -> std::time::Duration {
        match (self.started, self.finished) {
            (Some(s), Some(f)) => f.duration_since(s),
            (Some(s), None) => s.elapsed(),
            _ => std::time::Duration::default(),
        }
    }
}

/// Tryb gry Team (gracz + bot vs gracz papierowy)
/// Bot SUGERUJE ruch, ale gracz może strzelić gdzie indziej.
pub struct TeamGame {
    /// Plansza wroga (papierowa) - tylko bot ją widzi przez obserwacje gracza
    pub enemy_view: crate::targeting::EnemyView,
    /// Bot doradca
    pub advisor: BotPlayer,
    /// Plansza naszego zespołu (z botem)
    pub our_board: Board,
    /// RNG
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

    /// Suggest the next move (using the engine's default deadline).
    /// For tests, use [`TeamGame::suggest_fast`] which uses no deadline.
    pub fn suggest(&mut self) -> (usize, usize) {
        self.advisor.view = self.enemy_view.clone();
        self.advisor.choose_move(Deadline::default_20s())
    }

    /// Suggest the next move with no deadline (fast, for tests).
    pub fn suggest_fast(&mut self) -> (usize, usize) {
        self.advisor.view = self.enemy_view.clone();
        self.advisor.choose_move(Deadline::none())
    }

    /// Gracz wykonał ruch w (r,c), wrogi gracz papierowy podał wynik
    pub fn observe(&mut self, r: usize, c: usize, result: ShotResult) {
        self.enemy_view.observe(r, c, result);
        self.advisor.view = self.enemy_view.clone();
        self.moves += 1;
    }

    /// Wrogi gracz papierowy strzela w naszą planszę
    pub fn enemy_shoot(&mut self, r: usize, c: usize) -> ShotResult {
        self.our_board.shoot(r, c)
    }

    /// Czy nasz zespół przegrał
    pub fn is_defeated(&self) -> bool {
        self.our_board.all_sunk()
    }
}

/// Tryb "Suggest-only" - bot doradza człowiekowi
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

    /// Suggest with no deadline (fast, for tests).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::placement::PlacementConfig;

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
        assert!((r2, c2) != (r, c));
    }
}
