//! Definicja floty statków.

/// Długości statków (standard: 5,4,3,3,2)
pub const FLEET: &[u8] = &[5, 4, 3, 3, 2];

/// Łączna liczba pól statków
pub const FLEET_TOTAL: u32 = 5 + 4 + 3 + 3 + 2; // = 17

/// Ile statków
pub const FLEET_COUNT: usize = FLEET.len();

/// Maska pól zajętych przez statek o danej długości zaczynający się w (r,c) w orientacji horiz/vert
/// Zwraca None jeśli statek wystaje poza planszę
#[inline(always)]
pub fn ship_mask(r: usize, c: usize, len: u8, horizontal: bool) -> Option<u128> {
    if horizontal {
        if c + len as usize > 10 {
            return None;
        }
        let row_bits = ((1u128 << len) - 1) << c;
        Some(row_bits << (r * 10))
    } else {
        if r + len as usize > 10 {
            return None;
        }
        let col_bit = 1u128 << c;
        let mut m = 0u128;
        for i in 0..len as usize {
            m |= col_bit << ((r + i) * 10);
        }
        Some(m)
    }
}

/// Lista wszystkich możliwych (legalnych) pojedynczych umiejscowień statku o danej długości.
/// Obliczane raz, przechowywane w statycznej tablicy.
pub fn all_placements_for_len(len: u8) -> Vec<(usize, usize, bool, u128)> {
    let mut v = Vec::new();
    for r in 0..10 {
        for c in 0..10 {
            if let Some(m) = ship_mask(r, c, len, true) {
                v.push((r, c, true, m));
            }
            if len > 1 {
                if let Some(m) = ship_mask(r, c, len, false) {
                    v.push((r, c, false, m));
                }
            }
        }
    }
    v
}

/// Prekalkulowane umiejscowienia dla każdego rozmiaru statku.
/// Index: długość (2..=5) -> Vec<(r, c, horizontal, mask)>
pub fn all_placements_by_len() -> Vec<Vec<(usize, usize, bool, u128)>> {
    let mut out = Vec::with_capacity(6);
    for _ in 0..2 {
        out.push(Vec::new()); // długości 0 i 1 nieużywane
    }
    for len in 2..=5 {
        out.push(all_placements_for_len(len as u8));
    }
    out
}

/// Cached pre-calculated placements (initialised once on first access).
use std::sync::OnceLock;
static ALL_PLACEMENTS: OnceLock<Vec<Vec<(usize, usize, bool, u128)>>> = OnceLock::new();

/// Access the cached pre-calculated ship placements.
/// Index: length (2..=5) -> Vec<(r, c, horizontal, mask)>
pub fn placements() -> &'static Vec<Vec<(usize, usize, bool, u128)>> {
    ALL_PLACEMENTS.get_or_init(all_placements_by_len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fleet_total() {
        assert_eq!(FLEET_TOTAL, 17);
        assert_eq!(FLEET.len(), 5);
    }

    #[test]
    fn test_ship_mask_horizontal() {
        let m = ship_mask(0, 0, 3, true).unwrap();
        // Pola (0,0), (0,1), (0,2)
        assert_eq!(m & 0x7, 0x7);
        assert_eq!(m.count_ones(), 3);
    }

    #[test]
    fn test_ship_mask_vertical() {
        let m = ship_mask(0, 0, 3, false).unwrap();
        // Pola (0,0), (1,0), (2,0)
        assert_eq!(m.count_ones(), 3);
        assert!(m & 1 != 0);
        assert!(m & (1 << 10) != 0);
        assert!(m & (1 << 20) != 0);
    }

    #[test]
    fn test_ship_mask_out_of_bounds() {
        assert!(ship_mask(0, 8, 3, true).is_none());
        assert!(ship_mask(8, 0, 3, false).is_none());
        assert!(ship_mask(0, 8, 3, false).is_some()); // pionowo ok
        assert!(ship_mask(8, 0, 3, true).is_some()); // poziomo ok
    }

    #[test]
    fn test_all_placements_count() {
        // Dla statku długości 5: poziomo 60 (10 wierszy × 6 startów) + pionowo 60 = 120
        let p5 = all_placements_for_len(5);
        assert_eq!(p5.len(), 120);

        // Długość 4: poziomo 70 + pionowo 70 = 140
        let p4 = all_placements_for_len(4);
        assert_eq!(p4.len(), 140);

        // Długość 3: 80 + 80 = 160
        let p3 = all_placements_for_len(3);
        assert_eq!(p3.len(), 160);

        // Długość 2: 90 + 90 = 180
        let p2 = all_placements_for_len(2);
        assert_eq!(p2.len(), 180);

        // Długość 1: tylko poziomo (10×10 = 100, bo len==1 => pomijamy pionowo)
        let p1 = all_placements_for_len(1);
        assert_eq!(p1.len(), 100);
    }
}
