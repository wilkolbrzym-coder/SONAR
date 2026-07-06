//! Logika planszy + flota + strzały.

use crate::bitboard::{BitBoard, MASK_100};
use crate::fleet::{FLEET, ship_mask};

/// Stan pojedynczego pola z perspektywy gracza patrzącego na planszę wroga
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Unknown,    // nie strzelano
    Miss,       // pudło
    Hit,        // trafiony (ale nie zatopiony)
    Sunk,       // zatopione pole
}

/// Wynik strzału
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotResult {
    Miss,
    Hit,
    Sunk(u8),    // długość zatopionego statku
    AlreadyShot, // już tu strzelano
    Invalid,     // poza planszą
}

/// Plansza: pola statków + historia strzałów w tę planszę
#[derive(Clone, Debug, Default)]
pub struct Board {
    /// Gdzie są statki (maska)
    pub ships: BitBoard,
    /// Gdzie oddano strzały (maska)
    pub shots: BitBoard,
    /// Gdzie trafiono (maska) - podzbiór `shots`
    pub hits: BitBoard,
    /// Gdzie są zatopione pola (maska) - podzbiór `hits`
    pub sunk: BitBoard,
    /// Czy statek na danym polu jest zatopiony (dla każdego pola)
    /// używamy długości statku do obliczenia po zatopieniu
    /// Dla uproszczenia: lista statków z ich maskami i statusem
    pub ship_list: Vec<Ship>,
}

/// Statek na planszy
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
        Some(Self { r, c, len, horizontal, mask, sunk: false })
    }

    /// Czy statek zajmuje pole (r,c)
    #[inline(always)]
    pub fn occupies(&self, r: usize, c: usize) -> bool {
        (self.mask & (1u128 << (r * 10 + c))) != 0
    }

    /// Czy wszystkie pola statku są trafione
    #[inline]
    pub fn is_sunk_by(&self, hits: BitBoard) -> bool {
        (hits.0 & self.mask) == self.mask
    }

    /// Pola statku
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

    /// Czy dodanie statku (maska) jest legalne (nie zachodzi na inny statek, nie dotyka)
    pub fn can_place(&self, mask: u128) -> bool {
        // Sprawdź czy zachodzi na istniejące statki
        if (self.ships.0 & mask) != 0 {
            return false;
        }
        // Sprawdź czy dotyka (8-kierunkowo) - to standardowa zasada statków
        // Możemy zezwalać lub nie, standardowo: statki nie mogą się dotykać
        let dilated = BitBoard(mask).dilate8().0;
        if (self.ships.0 & dilated) != 0 {
            return false;
        }
        true
    }

    /// Połóż statek na planszy
    pub fn place_ship(&mut self, ship: Ship) -> bool {
        if !self.can_place(ship.mask) {
            return false;
        }
        self.ships.0 |= ship.mask;
        self.ship_list.push(ship);
        true
    }

    /// Czy ustawienie kompletne (wszystkie statki położone)
    pub fn is_complete(&self) -> bool {
        self.ship_list.len() == FLEET.len()
    }

    /// Czy wszystkie statki zatopione
    pub fn all_sunk(&self) -> bool {
        self.ship_list.iter().all(|s| s.sunk)
    }

    /// Oddaj strzał w pole (r, c). Zwraca wynik.
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
        // Sprawdź czy zatopiono statek
        for ship in &mut self.ship_list {
            if ship.occupies(r, c) && !ship.sunk {
                if ship.is_sunk_by(self.hits) {
                    ship.sunk = true;
                    // Oznacz pola jako zatopione
                    self.sunk.0 |= ship.mask;
                    // Dodatkowo: przy zatopieniu oznacz sąsiadujące pola jako strzelane (pudła)
                    // To standardowa zasada - jeśli zatopiono, otoczenie jest pudłami
                    let dilated = BitBoard(ship.mask).dilate8().0;
                    let extra = dilated & !ship.mask & MASK_100;
                    self.shots.0 |= extra;
                    return ShotResult::Sunk(ship.len);
                }
            }
        }
        ShotResult::Hit
    }

    /// Ile pól statku pozostało niezatopionych
    pub fn remaining_ship_cells(&self) -> u32 {
        self.ships.popcount() - self.sunk.popcount()
    }

    /// Czy pole (r,c) jest statkiem
    #[inline(always)]
    pub fn is_ship(&self, r: usize, c: usize) -> bool {
        self.ships.test(r, c)
    }

    /// Czy strzelano w pole
    #[inline(always)]
    pub fn is_shot(&self, r: usize, c: usize) -> bool {
        self.shots.test(r, c)
    }

    /// Czy pole jest trafione
    #[inline(always)]
    pub fn is_hit(&self, r: usize, c: usize) -> bool {
        self.hits.test(r, c)
    }

    /// Czy pole to statek zatopiony
    #[inline(always)]
    pub fn is_sunk_cell(&self, r: usize, c: usize) -> bool {
        self.sunk.test(r, c)
    }

    /// Stan pola z perspektywy gracza patrzącego na tę planszę jako planszę wroga
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

    /// Stan pola z perspektywy właściciela planszy (zawsze widzi swoje statki)
    #[inline(always)]
    pub fn cell_state_for_owner(&self, r: usize, c: usize) -> OwnerCell {
        let is_ship = self.is_ship(r, c);
        if !self.is_shot(r, c) {
            if is_ship { OwnerCell::Ship } else { OwnerCell::Empty }
        } else if self.is_sunk_cell(r, c) {
            OwnerCell::SunkShip
        } else if self.is_hit(r, c) {
            OwnerCell::HitShip
        } else {
            OwnerCell::MissedShot
        }
    }

    /// Wyczyść planszę
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Statki pozostałe (niezatopione) długości
    pub fn remaining_ship_lengths(&self) -> Vec<u8> {
        self.ship_list.iter().filter(|s| !s.sunk).map(|s| s.len).collect()
    }
}

/// Stan pola z perspektywy właściciela planszy
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerCell {
    Empty,        // puste pole, nie strzelano
    Ship,         // statek, nie strzelano
    MissedShot,   // pudło
    HitShip,      // trafiony niezatopiony
    SunkShip,     // zatopiony
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
        // Statek dotykający bocznie
        let s2 = Ship::new(1, 0, 3, true).unwrap();
        assert!(!b.place_ship(s2)); // nie można - dotyka
        // Statek w innym miejscu
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
        // Po zatopieniu sąsiednie pola powinny być strzelane (pudłami)
        // (4,5), (4,6), (6,5), (6,6), (5,4), (5,7) itd.
        assert!(b.is_shot(4, 4));
        assert!(b.is_shot(4, 5));
        assert!(b.is_shot(4, 6));
        assert!(b.is_shot(4, 7)); // też rogowe
        assert!(b.is_shot(6, 5));
        assert!(b.is_shot(5, 4));
        assert!(b.is_shot(5, 7));
        assert!(!b.is_shot(7, 7)); // poza otoczeniem
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
