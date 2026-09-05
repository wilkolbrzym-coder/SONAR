//! Ultra-light helper functions used throughout Sonar.
//!
//! Every function here is `#[inline(always)]` and compiles to a handful of
//! instructions or fewer. They exist to keep hot loops branch-free and
//! readable at the same time.

// ─────────────────────────────────────────────────────────────────────────────
// Coordinate helpers — the board is a flat 100-cell array indexed r*10+c.
// ─────────────────────────────────────────────────────────────────────────────

/// Convert (row, col) → linear cell index in `[0, 100)`.
#[inline(always)]
pub const fn cell_index(r: usize, c: usize) -> usize {
    r * 10 + c
}

/// Convert linear cell index → (row, col).
#[inline(always)]
pub const fn cell_rc(idx: usize) -> (usize, usize) {
    (idx / 10, idx % 10)
}

/// Returns `true` if `(r, c)` is inside the 10×10 board.
#[inline(always)]
pub const fn is_valid_cell(r: usize, c: usize) -> bool {
    r < 10 && c < 10
}

/// Returns `true` if a linear index is inside the board.
#[inline(always)]
pub const fn is_valid_index(idx: usize) -> bool {
    idx < 100
}

// ─────────────────────────────────────────────────────────────────────────────
// Bit helpers — wrap raw u128 intrinsics with intention-revealing names.
// ─────────────────────────────────────────────────────────────────────────────

/// Single-bit mask at `(r, c)`.
#[inline(always)]
pub const fn bit_at(r: usize, c: usize) -> u128 {
    1u128 << cell_index(r, c)
}

/// Single-bit mask at linear index `i`.
#[inline(always)]
pub const fn bit_at_idx(i: usize) -> u128 {
    1u128 << i
}

/// `true` iff bit `i` is set in `mask`.
#[inline(always)]
pub const fn bit_test(mask: u128, i: usize) -> bool {
    (mask & (1u128 << i)) != 0
}

/// Clear the lowest set bit of `mask` and return the new mask.
/// Combined with `trailing_zeros` this is the canonical bit-iteration step.
#[inline(always)]
pub const fn clear_lowest_bit(mask: u128) -> u128 {
    mask & (mask.wrapping_sub(1))
}

/// Index of the lowest set bit (UB if `mask == 0`).
#[inline(always)]
pub fn lowest_bit_index(mask: u128) -> usize {
    mask.trailing_zeros() as usize
}

/// Population count — single `POPCNT` instruction on BMI1.
#[inline(always)]
pub const fn popcount(mask: u128) -> u32 {
    mask.count_ones()
}

// ─────────────────────────────────────────────────────────────────────────────
// Parity helpers — used by the hunt-phase parity heuristic.
// ─────────────────────────────────────────────────────────────────────────────

/// `true` iff `(r + c)` is even — the "checkerboard" parity used to cover
/// all length-2 ships with half the shots.
#[inline(always)]
pub const fn is_parity_cell(r: usize, c: usize) -> bool {
    ((r + c) & 1) == 0
}

/// `true` iff linear index `i` has even parity.
#[inline(always)]
pub const fn is_parity_index(i: usize) -> bool {
    ((i / 10 + i % 10) & 1) == 0
}

// ─────────────────────────────────────────────────────────────────────────────
// Coordinate parsing — accept human input like "A5", "J10", "a1".
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a human coordinate such as `"A5"` or `"J10"` into `(row, col)`.
/// Returns `None` on malformed input.
pub fn parse_coordinate(s: &str) -> Option<(usize, usize)> {
    let s = s.trim();
    let mut chars = s.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    if !('A'..='J').contains(&letter) {
        return None;
    }
    let col = (letter as u8 - b'A') as usize;
    let rest: String = chars.collect();
    let row: usize = rest.parse().ok()?;
    if row < 1 || row > 10 {
        return None;
    }
    Some((row - 1, col))
}

/// Format a `(row, col)` pair as a human coordinate like `"A1"`.
#[inline]
pub fn format_coordinate(r: usize, c: usize) -> String {
    let letter = (b'A' + c as u8) as char;
    format!("{}{}", letter, r + 1)
}

// ─────────────────────────────────────────────────────────────────────────────
// Math helpers — small, branchless, hot.
// ─────────────────────────────────────────────────────────────────────────────

/// Branchless max for `f32`. Uses a full-bit mask to select between `a`
/// and `b` without a branch.
#[inline(always)]
pub fn fast_max_f32(a: f32, b: f32) -> f32 {
    // `a > b` → all-ones mask → select a; else all-zeros → select b.
    let mask = if a > b { u32::MAX } else { 0 };
    let bits = (a.to_bits() & mask) | (b.to_bits() & !mask);
    f32::from_bits(bits)
}

/// Branchless min for `f32`.
#[inline(always)]
pub fn fast_min_f32(a: f32, b: f32) -> f32 {
    let mask = if a < b { u32::MAX } else { 0 };
    let bits = (a.to_bits() & mask) | (b.to_bits() & !mask);
    f32::from_bits(bits)
}

/// Clamp `x` to `[lo, hi]`.
#[inline(always)]
pub fn fast_clamp_f32(x: f32, lo: f32, hi: f32) -> f32 {
    fast_max_f32(lo, fast_min_f32(x, hi))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_index_roundtrip() {
        for r in 0..10 {
            for c in 0..10 {
                let i = cell_index(r, c);
                assert_eq!(cell_rc(i), (r, c));
            }
        }
    }

    #[test]
    fn test_validity() {
        assert!(is_valid_cell(0, 0));
        assert!(is_valid_cell(9, 9));
        assert!(!is_valid_cell(10, 0));
        assert!(!is_valid_cell(0, 10));
        assert!(is_valid_index(0));
        assert!(is_valid_index(99));
        assert!(!is_valid_index(100));
    }

    #[test]
    fn test_bit_helpers() {
        assert_eq!(bit_at(0, 0), 1u128);
        assert_eq!(bit_at(0, 1), 2u128);
        assert_eq!(bit_at(1, 0), 1u128 << 10);
        assert!(bit_test(0b1010u128, 1));
        assert!(!bit_test(0b1010u128, 0));
        assert_eq!(clear_lowest_bit(0b1010u128), 0b1000u128);
        assert_eq!(lowest_bit_index(0b10000u128), 4);
        assert_eq!(popcount(0b1011u128), 3);
    }

    #[test]
    fn test_parity() {
        assert!(is_parity_cell(0, 0));
        assert!(!is_parity_cell(0, 1));
        assert!(is_parity_cell(1, 1));
        assert!(!is_parity_cell(1, 0));
        assert!(is_parity_index(0));
        assert!(!is_parity_index(1));
        assert!(is_parity_index(11)); // (1,1)
    }

    #[test]
    fn test_parse_coordinate() {
        assert_eq!(parse_coordinate("A1"), Some((0, 0)));
        assert_eq!(parse_coordinate("J10"), Some((9, 9)));
        assert_eq!(parse_coordinate("a5"), Some((4, 0)));
        assert_eq!(parse_coordinate("E7"), Some((6, 4)));
        assert_eq!(parse_coordinate("K1"), None); // K out of range
        assert_eq!(parse_coordinate("A0"), None); // 0 out of range
        assert_eq!(parse_coordinate("A11"), None); // 11 out of range
        assert_eq!(parse_coordinate(""), None);
        assert_eq!(parse_coordinate("A"), None);
    }

    #[test]
    fn test_format_coordinate() {
        assert_eq!(format_coordinate(0, 0), "A1");
        assert_eq!(format_coordinate(9, 9), "J10");
        assert_eq!(format_coordinate(4, 4), "E5");
    }

    #[test]
    fn test_fast_math() {
        assert_eq!(fast_max_f32(1.0, 2.0), 2.0);
        assert_eq!(fast_max_f32(2.0, 1.0), 2.0);
        assert_eq!(fast_min_f32(1.0, 2.0), 1.0);
        assert_eq!(fast_min_f32(2.0, 1.0), 1.0);
        assert_eq!(fast_clamp_f32(5.0, 0.0, 10.0), 5.0);
        assert_eq!(fast_clamp_f32(-1.0, 0.0, 10.0), 0.0);
        assert_eq!(fast_clamp_f32(11.0, 0.0, 10.0), 10.0);
    }
}
