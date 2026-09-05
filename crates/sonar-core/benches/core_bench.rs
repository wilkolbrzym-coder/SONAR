//! Core micro-benchmarks — run with `cargo bench`.
//!
//! Groups:
//!  - bitboard: raw bit operations on the 128-bit board masks
//!  - placement: random and penalty-minimising fleet generation
//!  - targeting: PDF density computation and move selection at several
//!    game stages (empty board, 10 misses, mid-game)
//!  - hypotheses: Bayesian filter regeneration at several soft targets
//!  - full_game: end-to-end self-play games (fast, count-limited)
//!
//! All benchmarks run with `Deadline::none()` — i.e. work-limited by the
//! hypothesis soft target — so measurements reflect the algorithm, not a
//! wall-clock race.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sonar::bitboard::BitBoard;
use sonar::board::{Board, ShotResult};
use sonar::hypothesis::{HybridTargeting, HypothesisFilter};
use sonar::placement::{best_fleet, fleet_penalty, random_fleet, PlacementConfig};
use sonar::player::{BotPlayer, Player};
use sonar::rng::Xoshiro256;
use sonar::targeting::{EnemyView, PdfConfig, PdfTargeting, TargetingStrategy};
use sonar::time_limit::Deadline;

fn bench_bitboard_ops(c: &mut Criterion) {
    c.bench_function("bitboard_set_test_100", |b| {
        b.iter(|| {
            let mut bb = BitBoard::new();
            for r in 0..10 {
                for c in 0..10 {
                    bb.set(r, c);
                }
            }
            for r in 0..10 {
                for c in 0..10 {
                    black_box(bb.test(r, c));
                }
            }
        })
    });

    c.bench_function("bitboard_popcount_100bits", |b| {
        let bb = BitBoard::FULL;
        b.iter(|| black_box(bb.popcount()))
    });

    c.bench_function("bitboard_iter_50bits", |b| {
        let mut bb = BitBoard::new();
        for i in 0..50 {
            bb.set(i / 10, i % 10);
        }
        b.iter(|| {
            let mut sum = 0u32;
            for (r, c) in bb.iter_cells() {
                sum += (r + c) as u32;
            }
            black_box(sum)
        })
    });

    c.bench_function("bitboard_dilate_50bits", |b| {
        let mut bb = BitBoard::new();
        for i in 0..50 {
            bb.set(i / 10, i % 10);
        }
        b.iter(|| black_box(bb.dilate8()))
    });
}

fn bench_placement(c: &mut Criterion) {
    let mut rng = Xoshiro256::from_seed(42);
    c.bench_function("random_fleet", |b| {
        b.iter(|| black_box(random_fleet(&mut rng)))
    });

    let cfg = PlacementConfig { candidates: 64, ..Default::default() };
    c.bench_function("best_fleet_64_candidates", |b| {
        b.iter(|| black_box(best_fleet(&mut rng, &cfg)))
    });

    let cfg2 = PlacementConfig { candidates: 1024, ..Default::default() };
    c.bench_function("best_fleet_1024_candidates", |b| {
        b.iter(|| black_box(best_fleet(&mut rng, &cfg2)))
    });

    let fleet = random_fleet(&mut rng).unwrap_or_else(|| {
        // Deterministic fallback so the penalty benchmark always has data.
        use sonar::board::Ship;
        use sonar::placement::FleetConfig;
        let ships = [
            Ship::new(0, 0, 5, true),
            Ship::new(2, 0, 4, true),
            Ship::new(4, 0, 3, true),
            Ship::new(6, 0, 3, true),
            Ship::new(8, 0, 2, true),
        ]
        .map(|s| s.unwrap_or(Ship { r: 0, c: 0, len: 1, horizontal: true, mask: 1, sunk: false }));
        FleetConfig::from_ships(ships)
    });
    c.bench_function("fleet_penalty", |b| {
        b.iter(|| black_box(fleet_penalty(black_box(&fleet), &cfg)))
    });
}

fn bench_targeting(c: &mut Criterion) {
    let mut rng = Xoshiro256::from_seed(42);
    let pdf = PdfTargeting::new(PdfConfig::default());

    c.bench_function("pdf_density_initial", |b| {
        let v = EnemyView::new();
        b.iter(|| black_box(pdf.compute_density(black_box(&v))))
    });

    c.bench_function("pdf_density_with_10_misses", |b| {
        let mut v = EnemyView::new();
        for i in 0..10 {
            v.observe(i, i, ShotResult::Miss);
        }
        b.iter(|| black_box(pdf.compute_density(black_box(&v))))
    });

    c.bench_function("pdf_density_midgame", |b| {
        // A realistic mid-game view: hits, sinks, and scattered misses.
        let mut v = EnemyView::new();
        v.observe(4, 4, ShotResult::Hit);
        v.observe(4, 5, ShotResult::Hit);
        v.observe(4, 6, ShotResult::Sunk(3));
        v.observe(0, 0, ShotResult::Miss);
        v.observe(9, 9, ShotResult::Miss);
        v.observe(1, 7, ShotResult::Miss);
        v.observe(7, 2, ShotResult::Miss);
        b.iter(|| black_box(pdf.compute_density(black_box(&v))))
    });

    c.bench_function("pdf_choose_move_initial", |b| {
        let mut pdf = PdfTargeting::new(PdfConfig::default());
        let v = EnemyView::new();
        b.iter(|| black_box(pdf.choose(black_box(&v), &mut rng, Deadline::none())))
    });
}

fn bench_hypotheses(c: &mut Criterion) {
    let mut rng = Xoshiro256::from_seed(42);

    let mut hf = HypothesisFilter::new(256);
    c.bench_function("hypothesis_regen_256_initial", |b| {
        let v = EnemyView::new();
        b.iter(|| black_box(hf.regenerate(black_box(&v), &mut rng, Deadline::none())))
    });

    let mut hf_mid = HypothesisFilter::new(256);
    c.bench_function("hypothesis_regen_256_midgame", |b| {
        let mut v = EnemyView::new();
        v.observe(4, 4, ShotResult::Hit);
        v.observe(4, 5, ShotResult::Hit);
        v.observe(4, 6, ShotResult::Sunk(3));
        for i in 0..20 {
            v.observe((i * 3) % 10, (i * 7) % 10, ShotResult::Miss);
        }
        b.iter(|| black_box(hf_mid.regenerate(black_box(&v), &mut rng, Deadline::none())))
    });

    let mut ht = HybridTargeting::new().with_soft_target(64);
    c.bench_function("hybrid_choose_move_64", |b| {
        let mut v = EnemyView::new();
        b.iter(|| {
            v.observe(5, 5, ShotResult::Miss);
            black_box(ht.choose(black_box(&v), &mut rng, Deadline::none()))
        })
    });
}

fn bench_full_game(c: &mut Criterion) {
    let mut rng = Xoshiro256::from_seed(42);
    c.bench_function("full_game_hybrid_vs_random", |b| {
        b.iter(|| {
            let mut p1 = BotPlayer::new("P1", 256, true).without_learning();
            let mut p2 = sonar::player::RandomBot::new("P2");
            p1.board = sonar::placement::place_best_fleet(&mut rng, &PlacementConfig::default());
            p2.board = sonar::placement::place_random_fleet(&mut rng);
            let mut g = sonar::game::Game::new(Box::new(p1), Box::new(p2));
            black_box(g.play(Deadline::none()))
        })
    });

    c.bench_function("full_game_hybrid_vs_hybrid", |b| {
        b.iter(|| {
            let mut p1 = BotPlayer::new("P1", 256, true).without_learning();
            let mut p2 = BotPlayer::new("P2", 256, true).without_learning();
            p1.place_fleet();
            p2.place_fleet();
            let mut g = sonar::game::Game::new(Box::new(p1), Box::new(p2));
            black_box(g.play(Deadline::none()))
        })
    });
}

criterion_group!(
    benches,
    bench_bitboard_ops,
    bench_placement,
    bench_targeting,
    bench_hypotheses,
    bench_full_game
);
criterion_main!(benches);
