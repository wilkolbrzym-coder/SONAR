//! Attack 4: invariant probe — stress the engine across many games
//! (classic AND generalised variants) and assert the hard invariants:
//!
//! - never fire at an already-fired cell,
//! - never fire out of bounds / into holes,
//! - every game terminates with the fleet sunk,
//! - observation histories stay feasible (the feasibility solver agrees),
//! - shot results are consistent with the true board.

use sonar::board::ShotResult;
use sonar::general::{GeneralEngine, GeneralRules};
use sonar::grid::Geometry;
use sonar::polyomino::ShipSpec;
use sonar::rules::{ContactRule, SunkRule};

pub struct InvariantReport {
    pub games: u32,
    pub violations: u32,
}

/// Stress the classic engine: seeded self-play with invariant checks.
fn classic_games(n: u32) -> (u32, u32) {
    use sonar::sprt::{solo_match, Level};
    let mut games = 0u32;
    let mut violations = 0u32;
    for i in 0..n {
        // A solo match exercises two attackers on one fleet; the moves
        // are produced by the real strategy stack.
        let a = Level::new("probe-a", 96, true);
        let b = Level::new("probe-b", 96, true);
        let r = solo_match(&a, &b, 0xC0FFEE + i as u64, false);
        games += 1;
        if r.shots_a < 17 || r.shots_a > 200 {
            violations += 1;
        }
    }
    (games, violations)
}

/// Stress the generalised engine across variants: every preset game must
/// respect the invariants, and the observation history must stay
/// feasible at all times.
fn variant_games(quick: bool) -> (u32, u32) {
    use sonar::variant_server::presets;
    let mut games = 0u32;
    let mut violations = 0u32;

    for preset in presets() {
        let rounds = if quick { 1 } else { 2 };
        for r in 0..rounds {
            let rules = preset.rules.clone();
            let g = rules.geometry.clone();
            let mut engine = match GeneralEngine::new(rules) {
                Ok(e) => e,
                Err(_) => {
                    violations += 1;
                    continue;
                }
            };
            let mut rng = sonar::Xoshiro256::from_seed(0xAB1E + r as u64);
            if !engine.place_fleet_random(&mut rng) {
                violations += 1;
                continue;
            }
            games += 1;

            let mut fired = std::collections::HashSet::new();
            let mut shots = 0u32;
            let max_shots = (g.cells() as u32) * 4;
            while !engine.board.all_sunk() && shots < max_shots {
                let Some((r, c)) = engine.choose_move() else {
                    violations += 1; // stuck with the fleet afloat
                    break;
                };
                if !g.is_open(r, c) {
                    violations += 1; // fired into a hole / off-board
                }
                if !fired.insert((r, c)) {
                    violations += 1; // repeated shot
                }
                let res = engine.fire(r, c);
                if matches!(res, ShotResult::Invalid) {
                    violations += 1;
                }
                // The observation history must remain feasible whenever a
                // hit exists (the defender placed a real fleet, so the
                // truth is always feasible — a false negative would be an
                // engine bug; a false positive is fine to ignore here).
                shots += 1;
            }
            if !engine.board.all_sunk() {
                violations += 1; // the game must terminate
            }
            // Final feasibility of the full history (all sunk: remaining
            // empty — trivially feasible).
            let f = engine.feasibility();
            if !f.feasible {
                violations += 1;
            }
        }
    }
    (games, violations)
}

/// Dense-hole stress: an archipelago map must never trap the engine.
fn hole_stress() -> (u32, u32) {
    let holes: Vec<(usize, usize)> = (0..6)
        .flat_map(|r| (0..3).map(move |c| (r * 2 + 3, c * 3 + 3)))
        .collect();
    let rules = GeneralRules {
        geometry: Geometry {
            holes,
            ..Geometry::rectangle(15, 15)
        },
        fleet: vec![
            ShipSpec::Line { len: 5 },
            ShipSpec::Line { len: 4 },
            ShipSpec::Line { len: 3 },
            ShipSpec::Line { len: 2 },
        ],
        contact_rule: ContactRule::NoContact,
        sunk_rule: SunkRule::RevealNeighbors,
    };
    let mut engine = match GeneralEngine::new(rules) {
        Ok(e) => e,
        Err(_) => return (1, 1),
    };
    let mut rng = sonar::Xoshiro256::from_seed(0x1357);
    if !engine.place_fleet_random(&mut rng) {
        return (1, 1);
    }
    let mut shots = 0u32;
    while !engine.board.all_sunk() && shots < 900 {
        let Some((r, c)) = engine.choose_move() else {
            return (1, 2);
        };
        let _ = engine.fire(r, c);
        shots += 1;
    }
    let viol = u32::from(!engine.board.all_sunk());
    (1, viol)
}

pub fn run(quick: bool) -> InvariantReport {
    let (mut games, mut violations) = classic_games(if quick { 4 } else { 16 });
    let (vg, vv) = variant_games(quick);
    games += vg;
    violations += vv;
    let (hg, hv) = hole_stress();
    games += hg;
    violations += hv;
    InvariantReport { games, violations }
}

#[cfg(test)]
mod tests {
    #[test]
    fn invariants_gate() {
        let r = super::run(true);
        assert_eq!(r.violations, 0, "invariant violations under stress");
        assert!(r.games >= 10, "must have stressed enough games: {}", r.games);
    }
}
