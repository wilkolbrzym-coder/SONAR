//! Attack 2: adversarial fleet placement — an oracle with full knowledge
//! of Sonar's public design searches for the fleet layout that costs
//! Sonar the most shots to sink.
//!
//! This measures **exploitability**: how much a deliberately hostile
//! defender gains over a uniformly-random defender. Sonar's defensive
//! design (GHOST FLEET mixed placement + pure-function targeting) aims
//! to keep this delta small.
//!
//! Method: hill-climbing with restarts over single-ship mutations of a
//! legal fleet; the fitness is the number of shots Sonar needs to sink
//! it (the attacker runs with work-limited budgets, so the evaluation
//! is deterministic).

use sonar::api::{Engine, EngineConfig};
use sonar::board::{Board, Ship};
use sonar::rng::Xoshiro256;

/// The measured exploitability of a run.
pub struct AdversaryReport {
    pub adversary_shots: f64,
    pub random_shots: f64,
    pub delta: f64,
    /// The redline: adversarial delta above this is a breach.
    pub threshold: f64,
}

/// One solo attack: how many shots does the engine need to sink `board`?
fn sonar_vs(board: &Board, seed: u64) -> u32 {
    let mut engine = Engine::new(EngineConfig {
        use_learning: false,
        hypothesis_soft_target: 256,
        default_deadline_secs: 0,
        ..Default::default()
    });
    engine.reseed(seed);
    let mut target = board.clone();
    let mut shots = 0u32;
    while !target.all_sunk() && shots < 200 {
        let (r, c) = engine.choose_move(sonar::Deadline::none());
        let res = target.shoot(r, c);
        engine.observe_result(r, c, res);
        shots += 1;
    }
    shots
}

/// Random legal fleet for the mutation search to start from.
fn random_board(rng: &mut Xoshiro256) -> Board {
    use sonar::placement::place_random_fleet;
    place_random_fleet(rng)
}

/// Mutate: move one ship to a random legal position.
fn mutate(board: &Board, rng: &mut Xoshiro256) -> Option<Board> {
    let mut b = board.clone();
    if b.ship_list.is_empty() {
        return None;
    }
    let victim = rng.gen_range(b.ship_list.len() as u64) as usize;
    let len = b.ship_list[victim].len;
    // Remove the victim.
    let removed = b.ship_list.remove(victim);
    b.ships.0 &= !removed.mask;
    // Re-place it somewhere legal.
    for _ in 0..200 {
        let r = rng.gen_range(10) as usize;
        let c = rng.gen_range(10) as usize;
        let h = rng.gen_range(2) == 0;
        if let Some(s) = Ship::new(r, c, len, h) {
            if b.place_ship(s) {
                return Some(b);
            }
        }
    }
    None
}

/// Hill-climb toward the worst-for-Sonar fleet.
pub fn run(quick: bool) -> AdversaryReport {
    let iterations = if quick { 24 } else { 120 };
    let evals_per_board = if quick { 2 } else { 3 };

    let mut rng = Xoshiro256::from_seed(0xBAAD_1DEA);

    // Baseline: uniformly-random defenders.
    let mut random_total = 0u64;
    let random_games = if quick { 8 } else { 24 };
    for i in 0..random_games {
        let b = random_board(&mut rng);
        random_total += sonar_vs(&b, 0xF1EE + i as u64) as u64;
    }
    let random_shots = random_total as f64 / random_games as f64;

    // Hill-climbing adversary (full knowledge of the public algorithm).
    let mut best_board = random_board(&mut rng);
    let mut best_score: f64 = {
        let mut s = 0u64;
        for e in 0..evals_per_board {
            s += sonar_vs(&best_board, 0xDEA1 + e as u64) as u64;
        }
        s as f64 / evals_per_board as f64
    };
    for _ in 0..iterations {
        let Some(candidate) = mutate(&best_board, &mut rng) else {
            continue;
        };
        let mut s = 0u64;
        for e in 0..evals_per_board {
            s += sonar_vs(&candidate, 0xDEA1 + e as u64) as u64;
        }
        let score = s as f64 / evals_per_board as f64;
        if score > best_score {
            best_score = score;
            best_board = candidate;
        }
    }

    AdversaryReport {
        adversary_shots: best_score,
        random_shots,
        delta: best_score - random_shots,
        // Redline: the adversary may not extract more than 14 extra shots
        // from a perfectly-informed defense. Calibration: 0.5.0-beta.1
        // measured delta ≈ 12.4 (corner/edge clustering vs the parity
        // hunt). This baseline is tracked in EXPLOITABILITY.md; the
        // long-term goal is to drive it toward zero.
        threshold: 14.0,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn adversary_gate() {
        let r = super::run(true);
        assert!(
            r.delta <= r.threshold,
            "adversarial delta {:.2} exceeds threshold {:.2}",
            r.delta,
            r.threshold
        );
        assert!(r.adversary_shots >= 17.0, "must sink at least the ship cells");
    }
}
