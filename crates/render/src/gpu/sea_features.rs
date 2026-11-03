//! Features standing on the sea, as objects of the feature batch (platform/MESHING.md
//! §2.1: the height field cannot carry a floating plate). Polar ice is a low
//! faceted sheet: where ice meets ice it reaches the hex edge exactly, so an
//! ice field reads as one sheet; where it meets open water the edge is ragged
//! and set back. An atoll is a broken
//! ring of sand islets around a shallow lagoon (the surface lifts the sea
//! floor of its cell, `biomes.rs`). Everything is a pure function of the tile:
//! a tile's objects change only with that tile and its neighbours.
use super::feature::FeatureVertex;
use crate::{
    scene::{
        fog::fog_at,
        layout::{tile_to_world, EDGE_NEIGHBORS},
        terrain::{corner_lattice, fields::hash_u32, lattice_to_world},
    },
    world::state::{FeatureKind, RenderWorld},
};
use kernel::ids::TileId;
use glam::Vec3;

const SEA_LEVEL: f32 = 0.08;
/// Ice plates float this high and reach this deep.
const ICE_TOP: f32 = 0.035;
const ICE_FOOT: f32 = -0.012;
/// Set-back of an open-water edge. Where ice meets ice the sheet reaches the
/// hex edge exactly: no hex outline.
const OPEN_EDGE: f32 = 0.16;

fn random(seed: u32) -> f32 {
    (hash_u32(seed) & 0xffff) as f32 / 65535.
}

fn feature(world: &RenderWorld, tile: u32) -> FeatureKind {
    world.feature.get(tile as usize).copied().unwrap_or_default()
}

/// Ice plates and atolls of the given tiles.
pub fn build(world: &RenderWorld, tiles: &[TileId]) -> Vec<FeatureVertex> {
    let mut out = vec![];
    for &tile in tiles {
        if !world.is_known(tile.0 as usize) {
            continue;
        }
        match feature(world, tile.0) {
            FeatureKind::Ice => ice(world, tile, &mut out),
            FeatureKind::Atoll => atoll(world, tile, &mut out),
            _ => {}
        }
    }
    out
}

fn push_triangle(out: &mut Vec<FeatureVertex>, p: [Vec3; 3], color: [f32; 3], fog: f32) {
    let mut n = (p[1] - p[0]).cross(p[2] - p[0]);
    let mut p = p;
    if n.y < 0. {
        p.swap(1, 2);
        n = -n;
    }
    let Some(n) = n.try_normalize() else { return };
    for v in p {
        out.push(FeatureVertex { pos: v.to_array(), normal: n.to_array(), color, fog });
    }
}

/// A side wall quad from a top edge down to the foot, facing `outward`.
fn push_wall(out: &mut Vec<FeatureVertex>, a: Vec3, b: Vec3, foot: f32, outward: Vec3, color: [f32; 3], fog: f32) {
    let (fa, fb) = (Vec3::new(a.x, foot, a.z), Vec3::new(b.x, foot, b.z));
    for tri in [[a, b, fb], [a, fb, fa]] {
        let mut t = tri;
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        if n.dot(outward) < 0. {
            t.swap(1, 2);
        }
        let Some(n) = (t[1] - t[0]).cross(t[2] - t[0]).try_normalize() else { continue };
        for v in t {
            out.push(FeatureVertex { pos: v.to_array(), normal: n.to_array(), color, fog });
        }
    }
}

fn ice(world: &RenderWorld, tile: TileId, out: &mut Vec<FeatureVertex>) {
    let width = world.width.max(1);
    let (q, r) = ((tile.0 % width) as i32, (tile.0 / width) as i32);
    let (cx, cz) = tile_to_world(q as u32, r as u32);
    let fog = fog_at(world, tile);
    let corner = |k: usize| {
        let (x, z) = lattice_to_world(corner_lattice(q, r, k % 6));
        Vec3::new(x, 0., z)
    };
    // Is the neighbour across edge d (between corners d and d+1) ice too?
    let joined = |d: usize| {
        let (dq, dr) = EDGE_NEIGHBORS[d];
        let (nq, nr) = (q + dq, r + dr);
        let inside = nq >= 0 && nr >= 0 && (nq as u32) < width && (nr as u32) < world.height;
        inside && feature(world, nr as u32 * width + nq as u32) == FeatureKind::Ice
    };
    let center = Vec3::new(cx, 0., cz);
    let top = SEA_LEVEL + ICE_TOP;
    // The outline: three points per edge; an open-water edge is set back and
    // ragged, an edge shared with ice reaches the hex edge exactly (one sheet).
    let mut rim: Vec<(Vec3, bool)> = vec![];
    for d in 0..6 {
        let (a, b) = (corner(d), corner(d + 1));
        let open = !joined(d);
        for k in 0..3 {
            let p = a.lerp(b, k as f32 / 3.);
            let jitter = random(hash_u32(tile.0.wrapping_mul(13) ^ (d as u32 * 7 + k as u32)) ^ 0x1CE) * 0.09;
            // A corner touches the previous edge too: open if either edge is open.
            let open_here = open || (k == 0 && !joined((d + 5) % 6));
            let inset = if open_here { OPEN_EDGE + jitter } else { 0. };
            rim.push((p + (center - p).normalize_or_zero() * inset, open));
        }
    }
    // A low dome of facets: the centre rises a little, so faces catch the sun.
    let hub = Vec3::new(cx, top + 0.012, cz);
    for i in 0..rim.len() {
        let (a, open) = rim[i];
        let (b, _) = rim[(i + 1) % rim.len()];
        let (a, b) = (Vec3::new(a.x, top, a.z), Vec3::new(b.x, top, b.z));
        let shade = 0.93 + random(hash_u32(tile.0 ^ i as u32 ^ 0xF00E)) * 0.07;
        push_triangle(out, [hub, a, b], [0.84 * shade, 0.91 * shade, 0.96 * shade], fog);
        if open {
            let outward = Vec3::new((a.x + b.x) * 0.5 - cx, 0., (a.z + b.z) * 0.5 - cz);
            push_wall(out, a, b, SEA_LEVEL + ICE_FOOT, outward, [0.60, 0.78, 0.88], fog);
        }
    }
}

fn atoll(world: &RenderWorld, tile: TileId, out: &mut Vec<FeatureVertex>) {
    let width = world.width.max(1);
    let (cx, cz) = tile_to_world(tile.0 % width, tile.0 / width);
    let fog = fog_at(world, tile);
    const SEGMENTS: usize = 14;
    let seed = hash_u32(tile.0 ^ 0xA70_11);
    let gaps = [seed % SEGMENTS as u32, (seed >> 8) % SEGMENTS as u32];
    let ring = |i: usize, radius: f32| {
        let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU + random(seed) * 0.4;
        let wobble = 1. + (random(seed ^ i as u32 * 7) - 0.5) * 0.22;
        Vec3::new(cx + a.cos() * radius * wobble, 0., cz + a.sin() * radius * wobble)
    };
    for i in 0..SEGMENTS {
        if gaps.contains(&(i as u32)) {
            continue;
        }
        let j = (i + 1) % SEGMENTS;
        let height = SEA_LEVEL + 0.02 + random(seed ^ (i as u32 * 31)) * 0.03;
        let lift = |p: Vec3, y: f32| Vec3::new(p.x, y, p.z);
        let (oi, oj) = (ring(i, 0.72), ring(j, 0.72));
        let (ii, ij) = (ring(i, 0.5), ring(j, 0.5));
        let (ti, tj) = (lift(ring(i, 0.62), height), lift(ring(j, 0.62), height));
        let sand = [0.90, 0.85, 0.64];
        let wet = [0.74, 0.76, 0.62];
        let foot = SEA_LEVEL - 0.03;
        // Outer and inner beaches slope from the crest into the water.
        push_triangle(out, [lift(oi, foot), lift(oj, foot), tj], wet, fog);
        push_triangle(out, [lift(oi, foot), tj, ti], sand, fog);
        push_triangle(out, [ti, tj, lift(ij, foot)], sand, fog);
        push_triangle(out, [ti, lift(ij, foot), lift(ii, foot)], wet, fog);
        // A green tuft on some islets.
        if random(seed ^ (i as u32 * 97)) < 0.4 {
            let p = (ti + tj) * 0.5;
            let tip = Vec3::new(p.x, height + 0.07, p.z);
            let green = [0.24, 0.52, 0.20];
            for k in 0..3 {
                let a = k as f32 * std::f32::consts::TAU / 3. + random(seed ^ i as u32) * 2.;
                let b = a + std::f32::consts::TAU / 3.;
                let pa = Vec3::new(p.x + a.cos() * 0.06, height, p.z + a.sin() * 0.06);
                let pb = Vec3::new(p.x + b.cos() * 0.06, height, p.z + b.sin() * 0.06);
                push_triangle(out, [pa, pb, tip], green, fog);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::state::TerrainCell;

    fn world() -> RenderWorld {
        let (width, height) = (8u32, 6u32);
        let n = (width * height) as usize;
        let mut feature = vec![FeatureKind::None; n];
        for q in 1..5 {
            feature[(2 * width + q) as usize] = FeatureKind::Ice;
        }
        feature[(4 * width + 6) as usize] = FeatureKind::Atoll;
        RenderWorld {
            width,
            height,
            visible: vec![1; n],
            explored: vec![1; n],
            terrain: (0..n as u32).map(|i| TerrainCell { tile: TileId(i), terrain: 4, owner: None, resource: None }).collect(),
            feature,
            ..Default::default()
        }
    }

    #[test]
    fn ice_plates_float_on_the_sea_and_stay_in_their_tile() {
        let w = world();
        let tiles: Vec<TileId> = (0..w.terrain.len() as u32).map(TileId).collect();
        let out = build(&w, &tiles);
        assert!(!out.is_empty());
        let again = build(&w, &tiles);
        assert!(out.iter().zip(&again).all(|(a, b)| a.pos == b.pos && a.color == b.color), "deterministic");
        for v in &out {
            assert!(v.pos[1] >= SEA_LEVEL - 0.04 && v.pos[1] <= SEA_LEVEL + 0.12, "{:?}", v.pos);
            let n = Vec3::from(v.normal);
            assert!((n.length() - 1.).abs() < 1e-3);
        }
        for t in out.chunks_exact(3) {
            let c = t.iter().fold(Vec3::ZERO, |s, v| s + Vec3::from(v.pos)) / 3.;
            let tile = crate::scene::picking::world_to_tile(c.x, c.z, (w.width, w.height)).unwrap();
            let kind = feature(&w, tile.0);
            assert!(kind == FeatureKind::Ice || kind == FeatureKind::Atoll, "object outside its tile");
        }
        // Tops face up.
        assert!(out.iter().filter(|v| v.normal[1] > 0.99).count() > 30);
    }

    #[test]
    fn unknown_ice_is_not_drawn() {
        let mut w = world();
        w.visible.fill(0);
        w.explored.fill(0);
        let tiles: Vec<TileId> = (0..w.terrain.len() as u32).map(TileId).collect();
        assert!(build(&w, &tiles).is_empty());
    }
}
