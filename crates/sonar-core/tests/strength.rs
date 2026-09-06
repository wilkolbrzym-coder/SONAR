//! Statistical strength gates — the engine must actually be strong,
//! measured properly.
//!
//! Each gate uses the Wilson score 95% interval (see `benchmark.rs`) so
//! conclusions hold at small sample sizes. Gates are regression alarms:
//! they fail when strength *drops*, catching accidental regressions long
//! before users notice.
//!
//! Configs use `Deadline::none()` — strength here is the *algorithm* under
//! a fixed hypothesis budget, independent of machine speed.

use sonar::benchmark::{BenchmarkConfig, BotKind, run_benchmark};

fn config(games: u32, seed: u64, soft: usize) -> BenchmarkConfig {
    BenchmarkConfig {
        games,
        threads: 4,
        max_hypotheses: soft,
        smart_placement: true,
        seed,
    }
}

#[test]
fn test_sonar_dominates_random() {
    // The flagship gate: Sonar vs uniform-random must be a statistical
    // blowout. Wilson lower bound above 90% over 120 games.
    let cfg = config(120, 0x517A_0445, 64);
    let (s1, _s2, _) = run_benchmark(&cfg, BotKind::Hybrid, BotKind::Random);
    let (lo, hi) = s1.win_rate_wilson_95();
    assert!(
        lo > 0.90,
        "Sonar vs Random: win rate {:.1}% (CI [{:.1}%, {:.1}%]) — lower bound under 90%",
        s1.win_rate(),
        lo * 100.0,
        hi * 100.0
    );
}

#[test]
fn test_pdf_dominates_random() {
    let cfg = config(80, 0x00DF_0004, 0);
    let (s1, _s2, _) = run_benchmark(&cfg, BotKind::Pdf, BotKind::Random);
    let (lo, hi) = s1.win_rate_wilson_95();
    assert!(
        lo > 0.85,
        "PDF vs Random: win rate {:.1}% (CI [{:.1}%, {:.1}%]) — lower bound under 85%",
        s1.win_rate(),
        lo * 100.0,
        hi * 100.0
    );
}

#[test]
fn test_sonar_beats_reference_bots() {
    use sonar::game::Game;
    use sonar::placement::{PlacementConfig, place_best_fleet};
    use sonar::player::{BotPlayer, Player};
    use sonar::reference_bots::{ReferenceKind, make_reference};
    use sonar::time_limit::Deadline;

    // Sonar (hypothesis filter) must beat the published HuntTarget
    // reference clearly. Both sides deterministically seeded.
    let games = 40u32;
    let mut wins = 0u32;
    for i in 0..games {
        let mut our = BotPlayer::new("Sonar", 256, true)
            .without_learning()
            .with_deadline(Deadline::none());
        let mut opp = make_reference(ReferenceKind::HuntTarget, "HuntTarget");
        our.reseed((i as u64) * 31 + 5);
        opp.reseed((i as u64) * 17 + 9);
        our.place_fleet();
        let mut rng_for_opp =
            sonar::rng::Xoshiro256::from_seed((i as u64).wrapping_mul(0xDEAD).wrapping_add(7));
        *opp.board_mut() = place_best_fleet(&mut rng_for_opp, &PlacementConfig::default());

        let mut g = Game::new(Box::new(our), Box::new(opp));
        if g.play(Deadline::none()) == 1 {
            wins += 1;
        }
    }
    let (lo, hi) = sonar::benchmark::PlayerStats {
        wins,
        losses: games - wins,
        ..Default::default()
    }
    .win_rate_wilson_95();
    assert!(
        lo > 0.75,
        "Sonar vs HuntTarget: {}/{} (CI [{:.1}%, {:.1}%]) — too weak",
        wins,
        games,
        lo * 100.0,
        hi * 100.0
    );
}

#[test]
fn test_sonar_outperforms_pdf_component() {
    // With a real hypothesis budget (512) the blended posterior must add
    // measurable strength over PDF alone — otherwise the filter is dead
    // weight. Measured: 62% (CI [52.2, 70.9]) over 100 games.
    // Gate: point estimate ≥ 58% with a Wilson lower bound above 50%.
    let cfg = config(100, 0x4A7D_0DFF, 512);
    let (s1, _s2, _) = run_benchmark(&cfg, BotKind::Hybrid, BotKind::Pdf);
    let (lo, hi) = s1.win_rate_wilson_95();
    assert!(
        s1.win_rate() >= 58.0 && lo > 0.50,
        "Sonar vs PdfOnly: win rate {:.1}% (CI [{:.1}%, {:.1}%]) — the hypothesis filter adds no strength",
        s1.win_rate(),
        lo * 100.0,
        hi * 100.0
    );
}

#[test]
fn test_hybrid_never_worse_than_pdf_at_low_budget() {
    // At a small hypothesis budget the adaptive blend (w = n/(n+K)) must
    // fall back toward the PDF, so the hybrid must never be *worse* than
    // PDF alone. Measured: 54% at soft target 128.
    let cfg = config(100, 0x081E_D000_0000_00DF, 128);
    let (s1, _s2, _) = run_benchmark(&cfg, BotKind::Hybrid, BotKind::Pdf);
    assert!(
        s1.win_rate() >= 50.0,
        "Sonar (blend) vs PdfOnly at low budget: {:.1}% — blend hurts where it should not",
        s1.win_rate()
    );
}

#[test]
fn test_smart_placement_is_not_a_liability() {
    // Side A uses the ε-band smart placement, side B places uniformly at
    // random — identical targeting on both sides. Smart placement must
    // never be a *net negative* (a penalty bug would show up exactly
    // here). Gate: point estimate ≥ 45% over 60 games.
    use sonar::game::Game;
    use sonar::placement::{PlacementConfig, place_best_fleet, place_random_fleet};
    use sonar::player::{BotPlayer, Player};
    use sonar::rng::Xoshiro256;
    use sonar::time_limit::Deadline;

    let games = 60u32;
    let mut smart_wins = 0u32;
    let dl = Deadline::none();
    for i in 0..games {
        let seed = (i as u64).wrapping_mul(6151).wrapping_add(13);
        let mut a = BotPlayer::new("Smart", 64, false)
            .without_learning()
            .with_deadline(Deadline::none());
        let mut b = BotPlayer::new("Random", 64, false)
            .without_learning()
            .with_deadline(Deadline::none());
        a.reseed(seed);
        b.reseed(seed ^ 0xB0A7D);
        let mut rng = Xoshiro256::from_seed(seed ^ 0x91ACE);
        *a.board_mut() = place_best_fleet(&mut rng, &PlacementConfig::default());
        *b.board_mut() = place_random_fleet(&mut rng);
        let mut g = Game::new(Box::new(a), Box::new(b));
        if g.play(dl) == 1 {
            smart_wins += 1;
        }
    }
    let rate = smart_wins as f64 / games as f64 * 100.0;
    assert!(
        rate >= 45.0,
        "smart placement side won only {:.1}% — placement is a liability",
        rate
    );
}

#[test]
fn test_first_mover_advantage_is_small() {
    // With identical smart-placement bots, the first mover's advantage
    // must be modest (a huge bias would indicate a game-loop bug).
    let cfg = config(80, 0xF125_7000, 64);
    let (s1, _s2, _) = run_benchmark(&cfg, BotKind::Hybrid, BotKind::Hybrid);
    // The first mover wins by firing first — some advantage is expected,
    // but it must stay below 70% at this strength.
    assert!(
        s1.win_rate() < 70.0,
        "first-mover win rate {:.1}% is implausibly high — game-loop asymmetry?",
        s1.win_rate()
    );
}
