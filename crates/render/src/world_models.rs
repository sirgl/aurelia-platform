//! World object models baked from the manifest's "world" section by build.rs (build/world.rs):
//! improvements, resources, natural wonders, world wonders and the modular city kit.
//!
//! Objects are drawn from their stable content key, mapped to a model by the manifest's key
//! table and resolved once per ruleset into a `WorldBook`; a key may carry a variant qualifier
//! (`<improvement key>@<terrain key>`) that is tried before the plain key.

use kernel::module::RulesModule;
use crate::gpu::buffers::UnitVertex;
use kernel::rules::CompiledRuleset;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldKind {
    Improvement,
    Resource,
    NaturalWonder,
    Wonder,
    City,
    /// Made by the bake from the footprint of other models: plinth and pile.
    Support,
}

/// How the terrain surface prepares a natural wonder's tile for its model
/// (`scene/terrain/surface/sites.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteShape {
    /// A level pad at the cell's own height.
    Pad,
    /// A bowl with a low rim.
    Crater,
    /// The sea floor lifted close under the surface around the tile.
    Reef,
}

/// How a model meets the final surface (see platform/tools/blender/world/kit.py).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ground {
    /// Rigid, standing at the lowest ground under its footprint.
    Upright,
    /// Sheared onto the ground plane fitted under the footprint.
    Plane,
    /// Floating at the water surface.
    Water,
}

/// How a rigid model is seated on uneven ground (place.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// A building: its high side is buried a little and a stone plinth (two tiers when
    /// tall) fills the gap below its low side.
    Terraced,
    /// Ground cover, resources and prepared sites: centred on the ground, no plinth.
    Bare,
    /// A quay or lighthouse: the land side sets the height, piles reach the sea floor
    /// under the part that stands over water.
    Shore,
}

#[derive(Debug)]
pub struct WorldModel {
    /// Asset id from the manifest ("improvement/farm").
    pub id: &'static str,
    pub kind: WorldKind,
    pub ground: Ground,
    /// Wonders: drawn on a water tile of the city ring instead of a land tile.
    pub water_site: bool,
    /// Natural wonders: the ground the surface shapes under the model.
    pub site_shape: SiteShape,
    /// Vertex range (start, count) per LOD in `vertices()`.
    pub lods: [(u32, u32); 3],
    /// Projected size in pixels below which LOD1 / the far marker is used.
    pub lod_px: [f32; 2],
    /// Max of height and footprint radius (hex units): the size the LOD choice projects.
    pub size: f32,
    /// Footprint radius around the anchor (hex units).
    pub radius: f32,
    /// Lowest and highest point of LOD0.
    pub low: f32,
    pub height: f32,
    pub seat: Seat,
    /// Where the model touches the ground (x, z in model space): the centre of its base and
    /// the farthest-apart points of the base outline (build/support.rs).
    pub contacts: &'static [[f32; 2]],
    /// Centre (x, z) and radius of the circle enclosing `contacts`.
    pub foot: [f32; 3],
}

#[derive(Debug)]
pub struct CityStyle {
    pub name: &'static str,
    pub center: u16,
    pub houses: &'static [u16],
}

/// House or landmark site on a city tile, game coordinates around the anchor.
#[derive(Debug)]
pub struct CitySlot {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub radius: f32,
    /// Outer ring (else inner ring).
    pub outer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Inner,
    Outer,
    /// The free outer slot closest to a water neighbour.
    Water,
    /// Around the whole tile (walls), takes no slot.
    Ring,
}

#[derive(Debug)]
pub struct CityLandmark {
    pub building: &'static str,
    pub model: u16,
    pub slot: SlotKind,
    /// The landmark of this building is hidden when this one is drawn.
    pub supersedes: Option<&'static str>,
}

/// Data of the modular city kit (platform/tools/blender/world/city_kit.py).
#[derive(Debug)]
pub struct CityKit {
    /// Index = era index.
    pub styles: &'static [CityStyle],
    /// (population threshold, houses): the last row whose threshold <= population.
    pub houses_by_population: &'static [(u32, usize)],
    pub older_style_share: u32,
    pub max_landmarks: usize,
    pub slots: &'static [CitySlot],
    /// Sorted by (priority, building key): the drawing order.
    pub landmarks: &'static [CityLandmark],
}

include!(concat!(env!("OUT_DIR"), "/world_models.rs"));

#[cfg(not(target_arch = "wasm32"))]
#[repr(C, align(4))]
struct Aligned<T: ?Sized>(T);

#[cfg(not(target_arch = "wasm32"))]
static VERTEX_BYTES: &Aligned<[u8]> = &Aligned(*include_bytes!(concat!(
    env!("OUT_DIR"),
    "/world_models.bin"
)));

/// The vertex data in the browser: it is not compiled into the wasm (it would be megabytes of it);
/// the page fetches `world_models.bin` and hands it over with `provide` before the renderer starts.
#[cfg(target_arch = "wasm32")]
static PROVIDED: std::sync::OnceLock<&'static [UnitVertex]> = std::sync::OnceLock::new();

/// Browser only: the contents of `world_models.bin` as little-endian words (a `Uint32Array`). Call it
/// once, before the renderer is created. The vector becomes the vertex storage: no copy.
#[cfg(target_arch = "wasm32")]
pub fn provide(words: Vec<u32>) -> Result<(), String> {
    let vertices = crate::model_data::vertices_from_words(words, WORLD_VERTEX_COUNT, "world_models.bin")?;
    PROVIDED.set(vertices).map_err(|_| "world_models.bin was provided twice".to_string())
}

/// Every LOD of every world model, in hex units, flat normals (one GPU buffer).
#[cfg(not(target_arch = "wasm32"))]
pub fn vertices() -> &'static [UnitVertex] {
    bytemuck::cast_slice(&VERTEX_BYTES.0)
}

/// Every LOD of every world model, in hex units, flat normals (one GPU buffer).
#[cfg(target_arch = "wasm32")]
pub fn vertices() -> &'static [UnitVertex] {
    PROVIDED.get().copied().expect("world_models.bin was not provided before the first frame (render::world_models::provide)")
}

pub fn models() -> &'static [WorldModel] {
    &WORLD_MODELS
}
/// Stone plinth of unit radius and height (`WorldModel::foot` is its own footprint).
pub fn plinth() -> WorldModelId {
    WorldModelId(PLINTH_MODEL)
}

/// Wooden pile of unit radius and height.
pub fn pile() -> WorldModelId {
    WorldModelId(PILE_MODEL)
}

/// The parts the wall ring is drawn as, in corner order, the gatehouse last.
pub fn wall_parts() -> impl Iterator<Item = WorldModelId> {
    WALL_PARTS.iter().map(|m| WorldModelId(*m))
}

/// The manifest wall ring model: a ring landmark of this model is drawn as `wall_parts()`.
pub fn is_wall_ring(model: WorldModelId) -> bool {
    WALLS_MODEL == Some(model.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorldModelId(pub u16);

impl WorldModelId {
    pub fn model(self) -> &'static WorldModel {
        &WORLD_MODELS[self.0 as usize]
    }

    pub fn mesh(self, lod: usize) -> &'static [UnitVertex] {
        let (start, count) = self.model().lods[lod];
        &vertices()[start as usize..(start + count) as usize]
    }
}

/// Model for a content key (with or without a qualifier); None when the manifest has none.
pub fn resolve(key: &str) -> Option<WorldModelId> {
    WORLD_KEYS
        .binary_search_by(|(k, _)| (*k).cmp(key))
        .ok()
        .map(|i| WorldModelId(WORLD_KEYS[i].1))
}

/// Number of houses of a city of this population.
pub fn house_count(population: u32) -> usize {
    let mut count = 0;
    for (threshold, houses) in CITY_KIT.houses_by_population {
        if population >= *threshold {
            count = *houses;
        }
    }
    count
}

/// Content of the running ruleset -> world models, built once per ruleset.
/// Indices are the ruleset's table indices (the projection's content ids).
#[derive(Debug, Clone, Default)]
pub struct WorldBook {
    improvements: Vec<Option<WorldModelId>>,
    /// (improvement, map terrain code) -> terrain variant of the improvement.
    improvement_variants: HashMap<(u16, u16), WorldModelId>,
    resources: Vec<Option<WorldModelId>>,
    natural_wonders: Vec<Option<WorldModelId>>,
    /// World wonder model per building kind.
    wonders: Vec<Option<WorldModelId>>,
    /// City kit landmark (index into CITY_KIT.landmarks) per building kind; unique buildings
    /// use the landmark of the building they replace.
    landmarks: Vec<Option<u16>>,
}

impl WorldBook {
    pub fn from_rules<M: RulesModule>(rules: &CompiledRuleset<M>) -> Self {
        let terrains: Vec<(&str, u16)> = rules
            .terrains
            .iter()
            .filter_map(|t| t.map_code.map(|code| (t.key.as_str(), code)))
            .collect();
        let mut improvement_variants = HashMap::new();
        for (i, improvement) in rules.improvements.iter().enumerate() {
            for (terrain, code) in &terrains {
                let key = format!("{}@{terrain}", improvement.key);
                if let Some(model) = resolve(&key) {
                    improvement_variants.insert((i as u16, *code), model);
                }
            }
        }
        let landmark_of = |key: &str| {
            CITY_KIT
                .landmarks
                .iter()
                .position(|l| l.building == key)
                .map(|i| i as u16)
        };
        let landmarks = rules
            .buildings
            .iter()
            .map(|b| {
                let replaced = b.replaces.and_then(|r| rules.buildings.get(r.0 as usize));
                landmark_of(&b.key).or_else(|| replaced.and_then(|r| landmark_of(&r.key)))
            })
            .collect();
        let wonders = rules
            .buildings
            .iter()
            .map(|b| resolve(&b.key).filter(|m| m.model().kind == WorldKind::Wonder))
            .collect();
        Self {
            improvements: rules.improvements.iter().map(|x| resolve(&x.key)).collect(),
            improvement_variants,
            resources: rules.resources.iter().map(|x| resolve(&x.key)).collect(),
            natural_wonders: rules.natural_wonders.iter().map(|x| resolve(&x.key)).collect(),
            wonders,
            landmarks,
        }
    }

    /// A book that draws manifest models as they are, whatever the rules bind (the developer
    /// gallery shows spare models with it). The tables are indexed by the caller; building `k`
    /// of the first `landmark_count` is the city kit's landmark `k`, the buildings above are
    /// the entries of `wonders`.
    pub fn of_models(
        improvements: Vec<Option<WorldModelId>>,
        resources: Vec<Option<WorldModelId>>,
        natural_wonders: Vec<Option<WorldModelId>>,
        wonders: Vec<Option<WorldModelId>>,
        landmark_count: usize,
    ) -> Self {
        Self {
            improvements,
            improvement_variants: HashMap::new(),
            resources,
            natural_wonders,
            wonders,
            landmarks: (0..landmark_count as u16).map(Some).collect(),
        }
    }

    /// The improvement's model on this terrain: the terrain variant, else the plain model.
    pub fn improvement(&self, improvement: u16, terrain: u16) -> Option<WorldModelId> {
        match self.improvement_variants.get(&(improvement, terrain)) {
            Some(model) => Some(*model),
            None => self.improvements.get(improvement as usize).copied().flatten(),
        }
    }

    pub fn resource(&self, resource: u16) -> Option<WorldModelId> {
        self.resources.get(resource as usize).copied().flatten()
    }

    pub fn natural_wonder(&self, wonder: u16) -> Option<WorldModelId> {
        self.natural_wonders.get(wonder as usize).copied().flatten()
    }

    pub fn wonder(&self, building: u16) -> Option<WorldModelId> {
        self.wonders.get(building as usize).copied().flatten()
    }

    /// The world wonder model a module's showpiece key names (a great project built in a
    /// city): the manifest's model of that key when it is a world wonder, else none.
    pub fn showpiece(&self, key: &str) -> Option<WorldModelId> {
        resolve(key).filter(|m| m.model().kind == WorldKind::Wonder)
    }

    pub fn landmark(&self, building: u16) -> Option<&'static CityLandmark> {
        let index = self.landmarks.get(building as usize).copied().flatten()?;
        CITY_KIT.landmarks.get(index as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.improvements.is_empty() && self.resources.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_world_models_are_finite_within_budget_and_lods_shrink() {
        assert_eq!(vertices().len(), WORLD_VERTEX_COUNT);
        for (i, model) in models().iter().enumerate() {
            let id = WorldModelId(i as u16);
            let lods: Vec<usize> = (0..3).map(|l| id.mesh(l).len() / 3).collect();
            assert!(lods[0] >= lods[1] && lods[1] >= lods[2] && lods[2] > 0, "{} {lods:?}", model.id);
            for v in id.mesh(0) {
                assert!(v.pos.iter().chain(&v.normal).all(|x| x.is_finite()), "{}", model.id);
                assert!(v.pos[0].hypot(v.pos[2]) <= model.radius + 1e-3, "{}", model.id);
            }
            match model.ground {
                Ground::Water => assert!(model.low < 0.0 && model.height > 0.0, "{}", model.id),
                Ground::Upright | Ground::Plane => assert!(model.low <= 0.02, "{} floats", model.id),
            }
        }
    }

    #[test]
    fn house_count_grows_with_population() {
        assert_eq!(house_count(0), 0);
        let mut last = 0;
        for pop in 1..40 {
            let n = house_count(pop);
            assert!(n >= last);
            assert!(n <= CITY_KIT.slots.len());
            last = n;
        }
        assert!(last > house_count(1));
    }
}
