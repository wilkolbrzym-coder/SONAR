//! Mikrobenchmarki rdzenia - użyj `cargo bench`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use statki::bitboard::BitBoard;
use statki::board::{Board, Ship, ShotResult};
use statki::placement::{best_fleet, fleet_penalty, random_fleet, PlacementConfig};
use statki::rng::Xoshiro256;
use statki::targeting::{EnemyView, PdfTargeting, TargetingStrategy};
use statki::hypothesis::HypothesisFilter;
use statki::player::{BotPlayer, Player};

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
        b.iter(|| {
            black_box(random_fleet(&mut rng).unwrap())
        })
    });

    let cfg = PlacementConfig { candidates: 64, ..Default::default() };
    c.bench_function("best_fleet_64", |b| {
        b.iter(|| {
            black_box(best_fleet(&mut rng, &cfg))
        })
    });

    let cfg2 = PlacementConfig { candidates: 1024, ..Default::default() };
    c.bench_function("best_fleet_1024", |b| {
        b.iter(|| {
            black_box(best_fleet(&mut rng, &cfg2))
        })
    });
}

fn bench_targeting(c: &mut Criterion) {
    let mut rng = Xoshiro256::from_seed(42);
    let pdf = PdfTargeting::new(statki::targeting::PdfConfig::default());

    c.bench_function("pdf_density_initial", |b| {
        let v = EnemyView::new();
        b.iter(|| black_box(pdf.compute_density(black_box(&v))))
    });

    c.bench_function("pdf_density_with_10_shots", |b| {
        let mut v = EnemyView::new();
        for i in 0..10 {
            v.observe(i, i, ShotResult::Miss);
        }
        b.iter(|| black_box(pdf.compute_density(black_box(&v))))
    });

    c.bench_function("pdf_choose_move_initial", |b| {
        let mut pdf = PdfTargeting::new(statki::targeting::PdfConfig::default());
        let mut v = EnemyView::new();
        b.iter(|| black_box(pdf.choose(black_box(&v), &mut rng)))
    });

    let mut hf = HypothesisFilter::new(256);
    c.bench_function("hypothesis_regen_256", |b| {
        let v = EnemyView::new();
        b.iter(|| black_box(hf.regenerate(black_box(&v), &mut rng)))
    });
}

fn bench_full_game(c: &mut Criterion) {
    let mut rng = Xoshiro256::from_seed(42);
    c.bench_function("full_game_pdf_vs_random", |b| {
        b.iter(|| {
            let mut p1 = BotPlayer::new("P1", 256, true);
            let mut p2 = statki::player::RandomBot::new("P2");
            p1.board = statki::placement::place_best_fleet(&mut rng, &PlacementConfig::default());
            p2.board = statki::placement::place_random_fleet(&mut rng);
            let mut g = statki::game::Game::new(Box::new(p1), Box::new(p2));
            black_box(g.play())
        })
    });
}

criterion_group!(benches, bench_bitboard_ops, bench_placement, bench_targeting, bench_full_game);
criterion_main!(benches);
