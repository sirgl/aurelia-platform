//! World objects as instanced models keyed by content id: improvements, resources and
//! natural wonders on their tiles ("props"), and city tiles from the modular kit with the
//! world wonders on their rings ("cities").
//!
//! Both are built from the observer's knowledge: known tiles show what the projection
//! holds for them, unknown tiles show nothing. Instance fog is the knowledge bound; the
//! shader takes the current visibility from the GPU fog field. Every model is grounded on
//! the final surface (place.rs). Instances are grouped by model: one draw per model and LOD.

pub mod city;
pub mod place;

use std::ops::Range;

use crate::gpu::buffers::{owner_color, HexInstance};
use crate::gpu::cells::{picked_ranges, CellGrid, Run};
use crate::gpu::terrain_job::Changed;
use crate::scene::layout::tile_to_world;
use crate::scene::terrain::fields::{hash_u32, TerrainGeometry};
use crate::unit_models::select_detail;
use crate::world::state::RenderWorld;
use crate::world_models::{self, Ground, WorldBook, WorldModelId};
use place::{seat, Placement, Seated};
use super::world_anim;

/// Neutral tint of objects without an owner (team-masked parts stay light).
const NEUTRAL: [f32; 3] = [0.85, 0.82, 0.74];
pub const FOG_KNOWN: f32 = 1.0;
const FOG_UNKNOWN: f32 = 0.18;
/// City pieces, wonders and props reach at most this many tiles from their tile.
const OBJECT_RING: i32 = 2;

/// The camera as the world-object draws need it.
#[derive(Debug, Clone, Copy)]
pub struct ModelView {
    pub planes: [glam::Vec4; 6],
    /// Screen pixels per hex unit at the view centre.
    pub px_per_unit: f32,
    pub eye: [f32; 3],
    /// Distance from the eye to the view centre.
    pub focus: f32,
}

impl ModelView {
    /// Screen size of a hex unit at `point` relative to the view centre (perspective).
    fn cell_scale(&self, point: [f32; 3]) -> f32 {
        let d = ((point[0] - self.eye[0]).powi(2) + (point[1] - self.eye[1]).powi(2) + (point[2] - self.eye[2]).powi(2))
            .sqrt();
        (self.focus / d.max(1e-3)).min(2.0)
    }
}

/// One draw: a LOD's vertex range times a model group's instance range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDraw {
    pub vertices: Range<u32>,
    pub instances: Range<u32>,
}

impl ModelDraw {
    pub fn triangles(&self) -> usize {
        self.vertices.len() / 3 * self.instances.len()
    }
}

/// A model instance and the culling cell of its tile.
#[derive(Debug, Clone, Copy)]
struct Item {
    model: WorldModelId,
    cell: u32,
    instance: HexInstance,
}

/// One model's instances: a contiguous range, split into runs of one culling cell each.
#[derive(Debug, Clone)]
struct Group {
    model: WorldModelId,
    count: u32,
    /// Range into `Batches::runs`.
    runs: Range<usize>,
}

/// Instances grouped by model and, inside a model, by culling cell; plus the per-model LOD
/// hysteresis state.
#[derive(Debug, Default)]
pub struct Batches {
    groups: Vec<Group>,
    /// Cells ascending inside each group.
    runs: Vec<Run>,
    grid: CellGrid,
    lods: Vec<usize>,
}

impl Batches {
    /// Sorts items by model and cell; returns the instance array to upload.
    fn regroup(&mut self, mut items: Vec<Item>, grid: CellGrid) -> Vec<HexInstance> {
        items.sort_by_key(|item| (item.model, item.cell));
        self.groups.clear();
        self.runs.clear();
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let index = out.len() as u32;
            let same_model = self.groups.last().is_some_and(|g| g.model == item.model);
            if !same_model {
                let at = self.runs.len();
                self.groups.push(Group { model: item.model, count: 0, runs: at..at });
            }
            let group = self.groups.last_mut().expect("group pushed above");
            group.count += 1;
            match self.runs.last_mut() {
                Some(run) if same_model && run.cell == item.cell => run.count += 1,
                _ => {
                    self.runs.push(Run { cell: item.cell, first: index, count: 1 });
                    group.runs.end = self.runs.len();
                }
            }
            out.push(item.instance);
        }
        self.grid = grid;
        if self.lods.len() != world_models::models().len() {
            self.lods = vec![0; world_models::models().len()];
        }
        out
    }

    pub fn instances(&self) -> usize {
        self.groups.iter().map(|g| g.count as usize).sum()
    }

    /// Draws for this frame: instances of cells in the view frustum, each cell at the LOD
    /// its distance from the eye asks for (the view centre keeps the model's hysteresis
    /// state). Neighbouring visible cells of one LOD merge into one draw.
    pub fn draws(&mut self, view: &ModelView) -> Vec<ModelDraw> {
        // Per cell: None outside the frustum, else the pixel scale at that cell.
        let scale: Vec<Option<f32>> = (0..self.grid.len() as u32)
            .map(|cell| {
                let (center, radius) = self.grid.sphere(cell);
                let seen = crate::camera::CameraController::sphere_visible(&view.planes, center, radius);
                seen.then(|| view.px_per_unit * view.cell_scale(center))
            })
            .collect();
        let mut draws = Vec::with_capacity(self.groups.len());
        for group in &self.groups {
            let m = group.model.model();
            let slot = &mut self.lods[group.model.0 as usize];
            *slot = select_detail(m.size, m.lod_px, view.px_per_unit, *slot);
            let current = *slot;
            // (lod, first, count) of the draw being extended.
            let mut open: Option<(usize, u32, u32)> = None;
            for &Run { cell, first, count } in &self.runs[group.runs.clone()] {
                let Some(px) = scale.get(cell as usize).copied().unwrap_or(Some(view.px_per_unit)) else {
                    if let Some((lod, start, n)) = open.take() {
                        draws.push(draw(group.model, lod, start, n));
                    }
                    continue;
                };
                let lod = select_detail(m.size, m.lod_px, px, current);
                open = match open {
                    Some((l, start, n)) if l == lod && start + n == first => Some((l, start, n + count)),
                    Some((l, start, n)) => {
                        draws.push(draw(group.model, l, start, n));
                        Some((lod, first, count))
                    }
                    None => Some((lod, first, count)),
                };
            }
            if let Some((lod, start, n)) = open {
                draws.push(draw(group.model, lod, start, n));
            }
        }
        draws
    }

    /// Culling cells of the instances.
    pub fn grid(&self) -> &CellGrid {
        &self.grid
    }

    /// Shadow casters: every instance of the picked cells at the reduced LOD,
    /// independent of the camera.
    pub fn caster_draws(&self, picked: &[bool]) -> Vec<ModelDraw> {
        let mut draws = Vec::new();
        for group in &self.groups {
            for range in picked_ranges(&self.runs[group.runs.clone()], picked) {
                draws.push(draw(group.model, 1, range.start, range.len() as u32));
            }
        }
        draws
    }

    /// Each model with the range of its instances in the array `regroup` returned.
    pub fn placed(&self) -> impl Iterator<Item = (WorldModelId, Range<usize>)> + '_ {
        self.groups.iter().map(|g| { let first = self.runs[g.runs.start].first as usize; (g.model, first..first + g.count as usize) })
    }

}

fn draw(model: WorldModelId, lod: usize, first: u32, count: u32) -> ModelDraw {
    let (start, vertices) = model.model().lods[lod];
    ModelDraw {
        vertices: start..start + vertices,
        instances: first..first + count,
    }
}

/// What one tile's props depend on: the cache key of `WorldObjects::tile_cache`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct TileKey {
    shown: bool,
    terrain: u16,
    improvement: Option<u16>,
    /// The resource as the observer sees it (None until unlocked).
    resource: Option<u16>,
    wonder: Option<u16>,
    /// The improvement is pillaged and in sight.
    pillaged: bool,
}

/// A city tile's pieces and the inputs they were composed from.
#[derive(Debug)]
struct CityEntry {
    city: crate::world::state::CityMarker,
    water: Vec<u8>,
    items: Vec<Item>,
}

#[derive(Debug, Default)]
pub struct WorldObjects {
    book: WorldBook,
    pub props: Batches,
    pub cities: Batches,
    /// City tiles and wonder sites: tiles where no tile object is drawn.
    pub occupied: Vec<u32>,
    /// Bumped when `occupied` changes; the props product keys on it.
    pub sites_generation: u64,
    /// Per tile: its key and grounded props. Only tiles whose key changed are rebuilt.
    tile_cache: Vec<(TileKey, Vec<Item>)>,
    /// Per city tile: its pieces, recomposed only when the city or its coast changes.
    city_cache: std::collections::HashMap<u32, CityEntry>,
}

impl WorldObjects {
    pub fn set_book(&mut self, book: WorldBook) {
        self.book = book;
        self.tile_cache.clear();
        self.city_cache.clear();
    }

    pub fn has_book(&self) -> bool {
        !self.book.is_empty()
    }

    /// Drops the cached placements on ground that changed: they are grounded again.
    pub fn follow_ground(&mut self, changed: &Changed) {
        let Some(near) = changed.near(OBJECT_RING) else {
            self.tile_cache.clear();
            self.city_cache.clear();
            return;
        };
        let touched = |tile: u32| near.get(tile as usize).copied().unwrap_or(true);
        for (i, entry) in self.tile_cache.iter_mut().enumerate() {
            if touched(i as u32) {
                *entry = (TileKey::default(), Vec::new());
            }
        }
        self.city_cache.retain(|tile, _| !touched(*tile));
    }

    /// Where the props emit particles (`ambient_fx`): natural wonders with a vent or a fountain,
    /// fires and craters by material, and every pillaged work.
    pub fn emitter_sites(&self) -> Vec<crate::gpu::ambient_fx::Site> {
        use crate::gpu::ambient_fx::{clock_of, emitter_of, Emitter, Site};
        let mut out = Vec::new();
        for (i, (key, items)) in self.tile_cache.iter().enumerate() {
            if !key.shown {
                continue;
            }
            let tile = i as u32;
            for item in items {
                let ruined = (item.instance.motion[0] as u32 & 16) != 0;
                let mut site = |emitter: Emitter, at: [f32; 3]| {
                    let (period, phase) = clock_of(emitter, item.instance.motion, tile);
                    out.push(Site { tile, emitter, pos: item.instance.transform(at), period, phase });
                };
                match emitter_of(item.model) {
                    Some((Emitter::Hearth, _)) if ruined => {}
                    Some((emitter, at)) => site(emitter, at),
                    None => {}
                }
                if ruined {
                    site(Emitter::Ruin, [0.0, 0.1, 0.0]);
                }
            }
        }
        out
    }

    /// City tiles and the tiles of world wonders as of the last build (sorted).
    pub fn occupied_tiles(&self) -> &[u32] {
        &self.occupied
    }

    /// City pieces and world wonders; refreshes the occupied tiles first.
    pub fn build_cities(&mut self, world: &RenderWorld, geom: &TerrainGeometry) -> Vec<HexInstance> {
        let grid = CellGrid::of(world);
        let sites = city::wonder_sites(world, &self.book, &|tile| city::site_relief(geom, tile_center(world, tile)));
        let mut occupied: Vec<u32> = world.cities.iter().map(|c| c.tile.0).collect();
        occupied.extend(sites.iter().map(|s| s.tile));
        occupied.sort_unstable();
        if occupied != self.occupied {
            self.occupied = occupied;
            self.sites_generation += 1;
        }
        let mut cache = std::mem::take(&mut self.city_cache);
        let mut items = Vec::new();
        for c in &world.cities {
            let water = city::water_dirs(world, c.tile.0);
            let fresh = cache.get(&c.tile.0).is_some_and(|e| e.city == *c && e.water == water);
            if !fresh {
                let pieces = self.city_items(world, geom, &grid, c, &water);
                cache.insert(c.tile.0, CityEntry { city: c.clone(), water, items: pieces });
            }
            items.extend_from_slice(&cache[&c.tile.0].items);
        }
        cache.retain(|tile, _| world.cities.iter().any(|c| c.tile.0 == *tile));
        self.city_cache = cache;
        for site in sites {
            let (mut x, mut z) = tile_center(world, site.tile);
            if !site.model.model().water_site {
                let (cx, cz) = tile_center(world, site.city);
                let d = (cx - x).hypot(cz - z).max(1e-3);
                x += (cx - x) / d * city::WONDER_PULL;
                z += (cz - z) / d * city::WONDER_PULL;
            }
            let fog = known_fog(world, site.tile);
            let at = Placement { x, z, yaw: 0.0, color: owner_color(site.owner), fog };
            let scale = city::wonder_scale(site.model);
            if let Some(seated) = seat(geom, site.model, at, [scale, scale]) {
                let anim = world_anim::instance_anim(site.model, site.tile, 0, false);
                push_seated(&mut items, site.model, grid.cell(site.tile), seated, anim);
            }
        }
        self.cities.regroup(items, grid)
    }

    fn city_items(
        &self,
        world: &RenderWorld,
        geom: &TerrainGeometry,
        grid: &CellGrid,
        c: &crate::world::state::CityMarker,
        water: &[u8],
    ) -> Vec<Item> {
        let (cx, cz) = tile_center(world, c.tile.0);
        let color = owner_color(c.owner);
        let fog = known_fog(world, c.tile.0);
        let mut items = Vec::new();
        let quality = city::slot_quality(geom, cx, cz);
        for (n, piece) in city::compose_fit(c, &self.book, water, c.tile.0, &quality).into_iter().enumerate() {
            let at = Placement { x: cx + piece.x, z: cz + piece.z, yaw: piece.yaw.to_radians(), color, fog };
            if let Some(seated) = seat(geom, piece.model, at, [1.0, 1.0]) {
                let anim = world_anim::instance_anim(piece.model, c.tile.0, 2 + n as u32, false);
                push_seated(&mut items, piece.model, grid.cell(c.tile.0), seated, anim);
            }
        }
        items
    }

    /// Improvements, resources and natural wonders of known tiles. Only tiles whose
    /// inputs changed since the last build are grounded again.
    pub fn build_props(&mut self, world: &RenderWorld, geom: &TerrainGeometry) -> Vec<HexInstance> {
        let grid = CellGrid::of(world);
        if self.tile_cache.len() != world.terrain.len() {
            self.tile_cache = vec![(TileKey::default(), Vec::new()); world.terrain.len()];
        }
        let mut items = Vec::new();
        for i in 0..world.terrain.len() {
            let key = self.tile_key(world, i);
            if self.tile_cache[i].0 != key {
                let mut fresh = Vec::new();
                if key.shown {
                    for (n, (model, at)) in self.tile_objects(world, i, key).into_iter().enumerate() {
                        if let Some(seated) = seat(geom, model, at, [1.0, 1.0]) {
                            // Only the improvement stands ruined; a resource or a wonder under it does not.
                            let ruined = key.pillaged && model.model().kind == world_models::WorldKind::Improvement;
                            let anim = world_anim::instance_anim(model, i as u32, n as u32, ruined);
                            push_seated(&mut fresh, model, grid.cell(i as u32), seated, anim);
                        }
                    }
                }
                self.tile_cache[i] = (key, fresh);
            }
            items.extend_from_slice(&self.tile_cache[i].1);
        }
        self.props.regroup(items, grid)
    }

    pub fn tile_centre(world: &RenderWorld, tile: u32) -> (f32, f32) {
        tile_center(world, tile)
    }

    fn tile_key(&self, world: &RenderWorld, i: usize) -> TileKey {
        let cell = &world.terrain[i];
        let shown = world.is_known(i) && self.occupied.binary_search(&(i as u32)).is_err();
        TileKey {
            shown,
            terrain: cell.terrain,
            improvement: world.improvements.get(i).copied().flatten(),
            resource: cell.resource.filter(|r| world.unlocked_resources.contains(r)),
            wonder: world.natural_wonders.get(i).copied().flatten(),
            pillaged: world.pillaged.get(i).copied().unwrap_or(false),
        }
    }

    /// Models standing on one known tile, with their placements.
    fn tile_objects(&self, world: &RenderWorld, i: usize, key: TileKey) -> Vec<(WorldModelId, Placement)> {
        let tile = i as u32;
        let (x, z) = tile_center(world, tile);
        let base = Placement { x, z, yaw: 0.0, color: NEUTRAL, fog: FOG_KNOWN };
        let mut out = Vec::new();
        if let Some(model) = key.wonder.and_then(|w| self.book.natural_wonder(w)) {
            out.push((model, base));
            return out;
        }
        if let Some(model) = key.improvement.and_then(|imp| self.book.improvement(imp, key.terrain)) {
            out.push((model, Placement { yaw: object_yaw(model, tile, 0), ..base }));
        }
        if let Some(model) = key.resource.and_then(|r| self.book.resource(r)) {
            out.push((model, Placement { yaw: object_yaw(model, tile, 1), ..base }));
        }
        out
    }
}

/// A seated model and the support under it, all in the culling cell of the tile. `anim` is the
/// motion of the model (`world_anim::instance_anim`); the support stands still.
fn push_seated(items: &mut Vec<Item>, model: WorldModelId, cell: u32, mut seated: Seated, anim: [f32; 4]) {
    seated.main.motion = anim;
    items.push(Item { model, cell, instance: seated.main });
    for (support, instance) in seated.support {
        items.push(Item { model: support, cell, instance });
    }
}

/// Deterministic yaw per tile: fields turn by hex sixths, buildings and props sway a little
/// around facing the camera, water objects turn freely.
fn object_yaw(model: WorldModelId, tile: u32, salt: u32) -> f32 {
    let h = hash_u32(tile.wrapping_mul(0x2545_F491) ^ salt.wrapping_mul(0x9E37_79B9));
    let degrees = match model.model().ground {
        Ground::Plane => (h % 6) as f32 * 60.0,
        Ground::Water => (h % 360) as f32,
        Ground::Upright => (h % 41) as f32 - 20.0,
    };
    degrees.to_radians()
}

pub fn tile_center(world: &RenderWorld, tile: u32) -> (f32, f32) {
    let width = world.width.max(1);
    tile_to_world(tile % width, tile / width)
}

/// Knowledge bound of the instance fog: known tiles 1.0, the rest unknown.
fn known_fog(world: &RenderWorld, tile: u32) -> f32 {
    if world.is_known(tile as usize) {
        FOG_KNOWN
    } else {
        FOG_UNKNOWN
    }
}

