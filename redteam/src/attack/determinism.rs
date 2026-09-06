//! Attack 3: determinism probe — same seed, same observation history ⇒
//! bit-identical decisions. The engine's adversarial-robustness contract
//! forbids any wall-clock, machine-load, or ordering side-channel from
//! leaking into *work-limited* decisions.

use sonar::api::{Engine, EngineConfig};
use sonar::rng::Xoshiro256;
use sonar::time_limit::Deadline;

pub struct DeterminismReport {
    pub seeds: u32,
    pub mismatches: u32,
}

/// The full move list of one seeded game (attacker side only).
fn replay(seed: u64) -> Vec<(usize, usize, sonar::board::ShotResult)> {
    let mut engine = Engine::new(EngineConfig {
        use_learning: false,
        hypothesis_soft_target: 128,
        default_deadline_secs: 0,
        ..Default::default()
    });
    engine.reseed(seed);

    // Deterministic defender fleet.
    let mut rng = Xoshiro256::from_seed(seed ^ 0xD1CE);
    let board = sonar::placement::place_random_fleet(&mut rng);
    let mut target = board.clone();

    let mut trace = Vec::new();
    let mut shots = 0;
    while !target.all_sunk() && shots < 200 {
        let (r, c) = engine.choose_move(Deadline::none());
        let res = target.shoot(r, c);
        engine.observe_result(r, c, res);
        trace.push((r, c, res));
        shots += 1;
    }
    trace
}

pub fn run(quick: bool) -> DeterminismReport {
    let seeds = if quick { 6 } else { 24 };
    let mut mismatches = 0u32;
    for i in 0..seeds {
        let seed = 0x5EED_0000 + i as u64;
        let a = replay(seed);
        let b = replay(seed);
        if a != b {
            mismatches += 1;
        }
        // Interleave a busy workload between replays: scheduling noise
        // must not change decisions.
        let burn = std::thread::Builder::new()
            .spawn(|| {
                let mut x: u64 = 3;
                for _ in 0..3_000_000 {
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                }
                x
            })
            .map(|h| h.join().unwrap_or(0))
            .unwrap_or(0);
        let c = replay(seed);
        if a != c {
            mismatches += 1;
        }
        let _ = burn;
    }
    DeterminismReport { seeds, mismatches }
}

#[cfg(test)]
mod tests {
    #[test]
    fn determinism_gate() {
        let r = super::run(true);
        assert_eq!(r.mismatches, 0, "replays must be bit-identical");
        assert!(r.seeds > 0);
    }
}
