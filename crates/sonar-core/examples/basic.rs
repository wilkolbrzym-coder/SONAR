//! Minimal but complete: let Sonar hunt a fleet that *we* referee.
//!
//! This exact file ships as `crates/sonar-core/examples/basic.rs` and is
//! compiled on every CI run — the documentation never shows unverified code.
//!
//! Run it locally with:
//!
//! ```text
//! cargo run --release -p sonar-core --example basic
//! ```

use sonar::time_limit::Deadline;
use sonar::{Engine, EngineConfig, ShotResult};

/// One ship of the fleet Sonar is hunting — hidden from the engine.
#[derive(Clone, Copy)]
struct Ship {
    r: usize,
    c: usize,
    len: u8,
    horizontal: bool,
    hits: u8,
}

impl Ship {
    fn occupies(&self, r: usize, c: usize) -> bool {
        if self.horizontal {
            r == self.r && c >= self.c && c < self.c + self.len as usize
        } else {
            c == self.c && r >= self.r && r < self.r + self.len as usize
        }
    }

    fn sunk(&self) -> bool {
        self.hits == self.len
    }
}

/// The hidden fleet for this demo (the classic 5-4-3-3-2 layout).
fn demo_fleet() -> Vec<Ship> {
    [
        (0, 0, 5, true),
        (2, 0, 4, true),
        (4, 0, 3, true),
        (6, 0, 3, true),
        (8, 0, 2, true),
    ]
    .into_iter()
    .map(|(r, c, len, horizontal)| Ship {
        r,
        c,
        len,
        horizontal,
        hits: 0,
    })
    .collect()
}

fn main() {
    // 1. Configure: 512 hypotheses, no wall-clock limit, no learning.
    //    Work-limited search is deterministic — the same seed replays the
    //    same game bit-for-bit on every machine.
    let config = EngineConfig {
        hypothesis_soft_target: 512,
        default_deadline_secs: 0,
        use_learning: false,
        ..EngineConfig::default()
    };
    let mut engine = Engine::new(config);
    engine.reseed(0x5EED_0000_0000_0001);
    engine.place_fleet_smart(); // the engine's own fleet (defensive side)

    // 2. The hunt loop: Sonar picks a cell, we referee the shot against
    //    OUR hidden fleet and report the result back.
    let mut fleet = demo_fleet();
    let mut moves = 0usize;
    while fleet.iter().any(|s| !s.sunk()) {
        let (r, c) = engine.choose_move(Deadline::none());
        moves += 1;

        let result = referee(&mut fleet, r, c);
        engine.observe_result(r, c, result);

        let label = match result {
            ShotResult::Hit => "hit".to_string(),
            ShotResult::Sunk(len) => format!("sunk(len={len})"),
            _ => "miss".to_string(),
        };
        println!("move {:>2}: ({:>2},{:>2}) -> {}", moves, r, c, label);
    }

    // 3. Inspect the engine's final reasoning (fully observable).
    let posterior = engine.probability_matrix();
    let top = posterior.iter().cloned().fold(0.0_f32, f32::max);
    println!();
    println!("Sonar sank the fleet in {} moves.", moves);
    println!("hypotheses maintained: {}", engine.hypothesis_count());
    println!("top posterior mass: {:.1}%", top * 100.0);
}

/// Referee one shot against the hidden fleet, updating per-ship hits.
fn referee(fleet: &mut [Ship], r: usize, c: usize) -> ShotResult {
    for ship in fleet.iter_mut() {
        if ship.occupies(r, c) {
            ship.hits += 1;
            return if ship.sunk() {
                ShotResult::Sunk(ship.len)
            } else {
                ShotResult::Hit
            };
        }
    }
    ShotResult::Miss
}
