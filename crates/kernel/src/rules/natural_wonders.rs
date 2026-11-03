//! Where the map generator places a natural wonder and what it makes of the
//! site (classic/docs/classic/natural_wonders.json `placement`). Read by
//! `scenario::landscape`. The catalog's `adjacent_tiles_care_about_*`
//! switches only enable the rules below; the importer folds them in.

use crate::ids::FeatureId;
use crate::rules::content::TerrainKindId;
use crate::rules::source::StableContentKey;

/// How a site is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PlacementMethod {
    /// A land tile checked against the rules (the default).
    #[default]
    Land,
    /// A water tile checked against the rules (Krakatoa: `land_based`
    /// false; the core becomes land).
    Water,
    /// Custom eligibility 1 (Great Barrier Reef): open water near a
    /// coast. One tile here; the reference game places a two-tile reef
    /// (unverified).
    Reef,
    /// Custom eligibility 2 (Rock of Gibraltar): the land tip of a
    /// peninsula. Its custom tile changes are not reproduced (unverified).
    PeninsulaTip,
}

/// What a neighbour of the core is counted as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdjacentKind<Terrain, Feature> {
    Hills,
    Mountain,
    HillsOrMountain,
    /// Any land tile (`anyland`).
    Land,
    /// A water tile (`shallow_water`: the map has one water code).
    Water,
    /// A land tile of this base terrain (desert, grassland, plains, tundra).
    Terrain(Terrain),
    /// A tile with this feature (jungle, marsh, ice).
    Feature(Feature),
}

/// How many neighbours of a kind the core may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdjacentBound {
    /// `adjacent_tiles_require_*` with `required_number_of_adjacent_*`.
    AtLeast(u8),
    /// `adjacent_tiles_avoid_*` with `maximum_allowed_adjacent_*`.
    AtMost(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AdjacentRule<Terrain, Feature> {
    pub kind: AdjacentKind<Terrain, Feature>,
    pub bound: AdjacentBound,
}

/// The relief the core tile takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CoreShape {
    /// Keeps hills or flat land as they are.
    #[default]
    Keep,
    /// `change_core_tile_to_mountain`.
    Mountain,
    /// `change_core_tile_to_flatland`.
    Flat,
}

/// The placement rules of one natural wonder, generic over how terrains
/// and features are named (keys in content, ids once compiled).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Placement<Terrain, Feature> {
    pub method: PlacementMethod,
    /// No ocean next to a land core (`avoid_land_adjacent_to_ocean`).
    pub avoid_ocean_coast: bool,
    /// Not on the largest landmass of the map (`avoid_biggest_landmass`).
    pub avoid_biggest_landmass: bool,
    /// Base terrains the core may have; empty: any
    /// (`core_tile_can_be_any_terrain_type`, `core_tile_can_be_*`).
    pub core_terrains: Vec<Terrain>,
    pub adjacent: Vec<AdjacentRule<Terrain, Feature>>,
    pub core_shape: CoreShape,
    /// The base terrain the core takes (`change_core_tile_terrain_to_*`).
    pub core_terrain: Option<Terrain>,
    /// Neighbours become water (`set_adjacent_tiles_to_shallow_water`).
    pub surround_with_water: bool,
}

impl<Terrain, Feature> Default for Placement<Terrain, Feature> {
    /// Any land tile, no rule, the core kept (the v1 content).
    fn default() -> Self {
        Self {
            method: PlacementMethod::Land,
            avoid_ocean_coast: false,
            avoid_biggest_landmass: false,
            core_terrains: Vec::new(),
            adjacent: Vec::new(),
            core_shape: CoreShape::Keep,
            core_terrain: None,
            surround_with_water: false,
        }
    }
}

/// As written in content.
pub type SourcePlacement = Placement<StableContentKey, StableContentKey>;

/// Compiled: references resolved.
pub type NaturalWonderPlacement = Placement<TerrainKindId, FeatureId>;

/// Most neighbours a hex tile has.
pub const NEIGHBOURS: u8 = 6;
