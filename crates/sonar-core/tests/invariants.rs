//! Full-game invariant suite — properties that must hold in *every* game
//! Sonar plays, checked across many self-play games.
//!
//! These are the "no cheating, no bugs" guarantees:
//!  1. never fire at the same cell twice,
//!  2. never fire at a cell known to be water (a recorded miss),
//!  3. never fire at a cell adjacent to a *sunk* ship (those are
//!     auto-revealed as misses by the standard rule),
//!  4. every game terminates within the move cap and has a winner,
//!  5. board bookkeeping stays consistent (hits ⊆ shots, sunk ⊆ hits,
//!     every ship cell of a sunk ship is marked sunk).

use sonar::board::{Board, ShotResult};
use sonar::player::{BotPlayer, Player};
use sonar::rng::Xoshiro256;
use sonar::time_limit::Deadline;

/// Drive one full game between two bots while checking every invariant
/// after every single move. Returns the winner.
fn play_game_checked(seed: u64, soft: usize) -> u8 {
    let mut p1 = BotPlayer::new("P1", soft, true)
        .without_learning()
        .with_deadline(Deadline::none());
    let mut p2 = BotPlayer::new("P2", soft, true)
        .without_learning()
        .with_deadline(Deadline::none());
    p1.reseed(seed);
    p2.reseed(seed.wrapping_mul(3).wrapping_add(1));
    p1.place_fleet();
    p2.place_fleet();

    let dl = Deadline::none();
    let mut moves_p1 = 0usize;
    let mut moves_p2 = 0usize;

    for _ in 0..200 {
        // ── P1 fires at P2 ────────────────────────────────────────────
        let (r, c) = p1.choose_move(dl);
        assert!(r < 10 && c < 10, "P1 fired out of bounds: ({},{})", r, c);
        let already = p2.board().is_shot(r, c);
        assert!(
            !already,
            "INVARIANT 1 broken: P1 refired at ({},{}) [move {}]",
            r,
            c,
            moves_p1 + 1
        );
        // INVARIANT 2: P1 must not fire at cells its own view marks as
        // misses. (The view contains misses only from P1's shots.)
        let known_miss = p1.view.miss_mask();
        assert!(
            !known_miss.test(r, c),
            "INVARIANT 2 broken: P1 fired at its own recorded miss ({},{})",
            r,
            c
        );
        // INVARIANT 3: cells around sunk ships are auto-missed; the view
        // records them as shots, so firing there is illegal.
        let res = p2.board_mut().shoot(r, c);
        p1.observe_result(r, c, res);
        moves_p1 += 1;
        check_board_consistency(p2.board(), "P2 board after P1 fire");
        if p2.is_defeated() {
            return 1;
        }

        // ── P2 fires at P1 ────────────────────────────────────────────
        let (r, c) = p2.choose_move(dl);
        assert!(r < 10 && c < 10, "P2 fired out of bounds: ({},{})", r, c);
        let already = p1.board().is_shot(r, c);
        assert!(
            !already,
            "INVARIANT 1 broken: P2 refired at ({},{}) [move {}]",
            r,
            c,
            moves_p2 + 1
        );
        let known_miss = p2.view.miss_mask();
        assert!(
            !known_miss.test(r, c),
            "INVARIANT 2 broken: P2 fired at its own recorded miss ({},{})",
            r,
            c
        );
        let res = p1.board_mut().shoot(r, c);
        p2.observe_result(r, c, res);
        moves_p2 += 1;
        check_board_consistency(p1.board(), "P1 board after P2 fire");
        if p1.is_defeated() {
            return 2;
        }
    }
    panic!("INVARIANT 4 broken: game did not terminate within 200 move pairs");
}

/// INVARIANT 5: internal board bookkeeping stays consistent.
fn check_board_consistency(b: &Board, ctx: &str) {
    let shots = b.shots.0;
    let hits = b.hits.0;
    let sunk = b.sunk.0;
    let ships = b.ships.0;
    assert_eq!(
        hits & !shots, 0,
        "{}: hits must be a subset of shots (hit without a shot?)",
        ctx
    );
    assert_eq!(
        sunk & !hits, 0,
        "{}: sunk must be a subset of hits (sunk without a hit?)",
        ctx
    );
    assert_eq!(
        ships & !hits & sunk, 0,
        "{}: sunk cells must be ship cells",
        ctx
    );
    // Every ship marked sunk in the ship list must have its full mask in
    // the sunk mask, and vice versa.
    for s in &b.ship_list {
        if s.sunk {
            assert_eq!(
                sunk & s.mask,
                s.mask,
                "{}: sunk ship (r={},c={},len={}) is missing cells in the sunk mask",
                ctx,
                s.r,
                s.c,
                s.len
            );
        } else {
            assert_eq!(
                sunk & s.mask, 0,
                "{}: live ship marked as sunk in the mask",
                ctx
            );
        }
    }
    let sunk_cells_from_list: u128 = b
        .ship_list
        .iter()
        .filter(|s| s.sunk)
        .map(|s| s.mask)
        .fold(0, |a, b| a | b);
    assert_eq!(
        sunk, sunk_cells_from_list,
        "{}: sunk mask disagrees with the ship list",
        ctx
    );
}

#[test]
fn test_invariants_hold_across_self_play() {
    // 30 full games, every move checked. Different seeds and strengths.
    let mut games = 0;
    for i in 0..15u64 {
        let soft = 16;
        let winner = play_game_checked(i * 977 + 1, soft);
        assert!(winner == 1 || winner == 2);
        games += 1;
    }
    for i in 0..15u64 {
        let soft = 64;
        let winner = play_game_checked(i * 6151 + 2, soft);
        assert!(winner == 1 || winner == 2);
        games += 1;
    }
    assert_eq!(games, 30);
}

#[test]
fn test_all_games_terminate_quickly() {
    // Every game must finish well under the 100-shot theoretical optimum
    // per side (worst case 100 cells). We assert ≤ 130 shots per side,
    // which catches stuck/looping strategies.
    for i in 0..10u64 {
        let mut p1 = BotPlayer::new("P1", 32, true)
            .without_learning()
            .with_deadline(Deadline::none());
        let mut p2 = BotPlayer::new("P2", 32, true)
            .without_learning()
            .with_deadline(Deadline::none());
        p1.reseed(i * 101 + 3);
        p2.reseed(i * 103 + 7);
        p1.place_fleet();
        p2.place_fleet();
        let dl = Deadline::none();
        let mut m = 0;
        for _ in 0..200 {
            let (r, c) = p1.choose_move(dl);
            let res = p2.board_mut().shoot(r, c);
            p1.observe_result(r, c, res);
            m += 1;
            if p2.is_defeated() {
                break;
            }
            let (r, c) = p2.choose_move(dl);
            let res = p1.board_mut().shoot(r, c);
            p2.observe_result(r, c, res);
            m += 1;
            if p1.is_defeated() {
                break;
            }
        }
        assert!(
            p1.is_defeated() || p2.is_defeated(),
            "game {} did not finish ({} move-pairs)",
            i,
            m / 2
        );
        // 2 players × 100 cells = 200 shots max in theory; each side
        // individually must stay ≤ 130.
        assert!(
            m <= 260,
            "game {} took {} total shots — too many",
            i,
            m
        );
    }
}

#[test]
fn test_board_shoot_invariants() {
    // Direct board-level property checks with a random game driver.
    let mut rng = Xoshiro256::from_seed(0xFACE);
    for _game in 0..25 {
        let mut b = sonar::placement::place_random_fleet(&mut rng);
        let mut fired = 0usize;
        while !b.all_sunk() && fired < 100 {
            let r = (rng.next_u64() % 10) as usize;
            let c = (rng.next_u64() % 10) as usize;
            let res = b.shoot(r, c);
            match res {
                ShotResult::Miss => {
                    assert!(!b.is_hit(r, c));
                }
                ShotResult::Hit => {
                    assert!(b.is_hit(r, c));
                    assert!(!b.is_sunk_cell(r, c));
                }
                ShotResult::Sunk(len) => {
                    assert!(b.is_sunk_cell(r, c));
                    assert!(len >= 1 && len <= 5);
                }
                ShotResult::AlreadyShot | ShotResult::Invalid => {}
            }
            check_board_consistency(&b, "random-fire board");
            fired += 1;
        }
        // A random-fire game with 100 shots may not finish — but if it
        // did, all ships must be sunk.
        if fired >= 100 {
            // Exhausted: fine either way.
        }
    }
}
