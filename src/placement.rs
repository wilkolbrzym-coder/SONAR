//! Intelligent fleet placement.
//!
//! Strategy: **constraint-dispersal** with parity + edge avoidance + entropy
//! padding.
//!
//! We generate `N` random legal fleet configurations and pick the one with
//! the smallest penalty (sum of):
//!   - ship-to-ship contact (orthogonal or diagonal) — heavy penalty
//!   - touching the board edge — light penalty
//!   - ships in corners — light penalty (PDFs love the centre)
//!   - parity imbalance — light penalty
//!
//! The randomness is deterministic per seed, which makes it harder for an
//! opponent's PDF to converge (our ships are never in the same place twice).

use crate::bitboard::{BitBoard, MASK_100};
use crate::board::{Board, Ship};
use crate::fleet::FLEET;
use crate::rng::Xoshiro256;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// PlacementConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Knobs for the placement strategy.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PlacementConfig {
    /// How many candidate fleets to sample.
    pub candidates: u32,
    /// Penalty for orthogonal/diagonal ship-to-ship contact.
    pub penalty_contact: f32,
    /// Penalty for touching the board edge.
    pub penalty_edge: f32,
    /// Penalty for occupying corner cells.
    pub penalty_corner: f32,
    /// If `true`, penalise fleets that are unbalanced between even and
    /// odd-parity cells.
    pub parity_balance: bool,
}

impl Default for PlacementConfig {
    fn default() -> Self {
        Self {
            candidates: 1024,
            penalty_contact: 10.0,
            penalty_edge: 0.3,
            penalty_corner: 1.5,
            parity_balance: true,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FleetConfig
// ─────────────────────────────────────────────────────────────────────────────

/// A complete fleet configuration: 5 ships on the 10×10 board.
#[derive(Clone, Debug)]
pub struct FleetConfig {
    pub ships: [Ship; 5],
    pub mask: u128,
}

impl FleetConfig {
    pub fn from_ships(ships: [Ship; 5]) -> Self {
        let mask = ships.iter().map(|s| s.mask).fold(0u128, |a, b| a | b);
        Self { ships, mask }
    }

    pub fn to_board(&self) -> Board {
        let mut b = Board::new();
        for s in &self.ships {
            b.place_ship(*s);
        }
        b
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Random fleet generation
// ─────────────────────────────────────────────────────────────────────────────

/// Generate one random legal fleet. Returns `None` if generation fails
/// after 100 attempts (extremely rare).
///
/// Uses pre-calculated ship placements for speed — picks a random
/// placement from the legal list rather than trying random (r,c)
/// coordinates.
pub fn random_fleet(rng: &mut Xoshiro256) -> Option<FleetConfig> {
    use crate::fleet::placements as get_placements;
    let placements = get_placements();

    for _ in 0..100 {
        let mut board = Board::new();
        let mut ships: Vec<Ship> = Vec::with_capacity(5);
        let mut ok = true;
        for &len in FLEET {
            let len_idx = len as usize;
            if len_idx >= placements.len() {
                ok = false;
                break;
            }
            let legal = &placements[len_idx];
            // Try a random sample of placements (max 300 tries).
            let mut placed = false;
            for _ in 0..300 {
                let idx = rng.gen_range(legal.len() as u64) as usize;
                let &(r, c, horiz, mask) = &legal[idx];
                // Fast inline placement check using the mask directly.
                let dilated = BitBoard(mask).dilate8().0;
                if (board.ships.0 & mask) == 0 && (board.ships.0 & dilated) == 0 {
                    // Place without going through Board::place_ship (faster).
                    board.ships.0 |= mask;
                    let ship = Ship { r, c, len, horizontal: horiz, mask, sunk: false };
                    board.ship_list.push(ship);
                    ships.push(ship);
                    placed = true;
                    break;
                }
            }
            if !placed {
                ok = false;
                break;
            }
        }
        if ok && ships.len() == 5 {
            let arr: [Ship; 5] = ships.try_into().ok()?;
            return Some(FleetConfig::from_ships(arr));
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────
// Penalty function
// ─────────────────────────────────────────────────────────────────────────────

/// Compute the placement penalty for a fleet configuration.
pub fn fleet_penalty(cfg: &FleetConfig, config: &PlacementConfig) -> f32 {
    let mut penalty = 0.0f32;
    let ships_mask = cfg.mask;

    // 1. Ship-to-ship contact (any 8-neighbour overlap).
    for s in &cfg.ships {
        let dilated = BitBoard(s.mask).dilate8().0;
        let others = ships_mask & !s.mask;
        let contact = (dilated & others).count_ones() as f32;
        penalty += contact * config.penalty_contact;
    }

    // 2. Touching the board edge.
    let edge_mask = BitBoard::row_mask(0) | BitBoard::row_mask(9)
        | BitBoard::col_mask(0) | BitBoard::col_mask(9);
    let edge_count = (ships_mask & edge_mask).count_ones() as f32;
    penalty += edge_count * config.penalty_edge;

    // 3. Corner cells.
    let corner_mask = (1u128 << 0) | (1u128 << 9) | (1u128 << 90) | (1u128 << 99);
    let corner_count = (ships_mask & corner_mask).count_ones() as f32;
    penalty += corner_count * config.penalty_corner;

    // 4. Parity balance.
    if config.parity_balance {
        let mut parity_mask = 0u128;
        for r in 0..10 {
            for c in 0..10 {
                if (r + c) & 1 == 0 {
                    parity_mask |= 1u128 << (r * 10 + c);
                }
            }
        }
        let on_parity = (ships_mask & parity_mask).count_ones() as f32;
        let off_parity = (ships_mask & !parity_mask & MASK_100).count_ones() as f32;
        let total = on_parity + off_parity;
        if total > 0.0 {
            let ratio = on_parity / total;
            penalty += (ratio - 0.5).abs() * 8.0;
        }
    }

    penalty
}

// ─────────────────────────────────────────────────────────────────────────────
// Best-fleet selection
// ─────────────────────────────────────────────────────────────────────────────

/// Sample `config.candidates` random fleets and return the one with the
/// smallest penalty.
pub fn best_fleet(rng: &mut Xoshiro256, config: &PlacementConfig) -> FleetConfig {
    let mut best: Option<FleetConfig> = None;
    let mut best_penalty = f32::INFINITY;

    for _ in 0..config.candidates {
        if let Some(cfg) = random_fleet(rng) {
            let p = fleet_penalty(&cfg, config);
            if p < best_penalty {
                best_penalty = p;
                best = Some(cfg);
            }
        }
    }

    best.unwrap_or_else(|| random_fleet(rng).expect("fleet generation failed"))
}

/// Place a smart (penalty-minimising) fleet on a fresh [`Board`].
pub fn place_best_fleet(rng: &mut Xoshiro256, config: &PlacementConfig) -> Board {
    best_fleet(rng, config).to_board()
}

/// Place a uniformly random legal fleet on a fresh [`Board`].
pub fn place_random_fleet(rng: &mut Xoshiro256) -> Board {
    random_fleet(rng).expect("fleet generation failed").to_board()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_fleet_valid() {
        let mut rng = Xoshiro256::from_seed(42);
        for _ in 0..20 {
            let cfg = random_fleet(&mut rng).expect("should generate fleet");
            assert_eq!(cfg.ships.len(), 5);
            assert_eq!(cfg.mask.count_ones(), 17); // 5+4+3+3+2
            let mut lens: Vec<u8> = cfg.ships.iter().map(|s| s.len).collect();
            lens.sort_unstable();
            assert_eq!(lens, vec![2, 3, 3, 4, 5]);
        }
    }

    #[test]
    fn test_no_overlapping_ships() {
        let mut rng = Xoshiro256::from_seed(123);
        for _ in 0..50 {
            let cfg = random_fleet(&mut rng).unwrap();
            assert_eq!(cfg.mask.count_ones(), 17);
            for i in 0..5 {
                for j in (i + 1)..5 {
                    let di = BitBoard(cfg.ships[i].mask).dilate8().0;
                    let overlap = di & cfg.ships[j].mask;
                    assert_eq!(
                        overlap, 0,
                        "ships {} (r={},c={},len={},h={}) and {} (r={},c={},len={},h={}) touch",
                        i, cfg.ships[i].r, cfg.ships[i].c, cfg.ships[i].len, cfg.ships[i].horizontal,
                        j, cfg.ships[j].r, cfg.ships[j].c, cfg.ships[j].len, cfg.ships[j].horizontal,
                    );
                }
            }
        }
    }

    #[test]
    fn test_best_fleet_beats_random() {
        let mut rng = Xoshiro256::from_seed(7);
        let cfg = PlacementConfig { candidates: 256, ..Default::default() };
        let best = best_fleet(&mut rng, &cfg);
        let best_p = fleet_penalty(&best, &cfg);
        let mut sum = 0.0;
        for _ in 0..50 {
            if let Some(r) = random_fleet(&mut rng) {
                sum += fleet_penalty(&r, &cfg);
            }
        }
        let avg = sum / 50.0;
        assert!(best_p < avg, "best ({}) should be < avg ({})", best_p, avg);
    }

    #[test]
    fn test_placement_config_serializes() {
        let cfg = PlacementConfig::default();
        let s = serde_json::to_string(&cfg).unwrap();
        let back: PlacementConfig = serde_json::from_str(&s).unwrap();
        assert_eq!(back.candidates, cfg.candidates);
    }
}
