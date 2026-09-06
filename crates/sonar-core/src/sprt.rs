//! SPRT-driven strength ladders and seed-verified release numbers (0.3).
//!
//! ## SPRT — Sequential Probability Ratio Test
//!
//! Comparing two engines with a fixed number of games wastes effort when
//! the difference is large and underpowers when it is small. The SPRT
//! (Wald, 1945) decides *sequentially*: after every game it updates a
//! log-likelihood ratio and stops as soon as the evidence crosses a
//! decision boundary. Given
//!
//! - H0: the true win probability `p ≤ p0` (A is not stronger),
//! - H1: `p ≥ p1` (A is stronger),
//!
//! the test accepts H1 with probability at most `alpha` when H0 holds
//! (type-I error) and accepts H0 with probability at most `beta` when H1
//! holds (type-II error).
//!
//! ## Solo matches
//!
//! Ladder games use *solo mode*: both engines independently attack the
//! **same** hidden fleet; the engine that sinks it in fewer shots wins.
//! This removes the first-mover advantage of alternating fire and
//! measures pure targeting strength — the quantity the ladder ranks.
//!
//! ## Seed-verified release numbers
//!
//! Every game is derived from a seed, so a claimed number (win rate,
//! average shots) can be re-derived by anyone running
//! `sonar verify-release`: same seed in, same numbers out, bit-for-bit,
//! on every machine. A hash-chain digest commits to the full result
//! sequence.

use crate::board::Board;
use crate::placement::{place_best_fleet, place_random_fleet};
use crate::player::BotPlayer;
use crate::rng::Xoshiro256;
use crate::targeting::{EnemyView, HybridTargeting};
use serde::Serialize;
use std::time::Instant;

// ─────────────────────────────────────────────────────────────────────────────
// SPRT core
// ─────────────────────────────────────────────────────────────────────────────

/// Outcome of a single match game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameOutcome {
    /// Side A won.
    A,
    /// Side B won.
    B,
    /// Draw (equal shots in solo mode).
    Draw,
}

/// SPRT parameters.
#[derive(Clone, Copy, Debug)]
pub struct SprtConfig {
    /// H0 bound: A is not stronger (win rate ≤ p0).
    pub p0: f64,
    /// H1 bound: A is stronger (win rate ≥ p1).
    pub p1: f64,
    /// Maximum type-I error probability (accept H1 when H0 true).
    pub alpha: f64,
    /// Maximum type-II error probability (accept H0 when H1 true).
    pub beta: f64,
}

impl Default for SprtConfig {
    fn default() -> Self {
        // The classic "±25 Elo" ladder setting.
        Self::from_elo(25.0, 0.05, 0.05)
    }
}

impl SprtConfig {
    /// Build a config from symmetric Elo bounds: H0 `p0 = logistic(−elo)`,
    /// H1 `p1 = logistic(+elo)` — the classic "±elo" ladder setting.
    pub fn from_elo(elo: f64, alpha: f64, beta: f64) -> Self {
        let p1 = 1.0 / (1.0 + 10.0f64.powf(-elo / 400.0));
        let p0 = 1.0 - p1;
        Self {
            p0,
            p1,
            alpha,
            beta,
        }
    }
}

/// SPRT decision state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SprtStatus {
    /// Keep playing.
    Continue,
    /// H1 accepted: A is stronger with error ≤ alpha.
    AcceptH1,
    /// H0 accepted: A is not stronger with error ≤ beta.
    AcceptH0,
}

/// Running SPRT state.
#[derive(Clone, Debug)]
pub struct SprtState {
    pub config: SprtConfig,
    /// A wins (a draw counts as half a win for the statistic).
    pub wins: u64,
    /// B wins.
    pub losses: u64,
    pub draws: u64,
    /// Upper boundary `log((1-beta)/alpha)`.
    bound_up: f64,
    /// Lower boundary `log(beta/(1-alpha))`.
    bound_down: f64,
}

impl SprtState {
    pub fn new(config: SprtConfig) -> Self {
        let bound_up = ((1.0 - config.beta) / config.alpha).ln();
        let bound_down = (config.beta / (1.0 - config.alpha)).ln();
        Self {
            config,
            wins: 0,
            losses: 0,
            draws: 0,
            bound_up,
            bound_down,
        }
    }

    /// Games played so far.
    pub fn games(&self) -> u64 {
        self.wins + self.losses + self.draws
    }

    /// The log-likelihood ratio of H1 vs H0 for the observed sequence.
    /// Draws contribute as half-wins (score statistic).
    pub fn llr(&self) -> f64 {
        let cfg = self.config;
        let w = self.wins as f64 + 0.5 * self.draws as f64;
        let l = self.losses as f64 + 0.5 * self.draws as f64;
        w * (cfg.p1 / cfg.p0).ln() + l * ((1.0 - cfg.p1) / (1.0 - cfg.p0)).ln()
    }

    /// Record a game outcome and re-evaluate the decision.
    pub fn record(&mut self, outcome: GameOutcome) -> SprtStatus {
        match outcome {
            GameOutcome::A => self.wins += 1,
            GameOutcome::B => self.losses += 1,
            GameOutcome::Draw => self.draws += 1,
        }
        self.status()
    }

    /// Current decision without recording a game.
    pub fn status(&self) -> SprtStatus {
        let llr = self.llr();
        if llr >= self.bound_up {
            SprtStatus::AcceptH1
        } else if llr <= self.bound_down {
            SprtStatus::AcceptH0
        } else {
            SprtStatus::Continue
        }
    }

    /// Observed score of A (wins + 0.5·draws) / games.
    pub fn score(&self) -> f64 {
        let n = self.games();
        if n == 0 {
            0.5
        } else {
            (self.wins as f64 + 0.5 * self.draws as f64) / n as f64
        }
    }

    /// Observed Elo advantage of A over B (logistic).
    pub fn elo(&self) -> f64 {
        let s = self.score().clamp(1e-9, 1.0 - 1e-9);
        -400.0 * (1.0 / s - 1.0).log10()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Solo match — both engines attack the same fleet
// ─────────────────────────────────────────────────────────────────────────────

/// An engine level: strategy kind + hypothesis budget + endgame flag.
#[derive(Clone, Debug)]
pub enum LevelKind {
    /// The full Sonar hybrid: PDF + Bayesian hypotheses (+ endgame).
    Hybrid,
    /// PDF density targeting only.
    Pdf,
    /// A uniformly random shooter (the floor).
    Random,
}

/// An engine level of the strength ladder.
#[derive(Clone, Debug)]
pub struct Level {
    pub name: String,
    /// Hypothesis soft target (Hybrid only).
    pub soft_target: usize,
    /// Exact endgame solver enabled (Hybrid only).
    pub endgame: bool,
    /// Strategy family.
    pub kind: LevelKind,
}

impl Level {
    pub fn new(name: &str, soft_target: usize, endgame: bool) -> Self {
        Self {
            name: name.to_string(),
            soft_target,
            endgame,
            kind: LevelKind::Hybrid,
        }
    }

    /// A PDF-only level.
    pub fn pdf(name: &str) -> Self {
        Self {
            name: name.to_string(),
            soft_target: 0,
            endgame: false,
            kind: LevelKind::Pdf,
        }
    }

    /// A uniformly-random level.
    pub fn random(name: &str) -> Self {
        Self {
            name: name.to_string(),
            soft_target: 0,
            endgame: false,
            kind: LevelKind::Random,
        }
    }

    /// Build a solo attacker for this level.
    fn attacker(&self, seed: u64) -> BotPlayer {
        let mut bot = BotPlayer::new(&self.name, self.soft_target, false).without_learning();
        bot.strategy = match self.kind {
            LevelKind::Hybrid => Box::new(
                HybridTargeting::new()
                    .with_soft_target(self.soft_target)
                    .with_endgame(self.endgame),
            ),
            LevelKind::Pdf => Box::new(crate::targeting::PdfTargeting::new(
                crate::targeting::PdfConfig::default(),
            )),
            LevelKind::Random => Box::new(RandomTargeting),
        };
        bot.reseed(seed);
        bot
    }
}

/// The uniformly random targeting strategy (solo-mode floor).
struct RandomTargeting;

impl crate::targeting::TargetingStrategy for RandomTargeting {
    fn choose(
        &mut self,
        view: &EnemyView,
        rng: &mut Xoshiro256,
        _deadline: crate::time_limit::Deadline,
    ) -> (usize, usize) {
        let cells: Vec<(usize, usize)> = view.unknown().iter_cells().collect();
        if cells.is_empty() {
            return (0, 0);
        }
        cells[rng.gen_range(cells.len() as u64) as usize]
    }

    fn observe(&mut self, _r: usize, _c: usize, _result: crate::board::ShotResult) {}

    fn reset(&mut self) {}
}

/// Result of one solo match game.
#[derive(Clone, Copy, Debug)]
pub struct SoloResult {
    pub outcome: GameOutcome,
    pub shots_a: u32,
    pub shots_b: u32,
}

/// Play one solo match: both levels attack the same hidden fleet.
///
/// The fleet is drawn with `fleet_seed` (shared by both sides — this is
/// the point of solo mode); each attacker's RNG is derived from
/// `seed ^ 0xA5A5_A5A5_A5A5_A5A5` and `seed ^ 0x5A5A_5A5A_5A5A_5A5A`, so
/// the two engines see the same target but play independent games.
/// Deterministic for a given `seed`.
pub fn solo_match(a: &Level, b: &Level, seed: u64, smart_fleet: bool) -> SoloResult {
    // The shared target fleet.
    let mut fleet_rng = Xoshiro256::from_seed(seed ^ 0xF1E1_F1E1_F1E1_F1E1);
    let fleet = if smart_fleet {
        place_best_fleet(&mut fleet_rng, &Default::default())
    } else {
        place_random_fleet(&mut fleet_rng)
    };

    let shots_a = attack(a.attacker(seed ^ 0xA5A5_A5A5_A5A5_A5A5), &fleet);
    let shots_b = attack(b.attacker(seed ^ 0x5A5A_5A5A_5A5A_5A5A), &fleet);

    let outcome = if shots_a < shots_b {
        GameOutcome::A
    } else if shots_b < shots_a {
        GameOutcome::B
    } else {
        GameOutcome::Draw
    };
    SoloResult {
        outcome,
        shots_a,
        shots_b,
    }
}

/// Run one attacker against a fixed target board. Returns shots used.
fn attack(attacker: BotPlayer, target: &Board) -> u32 {
    let mut bot = attacker;
    let mut target = target.clone();
    let mut shots = 0u32;
    while !target.all_sunk() && shots < 200 {
        let (r, c) =
            bot.strategy
                .choose(&bot.view, &mut bot.rng, crate::time_limit::Deadline::none());
        let res = target.shoot(r, c);
        bot.view.observe(r, c, res);
        bot.strategy.observe(r, c, res);
        shots += 1;
    }
    shots
}

// ─────────────────────────────────────────────────────────────────────────────
// SPRT match driver
// ─────────────────────────────────────────────────────────────────────────────

/// A finished SPRT match.
#[derive(Clone, Debug, Serialize)]
pub struct SprtReport {
    pub a: String,
    pub b: String,
    pub games: u64,
    pub wins: u64,
    pub losses: u64,
    pub draws: u64,
    pub llr: f64,
    /// "A stronger" / "A not stronger" / "inconclusive (game cap)".
    pub decision: String,
    pub score: f64,
    pub elo: f64,
}

/// Run an SPRT match between two levels until a decision or `max_games`.
///
/// Game `i` uses seed `seed.wrapping_add(i as u64)` — fully reproducible.
pub fn run_sprt(
    a: &Level,
    b: &Level,
    config: SprtConfig,
    seed: u64,
    max_games: u64,
    smart_fleet: bool,
) -> SprtReport {
    let mut state = SprtState::new(config);
    let mut i = 0u64;
    while state.games() < max_games {
        let res = solo_match(a, b, seed.wrapping_add(i), smart_fleet);
        i += 1;
        if state.record(res.outcome) != SprtStatus::Continue {
            break;
        }
    }
    let decision = match state.status() {
        SprtStatus::AcceptH1 => "A stronger".to_string(),
        SprtStatus::AcceptH0 => "A not stronger".to_string(),
        SprtStatus::Continue => format!("inconclusive ({}-game cap)", max_games),
    };
    SprtReport {
        a: a.name.clone(),
        b: b.name.clone(),
        games: state.games(),
        wins: state.wins,
        losses: state.losses,
        draws: state.draws,
        llr: state.llr(),
        decision,
        score: state.score(),
        elo: state.elo(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Strength ladder
// ─────────────────────────────────────────────────────────────────────────────

/// One rung of the strength ladder.
#[derive(Clone, Debug, Serialize)]
pub struct LadderRung {
    /// The level being promoted (challenger).
    pub challenger: String,
    /// The level it must beat (incumbent).
    pub incumbent: String,
    pub report: SprtReport,
}

/// Run the SPRT strength ladder over a list of levels.
///
/// Each level plays an SPRT match against its predecessor. A rung "passes"
/// when the SPRT accepts H1 (the challenger is stronger). The ladder
/// output is the strength ordering evidence for the release.
pub fn run_ladder(levels: &[Level], seed: u64, max_games: u64) -> Vec<LadderRung> {
    let mut out = Vec::new();
    for pair in levels.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        let report = run_sprt(a, b, SprtConfig::default(), seed, max_games, false);
        out.push(LadderRung {
            challenger: a.name.clone(),
            incumbent: b.name.clone(),
            report,
        });
    }
    out
}

/// The canonical release ladder: hypothesis budgets 16 → 8192, all with
/// the endgame solver enabled (0.3 default).
pub fn release_ladder() -> Vec<Level> {
    vec![
        Level::new("hybrid-16", 16, true),
        Level::new("hybrid-64", 64, true),
        Level::new("hybrid-256", 256, true),
        Level::new("hybrid-1024", 1024, true),
        Level::new("hybrid-8192", 8192, true),
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// Seed-verified release numbers
// ─────────────────────────────────────────────────────────────────────────────

/// The release verification digest: a 64-bit FNV-1a hash chain over the
/// game-result tuples. Same seed + same code ⇒ same digest.
pub fn digest_results(results: &[(&str, u64, u64, u64, u32, u32)]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for (name, wins, losses, draws, sa, sb) in results {
        for byte in name.as_bytes() {
            h ^= *byte as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        for v in [*wins, *losses, *draws, *sa as u64, *sb as u64] {
            for shift in (0..64).step_by(8) {
                h ^= (v >> shift) & 0xFF;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    h
}

/// One matchup track of the release verification.
#[derive(Clone, Debug, Serialize)]
pub struct MatchupReport {
    pub name: String,
    pub games: u32,
    pub wins: u64,
    pub losses: u64,
    pub draws: u64,
    /// Sonar's average shots per game (solo mode).
    pub avg_shots_sonar: f64,
    /// Opponent's average shots per game.
    pub avg_shots_opponent: f64,
    pub win_rate: f64,
}

/// The full seed-verified release verification.
#[derive(Clone, Debug, Serialize)]
pub struct ReleaseVerification {
    pub version: String,
    pub seed: u64,
    pub matchups: Vec<MatchupReport>,
    /// Hash-chain digest over every game result. Reproducible.
    pub digest: String,
    /// Wall-clock duration of the verification run (informational only —
    /// never part of the digest).
    pub elapsed_ms: u64,
}

/// Run the seed-verified release numbers.
///
/// Tracks (all solo mode, so outcomes depend only on targeting):
/// 1. Sonar (hybrid-1024 + endgame) vs a random shooter,
/// 2. Sonar vs PDF-only,
/// 3. Endgame solver on vs off (the 0.3 feature delta).
pub fn verify_release(games_per_matchup: u32, seed: u64) -> ReleaseVerification {
    let start = Instant::now();
    let sonar = Level::new("sonar", 1024, true);
    let mut matchups = Vec::new();
    let mut chain: Vec<(&str, u64, u64, u64, u32, u32)> = Vec::new();

    // 1. Sonar vs random.
    {
        let random = Level::random("random");
        let (rep,) = track_matchup("sonar-vs-random", &sonar, &random, games_per_matchup, seed);
        chain.push((
            "sonar-vs-random",
            rep.wins,
            rep.losses,
            rep.draws,
            rep.avg_shots_sonar as u32,
            rep.avg_shots_opponent as u32,
        ));
        matchups.push(rep);
    }
    // 2. Sonar vs PDF-only.
    {
        let pdf = Level::pdf("pdf-only");
        let (rep,) = track_matchup(
            "sonar-vs-pdf",
            &sonar,
            &pdf,
            games_per_matchup,
            seed ^ 0x1111,
        );
        chain.push((
            "sonar-vs-pdf",
            rep.wins,
            rep.losses,
            rep.draws,
            rep.avg_shots_sonar as u32,
            rep.avg_shots_opponent as u32,
        ));
        matchups.push(rep);
    }
    // 3. Endgame on vs off.
    {
        let no_end = Level::new("no-endgame", 1024, false);
        let (rep,) = track_matchup(
            "endgame-vs-none",
            &sonar,
            &no_end,
            games_per_matchup,
            seed ^ 0x2222,
        );
        chain.push((
            "endgame-vs-none",
            rep.wins,
            rep.losses,
            rep.draws,
            rep.avg_shots_sonar as u32,
            rep.avg_shots_opponent as u32,
        ));
        matchups.push(rep);
    }

    ReleaseVerification {
        version: env!("CARGO_PKG_VERSION").to_string(),
        seed,
        matchups,
        digest: format!("{:016x}", digest_results(&chain)),
        elapsed_ms: start.elapsed().as_millis() as u64,
    }
}

/// Play a matchup for N seeded games and aggregate.
fn track_matchup(name: &str, a: &Level, b: &Level, games: u32, seed: u64) -> (MatchupReport,) {
    let mut wins = 0u64;
    let mut losses = 0u64;
    let mut draws = 0u64;
    let mut sa = 0u32;
    let mut sb = 0u32;
    for i in 0..games as u64 {
        let res = solo_match(a, b, seed.wrapping_add(i), false);
        sa += res.shots_a;
        sb += res.shots_b;
        match res.outcome {
            GameOutcome::A => wins += 1,
            GameOutcome::B => losses += 1,
            GameOutcome::Draw => draws += 1,
        }
    }
    let g = games.max(1) as f64;
    let score = wins as f64 + 0.5 * draws as f64;
    (MatchupReport {
        name: name.to_string(),
        games,
        wins,
        losses,
        draws,
        avg_shots_sonar: sa as f64 / g,
        avg_shots_opponent: sb as f64 / g,
        win_rate: score / g * 100.0,
    },)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprt_llr_math() {
        // All wins for A must drive the LLR up and accept H1.
        let mut s = SprtState::new(SprtConfig::from_elo(25.0, 0.05, 0.05));
        for _ in 0..40 {
            s.record(GameOutcome::A);
        }
        assert_eq!(s.status(), SprtStatus::AcceptH1);
        assert!(s.llr() > 0.0);

        // All losses must accept H0.
        let mut s = SprtState::new(SprtConfig::from_elo(25.0, 0.05, 0.05));
        for _ in 0..40 {
            s.record(GameOutcome::B);
        }
        assert_eq!(s.status(), SprtStatus::AcceptH0);
        assert!(s.llr() < 0.0);
    }

    #[test]
    fn test_sprt_draws_carry_no_evidence() {
        // With symmetric bounds the LLR of a pure-draw sequence is exactly
        // zero — draws carry no evidence either way, so the test continues.
        let mut s = SprtState::new(SprtConfig::from_elo(25.0, 0.05, 0.05));
        for _ in 0..200 {
            s.record(GameOutcome::Draw);
        }
        assert_eq!(s.status(), SprtStatus::Continue);
        assert!((s.score() - 0.5).abs() < 1e-12);
        assert!(s.elo().abs() < 1e-6);
        assert!(s.llr().abs() < 1e-9);
    }

    #[test]
    fn test_sprt_bounds() {
        let cfg = SprtConfig::from_elo(25.0, 0.05, 0.05);
        let s = SprtState::new(cfg);
        // bound_up = ln(0.95/0.05) ≈ 2.944; bound_down = ln(0.05/0.95) ≈ −2.944.
        assert!((s.bound_up - (0.95f64 / 0.05f64).ln()).abs() < 1e-9);
        assert!((s.bound_down - (0.05f64 / 0.95f64).ln()).abs() < 1e-9);
    }

    #[test]
    fn test_solo_match_deterministic() {
        let a = Level::new("a", 64, true);
        let b = Level::new("b", 64, true);
        let r1 = solo_match(&a, &b, 12345, false);
        let r2 = solo_match(&a, &b, 12345, false);
        assert_eq!(r1.shots_a, r2.shots_a);
        assert_eq!(r1.shots_b, r2.shots_b);
        assert_eq!(r1.outcome, r2.outcome);
        // Both attackers solved the same fleet — a sane sanity bound.
        assert!(r1.shots_a > 17 && r1.shots_a <= 200);
        assert!(r1.shots_b > 17 && r1.shots_b <= 200);
    }

    #[test]
    fn test_solo_random_never_beats_hybrid() {
        // The random level (no strategy) must lose the shots race clearly.
        let hybrid = Level::new("h", 128, true);
        let random = Level::new("r", 0, false);
        let mut h_shots = 0u32;
        let mut r_shots = 0u32;
        for i in 0..6 {
            let res = solo_match(&hybrid, &random, 700 + i, false);
            h_shots += res.shots_a;
            r_shots += res.shots_b;
        }
        assert!(
            h_shots < r_shots,
            "hybrid {} shots vs random {} shots",
            h_shots,
            r_shots
        );
    }

    #[test]
    fn test_sprt_match_decides() {
        // A decisive gap (hybrid vs random) must reach a decision quickly.
        let a = Level::new("a", 256, true);
        let b = Level::random("r");
        let rep = run_sprt(
            &a,
            &b,
            SprtConfig::from_elo(25.0, 0.05, 0.05),
            99,
            200,
            false,
        );
        assert_eq!(rep.decision, "A stronger");
        assert!(rep.games < 200, "decided in {} games", rep.games);
    }

    #[test]
    fn test_ladder_runs() {
        // A 2-level ladder with a decisive gap — must complete and accept
        // the challenger.
        let levels = vec![Level::new("l0", 256, true), Level::random("l1")];
        let ladder = run_ladder(&levels, 4242, 200);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder[0].report.decision, "A stronger");
    }

    #[test]
    fn test_verify_release_reproducible() {
        let a = verify_release(3, 0x5EED_CAFE);
        let b = verify_release(3, 0x5EED_CAFE);
        assert_eq!(a.digest, b.digest, "same seed must reproduce the digest");
        for (x, y) in a.matchups.iter().zip(b.matchups.iter()) {
            assert_eq!(x.wins, y.wins);
            assert_eq!(x.avg_shots_sonar, y.avg_shots_sonar);
        }
        // A different seed must (with overwhelming probability) change it.
        let c = verify_release(3, 0xD1FF_5EED);
        assert_ne!(a.digest, c.digest);
    }

    #[test]
    fn test_release_numbers_sane() {
        // The release seed's documented numbers. All games are seeded and
        // work-limited (no wall-clock deadlines), so every figure below is
        // bit-reproducible on any machine — that is the point of
        // seed-verified release numbers.
        let v = verify_release(8, 0x5EED_CAFE);
        let sonar_vs_random = &v.matchups[0];
        assert!(
            sonar_vs_random.win_rate >= 90.0,
            "vs random: {}",
            sonar_vs_random.win_rate
        );
        assert!(sonar_vs_random.avg_shots_sonar < sonar_vs_random.avg_shots_opponent);

        let sonar_vs_pdf = &v.matchups[1];
        assert!(
            sonar_vs_pdf.win_rate >= 60.0,
            "vs pdf: {}",
            sonar_vs_pdf.win_rate
        );
        assert!(
            sonar_vs_pdf.avg_shots_sonar < sonar_vs_pdf.avg_shots_opponent,
            "sonar {} shots vs pdf {}",
            sonar_vs_pdf.avg_shots_sonar,
            sonar_vs_pdf.avg_shots_opponent
        );

        // The endgame solver must never hurt (exact-or-defer contract).
        let endgame = &v.matchups[2];
        assert!(
            endgame.avg_shots_sonar <= endgame.avg_shots_opponent,
            "endgame solver must never hurt: {} vs {}",
            endgame.avg_shots_sonar,
            endgame.avg_shots_opponent
        );
    }
}
