//! Exact integer lattice identity for shared geometry (v28a).
//!
//! Was: `corner_key(x, z)` via float division + `round()`; it worked,
//! but identity rested on epsilon luck (cos(30 degrees) in f32 != sqrt3/2 in f32).
//! Now: a doubled lattice with integer coordinates. Tile centre (q,r):
//! `X = 4q + 2r, Z = 6r`; corners/edges are integer offsets (checked against
//! trigonometry in tests). No sqrt/round/epsilon in producers.
//! Float->key (`corner_key`) remains only as a lookup shim for consumers
//! with world coordinates (heightfield sampling, probe tests): the inputs there
//! always come from exact points, round is stable.
//!
//! Units: X in quanta of sqrt3/4*HEX_SIZE, Z in 0.25*HEX_SIZE. (The old
//! `corner_key` gave half values; all constructors are only
//! from here, there are no raw key literals in the code.)

use crate::scene::layout::HEX_SIZE;

/// Canonical key of a shared point (a corner or an edge midpoint).
pub type LatticeKey = (i32, i32);

/// Tile centre in lattice units.
pub fn tile_lattice(q: i32, r: i32) -> LatticeKey {
    (4 * q + 2 * r, 6 * r)
}

/// Offsets of 6 corners (corner i at 60*i - 30 degrees, order = hex_corners/EDGE_NEIGHBORS).
pub const CORNER_OFFSETS: [(i32, i32); 6] = [(2, -2), (2, 2), (0, 4), (-2, 2), (-2, -2), (0, -4)];

/// Offsets of edge midpoints (edge i is between corners i and i+1).
pub const EDGE_OFFSETS: [(i32, i32); 6] = [(2, 0), (1, 3), (-1, 3), (-2, 0), (-1, -3), (1, -3)];

/// Exact key of corner i of tile (q,r). Neighbours get the same key
/// for a shared corner: shared without epsilon.
pub fn corner_lattice(q: i32, r: i32, i: usize) -> LatticeKey {
    let (cx, cz) = tile_lattice(q, r);
    let (ox, oz) = CORNER_OFFSETS[i % 6];
    (cx + ox, cz + oz)
}

/// Exact key of the midpoint of edge i of tile (q,r).
pub fn edge_lattice(q: i32, r: i32, i: usize) -> LatticeKey {
    let (cx, cz) = tile_lattice(q, r);
    let (ox, oz) = EDGE_OFFSETS[i % 6];
    (cx + ox, cz + oz)
}

/// Lattice -> world coordinates (the only place with float).
pub fn lattice_to_world(key: LatticeKey) -> (f32, f32) {
    const SQRT3: f32 = 1.7320508;
    (
        key.0 as f32 * (SQRT3 / 4.0) * HEX_SIZE,
        key.1 as f32 * 0.25 * HEX_SIZE,
    )
}

/// Float lookup shim: world coordinates -> key (for sampling).
/// Inputs must come from exact points (corners/grid samples),
/// otherwise round may collapse neighbours; producers use the constructors above.
pub fn key_from_world(x: f32, z: f32) -> LatticeKey {
    const SQRT3: f32 = 1.7320508;
    (
        (x / (SQRT3 / 4.0 * HEX_SIZE)).round() as i32,
        (z / (0.25 * HEX_SIZE)).round() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::layout::{tile_to_world, EDGE_NEIGHBORS};

    #[test]
    fn tables_match_trigonometry() {
        // Offsets against honest trigonometry: corner i at 60*i-30 degrees.
        for i in 0..6 {
            let a = (60.0 * i as f32 - 30.0).to_radians();
            let (ex, ez) = (a.cos(), a.sin());
            let (ox, oz) = CORNER_OFFSETS[i];
            // In units (sqrt3/4, 0.25): cos/(sqrt3/4) and sin/0.25.
            let qx = ex / (3.0f32.sqrt() / 4.0);
            let qz = ez / 0.25;
            assert!(
                (qx - ox as f32).abs() < 1e-5 && (qz - oz as f32).abs() < 1e-5,
                "corner {i}: trig=({qx},{qz}) table=({ox},{oz})"
            );
            // Edge midpoint = the average of the neighbouring corners (exact in integers).
            let (ax, az) = CORNER_OFFSETS[i];
            let (bx, bz) = CORNER_OFFSETS[(i + 1) % 6];
            assert_eq!(
                EDGE_OFFSETS[i],
                ((ax + bx) / 2, (az + bz) / 2),
                "edge {i} must be exact midpoint"
            );
        }
    }

    #[test]
    fn shared_corners_exact_across_neighbors() {
        // Edge i of a tile = edge (i+3)%6 of the neighbour; corners coincide EXACTLY (==, not epsilon).
        for q in -3..6 {
            for r in -3..6 {
                for (i, (dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
                    let (nq, nr) = (q + dq, r + dr);
                    // A shared edge is seen from two sides in opposite directions:
                    // A_i == B_{j+1}, A_{i+1} == B_j (j is the neighbour's edge).
                    let j = (i + 3) % 6;
                    assert_eq!(
                        corner_lattice(q, r, i),
                        corner_lattice(nq, nr, (j + 1) % 6),
                        "corner {i} of ({q},{r})"
                    );
                    assert_eq!(
                        corner_lattice(q, r, (i + 1) % 6),
                        corner_lattice(nq, nr, j),
                        "corner {i}+1 of ({q},{r})"
                    );
                    assert_eq!(
                        edge_lattice(q, r, i),
                        edge_lattice(nq, nr, j),
                        "edge {i} of ({q},{r})"
                    );
                }
            }
        }
    }

    #[test]
    fn world_roundtrip_stable() {
        // lattice -> world -> key: stable over a range of maps.
        for q in 0..24 {
            for r in 0..16 {
                for i in 0..6 {
                    let k = corner_lattice(q, r, i);
                    let (x, z) = lattice_to_world(k);
                    assert_eq!(key_from_world(x, z), k, "roundtrip ({q},{r})#{i}");
                }
                // The tile centre matches tile_to_world.
                let (cx, cz) = tile_lattice(q, r);
                let (wx, wz) = lattice_to_world((cx, cz));
                let (ex, ez) = tile_to_world(q as u32, r as u32);
                assert!(
                    (wx - ex).abs() < 1e-4 && (wz - ez).abs() < 1e-4,
                    "center ({q},{r})"
                );
            }
        }
    }
}
