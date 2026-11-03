//! Strongly typed identifiers (§3 platform/structure.md).
//!
//! Contract: knows nothing about stores/rules, does no lookups,
//! contains no gameplay logic. Runtime handles != persistent IDs.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct TileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct PlayerId(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitHandle {
    pub index: u32,
    pub generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CityHandle {
    pub index: u32,
    pub generation: u32,
}

/// Persistent identity (save/replay), independent of position in the dense array.
/// Allocated from one world counter in creation order: ascending ids are
/// the canonical order of units and cities (iteration, hash, save).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(pub u64);

// ---- Content IDs (runtime, after ruleset compilation) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitKindId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuildingKindId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TechId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PromotionId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TerrainId(pub u16);

/// An order in a city's production queue (Nation): build a unit or a building.
/// Copy + flat: goes into hash/save without per-element allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductionItem {
    Unit(UnitKindId),
    Building(BuildingKindId),
    /// A project (Apollo Program, Manhattan Project).
    Project(crate::rules::content::ProjectId),
}

/// A worker's job on a tile (Classic style: N turns in place).
/// Copy: lives right in UnitCore, without allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitWork {
    pub tile: TileId,
    pub target: WorkTarget,
    /// Work still to do, hundredths of worker-turns (Classic build time
    /// minus the progress: each turn takes the worker's rate,
    /// `systems::improvements::work_rate`).
    pub work_left: u16,
}

/// What is being built: a content improvement, a road or a feature chop (forest;
// not content: a road has no bonuses/prices, only passability; a chop's
// parameters come from FeatureDefinition at the moment of the order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkTarget {
    Improvement(ImprovementId),
    Road,
    /// Chop: FeatureId is fixed by the order; completion removes
    /// exactly that one (if it still stands; two workers on one tile).
    Chop(FeatureId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FeatureId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImprovementId(pub u16);
