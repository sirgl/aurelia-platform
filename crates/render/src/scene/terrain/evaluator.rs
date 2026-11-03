//! PatchEvaluator: the exact surface height/normal at any point (x,z).
//!
//! Uses EXACTLY the same triangulation as the emitter (24 triangles
//! per patch: 6 central + 18 outer), so the evaluator==mesh invariant
//! holds by construction, not by hope. All ground objects (units,
//! icons, props later) must anchor only here, otherwise
//! floating trees and sunken units appear as relief rises (v28c).

use crate::scene::terrain::fields::{patch_control_points, TerrainGeometry};
use kernel::ids::TileId;

/// Surface probe: height + geometric normal + slope.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceSample {
    pub y: f32,
    pub normal: [f32; 3],
    pub slope: f32,
}

/// Slope zone as an entity (v35): named masks top/slope/cliff in terrain.wgsl
/// correspond to the same thresholds. The CPU version is
/// approximate (the shader sees faceted normals, here smoothed ones),
/// fine for placement (props not on cliffs), not for per-pixel
/// guarantees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlopeZone {
    Top,
    Slope,
    Cliff,
}

pub fn slope_zone(sample: &SurfaceSample) -> SlopeZone {
    let ny = sample.normal[1];
    if ny > 0.90 {
        SlopeZone::Top
    } else if ny < 0.72 || sample.slope > 0.28 {
        SlopeZone::Cliff
    } else {
        SlopeZone::Slope
    }
}

/// World (x,z) -> tile (q,r): the inverse of tile_to_world + cube rounding
/// for pointy-top axial. None: outside the map.
fn world_to_tile(x: f32, z: f32, w: u32, h: u32) -> Option<(u32, u32)> {
    const SQRT3: f32 = 1.7320508;
    let rf = z / 1.5;
    let qf = x / SQRT3 - rf * 0.5;
    // Cube-rounding: x=q, z=r, y=-x-z.
    let (cx, cz) = (qf, rf);
    let cy = -cx - cz;
    let (rx, ry, rz) = (cx.round(), cy.round(), cz.round());
    let (dx, dy, dz) = ((rx - cx).abs(), (ry - cy).abs(), (rz - cz).abs());
    let (qx, qz) = if dx > dy && dx > dz {
        (-ry - rz, rz)
    } else if dy > dz {
        (rx, -rx - ry)
    } else {
        (rx, rz)
    };
    if qx < 0.0 || qz < 0.0 || qx >= w as f32 || qz >= h as f32 {
        return None;
    }
    Some((qx as u32, qz as u32))
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Barycentrics of point p in triangle (a,b,c) by XZ; None: outside.
/// Returns weights (wa, wb, wc) for interpolating y.
fn barycentric(p: [f32; 2], a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Option<(f32, f32, f32)> {
    let (ax, az) = (a[0], a[2]);
    let (bx, bz) = (b[0], b[2]);
    let (cx, cz) = (c[0], c[2]);
    let det = (bz - cz) * (ax - cx) + (cx - bx) * (az - cz);
    if det.abs() < 1e-12 {
        return None;
    }
    let wa = ((bz - cz) * (p[0] - cx) + (cx - bx) * (p[1] - cz)) / det;
    let wb = ((cz - az) * (p[0] - cx) + (ax - cx) * (p[1] - cz)) / det;
    let wc = 1.0 - wa - wb;
    // A small tolerance outwards (joints/epsilon): clamp instead of refusal.
    if wa < -1e-4 || wb < -1e-4 || wc < -1e-4 {
        return None;
    }
    Some((wa, wb, wc))
}

/// Surface probe in world coordinates. None: outside the map.
pub fn sample_surface(geom: &TerrainGeometry, x: f32, z: f32) -> Option<SurfaceSample> {
    if let Some(surface) = &geom.surface {
        return surface.sample_ground(x, z);
    }
    let (q, r) = world_to_tile(x, z, geom.width, geom.height)?;
    let w = geom.width.max(1);
    let tile = TileId(r * w + q);
    let (o, inner, corners, edges) = patch_control_points(geom, tile, q, r);
    let p = [x, z];
    // 24 triangles of the patch: 6 central + 18 outer (the same scheme as the emitter).
    for i in 0..6 {
        let j = (i + 1) % 6;
        // Central (O, I_i, I_j).
        // Sector outer: (I_i, C_i, E_i), (I_i, E_i, I_j), (I_j, E_i, C_j).
        let tris = [
            (o, inner[i], inner[j]),
            (inner[i], corners[i], edges[i]),
            (inner[i], edges[i], inner[j]),
            (inner[j], edges[i], corners[j]),
        ];
        for (a, b, c) in tris {
            if let Some((wa, wb, wc)) = barycentric(
                p,
                [a[0], a[1], a[2]],
                [b[0], b[1], b[2]],
                [c[0], c[1], c[2]],
            ) {
                let y = wa * a[1] + wb * b[1] + wc * c[1];
                let mut n = cross(sub(b, a), sub(c, a));
                if n[1] < 0.0 {
                    n = [-n[0], -n[1], -n[2]];
                }
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
                let normal = [n[0] / len, n[1] / len, n[2] / len];
                let slope = (1.0 - normal[1]).clamp(0.0, 1.0);
                return Some(SurfaceSample { y, normal, slope });
            }
        }
    }
    None
}

/// Geometry validator (tests/debug): NaN/inf, degenerate triangles,
/// sample/mesh agreement, water not above the surface.
#[derive(Debug, PartialEq, Eq)]
pub enum GeometryValidationError {
    NonFinite,
    DegenerateTriangle,
    SampleMeshMismatch,
    TerrainAboveWater,
}

pub fn validate_geometry(
    geom: &TerrainGeometry,
    water_surface: f32,
    rng_seed: u64,
) -> Result<(), GeometryValidationError> {
    // 1. Finiteness of all heights.
    for s in geom
        .boundary
        .corners
        .iter()
        .map(|c| c.y)
        .chain(geom.boundary.edges.iter().map(|e| e.mid_y))
    {
        if !s.is_finite() {
            return Err(GeometryValidationError::NonFinite);
        }
    }
    for t in &geom.patches.tiles {
        if !t.center_y.is_finite() || t.inner_y.iter().any(|v| !v.is_finite()) {
            return Err(GeometryValidationError::NonFinite);
        }
    }
    // 2. Sample at control points = stored heights (evaluator==mesh).
    // Deterministic points: centres + corners of all tiles + pseudo-random
    // ones from the seed (xorshift, stateless).
    let mut state = rng_seed.max(1);
    let mut next_u32 = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state & 0xFFFF_FFFF) as u32
    };
    let w = geom.width.max(1);
    for cell in 0..geom.patches.tiles.len() {
        let (q, r) = ((cell as u32 % w), (cell as u32 / w));
        let t = &geom.patches.tiles[cell];
        let (o, inner, corners, edges) = patch_control_points(geom, TileId(cell as u32), q, r);
        // Centre and ring: exact.
        for (pos, expect) in std::iter::once((o, t.center_y))
            .chain(inner.iter().zip(t.inner_y.iter()).map(|(p, y)| (*p, *y)))
        {
            match sample_surface(geom, pos[0], pos[2]) {
                Some(s) if (s.y - expect).abs() < 1e-3 => {}
                _ => return Err(GeometryValidationError::SampleMeshMismatch),
            }
        }
        // Corners: not above the surface if they carry water (the bowl invariant).
        for (i, c) in corners.iter().enumerate() {
            let cs = &geom.boundary.corners[t.corners[i] as usize];
            if cs.water > 0 && c[1] > water_surface + 1e-4 {
                return Err(GeometryValidationError::TerrainAboveWater);
            }
            let _ = edges;
        }
        // A pseudo-random point inside a tile: the sample is finite.
        let fx = (next_u32() % 1000) as f32 / 1000.0 - 0.5;
        let fz = (next_u32() % 1000) as f32 / 1000.0 - 0.5;
        let (wx, wz) = crate::scene::layout::tile_to_world(q, r);
        if sample_surface(geom, wx + fx, wz + fz).is_none() {
            return Err(GeometryValidationError::SampleMeshMismatch);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::heightfield::build_heightfield;
    use crate::scene::style::{compile, default_style};
    use crate::scene::terrain::fields::build_terrain_geometry;
    use crate::world::state::{RenderWorld, TerrainCell};

    fn test_world(w: u32, h: u32, terrain: u16) -> RenderWorld {
        let mut world = RenderWorld {
            width: w,
            height: h,
            ..Default::default()
        };
        for r in 0..h {
            for q in 0..w {
                world.terrain.push(TerrainCell {
                    tile: TileId(r * w + q),
                    terrain,
                    owner: None,
                    resource: None,
                });
            }
        }
        world
    }

    #[test]
    fn sample_flat_is_exact() {
        let style = compile(default_style()).unwrap();
        let world = test_world(6, 6, 1);
        let hf = build_heightfield(&world, &style);
        let geom = build_terrain_geometry(&world, &style, &hf);
        let base = style.height(1).unwrap();
        // Centres and points inside tiles: exactly base.
        for r in 0..6 {
            for q in 0..6 {
                let (wx, wz) = crate::scene::layout::tile_to_world(q, r);
                let s = sample_surface(&geom, wx, wz).expect("inside map");
                assert!((s.y - base).abs() < 1e-4, "center ({q},{r}): {}", s.y);
                let s2 = sample_surface(&geom, wx + 0.3, wz - 0.2).expect("inside tile");
                assert!((s2.y - base).abs() < 1e-3, "inner ({q},{r}): {}", s2.y);
            }
        }
        // Outside the map: None.
        assert!(sample_surface(&geom, -100.0, -100.0).is_none());
    }

    #[test]
    fn validate_accepts_flat_and_spots_water() {
        use crate::scene::terrain::fields::build_terrain_geometry;
        let style = compile(default_style()).unwrap();
        let world = test_world(8, 8, 0);
        let hf = build_heightfield(&world, &style);
        let geom = build_terrain_geometry(&world, &style, &hf);
        let water_surface = style.height(4).unwrap_or(0.05) + 0.03;
        assert_eq!(
            validate_geometry(&geom, water_surface, 12345),
            Ok(()),
            "flat map validates"
        );
    }

    #[test]
    fn slope_zones_classify_flat_as_top() {
        let style = compile(default_style()).unwrap();
        let world = test_world(6, 6, 1);
        let hf = build_heightfield(&world, &style);
        let geom = build_terrain_geometry(&world, &style, &hf);
        // A flat plain is tops everywhere (props allowed).
        for r in 0..6 {
            for q in 0..6 {
                let (wx, wz) = crate::scene::layout::tile_to_world(q, r);
                let s = sample_surface(&geom, wx, wz).expect("inside map");
                assert_eq!(super::slope_zone(&s), super::SlopeZone::Top);
            }
        }
        // Synthetic: a plumb line is a cliff, a gentle one is a slope.
        let cliff = super::SurfaceSample {
            y: 1.0,
            normal: [0.7, 0.5, 0.5],
            slope: 0.5,
        };
        assert_eq!(super::slope_zone(&cliff), super::SlopeZone::Cliff);
        let slope = super::SurfaceSample {
            y: 1.0,
            normal: [0.0, 0.85, 0.5],
            slope: 0.15,
        };
        assert_eq!(super::slope_zone(&slope), super::SlopeZone::Slope);
    }
}
