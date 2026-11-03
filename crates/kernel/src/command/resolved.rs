//! A validated kernel command: what the commit applies (validate decides
//! "may it?", the commit "what changes?" and still guards the invariants).

use crate::ids::{CityHandle, FeatureId, ImprovementId, PlayerId, ProductionItem, TechId, TileId, UnitHandle};
use crate::rules::content::SpecialistId;
use crate::world::citizens::CityFocus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedKernel {
    /// A move validated to fit this turn's move points.
    MoveUnit { unit: UnitHandle, to: TileId },
    March { unit: UnitHandle, to: TileId },
    CancelOrder { unit: UnitHandle },
    FoundCity { settler: UnitHandle, at: TileId, owner: PlayerId },
    Fortify { unit: UnitHandle },
    BuildImprovement { unit: UnitHandle, improvement: ImprovementId, tile: TileId, work_time: u32 },
    BuildRoad { unit: UnitHandle, tile: TileId },
    ChopForest { unit: UnitHandle, tile: TileId, feature: FeatureId, work_time: u32 },
    ChooseTechnology { player: PlayerId, technology: TechId },
    PlanResearch { player: PlayerId, target: TechId, queue: Vec<TechId> },
    EnqueueProduction { player: PlayerId, city: CityHandle, item: ProductionItem },
    ClearProduction { player: PlayerId, city: CityHandle },
    HurryProduction { player: PlayerId, city: CityHandle },
    PurchaseTile { player: PlayerId, city: CityHandle, tile: TileId },
    ToggleWorkedTile { player: PlayerId, city: CityHandle, tile: TileId },
    LockWorkedTile { player: PlayerId, city: CityHandle, tile: TileId },
    SetCityFocus { player: PlayerId, city: CityHandle, focus: CityFocus },
    SetSpecialists { player: PlayerId, city: CityHandle, specialist: SpecialistId, count: u8 },
    SetAvoidGrowth { player: PlayerId, city: CityHandle, avoid: bool },
    ChooseCityName { player: PlayerId, city: CityHandle, name: u16 },
    EndTurn { player: PlayerId },
}
