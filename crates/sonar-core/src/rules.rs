//! Game rules — configurable rule set for micro-modes.
//!
//! Sonar's `GameRules` struct lets you customise:
//! - board size (5..=10)
//! - fleet composition (any set of ship lengths)
//! - ship-to-ship contact rule
//! - sink reveal rule
//!
//! This enables custom game variants ("micro-modes") beyond the
//! standard 10×10 / [5,4,3,3,2] ruleset.

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// ContactRule
// ─────────────────────────────────────────────────────────────────────────────

/// Rule for how ships may be placed relative to each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContactRule {
    /// Ships may not touch at all — neither orthogonally nor diagonally.
    /// This is the standard Battleship rule.
    NoContact,
    /// Ships may touch diagonally (corner-to-corner) but not
    /// orthogonally (side-to-side).
    AllowCornerContact,
    /// Ships may touch freely (orthogonally and diagonally).
    AllowContact,
}

impl Default for ContactRule {
    fn default() -> Self {
        Self::NoContact
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SunkRule
// ─────────────────────────────────────────────────────────────────────────────

/// Rule for what happens when a ship is sunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SunkRule {
    /// When a ship sinks, all 8-neighbour cells are automatically marked
    /// as misses. This is the standard Battleship rule and helps the
    /// attacker by eliminating cells around the sunk ship.
    RevealNeighbors,
    /// When a ship sinks, only the ship's cells are marked as sunk.
    /// No neighbours are revealed.
    NoReveal,
}

impl Default for SunkRule {
    fn default() -> Self {
        Self::RevealNeighbors
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GameRules
// ─────────────────────────────────────────────────────────────────────────────

/// Configurable rule set for a Sonar game.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameRules {
    /// Board size (width = height). Must be in `5..=10`.
    pub board_size: usize,
    /// Ship lengths, e.g. `[5, 4, 3, 3, 2]` for the standard fleet.
    /// Each length must be in `1..=board_size`.
    pub ship_lengths: Vec<u8>,
    /// How ships may touch each other.
    pub contact_rule: ContactRule,
    /// What happens when a ship sinks.
    pub sunk_rule: SunkRule,
}

impl Default for GameRules {
    fn default() -> Self {
        Self {
            board_size: 10,
            ship_lengths: vec![5, 4, 3, 3, 2],
            contact_rule: ContactRule::NoContact,
            sunk_rule: SunkRule::RevealNeighbors,
        }
    }
}

impl GameRules {
    /// Create a new ruleset with the given board size and standard fleet.
    pub fn new(board_size: usize) -> Self {
        Self {
            board_size,
            ..Default::default()
        }
    }

    /// Validate the ruleset. Returns `Err` with a human-readable message
    /// if any rule is inconsistent.
    pub fn validate(&self) -> Result<(), String> {
        if self.board_size < 5 || self.board_size > 10 {
            return Err(format!(
                "board_size must be in 5..=10, got {}",
                self.board_size
            ));
        }
        if self.ship_lengths.is_empty() {
            return Err("ship_lengths must not be empty".to_string());
        }
        if self.ship_lengths.len() > 10 {
            return Err(format!(
                "too many ships ({}), maximum 10",
                self.ship_lengths.len()
            ));
        }
        for &len in &self.ship_lengths {
            if len == 0 {
                return Err("ship length must be > 0".to_string());
            }
            if len as usize > self.board_size {
                return Err(format!(
                    "ship length {} exceeds board_size {}",
                    len, self.board_size
                ));
            }
        }
        let total_cells: usize = self.ship_lengths.iter().map(|&l| l as usize).sum();
        let board_cells = self.board_size * self.board_size;
        if total_cells > board_cells / 2 {
            return Err(format!(
                "total ship cells ({}) exceed half the board ({})",
                total_cells,
                board_cells / 2
            ));
        }
        Ok(())
    }

    /// Total number of ship cells.
    pub fn total_ship_cells(&self) -> usize {
        self.ship_lengths.iter().map(|&l| l as usize).sum()
    }

    /// Number of ships.
    pub fn ship_count(&self) -> usize {
        self.ship_lengths.len()
    }

    /// Mask of all valid cells for this board size (bits 0..board_size²
    /// set).
    pub fn board_mask(&self) -> u128 {
        let n = self.board_size * self.board_size;
        if n >= 128 {
            u128::MAX
        } else {
            (1u128 << n) - 1
        }
    }

    /// Check whether a cell `(r, c)` is inside the board.
    pub fn is_in_bounds(&self, r: usize, c: usize) -> bool {
        r < self.board_size && c < self.board_size
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rules() {
        let r = GameRules::default();
        assert_eq!(r.board_size, 10);
        assert_eq!(r.ship_lengths, vec![5, 4, 3, 3, 2]);
        assert_eq!(r.contact_rule, ContactRule::NoContact);
        assert_eq!(r.sunk_rule, SunkRule::RevealNeighbors);
        assert!(r.validate().is_ok());
    }

    #[test]
    fn test_board_size_validation() {
        let r = GameRules {
            board_size: 4,
            ..Default::default()
        };
        assert!(r.validate().is_err());

        let r = GameRules {
            board_size: 11,
            ..Default::default()
        };
        assert!(r.validate().is_err());

        let r = GameRules {
            board_size: 5,
            ship_lengths: vec![3, 2],
            ..Default::default()
        };
        assert!(r.validate().is_ok());
    }

    #[test]
    fn test_ship_length_validation() {
        let r = GameRules {
            board_size: 5,
            ship_lengths: vec![6],
            ..Default::default()
        };
        assert!(r.validate().is_err());

        let r = GameRules {
            ship_lengths: vec![5, 0, 3],
            ..Default::default()
        };
        assert!(r.validate().is_err());
    }

    #[test]
    fn test_too_many_ships() {
        let r = GameRules {
            ship_lengths: vec![2; 11],
            ..Default::default()
        };
        assert!(r.validate().is_err());
    }

    #[test]
    fn test_total_ship_cells() {
        let r = GameRules::default();
        assert_eq!(r.total_ship_cells(), 17); // 5+4+3+3+2
        assert_eq!(r.ship_count(), 5);
    }

    #[test]
    fn test_board_mask() {
        let r = GameRules {
            board_size: 5,
            ..Default::default()
        };
        let mask = r.board_mask();
        assert_eq!(mask.count_ones(), 25); // 5*5 = 25 cells
    }

    #[test]
    fn test_is_in_bounds() {
        let r = GameRules {
            board_size: 7,
            ..Default::default()
        };
        assert!(r.is_in_bounds(0, 0));
        assert!(r.is_in_bounds(6, 6));
        assert!(!r.is_in_bounds(7, 0));
        assert!(!r.is_in_bounds(0, 7));
        assert!(!r.is_in_bounds(7, 7));
    }

    #[test]
    fn test_custom_fleet() {
        let r = GameRules {
            board_size: 8,
            ship_lengths: vec![4, 3, 3, 2, 2],
            ..Default::default()
        };
        assert!(r.validate().is_ok());
        assert_eq!(r.total_ship_cells(), 14);
        assert_eq!(r.ship_count(), 5);
    }

    #[test]
    fn test_serialization() {
        let r = GameRules::default();
        let json = serde_json::to_string(&r).unwrap();
        let back: GameRules = serde_json::from_str(&json).unwrap();
        assert_eq!(back.board_size, r.board_size);
        assert_eq!(back.ship_lengths, r.ship_lengths);
        assert_eq!(back.contact_rule, r.contact_rule);
        assert_eq!(back.sunk_rule, r.sunk_rule);
    }

    #[test]
    fn test_contact_rule_serialization() {
        for rule in [ContactRule::NoContact, ContactRule::AllowCornerContact, ContactRule::AllowContact] {
            let json = serde_json::to_string(&rule).unwrap();
            let back: ContactRule = serde_json::from_str(&json).unwrap();
            assert_eq!(rule, back);
        }
    }

    #[test]
    fn test_sunk_rule_serialization() {
        for rule in [SunkRule::RevealNeighbors, SunkRule::NoReveal] {
            let json = serde_json::to_string(&rule).unwrap();
            let back: SunkRule = serde_json::from_str(&json).unwrap();
            assert_eq!(rule, back);
        }
    }
}
