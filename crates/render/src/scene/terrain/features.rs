//! Deterministic mountain layout for the visual sandbox.
//! Sparse summit candidates define continuous broad rock massifs in terrain::mountains.
//! The simulation's mountain biome remains the source of mountain placement.

use crate::scene::layout::{tile_to_world, EDGE_NEIGHBORS};
use crate::scene::terrain::fields::{hash_u32, TerrainGeometry};
use crate::world::state::RenderWorld;
use kernel::ids::TileId;

/// Mountain tile (render-side): the mountain biome, not hills.
pub fn is_mountain_tile(world: &RenderWorld, tile: TileId) -> bool {
    world
        .terrain
        .get(tile.0 as usize)
        .map(|c| world.terrain_kinds.is_mountains(c.terrain))
        .unwrap_or(false)
}

/// A sparse dominant summit: deterministic local maximum among mountain cells.
/// Isolated mountains keep one summit, connected cells become its shoulders.
pub fn is_mountain_core(world: &RenderWorld, tile: TileId) -> bool {
    is_summit(world, tile, |t| is_mountain_tile(world, t))
}

/// `is_mountain_core` over the mountain cells `is_mountain` names, e.g. only
/// the known ones, so a hidden neighbour never changes the rank of a summit.
pub(crate) fn is_summit(world: &RenderWorld, tile: TileId, is_mountain: impl Fn(TileId) -> bool) -> bool {
    if !is_mountain(tile) {
        return false;
    }
    let here = hash_u32(tile.0 ^ 0xA9E5_C07E);
    let w = world.width.max(1) as i64;
    let h = world.height.max(1) as i64;
    let q = tile.0 as i64 % w;
    let r = tile.0 as i64 / w;
    for (dq, dr) in EDGE_NEIGHBORS {
        let nq = q + dq as i64;
        let nr = r + dr as i64;
        if nq < 0 || nr < 0 || nq >= w || nr >= h {
            continue;
        }
        let other = TileId((nr * w + nq) as u32);
        if is_mountain(other) && hash_u32(other.0 ^ 0xA9E5_C07E) > here {
            return false;
        }
    }
    true
}

/// Peak variant 0..=5 (ring rotation + proportions).
pub fn peak_variant(tile: TileId) -> usize {
    (hash_u32(tile.0 ^ 0x51ED_1EAB) % 6) as usize
}

/// 0..1 slot jitter (apex height, radius, offset).
fn peak_jitter(tile: TileId, slot: u32) -> f32 {
    (hash_u32(tile.0 ^ slot.wrapping_mul(0x85EB_CA6B)) % 1000) as f32 / 1000.0
}

/// Asymmetric summit anchor and approximate height for diagnostic cameras.
/// The massif envelope determines the final rendered elevation.
pub fn mountain_apex(geom: &TerrainGeometry, tile: TileId) -> [f32; 3] {
    let [x, z] = mountain_center(geom.width, tile);
    let base = geom.patches.tiles[tile.0 as usize].center_y;
    let h = base + 1.40 + 0.75 * peak_jitter(tile, 9);
    [x, h, z]
}

/// Ground-independent anchor shared by legacy diagnostics and the continuous field.
pub(crate) fn mountain_center(width: u32, tile: TileId) -> [f32; 2] {
    let w = width.max(1);
    let (q, r) = (tile.0 % w, tile.0 / w);
    let (wx, wz) = tile_to_world(q, r);
    let v = peak_variant(tile);
    let ang = (v as f32 * 60.0 + peak_jitter(tile, 7) * 30.0).to_radians();
    let dist = 0.18 + 0.35 * peak_jitter(tile, 8);
    [wx + ang.cos() * dist, wz + ang.sin() * dist]
}
