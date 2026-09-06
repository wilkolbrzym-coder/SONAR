//! Board geometry and **tiered bitboards** for generalised games (0.4).
//!
//! A [`Geometry`] describes a rectangular board `width × height`
//! (5..=30 cells per side), a set of **holes** (cells removed from play —
//! islands, voids, custom maps), and the **torus** flag (edges wrap
//! around: the left column touches the right column, the top row touches
//! the bottom row).
//!
//! Cell masks come in two tiers:
//!
//! - **Narrow tier** (`width × height ≤ 128`): the whole grid lives in a
//!   single `u128` — every operation is 2–3 CPU instructions with no
//!   heap. This is the same speed class as the classic 10×10 engine.
//! - **Wide tier** (larger boards, up to 30×30 = 900 cells): one `u64`
//!   word per row (`width ≤ 64`), with hot operations — 8-neighbour
//!   dilation and batch legality filtering — routed through the
//!   multi-ISA SIMD kernels of the `sonar-simd` crate (AVX2 / AVX-512 /
//!   NEON / wasm128 with runtime dispatch).
//!
//! Both tiers implement the same [`BitGrid`] operations, so the
//! generalised engine (see `general`) is tier-agnostic.

use serde::{Deserialize, Serialize};

/// Maximum board side supported by the generalised engine.
pub const MAX_SIDE: usize = 30;
/// Maximum board side of the narrow tier (cells ≤ 128).
pub const NARROW_MAX_SIDE: usize = 11;

// ─────────────────────────────────────────────────────────────────────────────
// Geometry
// ─────────────────────────────────────────────────────────────────────────────

/// The geometry of a generalised board: size, holes, topology.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Geometry {
    /// Board width (columns) — `5..=30`.
    pub width: usize,
    /// Board height (rows) — `5..=30`.
    pub height: usize,
    /// Cells removed from play (islands / voids). Coordinates must be
    /// in bounds; duplicates are tolerated and ignored.
    pub holes: Vec<(usize, usize)>,
    /// Torus topology: edges wrap (left↔right, top↔bottom).
    #[serde(default)]
    pub torus: bool,
}

impl Geometry {
    /// A plain rectangular board.
    pub fn rectangle(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            holes: Vec::new(),
            torus: false,
        }
    }

    /// Validate the geometry. Returns `Err` with a human-readable message.
    pub fn validate(&self) -> Result<(), String> {
        if self.width < 5 || self.width > MAX_SIDE {
            return Err(format!(
                "width must be in 5..={MAX_SIDE}, got {}",
                self.width
            ));
        }
        if self.height < 5 || self.height > MAX_SIDE {
            return Err(format!(
                "height must be in 5..={MAX_SIDE}, got {}",
                self.height
            ));
        }
        for &(r, c) in &self.holes {
            if r >= self.height || c >= self.width {
                return Err(format!(
                    "hole ({}, {}) outside the {}×{} board",
                    r, c, self.height, self.width
                ));
            }
        }
        Ok(())
    }

    /// Total number of cells (holes included).
    pub fn cells(&self) -> usize {
        self.width * self.height
    }

    /// Number of playable (non-hole) cells.
    pub fn open_cells(&self) -> usize {
        self.cells() - self.holes.len()
    }

    /// Is `(r, c)` inside the board and not a hole?
    pub fn is_open(&self, r: usize, c: usize) -> bool {
        r < self.height && c < self.width && !self.holes.contains(&(r, c))
    }

    /// Linear cell index: `r * width + c`.
    #[inline(always)]
    pub fn idx(&self, r: usize, c: usize) -> usize {
        r * self.width + c
    }

    /// Decode a linear index into `(r, c)`.
    #[inline(always)]
    pub fn rc(&self, i: usize) -> (usize, usize) {
        (i / self.width, i % self.width)
    }

    /// Is this geometry in the narrow tier (single `u128`)?
    pub fn is_narrow(&self) -> bool {
        self.cells() <= 128
    }

    /// The neighbour of `(r, c)` in direction `(dr, dc)` ∈ {−1, 0, 1}²,
    /// honouring the topology. Returns `None` when the neighbour falls
    /// off a non-torus board, or when the cell itself is out of bounds.
    #[inline]
    pub fn neighbor(&self, r: usize, c: usize, dr: i32, dc: i32) -> Option<(usize, usize)> {
        if r >= self.height || c >= self.width {
            return None;
        }
        let nr = wrap_coord(r as i32 + dr, self.height, self.torus)?;
        let nc = wrap_coord(c as i32 + dc, self.width, self.torus)?;
        Some((nr, nc))
    }

    /// The 8 (Moore) neighbours of `(r, c)` that are on the board.
    /// Holes are NOT filtered here — callers apply hole masks as needed.
    pub fn neighbors8(&self, r: usize, c: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::with_capacity(8);
        for dr in -1i32..=1 {
            for dc in -1i32..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                if let Some(n) = self.neighbor(r, c, dr, dc) {
                    out.push(n);
                }
            }
        }
        out
    }

    /// The 4 (von Neumann) neighbours of `(r, c)` that are on the board.
    pub fn neighbors4(&self, r: usize, c: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::with_capacity(4);
        for (dr, dc) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            if let Some(n) = self.neighbor(r, c, dr, dc) {
                out.push(n);
            }
        }
        out
    }

    /// The mask of every in-bounds cell (holes included — apply
    /// [`Geometry::hole_mask`] to exclude them).
    pub fn board_mask_grid(&self) -> BitGrid {
        if self.is_narrow() {
            let bits: u128 = if self.cells() >= 128 {
                u128::MAX
            } else {
                (1u128 << self.cells()) - 1
            };
            BitGrid::Narrow(bits)
        } else {
            let row: u64 = if self.width == 64 {
                u64::MAX
            } else {
                (1u64 << self.width) - 1
            };
            BitGrid::Wide(vec![row; self.height])
        }
    }

    /// The mask of hole cells.
    pub fn hole_mask_grid(&self) -> BitGrid {
        let mut g = BitGrid::empty(self);
        for &(r, c) in &self.holes {
            g.set(self, r, c);
        }
        g
    }

    /// The mask of playable cells (board minus holes).
    pub fn open_mask_grid(&self) -> BitGrid {
        self.board_mask_grid().diff(&self.hole_mask_grid())
    }
}

/** Wrap a coordinate around a side of length `len` (or reject on a
 * non-torus overflow).
 */
#[inline]
fn wrap_coord(v: i32, len: usize, torus: bool) -> Option<usize> {
    let n = len as i32;
    if (0..n).contains(&v) {
        return Some(v as usize);
    }
    if !torus {
        return None;
    }
    // One step out of range (|dr|, |dc| ≤ 1): wrap via Euclidean modulo.
    Some(v.rem_euclid(n) as usize)
}

// ─────────────────────────────────────────────────────────────────────────────
// BitGrid — the tiered bitboard
// ─────────────────────────────────────────────────────────────────────────────

/// A cell mask over a [`Geometry`], in one of two tiers.
///
/// Construction via [`BitGrid::empty`] / [`BitGrid::full`] chooses the
/// tier automatically from the geometry. Operations are total: bits
/// outside the board are never set, so AND/OR/XOR/NOT stay canonical.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BitGrid {
    /// Narrow tier: the whole grid in the low `width*height` bits of a
    /// `u128` (cells ≤ 128 — boards up to 11×11).
    Narrow(u128),
    /// Wide tier: one `u64` word per row (bit `c` = cell `(r, c)`).
    /// Hot ops dispatch to the `sonar-simd` kernels.
    Wide(Vec<u64>),
}

impl BitGrid {
    /// The empty grid for this geometry.
    pub fn empty(g: &Geometry) -> Self {
        if g.is_narrow() {
            BitGrid::Narrow(0)
        } else {
            BitGrid::Wide(vec![0u64; g.height])
        }
    }

    /// The full-board grid (holes included) for this geometry.
    pub fn full(g: &Geometry) -> Self {
        g.board_mask_grid()
    }

    /// Set `(r, c)`. No-op when out of bounds.
    pub fn set(&mut self, g: &Geometry, r: usize, c: usize) {
        if r >= g.height || c >= g.width {
            return;
        }
        match self {
            BitGrid::Narrow(bits) => *bits |= 1u128 << g.idx(r, c),
            BitGrid::Wide(words) => words[r] |= 1u64 << c,
        }
    }

    /// Test `(r, c)`. `false` when out of bounds.
    #[inline]
    pub fn test(&self, g: &Geometry, r: usize, c: usize) -> bool {
        if r >= g.height || c >= g.width {
            return false;
        }
        match self {
            BitGrid::Narrow(bits) => (*bits >> g.idx(r, c)) & 1 == 1,
            BitGrid::Wide(words) => (words[r] >> c) & 1 == 1,
        }
    }

    /// Clear `(r, c)`.
    pub fn clear(&mut self, g: &Geometry, r: usize, c: usize) {
        if r >= g.height || c >= g.width {
            return;
        }
        match self {
            BitGrid::Narrow(bits) => *bits &= !(1u128 << g.idx(r, c)),
            BitGrid::Wide(words) => words[r] &= !(1u64 << c),
        }
    }

    /// Number of set cells.
    pub fn popcount(&self, _g: &Geometry) -> u32 {
        match self {
            BitGrid::Narrow(bits) => bits.count_ones(),
            BitGrid::Wide(words) => {
                // The SIMD kernel dispatches to the best backend.
                sonar_simd::popcount_words(words) as u32
            }
        }
    }

    /// Is the grid empty?
    pub fn is_empty(&self) -> bool {
        match self {
            BitGrid::Narrow(bits) => *bits == 0,
            BitGrid::Wide(words) => words.iter().all(|&w| w == 0),
        }
    }

    /// Bitwise AND (union of constraints).
    pub fn and(&self, o: &BitGrid) -> BitGrid {
        match (self, o) {
            (BitGrid::Narrow(a), BitGrid::Narrow(b)) => BitGrid::Narrow(a & b),
            (BitGrid::Wide(a), BitGrid::Wide(b)) => {
                BitGrid::Wide(a.iter().zip(b.iter()).map(|(&x, &y)| x & y).collect())
            }
            // Tier mismatch cannot happen for grids of the same geometry;
            // degrade gracefully by treating the narrow side as empty-typed.
            _ => BitGrid::Narrow(0),
        }
    }

    /// Bitwise OR.
    pub fn or(&self, o: &BitGrid) -> BitGrid {
        match (self, o) {
            (BitGrid::Narrow(a), BitGrid::Narrow(b)) => BitGrid::Narrow(a | b),
            (BitGrid::Wide(a), BitGrid::Wide(b)) => {
                BitGrid::Wide(a.iter().zip(b.iter()).map(|(&x, &y)| x | y).collect())
            }
            _ => BitGrid::Narrow(0),
        }
    }

    /// Bitwise difference `self & !o`.
    pub fn diff(&self, o: &BitGrid) -> BitGrid {
        match (self, o) {
            (BitGrid::Narrow(a), BitGrid::Narrow(b)) => BitGrid::Narrow(a & !b),
            (BitGrid::Wide(a), BitGrid::Wide(b)) => {
                BitGrid::Wide(a.iter().zip(b.iter()).map(|(&x, &y)| x & !y).collect())
            }
            _ => BitGrid::Narrow(0),
        }
    }

    /// Iterate the set cells as `(r, c)` in row-major order.
    pub fn iter_cells(&self, g: &Geometry) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        match self {
            BitGrid::Narrow(bits) => {
                let mut m = *bits;
                while m != 0 {
                    let i = m.trailing_zeros() as usize;
                    out.push((i / g.width, i % g.width));
                    m &= m - 1;
                }
            }
            BitGrid::Wide(words) => {
                for (r, &w) in words.iter().enumerate() {
                    let mut m = w;
                    while m != 0 {
                        let c = m.trailing_zeros() as usize;
                        out.push((r, c));
                        m &= m - 1;
                    }
                }
            }
        }
        out
    }

    /// The first set cell, or `None`.
    pub fn first_cell(&self, g: &Geometry) -> Option<(usize, usize)> {
        match self {
            BitGrid::Narrow(bits) => {
                if *bits == 0 {
                    None
                } else {
                    let i = bits.trailing_zeros() as usize;
                    Some((i / g.width, i % g.width))
                }
            }
            BitGrid::Wide(words) => {
                for (r, &w) in words.iter().enumerate() {
                    if w != 0 {
                        return Some((r, w.trailing_zeros() as usize));
                    }
                }
                None
            }
        }
    }

    /// 8-neighbour dilation (Moore neighbourhood), topology-aware.
    ///
    /// The wide tier dispatches to the SIMD kernel; the narrow tier uses
    /// direct `u128` neighbour OR-ing. Holes are the caller's business
    /// (apply [`BitGrid::and`] with `open_mask` afterwards when needed).
    pub fn dilate8(&self, g: &Geometry) -> BitGrid {
        match self {
            BitGrid::Narrow(bits) => {
                let mut out = *bits;
                for r in 0..g.height {
                    for c in 0..g.width {
                        if (*bits >> g.idx(r, c)) & 1 == 1 {
                            for (nr, nc) in g.neighbors8(r, c) {
                                out |= 1u128 << g.idx(nr, nc);
                            }
                        }
                    }
                }
                BitGrid::Narrow(out)
            }
            BitGrid::Wide(words) => {
                let mut out = vec![0u64; g.height];
                // The SIMD kernel handles the torus wrap; hole masking is
                // applied by the caller.
                sonar_simd::dilate8_words(words, &mut out, g.width as u32, g.torus);
                BitGrid::Wide(out)
            }
        }
    }

    /// 4-neighbour dilation (von Neumann), topology-aware.
    pub fn dilate4(&self, g: &Geometry) -> BitGrid {
        match self {
            BitGrid::Narrow(bits) => {
                let mut out = *bits;
                for r in 0..g.height {
                    for c in 0..g.width {
                        if (*bits >> g.idx(r, c)) & 1 == 1 {
                            for (nr, nc) in g.neighbors4(r, c) {
                                out |= 1u128 << g.idx(nr, nc);
                            }
                        }
                    }
                }
                BitGrid::Narrow(out)
            }
            BitGrid::Wide(_) => {
                // Compose from dilate8: dilate4(x) ⊆ dilate8(x); filter via
                // per-cell neighbour counting would be slower than the
                // direct loop below, which is fine for the sizes involved.
                let mut out = self.clone();
                let cells = self.iter_cells(g);
                for (r, c) in cells {
                    for (nr, nc) in g.neighbors4(r, c) {
                        out.set(g, nr, nc);
                    }
                }
                out
            }
        }
    }

    /// Convert to the row-word representation (one `u64` per row) used
    /// by the SIMD kernels. For the narrow tier this materialises the
    /// wide layout on demand.
    pub fn to_row_words(&self, g: &Geometry) -> Vec<u64> {
        match self {
            BitGrid::Narrow(bits) => {
                let mut words = vec![0u64; g.height];
                for (r, w) in words.iter_mut().enumerate() {
                    for c in 0..g.width {
                        if (*bits >> g.idx(r, c)) & 1 == 1 {
                            *w |= 1u64 << c;
                        }
                    }
                }
                words
            }
            BitGrid::Wide(words) => words.clone(),
        }
    }

    /// Rebuild from the row-word representation.
    pub fn from_row_words(words: &[u64], g: &Geometry) -> BitGrid {
        if g.is_narrow() {
            let mut bits = 0u128;
            for (r, &w) in words.iter().enumerate().take(g.height) {
                for c in 0..g.width {
                    if (w >> c) & 1 == 1 {
                        bits |= 1u128 << g.idx(r, c);
                    }
                }
            }
            BitGrid::Narrow(bits)
        } else {
            BitGrid::Wide(words[..g.height.min(words.len())].to_vec())
        }
    }

    /// Serialise as a hex string (row-major bits, lowest bit = (0,0)),
    /// for the JSON protocol and snapshots.
    pub fn to_hex(&self, g: &Geometry) -> String {
        match self {
            BitGrid::Narrow(bits) => format!("{:032x}", bits),
            BitGrid::Wide(words) => {
                let mut s = String::with_capacity(words.len() * 16);
                for w in words.iter().take(g.height) {
                    s.push_str(&format!("{:016x}", w));
                }
                s
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geometry_validation() {
        assert!(Geometry::rectangle(10, 10).validate().is_ok());
        assert!(Geometry::rectangle(5, 30).validate().is_ok());
        assert!(Geometry::rectangle(4, 10).validate().is_err());
        assert!(Geometry::rectangle(31, 10).validate().is_err());
        let g = Geometry {
            holes: vec![(10, 10)],
            ..Geometry::rectangle(10, 10)
        };
        assert!(g.validate().is_err());
    }

    #[test]
    fn test_tier_selection() {
        assert!(Geometry::rectangle(10, 10).is_narrow());
        assert!(Geometry::rectangle(11, 11).is_narrow());
        assert!(!Geometry::rectangle(12, 12).is_narrow());
        assert!(!Geometry::rectangle(30, 30).is_narrow());
    }

    #[test]
    fn test_set_test_clear_both_tiers() {
        for g in [Geometry::rectangle(8, 8), Geometry::rectangle(16, 16)] {
            let mut b = BitGrid::empty(&g);
            b.set(&g, 0, 0);
            b.set(&g, g.height - 1, g.width - 1);
            b.set(&g, 3, 4);
            assert!(b.test(&g, 0, 0));
            assert!(b.test(&g, g.height - 1, g.width - 1));
            assert!(b.test(&g, 3, 4));
            assert!(!b.test(&g, 3, 5));
            assert_eq!(b.popcount(&g), 3);
            b.clear(&g, 3, 4);
            assert!(!b.test(&g, 3, 4));
            assert_eq!(b.popcount(&g), 2);
            // Out-of-bounds are ignored.
            b.set(&g, 99, 99);
            assert_eq!(b.popcount(&g), 2);
        }
    }

    #[test]
    fn test_dilate8_flat() {
        for g in [Geometry::rectangle(9, 9), Geometry::rectangle(15, 15)] {
            let mut b = BitGrid::empty(&g);
            b.set(&g, 7, 7);
            let d = b.dilate8(&g);
            assert_eq!(d.popcount(&g), 9); // centre + 8 neighbours
        }
    }

    #[test]
    fn test_dilate8_torus_both_tiers() {
        for g in [
            Geometry {
                torus: true,
                ..Geometry::rectangle(8, 8)
            },
            Geometry {
                torus: true,
                ..Geometry::rectangle(16, 16)
            },
        ] {
            let mut b = BitGrid::empty(&g);
            b.set(&g, 0, 0);
            let d = b.dilate8(&g);
            // On a torus every cell has exactly 8 neighbours.
            assert_eq!(d.popcount(&g), 9, "torus dilate must stay symmetric");
            assert!(d.test(&g, g.height - 1, 0)); // wrap up
            assert!(d.test(&g, 0, g.width - 1)); // wrap left
            assert!(d.test(&g, g.height - 1, g.width - 1)); // diagonal wrap
        }
    }

    #[test]
    fn test_dilate8_no_wrap_flat_board() {
        let g = Geometry::rectangle(10, 10);
        let mut b = BitGrid::empty(&g);
        b.set(&g, 0, 0);
        let d = b.dilate8(&g);
        assert_eq!(d.popcount(&g), 4); // corner + 3 neighbours
    }

    #[test]
    fn test_holes() {
        let g = Geometry {
            holes: vec![(5, 5), (5, 6), (6, 5)],
            ..Geometry::rectangle(10, 10)
        };
        assert_eq!(g.open_cells(), 97);
        let open = g.open_mask_grid();
        assert_eq!(open.popcount(&g), 97);
        assert!(!open.test(&g, 5, 5));
        assert!(open.test(&g, 5, 7));
        // Dilation may bleed into holes; callers mask them. The masked
        // result never contains the hole cells themselves.
        let mut b = BitGrid::empty(&g);
        b.set(&g, 5, 5);
        let d = b.dilate8(&g).and(&open);
        assert!(!d.test(&g, 5, 5));
        assert!(!d.test(&g, 5, 6));
        assert!(!d.test(&g, 6, 5));
        // But legal neighbours of the hole survive the mask.
        assert!(d.test(&g, 4, 5));
        assert!(d.test(&g, 5, 4));
    }

    #[test]
    fn test_row_words_roundtrip() {
        for g in [Geometry::rectangle(7, 9), Geometry::rectangle(20, 25)] {
            let mut b = BitGrid::empty(&g);
            for r in 0..g.height {
                for c in 0..g.width {
                    if (r + c) % 3 == 0 {
                        b.set(&g, r, c);
                    }
                }
            }
            let words = b.to_row_words(&g);
            assert_eq!(words.len(), g.height);
            let back = BitGrid::from_row_words(&words, &g);
            assert_eq!(back, b);
        }
    }

    #[test]
    fn test_hex_serialization() {
        let g = Geometry::rectangle(10, 10);
        let mut b = BitGrid::empty(&g);
        b.set(&g, 0, 0);
        let h = b.to_hex(&g);
        assert_eq!(h.len(), 32); // u128 as 32 hex chars
        assert!(h.ends_with('1'));

        let g2 = Geometry::rectangle(20, 20);
        let b2 = BitGrid::empty(&g2);
        assert_eq!(b2.to_hex(&g2).len(), 20 * 16);
    }

    #[test]
    fn test_iter_cells_row_major() {
        let g = Geometry::rectangle(12, 12);
        let mut b = BitGrid::empty(&g);
        b.set(&g, 3, 5);
        b.set(&g, 1, 2);
        let cells = b.iter_cells(&g);
        assert_eq!(cells, vec![(1, 2), (3, 5)]); // row-major order
    }

    #[test]
    fn test_diff_and_or() {
        let g = Geometry::rectangle(30, 30);
        let mut a = BitGrid::empty(&g);
        let mut b = BitGrid::empty(&g);
        a.set(&g, 1, 1);
        a.set(&g, 2, 2);
        b.set(&g, 2, 2);
        b.set(&g, 3, 3);
        assert_eq!(a.and(&b).popcount(&g), 1);
        assert_eq!(a.or(&b).popcount(&g), 3);
        assert_eq!(a.diff(&b).popcount(&g), 1);
    }

    #[test]
    fn test_dilate4() {
        let g = Geometry::rectangle(9, 9);
        let mut b = BitGrid::empty(&g);
        b.set(&g, 4, 4);
        assert_eq!(b.dilate4(&g).popcount(&g), 5); // centre + 4 neighbours

        let g2 = Geometry::rectangle(20, 20);
        let mut b2 = BitGrid::empty(&g2);
        b2.set(&g2, 10, 10);
        assert_eq!(b2.dilate4(&g2).popcount(&g2), 5);
    }

    #[test]
    fn test_neighbor_wrap() {
        let flat = Geometry::rectangle(10, 10);
        assert_eq!(flat.neighbor(0, 0, -1, 0), None);
        assert_eq!(flat.neighbor(9, 9, 1, 1), None);
        assert_eq!(flat.neighbor(5, 5, 1, 1), Some((6, 6)));

        let torus = Geometry {
            torus: true,
            ..Geometry::rectangle(10, 10)
        };
        assert_eq!(torus.neighbor(0, 0, -1, 0), Some((9, 0)));
        assert_eq!(torus.neighbor(0, 0, 0, -1), Some((0, 9)));
        assert_eq!(torus.neighbor(9, 9, 1, 1), Some((0, 0)));
    }
}
