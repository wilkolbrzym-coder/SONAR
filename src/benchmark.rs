//! Self-play benchmark.
//!
//! Uruchamia N gier bot vs bot i podaje:
//!  - % wygranych
//!  - średnią liczbę ruchów do zwycięstwa
//!  - średni czas ruchu (ns)
//!  - łączne ruchy na sekundę

use crate::game::Game;
use crate::player::{BotPlayer, PdfBot, Player, RandomBot};
use crate::placement::{place_best_fleet, place_random_fleet, PlacementConfig};
use crate::rng::Xoshiro256;
use crate::time_limit::Deadline;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Statystyki pojedynczego gracza w benchmarku
#[derive(Clone, Debug, Default)]
pub struct PlayerStats {
    pub wins: u32,
    pub losses: u32,
    pub moves_in_wins: u64,
    pub moves_in_losses: u64,
    pub total_moves: u64,
    pub total_time_ns: u64,
}

impl PlayerStats {
    pub fn games(&self) -> u32 { self.wins + self.losses }
    pub fn win_rate(&self) -> f64 {
        let g = self.games();
        if g == 0 { 0.0 } else { self.wins as f64 / g as f64 * 100.0 }
    }
    pub fn avg_moves_in_wins(&self) -> f64 {
        if self.wins == 0 { 0.0 } else { self.moves_in_wins as f64 / self.wins as f64 }
    }
    pub fn avg_move_time_us(&self) -> f64 {
        if self.total_moves == 0 { 0.0 } else {
            self.total_time_ns as f64 / self.total_moves as f64 / 1000.0
        }
    }
    pub fn moves_per_sec(&self) -> f64 {
        if self.total_time_ns == 0 { 0.0 } else {
            self.total_moves as f64 * 1e9 / self.total_time_ns as f64
        }
    }
}

/// Konfiguracja benchmarku
#[derive(Clone, Debug)]
pub struct BenchmarkConfig {
    pub games: u32,
    /// Ile gier na wątek
    pub games_per_thread: u32,
    /// Ile wątków
    pub threads: u32,
    /// Maks. hipotez w botach
    pub max_hypotheses: usize,
    /// Użyj inteligentnego placementu
    pub smart_placement: bool,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            games: 100,
            games_per_thread: 25,
            threads: 4,
            max_hypotheses: 256,
            smart_placement: true,
        }
    }
}

/// Rodzaj bota do benchmarku
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BotKind {
    Hybrid,    // PDF + Hipotezy
    Pdf,       // Tylko PDF
    Random,    // Losowy
}

/// Uruchom serię gier między dwoma rodzajami botów
pub fn run_benchmark(
    cfg: &BenchmarkConfig,
    p1_kind: BotKind,
    p2_kind: BotKind,
) -> (PlayerStats, PlayerStats, Duration) {
    let stats1 = Arc::new(AtomicStats::new());
    let stats2 = Arc::new(AtomicStats::new());
    let start = Instant::now();

    let total_games = cfg.games;
    let threads = cfg.threads.max(1) as usize;
    let games_per_thread = (total_games as usize + threads - 1) / threads;

    let mut handles = Vec::new();
    for _ in 0..threads {
        let s1 = stats1.clone();
        let s2 = stats2.clone();
        let seed = crate::rng::random_u64();
        let gpt = games_per_thread as u32;
        let mh = cfg.max_hypotheses;
        let sp = cfg.smart_placement;
        handles.push(std::thread::spawn(move || {
            let mut rng = Xoshiro256::from_seed(seed);
            let mut games_done = 0u32;
            while games_done < gpt {
                games_done += 1;
                let mut p1 = make_bot(p1_kind, "P1", mh);
                let mut p2 = make_bot(p2_kind, "P2", mh);
                if sp {
                    *p1.board_mut() = place_best_fleet(&mut rng, &PlacementConfig::default());
                    *p2.board_mut() = place_best_fleet(&mut rng, &PlacementConfig::default());
                } else {
                    *p1.board_mut() = place_random_fleet(&mut rng);
                    *p2.board_mut() = place_random_fleet(&mut rng);
                }

                let mut g = Game::new(p1, p2);
                let winner = g.play(Deadline::none());

                // Zbierz statystyki
                let m1 = g.moves_p1 as u64;
                let m2 = g.moves_p2 as u64;
                let d = g.duration().as_nanos() as u64;
                // Przybliżony czas ruchu: rozdziel proporcjonalnie
                let total_moves = m1 + m2;
                let t1 = if total_moves > 0 { d * m1 / total_moves } else { 0 };
                let t2 = if total_moves > 0 { d * m2 / total_moves } else { 0 };

                if winner == 1 {
                    s1.record_win(m1, t1);
                    s2.record_loss(m2, t2);
                } else {
                    s2.record_win(m2, t2);
                    s1.record_loss(m1, t1);
                }
            }
        }));
    }
    for h in handles { let _ = h.join(); }

    let elapsed = start.elapsed();
    (stats1.into_stats(), stats2.into_stats(), elapsed)
}

fn make_bot(kind: BotKind, name: &str, max_hyp: usize) -> Box<dyn Player> {
    match kind {
        BotKind::Hybrid => Box::new(
            BotPlayer::new(name, max_hyp, true)
                .without_learning()
                .with_deadline(Deadline::none()),
        ),
        BotKind::Pdf => Box::new(PdfBot::new(name)),
        BotKind::Random => Box::new(RandomBot::new(name)),
    }
}

/// Atomowe statystyki dla współbieżności
struct AtomicStats {
    wins: AtomicU64,
    losses: AtomicU64,
    moves_in_wins: AtomicU64,
    moves_in_losses: AtomicU64,
    total_moves: AtomicU64,
    total_time_ns: AtomicU64,
}

impl AtomicStats {
    fn new() -> Self {
        Self {
            wins: AtomicU64::new(0),
            losses: AtomicU64::new(0),
            moves_in_wins: AtomicU64::new(0),
            moves_in_losses: AtomicU64::new(0),
            total_moves: AtomicU64::new(0),
            total_time_ns: AtomicU64::new(0),
        }
    }

    fn record_win(&self, moves: u64, time_ns: u64) {
        self.wins.fetch_add(1, Ordering::Relaxed);
        self.moves_in_wins.fetch_add(moves, Ordering::Relaxed);
        self.total_moves.fetch_add(moves, Ordering::Relaxed);
        self.total_time_ns.fetch_add(time_ns, Ordering::Relaxed);
    }

    fn record_loss(&self, moves: u64, time_ns: u64) {
        self.losses.fetch_add(1, Ordering::Relaxed);
        self.moves_in_losses.fetch_add(moves, Ordering::Relaxed);
        self.total_moves.fetch_add(moves, Ordering::Relaxed);
        self.total_time_ns.fetch_add(time_ns, Ordering::Relaxed);
    }

    fn into_stats(&self) -> PlayerStats {
        PlayerStats {
            wins: self.wins.load(Ordering::Relaxed) as u32,
            losses: self.losses.load(Ordering::Relaxed) as u32,
            moves_in_wins: self.moves_in_wins.load(Ordering::Relaxed),
            moves_in_losses: self.moves_in_losses.load(Ordering::Relaxed),
            total_moves: self.total_moves.load(Ordering::Relaxed),
            total_time_ns: self.total_time_ns.load(Ordering::Relaxed),
        }
    }
}

/// Wydrukuj raport benchmarku
pub fn print_report(
    name1: &str, s1: &PlayerStats,
    name2: &str, s2: &PlayerStats,
    elapsed: Duration,
) {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           BENCHMARK SELF-PLAY - WYNIKI                       ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Gry łącznie: {:<46}║", format!("{}", s1.games()));
    println!("║  Czas łącznie: {:<45}║", format!("{:?}", elapsed));
    println!("║  Gry/sek: {:<50}║", format!("{:.1}", s1.games() as f64 / elapsed.as_secs_f64().max(0.001)));
    println!("╠══════════════════════════════════════════════════════════════╣");
    print_player_stats(name1, s1);
    println!("║                                                              ║");
    print_player_stats(name2, s2);
    println!("╚══════════════════════════════════════════════════════════════╝");
}

fn print_player_stats(name: &str, s: &PlayerStats) {
    println!("║  Gracz: {:<52}║", name);
    println!("║    Zwycięstwa: {:<46}║", format!("{} ({:.1}%)", s.wins, s.win_rate()));
    println!("║    Porażki:    {:<46}║", s.losses);
    println!("║    Średnio ruchów w wygranych: {:<29}║", format!("{:.1}", s.avg_moves_in_wins()));
    println!("║    Średni czas ruchu: {:<36}║", format!("{:.2} μs", s.avg_move_time_us()));
    println!("║    Ruchów/sek: {:<46}║", format!("{:.0}", s.moves_per_sec()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark_runs() {
        let cfg = BenchmarkConfig {
            games: 4,
            games_per_thread: 1,
            threads: 4,
            max_hypotheses: 64,
            smart_placement: true,
        };
        let (s1, s2, _) = run_benchmark(&cfg, BotKind::Hybrid, BotKind::Random);
        // Każdy z graczy ma 4 gry
        assert_eq!(s1.games(), 4);
        assert_eq!(s2.games(), 4);
        // Razem wygranych = 4 (każda gra ma zwycięzcę)
        assert_eq!(s1.wins + s2.wins, 4);
    }
}
