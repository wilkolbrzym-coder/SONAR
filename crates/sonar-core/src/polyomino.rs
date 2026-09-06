//! Polyomino ship shapes (0.4) — fleets beyond straight lines.
//!
//! A [`ShipShape`] is a set of cells (a polyomino) defined in its own
//! local coordinate frame, normalised so the minimum row and column are
//! 0. Shapes must be **4-connected** ( Battleship semantics: a ship is
//! one contiguous piece) and non-empty.
//!
//! Classic straight ships are the degenerate case: [`ShipShape::line`].
//! Arbitrary shapes — L-trominoes, T-tetrominoes, S-pieces, plus-shaped
//! "carrier" variants — are first-class citizens:
//!
//! ```ignore
//! use sonar::polyomino::ShipShape;
//!
//! let elle = ShipShape::from_cells(&[(0, 0), (1, 0), (1, 1)]).unwrap();
//! assert_eq!(elle.cell_count(), 3);
//! assert_eq!(elle.distinct_variants(), 4); // L + J under rotations (mirror dupes)
//! ```
//!
//! Placement enumeration (which translations of which variants fit a
//! given geometry) lives in `general`; this module owns shape algebra:
//! normalisation, rotation, reflection, and variant deduplication.

use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// ShipShape
// ─────────────────────────────────────────────────────────────────────────────

/// A polyomino ship shape in local (normalised) coordinates.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ShipShape {
    /// Human-readable name (used by the JSON protocol and docs).
    pub name: String,
    /// The occupied cells, normalised so `min(r) == 0 && min(c) == 0`,
    /// sorted row-major.
    pub cells: Vec<(i32, i32)>,
}

impl ShipShape {
    /// A straight ship of `len` cells (the classic kind).
    pub fn line(len: u8) -> Self {
        let cells: Vec<(i32, i32)> = (0..len as i32).map(|i| (0, i)).collect();
        Self {
            name: format!("line-{}", len),
            cells,
        }
    }

    /// Build a shape from arbitrary cells. `Err` when the cells are
    /// empty, duplicated, disconnected (4-neighbour), or beyond the
    /// size cap.
    pub fn from_cells(cells: &[(i32, i32)]) -> Result<Self, String> {
        if cells.is_empty() {
            return Err("shape must have at least one cell".to_string());
        }
        if cells.len() > 8 {
            return Err(format!("shape too large ({} cells, max 8)", cells.len()));
        }
        if cells.iter().any(|&(r, c)| r < 0 || c < 0) {
            return Err("shape cells must be non-negative".to_string());
        }
        let mut set = std::collections::BTreeSet::new();
        for &c in cells {
            if !set.insert(c) {
                return Err(format!("duplicate cell {:?}", c));
            }
        }
        if !is_connected(&set) {
            return Err("shape cells must be 4-connected".to_string());
        }
        let norm = normalise(cells);
        Ok(Self {
            name: format!("poly-{}", norm.len()),
            cells: norm,
        })
    }

    /// A named shape (convenience wrapper around [`ShipShape::from_cells`]).
    pub fn named(name: &str, cells: &[(i32, i32)]) -> Result<Self, String> {
        let mut s = Self::from_cells(cells)?;
        s.name = name.to_string();
        Ok(s)
    }

    /// Number of occupied cells (the "length" — hits needed to sink).
    pub fn cell_count(&self) -> u8 {
        self.cells.len() as u8
    }

    /// Bounding box `(height, width)`.
    pub fn bounding_box(&self) -> (usize, usize) {
        let max_r = self.cells.iter().map(|&(r, _)| r).max().unwrap_or(0);
        let max_c = self.cells.iter().map(|&(_, c)| c).max().unwrap_or(0);
        ((max_r + 1) as usize, (max_c + 1) as usize)
    }

    /// Rotate 90° clockwise: `(r, c) → (c, max_r − r)`, then re-normalise.
    pub fn rotate90(&self) -> ShipShape {
        let max_r = self.cells.iter().map(|&(r, _)| r).max().unwrap_or(0);
        let rotated: Vec<(i32, i32)> = self.cells.iter().map(|&(r, c)| (c, max_r - r)).collect();
        ShipShape {
            name: self.name.clone(),
            cells: normalise(&rotated),
        }
    }

    /// Mirror horizontally: `(r, c) → (r, max_c − c)`.
    pub fn reflect(&self) -> ShipShape {
        let max_c = self.cells.iter().map(|&(_, c)| c).max().unwrap_or(0);
        let mirrored: Vec<(i32, i32)> = self.cells.iter().map(|&(r, c)| (r, max_c - c)).collect();
        ShipShape {
            name: self.name.clone(),
            cells: normalise(&mirrored),
        }
    }

    /// All distinct variants under the dihedral group (rotations +
    /// reflections), deduplicated by canonical cell set. The original
    /// is always the first entry.
    pub fn distinct_variants(&self) -> Vec<ShipShape> {
        let mut variants = Vec::new();
        let mut current = self.clone();
        for _ in 0..4 {
            let reflected = current.reflect();
            for candidate in [&current, &reflected] {
                if !variants
                    .iter()
                    .any(|v: &ShipShape| v.cells == candidate.cells)
                {
                    variants.push(candidate.clone());
                }
            }
            current = current.rotate90();
        }
        variants
    }

    /// Preset fleets used by the variant server / web app.
    pub fn preset_l_fleet() -> Vec<ShipShape> {
        vec![
            ShipShape::named("L5", &[(0, 0), (1, 0), (2, 0), (2, 1), (2, 2)])
                .unwrap_or_else(|_| ShipShape::line(5)),
            ShipShape::named("T4", &[(0, 0), (0, 1), (0, 2), (1, 1)])
                .unwrap_or_else(|_| ShipShape::line(4)),
            ShipShape::named("S4", &[(0, 1), (0, 2), (1, 0), (1, 1)])
                .unwrap_or_else(|_| ShipShape::line(4)),
            ShipShape::named("L3", &[(0, 0), (1, 0), (1, 1)])
                .unwrap_or_else(|_| ShipShape::line(3)),
            ShipShape::named("O2", &[(0, 0), (0, 1)]).unwrap_or_else(|_| ShipShape::line(2)),
        ]
    }
}

/// Translate cells so the minimum row and column are 0; sort row-major.
fn normalise(cells: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let min_r = cells.iter().map(|&(r, _)| r).min().unwrap_or(0);
    let min_c = cells.iter().map(|&(_, c)| c).min().unwrap_or(0);
    let mut out: Vec<(i32, i32)> = cells.iter().map(|&(r, c)| (r - min_r, c - min_c)).collect();
    out.sort_unstable();
    out
}

/// 4-connectivity check via flood fill from the first cell.
fn is_connected(set: &std::collections::BTreeSet<(i32, i32)>) -> bool {
    let start = match set.iter().next() {
        Some(&s) => s,
        None => return false,
    };
    let mut visited = std::collections::BTreeSet::new();
    let mut stack = vec![start];
    visited.insert(start);
    while let Some((r, c)) = stack.pop() {
        for n in [(r - 1, c), (r + 1, c), (r, c - 1), (r, c + 1)] {
            if set.contains(&n) && !visited.contains(&n) {
                visited.insert(n);
                stack.push(n);
            }
        }
    }
    visited.len() == set.len()
}

// ─────────────────────────────────────────────────────────────────────────────
// ShipSpec — how a fleet describes its ships
// ─────────────────────────────────────────────────────────────────────────────

/// One ship of a generalised fleet: either a straight line or an
/// arbitrary polyomino shape.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum ShipSpec {
    /// A straight ship of `len` cells.
    Line {
        /// Ship length.
        len: u8,
    },
    /// A polyomino shape (cells in local coordinates).
    Shape {
        /// The shape's cells.
        cells: Vec<(i32, i32)>,
        /// Optional display name.
        #[serde(default)]
        name: Option<String>,
    },
}

impl ShipSpec {
    /// Resolve to a [`ShipShape`]; `Err` for invalid shapes.
    pub fn to_shape(&self) -> Result<ShipShape, String> {
        match self {
            ShipSpec::Line { len } => {
                if *len == 0 || *len > 30 {
                    Err(format!("line length {} out of range 1..=30", len))
                } else {
                    Ok(ShipShape::line(*len))
                }
            }
            ShipSpec::Shape { cells, name } => {
                let mut shape = ShipShape::from_cells(cells)?;
                if let Some(n) = name {
                    shape.name = n.clone();
                }
                Ok(shape)
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
    fn test_line_shape() {
        let s = ShipShape::line(5);
        assert_eq!(s.cell_count(), 5);
        assert_eq!(s.cells, vec![(0, 0), (0, 1), (0, 2), (0, 3), (0, 4)]);
        assert_eq!(s.bounding_box(), (1, 5));
    }

    #[test]
    fn test_from_cells_validation() {
        assert!(ShipShape::from_cells(&[(0, 0), (0, 1)]).is_ok());
        // Disconnected.
        assert!(ShipShape::from_cells(&[(0, 0), (2, 0)]).is_err());
        // Duplicate.
        assert!(ShipShape::from_cells(&[(0, 0), (0, 0)]).is_err());
        // Empty.
        assert!(ShipShape::from_cells(&[]).is_err());
        // Negative.
        assert!(ShipShape::from_cells(&[(0, 0), (0, -1)]).is_err());
        // Diagonal-only adjacency is NOT 4-connected.
        assert!(ShipShape::from_cells(&[(0, 0), (1, 1)]).is_err());
    }

    #[test]
    fn test_normalisation() {
        let s = ShipShape::from_cells(&[(3, 7), (4, 7), (4, 8)]).unwrap();
        assert_eq!(s.cells, vec![(0, 0), (1, 0), (1, 1)]);
    }

    #[test]
    fn test_rotation() {
        // L3: (0,0),(1,0),(1,1) rotated 90° CW → (0,0),(0,1),(1,0)... let's verify
        // via the property that 4 rotations restore the original.
        let s = ShipShape::from_cells(&[(0, 0), (1, 0), (1, 1)]).unwrap();
        let r1 = s.rotate90();
        let r2 = r1.rotate90();
        let r3 = r2.rotate90();
        let r4 = r3.rotate90();
        assert_eq!(r4, s);
        assert_eq!(r1.cell_count(), 3);
        assert_ne!(r1, s);
        // The rotate-90 of L3 (h=2,w=2) swaps the bounding box.
        assert_eq!(r1.bounding_box(), (2, 2));
    }

    #[test]
    fn test_reflection_involution() {
        let s = ShipShape::from_cells(&[(0, 0), (1, 0), (1, 1)]).unwrap();
        assert_eq!(s.reflect().reflect(), s);
    }

    #[test]
    fn test_distinct_variants_line() {
        // A straight line has 2 distinct variants (horizontal, vertical).
        let s = ShipShape::line(4);
        let v = s.distinct_variants();
        assert_eq!(v.len(), 2, "variants: {:?}", v);
    }

    #[test]
    fn test_distinct_variants_l() {
        // An L-tromino has 4 distinct variants under rotations
        // (reflections coincide with rotations for L3? no — L and J are
        // mirror images; rotations cover all 4 corners, and mirroring an
        // L gives a J which equals some rotation of L. So: 4).
        let s = ShipShape::from_cells(&[(0, 0), (1, 0), (1, 1)]).unwrap();
        assert_eq!(s.distinct_variants().len(), 4);
    }

    #[test]
    fn test_distinct_variants_asymmetric() {
        // A fully asymmetric pentomino (e.g. the "F" shape) has all 8
        // dihedral variants.
        let s = ShipShape::from_cells(&[(0, 1), (0, 2), (1, 0), (1, 1), (2, 1)]).unwrap();
        assert_eq!(
            s.distinct_variants().len(),
            8,
            "F pentomino must have 8 variants"
        );
    }

    #[test]
    fn test_symmetric_shape_fewer_variants() {
        // The square O-tetromino has exactly 1 variant.
        let s = ShipShape::from_cells(&[(0, 0), (0, 1), (1, 0), (1, 1)]).unwrap();
        assert_eq!(s.distinct_variants().len(), 1);
        // The T-tetromino has 4.
        let t = ShipShape::from_cells(&[(0, 0), (0, 1), (0, 2), (1, 1)]).unwrap();
        assert_eq!(t.distinct_variants().len(), 4);
    }

    #[test]
    fn test_ship_spec_roundtrip() {
        let spec = ShipSpec::Line { len: 3 };
        assert_eq!(spec.to_shape().unwrap(), ShipShape::line(3));
        let spec = ShipSpec::Shape {
            cells: vec![(0, 0), (1, 0), (1, 1)],
            name: Some("L3".to_string()),
        };
        let shape = spec.to_shape().unwrap();
        assert_eq!(shape.name, "L3");
        assert_eq!(shape.cell_count(), 3);
        // Serialization round-trips.
        let json = serde_json::to_string(&spec).unwrap();
        let back: ShipSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn test_ship_spec_invalid() {
        assert!(ShipSpec::Line { len: 0 }.to_shape().is_err());
        assert!(ShipSpec::Line { len: 31 }.to_shape().is_err());
        assert!(
            ShipSpec::Shape {
                cells: vec![(0, 0), (5, 5)],
                name: None
            }
            .to_shape()
            .is_err()
        );
    }

    #[test]
    fn test_preset_l_fleet() {
        let fleet = ShipShape::preset_l_fleet();
        assert_eq!(fleet.len(), 5);
        let total: u8 = fleet.iter().map(|s| s.cell_count()).sum();
        assert_eq!(total, 18); // 5 + 4 + 4 + 3 + 2
    }
}
