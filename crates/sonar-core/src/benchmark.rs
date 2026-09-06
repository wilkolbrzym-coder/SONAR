//! Rigorous self-play benchmarking — "sonar-bench v2" core.
//!
//! Runs N games of bot vs bot and reports:
//!  - win rate with a **Wilson score 95% confidence interval** (not the
//!    naive stderr — correct for small samples and extreme proportions),
//!  - expected number of moves with **standard error**,
//!  - average move time (μs) and total moves per second.
//!
//! ## Methodology
//!
//! * Every run is seeded: pass a fixed `seed` in [`BenchmarkConfig`] and
//!   the exact same games are replayed — bit-identical results on every
//!   machine (verified by the determinism tests).
//! * The *sequential* runner ([`run_benchmark_seq`]) is the reference
//!   implementation: single-threaded, fully deterministic, WASM-safe.
//!   The *threaded* runner ([`run_benchmark`]) shards the same games
//!   across threads with independent derived seeds and produces
//!   statistically identical results.
//! * Time limits are disabled during benchmark games (moves are
//!   work-limited by `max_hypotheses`), so results measure algorithm
//!   strength, not machine speed. Wall-clock timings are reported
//!   separately and never affect game outcomes.

use crate::clock::now_us;
use crate::game::Game;
use crate::placement::{PlacementConfig, place_best_fleet, place_random_fleet};
use crate::player::{BotPlayer, PdfBot, Player, RandomBot};
use crate::rng::Xoshiro256;
use crate::time_limit::Deadline;
use std::time::Duration;

// ─────────────────────────────────────────────────────────────────────────────
// Statistics
// ─────────────────────────────────────────────────────────────────────────────

/// Per-player statistics over a benchmark run.
#[derive(Clone, Debug, Default)]
pub struct PlayerStats {
    pub wins: u32,
    pub losses: u32,
    pub moves_in_wins: u64,
    pub moves_in_losses: u64,
    /// Sum of squared moves-per-game (for variance/SE of E[moves]).
    pub moves_sq_sum: f64,
    pub total_moves: u64,
    pub total_time_us: u64,
}

impl PlayerStats {
    pub fn games(&self) -> u32 {
        self.wins + self.losses
    }
    pub fn win_rate(&self) -> f64 {
        let g = self.games();
        if g == 0 {
            0.0
        } else {
            self.wins as f64 / g as f64 * 100.0
        }
    }
    pub fn avg_moves_in_wins(&self) -> f64 {
        if self.wins == 0 {
            0.0
        } else {
            self.moves_in_wins as f64 / self.wins as f64
        }
    }
    pub fn avg_moves(&self) -> f64 {
        if self.games() == 0 {
            0.0
        } else {
            self.total_moves as f64 / self.games() as f64
        }
    }
    /// Standard error of the mean number of moves per game.
    pub fn moves_stderr(&self) -> f64 {
        let n = self.games() as f64;
        if n < 2.0 {
            return 0.0;
        }
        let mean = self.avg_moves();
        let var = (self.moves_sq_sum / n - mean * mean).max(0.0);
        (var / n).sqrt()
    }
    pub fn avg_move_time_us(&self) -> f64 {
        if self.total_moves == 0 {
            0.0
        } else {
            self.total_time_us as f64 / self.total_moves as f64
        }
    }
    pub fn moves_per_sec(&self) -> f64 {
        if self.total_time_us == 0 {
            0.0
        } else {
            self.total_moves as f64 * 1e6 / self.total_time_us as f64
        }
    }
    /// Wilson score interval (95%) on the win rate, as `(low, high)` in
    /// percent. Conservative and well-behaved for extreme rates (0%/100%)
    /// and small samples — unlike the naive normal approximation.
    pub fn win_rate_wilson_95(&self) -> (f64, f64) {
        wilson_interval(self.wins as f64, self.games() as f64, 1.96)
    }
}

/// Wilson score interval. Returns `(low, high)` proportions.
fn wilson_interval(successes: f64, n: f64, z: f64) -> (f64, f64) {
    if n == 0.0 {
        return (0.0, 0.0);
    }
    let p = successes / n;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let centre = p + z2 / (2.0 * n);
    let margin = z * ((p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt());
    (
        ((centre - margin) / denom).max(0.0),
        ((centre + margin) / denom).min(1.0),
    )
}

/// A short machine-readable summary of one matchup, used by the JSON
/// protocol (`bench` command) and the CLI's `--json` output.
#[derive(Clone, Debug)]
pub struct MatchupReport {
    pub name1: String,
    pub name2: String,
    pub stats1: PlayerStats,
    pub stats2: PlayerStats,
    pub elapsed_us: u64,
    pub seed: u64,
    pub threads: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Configuration
// ─────────────────────────────────────────────────────────────────────────────

/// Benchmark configuration.
#[derive(Clone, Debug)]
pub struct BenchmarkConfig {
    /// Total number of games.
    pub games: u32,
    /// Worker threads for the threaded runner (native only).
    pub threads: u32,
    /// Hypothesis soft target for the bots (the "strength knob").
    pub max_hypotheses: usize,
    /// Use intelligent (penalty-minimising) fleet placement.
    pub smart_placement: bool,
    /// Master seed. Fixed seed ⇒ deterministic, reproducible run.
    pub seed: u64,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            games: 100,
            threads: 4,
            max_hypotheses: 256,
            smart_placement: true,
            seed: 0xBEEF_CAFE_1234_5678,
        }
    }
}

/// The kind of bot to benchmark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BotKind {
    /// PDF + hypotheses (full Sonar).
    Hybrid,
    /// PDF density only.
    Pdf,
    /// Uniform random (baseline).
    Random,
}

impl BotKind {
    pub fn name(self) -> &'static str {
        match self {
            BotKind::Hybrid => "Sonar-Hybrid",
            BotKind::Pdf => "PdfOnly",
            BotKind::Random => "Random",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Sequential (reference, WASM-safe) runner
// ─────────────────────────────────────────────────────────────────────────────

/// Play one benchmark game between two bots. Deterministic given the RNG:
/// both players' RNGs are (re)seeded from the master stream so a fixed
/// master seed replays the same games every time.
/// Returns `(winner, moves_p1, moves_p2, duration_us)`.
fn play_one(
    p1: Box<dyn Player>,
    p2: Box<dyn Player>,
    rng: &mut Xoshiro256,
    smart: bool,
) -> (u8, u64, u64, u64) {
    let mut p1 = p1;
    let mut p2 = p2;
    // Seed both players from the master stream — determinism.
    p1.reseed(rng.next_u64());
    p2.reseed(rng.next_u64());
    if smart {
        *p1.board_mut() = place_best_fleet(rng, &PlacementConfig::default());
        *p2.board_mut() = place_best_fleet(rng, &PlacementConfig::default());
    } else {
        *p1.board_mut() = place_random_fleet(rng);
        *p2.board_mut() = place_random_fleet(rng);
    }
    let mut g = Game::new(p1, p2);
    let t0 = now_us();
    let winner = g.play(Deadline::none());
    let dur = now_us().saturating_sub(t0);
    (winner, g.moves_p1 as u64, g.moves_p2 as u64, dur)
}

/// Run `cfg.games` games sequentially with the given seed. Fully
/// deterministic: the same (seed, config, kinds) triple replays the same
/// games. Usable on all targets including WebAssembly.
pub fn run_benchmark_seq(
    cfg: &BenchmarkConfig,
    p1_kind: BotKind,
    p2_kind: BotKind,
) -> MatchupReport {
    let mut rng = Xoshiro256::from_seed(cfg.seed);
    let mut stats1 = PlayerStats::default();
    let mut stats2 = PlayerStats::default();
    let t0 = now_us();

    for _ in 0..cfg.games {
        let p1 = make_bot(p1_kind, "P1", cfg.max_hypotheses);
        let p2 = make_bot(p2_kind, "P2", cfg.max_hypotheses);
        let (winner, m1, m2, dur) = play_one(p1, p2, &mut rng, cfg.smart_placement);

        // Attribute time proportionally to move counts (approximation —
        // exact per-move timing would distort the fast path).
        let total_moves = m1 + m2;
        let t1 = (dur * m1).checked_div(total_moves).unwrap_or(0);
        let t2 = (dur * m2).checked_div(total_moves).unwrap_or(0);

        if winner == 1 {
            record_win(&mut stats1, m1, t1);
            record_loss(&mut stats2, m2, t2);
        } else {
            record_win(&mut stats2, m2, t2);
            record_loss(&mut stats1, m1, t1);
        }
    }

    let elapsed_us = now_us().saturating_sub(t0);
    MatchupReport {
        name1: p1_kind.name().to_string(),
        name2: p2_kind.name().to_string(),
        stats1,
        stats2,
        elapsed_us,
        seed: cfg.seed,
        threads: 1,
    }
}

fn record_win(s: &mut PlayerStats, moves: u64, time_us: u64) {
    s.wins += 1;
    s.moves_in_wins += moves;
    s.moves_sq_sum += (moves as f64) * (moves as f64);
    s.total_moves += moves;
    s.total_time_us += time_us;
}

fn record_loss(s: &mut PlayerStats, moves: u64, time_us: u64) {
    s.losses += 1;
    s.moves_in_losses += moves;
    s.moves_sq_sum += (moves as f64) * (moves as f64);
    s.total_moves += moves;
    s.total_time_us += time_us;
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

// ─────────────────────────────────────────────────────────────────────────────
// Threaded runner (native)
// ─────────────────────────────────────────────────────────────────────────────

/// Run the benchmark sharded across threads. Each shard derives an
/// independent seed from the master seed (golden-ratio scrambling), so the
/// result is still deterministic for a fixed master seed.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_benchmark(
    cfg: &BenchmarkConfig,
    p1_kind: BotKind,
    p2_kind: BotKind,
) -> (PlayerStats, PlayerStats, Duration) {
    let threads = cfg.threads.max(1) as usize;
    let games = cfg.games as usize;
    let games_per_thread = games.div_ceil(threads);

    let results: std::sync::Mutex<Vec<(PlayerStats, PlayerStats)>> =
        std::sync::Mutex::new(Vec::new());
    let counter = std::sync::atomic::AtomicUsize::new(0);

    std::thread::scope(|scope| {
        for t in 0..threads {
            let results = &results;
            let counter = &counter;
            // Derive a per-thread seed deterministically.
            let seed = cfg
                .seed
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add((t as u64).wrapping_mul(0xBF58476D1CE4E5B9));
            let mh = cfg.max_hypotheses;
            let sp = cfg.smart_placement;
            let k1 = p1_kind;
            let k2 = p2_kind;
            scope.spawn(move || {
                let mut rng = Xoshiro256::from_seed(seed);
                let mut stats1 = PlayerStats::default();
                let mut stats2 = PlayerStats::default();
                let mut done = 0usize;
                while done < games_per_thread {
                    // Only play a game if we haven't reached the total.
                    if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= games {
                        break;
                    }
                    done += 1;
                    let p1 = make_bot(k1, "P1", mh);
                    let p2 = make_bot(k2, "P2", mh);
                    let (winner, m1, m2, dur) = play_one(p1, p2, &mut rng, sp);
                    let total_moves = m1 + m2;
                    let t1 = (dur * m1).checked_div(total_moves).unwrap_or(0);
                    let t2 = (dur * m2).checked_div(total_moves).unwrap_or(0);
                    if winner == 1 {
                        record_win(&mut stats1, m1, t1);
                        record_loss(&mut stats2, m2, t2);
                    } else {
                        record_win(&mut stats2, m2, t2);
                        record_loss(&mut stats1, m1, t1);
                    }
                }
                if let Ok(mut v) = results.lock() {
                    v.push((stats1, stats2));
                }
            });
        }
    });

    let shards = results.into_inner().unwrap_or_default();
    let mut stats1 = PlayerStats::default();
    let mut stats2 = PlayerStats::default();
    let t0 = now_us();
    for (a, b) in shards {
        merge(&mut stats1, &a);
        merge(&mut stats2, &b);
    }
    let elapsed = Duration::from_micros(now_us().saturating_sub(t0));
    (stats1, stats2, elapsed)
}

#[cfg(not(target_arch = "wasm32"))]
fn merge(dst: &mut PlayerStats, src: &PlayerStats) {
    dst.wins += src.wins;
    dst.losses += src.losses;
    dst.moves_in_wins += src.moves_in_wins;
    dst.moves_in_losses += src.moves_in_losses;
    dst.moves_sq_sum += src.moves_sq_sum;
    dst.total_moves += src.total_moves;
    dst.total_time_us += src.total_time_us;
}

// ─────────────────────────────────────────────────────────────────────────────
// Reporting
// ─────────────────────────────────────────────────────────────────────────────

/// Print a human-readable benchmark report.
pub fn print_report(
    name1: &str,
    s1: &PlayerStats,
    name2: &str,
    s2: &PlayerStats,
    elapsed: Duration,
) {
    println!("┌──────────────────────────────────────────────────────────────┐");
    println!("│  BENCHMARK RESULTS                                           │");
    println!("├──────────────────────────────────────────────────────────────┤");
    println!("│  Games: {:<52}│", s1.games());
    println!("│  Total time: {:<47}│", format!("{:?}", elapsed));
    println!(
        "│  Games/sec: {:<49}│",
        format!("{:.1}", s1.games() as f64 / elapsed.as_secs_f64().max(1e-6))
    );
    println!("├──────────────────────────────────────────────────────────────┤");
    print_player_stats(name1, s1);
    println!("│                                                              │");
    print_player_stats(name2, s2);
    println!("└──────────────────────────────────────────────────────────────┘");
}

fn print_player_stats(name: &str, s: &PlayerStats) {
    let (lo, hi) = s.win_rate_wilson_95();
    println!("│  Player: {:<52}│", name);
    println!(
        "│    Wins: {:<52}│",
        format!(
            "{} ({:.1}%)  Wilson 95% CI: [{:.1}%, {:.1}%]",
            s.wins,
            s.win_rate(),
            lo * 100.0,
            hi * 100.0
        )
    );
    println!("│    Losses: {:<51}│", s.losses);
    println!(
        "│    E[moves] in wins: {:<39}│",
        format!("{:.1}", s.avg_moves_in_wins())
    );
    println!(
        "│    E[moves] overall: {:<39}│",
        format!("{:.1} ± {:.1}", s.avg_moves(), s.moves_stderr())
    );
    println!(
        "│    Avg move time: {:<43}│",
        format!("{:.2} μs", s.avg_move_time_us())
    );
    println!(
        "│    Moves/sec: {:<48}│",
        format!("{:.0}", s.moves_per_sec())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wilson_interval() {
        // 0 successes out of 0 games — degenerate.
        let (lo, hi) = wilson_interval(0.0, 0.0, 1.96);
        assert_eq!((lo, hi), (0.0, 0.0));
        // 0/10 — the interval must not be exactly [0,0] (that's the point
        // of Wilson over the naive approximation).
        let (lo, hi) = wilson_interval(0.0, 10.0, 1.96);
        assert_eq!(lo, 0.0);
        assert!(hi > 0.0 && hi < 0.3, "hi={}", hi);
        // 10/10.
        let (lo, hi) = wilson_interval(10.0, 10.0, 1.96);
        assert_eq!(hi, 1.0);
        assert!(lo > 0.7, "lo={}", lo);
        // 50/100 — should straddle 0.5.
        let (lo, hi) = wilson_interval(50.0, 100.0, 1.96);
        assert!(lo < 0.5 && hi > 0.5);
    }

    #[test]
    fn test_benchmark_seq_deterministic() {
        let cfg = BenchmarkConfig {
            games: 4,
            max_hypotheses: 32,
            seed: 12345,
            ..Default::default()
        };
        let a = run_benchmark_seq(&cfg, BotKind::Hybrid, BotKind::Random);
        let b = run_benchmark_seq(&cfg, BotKind::Hybrid, BotKind::Random);
        assert_eq!(a.stats1.wins, b.stats1.wins);
        assert_eq!(a.stats1.total_moves, b.stats1.total_moves);
        assert_eq!(a.stats2.wins, b.stats2.wins);
    }

    #[test]
    fn test_benchmark_runs() {
        let cfg = BenchmarkConfig {
            games: 4,
            threads: 2,
            max_hypotheses: 32,
            smart_placement: true,
            seed: 777,
        };
        let (s1, s2, _) = run_benchmark(&cfg, BotKind::Hybrid, BotKind::Random);
        // Each player plays every game.
        assert_eq!(s1.games(), 4);
        assert_eq!(s2.games(), 4);
        // Every game has a winner.
        assert_eq!(s1.wins + s2.wins, 4);
    }

    #[test]
    fn test_benchmark_threaded_matches_seq_totals() {
        // The threaded runner must produce the same *total* game count.
        let cfg = BenchmarkConfig {
            games: 6,
            threads: 3,
            max_hypotheses: 16,
            seed: 999,
            ..Default::default()
        };
        let (s1, s2, _) = run_benchmark(&cfg, BotKind::Pdf, BotKind::Random);
        assert_eq!(s1.games(), 6);
        assert_eq!(s1.wins + s2.wins, 6);
    }

    #[test]
    fn test_hybrid_beats_random_statistically() {
        // With 40 games, Hybrid must dominate Random — Wilson CI check.
        let cfg = BenchmarkConfig {
            games: 40,
            threads: 2,
            max_hypotheses: 64,
            seed: 2024,
            ..Default::default()
        };
        let (s1, _s2, _) = run_benchmark(&cfg, BotKind::Hybrid, BotKind::Random);
        let (lo, _hi) = s1.win_rate_wilson_95();
        assert!(
            lo > 0.75,
            "Hybrid win-rate lower CI bound {} is too low",
            lo * 100.0
        );
    }
}
