//! Territory borders on the continuous surface, separate from selection.
//!
//! Each owner gets a band on its own side of the border: a crisp line on the
//! hex edge and a soft wash fading inward (platform/assets/shaders/soft_border.wgsl).
//! A line centered on the edge in the owner's color read as a river. The band
//! is draped on the final mesh with samples dense enough that it follows a
//! cliff wall instead of cutting through it or floating in front of it.
//! Fading out of the visible area is done by the shader from the fog field,
//! so a visibility change never rebuilds this mesh.
//!
//! Every border edge is built on its own (`edge_band`): from the middle of the
//! rounded corner at its start to the middle of the one at its end, the corners
//! rounded with the owner's neighbouring edges there. An edge's band depends only on
//! the owners of its cell's neighbours and on the ground under it, so the bands are
//! kept in chunks of one surface region of cells (`BorderChunks`) and an ownership
//! change rebuilds only the chunks of the cells around it.
use super::buffers::{owner_color, BorderVertex, EDGE_NEIGHBORS};
use super::context::GpuCore;
use super::budget::Budget;
use super::terrain_job::Changed;
use crate::camera::CameraController;
use crate::scene::terrain::surface::{SurfaceMesh, REGION};
use crate::scene::terrain::{corner_lattice, lattice_to_world};
use crate::world::state::RenderWorld;
use glam::Vec2;
use std::collections::BTreeMap;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub color: [f32; 3],
    /// 0 on the border line, 1 at the inner end of the band.
    pub across: f32,
    pub opacity: f32,
}

/// Width of the band inside the territory, world units.
const BAND: f32 = 0.17;
/// Height above the displayed surface.
const LIFT: f32 = 0.02;
/// Along the border, samples are at most this far apart...
const STEP: f32 = 0.1;
/// ...and are halved until neighbouring heights differ by at most this.
const MAX_RISE: f32 = 0.03;
/// How far a corner's rounding reaches along each edge, at most.
const ROUNDING: f32 = 0.18;

pub fn legacy(tris: &[BorderVertex]) -> Vec<Vertex> {
    tris.iter()
        .map(|v| Vertex {
            pos: v.pos,
            color: v.color,
            across: 0.,
            opacity: 1.,
        })
        .collect()
}

/// A border edge: corner `a` to corner `b` of the owner's cell, counter-clockwise
/// around it, so the territory is on its left.
#[derive(Debug, Clone, Copy)]
struct Edge {
    owner: u8,
    a: Vec2,
    b: Vec2,
}

type Key = (i32, i32);
fn key(p: Vec2) -> Key {
    ((p.x * 10000.).round() as i32, (p.y * 10000.).round() as i32)
}

fn owner_at(world: &RenderWorld, q: i32, r: i32) -> Option<u8> {
    if q < 0 || r < 0 || q >= world.width as i32 || r >= world.height as i32 {
        return None;
    }
    world.terrain.get((r as u32 * world.width + q as u32) as usize).and_then(|c| c.owner)
}

/// The border edges a cell draws: towards a neighbour of another owner with a
/// smaller id, an unowned neighbour or the map edge. Exactly one owner draws each
/// border, as `buffers::build_border_lines`.
fn cell_edges(world: &RenderWorld, q: i32, r: i32) -> Vec<Edge> {
    let Some(mine) = owner_at(world, q, r) else { return Vec::new() };
    // Corners from the integer lattice: the cells around a corner get the same bits.
    let corner = |i: usize| {
        let (x, z) = lattice_to_world(corner_lattice(q, r, i));
        Vec2::new(x, z)
    };
    let mut out = Vec::new();
    for (i, (dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
        let emit = match owner_at(world, q + dq, r + dr) {
            None => true,
            Some(other) => other != mine && other < mine,
        };
        if emit {
            out.push(Edge { owner: mine, a: corner(i), b: corner((i + 1) % 6) });
        }
    }
    out
}

/// The owner's edges around cell (q, r): its own and its neighbours'. Every edge that
/// shares a corner with an edge of the cell is among them.
fn edges_around(world: &RenderWorld, q: i32, r: i32) -> Vec<Edge> {
    let mut out = cell_edges(world, q, r);
    for (dq, dr) in EDGE_NEIGHBORS {
        out.extend(cell_edges(world, q + dq, r + dr));
    }
    out
}

/// The corner where `edge` meets the owner's other edge at `p` (`from` true: an edge
/// that ends at `p`, else one that starts there). Of several, the one with the
/// smallest far end, so every edge sees the same neighbour.
fn neighbour(edges: &[Edge], edge: &Edge, p: Vec2, from: bool) -> Option<Vec2> {
    let at = key(p);
    edges
        .iter()
        .filter(|e| e.owner == edge.owner)
        .filter_map(|e| if from { (key(e.b) == at).then_some(e.a) } else { (key(e.a) == at).then_some(e.b) })
        .filter(|far| key(*far) != key(if from { edge.b } else { edge.a }))
        .min_by_key(|far| key(*far))
}

/// The rounded corner at `p` between incoming direction from `before` and outgoing
/// direction to `after`: a quadratic from the incoming to the outgoing tangent point.
struct Corner {
    start: Vec2,
    p: Vec2,
    end: Vec2,
}

impl Corner {
    fn new(before: Vec2, p: Vec2, after: Vec2) -> Self {
        let reach = ROUNDING.min(p.distance(before) * 0.25).min(p.distance(after) * 0.25);
        Self { start: p + (before - p).normalize() * reach, p, end: p + (after - p).normalize() * reach }
    }
    fn at(&self, t: f32) -> Vec2 {
        self.start * (1. - t).powi(2) + self.p * (2. * t * (1. - t)) + self.end * t * t
    }
    /// Direction of the curve at its middle: the same for both edges that meet here.
    fn middle_tangent(&self) -> Vec2 {
        (self.end - self.start).normalize_or_zero()
    }
}

/// The band of one edge, from the middle of its start corner to the middle of its end
/// corner (or the corner itself where the owner's border ends).
fn edge_band(out: &mut Vec<Vertex>, edges: &[Edge], edge: &Edge, surface: &SurfaceMesh) {
    let direction = (edge.b - edge.a).normalize_or_zero();
    let mut path = Vec::with_capacity(8);
    let start_tangent = match neighbour(edges, edge, edge.a, true) {
        Some(before) => {
            let corner = Corner::new(before, edge.a, edge.b);
            path.extend([corner.at(0.5), corner.at(0.75), corner.end]);
            corner.middle_tangent()
        }
        None => {
            path.push(edge.a);
            direction
        }
    };
    let end_tangent = match neighbour(edges, edge, edge.b, false) {
        Some(after) => {
            let corner = Corner::new(edge.a, edge.b, after);
            path.extend([corner.start, corner.at(0.25), corner.at(0.5)]);
            corner.middle_tangent()
        }
        None => {
            path.push(edge.b);
            direction
        }
    };
    let color = owner_color(edge.owner).map(|v| v * 0.76 + 0.15);
    emit(out, &path, start_tangent, end_tangent, color, surface);
}

fn display_height(surface: &SurfaceMesh, p: Vec2) -> f32 {
    surface.sample_display(p.x, p.y).map_or(0.3, |s| s.y)
}

/// A point on the path and the displayed height there.
#[derive(Clone, Copy)]
struct Draped {
    p: Vec2,
    y: f32,
}

impl Draped {
    fn new(surface: &SurfaceMesh, p: Vec2) -> Self {
        Self { p, y: display_height(surface, p) }
    }
}

/// Points along the path: at most `STEP` apart, and halved where the surface
/// rises fast, so the band follows a wall instead of spanning it.
fn drape_points(surface: &SurfaceMesh, path: &[Vec2]) -> Vec<Draped> {
    let mut points = vec![Draped::new(surface, path[0])];
    for pair in path.windows(2) {
        let steps = (pair[0].distance(pair[1]) / STEP).ceil().max(1.) as usize;
        // The ends exactly: neighbouring edges share their joint bit for bit.
        let at = |s: usize| if s == steps { pair[1] } else { pair[0].lerp(pair[1], s as f32 / steps as f32) };
        for s in 1..=steps {
            let a = *points.last().expect("starts with the first point");
            refine(surface, a, Draped::new(surface, at(s)), 4, &mut points);
        }
    }
    points
}

/// Pushes the points after `a` up to `b`, halving the step while the rise is steep.
fn refine(surface: &SurfaceMesh, a: Draped, b: Draped, depth: u32, points: &mut Vec<Draped>) {
    if depth > 0 && (a.y - b.y).abs() > MAX_RISE {
        let mid = Draped::new(surface, a.p.lerp(b.p, 0.5));
        refine(surface, a, mid, depth - 1, points);
        refine(surface, mid, b, depth - 1, points);
        return;
    }
    points.push(b);
}

fn emit(out: &mut Vec<Vertex>, path: &[Vec2], start_tangent: Vec2, end_tangent: Vec2, color: [f32; 3], surface: &SurfaceMesh) {
    if path.len() < 2 {
        return;
    }
    let pts = drape_points(surface, path);
    let last = pts.len() - 1;
    let mut rows = Vec::with_capacity(pts.len());
    for (i, point) in pts.iter().enumerate() {
        let tangent = match i {
            0 => start_tangent,
            _ if i == last => end_tangent,
            _ => (pts[i + 1].p - pts[i - 1].p).normalize_or_zero(),
        };
        // The territory is on the left of the path.
        let inward = Vec2::new(-tangent.y, tangent.x);
        rows.push([0., 0.5, 1.].map(|across| {
            let q = point.p + inward * across * BAND;
            let y = if across == 0. { point.y } else { display_height(surface, q) };
            Vertex { pos: [q.x, y + LIFT, q.y], color, across, opacity: 0.85 }
        }));
    }
    for pair in rows.windows(2) {
        for side in 0..2 {
            let [a, b, c, d] = [pair[0][side], pair[0][side + 1], pair[1][side], pair[1][side + 1]];
            out.extend([a, b, c, b, d, c]);
        }
    }
}

/// Bands of the border edges of the given cells.
fn bands(world: &RenderWorld, surface: &SurfaceMesh, cells: impl Iterator<Item = (i32, i32)>) -> (Vec<Vertex>, usize) {
    let mut out = Vec::new();
    let mut count = 0;
    for (q, r) in cells {
        let own = cell_edges(world, q, r);
        if own.is_empty() {
            continue;
        }
        let around = edges_around(world, q, r);
        for edge in &own {
            edge_band(&mut out, &around, edge, surface);
            count += 1;
        }
    }
    (out, count)
}

/// One chunk's band on the GPU.
struct Chunk {
    buffer: crate::gpu::memory::GpuBuffer,
    vertices: u32,
    edges: usize,
    center: [f32; 3],
    radius: f32,
}

/// Border bands in chunks of `REGION`×`REGION` cells. An update rebuilds the chunks
/// whose cells or neighbours changed owner, and those on ground that changed.
#[derive(Default)]
pub struct BorderChunks {
    width: u32,
    height: u32,
    /// Owner of every cell at the last update.
    owners: Vec<Option<u8>>,
    chunks: BTreeMap<u32, Chunk>,
    /// The flat legacy renderer's ribbons, whole.
    legacy: Option<Chunk>,
    /// Chunks marked for a rebuild that a frame's budget did not reach yet.
    pending: Vec<bool>,
}

/// Uploads, chunks rebuilt and border edges drawn after an update.
#[derive(Debug, Default, Clone, Copy)]
pub struct BorderUpdate {
    pub bytes: u64,
    pub chunks_rebuilt: usize,
}

impl BorderChunks {
    fn columns(&self) -> u32 {
        self.width.div_ceil(REGION)
    }

    fn chunk_of(&self, q: u32, r: u32) -> u32 {
        (r / REGION) * self.columns() + q / REGION
    }

    fn cells_of(&self, chunk: u32) -> impl Iterator<Item = (i32, i32)> {
        let (cx, cy) = (chunk % self.columns(), chunk / self.columns());
        let qs = cx * REGION..((cx + 1) * REGION).min(self.width);
        let rs = cy * REGION..((cy + 1) * REGION).min(self.height);
        rs.flat_map(move |r| qs.clone().map(move |q| (q as i32, r as i32)))
    }

    /// Brings the bands up to date with the world's owners and the ground under them.
    pub fn update(&mut self, core: &GpuCore, world: &RenderWorld, surface: &SurfaceMesh, ground: Option<&Changed>, budget: Budget) -> BorderUpdate {
        self.mark(world, ground);
        let center = |chunk: usize| {
            let (q, r) = self.cells_of(chunk as u32).nth((REGION * REGION / 2) as usize).unwrap_or((0, 0));
            let (x, z) = crate::scene::layout::tile_to_world(q as u32, r as u32);
            [x, z]
        };
        let order = budget.order(&self.pending, center);
        self.rebuild(core, world, surface, &order, budget)
    }

    /// Marks the chunks to rebuild (every chunk when the map changed shape or the
    /// legacy ribbons were in use) and takes the world's owners.
    fn mark(&mut self, world: &RenderWorld, ground: Option<&Changed>) {
        let owners: Vec<Option<u8>> = world.terrain.iter().map(|c| c.owner).collect();
        let reshaped = (world.width, world.height) != (self.width, self.height) || owners.len() != self.owners.len() || self.legacy.is_some();
        self.width = world.width;
        self.height = world.height;
        self.legacy = None;
        let count = self.columns() * world.height.div_ceil(REGION);
        let mut dirty = vec![reshaped; count as usize];
        if !reshaped {
            self.mark_owner_changes(&owners, &mut dirty);
        }
        if let Some(ground) = ground {
            self.mark_ground_changes(ground, &mut dirty);
        }
        self.owners = owners;
        if self.pending.len() != dirty.len() {
            self.pending = vec![false; dirty.len()];
        }
        for (slot, marked) in self.pending.iter_mut().zip(dirty) {
            *slot |= marked;
        }
    }

    /// Builds the bands of the chunks in `order` a batch at a time on worker threads
    /// while the budget lasts, and uploads them.
    fn rebuild(&mut self, core: &GpuCore, world: &RenderWorld, surface: &SurfaceMesh, order: &[usize], budget: Budget) -> BorderUpdate {
        let mut update = BorderUpdate::default();
        for batch in budget.batches(order, false) {
            let cells: Vec<Vec<(i32, i32)>> = batch.iter().map(|&chunk| self.cells_of(chunk as u32).collect()).collect();
            let built = crate::scene::terrain::surface::parallel_map(&cells, |cells| bands(world, surface, cells.iter().copied()));
            for (&chunk, (mesh, edges)) in batch.iter().zip(built) {
                self.pending[chunk] = false;
                update.chunks_rebuilt += 1;
                match upload(core, &mesh, edges) {
                    Some(gpu) => {
                        update.bytes += std::mem::size_of_val(mesh.as_slice()) as u64;
                        self.chunks.insert(chunk as u32, gpu);
                    }
                    None => {
                        self.chunks.remove(&(chunk as u32));
                    }
                }
            }
        }
        update
    }

    /// Chunks of every cell whose owner changed and of its neighbours (their edges
    /// and corners read its owner).
    fn mark_owner_changes(&self, owners: &[Option<u8>], dirty: &mut [bool]) {
        for (i, (old, new)) in self.owners.iter().zip(owners).enumerate() {
            if old == new {
                continue;
            }
            let (q, r) = ((i as u32 % self.width) as i32, (i as u32 / self.width) as i32);
            for (dq, dr) in std::iter::once((0, 0)).chain(EDGE_NEIGHBORS) {
                let (nq, nr) = (q + dq, r + dr);
                if nq >= 0 && nr >= 0 && (nq as u32) < self.width && (nr as u32) < self.height {
                    dirty[self.chunk_of(nq as u32, nr as u32) as usize] = true;
                }
            }
        }
    }

    /// Chunks with a cell on ground that changed: their bands are draped again.
    fn mark_ground_changes(&self, ground: &Changed, dirty: &mut [bool]) {
        for (chunk, slot) in dirty.iter_mut().enumerate() {
            if *slot {
                continue;
            }
            *slot = self.cells_of(chunk as u32).any(|(q, r)| ground.tile(r as u32 * self.width + q as u32));
        }
    }

    /// Some marked chunks wait for a later frame.
    pub fn has_pending(&self) -> bool {
        self.pending.iter().any(|p| *p)
    }

    /// The flat renderer's ribbons replace every chunk.
    pub fn set_legacy(&mut self, core: &GpuCore, tris: &[Vertex], edges: usize) -> u64 {
        self.chunks.clear();
        self.owners.clear();
        self.pending.clear();
        self.legacy = upload(core, tris, edges);
        std::mem::size_of_val(tris) as u64
    }

    /// Band buffers in the view: (buffer, vertex count).
    pub fn visible<'a>(&'a self, planes: &'a [glam::Vec4; 6]) -> impl Iterator<Item = (&'a wgpu::Buffer, u32)> {
        self.chunks
            .values()
            .chain(self.legacy.as_ref())
            .filter(move |c| CameraController::sphere_visible(planes, c.center, c.radius))
            .map(|c| (&*c.buffer, c.vertices))
    }

    /// Border edges drawn.
    pub fn edges(&self) -> usize {
        self.chunks.values().chain(self.legacy.as_ref()).map(|c| c.edges).sum()
    }
}

fn upload(core: &GpuCore, mesh: &[Vertex], edges: usize) -> Option<Chunk> {
    if mesh.is_empty() {
        return None;
    }
    let buffer = core.buffer(crate::gpu::memory::Category::Borders, &wgpu::BufferDescriptor {
        label: Some("border chunk"),
        size: std::mem::size_of_val(mesh) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    core.queue.write_buffer(&buffer, 0, bytemuck::cast_slice(mesh));
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for v in mesh {
        for k in 0..3 {
            low[k] = low[k].min(v.pos[k]);
            high[k] = high[k].max(v.pos[k]);
        }
    }
    let center = std::array::from_fn(|k| (low[k] + high[k]) * 0.5);
    let radius = (0..3).map(|k| (high[k] - low[k]).powi(2)).sum::<f32>().sqrt() * 0.5 + 0.05;
    Some(Chunk { buffer, vertices: mesh.len() as u32, edges, center, radius })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::state::TerrainCell;
    use kernel::ids::TileId;

    fn owned_world(width: u32, height: u32, owner: impl Fn(u32, u32) -> Option<u8>) -> RenderWorld {
        let n = (width * height) as usize;
        RenderWorld {
            visual_surface: true,
            width,
            height,
            visible: vec![1; n],
            explored: vec![1; n],
            terrain: (0..width * height)
                .map(|i| TerrainCell { tile: TileId(i), terrain: 1, owner: owner(i % width, i / width), resource: None })
                .collect(),
            ..Default::default()
        }
    }

    fn surface(world: &RenderWorld) -> SurfaceMesh {
        let style = crate::scene::style::compile(crate::scene::style::default_style()).unwrap();
        crate::scene::terrain::surface::build_in_regions(world, &style, REGION)
    }

    #[test]
    fn each_owner_gets_a_band_inside_its_own_territory() {
        let world = owned_world(8, 6, |q, _| match q {
            2 | 3 => Some(0),
            4 | 5 => Some(1),
            _ => None,
        });
        let surface = surface(&world);
        let (mesh, edges) = bands(&world, &surface, (0..6).flat_map(|r| (0..8).map(move |q| (q, r))));
        assert!(edges > 10 && !mesh.is_empty());
        let mut checked = 0;
        for v in mesh.iter().filter(|v| v.across == 1.) {
            // The inner end of every band lies inside a cell of the owner of its color.
            let tile = crate::scene::picking::world_to_tile(v.pos[0], v.pos[2], (8, 6)).unwrap();
            let owner = world.terrain[tile.0 as usize].owner.expect("an owned cell under the band");
            assert_eq!(owner_color(owner).map(|c| c * 0.76 + 0.15), v.color);
            checked += 1;
        }
        assert!(checked > 50);
    }

    /// Differential: after random ownership changes, every chunk whose bands differ
    /// from before is one the update marks; the others keep their buffers.
    #[test]
    fn owner_changes_mark_every_chunk_whose_bands_change() {
        let (width, height) = (40u32, 34u32);
        let mut world = owned_world(width, height, |q, r| Some(((q / 5 + r / 7) % 3) as u8));
        let surface = surface(&world);
        let mut chunks = BorderChunks { width, height, owners: world.terrain.iter().map(|c| c.owner).collect(), ..Default::default() };
        let chunk_bands = |w: &RenderWorld, chunks: &BorderChunks| -> Vec<Vec<u8>> {
            let count = chunks.columns() * height.div_ceil(REGION);
            (0..count).map(|c| bytemuck::cast_slice::<Vertex, u8>(&bands(w, &surface, chunks.cells_of(c)).0).to_vec()).collect()
        };
        let mut seed = 5u32;
        for step in 0..12 {
            let before = chunk_bands(&world, &chunks);
            for _ in 0..3 {
                seed = crate::scene::terrain::fields::hash_u32(seed);
                let tile = (seed % (width * height)) as usize;
                world.terrain[tile].owner = if seed % 4 == 0 { None } else { Some((seed >> 8) as u8 % 4) };
            }
            let owners: Vec<Option<u8>> = world.terrain.iter().map(|c| c.owner).collect();
            let mut dirty = vec![false; before.len()];
            chunks.mark_owner_changes(&owners, &mut dirty);
            chunks.owners = owners;
            let after = chunk_bands(&world, &chunks);
            for c in 0..before.len() {
                if before[c] != after[c] {
                    assert!(dirty[c], "step {step}: chunk {c} changed but was not marked");
                }
            }
            assert!(dirty.iter().filter(|d| **d).count() <= 3 * 4, "step {step}: a few chunks per change");
        }
    }

    /// Neighbouring edges meet at the middle of their rounded corner with the same
    /// row of vertices: the band is continuous, the corner rounded within its reach.
    #[test]
    fn edges_meet_at_their_corner_middles_without_gaps() {
        let world = owned_world(10, 8, |q, r| (q >= 3 && q <= 5 && r >= 2 && r <= 4).then_some(2));
        let surface = surface(&world);
        let mut starts = Vec::new();
        let mut ends = Vec::new();
        for r in 0..8 {
            for q in 0..10 {
                let around = edges_around(&world, q, r);
                for edge in cell_edges(&world, q, r) {
                    let mut band = Vec::new();
                    edge_band(&mut band, &around, &edge, &surface);
                    // A row pair is two quads [a b c b d c]: the first row's vertices
                    // across 0 and 0.5 open the band, the last row's are 10 and 8 from its end.
                    starts.push([band[0].pos, band[1].pos].map(|p| p.map(f32::to_bits)));
                    let n = band.len();
                    ends.push([band[n - 10].pos, band[n - 8].pos].map(|p| p.map(f32::to_bits)));
                }
            }
        }
        assert!(starts.len() >= 12, "a closed territory border: {}", starts.len());
        for start in &starts {
            let at = start[0].map(f32::from_bits);
            assert!(ends.iter().any(|end| end == start), "the band starting at {at:?} meets no band end");
        }
    }
}
