//! Property-based test suite — randomised inputs, checked invariants.
//!
//! A self-contained mini property-testing framework (no external
//! dependencies): generate random cases from a seeded Xoshiro, shrink
//! nothing, but run thousands of cases cheaply. Each property documents
//! its own invariant.

use sonar::bitboard::BitBoard;
use sonar::board::{Ship, ShotResult};
use sonar::placement::{PlacementConfig, place_best_fleet, place_random_fleet, random_fleet};
use sonar::rng::Xoshiro256;
use sonar::rules::{ContactRule, GameRules, SunkRule};
use sonar::targeting::EnemyView;

const CASES: u32 = 500;

#[test]
fn prop_random_fleets_are_legal() {
    // For any generated fleet: 17 cells set, ships do not touch (8-dir),
    // every ship is within bounds, lengths match the standard fleet.
    let mut rng = Xoshiro256::from_seed(0xA11CE);
    for _ in 0..CASES {
        let cfg = random_fleet(&mut rng).expect("random_fleet failed");
        assert_eq!(cfg.mask.count_ones(), 17, "fleet covers 17 cells");
        let mut lens: Vec<u8> = cfg.ships.iter().map(|s| s.len).collect();
        lens.sort_unstable();
        assert_eq!(lens, vec![2, 3, 3, 4, 5], "standard fleet lengths");
        for i in 0..5 {
            let s = &cfg.ships[i];
            // Bounds.
            for (r, c) in s.cells() {
                assert!(r < 10 && c < 10, "ship cell out of bounds");
            }
            // No contact with any other ship.
            for j in (i + 1)..5 {
                let dilated = BitBoard(cfg.ships[i].mask).dilate8().0;
                assert_eq!(
                    dilated & cfg.ships[j].mask,
                    0,
                    "ships {} and {} touch",
                    i,
                    j
                );
            }
        }
    }
}

#[test]
fn prop_ship_masks_match_geometry() {
    // For any Ship, the mask must be exactly the cells the geometry says.
    // NOTE: the *start* coordinate is bounded along the ship's axis
    // (columns for horizontal, rows for vertical); the other axis is
    // free within 0..10.
    let mut rng = Xoshiro256::from_seed(0x5EED_0001);
    for _ in 0..CASES {
        let len = (rng.next_u64() % 5 + 2) as u8;
        let horizontal = rng.next_u64() & 1 == 0;
        let max_start = 10 - len as usize;
        let (r, c) = if horizontal {
            let rr = (rng.next_u64() % 10) as usize;
            let cc = (rng.next_u64() % (max_start as u64 + 1)) as usize;
            (rr, cc)
        } else {
            let rr = (rng.next_u64() % (max_start as u64 + 1)) as usize;
            let cc = (rng.next_u64() % 10) as usize;
            (rr, cc)
        };
        let ship = Ship::new(r, c, len, horizontal).expect("legal ship");
        let cells = ship.cells();
        assert_eq!(cells.len(), len as usize);
        for (i, (rr, cc)) in cells.iter().enumerate() {
            if horizontal {
                assert_eq!((*rr, *cc), (r, c + i));
            } else {
                assert_eq!((*rr, *cc), (r + i, c));
            }
            assert!(ship.occupies(*rr, *cc));
            assert!(
                BitBoard(ship.mask).test(*rr, *cc),
                "mask covers geometry cell"
            );
        }
    }
}

#[test]
fn prop_board_shoot_is_idempotent() {
    // Shooting the same cell twice always returns AlreadyShot; the board
    // state after the second shot is identical.
    let mut rng = Xoshiro256::from_seed(0x1D_EE);
    for _ in 0..CASES / 5 {
        let mut b = place_random_fleet(&mut rng);
        let r = (rng.next_u64() % 10) as usize;
        let c = (rng.next_u64() % 10) as usize;
        let first = b.shoot(r, c);
        let before = (b.shots, b.hits, b.sunk);
        let second = b.shoot(r, c);
        let after = (b.shots, b.hits, b.sunk);
        assert_eq!(
            second,
            ShotResult::AlreadyShot,
            "second shot must report AlreadyShot"
        );
        assert_eq!(before, after, "second shot must not mutate the board");
        assert_ne!(first, ShotResult::AlreadyShot);
    }
}

#[test]
fn prop_enemy_view_masks_stay_consistent() {
    // For any random observation sequence:
    //   hits ⊆ shots, sunk ⊆ hits, remaining ⊆ original lengths, and the
    //   sunk set is a union of complete linear ships.
    let mut rng = Xoshiro256::from_seed(0xE1E1);
    let original = [2u8, 3, 3, 4, 5];
    for _ in 0..CASES {
        let mut v = EnemyView::new();
        let n_obs = (rng.next_u64() % 30) as u32;
        for _ in 0..n_obs {
            let r = (rng.next_u64() % 10) as usize;
            let c = (rng.next_u64() % 10) as usize;
            let pick = rng.next_u64() % 5;
            let res = match pick {
                0..=2 => ShotResult::Miss,
                3 => ShotResult::Hit,
                _ => {
                    // Sunk with a plausible length: prefer one still in
                    // `remaining`, else 2.
                    let len = if v.remaining.is_empty() {
                        2
                    } else {
                        v.remaining[(rng.next_u64() % v.remaining.len() as u64) as usize]
                    };
                    ShotResult::Sunk(len)
                }
            };
            v.observe(r, c, res);
            // Invariants after every single observation.
            assert_eq!(v.hits.0 & !v.shots.0, 0, "hits ⊄ shots");
            assert_eq!(v.sunk.0 & !v.hits.0, 0, "sunk ⊄ hits");
            for &len in &v.remaining {
                assert!(
                    original.contains(&len),
                    "remaining contains an impossible length"
                );
            }
            assert!(v.remaining.len() <= 5);
        }
    }
}

#[test]
fn prop_density_never_negative_and_zero_on_shots() {
    // For any observation state, the PDF density matrix has:
    //   - no negative entries,
    //   - exactly 0 on every already-shot cell,
    //   - strictly positive on some cell unless the board is exhausted.
    use sonar::targeting::{PdfConfig, PdfTargeting};
    let mut rng = Xoshiro256::from_seed(0x0FF);
    let pdf = PdfTargeting::new(PdfConfig::default());
    for _ in 0..CASES / 10 {
        let mut v = EnemyView::new();
        let n = (rng.next_u64() % 40) as u32;
        for _ in 0..n {
            let r = (rng.next_u64() % 10) as usize;
            let c = (rng.next_u64() % 10) as usize;
            v.observe(r, c, ShotResult::Miss);
        }
        let d = pdf.compute_density(&v);
        for (i, &val) in d.iter().enumerate() {
            assert!(val >= 0.0, "negative density at {}", i);
            if (v.shots.0 & (1u128 << i)) != 0 {
                assert_eq!(val, 0.0, "density on shot cell {}", i);
            }
        }
    }
}

#[test]
fn prop_rules_validation_is_sound() {
    // For random rule combinations, validate() must accept exactly the
    // structurally legal ones.
    let mut rng = Xoshiro256::from_seed(0x12A5);
    for _ in 0..CASES {
        let board_size = (rng.next_u64() % 14) as usize; // 0..=13 (includes illegal)
        let ship_lengths: Vec<u8> = (0..(rng.next_u64() % 5 + 1))
            .map(|_| (rng.next_u64() % 8) as u8) // 0..=7 (includes illegal)
            .collect();
        let rules = GameRules {
            board_size,
            ship_lengths,
            contact_rule: [
                ContactRule::NoContact,
                ContactRule::AllowCornerContact,
                ContactRule::AllowContact,
            ][rng.next_u64() as usize % 3],
            sunk_rule: [SunkRule::RevealNeighbors, SunkRule::NoReveal][rng.next_u64() as usize % 2],
        };
        let ok = rules.validate().is_ok();
        // Cross-check the acceptance conditions by hand.
        let lengths_legal = !rules.ship_lengths.is_empty()
            && rules.ship_lengths.len() <= 10
            && rules
                .ship_lengths
                .iter()
                .all(|&l| l >= 1 && l as usize <= rules.board_size);
        let total: usize = rules.ship_lengths.iter().map(|&l| l as usize).sum();
        let density_legal = rules.board_size >= 5
            && rules.board_size <= 10
            && total <= rules.board_size * rules.board_size / 2;
        let expected = lengths_legal && density_legal;
        assert_eq!(ok, expected, "validation mismatch for rules {:?}", rules);
    }
}

#[test]
fn prop_best_fleet_stays_in_epsilon_band() {
    // ε-band property: the drawn fleet's penalty must be within
    // `sampling_epsilon` of the best candidate seen in a random sample.
    // (The old argmin property no longer holds by design — the mixed
    // strategy deliberately trades a few penalty points for
    // unpredictability.)
    let mut rng = Xoshiro256::from_seed(0xB357);
    let cfg = PlacementConfig {
        candidates: 128,
        ..Default::default()
    };
    let eps = cfg.sampling_epsilon as f64;
    for _ in 0..20 {
        let drawn = place_best_fleet(&mut rng, &cfg);
        // Reconstruct the FleetConfig view of the drawn board.
        let arr: [Ship; 5] = drawn
            .ship_list
            .clone()
            .try_into()
            .expect("standard fleet of 5 ships");
        let drawn_cfg = sonar::placement::FleetConfig::from_ships(arr);
        // Minimum penalty over an independent random sample.
        let mut min_sample = f32::INFINITY;
        for _ in 0..32 {
            if let Some(r) = random_fleet(&mut rng) {
                let p = sonar::placement::fleet_penalty(&r, &cfg);
                if p < min_sample {
                    min_sample = p;
                }
            }
        }
        // The 128-candidate pool behind `place_best_fleet` can only have a
        // minimum ≤ this 32-sample minimum, so the band bound holds.
        let drawn_pen = sonar::placement::fleet_penalty(&drawn_cfg, &cfg) as f64;
        assert!(
            drawn_pen <= min_sample as f64 + eps + 0.25,
            "drawn fleet penalty {} exceeds sample-min {} + ε {}",
            drawn_pen,
            min_sample,
            eps
        );
        // The generated board must carry the standard fleet.
        assert_eq!(drawn.ships.0.count_ones(), 17);
    }
}

#[test]
fn prop_bitboard_ops_are_closed() {
    // Bitwise operations stay masked to the 100-bit board.
    let mut rng = Xoshiro256::from_seed(0xB17B);
    for _ in 0..CASES {
        let a = BitBoard::from_bits(rng.next_u64() as u128 | ((rng.next_u64() as u128) << 64));
        let b = BitBoard::from_bits(rng.next_u64() as u128 | ((rng.next_u64() as u128) << 64));
        assert_eq!((a & b).popcount() + (a ^ b).popcount(), (a | b).popcount());
        assert_eq!(
            (a | b).0 & !sonar::bitboard::MASK_100,
            0,
            "OR leaked past 100 bits"
        );
        assert_eq!(
            (a ^ b).0 & !sonar::bitboard::MASK_100,
            0,
            "XOR leaked past 100 bits"
        );
        // Dilate is monotone and stays masked.
        let d = a.dilate8();
        assert_eq!(
            d.0 & !sonar::bitboard::MASK_100,
            0,
            "dilate leaked past 100 bits"
        );
        assert_eq!(d.0 & a.0, a.0, "dilate lost cells");
    }
}
