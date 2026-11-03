//! CPU-side projection. Disposable, rebuildable, never writes to GameState (§31).
//!
//! Per-tile and per-entity data are enough to build GPU geometry and instances without
//! touching AuthWorld. The projection is the observer's view: unknown tiles carry neutral
//! terrain and no infrastructure, hidden units and cities are absent.
//!
//! Each layer has its own revision, bumped only when that layer's content changes. The
//! renderer rebuilds and uploads exactly the GPU data that depends on a changed layer, so a
//! unit move in explored land never touches terrain geometry.

use kernel::ids::{FeatureId, TileId};
use kernel::module::RulesModule;
use kernel::navigation::cost::TerrainKinds;
use kernel::rules::roles::{FeatureRole, Ground};
use kernel::rules::CompiledRuleset;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainCell {
    pub tile: TileId,
    pub terrain: u16,
    pub owner: Option<u8>,
    /// Resource content id; None when the tile has none or is unknown.
    pub resource: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitMarker {
    /// Persistent identity of the unit (`EntityId`): the presentation names the unit it
    /// animates by it, across commands and save/load.
    pub id: u64,
    /// The unit holds a fortified stance (figures crouch, closed ranks).
    pub fortified: bool,
    /// A land unit afloat: it stands on water without hovering (the observer sees the
    /// unit on a tile it sees, `UnitModifiers::embarked_on`). It is drawn as a boat,
    /// never as its soldiers (`UnitModelBook::embarked`).
    pub embarked: bool,
    pub tile: TileId,
    pub owner: u8,
    pub kind: u16,
    /// Health in percent of the full 1..=100 (`figures::visible_figures`: the unit shows
    /// that share of its figures).
    pub hp: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityMarker {
    pub tile: TileId,
    pub owner: u8,
    /// Name and population for screen-space banners (same §28 filter as the marker).
    pub name: String,
    /// None: unknown, the remembered city of a save from before sightings; never a zero.
    pub population: Option<u32>,
    /// Building kinds (ruleset indices, ascending): walls, landmarks, wonders.
    pub buildings: Vec<u16>,
    /// The era of the owner as an index of the city kit's styles (0 the oldest .. 7): the city's
    /// style (`AuthWorld::city_era`).
    pub era: u8,
    /// Content keys of the module's showpieces standing in the city (`city_showpieces`): drawn
    /// like world wonders on its ring. Only a city in sight carries them: a remembered city
    /// keeps what its sighting recorded, and the sighting records buildings only.
    pub showpieces: Vec<String>,
    /// Tiles taken off work (overlay dimming).
    pub unworked: Vec<u32>,
}

impl CityMarker {
    /// The population for a banner: the number, or "?" when it was never recorded.
    pub fn population_label(&self) -> String {
        self.population.map_or_else(|| "?".to_owned(), |p| p.to_string())
    }
}

/// One independently invalidated part of the projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Terrain kinds, rivers, map size, surface mode and the explored set. Drives geometry.
    Terrain,
    /// Improvements and routes.
    Materials,
    /// Forest, resources, natural wonders and unlocked resources: trees, props and icons.
    Features,
    Units,
    Cities,
    /// Territory owners.
    Borders,
    /// Observer knowledge: the visible and explored masks.
    Knowledge,
}

/// Revision per layer. A layer revision changes if and only if that layer's content changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayerRevisions {
    pub terrain: u64,
    pub materials: u64,
    pub features: u64,
    pub units: u64,
    pub cities: u64,
    pub borders: u64,
    pub knowledge: u64,
}

impl LayerRevisions {
    /// Every layer at revision 1: a freshly built projection.
    pub const INITIAL: Self = Self {
        terrain: 1,
        materials: 1,
        features: 1,
        units: 1,
        cities: 1,
        borders: 1,
        knowledge: 1,
    };

    /// The revisions of a world built now: every layer one past a base of the world's own
    /// (the process counts its worlds), so no world ever shares a revision with another.
    /// Everything keyed by revisions — the renderer's products, the minimap's bake, the
    /// interface's route and hover caches — then takes a new match for what it is, and a
    /// new match started over an old one inherits none of them (decision D330).
    pub fn fresh() -> Self {
        static WORLDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let base = WORLDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) << 24;
        let at = |layer: u64| base + layer;
        Self {
            terrain: at(Self::INITIAL.terrain),
            materials: at(Self::INITIAL.materials),
            features: at(Self::INITIAL.features),
            units: at(Self::INITIAL.units),
            cities: at(Self::INITIAL.cities),
            borders: at(Self::INITIAL.borders),
            knowledge: at(Self::INITIAL.knowledge),
        }
    }

    pub fn get(&self, layer: Layer) -> u64 {
        match layer {
            Layer::Terrain => self.terrain,
            Layer::Materials => self.materials,
            Layer::Features => self.features,
            Layer::Units => self.units,
            Layer::Cities => self.cities,
            Layer::Borders => self.borders,
            Layer::Knowledge => self.knowledge,
        }
    }

    pub fn bump(&mut self, layer: Layer) {
        let slot = match layer {
            Layer::Terrain => &mut self.terrain,
            Layer::Materials => &mut self.materials,
            Layer::Features => &mut self.features,
            Layer::Units => &mut self.units,
            Layer::Cities => &mut self.cities,
            Layer::Borders => &mut self.borders,
            Layer::Knowledge => &mut self.knowledge,
        };
        *slot += 1;
    }

    /// Strictly grows with every bump of any layer: a cheap "anything changed" key.
    pub fn total(&self) -> u64 {
        self.terrain
            + self.materials
            + self.features
            + self.units
            + self.cities
            + self.borders
            + self.knowledge
    }
}

/// The features the renderer draws: the feature roles of the rules (`roles.ron`
/// binds each to a feature of the content).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FeatureKind {
    #[default]
    None,
    Forest,
    Jungle,
    Marsh,
    Oasis,
    FloodPlains,
    Ice,
    Atoll,
    Fallout,
    /// A feature of the rules the renderer has no art for.
    Other,
}

impl FeatureKind {
    /// The kind of a feature of the rules: the role it plays, `Other` without one.
    pub fn of<M: RulesModule>(rules: &CompiledRuleset<M>, feature: FeatureId) -> Self {
        match rules.roles.feature_role(feature) {
            Some(FeatureRole::Forest) => Self::Forest,
            Some(FeatureRole::Jungle) => Self::Jungle,
            Some(FeatureRole::Marsh) => Self::Marsh,
            Some(FeatureRole::Oasis) => Self::Oasis,
            Some(FeatureRole::FloodPlains) => Self::FloodPlains,
            Some(FeatureRole::Ice) => Self::Ice,
            Some(FeatureRole::Atoll) => Self::Atoll,
            Some(FeatureRole::Fallout) => Self::Fallout,
            None => Self::Other,
        }
    }

    /// Trees stand on the tile.
    pub fn has_trees(self) -> bool {
        matches!(self, Self::Forest | Self::Jungle)
    }
}

pub const ALL_LAYERS: [Layer; 7] = [
    Layer::Terrain,
    Layer::Materials,
    Layer::Features,
    Layer::Units,
    Layer::Cities,
    Layer::Borders,
    Layer::Knowledge,
];

/// Food, production, gold, science, culture and the rules module's yield of one tile.
pub type TileYields = [i8; 6];

#[derive(Debug, Clone, Default)]
pub struct RenderWorld {
    /// Explicit opt-in for the continuous visual surface.
    pub visual_surface: bool,
    pub width: u32,
    pub height: u32,
    pub terrain: Vec<TerrainCell>,
    /// What each terrain code is (water, hills, mountains): the rules of the
    /// projected game; `TerrainKinds::LEGACY` for hand-built worlds.
    pub terrain_kinds: TerrainKinds,
    /// Units the observer sees (own + visible foreign, on visible tiles; §28 filter).
    pub units: Vec<UnitMarker>,
    /// Cities the observer sees, on explored tiles.
    pub cities: Vec<CityMarker>,
    /// Observer of this projection.
    pub observer: u8,
    /// 1 = tile visible now (counts[observer] > 0).
    pub visible: Vec<u8>,
    /// 1 = tile explored (observer knowledge).
    pub explored: Vec<u8>,
    /// Improvement content id (ruleset index) per tile.
    pub improvements: Vec<Option<u16>>,
    /// The improvement of the tile is pillaged and in sight (the kernel's `pillaged` layer of
    /// the map; a remembered tile reads intact). Hand-built worlds may leave it empty.
    pub pillaged: Vec<bool>,
    /// Route per tile: 0 none, 1 road, 2 railroad (kernel::world::map::Route::code).
    pub road: Vec<u8>,
    /// Natural wonder content id (ruleset index) per tile.
    pub natural_wonders: Vec<Option<u16>>,
    /// Content keys of the ruleset's natural wonders, by id: renderers that
    /// shape or dress a site choose by key (`core:natural-wonder/...`).
    pub natural_wonder_keys: Vec<String>,
    /// 1 = river touches the tile.
    pub river: Vec<u8>,
    /// 1 = trees on the tile (forest or jungle), for the tree props.
    pub forest: Vec<u8>,
    /// Feature on the tile, by kind (surface materials and props). Hand-built
    /// worlds may leave it empty: then `forest` alone describes the tile.
    pub feature: Vec<FeatureKind>,
    /// River beds as canonical (min, max) tile edges, both ends explored.
    pub river_edges: Vec<(u32, u32)>,
    /// Resources unlocked for the observer (content ids).
    pub unlocked_resources: Vec<u16>,
    /// What each tile yields as the observer knows it (`plot_yields`: nature,
    /// revealed resource, improvement, the observer's technologies), in
    /// `TileYields` order. Hand-built worlds may leave it empty.
    pub yields: Vec<TileYields>,
    /// Per-layer revisions; see [`LayerRevisions`].
    pub layers: LayerRevisions,
}

impl RenderWorld {
    /// "Anything changed" key for caches that depend on the whole picture.
    pub fn revision(&self) -> u64 {
        self.layers.total()
    }

    /// Marks a layer changed after a caller edited its fields directly.
    pub fn touch(&mut self, layer: Layer) {
        self.layers.bump(layer);
    }

    /// Marks every layer changed (a hand-built or wholesale replaced world).
    pub fn touch_all(&mut self) {
        for layer in ALL_LAYERS {
            self.layers.bump(layer);
        }
    }

    /// The map code of water (the terrain style keys its height by it).
    pub fn water_code(&self) -> u16 {
        self.terrain_kinds.palette().code(Ground::Water)
    }

    /// Is the cell at `index` water?
    pub fn is_water(&self, index: usize) -> bool {
        self.terrain.get(index).is_some_and(|c| self.terrain_kinds.is_water(c.terrain))
    }

    /// Visible now or explored before: the tile's geography is part of the observer's view.
    pub fn is_known(&self, tile: usize) -> bool {
        self.explored.get(tile) == Some(&1) || self.visible.get(tile) == Some(&1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cache keyed by revisions (the renderer's products, the minimap's bake) must
    /// take a world built after another for a different one, whatever either has been
    /// through: no layer of one world is at a revision a layer of another has been at.
    #[test]
    fn no_two_worlds_share_a_revision_on_any_layer() {
        let (a, b) = (LayerRevisions::fresh(), LayerRevisions::fresh());
        let mut older = a;
        for layer in ALL_LAYERS {
            for _ in 0..1000 {
                older.bump(layer);
            }
        }
        for layer in ALL_LAYERS {
            assert_ne!(a.get(layer), b.get(layer), "{layer:?}");
            assert!(b.get(layer) > older.get(layer), "{layer:?}: a thousand bumps of the older world stay below the newer one");
        }
    }

    #[test]
    fn a_fresh_world_starts_one_past_a_base_on_every_layer() {
        let fresh = LayerRevisions::fresh();
        let base = fresh.terrain - LayerRevisions::INITIAL.terrain;
        for layer in ALL_LAYERS {
            assert_eq!(fresh.get(layer) - LayerRevisions::INITIAL.get(layer), base, "{layer:?}");
        }
    }
}
