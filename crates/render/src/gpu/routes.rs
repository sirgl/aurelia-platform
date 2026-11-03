//! Roads and railroads of game maps: ribbons draped on the final surface, built per tile
//! into the feature chunks.
//!
//! A route tile draws a half segment from its centre to the edge of every neighbour that
//! has a route or a city (cities join the network); a city tile draws the half segments
//! toward its route neighbours. Each half segment depends only on the tile and its six
//! neighbours, which the feature-chunk key covers. A railroad is a dark ballast bed with
//! sleepers and two steel rails; a road is a packed-earth track.

use crate::gpu::feature::FeatureVertex;
use crate::scene::fog::fog_at;
use crate::scene::layout::{tile_to_world, EDGE_NEIGHBORS};
use crate::scene::terrain::evaluator::sample_surface;
use crate::scene::terrain::fields::TerrainGeometry;
use crate::world::state::RenderWorld;
use kernel::ids::TileId;

const ROAD: u8 = 1;
const RAILROAD: u8 = 2;
/// Height of a route above the ground: clear of the terrain triangles, under every model.
const LIFT: f32 = 0.012;
/// Length of one draped section along a half segment.
const STEP: f32 = 0.11;
const ROAD_WIDTH: f32 = 0.11;
const ROAD_COLOR: [f32; 3] = [0.62, 0.52, 0.36];
const BALLAST_WIDTH: f32 = 0.13;
const BALLAST_COLOR: [f32; 3] = [0.40, 0.38, 0.36];
const SLEEPER_COLOR: [f32; 3] = [0.33, 0.23, 0.15];
const RAIL_COLOR: [f32; 3] = [0.20, 0.21, 0.23];
const RAIL_GAUGE: f32 = 0.032;
const RAIL_WIDTH: f32 = 0.012;
const SLEEPER_PITCH: f32 = 0.055;

/// Route code of a tile (0 none) and whether a city stands there.
fn route(world: &RenderWorld, tile: usize) -> u8 {
    world.road.get(tile).copied().unwrap_or(0)
}

/// City tiles as a per-tile mask (one pass over the cities).
pub fn city_mask(world: &RenderWorld) -> Vec<bool> {
    let mut mask = vec![false; world.terrain.len()];
    for city in &world.cities {
        if let Some(slot) = mask.get_mut(city.tile.0 as usize) {
            *slot = true;
        }
    }
    mask
}

/// Neighbour tile index in edge direction `dir` (EDGE_NEIGHBORS order).
pub fn neighbour(world: &RenderWorld, tile: u32, dir: usize) -> Option<usize> {
    let width = world.width as i32;
    let (dq, dr) = EDGE_NEIGHBORS[dir];
    let (q, r) = (tile as i32 % width + dq, tile as i32 / width + dr);
    let inside = q >= 0 && r >= 0 && q < width && r < world.height as i32;
    inside.then_some((r * width + q) as usize)
}

/// Bits per edge direction: the neighbour joins a route (has one, or is a city).
pub fn joins(world: &RenderWorld, cities: &[bool], tile: u32) -> u8 {
    let mut mask = 0u8;
    for dir in 0..6 {
        if let Some(n) = neighbour(world, tile, dir) {
            if world.is_known(n) && (route(world, n) != 0 || cities[n]) {
                mask |= 1 << dir;
            }
        }
    }
    mask
}

/// Route ribbons of the given known tiles.
pub fn build_for(world: &RenderWorld, geom: &TerrainGeometry, tiles: &[TileId]) -> Vec<FeatureVertex> {
    let mut out = Vec::new();
    let width = world.width.max(1);
    let cities = city_mask(world);
    for tile in tiles {
        let i = tile.0 as usize;
        if i >= world.terrain.len() || !world.is_known(i) {
            continue;
        }
        let own = route(world, i);
        let city = cities[i];
        if own == 0 && !city {
            continue;
        }
        let (x, z) = tile_to_world(tile.0 % width, tile.0 / width);
        let fog = fog_at(world, *tile);
        let mut ribbon = Ribbon { out: &mut out, geom, fog };
        let mut joined = false;
        for dir in 0..6 {
            let Some(n) = neighbour(world, tile.0, dir) else {
                continue;
            };
            let other = route(world, n);
            let connects = world.is_known(n) && (other != 0 || (own != 0 && cities[n]));
            if !connects || (city && own == 0 && other == 0) {
                continue;
            }
            joined = true;
            // A segment is a railroad only when both ends are railroads (or a city).
            let kind = if own.max(other) == RAILROAD && own != ROAD && other != ROAD { RAILROAD } else { ROAD };
            let angle = (dir as f32 * 60.0).to_radians();
            let end = [x + angle.cos() * 0.866, z + angle.sin() * 0.866];
            ribbon.track([x, z], end, kind);
        }
        if !joined && own != 0 {
            // A lone route tile: a short track across the tile so it still reads.
            ribbon.track([x - 0.3, z], [x + 0.3, z], own);
        }
    }
    out
}

struct Ribbon<'a> {
    out: &'a mut Vec<FeatureVertex>,
    geom: &'a TerrainGeometry,
    fog: f32,
}

impl Ribbon<'_> {
    fn track(&mut self, from: [f32; 2], to: [f32; 2], kind: u8) {
        if kind == RAILROAD {
            self.strip(from, to, 0.0, BALLAST_WIDTH, LIFT, BALLAST_COLOR);
            self.sleepers(from, to);
            for side in [-RAIL_GAUGE, RAIL_GAUGE] {
                self.strip(from, to, side, RAIL_WIDTH, LIFT + 0.010, RAIL_COLOR);
            }
        } else {
            self.strip(from, to, 0.0, ROAD_WIDTH, LIFT, ROAD_COLOR);
        }
    }

    /// Flat band along from -> to, offset sideways, draped section by section.
    fn strip(&mut self, from: [f32; 2], to: [f32; 2], offset: f32, width: f32, lift: f32, color: [f32; 3]) {
        let (dx, dz) = (to[0] - from[0], to[1] - from[1]);
        let length = dx.hypot(dz).max(1e-6);
        let (ux, uz) = (dx / length, dz / length);
        let (nx, nz) = (-uz, ux);
        let sections = (length / STEP).ceil().max(1.0) as usize;
        let point = |t: f32, side: f32| {
            let x = from[0] + dx * t + nx * (offset + side);
            let z = from[1] + dz * t + nz * (offset + side);
            [x, z]
        };
        for k in 0..sections {
            let (t0, t1) = (k as f32 / sections as f32, (k + 1) as f32 / sections as f32);
            let quad = [point(t0, -width / 2.0), point(t1, -width / 2.0), point(t1, width / 2.0), point(t0, width / 2.0)];
            self.quad(quad, lift, color);
        }
    }

    fn sleepers(&mut self, from: [f32; 2], to: [f32; 2]) {
        let (dx, dz) = (to[0] - from[0], to[1] - from[1]);
        let length = dx.hypot(dz).max(1e-6);
        let (ux, uz) = (dx / length, dz / length);
        let (nx, nz) = (-uz, ux);
        let count = (length / SLEEPER_PITCH) as usize;
        for k in 0..count {
            let s = (k as f32 + 0.5) * SLEEPER_PITCH;
            let c = [from[0] + ux * s, from[1] + uz * s];
            let (a, b) = (BALLAST_WIDTH * 0.42, 0.011);
            let quad = [
                [c[0] - nx * a - ux * b, c[1] - nz * a - uz * b],
                [c[0] - nx * a + ux * b, c[1] - nz * a + uz * b],
                [c[0] + nx * a + ux * b, c[1] + nz * a + uz * b],
                [c[0] + nx * a - ux * b, c[1] + nz * a - uz * b],
            ];
            self.quad(quad, LIFT + 0.005, SLEEPER_COLOR);
        }
    }

    /// Two triangles on the ground; skipped where the surface has no sample.
    fn quad(&mut self, corners: [[f32; 2]; 4], lift: f32, color: [f32; 3]) {
        let mut p = [[0.0f32; 3]; 4];
        for (dst, [x, z]) in p.iter_mut().zip(corners) {
            let Some(sample) = sample_surface(self.geom, x, z) else {
                return;
            };
            *dst = [x, sample.y + lift, z];
        }
        for tri in [[0, 2, 1], [0, 3, 2]] {
            let [a, b, c] = tri.map(|i| p[i]);
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let mut n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            if n[1] < 0.0 {
                n = n.map(|v| -v);
            }
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
            let normal = n.map(|v| v / len);
            for pos in [a, b, c] {
                self.out.push(FeatureVertex { pos, normal, color, fog: self.fog });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::heightfield::build_heightfield;
    use crate::scene::style::{compile, default_style};
    use crate::scene::terrain::fields::build_terrain_geometry;
    use crate::world::state::TerrainCell;

    fn world() -> RenderWorld {
        let n = 64;
        RenderWorld {
            width: 8,
            height: 8,
            visible: vec![1; n],
            explored: vec![1; n],
            road: vec![0; n],
            terrain: (0..n as u32)
                .map(|i| TerrainCell { tile: TileId(i), terrain: 1, owner: None, resource: None })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn routes_join_neighbours_drape_on_the_ground_and_railroads_carry_rails() {
        let mut w = world();
        let style = compile(default_style()).unwrap();
        let geom = build_terrain_geometry(&w, &style, &build_heightfield(&w, &style));
        let all: Vec<TileId> = (0..64).map(TileId).collect();
        assert!(build_for(&w, &geom, &all).is_empty());
        w.road[27] = ROAD;
        w.road[28] = ROAD;
        let road = build_for(&w, &geom, &all);
        assert!(!road.is_empty());
        for v in &road {
            let ground = sample_surface(&geom, v.pos[0], v.pos[2]).unwrap().y;
            assert!((v.pos[1] - ground - LIFT).abs() < 1e-4, "draped at the lift");
            assert!(v.color == ROAD_COLOR);
        }
        assert_eq!(joins(&w, &city_mask(&w), 27), 1, "tile 27 joins east only");
        w.road[27] = RAILROAD;
        w.road[28] = RAILROAD;
        let rail = build_for(&w, &geom, &all);
        assert!(rail.len() > road.len() * 2, "ballast, sleepers and two rails");
        assert!(rail.iter().any(|v| v.color == RAIL_COLOR));
        w.explored[28] = 0;
        w.visible[28] = 0;
        let half = build_for(&w, &geom, &all);
        assert!(half.len() < rail.len(), "no track toward or on unknown tiles");
    }
}
