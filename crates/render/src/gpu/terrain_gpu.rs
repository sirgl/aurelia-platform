//! The terrain on the GPU: buffers per piece (`terrain_job.rs`), replaced piece by
//! piece as updates from the terrain worker arrive.
//!
//! The renderer submits a snapshot whenever the world's terrain revision moves past
//! what is built or being built, and applies finished updates at the start of a frame.
//! A frame never waits for the worker except for the very first geometry, a world of
//! another shape, or when updates are asked to be immediate (screenshots, tests): the
//! published geometry stays on screen until its successor is complete. In the
//! background a large update is uploaded over several frames (`UPLOAD_BYTES` each)
//! into buffers not drawn yet, and published at once when its last piece is up.
use std::collections::BTreeMap;
use std::sync::Arc;

use super::buffers::{ChunkMesh, WaterVertex};
use super::context::GpuCore;
use super::memory::{Category, GpuBuffer};
use super::terrain_job::{Changed, Piece, PieceMeshes, TerrainUpdate, TerrainWorker};
use crate::camera::CameraController;
use crate::scene::heightfield::Heightfield;
use crate::scene::style::CompiledTerrainStyle;
use crate::scene::terrain::fields::TerrainGeometry;
use crate::scene::terrain::surface::{Levels, RegionId};
use crate::world::state::RenderWorld;

/// When finished terrain updates reach the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerrainSync {
    /// The frame shows the newest finished update; building never blocks it (the game).
    Background,
    /// Every frame waits for the update of the world it draws: each frame is a pure
    /// function of its world (screenshots, tests, showcases).
    #[default]
    Immediate,
}

/// A vertex buffer and its element count.
struct Vertices {
    buffer: GpuBuffer,
    count: u32,
}

/// Ground of one piece: indexed vertices and a bounding sphere.
pub struct Land {
    pub vertex_buf: GpuBuffer,
    pub index_buf: GpuBuffer,
    pub index_count: u32,
    pub center: [f32; 3],
    pub radius: f32,
}

#[derive(Default)]
struct PieceGpu {
    land: Option<Land>,
    water: Option<Vertices>,
    grid: Option<Vertices>,
    /// Bounding sphere of everything the piece draws.
    center: [f32; 3],
    radius: f32,
}

/// The geometry on screen and the world snapshot it was built from.
pub struct Published {
    pub revision: u64,
    pub levels: Levels,
    pub world: Arc<RenderWorld>,
    pub geometry: Arc<TerrainGeometry>,
    pub heightfield: Arc<Heightfield>,
}

/// What the terrain step of one frame did.
#[derive(Default)]
pub struct TerrainStep {
    /// Bytes uploaded.
    pub bytes: u64,
    /// Where the ground changed, when an update was applied.
    pub changed: Option<Changed>,
    pub updates: u32,
    pub regions_rebuilt: usize,
    pub pieces_uploaded: usize,
    /// Worker time of the applied updates.
    pub build_ms: f32,
    /// Feature chunks the applied updates prebuilt, in order.
    pub features: Vec<crate::gpu::feature_chunks::Prebuilt>,
}

/// What a snapshot is built for: the world's terrain revision at detail levels.
#[derive(Debug, Clone, PartialEq)]
struct Request {
    revision: u64,
    levels: Levels,
}

/// Bytes of pieces uploaded per frame in the background before an update is published.
/// A frame uploads at least one piece.
const UPLOAD_BYTES: u64 = 12 << 20;

/// An update whose pieces are being uploaded: drawn once all are.
struct Staged {
    update: TerrainUpdate,
    uploaded: Vec<(Piece, PieceGpu)>,
}

/// Regions whose detail level changes per job in the background: a zoom moves the
/// levels of the regions nearest the view first, a few at a time, so no publish
/// uploads the whole map.
const LEVEL_STEPS: usize = 6;

pub struct Terrain {
    worker: TerrainWorker,
    pub sync: TerrainSync,
    /// The last submitted request until its update arrives.
    pending: Option<Request>,
    published: Option<Published>,
    staged: Option<Staged>,
    pieces: BTreeMap<Piece, PieceGpu>,
    /// Bumped by every applied update: the key of products standing on the ground.
    pub generation: u64,
    /// Lowest and highest ground.
    pub ground_y: (f32, f32),
}

impl Terrain {
    pub fn new(style: CompiledTerrainStyle) -> Self {
        Self {
            worker: TerrainWorker::spawn(style),
            sync: TerrainSync::default(),
            pending: None,
            published: None,
            staged: None,
            pieces: BTreeMap::new(),
            generation: 0,
            ground_y: (0.0, 0.0),
        }
    }

    /// Submits the world when its terrain or the detail levels moved past what is built
    /// or being built, and applies every finished update. `desired`: the level each
    /// region in view should have, nearest first.
    pub fn step(&mut self, core: &GpuCore, world: &RenderWorld, desired: &[(RegionId, u8)]) -> TerrainStep {
        let request = Request { revision: world.layers.terrain, levels: self.next_levels(desired) };
        let published = self.published.as_ref().map(|p| Request { revision: p.revision, levels: p.levels.clone() });
        if published.as_ref() != Some(&request) && self.pending.as_ref() != Some(&request) {
            self.worker.submit(request.revision, Arc::new(world.clone()), request.levels.clone());
            self.pending = Some(request);
        }
        let mut step = TerrainStep::default();
        loop {
            if self.staged.is_none() {
                let Some(update) = self.next_update(world) else { break };
                self.staged = Some(Staged { update, uploaded: Vec::new() });
            }
            let allowance = self.upload_allowance().saturating_sub(step.bytes);
            let staged = self.staged.as_mut().expect("staged above");
            if !upload_some(core, staged, allowance, &mut step) {
                break;
            }
            let staged = self.staged.take().expect("staged above");
            self.publish(staged, &mut step);
        }
        step
    }

    /// Bytes a frame uploads of the staged update: all of them when it must be on
    /// screen this frame (immediate updates, the first geometry, a new map shape).
    fn upload_allowance(&self) -> u64 {
        let shape = |w: &RenderWorld| (w.width, w.height, w.visual_surface);
        let staged = self.staged.as_ref().map(|s| shape(&s.update.world));
        let foreign = self.published.as_ref().is_none_or(|p| Some(shape(&p.world)) != staged);
        if self.sync == TerrainSync::Immediate || foreign {
            u64::MAX
        } else {
            UPLOAD_BYTES
        }
    }

    /// The levels of the next request: those being built, moved towards `desired` by a
    /// few regions once nothing is building (all at once when updates are immediate).
    fn next_levels(&self, desired: &[(RegionId, u8)]) -> Levels {
        let building = self.pending.as_ref().map(|r| &r.levels).or(self.published.as_ref().map(|p| &p.levels));
        let mut levels = building.cloned().unwrap_or_default();
        if self.pending.is_some() && self.sync == TerrainSync::Background {
            return levels;
        }
        let steps = if self.sync == TerrainSync::Immediate { usize::MAX } else { LEVEL_STEPS };
        // Finer requests first: a region seen closer than it is built looks wrong.
        let mut moves: Vec<(RegionId, u8)> = desired.iter().copied().filter(|&(id, level)| levels.of(id) != level).collect();
        moves.sort_by_key(|&(id, level)| level > levels.of(id));
        for (id, level) in moves.into_iter().take(steps) {
            levels.set(id, level);
        }
        levels
    }

    /// A finished update, waiting for it when this frame must not draw without it.
    fn next_update(&mut self, world: &RenderWorld) -> Option<TerrainUpdate> {
        self.pending.as_ref()?;
        let shape = |w: &RenderWorld| (w.width, w.height, w.visual_surface);
        let foreign = self.published.as_ref().is_none_or(|p| shape(&p.world) != shape(world));
        if self.sync == TerrainSync::Immediate || foreign {
            Some(self.worker.wait())
        } else {
            self.worker.poll()
        }
    }

    /// Replaces the pieces of a fully uploaded update and puts it on screen.
    fn publish(&mut self, staged: Staged, step: &mut TerrainStep) {
        let Staged { update, uploaded } = staged;
        if self.pending.as_ref().is_some_and(|p| update.revision == p.revision && update.levels == p.levels) {
            self.pending = None;
        }
        for piece in &update.removed {
            self.pieces.remove(piece);
        }
        self.pieces.extend(uploaded);
        step.changed = Some(match step.changed.take() {
            Some(earlier) => earlier.merge(update.changed),
            None => update.changed,
        });
        step.updates += 1;
        step.features.push(update.features);
        step.regions_rebuilt += update.regions_rebuilt;
        step.build_ms += update.build_ms;
        self.ground_y = update.terrain_y;
        self.generation += 1;
        self.published = Some(Published {
            revision: update.revision,
            levels: update.levels,
            world: update.world,
            geometry: update.geometry,
            heightfield: update.heightfield,
        });
    }

    /// Any request is being built (new terrain or new detail levels).
    pub fn building(&self) -> bool {
        self.pending.is_some()
    }

    /// A terrain newer than the published one is being built (not only new levels).
    pub fn pending(&self) -> bool {
        let published = self.published.as_ref().map(|p| p.revision);
        self.pending.as_ref().is_some_and(|r| Some(r.revision) != published)
    }

    /// Levels of the published regions.
    pub fn levels(&self) -> Levels {
        self.published.as_ref().map(|p| p.levels.clone()).unwrap_or_default()
    }

    pub fn published(&self) -> Option<&Published> {
        self.published.as_ref()
    }

    pub fn geometry(&self) -> Option<&TerrainGeometry> {
        self.published.as_ref().map(|p| p.geometry.as_ref())
    }

    pub fn heightfield(&self) -> Option<&Heightfield> {
        self.published.as_ref().map(|p| p.heightfield.as_ref())
    }

    /// Ground of every piece with its bounding sphere.
    pub fn lands(&self) -> impl Iterator<Item = &Land> {
        self.pieces.values().filter_map(|p| p.land.as_ref())
    }

    /// Ground of the pieces in the view: (vertices, indices, index count).
    pub fn visible_lands<'a>(&'a self, planes: &'a [glam::Vec4; 6]) -> impl Iterator<Item = &'a Land> {
        self.lands().filter(|land| CameraController::sphere_visible(planes, land.center, land.radius))
    }

    /// Water of the pieces in the view: (buffer, vertex count).
    pub fn visible_water<'a>(&'a self, planes: &'a [glam::Vec4; 6]) -> impl Iterator<Item = (&'a wgpu::Buffer, u32)> {
        self.visible(planes).filter_map(|p| p.water.as_ref().map(|w| (&*w.buffer, w.count)))
    }

    /// Grid lines of the pieces in the view: (buffer, vertex count).
    pub fn visible_grid<'a>(&'a self, planes: &'a [glam::Vec4; 6]) -> impl Iterator<Item = (&'a wgpu::Buffer, u32)> {
        self.visible(planes).filter_map(|p| p.grid.as_ref().map(|g| (&*g.buffer, g.count)))
    }

    fn visible<'a>(&'a self, planes: &'a [glam::Vec4; 6]) -> impl Iterator<Item = &'a PieceGpu> {
        self.pieces.values().filter(|p| CameraController::sphere_visible(planes, p.center, p.radius))
    }

    pub fn land_count(&self) -> usize {
        self.lands().count()
    }

    pub fn water_vertices(&self) -> u32 {
        self.pieces.values().filter_map(|p| p.water.as_ref()).map(|w| w.count).sum()
    }

    pub fn grid_vertices(&self) -> u32 {
        self.pieces.values().filter_map(|p| p.grid.as_ref()).map(|g| g.count).sum()
    }
}

/// Uploads pieces of a staged update until `allowance` bytes are spent (at least one
/// piece); returns whether all of its pieces are up.
fn upload_some(core: &GpuCore, staged: &mut Staged, allowance: u64, step: &mut TerrainStep) -> bool {
    let sizes: Vec<u64> = staged.update.pieces.iter().map(|(_, meshes)| meshes_bytes(meshes)).collect();
    let count = pieces_within(&sizes, allowance);
    for (piece, meshes) in staged.update.pieces.drain(..count) {
        staged.uploaded.push((piece, upload(core, meshes)));
    }
    step.bytes += sizes[..count].iter().sum::<u64>();
    step.pieces_uploaded += count;
    staged.update.pieces.is_empty()
}

/// How many of the pieces (of these sizes, in order) one frame uploads: until the
/// allowance is reached, and at least one.
fn pieces_within(sizes: &[u64], allowance: u64) -> usize {
    let mut spent = 0u64;
    for (i, size) in sizes.iter().enumerate() {
        spent = spent.saturating_add(*size);
        if spent >= allowance {
            return i + 1;
        }
    }
    sizes.len()
}

/// Screen pixels per hex unit below which a region is drawn one level coarser
/// (platform/MESHING.md §2.5, LOD): level 1 below the first, level 2 below the second.
const LEVEL_PX: [f32; 2] = [14.0, 7.0];

/// The camera as the level choice needs it.
pub struct DetailView {
    pub planes: [glam::Vec4; 6],
    pub eye: [f32; 3],
    /// Distance from the eye to the view centre.
    pub focus: f32,
    /// Screen pixels per hex unit at the view centre.
    pub px_per_unit: f32,
}

/// The level every region in view should have, nearest first: coarser where a hex
/// shrinks on screen, with the hysteresis of `select_detail` so a resting camera does
/// not flicker between levels. Regions out of view keep theirs.
pub fn detail_levels(view: &DetailView, width: u32, height: u32, current: &Levels) -> Vec<(RegionId, u8)> {
    use crate::scene::terrain::surface::REGION;
    let mut out: Vec<(f32, RegionId, u8)> = Vec::new();
    for y in 0..height.div_ceil(REGION) {
        for x in 0..width.div_ceil(REGION) {
            let id = RegionId { x, y };
            let (q, r) = ([x * REGION, ((x + 1) * REGION).min(width) - 1], [y * REGION, ((y + 1) * REGION).min(height) - 1]);
            // The cells form a sheared block: its centre and its farthest corner.
            let corners = [(q[0], r[0]), (q[1], r[0]), (q[0], r[1]), (q[1], r[1])].map(|(q, r)| crate::scene::layout::tile_to_world(q, r));
            let center = [corners.iter().map(|c| c.0).sum::<f32>() * 0.25, 0.5, corners.iter().map(|c| c.1).sum::<f32>() * 0.25];
            let radius = corners.iter().map(|c| (c.0 - center[0]).hypot(c.1 - center[2])).fold(0.0, f32::max) + 3.0;
            if !CameraController::sphere_visible(&view.planes, center, radius) {
                continue;
            }
            let distance = (0..3).map(|k| (center[k] - view.eye[k]).powi(2)).sum::<f32>().sqrt();
            let px = view.px_per_unit * (view.focus / distance.max(1e-3)).min(2.0);
            let level = crate::unit_models::select_detail(1.0, LEVEL_PX, px, current.of(id) as usize) as u8;
            out.push((distance, id, level));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    out.into_iter().map(|(_, id, level)| (id, level)).collect()
}

fn meshes_bytes(meshes: &PieceMeshes) -> u64 {
    let land = meshes.land.as_ref().map_or(0, |l| std::mem::size_of_val(l.vertices.as_slice()) + std::mem::size_of_val(l.indices.as_slice()));
    (land + std::mem::size_of_val(meshes.water.as_slice()) + std::mem::size_of_val(meshes.grid.as_slice())) as u64
}

fn upload(core: &GpuCore, meshes: PieceMeshes) -> PieceGpu {
    let land = meshes.land.filter(|l| !l.vertices.is_empty()).map(|l| upload_land(core, &l));
    let water = (!meshes.water.is_empty()).then(|| Vertices {
        buffer: buffer_with(core, Category::TerrainWater, "piece water", wgpu::BufferUsages::VERTEX, &meshes.water),
        count: meshes.water.len() as u32,
    });
    let grid = (!meshes.grid.is_empty()).then(|| Vertices {
        buffer: buffer_with(core, Category::TerrainGrid, "piece grid", wgpu::BufferUsages::VERTEX, &meshes.grid),
        count: meshes.grid.len() as u32,
    });
    let (center, radius) = sphere(land.as_ref(), &meshes.water, &meshes.grid);
    PieceGpu { land, water, grid, center, radius }
}

fn upload_land(core: &GpuCore, mesh: &ChunkMesh) -> Land {
    Land {
        vertex_buf: buffer_with(core, Category::TerrainVertices, "piece ground vertices", wgpu::BufferUsages::VERTEX, &mesh.vertices),
        index_buf: buffer_with(core, Category::TerrainIndices, "piece ground indices", wgpu::BufferUsages::INDEX, &mesh.indices),
        index_count: mesh.indices.len() as u32,
        center: mesh.center,
        radius: mesh.radius,
    }
}

/// A sphere around the ground sphere and every water and grid point.
fn sphere(land: Option<&Land>, water: &[WaterVertex], grid: &[[f32; 3]]) -> ([f32; 3], f32) {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    let mut add = |p: [f32; 3], r: f32| {
        for k in 0..3 {
            low[k] = low[k].min(p[k] - r);
            high[k] = high[k].max(p[k] + r);
        }
    };
    if let Some(land) = land {
        add(land.center, land.radius);
    }
    for v in water {
        add(v.pos, 0.0);
    }
    for p in grid {
        add(*p, 0.0);
    }
    if low[0] > high[0] {
        return ([0.0; 3], 0.0);
    }
    let center = std::array::from_fn(|k| (low[k] + high[k]) * 0.5);
    let radius = (0..3).map(|k| (high[k] - low[k]).powi(2)).sum::<f32>().sqrt() * 0.5 + 0.1;
    (center, radius)
}

fn buffer_with<T: bytemuck::Pod>(core: &GpuCore, category: Category, label: &str, usage: wgpu::BufferUsages, data: &[T]) -> GpuBuffer {
    let buffer = core.buffer(category, &wgpu::BufferDescriptor {
        label: Some(label),
        size: std::mem::size_of_val(data).max(4) as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    core.queue.write_buffer(&buffer, 0, bytemuck::cast_slice(data));
    buffer
}

#[cfg(test)]
mod upload_tests {
    use super::pieces_within;

    #[test]
    fn a_frame_uploads_up_to_the_allowance_and_at_least_one_piece() {
        assert_eq!(pieces_within(&[], 10), 0);
        assert_eq!(pieces_within(&[4, 4, 4], 10), 3);
        assert_eq!(pieces_within(&[4, 4, 4, 4], 10), 3);
        assert_eq!(pieces_within(&[20, 4], 10), 1);
        assert_eq!(pieces_within(&[20, 4], 0), 1);
        assert_eq!(pieces_within(&[1, 2, 3], u64::MAX), 3);
    }
}
