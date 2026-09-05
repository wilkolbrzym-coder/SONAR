//! Fleet definition.

/// Ship lengths (standard: 5, 4, 3, 3, 2).
pub const FLEET: &[u8] = &[5, 4, 3, 3, 2];

/// Total number of ship cells.
pub const FLEET_TOTAL: u32 = 5 + 4 + 3 + 3 + 2; // = 17

/// Number of ships.
pub const FLEET_COUNT: usize = FLEET.len();

/// Mask of the cells occupied by a ship of the given length starting at
/// (r, c) in the given orientation. Returns `None` if the ship would
/// extend past the board edge.
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

/// All legal placements of a single ship of the given length (computed
/// once, cached in a static table).
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

/// Pre-computed placements per ship length.
/// Index: length (2..=5) -> Vec<(r, c, horizontal, mask)>
pub fn all_placements_by_len() -> Vec<Vec<(usize, usize, bool, u128)>> {
    let mut out = Vec::with_capacity(6);
    for _ in 0..2 {
        out.push(Vec::new()); // lengths 0 and 1 are unused
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
        // Length 5: horizontal 60 (10 rows × 6 start columns) + vertical 60 = 120
        let p5 = all_placements_for_len(5);
        assert_eq!(p5.len(), 120);

        // Length 4: horizontal 70 + vertical 70 = 140
        let p4 = all_placements_for_len(4);
        assert_eq!(p4.len(), 140);

        // Length 3: 80 + 80 = 160
        let p3 = all_placements_for_len(3);
        assert_eq!(p3.len(), 160);

        // Length 2: 90 + 90 = 180
        let p2 = all_placements_for_len(2);
        assert_eq!(p2.len(), 180);

        // Length 1: horizontal only (10×10 = 100 — vertical is skipped for len == 1)
        let p1 = all_placements_for_len(1);
        assert_eq!(p1.len(), 100);
    }
}
