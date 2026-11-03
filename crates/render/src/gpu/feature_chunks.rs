//! Trees and props in small tile chunks with their own GPU buffers.
//!
//! Each tile's trees, props and route ribbons depend only on that tile (terrain, forest,
//! improvement, route, river, resource, a city or a unit standing on it), on which
//! neighbours join its routes, and on the terrain geometry.
//! A per-tile input key finds the changed tiles; only their chunks are rebuilt and
//! re-uploaded. A unit stepping into a forest rebuilds one or two chunks, not the map.
use super::context::GpuCore;
use super::memory::{Category, GpuBuffer};
use super::feature::{FeatureVertex, GpuFeatureVertex};
use crate::camera::CameraController;
use crate::scene::terrain::fields::TerrainGeometry;
use crate::gpu::budget::Budget;
use crate::gpu::terrain_job::Changed;
use crate::world::state::{LayerRevisions, RenderWorld};
use kernel::ids::TileId;

/// Chunk side in tiles: small enough that a local change uploads tens of kilobytes,
/// large enough that a strategic view stays within a few hundred culled draws.
pub const FEATURE_CHUNK: u32 = 8;

#[derive(Default)]
struct Chunk {
    buffer: Option<GpuBuffer>,
    capacity: u64,
    vertices: u32,
    center: [f32; 3],
    radius: f32,
}

#[derive(Default)]
pub struct FeatureChunks {
    cols: u32,
    rows: u32,
    width: u32,
    chunks: Vec<Chunk>,
    keys: Vec<TileKey>,
    /// Tiles whose trees make room for a unit standing there (sorted).
    unit_tiles: Vec<u32>,
    /// Layer revisions `keys` were computed at.
    layers: Option<LayerRevisions>,
    /// Chunks marked for a rebuild that a frame's budget did not reach yet.
    pending: Vec<bool>,
    /// Which tiles were known when `keys` were computed.
    known: Vec<bool>,
    /// Meshes the terrain worker built for chunks on changed ground, with the tile keys
    /// they were built for; used instead of a rebuild while the keys still match.
    prebuilt: std::collections::HashMap<usize, (Vec<TileKey>, Vec<GpuFeatureVertex>)>,
    /// Bumped whenever any chunk changes (shadow casters follow).
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FeatureUpdate {
    pub bytes: u64,
    pub chunks_rebuilt: usize,
}

/// The key bit of a unit standing on the tile (`tile_keys`).
const UNIT_BIT: u64 = 1 << 44;

/// What a tile's trees and props read: its own data packed in one word, and which
/// tiles around it are known (the knowledge wash of `fog_at_position` reads tiles up to
/// `WASH_RING` away from a point of the tile).
type TileKey = (u64, u64);

/// Tiles around a tile whose knowledge washes its trees: hex distance at most 3, 37
/// tiles, one bit each.
const WASH_RING: i32 = 3;

fn knowledge_around(world: &RenderWorld, tile: usize) -> u64 {
    let (w, h) = (world.width as i32, world.height as i32);
    let (q, r) = (tile as i32 % w.max(1), tile as i32 / w.max(1));
    let mut bits = 0u64;
    let mut bit = 0;
    for dr in -WASH_RING..=WASH_RING {
        for dq in -WASH_RING..=WASH_RING {
            if (dq.abs() + dr.abs() + (dq + dr).abs()) / 2 > WASH_RING {
                continue;
            }
            let (nq, nr) = (q + dq, r + dr);
            let known = nq >= 0 && nr >= 0 && nq < w && nr < h && world.is_known((nr * w + nq) as usize);
            bits |= u64::from(known) << bit;
            bit += 1;
        }
    }
    bits
}

/// Tiles where a unit stands and occupancy matters, sorted and deduplicated.
fn unit_tiles(world: &RenderWorld) -> Vec<u32> {
    let mut tiles: Vec<u32> = world
        .units
        .iter()
        .filter(|unit| (unit.tile.0 as usize) < world.terrain.len())
        .filter(|unit| super::visual_props::occupancy_matters(world, unit.tile))
        .map(|unit| unit.tile.0)
        .collect();
    tiles.sort_unstable();
    tiles.dedup();
    tiles
}

/// Tiles in exactly one of two sorted lists.
fn symmetric_difference(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = a.iter().copied().filter(|t| b.binary_search(t).is_err()).collect();
    out.extend(b.iter().copied().filter(|t| a.binary_search(t).is_err()));
    out
}

/// Everything a tile's trees and props read from the projection.
fn tile_keys(world: &RenderWorld) -> Vec<TileKey> {
    let reader = KeyReader::new(world);
    (0..world.terrain.len()).map(|i| reader.key(world, i)).collect()
}

/// What tile keys read besides the tile: the tiles units and cities stand on, and the
/// city mask that route joins read.
struct KeyReader {
    occupied: Vec<u64>,
    cities: Vec<bool>,
}

impl KeyReader {
    fn new(world: &RenderWorld) -> Self {
        let mut occupied = vec![0u64; world.terrain.len()];
        for tile in unit_tiles(world) {
            occupied[tile as usize] |= UNIT_BIT;
        }
        for city in &world.cities {
            if let Some(slot) = occupied.get_mut(city.tile.0 as usize) {
                *slot |= 1 << 45;
            }
        }
        Self { occupied, cities: super::routes::city_mask(world) }
    }

    fn key(&self, world: &RenderWorld, i: usize) -> TileKey {
        let byte = |v: &[u8], i: usize| v.get(i).copied().unwrap_or(0) as u64;
        let (occupied, cities) = (&self.occupied, &self.cities);
        {
            let cell = world.terrain[i];
            let resource = cell.resource.map_or(0, |r| r as u64 + 1);
            let own = cell.terrain as u64
                | resource << 16
                | byte(&world.forest, i).min(1) << 32
                | u64::from(world.improvements.get(i).copied().flatten().is_some()) << 33
                | byte(&world.road, i) << 34
                | byte(&world.river, i).min(1) << 42
                | occupied[i]
                | u64::from(world.is_known(i)) << 46
                | u64::from(world.natural_wonders.get(i).copied().flatten().is_some()) << 47
                | u64::from(super::routes::joins(world, &cities, i as u32)) << 48
                | (world.feature.get(i).copied().unwrap_or_default() as u64) << 49;
            (own, knowledge_around(world, i))
        }
    }
}

/// Feature chunk meshes a terrain update brings along: built by the terrain worker
/// for the chunks holding a tile whose ground changed, from its snapshot.
#[derive(Default)]
pub struct Prebuilt {
    chunks: Vec<(usize, Vec<TileKey>, Vec<GpuFeatureVertex>)>,
}

/// The chunks on changed ground, built from the snapshot's knowledge view `view` on
/// the update's geometry, with the keys of their tiles.
pub fn prebuild(view: &RenderWorld, geom: &TerrainGeometry, changed: &Changed) -> Prebuilt {
    let layout = FeatureChunks {
        cols: view.width.div_ceil(FEATURE_CHUNK),
        rows: view.height.div_ceil(FEATURE_CHUNK),
        width: view.width,
        ..Default::default()
    };
    let count = (layout.cols * layout.rows) as usize;
    let mut marked = vec![false; count];
    for (index, slot) in marked.iter_mut().enumerate() {
        *slot = layout.tiles_of(index, view.height).iter().any(|t| changed.tile(t.0));
    }
    let indices: Vec<usize> = (0..count).filter(|&i| marked[i]).collect();
    let reader = KeyReader::new(view);
    let chunks = crate::scene::terrain::surface::parallel_map(&indices, |&index| {
        let tiles = layout.tiles_of(index, view.height);
        let keys = tiles.iter().map(|t| reader.key(view, t.0 as usize)).collect();
        (index, keys, chunk_mesh(view, geom, &tiles))
    });
    Prebuilt { chunks }
}

impl FeatureChunks {
    fn chunk_of(&self, tile: usize) -> usize {
        let q = tile as u32 % self.width;
        let r = tile as u32 / self.width;
        (r / FEATURE_CHUNK * self.cols + q / FEATURE_CHUNK) as usize
    }

    fn tiles_of(&self, chunk: usize, height: u32) -> Vec<TileId> {
        let cx = chunk as u32 % self.cols;
        let cy = chunk as u32 / self.cols;
        let mut tiles = Vec::new();
        for r in cy * FEATURE_CHUNK..((cy + 1) * FEATURE_CHUNK).min(height) {
            for q in cx * FEATURE_CHUNK..((cx + 1) * FEATURE_CHUNK).min(self.width) {
                tiles.push(TileId(r * self.width + q));
            }
        }
        tiles
    }

    /// Rebuilds the chunks whose tiles changed and those on ground that changed.
    /// `world` is the geometry view (visibility reduced to knowledge).
    pub fn update(
        &mut self,
        core: &GpuCore,
        world: &RenderWorld,
        geom: &TerrainGeometry,
        ground: Option<&Changed>,
        budget: Budget,
    ) -> FeatureUpdate {
        let dirty = self.plan(world, ground);
        if self.pending.len() != dirty.len() {
            self.pending = vec![false; dirty.len()];
        }
        for (slot, marked) in self.pending.iter_mut().zip(dirty) {
            *slot |= marked;
        }
        let center = |index: usize| {
            let tiles = self.tiles_of(index, world.height);
            let middle = tiles[tiles.len() / 2].0;
            let (x, z) = crate::scene::layout::tile_to_world(middle % self.width, middle / self.width);
            [x, z]
        };
        let order = budget.order(&self.pending, center);
        let mut update = FeatureUpdate::default();
        let missing = self.upload_prebuilt(core, order, world.height, budget, &mut update);
        self.build_missing(core, world, geom, &missing, budget, &mut update);
        if !self.has_pending() {
            // What no chunk took is stale.
            self.prebuilt.clear();
        }
        if update.chunks_rebuilt > 0 {
            self.generation += 1;
        }
        update
    }

    /// Uploads the chunks of `order` the terrain worker prebuilt, while the budget
    /// lasts: a prebuilt mesh whose tile keys still match is the mesh a rebuild would
    /// give. Returns the chunks it has no mesh for.
    fn upload_prebuilt(
        &mut self,
        core: &GpuCore,
        order: Vec<usize>,
        height: u32,
        budget: Budget,
        update: &mut FeatureUpdate,
    ) -> Vec<usize> {
        let mut missing = Vec::new();
        for index in order {
            if update.chunks_rebuilt > 0 && budget.spent() {
                break;
            }
            match self.take_prebuilt(index, height) {
                Some(mesh) => self.upload(core, index, &mesh, update),
                None => missing.push(index),
            }
        }
        missing
    }

    /// Builds the `missing` chunks a batch at a time on worker threads while the
    /// budget lasts, and uploads them.
    fn build_missing(
        &mut self,
        core: &GpuCore,
        world: &RenderWorld,
        geom: &TerrainGeometry,
        missing: &[usize],
        budget: Budget,
        update: &mut FeatureUpdate,
    ) {
        for batch in budget.batches(missing, update.chunks_rebuilt > 0) {
            let tiles: Vec<Vec<TileId>> = batch.iter().map(|&index| self.tiles_of(index, world.height)).collect();
            let meshes = crate::scene::terrain::surface::parallel_map(&tiles, |tiles| chunk_mesh(world, geom, tiles));
            for (&index, mesh) in batch.iter().zip(meshes) {
                self.upload(core, index, &mesh, update);
            }
        }
    }

    fn upload(&mut self, core: &GpuCore, index: usize, mesh: &[GpuFeatureVertex], update: &mut FeatureUpdate) {
        update.bytes += self.chunks[index].upload(core, mesh);
        update.chunks_rebuilt += 1;
        self.pending[index] = false;
    }

    /// Which chunks to rebuild: those whose tiles' keys changed and those on ground
    /// that changed. Updates the keys.
    fn plan(&mut self, world: &RenderWorld, ground: Option<&Changed>) -> Vec<bool> {
        let cols = world.width.div_ceil(FEATURE_CHUNK);
        let rows = world.height.div_ceil(FEATURE_CHUNK);
        let reshaped = (cols, rows, world.width) != (self.cols, self.rows, self.width)
            || world.terrain.len() != self.keys.len();
        if reshaped {
            self.cols = cols;
            self.rows = rows;
            self.width = world.width;
            self.chunks = (0..cols * rows).map(|_| Chunk::default()).collect();
        }
        let mut dirty = vec![reshaped; self.chunks.len()];
        if let Some(ground) = ground {
            // A tile's trees and props stand within its hex: its ground digest covers them.
            for (index, slot) in dirty.iter_mut().enumerate() {
                *slot |= self.tiles_of(index, world.height).iter().any(|t| ground.tile(t.0));
            }
        }
        let changed = if reshaped { Vec::new() } else { self.refresh_keys(world) };
        if reshaped {
            self.keys = tile_keys(world);
            self.unit_tiles = unit_tiles(world);
            self.known = (0..world.terrain.len()).map(|i| world.is_known(i)).collect();
        }
        self.layers = Some(world.layers);
        for tile in changed {
            let chunk = self.chunk_of(tile);
            dirty[chunk] = true;
        }
        dirty
    }

    /// Brings the tile keys up to date; returns the tiles whose key changed. When only
    /// units moved, only the unit bit of the tiles a unit left or entered can change.
    fn refresh_keys(&mut self, world: &RenderWorld) -> Vec<usize> {
        // A move changes what is visible, rarely what is known: keys read knowledge only.
        let same_known = (0..world.terrain.len()).all(|i| world.is_known(i) == self.known.get(i).is_some_and(|k| *k));
        let units_only = same_known
            && self.layers.is_some_and(|old| {
                let now = world.layers;
                LayerRevisions { units: now.units, knowledge: now.knowledge, ..old } == now
            });
        if !same_known {
            self.known = (0..world.terrain.len()).map(|i| world.is_known(i)).collect();
        }
        let unit_tiles = unit_tiles(world);
        if units_only {
            let mut changed = Vec::new();
            for tile in symmetric_difference(&self.unit_tiles, &unit_tiles) {
                let i = tile as usize;
                let occupied = if unit_tiles.binary_search(&tile).is_ok() { UNIT_BIT } else { 0 };
                let key = ((self.keys[i].0 & !UNIT_BIT) | occupied, self.keys[i].1);
                if key != self.keys[i] {
                    self.keys[i] = key;
                    changed.push(i);
                }
            }
            self.unit_tiles = unit_tiles;
            return changed;
        }
        let keys = tile_keys(world);
        let changed = (0..keys.len()).filter(|&i| keys[i] != self.keys[i]).collect();
        self.keys = keys;
        self.unit_tiles = unit_tiles;
        changed
    }

    /// The prebuilt mesh of a chunk if the keys it was built for are the current ones.
    fn take_prebuilt(&mut self, index: usize, height: u32) -> Option<Vec<GpuFeatureVertex>> {
        let (keys, mesh) = self.prebuilt.remove(&index)?;
        let tiles = self.tiles_of(index, height);
        let current = keys.len() == tiles.len() && tiles.iter().zip(&keys).all(|(t, k)| self.keys[t.0 as usize] == *k);
        current.then_some(mesh)
    }

    /// Takes the meshes a terrain update prebuilt (later ones replace earlier ones).
    pub fn adopt(&mut self, prebuilt: Prebuilt) {
        for (index, keys, mesh) in prebuilt.chunks {
            self.prebuilt.insert(index, (keys, mesh));
        }
    }

    /// Some marked chunks wait for a later frame.
    pub fn has_pending(&self) -> bool {
        self.pending.iter().any(|p| *p)
    }

    pub fn triangles(&self) -> usize {
        self.chunks.iter().map(|c| c.vertices as usize / 3).sum()
    }

    /// Visible chunk buffers and their vertex counts.
    pub fn visible<'a>(&'a self, planes: &'a [glam::Vec4; 6]) -> impl Iterator<Item = (&'a wgpu::Buffer, u32)> {
        self.chunks.iter().filter_map(move |c| {
            let buffer: &wgpu::Buffer = c.buffer.as_ref()?;
            let seen = c.vertices > 0 && CameraController::sphere_visible(planes, c.center, c.radius);
            seen.then_some((buffer, c.vertices))
        })
    }

    /// Chunks overlapping a world XZ box (shadow casters).
    pub fn within(&self, low: [f32; 3], high: [f32; 3]) -> impl Iterator<Item = (&wgpu::Buffer, u32)> {
        self.chunks.iter().filter_map(move |c| {
            let buffer: &wgpu::Buffer = c.buffer.as_ref()?;
            let inside = c.vertices > 0
                && c.center[0] + c.radius >= low[0]
                && c.center[0] - c.radius <= high[0]
                && c.center[2] + c.radius >= low[2]
                && c.center[2] - c.radius <= high[2];
            inside.then_some((buffer, c.vertices))
        })
    }
}

/// Trees and props of the chunk's tiles, with the conservative knowledge wash, in the
/// GPU form: meshes waiting for upload hold 32 B per vertex, not 40.
fn chunk_mesh(world: &RenderWorld, geom: &TerrainGeometry, tiles: &[TileId]) -> Vec<GpuFeatureVertex> {
    let parts = [
        super::feature::build_tree_mesh_for(world, geom, tiles),
        super::visual_props::build_for(world, geom, tiles),
        super::routes::build_for(world, geom, tiles),
    ];
    let mut mesh = Vec::with_capacity(parts.iter().map(Vec::len).sum());
    for vertex in parts.iter().flatten() {
        let wash = crate::scene::fog::fog_at_position(world, vertex.pos[0], vertex.pos[2]);
        mesh.push(GpuFeatureVertex::from(&FeatureVertex { fog: vertex.fog.min(wash), ..*vertex }));
    }
    mesh
}

/// Bytes of a chunk buffer for a mesh of `bytes`: an eighth more for growth, in pages.
/// A chunk is rewritten whole on every change, so headroom only saves re-creating the
/// buffer; powers of two cost a third of the features' GPU memory.
fn chunk_capacity(bytes: u64) -> u64 {
    (bytes + bytes / 8).next_multiple_of(4096)
}

impl Chunk {
    fn upload(&mut self, core: &GpuCore, mesh: &[GpuFeatureVertex]) -> u64 {
        self.vertices = mesh.len() as u32;
        if mesh.is_empty() {
            if let Some(buffer) = &mut self.buffer {
                buffer.set_used(0);
            }
            return 0;
        }
        let bytes = std::mem::size_of_val(mesh) as u64;
        if bytes > self.capacity || self.buffer.is_none() {
            self.capacity = chunk_capacity(bytes);
            self.buffer = Some(core.buffer(
                Category::Features,
                &wgpu::BufferDescriptor {
                    label: Some("feature chunk"),
                    size: self.capacity,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                },
            ));
        }
        let buffer = self.buffer.as_mut().expect("feature chunk buffer allocated");
        buffer.set_used(bytes);
        core.queue.write_buffer(buffer, 0, bytemuck::cast_slice(mesh));
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for v in mesh {
            for k in 0..3 {
                low[k] = low[k].min(v.pos[k]);
                high[k] = high[k].max(v.pos[k]);
            }
        }
        self.center = std::array::from_fn(|k| (low[k] + high[k]) * 0.5);
        let extent: f32 = (0..3).map(|k| (high[k] - low[k]).powi(2)).sum::<f32>().sqrt();
        self.radius = extent * 0.5 + 0.05;
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::state::{TerrainCell, UnitMarker};

    fn world(w: u32, h: u32) -> RenderWorld {
        let n = (w * h) as usize;
        RenderWorld {
            width: w,
            height: h,
            terrain: (0..n as u32)
                .map(|i| TerrainCell {
                    tile: TileId(i),
                    terrain: 0,
                    owner: None,
                    resource: None,
                })
                .collect(),
            visible: vec![1; n],
            explored: vec![1; n],
            improvements: vec![None; n],
            road: vec![0; n],
            river: vec![0; n],
            forest: vec![1; n],
            ..Default::default()
        }
    }

    #[test]
    fn moving_on_bare_ground_does_not_rebuild_feature_chunks() {
        let mut before = world(32, 24);
        before.forest.fill(0);
        before.units.push(UnitMarker { id: 0, embarked: false, fortified: false, tile: TileId(3), owner: 0, kind: 1, hp: 100 });
        let mut after = before.clone();
        after.units[0].tile = TileId(4 * 32 + 20);
        assert!(unit_tiles(&before).is_empty());
        assert_eq!(tile_keys(&before), tile_keys(&after), "no random rocks to move out of a unit's path");
    }

    #[test]
    fn unit_step_dirties_only_the_chunks_of_its_two_tiles() {
        let mut a = world(32, 24);
        a.units.push(UnitMarker {
            id: 0, embarked: false, fortified: false, tile: TileId(3),
            owner: 0,
            kind: 1,
            hp: 100,
        });
        let mut b = a.clone();
        b.units[0].tile = TileId(4 * 32 + 20);
        let (ka, kb) = (tile_keys(&a), tile_keys(&b));
        let chunks = FeatureChunks {
            cols: 4,
            rows: 3,
            width: 32,
            ..Default::default()
        };
        let mut dirty: Vec<usize> = ka
            .iter()
            .zip(&kb)
            .enumerate()
            .filter(|(_, (x, y))| x != y)
            .map(|(i, _)| chunks.chunk_of(i))
            .collect();
        dirty.dedup();
        assert_eq!(dirty, vec![0, 2]);
    }

    /// Differential: keys updated unit by unit equal keys computed from scratch, and the
    /// reported tiles are exactly those whose key changed.
    #[test]
    fn incremental_keys_equal_fresh_keys() {
        let mut world = world(32, 24);
        for i in 0..32 * 24 {
            world.forest[i] = u8::from(i % 3 != 0);
        }
        world.layers = LayerRevisions::INITIAL;
        let mut chunks = FeatureChunks {
            cols: 4,
            rows: 3,
            width: 32,
            keys: tile_keys(&world),
            unit_tiles: unit_tiles(&world),
            layers: Some(world.layers),
            ..Default::default()
        };
        let mut seed = 11u32;
        for step in 0..80 {
            seed = crate::scene::terrain::fields::hash_u32(seed);
            let tile = TileId(seed % (32 * 24));
            if world.units.len() < 12 || seed % 5 == 0 {
                world.units.push(UnitMarker { id: 0, embarked: false, fortified: false, tile, owner: 0, kind: 1, hp: 100 });
            } else {
                let i = (seed >> 8) as usize % world.units.len();
                world.units[i].tile = tile;
            }
            world.touch(crate::world::state::Layer::Units);
            // A move also changes what is visible; every tile stays known.
            let seen = (seed >> 4) as usize % (32 * 24);
            world.visible[seen] ^= 1;
            world.touch(crate::world::state::Layer::Knowledge);
            let before = chunks.keys.clone();
            let changed = chunks.refresh_keys(&world);
            chunks.layers = Some(world.layers);
            let fresh = tile_keys(&world);
            assert_eq!(chunks.keys, fresh, "step {step}");
            let expected: Vec<usize> = (0..fresh.len()).filter(|&i| fresh[i] != before[i]).collect();
            let mut changed = changed;
            changed.sort_unstable();
            assert_eq!(changed, expected, "step {step}");
        }
    }

    /// Differential: while a generated map is revealed, every chunk whose trees, props
    /// or routes come out different from before is one the plan rebuilds.
    #[test]
    fn a_reveal_rebuilds_every_chunk_whose_meshes_change() {
        use crate::gpu::terrain_job::TerrainBuilder;
        use crate::world::projection::{rebuild_render_world, refresh};
        use kernel::ids::PlayerId;
        let style = crate::scene::style::compile(crate::scene::style::default_style()).unwrap();
        let rules = sandbox_content::rules().expect("sandbox rules");
        let config = kernel::scenario::ScenarioConfig { seed: 3, players: 2, width: 48, height: 40, cities: 0, units: 0, water_ratio: 30 };
        let mut game = kernel::scenario::generate(&config, &rules);
        let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
        world.visual_surface = true;
        let mut builder = TerrainBuilder::new(style);
        let first = builder.build(0, std::sync::Arc::new(world.clone()), &Default::default());
        let mut chunks = FeatureChunks::default();
        let mut view = crate::scene::fog::knowledge_view(&world);
        chunks.plan(&view, Some(&first.changed));
        let all = |view: &RenderWorld, geom: &TerrainGeometry, chunks: &FeatureChunks| -> Vec<Vec<u8>> {
            (0..chunks.chunks.len()).map(|c| bytemuck::cast_slice::<GpuFeatureVertex, u8>(&chunk_mesh(view, geom, &chunks.tiles_of(c, view.height))).to_vec()).collect()
        };
        let mut before = all(&view, &first.geometry, &chunks);
        let (mut rebuilt, mut changed_chunks) = (0, 0);
        for (step, (cq, cr)) in [(8, 8), (12, 10), (17, 13), (24, 20), (33, 28)].into_iter().enumerate() {
            let patch: Vec<TileId> = (0..config.height)
                .flat_map(|r| (0..config.width).map(move |q| (q, r)))
                .filter(|&(q, r)| {
                    let (dq, dr) = (q as i32 - cq, r as i32 - cr);
                    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2 <= 3
                })
                .map(|(q, r)| TileId(r * config.width + q))
                .collect();
            game.debug_reveal(PlayerId(0), &patch);
            refresh(&mut world, game.view(), PlayerId(0), &rules);
            let update = builder.build(step as u64 + 1, std::sync::Arc::new(world.clone()), &Default::default());
            view = crate::scene::fog::knowledge_view(&world);
            let dirty = chunks.plan(&view, Some(&update.changed));
            // The worker's prebuilt chunks hold every chunk on changed ground, and a
            // prebuilt mesh is the mesh of any world whose tile keys match: here a world
            // where units moved after the snapshot.
            let mut moved = view.clone();
            moved.units = (0..30).map(|i| crate::world::state::UnitMarker { id: 0, embarked: false, fortified: false, tile: TileId((i * 61 + step as u32 * 7) % (48 * 40)), owner: 0, kind: 1, hp: 100 }).collect();
            let reader = KeyReader::new(&moved);
            let mut matched = 0;
            for (index, keys, mesh) in &update.features.chunks {
                let tiles = chunks.tiles_of(*index, view.height);
                let now: Vec<TileKey> = tiles.iter().map(|t| reader.key(&moved, t.0 as usize)).collect();
                if now == *keys {
                    matched += 1;
                    let fresh = chunk_mesh(&moved, &update.geometry, &tiles);
                    assert!(bytemuck::cast_slice::<GpuFeatureVertex, u8>(&fresh) == bytemuck::cast_slice::<GpuFeatureVertex, u8>(mesh), "step {step}: prebuilt chunk {index}");
                }
            }
            let prebuilt: Vec<usize> = update.features.chunks.iter().map(|c| c.0).collect();
            for (c, marked) in dirty.iter().enumerate() {
                let on_ground = chunks.tiles_of(c, view.height).iter().any(|t| update.changed.tile(t.0));
                assert!(!on_ground || (*marked && prebuilt.contains(&c)), "step {step}: chunk {c} on changed ground");
            }
            assert!(matched > 0 || prebuilt.is_empty(), "step {step}: some prebuilt chunks are usable");
            let after = all(&view, &update.geometry, &chunks);
            for c in 0..after.len() {
                if after[c] != before[c] {
                    changed_chunks += 1;
                    assert!(dirty[c], "step {step}: chunk {c} changed but is not rebuilt");
                }
            }
            rebuilt += dirty.iter().filter(|d| **d).count();
            before = after;
        }
        assert!(changed_chunks > 0, "the reveals change trees");
        assert!(rebuilt < 5 * chunks.chunks.len() / 2, "a reveal rebuilds a part of the map: {rebuilt}");
    }

    #[test]
    fn every_tile_belongs_to_exactly_one_chunk() {
        let chunks = FeatureChunks {
            cols: 5,
            rows: 3,
            width: 37,
            ..Default::default()
        };
        let mut seen = vec![0; 37 * 19];
        for c in 0..15 {
            for t in chunks.tiles_of(c, 19) {
                seen[t.0 as usize] += 1;
                assert_eq!(chunks.chunk_of(t.0 as usize), c);
            }
        }
        assert!(seen.iter().all(|&n| n == 1));
    }
}
