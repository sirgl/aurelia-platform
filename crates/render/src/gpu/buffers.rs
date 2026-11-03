//! CPU side of GPU data: terrain chunk meshes + unit instances. No wgpu.
//!
//! Terrain is splat blending: a vertex carries up to 4 terrains with weights
//! (at most 3 hexes touch a corner, which is plenty), the shader samples the layers of the
//! texture array and mixes them. There are no per-vertex colours any more: albedo comes from textures.
//! Why chunks and not a prism instance per tile:
//! - the tops of neighbouring hexes join bit for bit, so there are no gaps;
//! - skirts ONLY at the map edge (the surface is continuous inside);
//! - indexed mesh + 32x32 chunks + frustum cull: we draw what is visible.

use crate::scene::fog::{fog_at, fog_at_position};
use crate::scene::heightfield::{corner_key, Heightfield};
use crate::scene::layout::{tile_to_world, HEX_SIZE};
use crate::scene::style::CompiledTerrainStyle;
use crate::world::state::RenderWorld;
use kernel::ids::TileId;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, Ordering};

/// Chunk size in tiles.
pub const CHUNK: u32 = 32;

/// World units per texture repeat (detail; later moves into the style).
pub const UV_TILING: f32 = 4.0;

/// Terrain vertex: position + smooth normal + UV + splat (4 layers) + fog.
/// 44 bytes: 12 + 12 + 8 + 4 + 4 + 4. Fog: 1.0 visible / 0.45 explored / 0.18 murk.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerrainVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// 4 terrain ids, a byte each (unused slots are 0).
    pub blend_idx: u32,
    /// 4 weights 0..=255, summing to exactly 255.
    pub blend_w: u32,
    /// Fog-of-war factor of the observer.
    pub fog: f32,
    /// Water votes from carve (0..=2): a sandy strip on the shore.
    pub water: f32,
    /// Display-space continuous-surface color; alpha zero selects legacy splat.
    pub albedo: [f32; 4],
}

/// Hex corners (pointy-top), shared by all meshes: shared edges bit for bit.
pub fn hex_corners() -> [[f32; 2]; 6] {
    let mut out = [[0.0; 2]; 6];
    for i in 0..6 {
        let a = (60.0 * i as f32 - 30.0).to_radians();
        out[i as usize] = [a.cos() * HEX_SIZE, a.sin() * HEX_SIZE];
    }
    out
}
/// The neighbour across edge i comes from layout (a single source; see the docs there).
pub use crate::scene::layout::EDGE_NEIGHBORS;

/// Instance of a baked model (unit figure, world object, city piece): offset, owner
/// color, (xz scale, height scale), fog, yaw, ground slope and the animation state. 80 bytes.
///
/// World position of a model vertex p: rotate p.xz by `yaw` (radians, +x toward +z),
/// scale, then y += slope.x * dx + slope.y * dz (the ground plane under the anchor; zero
/// for upright models), then add `offset`. `transform` is the CPU twin of the shaders.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HexInstance {
    pub offset: [f32; 3],
    pub color: [f32; 3],
    pub scale_height: [f32; 2],
    /// Fog-of-war factor (as for terrain: 1.0/0.45/0.18).
    pub fog: f32,
    pub yaw: f32,
    /// Ground plane gradient (dy/dx, dy/dz) a "plane" model is sheared onto.
    pub slope: [f32; 2],
    /// Animation (vertex shader, hex.wgsl): walk phase in radians, walk amplitude 0..1,
    /// the aim and the strike of an attack 0..1.
    pub anim: [f32; 4],
    /// Animation constants of the model: action code (see `unit_poses::action_code`), rig
    /// (`Rig as u32`, 0 = none), authoring units per hex unit (1 / model scale), baked yaw.
    pub rig: [f32; 4],
    /// Presentation-only motion of the instance (`gpu::world_anim`): x = kind + 16 * state
    /// bits, y = phase (radians), z = amplitude, w = angular speed. All zero: standing still.
    pub motion: [f32; 4],
}

impl HexInstance {
    /// Upright instance: no yaw, no shear.
    pub fn upright(offset: [f32; 3], color: [f32; 3], scale: [f32; 2], fog: f32) -> Self {
        Self {
            offset,
            color,
            scale_height: scale,
            fog,
            yaw: 0.0,
            slope: [0.0, 0.0],
            anim: [0.0; 4],
            rig: [0.0; 4],
            motion: [0.0; 4],
        }
    }

    /// World position of a model-space point (same math as hex.wgsl and the shadow pass).
    pub fn transform(&self, p: [f32; 3]) -> [f32; 3] {
        let (sin, cos) = self.yaw.sin_cos();
        let dx = (p[0] * cos - p[2] * sin) * self.scale_height[0];
        let dz = (p[0] * sin + p[2] * cos) * self.scale_height[0];
        let dy = p[1] * self.scale_height[1] + self.slope[0] * dx + self.slope[1] * dz;
        [self.offset[0] + dx, self.offset[1] + dy, self.offset[2] + dz]
    }
}

/// Model vertex (units/cities, Blender bake): position + flat normal +
/// baked colour + team mask (1.0 is the owner colour, 0.0 is fixed).
/// 40B, the same size as FeatureVertex. The format is set by build.rs from the .glb.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UnitVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
    pub team: f32,
}

// The cube figurines were removed in v36: units/cities are Blender models,
// baked by build.rs from platform/assets/models/*.glb (see crate::models).
// The model dimensions are checked by the bake (units |x|,|z| <= 0.6, y <= 2.1;
// cities <= 0.7 / <= 1.5); the world scale goes through the instance's scale_height.
const OWNER_COLORS: [[f32; 3]; 8] = [
    [0.95, 0.25, 0.25],
    [0.25, 0.45, 0.95],
    [0.95, 0.80, 0.20],
    [0.30, 0.80, 0.35],
    [0.75, 0.30, 0.85],
    [0.95, 0.50, 0.20],
    [0.30, 0.85, 0.85],
    [0.85, 0.85, 0.85],
];

/// "No such player" for [`set_outsider`].
const NO_OUTSIDER: u16 = u16::MAX;
static OUTSIDER: AtomicU16 = AtomicU16::new(NO_OUTSIDER);

/// The fixed colour of the outsider player: near black, away from every colour of the palette
/// (the barbarians of the classic module are the outsider; units, borders and banners
/// are lit and tinted from it, so it stays a charcoal, not a pure black).
pub const OUTSIDER_COLOR: [f32; 3] = [0.10, 0.10, 0.12];

/// A player outside the powers (a module's own kind, such as the classic barbarians: the
/// module asks for it, otherwise it draws in the palette) is drawn in
/// [`OUTSIDER_COLOR`] instead of its palette slot. `None` (the default) puts
/// every player back in the palette. Set it before the first frame of a
/// match: units' figures keep the colour they were built with.
pub fn set_outsider(owner: Option<u8>) {
    OUTSIDER.store(owner.map_or(NO_OUTSIDER, u16::from), Ordering::Relaxed);
}

pub fn owner_color(owner: u8) -> [f32; 3] {
    if u16::from(owner) == OUTSIDER.load(Ordering::Relaxed) {
        return OUTSIDER_COLOR;
    }
    OWNER_COLORS[(owner as usize) % OWNER_COLORS.len()]
}

/// Gluing several meshes into one vertex buffer + ranges
/// (start, count) for each. Deterministic (input order).
/// Use: unit kinds / city tiers in one draw buffer.
pub fn concatenated_meshes(meshes: &[&[UnitVertex]]) -> (Vec<UnitVertex>, Vec<(u32, u32)>) {
    let mut verts = Vec::new();
    let mut ranges = Vec::with_capacity(meshes.len());
    for m in meshes {
        let start = verts.len() as u32;
        verts.extend_from_slice(m);
        ranges.push((start, m.len() as u32));
    }
    (verts, ranges)
}

/// One terrain chunk: mesh + bounding sphere for cull.
pub struct ChunkMesh {
    pub vertices: Vec<TerrainVertex>,
    pub indices: Vec<u32>,
    pub center: [f32; 3],
    pub radius: f32,
    pub tiles: usize,
}

/// Pack a corner's terrain distribution into (idx, w).
/// Weights are normalised to sum to exactly 255 (the remainder goes to the first slot).
/// style is needed to resolve terrain id -> texture array layer.
pub fn pack_blend(counts: &[(u16, u8); 4], style: &CompiledTerrainStyle) -> (u32, u32) {
    let total: u32 = counts.iter().map(|c| c.1 as u32).sum();
    if total == 0 {
        return (0, 0xFF); // degenerate: pure layer 0
    }
    let mut idx = 0u32;
    let mut w = 0u32;
    // Floor weights + the remainder (255 - sum) in the first non-zero slot.
    // The floors always sum to <= 255; a non-zero one exists when total > 0
    // (max exact >= 255/4). Empty slots get no remainder.
    let mut weights = [0u32; 4];
    for (i, (terrain, n)) in counts.iter().enumerate() {
        let layer = style.layer(*terrain).unwrap_or(0);
        idx |= layer << (i * 8);
        weights[i] = (*n as u32 * 255) / total;
    }
    let sum: u32 = weights.iter().sum();
    let rem = 255 - sum;
    if rem > 0 {
        match weights.iter().position(|x| *x > 0) {
            Some(j) => weights[j] += rem,
            None => weights[0] = 255,
        }
    }
    for (i, weight) in weights.iter().copied().enumerate() {
        w |= weight.min(255) << (i * 8);
    }
    (idx, w)
}

/// Build chunk (cx, cy) from TerrainGeometry (v28b: 19-pt patch).
/// Boundary vertices are shared inside the chunk; normals by averaging faces.
/// Deterministic: the same inputs give the same bytes.
pub fn build_chunk(
    world: &RenderWorld,
    style: &CompiledTerrainStyle,
    geom: &crate::scene::terrain::fields::TerrainGeometry,
    cx: u32,
    cy: u32,
) -> ChunkMesh {
    use crate::scene::terrain::fields::{avg_counts, patch_control_points};
    use crate::scene::terrain::{corner_lattice, edge_lattice, LatticeKey};
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut blend_idx: Vec<u32> = Vec::new();
    let mut blend_w: Vec<u32> = Vec::new();
    let mut fogs: Vec<f32> = Vec::new();
    let mut waters: Vec<f32> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut corner_vert: HashMap<LatticeKey, u32> = HashMap::new();
    let mut tiles = 0;
    // Centre/sphere: roughly from the chunk rectangle + a height margin.
    let (x0, z0) = tile_to_world(cx * CHUNK, cy * CHUNK);
    let (x1, z1) = tile_to_world(
        (cx * CHUNK + CHUNK - 1).min(world.width.saturating_sub(1)),
        (cy * CHUNK + CHUNK - 1).min(world.height.saturating_sub(1)),
    );
    let center = [(x0 + x1) * 0.5, 0.45, (z0 + z1) * 0.5];
    let radius = (((x1 - x0).powi(2) + (z1 - z0).powi(2)).sqrt() * 0.5 + 2.0).max(2.0);

    // Vertex: shared by lattice key (boundary), own when key=None.
    // Boundary data comes only from BoundaryField (cracks are impossible).
    // Shared fog is max (light if at least one referrer sees it).
    let emit = |key: Option<LatticeKey>,
                pos: [f32; 3],
                bi: u32,
                bw: u32,
                fog: f32,
                water: f32,
                positions: &mut Vec<[f32; 3]>,
                normals: &mut Vec<[f32; 3]>,
                uvs: &mut Vec<[f32; 2]>,
                blend_idx: &mut Vec<u32>,
                blend_w: &mut Vec<u32>,
                fogs: &mut Vec<f32>,
                waters: &mut Vec<f32>,
                corner_vert: &mut HashMap<LatticeKey, u32>| {
        if let Some(k) = key {
            if let Some(&idx) = corner_vert.get(&k) {
                let fi = idx as usize;
                if fog > fogs[fi] {
                    fogs[fi] = fog;
                }
                return idx;
            }
        }
        let idx = positions.len() as u32;
        positions.push(pos);
        normals.push([0.0, 0.0, 0.0]);
        uvs.push([pos[0] / UV_TILING, pos[2] / UV_TILING]);
        blend_idx.push(bi);
        blend_w.push(bw);
        fogs.push(fog);
        waters.push(water);
        if let Some(k) = key {
            corner_vert.insert(k, idx);
        }
        idx
    };

    for r in (cy * CHUNK)..((cy + 1) * CHUNK).min(world.height) {
        for q in (cx * CHUNK)..((cx + 1) * CHUNK).min(world.width) {
            let t = r * world.width + q;
            let cell = match world.terrain.get(t as usize) {
                Some(c) => c,
                None => continue,
            };
            tiles += 1;
            let tile_fog = fog_at(world, TileId(t));
            let patch = &geom.patches.tiles[t as usize];
            let layer = style.layer(cell.terrain).unwrap_or(0);
            // Control points come only from patch_control_points
            // (the same math as the evaluator: sample==mesh).
            let (o_pos, inner_pos, corner_pos, edge_pos) =
                patch_control_points(geom, TileId(t), q, r);
            // The centre and the inner ring are own (the tile's pure blend).
            let center_idx = emit(
                None,
                o_pos,
                layer,
                0xFF,
                tile_fog,
                0.0,
                &mut positions,
                &mut normals,
                &mut uvs,
                &mut blend_idx,
                &mut blend_w,
                &mut fogs,
                &mut waters,
                &mut corner_vert,
            );
            let mut inner_idx = [0u32; 6];
            for i in 0..6 {
                inner_idx[i] = emit(
                    None,
                    inner_pos[i],
                    layer,
                    0xFF,
                    tile_fog,
                    0.0,
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut blend_idx,
                    &mut blend_w,
                    &mut fogs,
                    &mut waters,
                    &mut corner_vert,
                );
            }
            // The boundary is shared, data only from BoundaryField.
            let mut ring = [0u32; 6];
            let mut edge_idx = [0u32; 6];
            for i in 0..6 {
                let cs = &geom.boundary.corners[patch.corners[i] as usize];
                let (cbi, cbw) = pack_blend(&cs.counts, style);
                ring[i] = emit(
                    Some(corner_lattice(q as i32, r as i32, i)),
                    corner_pos[i],
                    cbi,
                    cbw,
                    cs.fog,
                    cs.water as f32,
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut blend_idx,
                    &mut blend_w,
                    &mut fogs,
                    &mut waters,
                    &mut corner_vert,
                );
                let j = (i + 1) % 6;
                let es = &geom.boundary.edges[patch.edges[i] as usize];
                let ds = &geom.boundary.corners[patch.corners[j] as usize];
                let (ebi, ebw) = pack_blend(&avg_counts(&cs.counts, &ds.counts), style);
                edge_idx[i] = emit(
                    Some(edge_lattice(q as i32, r as i32, i)),
                    edge_pos[i],
                    ebi,
                    ebw,
                    es.fog,
                    es.water as f32,
                    &mut positions,
                    &mut normals,
                    &mut uvs,
                    &mut blend_idx,
                    &mut blend_w,
                    &mut fogs,
                    &mut waters,
                    &mut corner_vert,
                );
            }
            // 24 triangles of the patch (the same scheme as the evaluator):
            // 6 central + 18 outer.
            for i in 0..6usize {
                let j = (i + 1) % 6;
                let (ci, cj) = (inner_idx[i], inner_idx[j]);
                let (ri, rj) = (ring[i], ring[j]);
                let e = edge_idx[i];
                add_tri(&positions, &mut normals, &mut indices, center_idx, ci, cj);
                add_tri(&positions, &mut normals, &mut indices, ci, ri, e);
                add_tri(&positions, &mut normals, &mut indices, ci, e, cj);
                add_tri(&positions, &mut normals, &mut indices, cj, e, rj);
            }
            // Skirts only at the map edge (inside the surface is continuous).
            // The skirt top is the shared ring corners (already emitted above).
            for (i, (dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
                let nq = q as i32 + dq;
                let nr = r as i32 + dr;
                let outside =
                    nq < 0 || nr < 0 || nq >= world.width as i32 || nr >= world.height as i32;
                if !outside {
                    continue;
                }
                let (ai, bi) = (ring[i], ring[(i + 1) % 6]);
                // The skirt bottom: new vertices (y=0), an exact horizontal normal
                // (the direction from the tile centre to the edge midpoint),
                // blend and fog as at the top of the edge.
                let pa = positions[ai as usize];
                let pb = positions[bi as usize];
                let cc = positions[center_idx as usize];
                let mid = [(pa[0] + pb[0]) * 0.5 - cc[0], (pa[2] + pb[2]) * 0.5 - cc[2]];
                let len = (mid[0] * mid[0] + mid[1] * mid[1]).sqrt().max(1e-6);
                let n = [mid[0] / len, 0.0, mid[1] / len];
                let alo = positions.len() as u32;
                positions.push([pa[0], 0.0, pa[2]]);
                normals.push(n);
                uvs.push([pa[0] / UV_TILING, pa[2] / UV_TILING]);
                blend_idx.push(blend_idx[ai as usize]);
                blend_w.push(blend_w[ai as usize]);
                fogs.push(fogs[ai as usize]);
                waters.push(waters[ai as usize]);
                let blo = positions.len() as u32;
                positions.push([pb[0], 0.0, pb[2]]);
                normals.push(n);
                uvs.push([pb[0] / UV_TILING, pb[2] / UV_TILING]);
                blend_idx.push(blend_idx[bi as usize]);
                blend_w.push(blend_w[bi as usize]);
                fogs.push(fogs[bi as usize]);
                waters.push(waters[bi as usize]);
                indices.extend_from_slice(&[ai, bi, blo, ai, blo, alo]);
            }
            // EdgeField steps: see fields.rs (the dip of the edge midpoint).
            // There are no separate wall aprons: vertical quads gave
            // black "fences" (z-fighting with the slope + black ends
            // in fog), revert v33.
        }
    }
    // Finalising the top normals: normalize (skirts are already exact).
    let vertices = positions
        .into_iter()
        .zip(normals)
        .zip(uvs)
        .zip(blend_idx)
        .zip(blend_w)
        .zip(fogs)
        .zip(waters)
        .map(|((((((pos, n), uv), bi), bw), _fog), water)| {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
            TerrainVertex {
                pos,
                normal: if len < 1e-3 {
                    [0.0, 1.0, 0.0]
                } else {
                    [n[0] / len, n[1] / len, n[2] / len]
                },
                uv,
                blend_idx: bi,
                blend_w: bw,
                fog: fog_at_position(world, pos[0], pos[2]),
                water,
                albedo: [0.0; 4],
            }
        })
        .collect();
    ChunkMesh {
        vertices,
        indices,
        center,
        radius,
        tiles,
    }
}

/// A triangle into indices + a normal contribution (sign towards +Y: tops face up).
fn add_tri(
    positions: &[[f32; 3]],
    normals: &mut [[f32; 3]],
    indices: &mut Vec<u32>,
    a: u32,
    b: u32,
    c: u32,
) {
    let pa = positions[a as usize];
    let pb = positions[b as usize];
    let pc = positions[c as usize];
    let e1 = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
    let e2 = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
    let mut n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    if n[1] < 0.0 {
        n = [-n[0], -n[1], -n[2]];
    }
    for v in [a, b, c] {
        normals[v as usize][0] += n[0];
        normals[v as usize][1] += n[1];
        normals[v as usize][2] += n[2];
    }
    indices.extend_from_slice(&[a, b, c]);
}

/// Hex grid: unique top edges (LineList), lifted above
/// the surface by GRID_LIFT (no z-fighting with slopes).
/// A separate layer (§39): toggled via overlays, does not touch the simulation.
/// A shared edge is drawn once (dedup by the pair of corner keys).
pub const GRID_LIFT: f32 = 0.03;

/// Hex grid: tile edges (LineList). Exact lattice constructors
/// (corner_lattice + lattice_to_world): the trigonometry of hex_corners + the old
/// key_x/key_z gave a double scale from the origin (lines off the borders).
/// Dedup by the pair of lattice keys; heights come from heightfield + GRID_LIFT.
pub fn build_grid_lines(world: &RenderWorld, hf: &Heightfield) -> Vec<[f32; 3]> {
    use crate::scene::terrain::{corner_lattice, lattice_to_world};
    use std::collections::HashSet;
    let mut seen: HashSet<((i32, i32), (i32, i32))> = HashSet::new();
    let mut lines: Vec<[f32; 3]> = Vec::new();
    for cell in &world.terrain {
        if !world.explored.is_empty()
            && world.explored.get(cell.tile.0 as usize) != Some(&1)
            && world.visible.get(cell.tile.0 as usize) != Some(&1)
        {
            continue;
        }
        let q = (cell.tile.0 % world.width.max(1)) as i32;
        let r = (cell.tile.0 / world.width.max(1)) as i32;
        for i in 0..6 {
            let ka = corner_lattice(q, r, i);
            let kb = corner_lattice(q, r, (i + 1) % 6);
            let key = if ka < kb { (ka, kb) } else { (kb, ka) };
            if !seen.insert(key) {
                continue;
            }
            for k in [ka, kb] {
                let (x, z) = lattice_to_world(k);
                let h = hf.get(k).map(|d| d.height).unwrap_or(0.3);
                lines.push([x, h + GRID_LIFT, z]);
            }
        }
    }
    lines
}

/// Border vertex: position + owner colour (LineList, like the grid).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BorderVertex {
    pub pos: [f32; 3],
    pub color: [f32; 3],
}

/// Ownership borders: edges between tiles of different owners (including the map
/// edge at an owned tile: an island border). Each edge exactly once:
/// drawn from the side of the larger owner id (the smaller skips), the colour is
/// that of the owner whose edge it is. No coplanar duplicates by construction (z-fighting
/// is excluded). Height comes from heightfield + lift.
pub const BORDER_LIFT: f32 = 0.04;
/// Border ribbon width (world units; for a radius-1 hex the ribbon is ~1/18 of an edge).
pub const BORDER_WIDTH: f32 = 0.055;
/// The highlight is wider than the border (readable on top of it).
pub const SELECTION_WIDTH: f32 = 0.18;

pub fn build_border_lines(world: &RenderWorld, hf: &Heightfield) -> Vec<BorderVertex> {
    use crate::scene::heightfield::corner_key;
    let corners = hex_corners();
    let owner_of = |q: i32, r: i32| -> Option<u8> {
        if q < 0 || r < 0 || q >= world.width as i32 || r >= world.height as i32 {
            return None;
        }
        let t = (r as u32) * world.width + (q as u32);
        world.terrain.get(t as usize).and_then(|c| c.owner)
    };
    let mut out = Vec::new();
    for cell in &world.terrain {
        let Some(mine) = cell.owner else {
            continue;
        };
        let q = (cell.tile.0 % world.width.max(1)) as i32;
        let r = (cell.tile.0 / world.width.max(1)) as i32;
        let (wx, wz) = tile_to_world(q as u32, r as u32);
        let color = owner_color(mine);
        for (i, (dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
            let other = owner_of(q + dq, r + dr);
            // Draw an edge if the neighbour is foreign/empty AND (the foreign id is smaller than mine
            // or there is no neighbour): exactly one emitter per border.
            let emit = match other {
                None => true,
                Some(o) if o != mine => o < mine,
                _ => false,
            };
            if !emit {
                continue;
            }
            let (a, b) = (corners[i], corners[(i + 1) % 6]);
            for (cx, cz) in [(wx + a[0], wz + a[1]), (wx + b[0], wz + b[1])] {
                let h = hf.get(corner_key(cx, cz)).map(|d| d.height).unwrap_or(0.3);
                out.push(BorderVertex {
                    pos: [cx, h + BORDER_LIFT, cz],
                    color,
                });
            }
        }
    }
    out
}

/// Highlight segment key: a quantised pair of corners (for sort+dedup).
type SegKey = ((i32, i32), (i32, i32));

/// Ribbon end for a joint disc: quantised key + (x, z, colour).
type JointKey = ((i32, i32), (f32, f32, [f32; 3]));

/// Highlight of the selected city's work area: outlines of all ring tiles
/// (including inner edges, so the zone is visible, not just the perimeter).
/// In white, lift above the borders (lies on top). Dedup via sort+dedup
/// of quantised pairs (deterministic; HashSet iteration is not).
/// Reuses BorderVertex + border_pipeline: no new shader.
pub const SELECTION_LIFT: f32 = 0.07;
pub const SELECTION_COLOR: [f32; 3] = [1.0, 1.0, 1.0];
/// Tiles taken off work: a dim red outline (the decision is visible).
pub const UNWORKED_COLOR: [f32; 3] = [0.65, 0.35, 0.35];

pub fn build_city_highlight(
    world: &RenderWorld,
    hf: &Heightfield,
    center: TileId,
    radius: i32,
    dim: &[TileId],
) -> Vec<BorderVertex> {
    let dims = kernel::math::MapDimensions {
        width: world.width,
        height: world.height,
    };
    let corners = hex_corners();
    // Collect segment keys with a dim flag, sort, deduplicate
    // (a shared worked/dim edge is dim: the exclusion must be visible),
    // then build. Deterministic.
    let quant = |v: f32| (v * 1e4).round() as i32;
    let mut keys: Vec<(SegKey, bool)> = Vec::new();
    for t in kernel::math::tiles_in_hex_radius(&dims, center, radius) {
        let w = world.width.max(1);
        let (wx, wz) = tile_to_world(t.0 % w, t.0 / w);
        let is_dim = dim.contains(&t);
        for i in 0..6 {
            let (a, b) = (corners[i], corners[(i + 1) % 6]);
            let ka = (quant(wx + a[0]), quant(wz + a[1]));
            let kb = (quant(wx + b[0]), quant(wz + b[1]));
            let seg = if ka < kb { (ka, kb) } else { (kb, ka) };
            keys.push((seg, is_dim));
        }
    }
    keys.sort();
    // Manual dedup: duplicates are merged, dim wins.
    let mut uniq: Vec<(SegKey, bool)> = Vec::with_capacity(keys.len());
    for (seg, is_dim) in keys {
        match uniq.last_mut() {
            Some(last) if last.0 == seg => {
                last.1 = last.1 || is_dim;
            }
            _ => uniq.push((seg, is_dim)),
        }
    }
    uniq.into_iter()
        .flat_map(|(((ax, az), (bx, bz)), is_dim)| {
            let color = if is_dim {
                UNWORKED_COLOR
            } else {
                SELECTION_COLOR
            };
            [(ax, az), (bx, bz)].into_iter().map(move |(qx, qz)| {
                let (cx, cz) = (qx as f32 / 1e4, qz as f32 / 1e4);
                let h = hf.get(corner_key(cx, cz)).map(|d| d.height).unwrap_or(0.3);
                BorderVertex {
                    pos: [cx, h + SELECTION_LIFT, cz],
                    color,
                }
            })
        })
        .collect()
}

/// Ribbon from segments: WebGPU lines are always 1px, so the thickness is given
/// by geometry (two triangles per segment, non-indexed). Corner heights come
/// from heightfield + lift (the ribbon lies on the surface, the depth test cleans up).
/// Colours are preserved (the same BorderVertex/border.wgsl: no new shader).
/// Degenerate segments (<1e-6) are skipped, so no garbage goes into the buffer.
/// Joints are closed with discs (otherwise the lower layer shows in the gaps at corners).
pub fn build_ribbon_tris(
    lines: &[BorderVertex],
    width: f32,
    hf: &Heightfield,
    lift: f32,
) -> Vec<BorderVertex> {
    use crate::scene::heightfield::corner_key;
    let mut out = Vec::with_capacity(lines.len() * 3);
    // Segment ends for discs: quantised key -> (x, z, colour).
    // Deterministic: Vec + stable sort, the colour is that of the first segment
    // (HashMap iteration is non-deterministic, so we do not use a map).
    let quant = |v: f32| (v * 1e4).round() as i32;
    let mut joints: Vec<JointKey> = Vec::new();
    for pair in lines.chunks(2) {
        if pair.len() < 2 {
            continue;
        }
        let (a, b) = (pair[0], pair[1]);
        let dx = b.pos[0] - a.pos[0];
        let dz = b.pos[2] - a.pos[2];
        let len = (dx * dx + dz * dz).sqrt();
        if len < 1e-6 {
            continue;
        }
        let (nx, nz) = (-dz / len * width * 0.5, dx / len * width * 0.5);
        let corners = [
            (a.pos[0] + nx, a.pos[2] + nz, a.color),
            (a.pos[0] - nx, a.pos[2] - nz, a.color),
            (b.pos[0] + nx, b.pos[2] + nz, b.color),
            (b.pos[0] - nx, b.pos[2] - nz, b.color),
        ];
        // Cull off, so the order does not matter; we fix (0,1,2)(1,3,2) for stability.
        for &(cx, cz, color) in [
            &corners[0],
            &corners[1],
            &corners[2],
            &corners[1],
            &corners[3],
            &corners[2],
        ] {
            let h = hf.get(corner_key(cx, cz)).map(|d| d.height).unwrap_or(0.3);
            // The height is resampled (the input lift is ignored, the ribbon has its own),
            // + a quarter of the width so that the edge does not sink into the slope.
            out.push(BorderVertex {
                pos: [cx, h + lift + width * 0.25, cz],
                color,
            });
        }
        joints.push((
            (quant(a.pos[0]), quant(a.pos[2])),
            (a.pos[0], a.pos[2], a.color),
        ));
        joints.push((
            (quant(b.pos[0]), quant(b.pos[2])),
            (b.pos[0], b.pos[2], b.color),
        ));
    }
    // Joint discs (round caps): a fan of 6 triangles of radius width/2.
    joints.sort_by_key(|j| j.0);
    let mut last: Option<(i32, i32)> = None;
    let r = width * 0.5;
    for (key, (cx, cz, color)) in &joints {
        if last == Some(*key) {
            continue;
        }
        last = Some(*key);
        let h = hf
            .get(corner_key(*cx, *cz))
            .map(|d| d.height)
            .unwrap_or(0.3);
        let y = h + lift + width * 0.25;
        for i in 0..6 {
            let a0 = std::f32::consts::TAU * i as f32 / 6.0;
            let a1 = std::f32::consts::TAU * (i + 1) as f32 / 6.0;
            out.push(BorderVertex {
                pos: [*cx, y, *cz],
                color: *color,
            });
            out.push(BorderVertex {
                pos: [cx + r * a0.cos(), y, cz + r * a0.sin()],
                color: *color,
            });
            out.push(BorderVertex {
                pos: [cx + r * a1.cos(), y, cz + r * a1.sin()],
                color: *color,
            });
        }
    }
    out
}

/// Yield icon: a flat quad on the tile (as in Nation: food/production visible at once).
/// They lie on the surface (no billboard needed, they read from above), only
/// explored tiles (they remember yields honestly). There is no money: there is no gold mechanic,
/// drawing invented things is forbidden (§HUD honesty).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct YieldInstance {
    pub offset: [f32; 3],
    /// cell.x = icon (0 food, 1 production), cell.y = quad size.
    pub cell_scale: [f32; 2],
}

pub const YIELD_ATLAS_CELLS: u32 = 6;
pub const YIELD_ATLAS_CELL: u32 = 32;
pub const YIELD_ICON_SCALE: f32 = 0.36;
pub const YIELD_ICON_LIFT: f32 = 0.10;
pub const YIELD_ICON_GAP: f32 = 0.40;

/// Icon atlas 192x32 RGBA: food is an apple, production a gear,
/// wheat an ear, iron an ingot, gold a nugget, road a highway.
/// 32px/cell: bold silhouettes + a 1px auto-outline: readable up close
/// and on distant tiles (the old 16px ones with white highlights merged into mush).
/// Row-major layout horizontally (6 cells per row): exactly how
/// the GPU and the shader read the texture (u = (cell + uv)/6); cell-major gave the
/// shader a mix of all cells, so the icons were always garbage.
/// Procedural (deterministic, no assets); transparent background.
pub fn build_yield_atlas() -> (u32, u32, Vec<u8>) {
    const S: usize = 32;
    const CELLS: usize = 6;
    let mut px = vec![0u8; CELLS * S * S * 4];
    fn set(px: &mut [u8], cell: usize, x: i32, y: i32, c: [u8; 4]) {
        if x < 0 || y < 0 || x >= S as i32 || y >= S as i32 {
            return;
        }
        let i = (y as usize * (CELLS * S) + cell * S + x as usize) * 4;
        px[i..i + 4].copy_from_slice(&c);
    }
    fn alpha(px: &[u8], cell: usize, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= S as i32 || y >= S as i32 {
            return 0;
        }
        px[(y as usize * (CELLS * S) + cell * S + x as usize) * 4 + 3]
    }
    /// Auto-outline: an empty pixel adjacent to a silhouette becomes a dark
    /// outline: a single readable edge on all six icons (collected in two
    /// passes, no cascade).
    fn outline(px: &mut [u8], cell: usize, c: [u8; 4]) {
        let mut add = Vec::new();
        for y in 0..S as i32 {
            for x in 0..S as i32 {
                if alpha(px, cell, x, y) == 0
                    && (alpha(px, cell, x - 1, y) > 0
                        || alpha(px, cell, x + 1, y) > 0
                        || alpha(px, cell, x, y - 1) > 0
                        || alpha(px, cell, x, y + 1) > 0)
                {
                    add.push((x, y));
                }
            }
        }
        for (x, y) in add {
            set(px, cell, x, y, c);
        }
    }
    // Apple: body r=10.5, stem, leaf, a restrained highlight; the large
    // white spots of the old version read as dirt when upscaled.
    for y in 0..S as i32 {
        for x in 0..S as i32 {
            let d = ((x as f32 - 15.5).powi(2) + (y as f32 - 18.5).powi(2)).sqrt();
            if d <= 10.5 {
                set(&mut px, 0, x, y, [232, 64, 56, 255]);
            }
        }
    }
    for y in 6..=11 {
        set(&mut px, 0, 15, y, [112, 72, 32, 255]);
        set(&mut px, 0, 16, y, [112, 72, 32, 255]);
    }
    for y in 0..S as i32 {
        for x in 0..S as i32 {
            let ex = (x as f32 - 22.0) / 5.0;
            let ey = (y as f32 - 9.0) / 3.0;
            if ex * ex + ey * ey <= 1.0 && x >= 17 && y <= 13 {
                set(&mut px, 0, x, y, [56, 154, 62, 255]);
            }
        }
    }
    for y in 0..S as i32 {
        for x in 0..S as i32 {
            let d = ((x as f32 - 11.5).powi(2) + (y as f32 - 15.0).powi(2)).sqrt();
            if d <= 3.0 {
                set(&mut px, 0, x, y, [255, 178, 158, 255]);
            }
        }
    }
    outline(&mut px, 0, [118, 24, 20, 255]);
    // Gear (production): 8 wide teeth (r 10.0/13.4), a through hub.
    for y in 0..S as i32 {
        for x in 0..S as i32 {
            let dx = x as f32 - 15.5;
            let dy = y as f32 - 15.5;
            let r = (dx * dx + dy * dy).sqrt();
            if r >= 14.6 {
                continue;
            }
            let mut t = dy.atan2(dx) * 8.0 / std::f32::consts::TAU;
            t = ((t % 1.0) + 1.0) % 1.0;
            let edge = if t < 0.55 { 13.4 } else { 10.0 };
            if r > edge || r < 4.2 {
                continue;
            }
            set(&mut px, 1, x, y, [246, 188, 58, 255]);
        }
    }
    outline(&mut px, 1, [132, 82, 16, 255]);
    // Ear: a through stem + two braids of grains staggered (3x3, a gap between
    // the rows, the auto-outline draws a dark seam, the grains are separated).
    // The old braids touching each other merged into a solid "tent": it did not read.
    for y in 1..=29 {
        set(&mut px, 2, 15, y, [176, 138, 40, 255]);
        set(&mut px, 2, 16, y, [176, 138, 40, 255]);
    }
    for y in 1..=3 {
        for x in 15..=16 {
            set(&mut px, 2, x, y, [234, 196, 74, 255]);
        }
    }
    for (yb, x0) in [
        (6, 12),
        (10, 12),
        (14, 12),
        (18, 12),
        (4, 17),
        (8, 17),
        (12, 17),
        (16, 17),
    ] {
        for yy in yb..=yb + 2 {
            for x in x0..=x0 + 2 {
                let c = if yy == yb {
                    [250, 218, 116, 255]
                } else if yy == yb + 2 {
                    [204, 164, 50, 255]
                } else {
                    [234, 196, 74, 255]
                };
                set(&mut px, 2, x, yy, c);
            }
        }
    }
    outline(&mut px, 2, [140, 100, 26, 255]);
    // Ingot: a truncated pyramid, a light top face with a highlight, the face
    // edge, the body widens downwards. The old variant narrowed downwards and
    // read as a cup, not an ingot.
    for y in 8..=11 {
        for x in 8..=24 {
            set(&mut px, 3, x, y, [214, 220, 236, 255]);
        }
    }
    for y in 9..=10 {
        for x in 10..=14 {
            set(&mut px, 3, x, y, [246, 248, 253, 255]);
        }
    }
    for x in 8..=24 {
        set(&mut px, 3, x, 12, [110, 118, 142, 255]);
    }
    for y in 13..=21 {
        let half = 8 + (y - 13) * 3 / 8;
        for x in (16 - half)..=(16 + half) {
            let c = if y == 21 {
                [148, 156, 178, 255]
            } else {
                [176, 184, 204, 255]
            };
            set(&mut px, 3, x, y, c);
        }
    }
    outline(&mut px, 3, [84, 90, 108, 255]);
    // Nugget: a cluster of three "stones" + a highlight.
    let nug = [(12.0f32, 19.0, 7.0), (19.0, 14.5, 6.5), (21.0, 21.0, 5.5)];
    for y in 0..S as i32 {
        for x in 0..S as i32 {
            if nug
                .iter()
                .any(|&(cx, cy, r)| (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2) <= r * r)
            {
                set(&mut px, 4, x, y, [238, 188, 52, 255]);
            }
        }
    }
    for y in 0..S as i32 {
        for x in 0..S as i32 {
            let d = (x as f32 - 10.5).powi(2) + (y as f32 - 16.5).powi(2);
            if d <= 6.5 {
                set(&mut px, 4, x, y, [255, 230, 134, 255]);
            }
        }
    }
    outline(&mut px, 4, [146, 104, 18, 255]);
    // Road: a highway with shoulders and a dashed centre line.
    for y in 11..=21 {
        for x in 3..=28 {
            let c = if y == 11 || y == 21 {
                [118, 80, 36, 255]
            } else {
                [164, 120, 62, 255]
            };
            set(&mut px, 5, x, y, c);
        }
    }
    for x in 3..=28 {
        if (x - 3) % 7 < 4 {
            for y in 15..=17 {
                set(&mut px, 5, x, y, [234, 214, 170, 255]);
            }
        }
    }
    outline(&mut px, 5, [96, 64, 30, 255]);
    (CELLS as u32 * S as u32, S as u32, px)
}

/// Food and production icons of a bare terrain code. The projection carries
/// no tile yields yet, so the icons show the terrain alone (base
/// yields of the map codes: plains 1/1, grassland 2/0, hills 0/2, desert,
/// snow and mountains 0/0; water is not drawn); the game HUD reads the full
/// tile yields from the core (`derived::tile_yields`).
pub fn terrain_icon_food_prod(terrain: u16) -> (i32, i32) {
    match terrain {
        0 => (1, 1),
        1 => (2, 0),
        2 => (0, 2),
        _ => (0, 0),
    }
}

/// Icon instances: food in the top row, production in the bottom (as in Nation: reads
/// instantly and does not overlap neighbours: a row of <=3 icons x a step narrower than a hex).
/// One icon per unit (cap 3+3). Only explored land with yields > 0.
/// Anchors come via the evaluator (relief v28c: style.height lies on hills).
pub fn build_yield_instances(
    world: &RenderWorld,
    style: &CompiledTerrainStyle,
    geom: &crate::scene::terrain::fields::TerrainGeometry,
) -> Vec<YieldInstance> {
    let mut out = Vec::new();
    for (i, cell) in world.terrain.iter().enumerate() {
        if world.explored.get(i).copied().unwrap_or(0) == 0 {
            continue;
        }
        let (f, p) = terrain_icon_food_prod(cell.terrain);
        let nf = f.clamp(0, 3) as usize;
        let np = p.clamp(0, 3) as usize;
        let n = nf + np;
        if n == 0 {
            continue;
        }
        let w = world.width.max(1);
        let (wx, wz) = tile_to_world(cell.tile.0 % w, cell.tile.0 / w);
        let ground = crate::scene::terrain::evaluator::sample_surface(geom, wx, wz)
            .map(|s| s.y)
            .unwrap_or_else(|| style.height(cell.terrain).unwrap_or(0.3));
        // Rows are centred SEPARATELY (index within its own row):
        // a shared counter made them overlap neighbouring tiles.
        const ROW_DZ: f32 = 0.20;
        for (k, (is_food, dz)) in (0..nf)
            .map(|_| (true, -ROW_DZ))
            .chain((0..np).map(|_| (false, ROW_DZ)))
            .enumerate()
        {
            let (row_k, row_n) = if is_food { (k, nf) } else { (k - nf, np) };
            let xoff = (row_k as f32 - (row_n - 1) as f32 * 0.5) * YIELD_ICON_GAP;
            out.push(YieldInstance {
                offset: [wx + xoff, ground + YIELD_ICON_LIFT, wz + dz],
                cell_scale: [if is_food { 0.0 } else { 1.0 }, YIELD_ICON_SCALE],
            });
        }
        // Resource: one icon under the rows (only unlocked ones;
        // those closed until the tech are not shown at all, like the undiscovered in Nation).
        // Positional convention (layers do not know content keys, §1):
        // runtime id 0/1/2 -> cells 2/3/4; other resources have no icon
        // (it needs a new atlas cell: an art bump, not quiet lying).
        if let Some(rid) = cell.resource {
            if rid < 3 && world.unlocked_resources.contains(&rid) {
                out.push(YieldInstance {
                    offset: [wx, ground + YIELD_ICON_LIFT, wz + ROW_DZ * 2.0 + 0.05],
                    cell_scale: [2.0 + rid as f32, YIELD_ICON_SCALE],
                });
            }
        }
        // Road: a highway at the tile centre (only explored, like yields).
        // The position is the centre: roads are visible even without yield icons beside them.
        if world.road.get(i).copied().unwrap_or(0) != 0
            && world.explored.get(i).copied().unwrap_or(0) != 0
        {
            out.push(YieldInstance {
                offset: [wx, ground + YIELD_ICON_LIFT, wz],
                cell_scale: [5.0, YIELD_ICON_SCALE * 1.5],
            });
        }
    }
    out
}
/// Rivers: segments ALONG HEX EDGES (as in Nation), by canonical pairs
/// (min,max) of ids. The direction a->b is found by axes (EDGE_NEIGHBORS order);
/// a non-adjacent pair (broken data) is skipped silently.
/// Heights y=0: they are set by the ribbon from heightfield (already with shore carve).
pub const RIVER_COLOR: [f32; 3] = [0.15, 0.36, 0.75];
pub const RIVER_WIDTH: f32 = 0.22;
/// The water lift above the ground = the single waterline (see style::WATER_SURFACE_LIFT).
pub const RIVER_LIFT: f32 = crate::scene::style::WATER_SURFACE_LIFT;

/// River bed path point: position + fog (max over neighbouring tiles).
#[derive(Debug, Clone, Copy)]
pub struct PathPt {
    pub pos: [f32; 2],
    pub fog: f32,
}

/// Geometric segments of the beds: pairs of corner points (x,z) + fog.
/// Shared logic with build_river_lines (edge -> corners i,i+1), without colours/heights.
fn river_segments(world: &RenderWorld) -> Vec<([f32; 2], [f32; 2], f32)> {
    let corners = hex_corners();
    let mut out = Vec::new();
    for (a, b) in &world.river_edges {
        // An edge at water is not drawn as a ribbon: there is the lake surface with its own foam,
        // and the bed edges would give a neon seam across the lake. The path ends
        // at the shore with a wedge (source/mouth), stitched in build_river_paths.
        let water_side = [a, b].iter().any(|t| world.is_water(**t as usize));
        if water_side {
            continue;
        }
        let w = world.width.max(1);
        let (aq, ar) = ((a % w) as i32, (a / w) as i32);
        let (bq, br) = ((b % w) as i32, (b / w) as i32);
        let dir = EDGE_NEIGHBORS
            .iter()
            .position(|(dq, dr)| aq + dq == bq && ar + dr == br);
        let Some(i) = dir else { continue };
        let (wx, wz) = tile_to_world(aq as u32, ar as u32);
        let (ca, cb) = (corners[i], corners[(i + 1) % 6]);
        let fog = fog_at(world, TileId(*a)).max(fog_at(world, TileId(*b)));
        out.push(([wx + ca[0], wz + ca[1]], [wx + cb[0], wz + cb[1]], fog));
    }
    out
}

/// Connected polylines of the beds: segments are stitched at shared corners (corner_key).
/// Deterministic: traversal in segment order, forks cut the path
/// (a vertex of degree != 2 is a path end; it gets a disc, see below).
/// An empty map is empty; broken edges are already filtered out in river_segments.
pub fn build_river_paths(world: &RenderWorld) -> Vec<Vec<PathPt>> {
    use crate::scene::heightfield::CornerKey;
    let segs = river_segments(world);
    // Adjacency: corner -> segments ending at it.
    let mut at: HashMap<CornerKey, Vec<usize>> = HashMap::new();
    for (idx, (a, b, _)) in segs.iter().enumerate() {
        at.entry(corner_key(a[0], a[1])).or_default().push(idx);
        at.entry(corner_key(b[0], b[1])).or_default().push(idx);
    }
    // Segment point with fog.
    let pt = |p: [f32; 2], f: f32| PathPt { pos: p, fog: f };
    let mut used = vec![false; segs.len()];
    let mut paths = Vec::new();
    for start in 0..segs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (sa, sb, sf) = segs[start];
        let mut pts = vec![pt(sa, sf), pt(sb, sf)];
        // Forward from the end while there is exactly one continuation (a fork stops).
        loop {
            let tip = pts.last().unwrap().pos;
            let next = at
                .get(&corner_key(tip[0], tip[1]))
                .map(|v| v.iter().filter(|i| !used[**i]).collect::<Vec<_>>())
                .unwrap_or_default();
            if next.len() != 1 {
                break;
            }
            let ni = *next[0];
            used[ni] = true;
            let (a, b, f) = segs[ni];
            let ka = corner_key(a[0], a[1]);
            let tipk = corner_key(tip[0], tip[1]);
            let (np, nf) = if ka == tipk { (b, f) } else { (a, f) };
            // Shared corner: fog max (the point is light if at least one path is light).
            let tip_fog = pts.last().unwrap().fog.max(nf);
            pts.last_mut().unwrap().fog = tip_fog;
            pts.push(pt(np, nf));
        }
        // Backward from the start, symmetrically.
        loop {
            let tip = pts[0].pos;
            let tipf = pts[0].fog;
            let next = at
                .get(&corner_key(tip[0], tip[1]))
                .map(|v| v.iter().filter(|i| !used[**i]).collect::<Vec<_>>())
                .unwrap_or_default();
            if next.len() != 1 {
                break;
            }
            let ni = *next[0];
            used[ni] = true;
            let (a, b, f) = segs[ni];
            let ka = corner_key(a[0], a[1]);
            let tipk = corner_key(tip[0], tip[1]);
            let (np, nf) = if ka == tipk { (b, f) } else { (a, f) };
            pts[0].fog = tipf.max(nf);
            pts.insert(0, pt(np, nf));
        }
        paths.push(pts);
    }
    paths
}

pub fn build_river_lines(world: &RenderWorld) -> Vec<BorderVertex> {
    // The bed runs ALONG EDGES (as in Nation). Geometry comes from river_segments;
    // height later (ribbons resample from heightfield with shore carve).
    // A non-adjacent pair (broken data) is skipped silently, in river_segments.
    river_segments(world)
        .into_iter()
        .flat_map(|(a, b, _)| {
            [
                BorderVertex {
                    pos: [a[0], 0.0, a[1]],
                    color: RIVER_COLOR,
                },
                BorderVertex {
                    pos: [b[0], 0.0, b[1]],
                    color: RIVER_COLOR,
                },
            ]
        })
        .collect()
}
/// Water surface vertex: position + uv (u along the world, v across:
/// 0/1 shore, 0.5 middle: foam and depth in the shader from v) + shade
/// (ocean depth 0/1: large connected bodies of water are darker; rivers are always 0).
/// 44B, including actual distance to the dry shoreline for continuous water.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WaterVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// Fog-of-war factor (as for terrain: 1.0/0.45/0.18).
    pub fog: f32,
    /// Ocean depth (0 is river/lake, 1 is a body of water of >= OCEAN_BODY tiles).
    pub shade: f32,
    /// Metres to the dry shoreline; negative selects the legacy foam profile.
    pub shore: f32,
    /// River water only: the weight of the river mouth (1: the sea's own colour), the position
    /// on the water palette (0 clear .. 1 icy) and the weight of white water. Zero elsewhere.
    pub tone: [f32; 3],
}

/// Bed width irregularity: 0.90 + sines along the path length.
/// The phase is a deterministic hash of the first point's coordinates (not RNG:
/// the render is deterministic with the same map seed). This is NOT "wider towards the mouth":
/// there is no flow direction in the data (the edges are canonical min/max),
/// only a pleasant unevenness instead of a mechanical ribbon.
fn width_scale(s: f32, phase: f32) -> f32 {
    0.90 + 0.07 * (s * 1.21 + phase).sin() + 0.04 * (s * 2.73 + phase * 0.7).sin()
}

fn width_phase(p: [f32; 2]) -> f32 {
    let h = (p[0] * 13.7 + p[1] * 7.3).fract().abs();
    h * std::f32::consts::TAU
}
/// Water surface as connected polylines: miter joints instead of independent quads
/// (gaps on the outer side of a bend and dark overlap wedges go away).
/// The width is uneven (width_scale along the length, phase from coordinates, not RNG).
/// v: 0/1 shore, 0.5 middle; u is the path length x0.35 (the ripple flows without seams).
/// Path ends: single ones taper with a wedge x0.12 (source/mouth),
/// those shared by >=2 paths are closed with a disc r=width/2 (forks without notches).
/// Deterministic (path order = edge order).
pub fn build_water_strip(
    paths: &[Vec<PathPt>],
    width: f32,
    hf: &Heightfield,
    lift: f32,
) -> Vec<WaterVertex> {
    use crate::scene::heightfield::corner_key;
    let hw = width * 0.5;
    let mut out = Vec::new();
    // Water point: height from heightfield + the common lift + the point's fog.
    // Rivers are always shade 0 (ocean only for large lakes/seas).
    let wet = |cx: f32, cz: f32, u: f32, v: f32, fog: f32, out: &mut Vec<WaterVertex>| {
        let h = hf.get(corner_key(cx, cz)).map(|d| d.height).unwrap_or(0.3);
        out.push(WaterVertex {
            pos: [cx, h + lift + width * 0.25, cz],
            uv: [u, v],
            fog,
            shade: 0.0,
            shore: -1.0,
            tone: [0.; 3],
        });
    };
    // End degrees: an end shared by >=2 paths is a fork (a disc),
    // a single one is a source/mouth (a wedge by tapering). The end's fog is the max of the paths.
    use crate::scene::heightfield::CornerKey;
    use std::collections::HashSet;
    let mut ends: HashMap<CornerKey, Vec<(PathPt, f32)>> = HashMap::new();
    for path in paths {
        if path.len() < 2 {
            continue;
        }
        // Path length for u (the ends need it too).
        let mut len = 0.0f32;
        for w in path.windows(2) {
            let dx = w[1].pos[0] - w[0].pos[0];
            let dz = w[1].pos[1] - w[0].pos[1];
            len += (dx * dx + dz * dz).sqrt();
        }
        let first = path[0];
        let last = path[path.len() - 1];
        ends.entry(corner_key(first.pos[0], first.pos[1]))
            .or_default()
            .push((first, 0.0));
        ends.entry(corner_key(last.pos[0], last.pos[1]))
            .or_default()
            .push((last, len * 0.35));
    }
    for path in paths {
        if path.len() < 2 {
            continue;
        }
        // Path length for u + width scale along the length (an uneven bed).
        let mut cum = vec![0.0f32; path.len()];
        for i in 1..path.len() {
            let dx = path[i].pos[0] - path[i - 1].pos[0];
            let dz = path[i].pos[1] - path[i - 1].pos[1];
            cum[i] = cum[i - 1] + (dx * dx + dz * dz).sqrt();
        }
        let phase = width_phase(path[0].pos);
        let ws: Vec<f32> = cum.iter().map(|s| width_scale(*s, phase)).collect();
        // Segment normals.
        let mut seg_n = Vec::with_capacity(path.len() - 1);
        for w in path.windows(2) {
            let dx = w[1].pos[0] - w[0].pos[0];
            let dz = w[1].pos[1] - w[0].pos[1];
            let len = (dx * dx + dz * dz).sqrt().max(1e-6);
            seg_n.push([-dz / len, dx / len]);
        }
        // Miter at a point: the bisector of the neighbours' normals (ends use the segment normal).
        // Hex grid: a bend >=60 degrees, miter <=2.0: a clamp against hairpin turns.
        let mut off: Vec<[f32; 2]> = Vec::with_capacity(path.len());
        for i in 0..path.len() {
            let nn: [f32; 2] = if i == 0 {
                seg_n[0]
            } else if i == path.len() - 1 {
                seg_n[seg_n.len() - 1]
            } else {
                let a = seg_n[i - 1];
                let b = seg_n[i];
                let (mx, mz) = (a[0] + b[0], a[1] + b[1]);
                let mlen = (mx * mx + mz * mz).sqrt().max(1e-6);
                let (ux, uz) = (mx / mlen, mz / mlen);
                let scale = (1.0 / (ux * a[0] + uz * a[1]).max(0.5)).min(2.0);
                [ux * scale, uz * scale]
            };
            off.push(nn);
        }
        // Source/mouth wedge: single ends taper x0.12.
        for end in [0, path.len() - 1] {
            let p = path[end].pos;
            if ends
                .get(&corner_key(p[0], p[1]))
                .map(|v| v.len())
                .unwrap_or(0)
                < 2
            {
                off[end][0] *= 0.12;
                off[end][1] *= 0.12;
            }
        }
        // A strip of triangles (fog from its own points; the width is its own).
        for i in 0..path.len() - 1 {
            let u0 = cum[i] * 0.35;
            let u1 = cum[i + 1] * 0.35;
            let (p0, f0) = (path[i].pos, path[i].fog);
            let (p1, f1) = (path[i + 1].pos, path[i + 1].fog);
            let (n0, n1) = (off[i], off[i + 1]);
            let (hw0, hw1) = (hw * ws[i], hw * ws[i + 1]);
            let l0 = [p0[0] + n0[0] * hw0, p0[1] + n0[1] * hw0];
            let r0 = [p0[0] - n0[0] * hw0, p0[1] - n0[1] * hw0];
            let l1 = [p1[0] + n1[0] * hw1, p1[1] + n1[1] * hw1];
            let r1 = [p1[0] - n1[0] * hw1, p1[1] - n1[1] * hw1];
            wet(l0[0], l0[1], u0, 0.0, f0, &mut out);
            wet(r0[0], r0[1], u0, 1.0, f0, &mut out);
            wet(l1[0], l1[1], u1, 0.0, f1, &mut out);
            wet(r0[0], r0[1], u0, 1.0, f0, &mut out);
            wet(r1[0], r1[1], u1, 1.0, f1, &mut out);
            wet(l1[0], l1[1], u1, 0.0, f1, &mut out);
        }
    }
    // Fork discs: a shared end of >=2 paths is closed with a fan of 8 triangles
    // (centre v=0.5, rim v=1.0; fog is the max of the ends): notches at joints go away.
    let mut capped: HashSet<CornerKey> = HashSet::new();
    for (key, pts) in &ends {
        if pts.len() < 2 || !capped.insert(*key) {
            continue;
        }
        let (pp, u) = pts[0];
        let (p, fog) = (pp.pos, pts.iter().map(|(q, _)| q.fog).fold(0.0, f32::max));
        let mut prev = [p[0] + hw, p[1]];
        for k in 1..=8 {
            let a = std::f32::consts::TAU * k as f32 / 8.0;
            let cur = [p[0] + hw * a.cos(), p[1] + hw * a.sin()];
            wet(p[0], p[1], u, 0.5, fog, &mut out);
            wet(prev[0], prev[1], u, 1.0, fog, &mut out);
            wet(cur[0], cur[1], u, 1.0, fog, &mut out);
            prev = cur;
        }
    }
    out
}
#[derive(Clone, Copy)]
struct WaterCoast {
    a: [f32; 2],
    b: [f32; 2],
}

fn known_water_coasts(world: &RenderWorld) -> HashMap<TileId, Vec<WaterCoast>> {
    let mut coasts = HashMap::new();
    let corners = hex_corners();
    for cell in &world.terrain {
        if cell.terrain != 4 || fog_at(world, cell.tile) < 0.4 {
            continue;
        }
        let q = (cell.tile.0 % world.width.max(1)) as i32;
        let r = (cell.tile.0 / world.width.max(1)) as i32;
        let (x, z) = tile_to_world(q as u32, r as u32);
        for (edge, (dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
            let nq = q + dq;
            let nr = r + dr;
            if nq < 0 || nr < 0 || nq >= world.width as i32 || nr >= world.height as i32 {
                continue;
            }
            let tile = TileId(nr as u32 * world.width + nq as u32);
            // An unexplored sanitized tile is not a discovered coastline.
            if fog_at(world, tile) < 0.4 {
                continue;
            }
            if world
                .terrain
                .get(tile.0 as usize)
                .is_none_or(|c| c.terrain == 4)
            {
                continue;
            }
            let a = corners[edge];
            let b = corners[(edge + 1) % 6];
            coasts
                .entry(cell.tile)
                .or_insert_with(Vec::new)
                .push(WaterCoast {
                    a: [x + a[0], z + a[1]],
                    b: [x + b[0], z + b[1]],
                });
        }
    }
    coasts
}

fn water_coast_distance(coasts: &[WaterCoast], x: f32, z: f32) -> f32 {
    let mut distance = 2.0f32;
    for coast in coasts {
        let dx = coast.b[0] - coast.a[0];
        let dz = coast.b[1] - coast.a[1];
        let t =
            (((x - coast.a[0]) * dx + (z - coast.a[1]) * dz) / (dx * dx + dz * dz)).clamp(0.0, 1.0);
        distance = distance
            .min(((x - coast.a[0] - dx * t).powi(2) + (z - coast.a[1] - dz * t).powi(2)).sqrt());
    }
    distance
}

fn lake_tile_mesh(
    out: &mut Vec<WaterVertex>,
    world: &RenderWorld,
    tile: TileId,
    y: f32,
    shade: f32,
    coasts: &HashMap<TileId, Vec<WaterCoast>>,
) {
    let w = world.width.max(1);
    let q = (tile.0 % w) as i32;
    let r = (tile.0 / w) as i32;
    let (x, z) = tile_to_world(q as u32, r as u32);
    let mut nearby = Vec::new();
    for nr in (r - 2).max(0)..=(r + 2).min(world.height as i32 - 1) {
        for nq in (q - 2).max(0)..=(q + 2).min(world.width as i32 - 1) {
            if let Some(edges) = coasts.get(&TileId(nr as u32 * w + nq as u32)) {
                nearby.extend_from_slice(edges);
            }
        }
    }
    let vertex = |p: [f32; 2]| {
        let distance = water_coast_distance(&nearby, p[0], p[1]);
        let shelf = (1.0 - distance / 2.2).clamp(0.0, 1.0);
        WaterVertex {
            pos: [p[0], y, p[1]],
            uv: [(p[0] + p[1]) * 0.35, 0.5 + shelf * 0.5],
            fog: fog_at_position(world, p[0], p[1]),
            shade,
            shore: -1.0,
            tone: [0.; 3],
        }
    };
    let corners = hex_corners();
    let detailed = !nearby.is_empty()
        || corners
            .iter()
            .any(|p| fog_at_position(world, x + p[0], z + p[1]) < 0.999);
    let center = vertex([x, z]);
    if !detailed {
        for i in 0..6 {
            out.extend([
                center,
                vertex([x + corners[i][0], z + corners[i][1]]),
                vertex([x + corners[(i + 1) % 6][0], z + corners[(i + 1) % 6][1]]),
            ]);
        }
        return;
    }
    // A middle ring and edge midpoints resolve the shore/fog gradient instead
    // of spreading one corner's turquoise color across a giant triangle.
    let mut rim = [[0.0f32; 2]; 12];
    for i in 0..6 {
        rim[2 * i] = corners[i];
        rim[2 * i + 1] = [
            (corners[i][0] + corners[(i + 1) % 6][0]) * 0.5,
            (corners[i][1] + corners[(i + 1) % 6][1]) * 0.5,
        ];
    }
    for i in 0..12 {
        let j = (i + 1) % 12;
        let a = vertex([x + rim[i][0] * 0.55, z + rim[i][1] * 0.55]);
        let b = vertex([x + rim[j][0] * 0.55, z + rim[j][1] * 0.55]);
        let c = vertex([x + rim[i][0], z + rim[i][1]]);
        let d = vertex([x + rim[j][0], z + rim[j][1]]);
        out.extend([center, a, b, a, c, d, a, d, b]);
    }
}

/// Lake surface: a fan over WATER tiles with the same water shader (ripple/glints).
/// Centre v=0.5 (deep); a corner v=1.0 only towards land (foam at the shore),
/// towards a water neighbour v=0.5 (no foam lines inside the lake).
/// y is the water height from the style + lift (carved shores are lower and go under water).
/// Ocean (v35): connected bodies of water of >= OCEAN_BODY tiles get shade=1
/// (deep dark blue in the shader); small lakes get shade=0.
/// Deterministic (tile order, BFS row-major).
pub fn build_lake_surface(
    world: &RenderWorld,
    style: &CompiledTerrainStyle,
    lift: f32,
) -> Vec<WaterVertex> {
    let water_h = style.height(world.water_code()).unwrap_or(0.05);
    // Connected bodies of water (BFS over edges, row-major, deterministic).
    let w = world.width.max(1);
    let hgt = world.height.max(1);
    let is_water_idx = |idx: u32| -> bool { world.is_water(idx as usize) };
    let mut body_of = vec![u32::MAX; world.terrain.len()];
    let mut body_size: Vec<u32> = Vec::new();
    for (idx, cell) in world.terrain.iter().enumerate() {
        if !world.terrain_kinds.is_water(cell.terrain) || body_of[idx] != u32::MAX {
            continue;
        }
        let id = body_size.len() as u32;
        let mut size = 0u32;
        let mut stack = vec![idx as u32];
        body_of[idx] = id;
        while let Some(cur) = stack.pop() {
            size += 1;
            let (q, r) = (cur % w, cur / w);
            for (dq, dr) in EDGE_NEIGHBORS {
                let (nq, nr) = (q as i64 + dq as i64, r as i64 + dr as i64);
                if nq < 0 || nr < 0 || nq >= w as i64 || nr >= hgt as i64 {
                    continue;
                }
                let ni = (nr as u32 * w + nq as u32) as usize;
                if is_water_idx(ni as u32) && body_of[ni] == u32::MAX {
                    body_of[ni] = id;
                    stack.push(ni as u32);
                }
            }
        }
        body_size.push(size);
    }
    const OCEAN_BODY: u32 = 12;
    let coasts = known_water_coasts(world);
    let mut out = Vec::new();
    for cell in &world.terrain {
        if !world.terrain_kinds.is_water(cell.terrain) {
            continue;
        }
        let shade = if body_size
            .get(body_of[cell.tile.0 as usize] as usize)
            .copied()
            .unwrap_or(0)
            >= OCEAN_BODY
        {
            1.0
        } else {
            0.0
        };
        lake_tile_mesh(&mut out, world, cell.tile, water_h + lift, shade, &coasts);
    }
    out
}

/// City tiers by population, with a compact walled footprint across one hex.
/// A colour slightly darker than units of the same owner (to tell them apart).
#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::heightfield::build_heightfield;
    use crate::scene::style::{compile, default_style};
    use crate::world::state::{RenderWorld, TerrainCell};
    use kernel::ids::TileId;

    /// The outsider (the barbarians) is a dark colour no owner of the palette has, so it is
    /// never taken for a nation of the match.
    #[test]
    fn the_outsider_colour_is_dark_and_not_in_the_palette() {
        assert!(OUTSIDER_COLOR.iter().all(|c| *c < 0.2), "dark: {OUTSIDER_COLOR:?}");
        assert!(OWNER_COLORS.iter().all(|c| *c != OUTSIDER_COLOR));
        assert!(OWNER_COLORS.iter().all(|c| c.iter().any(|v| *v > 0.5)), "palette colours are all bright");
    }

    fn flat_world(w: u32, h: u32, terrain: u16) -> RenderWorld {
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

    fn test_style() -> CompiledTerrainStyle {
        compile(default_style()).unwrap()
    }

    #[test]
    fn grid_lines_sit_on_lattice_corners() {
        use crate::scene::terrain::{corner_lattice, lattice_to_world};
        // Regression v32: the old key_x/key_z gave a double scale from the origin
        // (lines off the borders). 2x2: 19 unique edges (5 shared:
        // E x2, SE x2, the SW diagonal) -> 38 vertices, each exactly on
        // the lattice corner of its tile.
        let world = flat_world(2, 2, 0);
        let hf = build_heightfield(&world, &test_style());
        let lines = build_grid_lines(&world, &hf);
        assert_eq!(lines.len(), 38, "19 unique edges as segments");
        let mut expected: Vec<(f32, f32)> = Vec::new();
        for r in 0..2 {
            for q in 0..2 {
                for i in 0..6 {
                    expected.push(lattice_to_world(corner_lattice(q, r, i)));
                }
            }
        }
        for v in &lines {
            assert!(
                expected
                    .iter()
                    .any(|(x, z)| (v[0] - x).abs() < 1e-4 && (v[2] - z).abs() < 1e-4),
                "grid endpoint {:?} is a lattice corner",
                v
            );
            // Height comes from heightfield (flat plains: base + lift).
            let expect_h = test_style().height(0).unwrap() + GRID_LIFT;
            assert!(
                (v[1] - expect_h).abs() < 1e-4,
                "grid height from heightfield, got {}",
                v[1]
            );
        }
    }

    /// Geometry for anchor/chunk tests (deterministic in the inputs).
    fn geom_of(
        world: &RenderWorld,
        style: &CompiledTerrainStyle,
    ) -> crate::scene::terrain::fields::TerrainGeometry {
        let hf = build_heightfield(world, style);
        crate::scene::terrain::fields::build_terrain_geometry(world, style, &hf)
    }

    #[test]
    fn water_boundary_fades_inside_known_area_without_fake_shore() {
        let mut world = flat_world(8, 8, 4);
        world.visible = vec![1; 64];
        world.explored = vec![1; 64];
        let hidden = TileId(3 * 8 + 4);
        world.visible[hidden.0 as usize] = 0;
        world.explored[hidden.0 as usize] = 0;
        world.terrain[hidden.0 as usize].terrain = 1;
        let (x, z) = tile_to_world(3, 3);
        assert!((fog_at_position(&world, x + 1.7320508 * 0.5, z) - 0.18).abs() < 0.001);
        // The fade runs on the known side next to the shared edge, while the
        // visible neighbour's centre stays fully visible (sight is not eaten).
        let near_edge = fog_at_position(&world, x + 1.7320508 * 0.5 - 0.3, z);
        assert!(near_edge > 0.18 && near_edge < 0.9, "fade near the edge: {near_edge}");
        assert_eq!(fog_at_position(&world, x, z), 1.0, "a visible tile's centre is clear");
        assert!(
            known_water_coasts(&world).is_empty(),
            "unknown sanitized geography must not make a shoreline"
        );
        let before = fog_at_position(&world, x, z);
        world.terrain[hidden.0 as usize].terrain = 6;
        assert_eq!(
            before,
            fog_at_position(&world, x, z),
            "fog depends only on knowledge"
        );
        assert!(known_water_coasts(&world).is_empty());
        world.explored[hidden.0 as usize] = 1;
        assert!(
            !known_water_coasts(&world).is_empty(),
            "known land creates real shoreline"
        );
    }

    #[test]
    fn adjacent_water_vertices_share_fog_and_shore_values() {
        let mut world = flat_world(8, 8, 4);
        world.visible = vec![1; 64];
        world.explored = vec![1; 64];
        world.visible[4 * 8 + 4] = 0;
        world.explored[4 * 8 + 4] = 0;
        world.terrain[3 * 8 + 4].terrain = 0;
        let mesh = build_lake_surface(&world, &test_style(), 0.03);
        let mut shared = std::collections::HashMap::<(i32, i32), (f32, f32)>::new();
        for vertex in mesh {
            assert!(vertex.pos.iter().all(|v| v.is_finite()));
            let key = (
                (vertex.pos[0] * 10000.0).round() as i32,
                (vertex.pos[2] * 10000.0).round() as i32,
            );
            if let Some((fog, shore)) = shared.insert(key, (vertex.fog, vertex.uv[1])) {
                assert!((fog - vertex.fog).abs() < 0.0001, "shared water fog seam");
                assert!((shore - vertex.uv[1]).abs() < 0.0001, "shared shore seam");
            }
        }
    }

    #[test]
    fn pack_blend_sums_to_255() {
        let style = test_style();
        // One terrain: a pure layer.
        let (idx, w) = pack_blend(&[(2, 3), (0, 0), (0, 0), (0, 0)], &style);
        assert_eq!(idx & 0xFF, style.layer(2).unwrap());
        assert_eq!(w & 0xFF, 255);
        // Two terrains 1:1 -> ~127/128, the sum exactly 255.
        let (idx, w) = pack_blend(&[(0, 1), (1, 1), (0, 0), (0, 0)], &style);
        let w0 = w & 0xFF;
        let w1 = (w >> 8) & 0xFF;
        assert_eq!(w0 + w1, 255);
        assert_eq!(idx & 0xFF, style.layer(0).unwrap());
        assert_eq!((idx >> 8) & 0xFF, style.layer(1).unwrap());
        // Empty: degenerate, but valid.
        let (_, w) = pack_blend(&[(0, 0), (0, 0), (0, 0), (0, 0)], &style);
        assert_eq!(w & 0xFF, 255);
    }

    #[test]
    fn flat_map_topology_counts() {
        // 19-pt patch: tops 64x24 triangles + 62 boundary skirts.
        // Vertices are shared: fewer than in the duplicated version.
        let world = flat_world(8, 8, 0);
        let style = test_style();
        let hf = build_heightfield(&world, &style);
        let geom = crate::scene::terrain::fields::build_terrain_geometry(&world, &style, &hf);
        let chunk = build_chunk(&world, &style, &geom, 0, 0);
        assert_eq!(chunk.tiles, 64);
        assert_eq!(chunk.indices.len(), 64 * 72 + 62 * 6);
        assert!(
            chunk.vertices.len() < 64 * 19 + 62 * 4,
            "sharing saves verts"
        );
        let nv = chunk.vertices.len();
        assert!(chunk.indices.iter().all(|i| (*i as usize) < nv));
        // UVs cover the map: range ~ size/tiling.
        let max_u = chunk.vertices.iter().map(|v| v.uv[0]).fold(0f32, f32::max);
        assert!(max_u > 1.0, "uv tiles across map");
    }

    #[test]
    fn shared_corners_exist_once_and_edges_close() {
        use std::collections::{HashMap, HashSet};
        // Edge position: a pair of corner points. The type is extracted for readability.
        type EdgeKey = ((u32, u32), (u32, u32));
        let world = flat_world(4, 4, 1);
        let style = test_style();
        let hf = build_heightfield(&world, &style);
        let geom = crate::scene::terrain::fields::build_terrain_geometry(&world, &style, &hf);
        let chunk = build_chunk(&world, &style, &geom, 0, 0);
        let h = style.height(1).unwrap();
        // Positions of top vertices without duplicates (sharing works).
        let mut seen = HashSet::new();
        let mut dups = 0;
        for v in &chunk.vertices {
            if (v.pos[1] - h).abs() > 1e-6 || v.pos[1] == 0.0 {
                continue;
            }
            if !seen.insert((v.pos[0].to_bits(), v.pos[2].to_bits())) {
                dups += 1;
            }
        }
        assert_eq!(dups, 0, "shared corners must not duplicate");
        // Closedness of the patch tops: each edge of the top triangles
        // occurs x1 (map boundary) or x2 (inside).
        let pos = |i: u32| {
            let v = &chunk.vertices[i as usize];
            ((v.pos[0].to_bits(), v.pos[2].to_bits()), v.pos[1])
        };
        let mut counts: HashMap<EdgeKey, usize> = HashMap::new();
        let mut top_tris = 0;
        for tri in chunk.indices.chunks(3) {
            let ps: Vec<_> = tri.iter().map(|i| pos(*i)).collect();
            if !ps.iter().all(|(_, y)| (y - h).abs() < 1e-6) {
                continue;
            }
            top_tris += 1;
            let pts: Vec<_> = ps.iter().map(|(p, _)| *p).collect();
            for (a, b) in [(pts[0], pts[1]), (pts[1], pts[2]), (pts[2], pts[0])] {
                let key = if a < b { (a, b) } else { (b, a) };
                *counts.entry(key).or_insert(0) += 1;
            }
        }
        assert_eq!(top_tris, 16 * 24, "19-pt patch: 24 tris per tile");
        let ones = counts.values().filter(|c| **c == 1).count();
        let twos = counts.values().filter(|c| **c == 2).count();
        assert!(counts.values().all(|c| *c == 1 || *c == 2));
        assert_eq!(ones + 2 * twos, 3 * top_tris, "edge conservation");
    }

    #[test]
    fn height_steps_dip_edge_mids() {
        // Half hills: a step plains(0.35)/hills(0.55) = 0.20,
        // the midpoints of boundary edges are sunk by step/4 = 0.05
        // (S-profile; no wall aprons, they gave "fences").
        // No walls: the same number of indices as a flat one. Boundary corners: a blend of two
        // terrains with weights summing to 255.
        let mut world = flat_world(8, 4, 0);
        for c in world.terrain.iter_mut().filter(|c| c.tile.0 % 8 >= 4) {
            c.terrain = 2;
        }
        let style = test_style();
        let hf = build_heightfield(&world, &style);
        let geom = crate::scene::terrain::fields::build_terrain_geometry(&world, &style, &hf);
        let chunk = build_chunk(&world, &style, &geom, 0, 0);
        let flat = flat_world(8, 4, 0);
        let hf_flat = build_heightfield(&flat, &style);
        let geom_flat =
            crate::scene::terrain::fields::build_terrain_geometry(&flat, &style, &hf_flat);
        let chunk_flat = build_chunk(&flat, &style, &geom_flat, 0, 0);
        assert_eq!(
            chunk.indices.len(),
            chunk_flat.indices.len(),
            "no extra walls on steps"
        );
        // Edge E of tile (3,0) is the plains/hills boundary: the midpoint is sunk.
        let patch = &geom.patches.tiles[3];
        let em = &geom.boundary.edges[patch.edges[0] as usize];
        let ca = &geom.boundary.corners[patch.corners[0] as usize];
        let cb = &geom.boundary.corners[patch.corners[1] as usize];
        assert!(
            ((ca.y + cb.y) * 0.5 - em.mid_y - 0.05).abs() < 1e-4,
            "edge-mid dipped by step/4: mid {} vs avg {}",
            em.mid_y,
            (ca.y + cb.y) * 0.5
        );
        // A blend vertex exists: two layers, weights sum to 255.
        let blended = chunk.vertices.iter().find(|v| {
            let w0 = v.blend_w & 0xFF;
            let w1 = (v.blend_w >> 8) & 0xFF;
            w0 > 0 && w0 < 255 && w1 > 0 && w0 + w1 == 255
        });
        assert!(blended.is_some(), "border verts must blend two layers");
        // Normals are unit.
        for v in &chunk.vertices {
            let n = (v.normal[0].powi(2) + v.normal[1].powi(2) + v.normal[2].powi(2)).sqrt();
            assert!((n - 1.0).abs() < 1e-4, "normal len {}", n);
        }
    }

    #[test]
    fn build_is_deterministic() {
        let world = flat_world(10, 7, 3);
        let style = test_style();
        let hf = build_heightfield(&world, &style);
        let geom = crate::scene::terrain::fields::build_terrain_geometry(&world, &style, &hf);
        let (a, b) = (
            build_chunk(&world, &style, &geom, 0, 0),
            build_chunk(&world, &style, &geom, 0, 0),
        );
        assert_eq!(a.vertices.len(), b.vertices.len());
        assert_eq!(a.indices, b.indices);
        for (x, y) in a.vertices.iter().zip(b.vertices.iter()) {
            assert_eq!(x.pos, y.pos);
            assert_eq!(x.uv, y.uv);
            assert_eq!(x.blend_idx, y.blend_idx);
            assert_eq!(x.blend_w, y.blend_w);
            assert_eq!(x.normal, y.normal);
            assert_eq!(x.fog, y.fog);
        }
    }

    #[test]
    fn vertex_layouts_match_gpu_contract() {
        assert_eq!(std::mem::size_of::<HexInstance>(), 96);
        assert_eq!(std::mem::offset_of!(HexInstance, anim), 48);
        assert_eq!(std::mem::offset_of!(HexInstance, rig), 64);
        assert_eq!(std::mem::offset_of!(HexInstance, motion), 80);
        assert_eq!(std::mem::offset_of!(HexInstance, yaw), 36);
        assert_eq!(std::mem::offset_of!(HexInstance, slope), 40);
        assert_eq!(std::mem::size_of::<TerrainVertex>(), 64);
        assert_eq!(std::mem::offset_of!(TerrainVertex, albedo), 48);
        assert_eq!(std::mem::size_of::<BorderVertex>(), 24);
        // v35: +shade (ocean depth), a deliberate format change.
        assert_eq!(std::mem::size_of::<WaterVertex>(), 44);
        assert_eq!(std::mem::offset_of!(WaterVertex, shore), 28);
        // v36: a model vertex = pos + normal + color + team.
        assert_eq!(std::mem::size_of::<UnitVertex>(), 40);
    }

    #[test]
    fn borders_drawn_once_without_doubles() {
        // 4x4 split in half: a vertical boundary of 4 edges; the map edge gives
        // the perimeter. Each boundary edge is exactly 2 vertices (no duplicates).
        let mut world = flat_world(4, 4, 0);
        for c in world.terrain.iter_mut().filter(|c| c.tile.0 % 4 >= 2) {
            c.owner = Some(1);
        }
        for c in world.terrain.iter_mut().filter(|c| c.tile.0 % 4 < 2) {
            c.owner = Some(0);
        }
        let style = test_style();
        let hf = build_heightfield(&world, &style);
        let lines = build_border_lines(&world, &hf);
        assert_eq!(lines.len() % 2, 0);
        // Inner boundary of 4 edges + the perimeter of the owned zone.
        // More precisely: the uniqueness of every vertex pair.
        // Quantisation 1e-4: the same corner from the centres of different tiles
        // gives different float noise; a bitwise comparison would miss the duplicates.
        use std::collections::HashSet;
        let quant = |v: f32| (v * 1e4).round() as i32;
        let mut segs = HashSet::new();
        for pair in lines.chunks(2) {
            let a = (quant(pair[0].pos[0]), quant(pair[0].pos[2]));
            let b = (quant(pair[1].pos[0]), quant(pair[1].pos[2]));
            let key = if a < b { (a, b) } else { (b, a) };
            assert!(segs.insert(key), "duplicate border segment");
        }
        // Colours are owners only (0/1), no water/emptiness in the lines.
        assert!(lines
            .iter()
            .all(|v| v.color == owner_color(0) || v.color == owner_color(1)));
        // Determinism.
        let lines2 = build_border_lines(&world, &hf);
        assert_eq!(lines.len(), lines2.len());
        for (a, b) in lines.iter().zip(lines2.iter()) {
            assert_eq!(a.pos, b.pos);
        }
    }

    #[test]
    fn city_highlight_covers_ring_without_doubles() {
        // Selected city overlay: outlines of the 7 ring tiles (centre +
        // 6 neighbours), inner edges once, all white, lift above the
        // borders. 7 hexes = 42 edges minus 6 shared with the centre minus 6
        // between the ring neighbours = 30 unique segments.
        let world = flat_world(8, 8, 0);
        let style = test_style();
        let hf = build_heightfield(&world, &style);
        let center = TileId(4 * 8 + 4);
        let lines = build_city_highlight(&world, &hf, center, 1, &[]);
        assert_eq!(lines.len(), 30 * 2, "30 unique segments");
        assert!(lines.iter().all(|v| v.color == SELECTION_COLOR));
        assert!(lines.iter().all(|v| v.pos[1] > BORDER_LIFT + 0.2));
        // No duplicates (quantised pairs are unique).
        use std::collections::HashSet;
        let quant = |x: f32| (x * 1e4).round() as i32;
        let mut segs = HashSet::new();
        for pair in lines.chunks(2) {
            let a = (quant(pair[0].pos[0]), quant(pair[0].pos[2]));
            let b = (quant(pair[1].pos[0]), quant(pair[1].pos[2]));
            let key = if a < b { (a, b) } else { (b, a) };
            assert!(segs.insert(key), "duplicate highlight segment");
        }
        // Byte-wise determinism.
        let lines2 = build_city_highlight(&world, &hf, center, 1, &[]);
        assert_eq!(lines.len(), lines2.len());
        for (a, b) in lines.iter().zip(lines2.iter()) {
            assert_eq!((a.pos, a.color), (b.pos, b.color));
        }
        // A tile taken off: its 6 edges are dim, foreign ones are white, no duplicates.
        let dim_tile = TileId(4 * 8 + 5);
        let dimmed = build_city_highlight(&world, &hf, center, 1, &[dim_tile]);
        assert_eq!(dimmed.len(), 30 * 2);
        let dim_count = dimmed.iter().filter(|v| v.color == UNWORKED_COLOR).count();
        let lit_count = dimmed.iter().filter(|v| v.color == SELECTION_COLOR).count();
        assert_eq!(dim_count, 6 * 2, "6 dimmed segments");
        assert_eq!(lit_count + dim_count, dimmed.len());
    }

    #[test]
    fn ribbon_has_width_and_keeps_colors() {
        // Ribbon: a segment -> 6 vertices (2 triangles) + joint discs
        // (18 vertices per unique end); width across, colours preserved,
        // degenerate segments skipped.
        let mut world = flat_world(8, 8, 0);
        for c in world.terrain.iter_mut().filter(|c| c.tile.0 % 8 < 4) {
            c.owner = Some(0);
        }
        let style = test_style();
        let hf = build_heightfield(&world, &style);
        let lines = build_border_lines(&world, &hf);
        assert!(!lines.is_empty());
        let width = 0.14;
        let tris = build_ribbon_tris(&lines, width, &hf, BORDER_LIFT);
        let segs = lines.len() / 2;
        assert!(tris.len() >= segs * 6, "at least strip tris");
        assert_eq!((tris.len() - segs * 6) % 18, 0, "discs are 18 verts each");
        assert!(
            !tris.is_empty() && tris.len() > segs * 6,
            "joints add discs"
        );
        // The cross width of the first ribbon ~ width (flat map: exact).
        let (p0, p1) = (tris[0].pos, tris[1].pos);
        let dx = p0[0] - p1[0];
        let dz = p0[2] - p1[2];
        let got = (dx * dx + dz * dz).sqrt();
        assert!((got - width).abs() < 1e-3, "width={}", got);
        // Colours: a vertex pair shares the colour of the source end.
        assert_eq!(tris[0].color, lines[0].color);
        // A degenerate segment (zero length): skipped without a panic.
        let degen = vec![
            BorderVertex {
                pos: [1.0, 0.5, 2.0],
                color: [1.0, 0.0, 0.0],
            },
            BorderVertex {
                pos: [1.0, 0.5, 2.0],
                color: [1.0, 0.0, 0.0],
            },
        ];
        assert!(build_ribbon_tris(&degen, width, &hf, BORDER_LIFT).is_empty());
        // An odd tail is ignored.
        let odd = vec![BorderVertex {
            pos: [0.0, 0.0, 0.0],
            color: [0.0; 3],
        }];
        assert!(build_ribbon_tris(&odd, width, &hf, BORDER_LIFT).is_empty());
    }

    #[test]
    fn yield_atlas_cells_opaque_and_distinct() {
        // Atlas 192x32, row-major (6 cells horizontally, as the GPU reads it):
        // apple, gear, ear, ingot, nugget, highway.
        // We check recognisable pixels of each cell.
        let (w, h, px) = build_yield_atlas();
        assert_eq!((w, h), (192, 32));
        let rgba = |cell: usize, x: usize, y: usize| {
            let i = (y * 192 + cell * 32 + x) * 4;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(rgba(0, 16, 19), [232, 64, 56, 255], "apple body red");
        assert_eq!(rgba(0, 15, 8), [112, 72, 32, 255], "apple stem brown");
        assert_eq!(rgba(0, 21, 10), [56, 154, 62, 255], "apple leaf green");
        assert_eq!(rgba(0, 11, 15), [255, 178, 158, 255], "apple glint");
        assert_eq!(rgba(0, 16, 29), [118, 24, 20, 255], "apple outline");
        assert_eq!(rgba(0, 0, 0)[3], 0, "corner transparent");
        assert_eq!(rgba(1, 16, 16)[3], 0, "gear hub hole transparent");
        assert_eq!(rgba(1, 16, 25), [246, 188, 58, 255], "gear fill");
        assert_eq!(rgba(1, 16, 26), [132, 82, 16, 255], "gear outline");
        assert_eq!(rgba(1, 28, 16), [246, 188, 58, 255], "gear tooth");
        assert_eq!(rgba(2, 15, 25), [176, 138, 40, 255], "wheat stalk");
        assert_eq!(rgba(2, 18, 13), [234, 196, 74, 255], "wheat kernel");
        assert_eq!(rgba(2, 18, 8), [250, 218, 116, 255], "wheat kernel top");
        assert_eq!(rgba(2, 13, 9), [140, 100, 26, 255], "wheat seam");
        assert_eq!(rgba(3, 16, 9), [214, 220, 236, 255], "ingot top face");
        assert_eq!(rgba(3, 12, 9), [246, 248, 253, 255], "ingot shine");
        assert_eq!(rgba(3, 16, 12), [110, 118, 142, 255], "ingot edge");
        assert_eq!(rgba(3, 16, 17), [176, 184, 204, 255], "ingot body");
        assert_eq!(rgba(3, 16, 21), [148, 156, 178, 255], "ingot base");
        assert_eq!(rgba(3, 16, 22), [84, 90, 108, 255], "ingot outline");
        assert_eq!(rgba(4, 19, 14), [238, 188, 52, 255], "nugget body");
        assert_eq!(rgba(4, 10, 16), [255, 230, 134, 255], "nugget shine");
        assert_eq!(rgba(4, 12, 27), [146, 104, 18, 255], "nugget outline");
        assert_eq!(rgba(5, 16, 16), [164, 120, 62, 255], "road track");
        assert_eq!(rgba(5, 4, 16), [234, 214, 170, 255], "road dash");
        assert_eq!(rgba(5, 16, 11), [118, 80, 36, 255], "road edge");
        assert_eq!(rgba(5, 16, 10), [96, 64, 30, 255], "road outline");
        let cell_rgb = |cell: usize| -> Vec<u8> {
            let mut v = Vec::with_capacity(32 * 32 * 3);
            for y in 0..32 {
                for x in 0..32 {
                    let i = (y * 192 + cell * 32 + x) * 4;
                    v.extend_from_slice(&px[i..i + 3]);
                }
            }
            v
        };
        assert_ne!(cell_rgb(0), cell_rgb(1), "food and prod icons differ");
    }

    #[test]
    fn yield_instances_exact_rows_per_terrain() {
        // Grass (2 food, 0 production): exactly 2 food instances per tile. Hills (0F2P):
        // 2 gears. Snow (0F0P): nothing. The test catches any doubling hard.
        let mut world = flat_world(4, 4, 1);
        world.explored = vec![1u8; 16];
        let style = test_style();
        let inst = build_yield_instances(&world, &style, &geom_of(&world, &style));
        assert_eq!(inst.len(), 32);
        assert!(inst.iter().all(|v| v.cell_scale[0] == 0.0));
        // Rows on plains (1F1P): food higher (dz<0), production lower (dz>0).
        let mut plains = flat_world(4, 4, 0);
        plains.explored = vec![1u8; 16];
        let pinst = build_yield_instances(&plains, &style, &geom_of(&plains, &style));
        let dz0 = pinst[0].offset[2] - pinst[1].offset[2];
        assert!(dz0 < -0.1, "food row above prod row, dz={}", dz0);
        let mut hills = flat_world(2, 2, 2);
        hills.explored = vec![1u8; 4];
        let hinst = build_yield_instances(&hills, &style, &geom_of(&hills, &style));
        assert_eq!(hinst.len(), 8, "hills: 2 gears per tile");
        assert!(hinst.iter().all(|v| v.cell_scale[0] == 1.0));
        let mut snow = flat_world(2, 2, 5);
        snow.explored = vec![1u8; 4];
        let sinst = build_yield_instances(&snow, &style, &geom_of(&snow, &style));
        assert!(sinst.is_empty(), "snow yields nothing");
    }

    #[test]
    fn yield_instances_cover_explored_land_only() {
        // Plains (1F1P) x explored 4x4: 2 icons per tile = 32 instances;
        // unexplored and water: zero. Order: food first, deterministic.
        let mut world = flat_world(4, 4, 0);
        world.explored = vec![1u8; 16];
        let style = test_style();
        let inst = build_yield_instances(&world, &style, &geom_of(&world, &style));
        assert_eq!(inst.len(), 32);
        assert!(inst.iter().step_by(2).all(|v| v.cell_scale[0] == 0.0));
        assert!(inst
            .iter()
            .skip(1)
            .step_by(2)
            .all(|v| v.cell_scale[0] == 1.0));
        world.explored = vec![0u8; 16];
        assert!(build_yield_instances(&world, &style, &geom_of(&world, &style)).is_empty());
        // Water gives no icons (f=p=0).
        let mut sea = flat_world(2, 2, 4);
        sea.explored = vec![1u8; 4];
        assert!(build_yield_instances(&sea, &style, &geom_of(&sea, &style)).is_empty());
        // Determinism.
        world.explored = vec![1u8; 16];
        let inst2 = build_yield_instances(&world, &style, &geom_of(&world, &style));
        assert_eq!(inst.len(), inst2.len());
        for (a, b) in inst.iter().zip(inst2.iter()) {
            assert_eq!((a.offset, a.cell_scale), (b.offset, b.cell_scale));
        }
    }

    #[test]
    fn road_icons_centered_and_explored_only() {
        // Road: +1 highway icon (cell 5) at the tile centre;
        // without explored: zero.
        let mut world = flat_world(2, 2, 0);
        world.explored = vec![1u8; 4];
        world.road = vec![0u8; 4];
        world.road[0] = 1;
        let style = test_style();
        let inst = build_yield_instances(&world, &style, &geom_of(&world, &style));
        // 4 plains x 2 yield icons + 1 highway = 9.
        assert_eq!(inst.len(), 9);
        let road: Vec<_> = inst.iter().filter(|v| v.cell_scale[0] == 5.0).collect();
        assert_eq!(road.len(), 1);
        world.explored = vec![0u8; 4];
        assert!(
            build_yield_instances(&world, &style, &geom_of(&world, &style))
                .iter()
                .all(|v| v.cell_scale[0] != 5.0)
        );
    }

    #[test]
    fn river_paths_stitch_chained_edges() {
        // Stitching by shared corners: two edges with a shared corner are found by search
        // (ids cannot be picked by hand: neighbouring tiles give parallel edges).
        use crate::scene::heightfield::corner_key;
        // An edge + its corner points (readability of a complex signature).
        type RibPts = ((u32, u32), [f32; 2], [f32; 2]);
        let mut world = flat_world(8, 8, 0);
        world.explored = vec![1u8; 64];
        let mut ribs: Vec<RibPts> = Vec::new();
        for t in [27u32, 28, 35, 36] {
            for (dx, dy) in [(1, 0), (0, 1), (1, 1)] {
                let nb = t + dx + dy * 8;
                world.river_edges = vec![(t, nb)];
                let lines = build_river_lines(&world);
                if lines.len() == 2 {
                    ribs.push((
                        (t, nb),
                        [lines[0].pos[0], lines[0].pos[2]],
                        [lines[1].pos[0], lines[1].pos[2]],
                    ));
                }
            }
        }
        let mut chained = None;
        for (i, (e1, a1, b1)) in ribs.iter().enumerate() {
            for (e2, a2, b2) in ribs.iter().skip(i + 1) {
                let keys1 = [corner_key(a1[0], a1[1]), corner_key(b1[0], b1[1])];
                let keys2 = [corner_key(a2[0], a2[1]), corner_key(b2[0], b2[1])];
                if keys1.iter().any(|k| keys2.contains(k)) {
                    chained = Some((*e1, *e2));
                    break;
                }
            }
            if chained.is_some() {
                break;
            }
        }
        let (e1, e2) = chained.expect("nearby ribs must share a corner");
        world.river_edges = vec![e1, e2];
        let paths = build_river_paths(&world);
        assert_eq!(paths.len(), 1, "chained ribs form one path: {e1:?} {e2:?}");
        assert_eq!(paths[0].len(), 3, "2 ribs share the joint");
        // Far edges are separate paths of 2 points; empty is empty.
        world.river_edges = vec![(0, 1), (62, 63)];
        let far = build_river_paths(&world);
        assert!(!far.is_empty());
        assert!(far.iter().all(|p| p.len() == 2));
        world.river_edges = vec![];
        assert!(build_river_paths(&world).is_empty());
    }

    #[test]
    fn water_strip_counts_and_seamless_uv() {
        // A strip straight from paths: 6 vertices per segment, no caps
        // (the ends taper by a wedge; forks do not occur within one path).
        // v in {0, 0.5, 1}, u grows along the path (ripple without seams).
        let mk = |x: f32, z: f32| PathPt {
            pos: [x, z],
            fog: 1.0,
        };
        let paths = vec![vec![mk(0.0, 0.0), mk(1.0, 0.0), mk(1.5, 0.866)]];
        let hf = Heightfield::default();
        let surf = build_water_strip(&paths, RIVER_WIDTH, &hf, RIVER_LIFT);
        assert_eq!(surf.len(), 2 * 6, "strip only, no caps");
        assert!(surf.iter().all(|v| [0.0, 0.5, 1.0].contains(&v.uv[1])));
        // The width is uneven: the half-width of segments differs.
        let w0 = (surf[0].pos[0] - surf[1].pos[0]).hypot(surf[0].pos[2] - surf[1].pos[2]);
        let w1 = (surf[6].pos[0] - surf[7].pos[0]).hypot(surf[6].pos[2] - surf[7].pos[2]);
        assert!((w0 - w1).abs() > 1e-4, "variable width: {w0} vs {w1}");
        let strip_us: Vec<f32> = surf[..12].iter().step_by(6).map(|v| v.uv[0]).collect();
        assert_eq!(strip_us.len(), 2, "first vert of each segment");
        assert!(
            strip_us.windows(2).all(|w| w[1] >= w[0] - 1e-6),
            "u monotonic: {strip_us:?}"
        );
        // Empty and a single one: empty, not a panic.
        let mk1 = |x: f32, z: f32| PathPt {
            pos: [x, z],
            fog: 1.0,
        };
        assert!(build_water_strip(&[], RIVER_WIDTH, &hf, RIVER_LIFT).is_empty());
        assert!(build_water_strip(&[vec![mk1(0.0, 0.0)]], RIVER_WIDTH, &hf, RIVER_LIFT).is_empty());
    }
    #[test]
    fn river_lines_chain_without_doubles() {
        // Beds by edges: 2 adjacent edges (35-36, 36-37) give 2 segments,
        // without duplicates, water colour, y=0 (heights are set by the ribbon).
        let mut world = flat_world(8, 8, 0);
        world.explored = vec![1u8; 64];
        world.river_edges = vec![(35, 36), (36, 37)];
        let lines = build_river_lines(&world);
        assert_eq!(lines.len(), 2 * 2, "2 chained segments");
        assert!(lines.iter().all(|v| v.color == RIVER_COLOR));
        assert!(lines.iter().all(|v| v.pos[1] == 0.0));
        // A non-adjacent pair is skipped silently (broken data do not crash the render).
        world.river_edges = vec![(0, 63)];
        assert!(build_river_lines(&world).is_empty());
        // Determinism.
        world.river_edges = vec![(35, 36), (36, 37)];
        let lines2 = build_river_lines(&world);
        assert_eq!(lines.len(), lines2.len());
        for (a, b) in lines.iter().zip(lines2.iter()) {
            assert_eq!((a.pos, a.color), (b.pos, b.color));
        }
    }

    #[test]
    fn baked_model_meshes_counts_and_bounds() {
        // Models come from build.rs (.glb bake). Cities and units: world_models / unit_models.
        // Pins (forest): the same bake invariants, a budget of 300 tris, team=0
        // (features do not read the team mask, but the bake validates A in {0,1}).
        use crate::models::PINE_MESHES;
        assert_eq!(PINE_MESHES.len(), 3);
        for m in PINE_MESHES {
            assert!(!m.is_empty(), "pine mesh is baked");
            assert!(m.len() % 3 == 0, "triangle soup");
            assert!(m.len() <= 300 * 3, "pine tri budget");
            for v in m {
                assert!(
                    v.pos[0].abs() <= 0.5 && v.pos[2].abs() <= 0.5,
                    "pine footprint {:?}",
                    v.pos
                );
                assert!((-0.1..=1.5).contains(&v.pos[1]), "pine height {:?}", v.pos);
                assert!(v.color.iter().all(|c| (0.0..=1.0).contains(c)));
                assert_eq!(v.team, 0.0, "pines are neutral");
                let n = (v.normal[0].powi(2) + v.normal[1].powi(2) + v.normal[2].powi(2)).sqrt();
                assert!((n - 1.0).abs() < 1e-4, "pine normal {:?}", v.normal);
            }
        }
    }
}
