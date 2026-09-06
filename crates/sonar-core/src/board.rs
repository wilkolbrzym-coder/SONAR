//! Board logic: fleet, shots, and hit tracking.

use crate::bitboard::{BitBoard, MASK_100};
use crate::fleet::{FLEET, ship_mask};

/// State of a single cell from the attacker's perspective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Unknown, // not fired at yet
    Miss,    // water
    Hit,     // hit (not yet sunk)
    Sunk,    // sunk ship cell
}

/// The result of a shot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotResult {
    Miss,
    Hit,
    Sunk(u8),    // length of the sunk ship
    AlreadyShot, // this cell was already fired at
    Invalid,     // outside the board
}

/// A board: ship cells + the history of shots fired at it.
#[derive(Clone, Debug, Default)]
pub struct Board {
    /// Where the ships are (mask).
    pub ships: BitBoard,
    /// Where shots were fired (mask).
    pub shots: BitBoard,
    /// Where hits landed (mask) — subset of `shots`.
    pub hits: BitBoard,
    /// Sunk cells (mask) — subset of `hits`.
    pub sunk: BitBoard,
    /// The ships on this board with their masks and sunk status.
    pub ship_list: Vec<Ship>,
}

/// A ship on the board.
#[derive(Clone, Copy, Debug)]
pub struct Ship {
    pub r: usize,
    pub c: usize,
    pub len: u8,
    pub horizontal: bool,
    pub mask: u128,
    pub sunk: bool,
}

impl Ship {
    pub fn new(r: usize, c: usize, len: u8, horizontal: bool) -> Option<Self> {
        let mask = ship_mask(r, c, len, horizontal)?;
        Some(Self {
            r,
            c,
            len,
            horizontal,
            mask,
            sunk: false,
        })
    }

    /// Does the ship occupy cell (r, c)?
    #[inline(always)]
    pub fn occupies(&self, r: usize, c: usize) -> bool {
        (self.mask & (1u128 << (r * 10 + c))) != 0
    }

    /// Are all of the ship's cells hit?
    #[inline]
    pub fn is_sunk_by(&self, hits: BitBoard) -> bool {
        (hits.0 & self.mask) == self.mask
    }

    /// The ship's cells.
    pub fn cells(&self) -> Vec<(usize, usize)> {
        let mut v = Vec::with_capacity(self.len as usize);
        for i in 0..self.len as usize {
            if self.horizontal {
                v.push((self.r, self.c + i));
            } else {
                v.push((self.r + i, self.c));
            }
        }
        v
    }
}

impl Board {
    pub fn new() -> Self {
        Self::default()
    }

    /// Is adding a ship with this mask legal (no overlap with, and no
    /// contact with, existing ships)?
    pub fn can_place(&self, mask: u128) -> bool {
        // Check for overlap with existing ships.
        if (self.ships.0 & mask) != 0 {
            return false;
        }
        // Check for 8-directional contact — the standard Battleship rule:
        // ships may not touch.
        let dilated = BitBoard(mask).dilate8().0;
        if (self.ships.0 & dilated) != 0 {
            return false;
        }
        true
    }

    /// Place a ship on the board.
    pub fn place_ship(&mut self, ship: Ship) -> bool {
        if !self.can_place(ship.mask) {
            return false;
        }
        self.ships.0 |= ship.mask;
        self.ship_list.push(ship);
        true
    }

    /// Is the placement complete (all ships placed)?
    pub fn is_complete(&self) -> bool {
        self.ship_list.len() == FLEET.len()
    }

    /// Are all ships sunk?
    pub fn all_sunk(&self) -> bool {
        self.ship_list.iter().all(|s| s.sunk)
    }

    /// Fire at cell (r, c). Returns the result.
    pub fn shoot(&mut self, r: usize, c: usize) -> ShotResult {
        if r >= 10 || c >= 10 {
            return ShotResult::Invalid;
        }
        let bit = 1u128 << (r * 10 + c);
        if (self.shots.0 & bit) != 0 {
            return ShotResult::AlreadyShot;
        }
        self.shots.0 |= bit;
        let is_hit = (self.ships.0 & bit) != 0;
        if !is_hit {
            return ShotResult::Miss;
        }
        self.hits.0 |= bit;
        // Check whether a ship was sunk.
        for ship in &mut self.ship_list {
            if ship.occupies(r, c) && !ship.sunk && ship.is_sunk_by(self.hits) {
                ship.sunk = true;
                // Oznacz pola jako zatopione
                self.sunk.0 |= ship.mask;
                // On a sink, mark the neighbouring cells as fired
                // (misses) — the standard Battleship rule.
                let dilated = BitBoard(ship.mask).dilate8().0;
                let extra = dilated & !ship.mask & MASK_100;
                self.shots.0 |= extra;
                return ShotResult::Sunk(ship.len);
            }
        }
        ShotResult::Hit
    }

    /// How many ship cells remain afloat.
    pub fn remaining_ship_cells(&self) -> u32 {
        self.ships.popcount() - self.sunk.popcount()
    }

    /// Is cell (r, c) a ship?
    #[inline(always)]
    pub fn is_ship(&self, r: usize, c: usize) -> bool {
        self.ships.test(r, c)
    }

    /// Was cell (r, c) fired at?
    #[inline(always)]
    pub fn is_shot(&self, r: usize, c: usize) -> bool {
        self.shots.test(r, c)
    }

    /// Was cell (r, c) a hit?
    #[inline(always)]
    pub fn is_hit(&self, r: usize, c: usize) -> bool {
        self.hits.test(r, c)
    }

    /// Is cell (r, c) a sunk ship cell?
    #[inline(always)]
    pub fn is_sunk_cell(&self, r: usize, c: usize) -> bool {
        self.sunk.test(r, c)
    }

    /// Cell state from the attacker's perspective.
    #[inline(always)]
    pub fn cell_state_for_attacker(&self, r: usize, c: usize) -> Cell {
        if !self.is_shot(r, c) {
            Cell::Unknown
        } else if self.is_sunk_cell(r, c) {
            Cell::Sunk
        } else if self.is_hit(r, c) {
            Cell::Hit
        } else {
            Cell::Miss
        }
    }

    /// Cell state from the owner's perspective (ships are always visible).
    #[inline(always)]
    pub fn cell_state_for_owner(&self, r: usize, c: usize) -> OwnerCell {
        let is_ship = self.is_ship(r, c);
        if !self.is_shot(r, c) {
            if is_ship {
                OwnerCell::Ship
            } else {
                OwnerCell::Empty
            }
        } else if self.is_sunk_cell(r, c) {
            OwnerCell::SunkShip
        } else if self.is_hit(r, c) {
            OwnerCell::HitShip
        } else {
            OwnerCell::MissedShot
        }
    }

    /// Clear the board.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Lengths of the surviving (not yet sunk) ships.
    pub fn remaining_ship_lengths(&self) -> Vec<u8> {
        self.ship_list
            .iter()
            .filter(|s| !s.sunk)
            .map(|s| s.len)
            .collect()
    }
}

/// Cell state from the board owner's perspective.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerCell {
    Empty,      // water, not fired at
    Ship,       // ship, not fired at
    MissedShot, // a miss
    HitShip,    // hit, not yet sunk
    SunkShip,   // sunk
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_place_and_shoot() {
        let mut b = Board::new();
        let s = Ship::new(0, 0, 3, true).unwrap();
        assert!(b.place_ship(s));
        assert!(b.is_ship(0, 0));
        assert!(b.is_ship(0, 2));
        assert!(!b.is_ship(0, 3));

        assert_eq!(b.shoot(5, 5), ShotResult::Miss);
        assert_eq!(b.shoot(0, 0), ShotResult::Hit);
        assert_eq!(b.shoot(0, 1), ShotResult::Hit);
        assert_eq!(b.shoot(0, 2), ShotResult::Sunk(3));
        assert!(b.all_sunk());
    }

    #[test]
    fn test_adjacency_blocked() {
        let mut b = Board::new();
        let s1 = Ship::new(0, 0, 3, true).unwrap();
        assert!(b.place_ship(s1));
        // A ship touching side-by-side.
        let s2 = Ship::new(1, 0, 3, true).unwrap();
        assert!(!b.place_ship(s2)); // rejected — touching
        // A ship somewhere else.
        let s3 = Ship::new(5, 5, 2, true).unwrap();
        assert!(b.place_ship(s3));
    }

    #[test]
    fn test_already_shot() {
        let mut b = Board::new();
        let s = Ship::new(0, 0, 2, true).unwrap();
        b.place_ship(s);
        b.shoot(5, 5);
        assert_eq!(b.shoot(5, 5), ShotResult::AlreadyShot);
        assert_eq!(b.shoot(0, 0), ShotResult::Hit);
        assert_eq!(b.shoot(0, 0), ShotResult::AlreadyShot);
    }

    #[test]
    fn test_invalid_shot() {
        let mut b = Board::new();
        assert_eq!(b.shoot(10, 0), ShotResult::Invalid);
        assert_eq!(b.shoot(0, 10), ShotResult::Invalid);
    }

    #[test]
    fn test_sunk_marks_neighbors_as_miss() {
        let mut b = Board::new();
        let s = Ship::new(5, 5, 2, true).unwrap();
        b.place_ship(s);
        b.shoot(5, 5);
        b.shoot(5, 6);
        // After the sink, the neighbourhood must be marked as fired.
        assert!(b.is_shot(4, 4));
        assert!(b.is_shot(4, 5));
        assert!(b.is_shot(4, 6));
        assert!(b.is_shot(4, 7)); // corner cells too
        assert!(b.is_shot(6, 5));
        assert!(b.is_shot(5, 4));
        assert!(b.is_shot(5, 7));
        assert!(!b.is_shot(7, 7)); // outside the neighbourhood
    }

    #[test]
    fn test_remaining_lengths() {
        let mut b = Board::new();
        b.place_ship(Ship::new(0, 0, 5, true).unwrap());
        b.place_ship(Ship::new(5, 0, 2, true).unwrap());
        let lens = b.remaining_ship_lengths();
        assert_eq!(lens, vec![5, 2]);

        b.shoot(5, 0);
        b.shoot(5, 1);
        let lens2 = b.remaining_ship_lengths();
        assert_eq!(lens2, vec![5]);
    }
}
