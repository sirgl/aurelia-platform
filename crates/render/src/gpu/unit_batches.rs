//! Units -> instance batches per model, frustum and shadow culling, and the per-frame
//! level of detail.
//!
//! One `UnitMarker` (one UnitId) becomes the figures of its model's formation. Inside a
//! model's batch the leader figures (slot 0) come first, so the far marker draws only
//! them: one marker per unit. Leaders and followers are each stored culling cell by
//! culling cell (`cells.rs`): the scene draws the cells in the camera frustum, the
//! shadow pass the cells in the shadow box. The model comes from the unit's content key
//! through the `UnitModelBook`, never from the numeric kind. A land unit afloat
//! (`UnitMarker::embarked`) is drawn as the book's boat, not as its soldiers.
//!
//! A rebuild is incremental: the figures of a unit depend only on its tile, kind, owner,
//! place in the tile's stack, fog and the surface under it, so they are reused while
//! those stay the same, and only the instances that differ from the last upload are
//! written again.

use std::collections::HashMap;
use std::ops::Range;

use crate::gpu::buffers::{owner_color, HexInstance};
use crate::gpu::cells::{picked_ranges, push_runs, CellGrid, Run};
use crate::gpu::terrain_job::Changed;
use crate::scene::fog::fog_at;
use crate::scene::layout::tile_to_world;
use crate::scene::style::CompiledTerrainStyle;
use crate::scene::terrain::fields::TerrainGeometry;
use crate::unit_models::{self, select_lod, Contact, UnitModelBook, UnitModelId, MARKER_LOD};
use crate::world::state::RenderWorld;

/// A second and third unit on one tile (a civilian under escort) step aside.
const STACK_OFFSETS: [[f32; 2]; 3] = [[0.0, 0.0], [0.30, 0.26], [-0.30, 0.26]];
/// Feet sit just above the surface so they never z-fight with it.
const GROUND_LIFT: f32 = 0.015;
/// Height of a carrier's flight deck above the water (aircraft parked on a ship).
const DECK_HEIGHT: f32 = 0.17;
/// Shadow casters use the reduced model.
const CASTER_LOD: usize = 1;
/// Figures stand on their tile or, stacked and in formation, at most this many tiles
/// away from its centre.
const FIGURE_RING: i32 = 1;

/// Runs of one model: leaders, then followers (ranges into `UnitBatches::runs`; the
/// followers' runs directly follow the leaders').
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ModelRuns {
    leaders: Range<usize>,
    followers: Range<usize>,
}

impl ModelRuns {
    /// Runs of every figure: picked neighbours merge across the two tiers.
    fn all(&self) -> Range<usize> {
        match (self.leaders.is_empty(), self.followers.is_empty()) {
            (true, _) => self.followers.clone(),
            (false, true) => self.leaders.clone(),
            (false, false) => self.leaders.start..self.followers.end,
        }
    }
}

/// One draw: a LOD's vertex range times a batch's instance range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitDraw {
    pub vertices: Range<u32>,
    pub instances: Range<u32>,
}

impl UnitDraw {
    pub fn triangles(&self) -> usize {
        self.vertices.len() / 3 * self.instances.len()
    }
}

/// Everything a unit's figures depend on, besides the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FigureKey {
    tile: u32,
    kind: u16,
    owner: u8,
    /// Place in the tile's stack (0 for the first unit on the tile).
    stack: u8,
    fog: u32,
    fortified: bool,
    /// Afloat: the unit is drawn as the boat of the book, not its soldiers.
    embarked: bool,
    /// The presentation draws the unit this frame (`unit_poses`): its resting figures
    /// keep their slots in the array but collapse to nothing.
    posed: bool,
    /// Persistent id of the unit: which of its figures stand at its health is its own
    /// (`figures::stands`).
    id: u64,
    /// Figures standing at the unit's health.
    shown: u8,
}

/// A unit's formation: slot 0 is the leader.
#[derive(Debug, Clone)]
struct Figures {
    model: UnitModelId,
    instances: Vec<HexInstance>,
}

#[derive(Debug)]
pub struct UnitBatches {
    book: UnitModelBook,
    grid: CellGrid,
    /// Per model id.
    models: Vec<ModelRuns>,
    runs: Vec<Run>,
    /// Current LOD per model: the hysteresis state of `select_lod`.
    lods: Vec<usize>,
    figures: HashMap<FigureKey, Figures>,
    /// The instances of the last build, as uploaded.
    instances: Vec<HexInstance>,
    /// Ids of the units the presentation draws (sorted).
    posed: Vec<u64>,
    /// Where each posed unit rests in the projection (as of the last build).
    rests: HashMap<u64, crate::gpu::unit_poses::Rest>,
}

impl Default for UnitBatches {
    fn default() -> Self {
        let count = unit_models::models().len();
        Self {
            book: UnitModelBook::default(),
            grid: CellGrid::default(),
            models: vec![ModelRuns::default(); count],
            runs: Vec::new(),
            lods: vec![0; count],
            figures: HashMap::new(),
            instances: Vec::new(),
            posed: Vec::new(),
            rests: HashMap::new(),
        }
    }
}

/// Where one figure goes in the instance array: sorted by these fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Slot {
    model: u16,
    /// 0 leader, 1 follower.
    tier: u8,
    cell: u32,
    /// Order of the unit in the projection, then of the figure in its formation.
    unit: u32,
    figure: u32,
}

impl UnitBatches {
    pub fn set_book(&mut self, book: UnitModelBook) {
        self.book = book;
        self.figures.clear();
    }

    /// Units the presentation draws this frame (their resting figures collapse). Returns
    /// whether the set changed: the next `build` then rewrites only their figures.
    pub fn set_posed(&mut self, mut ids: Vec<u64>) -> bool {
        ids.sort_unstable();
        ids.dedup();
        if ids == self.posed {
            return false;
        }
        self.posed = ids;
        true
    }

    pub fn book(&self) -> &UnitModelBook {
        &self.book
    }

    /// Resting place of every posed unit the projection shows.
    pub fn rests(&self) -> &HashMap<u64, crate::gpu::unit_poses::Rest> {
        &self.rests
    }

    /// The instances of the last build.
    pub fn instances(&self) -> &[HexInstance] {
        &self.instances
    }

    /// Forgets the figures standing on ground that changed: they are placed again.
    pub fn follow_ground(&mut self, changed: &Changed) {
        let Some(near) = changed.near(FIGURE_RING) else {
            self.figures.clear();
            return;
        };
        self.figures.retain(|key, _| !near.get(key.tile as usize).copied().unwrap_or(true));
    }

    /// Rebuilds the instance array: figures grouped by model, leaders first, each tier
    /// cell by cell, on the surface `geom` (`follow_ground` tells where it changed).
    /// Returns the range of instances that differ from the previous build (None when
    /// nothing changed).
    pub fn build(&mut self, world: &RenderWorld, style: &CompiledTerrainStyle, geom: &TerrainGeometry) -> Option<Range<usize>> {
        let grid = CellGrid::of(world);
        if grid != self.grid {
            self.grid = grid;
        }
        let keys = self.place_units(world, style, geom);
        let instances = self.regroup(&keys);
        let changed = changed_range(&self.instances, &instances);
        self.instances = instances;
        changed
    }

    /// The figure key of every drawn unit, in projection order; builds missing figures.
    fn place_units(&mut self, world: &RenderWorld, style: &CompiledTerrainStyle, geom: &TerrainGeometry) -> Vec<FigureKey> {
        // The HUD shows a garrison badge; figures must not intersect city buildings.
        let mut garrisons: Vec<u32> = world.cities.iter().map(|c| c.tile.0).collect();
        garrisons.sort_unstable();
        let mut previous = std::mem::take(&mut self.figures);
        let mut stacks: HashMap<u32, u8> = HashMap::new();
        let mut keys = Vec::with_capacity(world.units.len());
        self.rests.clear();
        for unit in &world.units {
            if garrisons.binary_search(&unit.tile.0).is_ok() {
                continue;
            }
            let stack = stacks.entry(unit.tile.0).or_insert(0);
            let key = FigureKey {
                tile: unit.tile.0,
                kind: unit.kind,
                owner: unit.owner,
                stack: *stack,
                fog: fog_at(world, unit.tile).to_bits(),
                // A boat does not crouch.
                fortified: unit.fortified && !unit.embarked,
                embarked: unit.embarked,
                posed: self.posed.binary_search(&unit.id).is_ok(),
                id: unit.id,
                shown: {
                    let total = self.book.model_for_unit(unit.kind, unit.embarked).model().slots.len();
                    crate::gpu::figures::visible_figures(total, unit.hp) as u8
                },
            };
            if key.posed {
                let rest = crate::gpu::unit_poses::Rest { stack: key.stack, fortified: key.fortified };
                self.rests.insert(unit.id, rest);
            }
            *stack = stack.saturating_add(1);
            let figures = match previous.remove(&key) {
                Some(figures) => figures,
                None => self.figures_of(world, style, geom, key),
            };
            self.figures.insert(key, figures);
            keys.push(key);
        }
        keys
    }

    fn figures_of(&self, world: &RenderWorld, style: &CompiledTerrainStyle, geom: &TerrainGeometry, key: FigureKey) -> Figures {
        let id = self.book.model_for_unit(key.kind, key.embarked);
        let model = id.model();
        let offset = stack_offset(key.stack);
        let width = world.width.max(1);
        let (cx, cz) = tile_to_world(key.tile % width, key.tile / width);
        let stance = Stance::of(key.fortified);
        let total = model.slots.len();
        let instances = model
            .slots
            .iter()
            .enumerate()
            .filter(|(slot, _)| crate::gpu::figures::rank(key.id, *slot, total) < key.shown as usize)
            .map(|(_, [x, z, s])| {
                let (px, pz) = (cx + offset[0] + x * stance.spread, cz + offset[1] + z * stance.spread);
                let (y, slope) = seat_figure(world, style, geom, key.tile, px, pz, id, *s);
                let scale = match key.posed {
                    true => [0.0, 0.0],
                    false => [*s, *s * stance.height],
                };
                let mut figure = HexInstance::upright([px, y, pz], owner_color(key.owner), scale, f32::from_bits(key.fog));
                figure.slope = slope;
                figure.rig = model.rig_state();
                figure
            })
            .collect();
        Figures { model: id, instances }
    }

    /// Sorts the figures by model, tier and cell into one array and records the runs.
    fn regroup(&mut self, keys: &[FigureKey]) -> Vec<HexInstance> {
        let mut slots = Vec::new();
        for (unit, key) in keys.iter().enumerate() {
            let figures = &self.figures[key];
            let cell = self.grid.cell(key.tile);
            for figure in 0..figures.instances.len() {
                let tier = u8::from(figure > 0);
                slots.push(Slot { model: figures.model.0, tier, cell, unit: unit as u32, figure: figure as u32 });
            }
        }
        slots.sort_unstable();
        for runs in &mut self.models {
            *runs = ModelRuns::default();
        }
        self.runs.clear();
        let mut out = Vec::with_capacity(slots.len());
        let mut start = 0;
        while start < slots.len() {
            let (model, tier) = (slots[start].model, slots[start].tier);
            let end = start + slots[start..].iter().take_while(|s| (s.model, s.tier) == (model, tier)).count();
            let first_run = self.runs.len();
            push_runs(&mut self.runs, slots[start..end].iter().map(|s| s.cell), out.len() as u32);
            let runs = first_run..self.runs.len();
            let entry = &mut self.models[model as usize];
            if tier == 0 {
                entry.leaders = runs;
            } else {
                entry.followers = runs;
            }
            for s in &slots[start..end] {
                out.push(self.figures[&keys[s.unit as usize]].instances[s.figure as usize]);
            }
            start = end;
        }
        out
    }

    /// Culling cells of the instances (scene and shadow passes pick from these).
    pub fn grid(&self) -> &CellGrid {
        &self.grid
    }

    /// Shadow-caster draws: every figure of the picked cells at the reduced LOD,
    /// independent of the camera.
    pub fn caster_draws(&self, picked: &[bool]) -> Vec<UnitDraw> {
        let mut draws = Vec::new();
        for (index, runs) in self.models.iter().enumerate() {
            let (start, count) = UnitModelId(index as u16).model().lods[CASTER_LOD];
            for instances in picked_ranges(&self.runs[runs.all()], picked) {
                draws.push(UnitDraw { vertices: start..start + count, instances });
            }
        }
        draws
    }

    /// Draws for this frame: the figures of the cells in view. `px_per_unit`: screen
    /// pixels per hex unit at the view centre; the far marker draws leaders only.
    pub fn draws(&mut self, px_per_unit: f32, visible: &[bool]) -> Vec<UnitDraw> {
        let mut draws = Vec::new();
        for (index, runs) in self.models.iter().enumerate() {
            if runs.leaders.is_empty() && runs.followers.is_empty() {
                continue;
            }
            let model = UnitModelId(index as u16).model();
            let lod = select_lod(model, px_per_unit, self.lods[index]);
            self.lods[index] = lod;
            let (start, count) = model.lods[lod];
            let figures = if lod == MARKER_LOD { runs.leaders.clone() } else { runs.all() };
            for instances in picked_ranges(&self.runs[figures], visible) {
                draws.push(UnitDraw { vertices: start..start + count, instances });
            }
        }
        draws
    }
}

/// Elements that differ between two instance arrays: everything when the length changed.
fn changed_range(old: &[HexInstance], new: &[HexInstance]) -> Option<Range<usize>> {
    if old.len() != new.len() {
        return Some(0..new.len());
    }
    let same = |i: &usize| bytemuck::bytes_of(&old[*i]) == bytemuck::bytes_of(&new[*i]);
    let first = (0..new.len()).find(|i| !same(i))?;
    let last = (first..new.len()).rev().find(|i| !same(i)).unwrap_or(first);
    Some(first..last + 1)
}

/// How a resting formation stands: fortified figures crouch and close ranks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stance {
    /// Slot offsets scale (closer ranks).
    pub spread: f32,
    /// Figure height scale (crouch).
    pub height: f32,
}

impl Stance {
    pub(crate) fn of(fortified: bool) -> Self {
        match fortified {
            true => Self { spread: 0.8, height: 0.86 },
            false => Self { spread: 1.0, height: 1.0 },
        }
    }
}

/// The figures of one unit of `model` standing on `tile` as the game places them (developer
/// probes: `gallery::underground`). A pure function of the surface.
pub fn probe_figures(
    world: &RenderWorld,
    style: &CompiledTerrainStyle,
    geom: &TerrainGeometry,
    model: UnitModelId,
    tile: u32,
    stack: u8,
    fortified: bool,
) -> Vec<HexInstance> {
    let m = model.model();
    let offset = stack_offset(stack);
    let width = world.width.max(1);
    let (cx, cz) = tile_to_world(tile % width, tile / width);
    let stance = Stance::of(fortified);
    m.slots
        .iter()
        .map(|[x, z, s]| {
            let (px, pz) = (cx + offset[0] + x * stance.spread, cz + offset[1] + z * stance.spread);
            let (y, slope) = seat_figure(world, style, geom, tile, px, pz, model, *s);
            let mut figure = HexInstance::upright([px, y, pz], owner_color(0), [*s, *s * stance.height], 1.0);
            figure.slope = slope;
            figure
        })
        .collect()
}

/// Where a formation of the stack slot `stack` on `tile` stands (its centre).
pub(crate) fn stack_offset(stack: u8) -> [f32; 2] {
    STACK_OFFSETS[(stack as usize).min(STACK_OFFSETS.len() - 1)]
}

/// The steepest ground a figure's feet are sheared onto, and the most its base may hang above the
/// lowest ground under it.
const FEET_SLOPE_MAX: f32 = 1.0;
const FEET_HANG: f32 = 0.10;

/// Where a figure stands: its base height at the anchor and the plane (gradient) its feet follow.
/// A figure on a slope used to be seated at the ground under its centre, so the uphill half of
/// its feet was inside the hill; now the base follows the plane through the ground at four points
/// of its foot circle (`UnitModelId::foot_radius`) and the highest of them is the base. Ships and
/// helicopters keep the single sample.
pub(crate) fn seat_figure(
    world: &RenderWorld,
    style: &CompiledTerrainStyle,
    geom: &TerrainGeometry,
    tile: u32,
    x: f32,
    z: f32,
    model: UnitModelId,
    scale: f32,
) -> (f32, [f32; 2]) {
    let contact = model.model().contact;
    let y = ground_y(world, style, geom, tile, x, z, contact);
    if contact != Contact::Ground || !crate::debug_view::slope_mode_new() {
        return (y, [0.0, 0.0]);
    }
    let r = (model.foot_radius() * scale).max(0.02);
    let at = |dx: f32, dz: f32| crate::scene::terrain::evaluator::sample_surface(geom, x + dx, z + dz).map(|s| s.y);
    let (Some(a), Some(b), Some(c), Some(d)) = (at(r, 0.0), at(-r, 0.0), at(0.0, r), at(0.0, -r)) else {
        return (y, [0.0, 0.0]);
    };
    let mut slope = [(a - b) / (2.0 * r), (c - d) / (2.0 * r)];
    let steep = slope[0].hypot(slope[1]);
    if steep > FEET_SLOPE_MAX {
        slope = [slope[0] * FEET_SLOPE_MAX / steep, slope[1] * FEET_SLOPE_MAX / steep];
    }
    let centre = y - GROUND_LIFT;
    let residuals = [centre, a - slope[0] * r, b + slope[0] * r, c - slope[1] * r, d + slope[1] * r];
    let (lo, hi) = residuals.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), v| (l.min(*v), h.max(*v)));
    // Never below the ground under the anchor: on broken ground the hang limit would drag the feet into the hill.
    (hi.min(lo + FEET_HANG).max(centre) + GROUND_LIFT, slope)
}

pub(crate) fn ground_y(
    world: &RenderWorld,
    style: &CompiledTerrainStyle,
    geom: &TerrainGeometry,
    tile: u32,
    x: f32,
    z: f32,
    contact: Contact,
) -> f32 {
    // A model that stands on ground but on a water tile rides a ship (an aircraft on a
    // carrier): it sits on the deck, above the displayed water, not on the sea floor.
    let on_deck = contact == Contact::Ground && world.is_water(tile as usize);
    let sample = match (&geom.surface, contact) {
        // Ships float on the displayed water surface, everything else stands on ground.
        (Some(surface), Contact::Water) => surface.sample_display(x, z),
        (Some(surface), _) if on_deck => surface.sample_display(x, z),
        _ => crate::scene::terrain::evaluator::sample_surface(geom, x, z),
    };
    let surface = match sample {
        Some(s) => s.y,
        None => world
            .terrain
            .get(tile as usize)
            .and_then(|cell| style.height(cell.terrain))
            .unwrap_or(0.0),
    };
    match contact {
        Contact::Water => surface,
        Contact::Ground | Contact::Hover if on_deck => surface + DECK_HEIGHT,
        Contact::Ground | Contact::Hover => surface + GROUND_LIFT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::heightfield::build_heightfield;
    use crate::scene::style::{compile, default_style};
    use crate::unit_models::model_by_id;
    use crate::world::state::{CityMarker, TerrainCell, UnitMarker};
    use kernel::ids::TileId;

    /// Shared models, which every build has (the content keys that name them are the modules').
    /// The third stands for a multi-figure infantry formation, the fourth for a ship.
    const MODELS: [&str; 4] = ["unit/warrior", "unit/settler", "unit/swordsman", "unit/trireme"];
    /// The boat land units cross the water in.
    const BOAT: &str = "unit/workboat";

    fn model(id: &str) -> UnitModelId {
        model_by_id(id).unwrap_or_else(|| panic!("{id} is not a baked model"))
    }

    fn book() -> UnitModelBook {
        UnitModelBook::from_models(MODELS.map(|id| Some(model(id))))
    }

    fn world_sized(width: u32, height: u32, units: &[(u32, u16)]) -> (RenderWorld, CompiledTerrainStyle, TerrainGeometry) {
        let mut world = RenderWorld {
            width,
            height,
            ..Default::default()
        };
        let n = width * height;
        for tile in 0..n {
            world.terrain.push(TerrainCell {
                tile: TileId(tile),
                terrain: 0,
                owner: None,
                resource: None,
            });
        }
        world.visible = vec![1; n as usize];
        world.explored = vec![1; n as usize];
        for &(tile, kind) in units {
            world.units.push(UnitMarker {
                id: 0, embarked: false, fortified: false, tile: TileId(tile),
                owner: 1,
                kind,
                hp: 100,
            });
        }
        let style = compile(default_style()).unwrap();
        let hf = build_heightfield(&world, &style);
        let geom = crate::scene::terrain::fields::build_terrain_geometry(&world, &style, &hf);
        (world, style, geom)
    }

    fn world_with(units: &[(u32, u16)]) -> (RenderWorld, CompiledTerrainStyle, TerrainGeometry) {
        world_sized(6, 6, units)
    }

    fn batches() -> UnitBatches {
        let mut batches = UnitBatches::default();
        batches.set_book(book());
        batches
    }

    fn all(batches: &UnitBatches) -> Vec<bool> {
        vec![true; batches.grid().len()]
    }

    #[test]
    fn each_kind_draws_its_own_model_from_its_content_key() {
        let (world, style, geom) = world_with(&[(7, 0), (9, 1), (14, 2), (20, 3)]);
        let mut units = batches();
        units.build(&world, &style, &geom);
        let visible = all(&units);
        let draws = units.draws(60.0, &visible);
        assert_eq!(draws.len(), 4, "four kinds in one cell: one draw per model");
        for id in MODELS {
            let model = model(id).model();
            let (start, count) = model.lods[0];
            assert!(
                draws.iter().any(|d| d.vertices == (start..start + count)
                    && d.instances.len() == model.slots.len()),
                "{id} draws its own LOD0 once per formation figure"
            );
        }
    }

    /// The book of `MODELS` with the boat for every land kind (the trireme is a ship).
    fn embarking_book() -> UnitModelBook {
        book().with_embarked(Some(model(BOAT)), [true, true, true, false])
    }

    #[test]
    fn an_embarked_unit_is_drawn_as_the_boat_and_never_as_its_soldiers() {
        let (mut world, style, geom) = world_with(&[(7, 2), (9, 2), (14, 3)]);
        world.units[0].embarked = true;
        let mut units = UnitBatches::default();
        units.set_book(embarking_book());
        units.build(&world, &style, &geom);
        let legion = model(MODELS[2]).model();
        let boat = model(BOAT).model();
        let trireme = model(MODELS[3]).model();
        assert!(legion.slots.len() > 1 && boat.contact == Contact::Water);
        assert_eq!(
            units.instances().len(),
            boat.slots.len() + legion.slots.len() + trireme.slots.len(),
            "the afloat legion is one boat, its neighbour on land keeps its soldiers"
        );
        let visible = all(&units);
        let draws = units.draws(60.0, &visible);
        let drawn = |model: &crate::unit_models::UnitModel| {
            let (start, count) = model.lods[0];
            draws.iter().find(|d| d.vertices == (start..start + count)).map_or(0, |d| d.instances.len())
        };
        assert_eq!(drawn(boat), boat.slots.len(), "the boat is drawn once");
        assert_eq!(drawn(legion), legion.slots.len(), "only the unit on land draws legionaries");
    }

    #[test]
    fn a_boat_sits_on_the_water_surface_in_its_owners_colour() {
        let (mut world, style, geom) = world_with(&[(7, 2)]);
        world.units[0].embarked = true;
        world.units[0].fortified = true;
        let mut units = UnitBatches::default();
        units.set_book(embarking_book());
        units.build(&world, &style, &geom);
        let [boat] = units.instances() else { panic!("one boat, got {:?}", units.instances()) };
        let (x, z) = tile_to_world(7 % 6, 7 / 6);
        let slot = model(BOAT).model().slots[0];
        assert!((boat.offset[0] - x - slot[0]).abs() < 1e-5 && (boat.offset[2] - z - slot[1]).abs() < 1e-5);
        let surface = crate::scene::terrain::evaluator::sample_surface(&geom, boat.offset[0], boat.offset[2]).map_or(0.0, |s| s.y);
        assert!(boat.offset[1] <= surface + 1e-3, "the hull is at the waterline, not lifted: {} over {surface}", boat.offset[1]);
        assert_eq!(boat.color, owner_color(1), "the owner's colour is on the boat");
        assert_eq!(boat.scale_height[1], boat.scale_height[0], "a boat does not crouch while fortified");
    }

    #[test]
    fn without_a_boat_binding_an_embarked_unit_keeps_its_model() {
        let (mut world, style, geom) = world_with(&[(7, 2)]);
        world.units[0].embarked = true;
        let mut units = batches();
        units.build(&world, &style, &geom);
        assert_eq!(units.instances().len(), model(MODELS[2]).model().slots.len());
    }

    #[test]
    fn a_unit_back_on_land_draws_its_soldiers_again() {
        let (mut world, style, geom) = world_with(&[(7, 2)]);
        let mut units = UnitBatches::default();
        units.set_book(embarking_book());
        world.units[0].embarked = true;
        units.build(&world, &style, &geom);
        assert_eq!(units.instances().len(), 1);
        world.units[0].embarked = false;
        units.build(&world, &style, &geom);
        assert_eq!(units.instances().len(), model(MODELS[2]).model().slots.len());
    }

    #[test]
    fn formation_is_one_unit_and_the_marker_draws_its_leader_only() {
        let (world, style, geom) = world_with(&[(7, 0), (21, 0)]);
        let mut units = batches();
        units.build(&world, &style, &geom);
        let instances = units.instances().to_vec();
        let warrior = model(MODELS[0]).model();
        assert_eq!(instances.len(), 2 * warrior.slots.len());
        let visible = all(&units);
        let far = units.draws(0.5, &visible);
        assert_eq!(far.len(), 1);
        assert_eq!(far[0].instances.len(), 2, "one marker per unit");
        let (start, count) = warrior.lods[MARKER_LOD];
        assert_eq!(far[0].vertices, start..start + count);
        // Leaders come first: both markers stand on their own tile's leader slot.
        let leader = warrior.slots[0];
        for (instance, tile) in instances.iter().zip([7u32, 21]) {
            let (x, z) = tile_to_world(tile % 6, tile / 6);
            assert!((instance.offset[0] - x - leader[0]).abs() < 1e-5);
            assert!((instance.offset[2] - z - leader[1]).abs() < 1e-5);
        }
    }

    #[test]
    fn garrison_is_hidden_and_unknown_kinds_draw_the_fallback() {
        let (mut world, style, geom) = world_with(&[(7, 0), (9, 99)]);
        world.cities.push(CityMarker {
            tile: TileId(7),
            owner: 1,
            name: "A".into(),
            population: Some(1),
            buildings: Vec::new(),
            era: 0,
            showpieces: Vec::new(),
            unworked: Vec::new(),
        });
        let mut units = batches();
        units.build(&world, &style, &geom);
        let fallback = UnitModelId::FALLBACK.model();
        assert_eq!(units.instances().len(), fallback.slots.len());
        let visible = all(&units);
        let draws = units.draws(60.0, &visible);
        let (start, count) = fallback.lods[0];
        assert_eq!(
            draws,
            vec![UnitDraw {
                vertices: start..start + count,
                instances: 0..1
            }]
        );
    }

    #[test]
    fn figures_stand_on_the_ground() {
        let (world, style, geom) = world_with(&[(14, 0)]);
        let mut units = batches();
        units.build(&world, &style, &geom);
        for instance in units.instances() {
            let ground = crate::scene::terrain::evaluator::sample_surface(
                &geom,
                instance.offset[0],
                instance.offset[2],
            )
            .unwrap()
            .y;
            assert!((instance.offset[1] - ground - GROUND_LIFT).abs() < 1e-5);
        }
    }

    /// Differential: after any sequence of moves, the incremental build equals a build
    /// from scratch, and the reported range covers every changed instance.
    #[test]
    fn incremental_build_equals_a_fresh_build() {
        let (mut world, style, geom) = world_sized(40, 30, &[]);
        let mut units = batches();
        let mut seed = 7u32;
        let mut next = |n: u32| {
            seed = crate::scene::terrain::fields::hash_u32(seed);
            seed % n
        };
        for step in 0..60 {
            if world.units.len() < 30 || next(4) == 0 {
                world.units.push(UnitMarker { id: 0, embarked: false, fortified: false, tile: TileId(next(1200)), owner: next(3) as u8, kind: next(4) as u16, hp: 100 });
            } else {
                let i = next(world.units.len() as u32) as usize;
                if next(5) == 0 {
                    world.units.remove(i);
                } else {
                    world.units[i].tile = TileId(next(1200));
                }
            }
            let before = units.instances().to_vec();
            let changed = units.build(&world, &style, &geom);
            let mut fresh = batches();
            fresh.build(&world, &style, &geom);
            let bytes = |v: &[HexInstance]| bytemuck::cast_slice::<HexInstance, u8>(v).to_vec();
            assert_eq!(bytes(units.instances()), bytes(fresh.instances()), "step {step}");
            assert_eq!(units.runs, fresh.runs, "step {step}");
            let mut patched = before;
            match changed {
                Some(range) if patched.len() == units.instances().len() => patched[range.clone()].copy_from_slice(&units.instances()[range]),
                Some(_) => patched = units.instances().to_vec(),
                None => {}
            }
            assert_eq!(bytes(&patched), bytes(units.instances()), "range covers the change at step {step}");
        }
    }

    /// Differential: figures kept across terrain updates (`follow_ground`) stand where a
    /// fresh build on the new terrain puts them.
    #[test]
    fn figures_follow_the_ground_a_reveal_changes() {
        use crate::gpu::terrain_job::TerrainBuilder;
        use crate::world::projection::{rebuild_render_world, refresh};
        use kernel::ids::PlayerId;
        let style = compile(default_style()).unwrap();
        let rules = sandbox_content::rules().expect("sandbox rules");
        let config = kernel::scenario::ScenarioConfig { seed: 9, players: 2, width: 48, height: 40, cities: 0, units: 0, water_ratio: 30 };
        let mut game = kernel::scenario::generate(&config, &rules);
        let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
        world.visual_surface = true;
        let mut builder = TerrainBuilder::new(style.clone());
        let mut units = batches();
        for (step, (cq, cr)) in [(10, 9), (14, 12), (20, 16), (27, 22)].into_iter().enumerate() {
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
            // Units on and around the revealed patch, the same every step.
            world.units = (0..40).map(|i| UnitMarker { id: 0, embarked: false, fortified: false, tile: TileId((i * 97 + 31) % (48 * 40)), owner: 0, kind: (i % 4) as u16, hp: 100 }).collect();
            let update = builder.build(step as u64, std::sync::Arc::new(world.clone()), &Default::default());
            units.follow_ground(&update.changed);
            units.build(&world, &style, &update.geometry);
            let mut fresh = batches();
            fresh.build(&world, &style, &update.geometry);
            let bytes = |v: &[HexInstance]| bytemuck::cast_slice::<HexInstance, u8>(v).to_vec();
            assert_eq!(bytes(units.instances()), bytes(fresh.instances()), "step {step}");
        }
    }

    /// The presentation poses one unit: only its figures change (they collapse), the array
    /// keeps its length, and releasing the pose restores the very same instances.
    #[test]
    fn posing_a_unit_collapses_only_its_figures() {
        let (mut world, style, geom) = world_sized(20, 20, &[]);
        world.units = (0..30u32)
            .map(|i| UnitMarker { id: u64::from(i) + 1, embarked: false, fortified: i % 5 == 0, tile: TileId(i * 13 % 400), owner: 0, kind: (i % 4) as u16, hp: 100 })
            .collect();
        let mut units = batches();
        units.build(&world, &style, &geom);
        let before = units.instances().to_vec();
        assert!(units.set_posed(vec![6]));
        assert!(!units.set_posed(vec![6]), "the same set is no change");
        units.build(&world, &style, &geom).expect("its figures collapse");
        assert_eq!(units.instances().len(), before.len());
        let bytes = |v: &HexInstance| bytemuck::bytes_of(v).to_vec();
        let differing: Vec<_> = (0..before.len()).filter(|&i| bytes(&before[i]) != bytes(&units.instances()[i])).collect();
        let slots = units.book().model_for_kind(world.units[5].kind).model().slots.len();
        assert_eq!(differing.len(), slots);
        assert!(differing.iter().all(|&i| units.instances()[i].scale_height == [0.0, 0.0]));
        assert_eq!(units.rests()[&6], crate::gpu::unit_poses::Rest { stack: 0, fortified: true }, "unit 6 rests fortified");
        assert!(units.set_posed(Vec::new()));
        units.build(&world, &style, &geom);
        let all = |v: &[HexInstance]| bytemuck::cast_slice::<HexInstance, u8>(v).to_vec();
        assert_eq!(all(units.instances()), all(&before));
    }

    /// Off-screen units are not drawn, and the shadow pass takes only the cells in its box.
    #[test]
    fn units_outside_the_view_or_the_shadow_box_are_culled() {
        let (world, style, geom) = world_sized(48, 40, &[(0, 0), (47 * 1 + 39 * 48, 0)]);
        let mut units = batches();
        units.build(&world, &style, &geom);
        let warrior = model(MODELS[0]).model();
        let figures = |draws: &[UnitDraw]| draws.iter().map(|d| d.instances.len()).sum::<usize>();
        let visible = all(&units);
        assert_eq!(figures(&units.draws(60.0, &visible)), 2 * warrior.slots.len());
        let (x, z) = tile_to_world(0, 0);
        let near_first = units.grid().in_box([x - 2.0, z - 2.0], [x + 2.0, z + 2.0]);
        assert_eq!(figures(&units.draws(60.0, &near_first)), warrior.slots.len());
        assert_eq!(figures(&units.caster_draws(&near_first)), warrior.slots.len());
        let none = vec![false; units.grid().len()];
        assert!(units.draws(60.0, &none).is_empty());
    }
}
