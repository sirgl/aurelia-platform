//! Terrain products off the frame (platform/MESHING.md §2.5: background builds, versioned
//! publish).
//!
//! A job is a snapshot of the world at one terrain revision. The worker derives the
//! knowledge view, the heightfield and the surface, reusing the part of every region
//! whose inputs did not change (`SurfaceBuilder`), compares each region with the last
//! update it sent and returns only the GPU-ready meshes that differ. Updates arrive in
//! job order; each is a pure function of its snapshot and the previous update, so
//! after applying them all the renderer holds exactly what a full build of the last
//! snapshot gives. Snapshots that a newer one overtook in the queue are never built.
//!
//! The flat legacy renderer (no continuous surface) is rebuilt whole by the same path.
use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::sync::Arc;

use crate::gpu::buffers::{
    build_chunk, build_grid_lines, build_lake_surface, build_river_paths, build_water_strip, ChunkMesh, WaterVertex,
    CHUNK, RIVER_LIFT, RIVER_WIDTH,
};
use crate::gpu::surface_mesh::{region_meshes, RegionMeshes};
use crate::scene::fog::knowledge_view;
use crate::scene::heightfield::{build_heightfield, Heightfield};
use crate::scene::style::CompiledTerrainStyle;
use crate::scene::terrain::fields::{build_terrain_geometry, TerrainGeometry};
use crate::scene::terrain::surface::{Levels, RegionId, SurfaceBuilder, SurfaceMesh, REGION};
use crate::world::state::RenderWorld;

/// One independently replaced part of the terrain on the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Piece {
    /// A surface region: its ground, water and grid.
    Region(RegionId),
    /// A chunk of the flat legacy ground.
    Chunk(u32),
    /// The legacy renderer's water and grid, whole.
    Overlay,
}

/// CPU meshes of a piece, ready to upload.
#[derive(Default)]
pub struct PieceMeshes {
    pub land: Option<ChunkMesh>,
    pub water: Vec<WaterVertex>,
    pub grid: Vec<[f32; 3]>,
}

/// Where the ground changed: what stands there is placed again.
#[derive(Debug, Clone, PartialEq)]
pub enum Changed {
    Everything,
    /// Tiles whose ground or water changed (`SurfaceMesh::tile_digest`).
    Tiles(TileSet),
}

/// A set of tiles of one map.
#[derive(Debug, Clone, PartialEq)]
pub struct TileSet {
    width: u32,
    height: u32,
    marked: Vec<bool>,
    list: Vec<u32>,
}

impl TileSet {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, marked: vec![false; (width * height) as usize], list: Vec::new() }
    }

    pub fn insert(&mut self, tile: u32) {
        if let Some(slot) = self.marked.get_mut(tile as usize) {
            if !*slot {
                *slot = true;
                self.list.push(tile);
            }
        }
    }
}

impl Changed {
    pub fn tile(&self, tile: u32) -> bool {
        match self {
            Changed::Everything => true,
            Changed::Tiles(set) => set.marked.get(tile as usize).copied().unwrap_or(true),
        }
    }

    /// Tiles at most `ring` steps from a changed tile; None when every tile changed.
    pub fn near(&self, ring: i32) -> Option<Vec<bool>> {
        let Changed::Tiles(set) = self else { return None };
        let (w, h) = (set.width as i32, set.height as i32);
        let mut near = vec![false; set.marked.len()];
        for &tile in &set.list {
            let (q, r) = (tile as i32 % w, tile as i32 / w);
            for dr in -ring..=ring {
                for dq in -ring..=ring {
                    let (nq, nr) = (q + dq, r + dr);
                    let inside = nq >= 0 && nr >= 0 && nq < w && nr < h;
                    if inside && (dq.abs() + dr.abs() + (dq + dr).abs()) / 2 <= ring {
                        near[(nr * w + nq) as usize] = true;
                    }
                }
            }
        }
        Some(near)
    }

    /// Both changes: what changed in either.
    pub fn merge(self, other: Changed) -> Changed {
        match (self, other) {
            (Changed::Tiles(mut a), Changed::Tiles(b)) if a.marked.len() == b.marked.len() => {
                for tile in b.list {
                    a.insert(tile);
                }
                Changed::Tiles(a)
            }
            _ => Changed::Everything,
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Changed::Tiles(set) if set.list.is_empty())
    }
}

pub struct TerrainUpdate {
    /// Terrain revision of the snapshot.
    pub revision: u64,
    /// Detail levels the regions were built at.
    pub levels: Levels,
    /// The snapshot: its knowledge is what the products show.
    pub world: Arc<RenderWorld>,
    pub heightfield: Arc<Heightfield>,
    pub geometry: Arc<TerrainGeometry>,
    /// Pieces to (re)place.
    pub pieces: Vec<(Piece, PieceMeshes)>,
    /// Pieces the new geometry no longer has.
    pub removed: Vec<Piece>,
    pub changed: Changed,
    /// Lowest and highest ground.
    pub terrain_y: (f32, f32),
    /// Feature chunks on the changed ground, built from the snapshot.
    pub features: crate::gpu::feature_chunks::Prebuilt,
    /// Worker time of the build and the regions it triangulated.
    pub build_ms: f32,
    pub regions_rebuilt: usize,
}

/// The worker's state: everything the next update is compared with.
pub struct TerrainBuilder {
    style: CompiledTerrainStyle,
    surface: SurfaceBuilder,
    /// Per region: digest of its data, and of the meshes last sent for it.
    regions: BTreeMap<RegionId, (u64, u64)>,
    /// Per tile: digest of the ground under it (`SurfaceMesh::tile_digest`).
    tiles: Vec<u64>,
    /// Pieces the renderer holds after the last update.
    pieces: BTreeSet<Piece>,
    /// Map size and mode of the last build.
    shape: Option<(u32, u32, bool)>,
    /// The geometry of the last update, shared with the renderer: the surface builder
    /// copies the regions it keeps from it.
    last: Option<Arc<TerrainGeometry>>,
}

impl TerrainBuilder {
    pub fn new(style: CompiledTerrainStyle) -> Self {
        Self {
            style,
            surface: SurfaceBuilder::default(),
            regions: BTreeMap::new(),
            tiles: Vec::new(),
            pieces: BTreeSet::new(),
            shape: None,
            last: None,
        }
    }

    pub fn build(&mut self, revision: u64, world: Arc<RenderWorld>, levels: &Levels) -> TerrainUpdate {
        let start = web_time::Instant::now();
        let view = knowledge_view(&world);
        // The heightfield shapes only the flat legacy ground; on the surface every
        // height comes from the mesh (lines are draped on it).
        let hf = if view.visual_surface { Heightfield::default() } else { build_heightfield(&view, &self.style) };
        let shape = (view.width, view.height, view.visual_surface);
        let reshaped = self.shape != Some(shape);
        if reshaped {
            self.shape = Some(shape);
            self.regions.clear();
            self.tiles = Vec::new();
        }
        let mut products = if view.visual_surface { self.surface_products(&view, &hf, levels) } else { self.legacy_products(&view, &hf) };
        if reshaped {
            products.changed = Changed::Everything;
        }
        let held: BTreeSet<Piece> = products.held;
        let removed: Vec<Piece> = self.pieces.difference(&held).copied().collect();
        self.pieces = held;
        let features = crate::gpu::feature_chunks::prebuild(&view, &products.geometry, &products.changed);
        let geometry = Arc::new(products.geometry);
        self.last = Some(geometry.clone());
        TerrainUpdate {
            revision,
            levels: levels.clone(),
            world,
            heightfield: Arc::new(hf),
            geometry,
            pieces: products.pieces,
            removed,
            changed: products.changed,
            features,
            terrain_y: products.terrain_y,
            build_ms: start.elapsed().as_secs_f32() * 1000.0,
            regions_rebuilt: products.rebuilt,
        }
    }

    /// The surface and the meshes of the regions that differ from the last update: a
    /// region whose data changed, or whose neighbour's did (normals at the seam).
    fn surface_products(&mut self, view: &RenderWorld, hf: &Heightfield, levels: &Levels) -> Products {
        let previous = self.last.as_ref().and_then(|geometry| geometry.surface.as_ref());
        let surface = self.surface.build(view, &self.style, REGION, levels, previous);
        let rebuilt = self.surface.rebuilt.len();
        // The relief, boundary and patches of the flat ground stay empty: on the
        // surface every query reads the mesh (`evaluator::sample_surface`).
        let geometry = TerrainGeometry { surface: Some(surface), width: view.width, height: view.height, ..TerrainGeometry::default() };
        let surface = geometry.surface.as_ref().expect("surface built above");
        let digests = self.region_digests(surface);
        let changed: Vec<usize> =
            (0..digests.len()).filter(|&r| self.regions.get(&surface.regions[r].id).map(|d| d.0) != Some(digests[r])).collect();
        let mut targets: BTreeSet<usize> = changed.iter().copied().collect();
        for &r in &changed {
            targets.extend(surface.neighbour_slots(r));
        }
        let targets: Vec<usize> = targets.into_iter().collect();
        let lines = build_grid_lines(view, hf);
        let meshes = region_meshes(surface, view, &lines, &targets);
        let (sent, pieces) = self.new_pieces(surface, &digests, meshes);
        let gone: Vec<RegionId> = self.regions.keys().filter(|id| !sent.contains_key(id)).copied().collect();
        let ids: Vec<RegionId> = changed.iter().map(|&r| surface.regions[r].id).chain(gone).collect();
        let tiles = self.changed_tiles(view, surface, &ids);
        self.regions = sent;
        let held = self.regions.keys().map(|&id| Piece::Region(id)).collect();
        let terrain_y = ground_range(surface);
        Products { geometry, pieces, held, changed: Changed::Tiles(tiles), terrain_y, rebuilt }
    }

    /// The data digest of every region slot. A region not assembled again kept its
    /// final data: its digest is the last one.
    fn region_digests(&self, surface: &SurfaceMesh) -> Vec<u64> {
        let slots: Vec<usize> = (0..surface.regions.len()).collect();
        let reassembled: BTreeSet<RegionId> = self.surface.reassembled.iter().copied().collect();
        crate::scene::terrain::surface::parallel_map(&slots, |&r| {
            let id = surface.regions[r].id;
            match self.regions.get(&id) {
                Some(&(digest, _)) if !reassembled.contains(&id) => digest,
                _ => surface.region_digest(r),
            }
        })
    }

    /// The digests to remember per region, and the region pieces whose meshes differ
    /// from the ones last sent.
    fn new_pieces(
        &self,
        surface: &SurfaceMesh,
        digests: &[u64],
        meshes: Vec<RegionMeshes>,
    ) -> (BTreeMap<RegionId, (u64, u64)>, Vec<(Piece, PieceMeshes)>) {
        let mut sent: BTreeMap<RegionId, (u64, u64)> = BTreeMap::new();
        for (r, digest) in digests.iter().enumerate() {
            let id = surface.regions[r].id;
            let mesh = self.regions.get(&id).map_or(0, |d| d.1);
            sent.insert(id, (*digest, mesh));
        }
        let mut pieces = Vec::new();
        for meshes in meshes {
            let digest = mesh_digest(&meshes.land, &meshes.water, &meshes.grid);
            let entry = sent.get_mut(&meshes.id).expect("a target is a built region");
            if entry.1 == digest && self.regions.contains_key(&meshes.id) {
                continue;
            }
            entry.1 = digest;
            pieces.push((Piece::Region(meshes.id), PieceMeshes { land: Some(meshes.land), water: meshes.water, grid: meshes.grid }));
        }
        (sent, pieces)
    }

    /// Tiles whose ground digest changed, among the tiles of the regions `ids` and the
    /// ring around them (the faces of a region reach its neighbours' edge tiles).
    fn changed_tiles(&mut self, view: &RenderWorld, surface: &SurfaceMesh, ids: &[RegionId]) -> TileSet {
        let (width, height) = (view.width, view.height);
        let mut candidates = TileSet::new(width, height);
        if self.tiles.len() != (width * height) as usize {
            self.tiles = vec![0; (width * height) as usize];
            for tile in 0..width * height {
                candidates.insert(tile);
            }
        }
        for &id in ids {
            for r in (id.y * REGION).saturating_sub(1)..((id.y + 1) * REGION + 1).min(height) {
                for q in (id.x * REGION).saturating_sub(1)..((id.x + 1) * REGION + 1).min(width) {
                    candidates.insert(r * width + q);
                }
            }
        }
        let digests = crate::scene::terrain::surface::parallel_map(&candidates.list, |&t| surface.tile_digest(t));
        let mut changed = TileSet::new(width, height);
        for (&tile, digest) in candidates.list.iter().zip(digests) {
            if self.tiles[tile as usize] != digest {
                self.tiles[tile as usize] = digest;
                changed.insert(tile);
            }
        }
        changed
    }

    /// The flat renderer, whole: chunks of the legacy ground, water and grid.
    fn legacy_products(&mut self, view: &RenderWorld, hf: &Heightfield) -> Products {
        let geometry = build_terrain_geometry(view, &self.style, hf);
        let cols = view.width.div_ceil(CHUNK);
        let rows = view.height.div_ceil(CHUNK);
        let mut pieces = Vec::new();
        let mut y = (f32::INFINITY, f32::NEG_INFINITY);
        for cy in 0..rows {
            for cx in 0..cols {
                let chunk = build_chunk(view, &self.style, &geometry, cx, cy);
                if chunk.vertices.is_empty() {
                    continue;
                }
                for v in &chunk.vertices {
                    y = (y.0.min(v.pos[1]), y.1.max(v.pos[1]));
                }
                pieces.push((Piece::Chunk(cy * cols + cx), PieceMeshes { land: Some(chunk), ..PieceMeshes::default() }));
            }
        }
        let paths = if crate::gpu::visual_river::handles_world(view) { Vec::new() } else { build_river_paths(view) };
        let mut water = build_water_strip(&paths, RIVER_WIDTH, hf, RIVER_LIFT);
        water.extend(build_lake_surface(view, &self.style, RIVER_LIFT));
        let grid = build_grid_lines(view, hf);
        pieces.push((Piece::Overlay, PieceMeshes { land: None, water, grid }));
        let held = pieces.iter().map(|(piece, _)| *piece).collect();
        let terrain_y = if y.0 <= y.1 { y } else { (0.0, 0.0) };
        Products { geometry, pieces, held, changed: Changed::Everything, terrain_y, rebuilt: 0 }
    }
}

struct Products {
    geometry: TerrainGeometry,
    pieces: Vec<(Piece, PieceMeshes)>,
    /// Every piece the renderer holds after this update.
    held: BTreeSet<Piece>,
    changed: Changed,
    terrain_y: (f32, f32),
    rebuilt: usize,
}

fn mesh_digest(land: &ChunkMesh, water: &[WaterVertex], grid: &[[f32; 3]]) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    hasher.write(bytemuck::cast_slice(&land.vertices));
    hasher.write(bytemuck::cast_slice(&land.indices));
    hasher.write(bytemuck::cast_slice(water));
    hasher.write(bytemuck::cast_slice(grid));
    hasher.finish()
}

fn ground_range(surface: &SurfaceMesh) -> (f32, f32) {
    let mut y = (f32::INFINITY, f32::NEG_INFINITY);
    for v in &surface.vertices {
        y = (y.0.min(v.position[1]), y.1.max(v.position[1]));
    }
    if y.0 <= y.1 {
        y
    } else {
        (0.0, 0.0)
    }
}

/// A job: the world at one terrain revision.
#[cfg(not(target_arch = "wasm32"))]
struct Job {
    revision: u64,
    world: Arc<RenderWorld>,
    levels: Levels,
}

/// The terrain builder on its own thread. Jobs go in through one channel, updates come
/// back through another, in job order; the builder has one owner, the worker.
#[cfg(not(target_arch = "wasm32"))]
pub struct TerrainWorker {
    jobs: Option<std::sync::mpsc::Sender<Job>>,
    updates: std::sync::mpsc::Receiver<TerrainUpdate>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl TerrainWorker {
    pub fn spawn(style: CompiledTerrainStyle) -> Self {
        let (jobs, inbox) = std::sync::mpsc::channel::<Job>();
        let (outbox, updates) = std::sync::mpsc::channel::<TerrainUpdate>();
        let thread = std::thread::Builder::new()
            .name("terrain builder".into())
            .spawn(move || {
                let mut builder = TerrainBuilder::new(style);
                while let Ok(mut job) = inbox.recv() {
                    // Only the newest snapshot matters: overtaken ones are skipped.
                    while let Ok(newer) = inbox.try_recv() {
                        job = newer;
                    }
                    if outbox.send(builder.build(job.revision, job.world, &job.levels)).is_err() {
                        return;
                    }
                }
            })
            .expect("spawn the terrain builder");
        Self { jobs: Some(jobs), updates, thread: Some(thread) }
    }

    pub fn submit(&self, revision: u64, world: Arc<RenderWorld>, levels: Levels) {
        if let Some(jobs) = &self.jobs {
            // A closed channel means the worker panicked; `wait` reports it.
            let _ = jobs.send(Job { revision, world, levels });
        }
    }

    /// The next update if one is ready.
    pub fn poll(&self) -> Option<TerrainUpdate> {
        self.updates.try_recv().ok()
    }

    /// The next update; blocks until the worker sends it.
    pub fn wait(&self) -> TerrainUpdate {
        self.updates.recv().expect("the terrain builder stopped")
    }
}

/// The terrain builder in a browser page: there are no threads, so a job is built where it
/// is submitted and its update waits for the frame to take it.
#[cfg(target_arch = "wasm32")]
pub struct TerrainWorker {
    builder: std::cell::RefCell<TerrainBuilder>,
    updates: std::cell::RefCell<std::collections::VecDeque<TerrainUpdate>>,
}

#[cfg(target_arch = "wasm32")]
impl TerrainWorker {
    pub fn spawn(style: CompiledTerrainStyle) -> Self {
        Self { builder: std::cell::RefCell::new(TerrainBuilder::new(style)), updates: Default::default() }
    }

    pub fn submit(&self, revision: u64, world: Arc<RenderWorld>, levels: Levels) {
        let update = self.builder.borrow_mut().build(revision, world, &levels);
        self.updates.borrow_mut().push_back(update);
    }

    pub fn poll(&self) -> Option<TerrainUpdate> {
        self.updates.borrow_mut().pop_front()
    }

    pub fn wait(&self) -> TerrainUpdate {
        self.poll().expect("a submitted terrain job has its update")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::style::{compile, default_style};
    use crate::world::projection::{rebuild_render_world, refresh};
    use kernel::ids::{PlayerId, TileId};

    /// The renderer's pieces as bytes, after applying an update.
    type Mirror = BTreeMap<Piece, Vec<u8>>;

    fn bytes(meshes: &PieceMeshes) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(land) = &meshes.land {
            out.extend_from_slice(bytemuck::cast_slice(&land.vertices));
            out.extend_from_slice(bytemuck::cast_slice(&land.indices));
        }
        out.extend_from_slice(bytemuck::cast_slice(&meshes.water));
        out.extend_from_slice(bytemuck::cast_slice(&meshes.grid));
        out
    }

    fn apply(mirror: &mut Mirror, update: &TerrainUpdate) {
        for piece in &update.removed {
            mirror.remove(piece);
        }
        for (piece, meshes) in &update.pieces {
            mirror.insert(*piece, bytes(meshes));
        }
    }

    /// Differential (platform/MESHING.md §3.7): the pieces held after a sequence of updates
    /// equal the pieces of one fresh build of the last world, after every reveal, and
    /// an update sends only a few regions.
    #[test]
    fn updates_leave_exactly_the_pieces_of_a_fresh_build() {
        let style = compile(default_style()).unwrap();
        let rules = sandbox_content::rules().expect("sandbox rules");
        let config = kernel::scenario::ScenarioConfig { seed: 11, players: 2, width: 64, height: 48, cities: 0, units: 0, water_ratio: 30 };
        let mut game = kernel::scenario::generate(&config, &rules);
        let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
        world.visual_surface = true;
        let mut builder = TerrainBuilder::new(style.clone());
        let mut mirror = Mirror::new();
        let mut sent = vec![];
        for (step, (cq, cr)) in [(10, 10), (13, 12), (17, 14), (22, 20), (30, 26), (40, 30), (50, 36)].into_iter().enumerate() {
            let patch: Vec<TileId> = (0..config.height)
                .flat_map(|r| (0..config.width).map(move |q| (q, r)))
                .filter(|&(q, r)| {
                    let (dq, dr) = (q as i32 - cq, r as i32 - cr);
                    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2 <= 4
                })
                .map(|(q, r)| TileId(r * config.width + q))
                .collect();
            game.debug_reveal(PlayerId(0), &patch);
            refresh(&mut world, game.view(), PlayerId(0), &rules);
            let update = builder.build(step as u64, Arc::new(world.clone()), &Levels::default());
            apply(&mut mirror, &update);
            if step > 0 {
                sent.push(update.pieces.len());
            }
            let fresh = TerrainBuilder::new(style.clone()).build(step as u64, Arc::new(world.clone()), &Levels::default());
            let mut expected = Mirror::new();
            apply(&mut expected, &fresh);
            assert_eq!(mirror.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>(), "step {step}: pieces");
            for (piece, data) in &expected {
                assert!(mirror[piece] == *data, "step {step}: {piece:?} differs from a fresh build");
            }
        }
        let regions = mirror.len();
        assert!(sent.iter().all(|&n| n < regions), "a local reveal sends some regions, not all: {sent:?} of {regions}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for TerrainWorker {
    fn drop(&mut self) {
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
