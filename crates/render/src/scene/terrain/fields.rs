//! Immutable staged terrain geometry (v28b): Relief → Boundary → Patch.
//!
//! Each stage is built whole and is read-only afterwards: no mutating
//! synchronisation between fields. Everything is deterministic: row-major traversal,
//! ids are assigned in traversal order, fog merging is max (commutative).
//! Float arithmetic only inside value formation; identity is always
//! the integer lattice (see lattice.rs).
//!
//! v28b sets flat parameters (the surface matches the old heightfield
//! plus the top fan): center is the tile height, inner and edge are lerps of the old
//! planes. Relief with raised hills is v28c, only through these structures.

use crate::scene::fog::fog_at;
use crate::scene::heightfield::{CornerData, Heightfield};
use crate::scene::style::CompiledTerrainStyle;
use crate::scene::terrain::{corner_lattice, edge_lattice, LatticeKey};
use crate::world::state::RenderWorld;
use kernel::ids::TileId;
use kernel::navigation::cost::TerrainKinds;

/// Fraction of the path centre -> edge midpoint for the inner ring of the patch.
pub const INNER_FRACTION: f32 = 0.55;

/// Mountains are narrower: the slope starts closer to the centre (steep towers of v35,
/// not broad domes). Hills keep 0.64 (mound), others 0.55.
pub const MOUNTAIN_INNER_FRACTION: f32 = 0.48;

/// Tile morphology: what the terrain code means in the projection rules
/// (`RenderWorld::terrain_kinds`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReliefKind {
    Flat,
    Hill,
    Mountain,
}

/// terrain id -> morphology (a flat mapping, no combinatorial explosion of
/// SnowMountain/DesertHill: material and relief are different axes).
pub fn relief_kind(kinds: TerrainKinds, terrain: u16) -> ReliefKind {
    if kinds.is_hills(terrain) {
        ReliefKind::Hill
    } else if kinds.is_mountains(terrain) {
        ReliefKind::Mountain
    } else {
        ReliefKind::Flat
    }
}

/// Tile semantics: base height + morphology + variation (v28b: 0).
#[derive(Debug, Clone, Copy)]
pub struct ReliefTile {
    pub base_y: f32,
    pub kind: ReliefKind,
    pub variation: f32,
}

#[derive(Debug, Clone, Default)]
pub struct ReliefField {
    pub tiles: Vec<ReliefTile>,
}

/// Shared corner: height + water + splat + fog (max of referrers).
#[derive(Debug, Clone, Copy)]
pub struct CornerSample {
    pub y: f32,
    pub water: u8,
    pub counts: [(u16, u8); 4],
    pub fog: f32,
}

/// Shared edge midpoint: height + fog (max of two sides) + water (max).
#[derive(Debug, Clone, Copy)]
pub struct EdgeSample {
    pub mid_y: f32,
    pub fog: f32,
    pub water: u8,
}

#[derive(Debug, Clone, Default)]
pub struct BoundaryField {
    pub corners: Vec<CornerSample>,
    pub edges: Vec<EdgeSample>,
}

/// Tile patch: own (centre + inner ring) + references to shared.
/// The boundary geometry belongs to BoundaryField; cracks are impossible
/// if the emitter takes the boundary only from here.
#[derive(Debug, Clone)]
pub struct TilePatch {
    pub center_y: f32,
    /// Dome apex shift from the tile centre (+-0.15): the stars of neighbouring
    /// domes are not coaxial, so each hill "looks" in its own direction.
    pub apex: [f32; 2],
    pub inner_y: [f32; 6],
    pub corners: [u32; 6],
    pub edges: [u32; 6],
    pub fog: f32,
    pub terrain: u16,
    pub relief: ReliefKind,
}

#[derive(Debug, Clone, Default)]
pub struct PatchField {
    /// Row-major, length = number of map tiles.
    pub tiles: Vec<TilePatch>,
}

#[derive(Debug, Clone, Default)]
pub struct TerrainGeometry {
    pub surface: Option<super::surface::SurfaceMesh>,
    pub relief: ReliefField,
    pub boundary: BoundaryField,
    pub patches: PatchField,
    pub width: u32,
    pub height: u32,
}

/// A stateless integer hash (relief variation): the same input gives
///
/// the same output on any platform and in any traversal order.
/// Render-side determinism (the simulation does not read these numbers).
pub fn hash_u32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^= x >> 16;
    x
}

/// Tile ridge axis 0..=5 (through opposite inner vertices).
/// Deterministic by tile id.
pub fn ridge_axis(tile: TileId) -> usize {
    (hash_u32(tile.0 ^ 0x9E3779B9) % 6) as usize
}

/// Slot jitter 0..5 in the range +-1 (deterministic by the tile+slot pair).
fn slot_jitter(tile: TileId, slot: usize) -> f32 {
    let h = hash_u32(tile.0 ^ ((slot as u32 + 1).wrapping_mul(0x85EBCA6B)));
    (h % 1000) as f32 / 1000.0 * 2.0 - 1.0
}

/// Hill morphology: (dome height x, ridge amplitude x).
/// Was: all hills one dome 0.95 with the same ridge ("all hills are
/// the same"). Now: height 0.75..1.25, ridge 0.6..1.4, from round
/// domes to sharp crests. A pure function of tile (deterministic).
fn dome_variety(tile: TileId) -> (f32, f32) {
    let h1 = (hash_u32(tile.0 ^ 0xD0E5) % 1000) as f32 / 1000.0;
    let h2 = (hash_u32(tile.0 ^ 0x9A17) % 1000) as f32 / 1000.0;
    (0.75 + 0.5 * h1, 0.6 + 0.8 * h2)
}

/// Second dome harmonic (barely noticeable asymmetry, not a saddle:
/// a strong one broke the silhouette into "sea urchins"). A pure function of tile.
fn saddle_lobe(tile: TileId) -> (f32, f32) {
    let a = (hash_u32(tile.0 ^ 0x5ADD1E) % 1000) as f32 / 1000.0;
    let p = (hash_u32(tile.0 ^ 0x9DA5E) % 1000) as f32 / 1000.0 * std::f32::consts::TAU;
    (0.12 * a, p)
}

/// Coherent ridge noise 0..1: bilinear interpolation with smoothing.
/// A per-tile hash gave isolated pimples (each hill on its own);
/// a coherent field gathers ranges and valleys. Render-side float,
/// deterministic (a pure function of coordinates).
fn range_noise_cell(x: f32, z: f32, cell: f32) -> f32 {
    let lat = |ix: i32, iz: i32| {
        let h =
            hash_u32((ix as u32).wrapping_mul(0x8DA6B343) ^ (iz as u32).wrapping_mul(0xD8163841));
        (h % 1000) as f32 / 1000.0
    };
    let (gx, gz) = (x / cell, z / cell);
    let (ix, iz) = (gx.floor() as i32, gz.floor() as i32);
    let (tx, ty) = (gx - ix as f32, gz - iz as f32);
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    lat(ix, iz) * (1.0 - sx) * (1.0 - sy)
        + lat(ix + 1, iz) * sx * (1.0 - sy)
        + lat(ix, iz + 1) * (1.0 - sx) * sy
        + lat(ix + 1, iz + 1) * sx * sy
}

/// Ridge noise with a standard cell of 4.5 world units.
fn range_noise(x: f32, z: f32) -> f32 {
    range_noise_cell(x, z, 4.5)
}
/// No allocations: a merge of two sorted quadruples (count desc, id asc),
/// top 4. Deterministic; called for every edge midpoint (a hot path).
/// Average the splat counters of two corners (for blending edge midpoints).
pub fn avg_counts(a: &[(u16, u8); 4], b: &[(u16, u8); 4]) -> [(u16, u8); 4] {
    // Merge, summing the weights of equal ids (each quadruple is unique by id).
    let mut tmp = [(0u16, 0u32); 8];
    for (i, (t, c)) in a.iter().chain(b.iter()).enumerate() {
        tmp[i] = (*t, *c as u32);
    }
    // Insertion sort of 8 elements: count desc, id asc.
    for i in 1..8 {
        let mut j = i;
        while j > 0
            && (tmp[j].1 > tmp[j - 1].1 || (tmp[j].1 == tmp[j - 1].1 && tmp[j].0 < tmp[j - 1].0))
        {
            tmp.swap(j, j - 1);
            j -= 1;
        }
    }
    // Sum duplicates (adjacent after sorting by id? no, by count;
    // at most 4 pairs of duplicates: a linear pass with an accumulator).
    let mut out = [(0u16, 0u8); 4];
    let mut n = 0;
    let mut i = 0;
    while i < 8 && n < 4 {
        let (t, mut c) = tmp[i];
        if c == 0 {
            break;
        }
        // Collect the same id further along the array.
        let mut j = i + 1;
        while j < 8 {
            if tmp[j].0 == t {
                c += tmp[j].1;
                tmp[j].1 = 0;
            }
            j += 1;
        }
        out[n] = (t, c.min(255) as u8);
        n += 1;
        i += 1;
        // Skip the zeroed ones.
        while i < 8 && tmp[i].1 == 0 {
            i += 1;
        }
    }
    // Re-sort the result (sums may have broken the order): <=4 elements.
    for i in 1..n {
        let mut j = i;
        while j > 0
            && (out[j].1 > out[j - 1].1 || (out[j].1 == out[j - 1].1 && out[j].0 < out[j - 1].0))
        {
            out.swap(j, j - 1);
            j -= 1;
        }
    }
    out
}

/// Build all the geometry of the map (call when the world revision changes).
/// Dense grids instead of HashMap (keys are a compact lattice range):
/// building is O(tiles), no hashing in the hot path.
pub fn build_terrain_geometry(
    world: &RenderWorld,
    style: &CompiledTerrainStyle,
    hf: &Heightfield,
) -> TerrainGeometry {
    let w = world.width.max(1);
    let n = world.terrain.len();
    // Lattice range bounds: X in [-2, 4(W-1)+2(H-1)+2], Z in [-4, 6(H-1)+4].
    let (gw, gh) = (
        (4 * w + 2 * world.height + 4) as usize,
        (6 * world.height + 9) as usize,
    );
    let (ox, oz) = (2i64, 4i64);
    let gidx = |key: LatticeKey| -> usize {
        ((key.1 as i64 + oz) * gw as i64 + (key.0 as i64 + ox)) as usize
    };
    const NONE: u32 = u32::MAX;
    // 1. Relief: base + morphology (variation 0 until v28c).
    let mut relief_tiles = Vec::with_capacity(n);
    for cell in &world.terrain {
        relief_tiles.push(ReliefTile {
            base_y: style.height(cell.terrain).unwrap_or(0.3),
            kind: relief_kind(world.terrain_kinds, cell.terrain),
            variation: 0.0,
        });
    }
    // 2. Boundary: corners and edge midpoints, ids in traversal order.
    // Fog of a shared point is max (commutative -> order does not matter).
    let mut boundary = BoundaryField::default();
    let mut corner_ids = vec![NONE; gw * gh];
    let mut edge_ids = vec![NONE; gw * gh];
    let corner_of = |boundary: &mut BoundaryField,
                     corner_ids: &mut Vec<u32>,
                     key: LatticeKey,
                     sample: CornerSample|
     -> u32 {
        let gi = gidx(key);
        if corner_ids[gi] != NONE {
            let id = corner_ids[gi];
            let s = &mut boundary.corners[id as usize];
            if sample.fog > s.fog {
                s.fog = sample.fog;
            }
            return id;
        }
        let id = boundary.corners.len() as u32;
        boundary.corners.push(sample);
        corner_ids[gi] = id;
        id
    };
    // Corners: first all of them (edge midpoints are the average of the end heights).
    for cell in &world.terrain {
        let (q, r) = ((cell.tile.0 % w) as i32, (cell.tile.0 / w) as i32);
        let tile_fog = fog_at(world, cell.tile);
        for i in 0..6 {
            let key = corner_lattice(q, r, i);
            let data: CornerData = hf.get(key).unwrap_or(CornerData {
                height: style.height(cell.terrain).unwrap_or(0.3),
                counts: [(cell.terrain, 1), (0, 0), (0, 0), (0, 0)],
                n: 1,
                water: 0,
            });
            corner_of(
                &mut boundary,
                &mut corner_ids,
                key,
                CornerSample {
                    y: data.height,
                    water: data.water,
                    counts: data.counts,
                    fog: tile_fog,
                },
            );
        }
    }
    // Edge midpoints: the average of the end heights + the max of the sides' fogs.
    // Row-major x 6 traversal: deterministic; max is commutative.
    for cell in &world.terrain {
        let (q, r) = ((cell.tile.0 % w) as i32, (cell.tile.0 / w) as i32);
        let tile_fog = fog_at(world, cell.tile);
        for i in 0..6 {
            let key = edge_lattice(q, r, i);
            let gi = gidx(key);
            if edge_ids[gi] != NONE {
                let id = edge_ids[gi];
                let s = &mut boundary.edges[id as usize];
                if tile_fog > s.fog {
                    s.fog = tile_fog;
                }
                // Water of the second side (max, commutative).
                let ca = boundary.corners[corner_ids[gidx(corner_lattice(q, r, i))] as usize];
                let cb =
                    boundary.corners[corner_ids[gidx(corner_lattice(q, r, (i + 1) % 6))] as usize];
                let wtr = ca.water.max(cb.water);
                if wtr > s.water {
                    s.water = wtr;
                }
                continue;
            }
            let ca = boundary.corners[corner_ids[gidx(corner_lattice(q, r, i))] as usize];
            let cb = boundary.corners[corner_ids[gidx(corner_lattice(q, r, (i + 1) % 6))] as usize];
            // EdgeField step: the edge midpoint sinks towards the lower
            // side by a quarter of the base difference (an S-profile instead of an even
            // slope: the steep upper part paints itself as rock;
            // the evaluator reads the same field -> sample==mesh is preserved).
            // Canonical (the difference's absolute value), the first writer wins.
            // Water is excluded (a shore is carve+sand, not a step).
            let mut dip = 0.0f32;
            if ca.water == 0 && cb.water == 0 {
                let (dq, dr) = crate::scene::layout::EDGE_NEIGHBORS[i];
                let (nq, nr) = (q + dq, r + dr);
                let nb_ok = nq >= 0 && nr >= 0 && (nq as u32) < w && (nr as u32) < world.height;
                if nb_ok {
                    let nt = nr as u32 * w + nq as u32;
                    if let Some(ncell) = world.terrain.get(nt as usize) {
                        let ba = style.height(cell.terrain).unwrap_or(0.3);
                        let bb = style.height(ncell.terrain).unwrap_or(0.3);
                        let step = (ba - bb).abs();
                        if step >= 0.18
                            && !world.terrain_kinds.is_water(cell.terrain)
                            && !world.terrain_kinds.is_water(ncell.terrain)
                        {
                            dip = step * 0.25;
                        }
                    }
                }
            }
            let id = boundary.edges.len() as u32;
            boundary.edges.push(EdgeSample {
                mid_y: (ca.y + cb.y) * 0.5 - dip,
                fog: tile_fog,
                water: ca.water.max(cb.water),
            });
            edge_ids[gi] = id;
        }
    }
    // 3. Patches: centre = base + raise; inner = lerp towards the midpoints +
    // a ridge bump (mean-preserving: the mean level does not change, only
    // asymmetry; at raise 0 a pure lerp = the v28b surface).
    // Water: raise must be 0 (centre = base = the lake planes).
    let mut patch_tiles = Vec::with_capacity(n);
    for cell in &world.terrain {
        let (q, r) = ((cell.tile.0 % w) as i32, (cell.tile.0 / w) as i32);
        let rel = &relief_tiles[cell.tile.0 as usize];
        let raise = style.raise(cell.terrain).unwrap_or([0.0, 0.0]);
        debug_assert!(
            !world.terrain_kinds.is_water(cell.terrain) || raise == [0.0, 0.0],
            "water raise must stay zero (lake planes)"
        );
        let (h_scale, r_scale) = dome_variety(cell.tile);
        let (wx, wz) = crate::scene::layout::tile_to_world(q as u32, r as u32);
        // Coherent dome modulation: ranges and valleys instead of pimples.
        // Mountains: a coarse cell (7.5), a strong swing and a high mean:
        // a chain across 2-4 tiles with a common high line (v35 towers),
        // not pyramid pimples. Hills: gently.
        // Soft (0.85..1.15): a strong one pushed hills wholly into the snow.
        let range_mult = if rel.kind == ReliefKind::Mountain {
            0.62 + 0.76 * range_noise_cell(wx, wz, 7.5)
        } else {
            0.85 + 0.3 * range_noise(wx, wz)
        };
        let center_y = rel.base_y + raise[0] * h_scale * range_mult;
        // Apex drift +-0.15 (inside the inner ring, so no twists).
        let apex = [
            ((hash_u32(cell.tile.0 ^ 0xA9EC5) % 1000) as f32 / 1000.0 - 0.5) * 0.3,
            ((hash_u32(cell.tile.0 ^ 0xD21F7) % 1000) as f32 / 1000.0 - 0.5) * 0.3,
        ];
        let mut corners = [0u32; 6];
        let mut edges = [0u32; 6];
        let mut inner_y = [0.0f32; 6];
        let axis = ridge_axis(cell.tile);
        let (lobe_amp, lobe_phase) = saddle_lobe(cell.tile);
        // Hill-mound: a hill is a broad soft mass (a wider dome, a quieter ridge
        // and jitter), not a mini-mountain. Mountains keep sharp relief.
        // INNER is wider for hills: the slope starts farther from the centre.
        let is_hill = rel.kind == ReliefKind::Hill;
        let mound_damp = if is_hill { 0.55 } else { 1.0 };
        for i in 0..6 {
            corners[i] = corner_ids[gidx(corner_lattice(q, r, i))];
            edges[i] = edge_ids[gidx(edge_lattice(q, r, i))];
            let mid_y = boundary.edges[edges[i] as usize].mid_y;
            let frac = if is_hill {
                0.64
            } else if rel.kind == ReliefKind::Mountain {
                MOUNTAIN_INNER_FRACTION
            } else {
                INNER_FRACTION
            };
            let base_lerp = center_y + (mid_y - center_y) * frac;
            // Ridge across the axis: 1.0 on the axis -> 0.1 opposite; mean 0.55.
            let theta = (i as f32 - axis as f32) * std::f32::consts::PI / 3.0;
            let ridge = 0.55 + 0.45 * theta.cos();
            // Second harmonic (saddle): breaks the 6-ray symmetry.
            let lobe = lobe_amp * (2.0 * theta - lobe_phase).cos();
            inner_y[i] = base_lerp
                + raise[1] * (ridge - 0.55) * r_scale * mound_damp
                + raise[1] * lobe * mound_damp
                + raise[1] * 0.12 * slot_jitter(cell.tile, i) * mound_damp;
        }
        patch_tiles.push(TilePatch {
            center_y,
            apex,
            inner_y,
            corners,
            edges,
            fog: fog_at(world, cell.tile),
            terrain: cell.terrain,
            relief: rel.kind,
        });
    }
    TerrainGeometry {
        surface: world.visual_surface.then(|| super::surface::build_measured(world, style)),
        relief: ReliefField {
            tiles: relief_tiles,
        },
        boundary,
        patches: PatchField { tiles: patch_tiles },
        width: world.width,
        height: world.height,
    }
}

/// Patch control points: centre, inner ring, corners, edge midpoints.
pub type ControlPoints = ([f32; 3], [[f32; 3]; 6], [[f32; 3]; 6], [[f32; 3]; 6]);

/// Positions of the patch control points (the only place of the math;
/// the emitter and the evaluator must use only it).
/// Returns (center, inner[6], corner_pos[6], edge_pos[6]).
pub fn patch_control_points(geom: &TerrainGeometry, tile: TileId, q: u32, r: u32) -> ControlPoints {
    use crate::scene::terrain::{edge_lattice, lattice_to_world};
    let p = &geom.patches.tiles[tile.0 as usize];
    let (wx, wz) = crate::scene::layout::tile_to_world(q, r);
    let center = [wx + p.apex[0], p.center_y, wz + p.apex[1]];
    let mut inner = [[0.0f32; 3]; 6];
    let mut corners = [[0.0f32; 3]; 6];
    let mut edges = [[0.0f32; 3]; 6];
    for i in 0..6 {
        let (ex, ez) = lattice_to_world(edge_lattice(q as i32, r as i32, i));
        edges[i] = [ex, geom.boundary.edges[p.edges[i] as usize].mid_y, ez];
        // Ragged ring: the fraction of the path to the edge midpoint is its own for each spoke.
        // Barely (+-8%): a strong one tore the silhouette into stars. Plus a turn of the whole
        // ring by a hash angle (+-14 degrees): the spokes do not look at the edge midpoints
        // the same way, so each hill's fan is its own. Hills have a wider base radius
        // (mound), mountains a narrower one (towers). Pure functions of (tile, spoke), so the
        // evaluator will match.
        let base_frac = if p.relief == ReliefKind::Hill {
            0.64
        } else if p.relief == ReliefKind::Mountain {
            MOUNTAIN_INNER_FRACTION
        } else {
            INNER_FRACTION
        };
        let frac = base_frac * (0.92 + 0.16 * (slot_jitter(tile, i) * 0.5 + 0.5));
        let tw = ((hash_u32(tile.0 ^ 0x7ED) % 1000) as f32 / 1000.0 - 0.5) * 0.5;
        let (dx, dz) = (ex - wx, ez - wz);
        let (rx, rz) = (dx * tw.cos() - dz * tw.sin(), dx * tw.sin() + dz * tw.cos());
        inner[i] = [wx + rx * frac, p.inner_y[i], wz + rz * frac];
        let (kx, kz) =
            lattice_to_world(crate::scene::terrain::corner_lattice(q as i32, r as i32, i));
        corners[i] = [kx, geom.boundary.corners[p.corners[i] as usize].y, kz];
    }
    (center, inner, corners, edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::heightfield::build_heightfield;
    use crate::scene::style::{compile, default_style};
    use crate::world::state::TerrainCell;

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
    fn boundary_shared_bit_for_bit() {
        // Cracks are impossible: neighbouring tiles reference the same boundary
        // samples (the indices match), positions are computed by one function.
        let style = compile(default_style()).unwrap();
        let world = test_world(8, 8, 0);
        let hf = build_heightfield(&world, &style);
        let geom = build_terrain_geometry(&world, &style, &hf);
        // Tiles (3,3)-(4,3): a shared E/W edge.
        let a = &geom.patches.tiles[(3 * 8 + 3) as usize];
        let b = &geom.patches.tiles[(3 * 8 + 4) as usize];
        // Edge 0 (E) of tile A == edge 3 (W) of tile B (the same ids).
        assert_eq!(a.edges[0], b.edges[3], "shared edge id");
        assert_eq!(a.corners[0], b.corners[4], "shared corner id A0==B4");
        assert_eq!(a.corners[1], b.corners[3], "shared corner id A1==B3");
        // Positions from patch_control_points match bit for bit.
        let (_, _, ca, ea) = patch_control_points(&geom, TileId(3 * 8 + 3), 3, 3);
        let (_, _, cb, _) = patch_control_points(&geom, TileId(3 * 8 + 4), 4, 3);
        assert_eq!(
            ea[0],
            patch_control_points(&geom, TileId(3 * 8 + 4), 4, 3).3[3]
        );
        assert_eq!(ca[0], cb[4]);
        assert_eq!(ca[1], cb[3]);
    }

    #[test]
    fn hills_raise_center_and_vary() {
        // Hills are DIFFERENT: centre = base + raise x (0.75..1.25 by hash);
        // inner ones are uneven (ridge x (0.6..1.4) + jitter);
        // water: centre strictly the base (the lake plane).
        let style = compile(default_style()).unwrap();
        let world = test_world(6, 6, 2);
        let hf = build_heightfield(&world, &style);
        let geom = build_terrain_geometry(&world, &style, &hf);
        let (base, raise) = (style.height(2).unwrap(), style.raise(2).unwrap());
        assert_eq!((base, raise), (0.55, [0.40, 0.20]));
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for t in &geom.patches.tiles {
            // A band: h_scale 0.75..1.25 x range_mult 0.7..1.3.
            assert!(
                t.center_y >= base + raise[0] * 0.5 - 1e-6
                    && t.center_y <= base + raise[0] * 1.65 + 1e-6,
                "hill center in variety band: {}",
                t.center_y
            );
            lo = lo.min(t.center_y);
            hi = hi.max(t.center_y);
        }
        assert!(hi - lo > 0.15, "dome heights vary: {lo}..{hi}");
        // The apex drifts (+-0.15): the stars are not coaxial.
        for t in &geom.patches.tiles {
            assert!(
                t.apex[0].abs() <= 0.151 && t.apex[1].abs() <= 0.151,
                "apex drift bounded: {:?}",
                t.apex
            );
        }
        // Ridge: the inner ones are not all equal (there is asymmetry).
        let first = &geom.patches.tiles[0];
        assert!(
            first
                .inner_y
                .iter()
                .any(|v| (*v - first.inner_y[0]).abs() > 1e-4),
            "ridge varies: {:?}",
            first.inner_y
        );
        // Determinism: a rebuild gives the same inner bytes.
        let geom2 = build_terrain_geometry(&world, &style, &hf);
        for (a, b) in geom.patches.tiles.iter().zip(geom2.patches.tiles.iter()) {
            assert_eq!(a.inner_y, b.inner_y);
            assert_eq!(a.center_y, b.center_y);
        }
        // Water is flat: the centre is strictly the base everywhere (carve does not touch centres);
        // inner strictly the base for deep tiles (farther than 1 from the map edge:
        // the extreme corners sink by shore-carve, the old heightfield behaviour).
        let sea = test_world(6, 6, 4);
        let hf_sea = build_heightfield(&sea, &style);
        let geom_sea = build_terrain_geometry(&sea, &style, &hf_sea);
        let wb = style.height(4).unwrap();
        assert!(geom_sea.patches.tiles.iter().all(|t| t.center_y == wb));
        for r in 2..4 {
            for q in 2..4 {
                let t = &geom_sea.patches.tiles[(r * 6 + q) as usize];
                assert!(
                    t.inner_y.iter().all(|v| *v == wb),
                    "deep water inner {q},{r}: {:?}",
                    t.inner_y
                );
            }
        }
    }

    #[test]
    fn flat_params_reproduce_fan_surface() {
        // v28b: inner/edge are lerps of the old fan planes: a point inside a
        // triangle lies on the same plane (the evaluator checks this
        // separately; here are the control values).
        let style = compile(default_style()).unwrap();
        let world = test_world(6, 6, 1);
        let hf = build_heightfield(&world, &style);
        let geom = build_terrain_geometry(&world, &style, &hf);
        let base = style.height(1).unwrap();
        for t in &geom.patches.tiles {
            assert!((t.center_y - base).abs() < 1e-6);
            assert!(t.inner_y.iter().all(|v| (*v - base).abs() < 1e-4));
        }
        for c in &geom.boundary.corners {
            assert!((c.y - base).abs() < 1e-4, "flat corner {}", c.y);
        }
    }
}
