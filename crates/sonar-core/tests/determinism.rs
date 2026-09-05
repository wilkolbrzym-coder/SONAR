//! Determinism test suite — the same seed must replay the same game,
//! bit for bit.
//!
//! This is the reproducibility contract from `STABILITY.md`: with
//! `Deadline::none()` (work-limited search), a fixed seed produces an
//! identical move sequence on every run, every platform, every ISA.
//! Wall-clock deadlines are explicitly *excluded* from this contract —
//! they trade reproducibility for extra thinking time.

use sonar::board::ShotResult;
use sonar::player::{BotPlayer, Player};
use sonar::placement::{place_best_fleet, place_random_fleet, PlacementConfig};
use sonar::rng::Xoshiro256;
use sonar::time_limit::Deadline;

/// Play one full deterministic game and return the complete move log:
/// every shot of both players plus the winner.
fn play_logged(seed: u64, soft_target: usize, smart: bool) -> (Vec<((usize, usize), u8)>, u8) {
    let mut rng = Xoshiro256::from_seed(seed);
    let mut p1 = BotPlayer::new("P1", soft_target, smart)
        .without_learning()
        .with_deadline(Deadline::none());
    let mut p2 = BotPlayer::new("P2", soft_target, smart)
        .without_learning()
        .with_deadline(Deadline::none());
    p1.reseed(seed ^ 0x1111_1111_1111_1111);
    p2.reseed(seed ^ 0x2222_2222_2222_2222);

    if smart {
        *p1.board_mut() = place_best_fleet(&mut rng, &PlacementConfig::default());
        *p2.board_mut() = place_best_fleet(&mut rng, &PlacementConfig::default());
    } else {
        *p1.board_mut() = place_random_fleet(&mut rng);
        *p2.board_mut() = place_random_fleet(&mut rng);
    }

    let mut log = Vec::new();
    let dl = Deadline::none();
    let mut winner = 0u8;
    for _ in 0..200 {
        // P1 fires.
        let (r, c) = p1.choose_move(dl);
        let res = p2.board_mut().shoot(r, c);
        p1.observe_result(r, c, res);
        log.push(((r, c), result_code(res)));
        if p2.is_defeated() {
            winner = 1;
            break;
        }
        // P2 fires.
        let (r, c) = p2.choose_move(dl);
        let res = p1.board_mut().shoot(r, c);
        p2.observe_result(r, c, res);
        log.push(((r, c), result_code(res)));
        if p1.is_defeated() {
            winner = 2;
            break;
        }
    }
    (log, winner)
}

fn result_code(r: ShotResult) -> u8 {
    match r {
        ShotResult::Miss => 0,
        ShotResult::Hit => 1,
        ShotResult::Sunk(len) => 2 + len,
        ShotResult::AlreadyShot => 200,
        ShotResult::Invalid => 201,
    }
}

#[test]
fn test_same_seed_replays_identical_game() {
    for &seed in &[1u64, 42, 0xDEAD_BEEF, 1_000_003] {
        let (log_a, win_a) = play_logged(seed, 64, true);
        let (log_b, win_b) = play_logged(seed, 64, true);
        assert_eq!(
            win_a, win_b,
            "winner differs between replays of seed {}",
            seed
        );
        assert_eq!(
            log_a, log_b,
            "move logs differ between replays of seed {} ({} vs {} moves)",
            seed,
            log_a.len(),
            log_b.len()
        );
    }
}

#[test]
fn test_different_seeds_usually_differ() {
    // Different seeds must not accidentally produce the identical game
    // (that would indicate a seeding bug).
    let (log_a, win_a) = play_logged(101, 64, true);
    let (log_b, win_b) = play_logged(202, 64, true);
    let same = log_a == log_b && win_a == win_b;
    assert!(!same, "seeds 101 and 202 produced identical games");
}

#[test]
fn test_placement_deterministic_per_seed() {
    let mut rng_a = Xoshiro256::from_seed(777);
    let mut rng_b = Xoshiro256::from_seed(777);
    for _ in 0..10 {
        let a = place_best_fleet(&mut rng_a, &PlacementConfig::default());
        let b = place_best_fleet(&mut rng_b, &PlacementConfig::default());
        assert_eq!(
            a.ships.0, b.ships.0,
            "placement masks diverged for the same seed"
        );
        assert_eq!(
            a.ship_list.len(),
            b.ship_list.len(),
            "ship count diverged for the same seed"
        );
    }
}

#[test]
fn test_reset_equals_fresh_engine() {
    // Adversarial-robustness property: after reset() the engine's *next
    // move* must be the same as a fresh engine's first move (same seed) —
    // no state may leak between games.
    let seed = 0x5EED_0001;
    let soft = 64;

    // No wall-clock deadline anywhere: determinism is defined on the
    // work-limited fast path (see STABILITY.md).
    let mut fresh = BotPlayer::new("A", soft, true)
        .without_learning()
        .with_deadline(Deadline::none());
    fresh.reseed(seed);
    let move_fresh = fresh.choose_move(Deadline::none());

    let mut used = BotPlayer::new("B", soft, true)
        .without_learning()
        .with_deadline(Deadline::none());
    used.reseed(seed);
    // Play a few arbitrary moves to dirty the internal state.
    let mut rng = Xoshiro256::from_seed(seed ^ 0xABCD);
    for i in 0..12 {
        let r = (rng.next_u64() % 10) as usize;
        let c = (rng.next_u64() % 10) as usize;
        let res = if (r + c + i) % 3 == 0 { ShotResult::Hit } else { ShotResult::Miss };
        used.observe_result(r, c, res);
    }
    let _ = used.choose_move(Deadline::none());
    // Reset and compare against the fresh engine.
    used.reset();
    used.reseed(seed);
    let move_used = used.choose_move(Deadline::none());

    assert_eq!(
        move_fresh, move_used,
        "reset() did not restore first-move behaviour (state leaked between games)"
    );
}

#[test]
fn test_benchmark_seed_determinism() {
    // The sequential benchmark must be a pure function of (config, seed).
    let base = sonar::benchmark::BenchmarkConfig {
        games: 8,
        threads: 1,
        max_hypotheses: 48,
        smart_placement: true,
        seed: 0xC0FFEE,
    };
    let a = sonar::benchmark::run_benchmark_seq(
        &base,
        sonar::benchmark::BotKind::Hybrid,
        sonar::benchmark::BotKind::Random,
    );
    let b = sonar::benchmark::run_benchmark_seq(
        &base,
        sonar::benchmark::BotKind::Hybrid,
        sonar::benchmark::BotKind::Random,
    );
    assert_eq!(a.stats1.wins, b.stats1.wins, "wins differ across identical benchmark runs");
    assert_eq!(a.stats1.total_moves, b.stats1.total_moves, "move totals differ");
    assert_eq!(
        a.stats1.moves_sq_sum as u64, b.stats1.moves_sq_sum as u64,
        "move distribution differs"
    );
}
