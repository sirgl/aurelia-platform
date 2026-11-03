//! The kernel's commands: the orders every 4X game has (move, found, build,
//! produce, research, work tiles, end the turn). A rules module's command
//! type routes them to the kernel (`RulesModule::Command`, Classic:
//! `Command::kernel`); everything else is the module's.

use crate::ids::{CityHandle, ImprovementId, PlayerId, ProductionItem, TechId, TileId, UnitHandle};
use crate::module::RulesModule;
use crate::rules::content::SpecialistId;
use crate::world::citizens::CityFocus;
use crate::world::state::AuthWorld;

/// An order of the kernel's systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelCommand {
    /// Walk a path that fits this turn's moves.
    MoveUnit { unit: UnitHandle, destination: TileId },
    /// Walk towards a tile as far as the moves go; the order stays.
    MarchTo { unit: UnitHandle, destination: TileId },
    /// End the unit's standing order (a march, a build): it stops where it
    /// stands and keeps the moves it has left. Free.
    CancelOrder { unit: UnitHandle },
    FoundCity { settler: UnitHandle },
    /// Fortify: cancelled by movement and attack.
    Fortify { unit: UnitHandle },
    /// Start a build on the worker's tile.
    BuildImprovement { unit: UnitHandle, improvement: ImprovementId },
    BuildRoad { unit: UnitHandle },
    /// Remove the feature of the worker's tile.
    ChopForest { unit: UnitHandle },
    ChooseTechnology { player: PlayerId, technology: TechId },
    /// Atomically replace the plan with missing prerequisites, then the target.
    PlanResearch { player: PlayerId, target: TechId },
    /// Append to a city's queue.
    EnqueueProduction { player: PlayerId, city: CityHandle, item: ProductionItem },
    ClearProduction { player: PlayerId, city: CityHandle },
    /// Buy the head of a city's queue with gold.
    HurryProduction { player: PlayerId, city: CityHandle },
    /// Buy a tile for a city with gold.
    PurchaseTile { player: PlayerId, city: CityHandle, tile: TileId },
    /// Take a tile off work or give it back.
    ToggleWorkedTile { player: PlayerId, city: CityHandle, tile: TileId },
    /// Lock a tile so the city works it first, or unlock it.
    LockWorkedTile { player: PlayerId, city: CityHandle, tile: TileId },
    /// What the automatic citizen assignment favours.
    SetCityFocus { player: PlayerId, city: CityHandle, focus: CityFocus },
    /// Hand-assigned specialists of one kind (0 gives them back).
    SetSpecialists { player: PlayerId, city: CityHandle, specialist: SpecialistId, count: u8 },
    /// Stop the city's growth, or let it grow again (Classic «Avoid Growth»).
    SetAvoidGrowth { player: PlayerId, city: CityHandle, avoid: bool },
    /// Give the city the name at this place of its owner's nation list
    /// (`world::city::free_city_names`).
    ChooseCityName { player: PlayerId, city: CityHandle, name: u16 },
    EndTurn { player: PlayerId },
}

impl KernelCommand {
    /// The player the command acts for: the owner of the ordered unit, or
    /// the player it names. None for an unknown unit (its own check reports
    /// it).
    pub fn acting_player<M: RulesModule>(&self, auth: &AuthWorld<M>) -> Option<PlayerId> {
        let unit_owner = |unit: UnitHandle| auth.units().get(unit).map(|u| u.core.owner);
        match *self {
            KernelCommand::MoveUnit { unit, .. }
            | KernelCommand::MarchTo { unit, .. }
            | KernelCommand::CancelOrder { unit }
            | KernelCommand::Fortify { unit }
            | KernelCommand::BuildImprovement { unit, .. }
            | KernelCommand::BuildRoad { unit }
            | KernelCommand::ChopForest { unit } => unit_owner(unit),
            KernelCommand::FoundCity { settler } => unit_owner(settler),
            KernelCommand::ChooseTechnology { player, .. }
            | KernelCommand::PlanResearch { player, .. }
            | KernelCommand::EnqueueProduction { player, .. }
            | KernelCommand::ClearProduction { player, .. }
            | KernelCommand::HurryProduction { player, .. }
            | KernelCommand::PurchaseTile { player, .. }
            | KernelCommand::ToggleWorkedTile { player, .. }
            | KernelCommand::LockWorkedTile { player, .. }
            | KernelCommand::SetCityFocus { player, .. }
            | KernelCommand::SetSpecialists { player, .. }
            | KernelCommand::SetAvoidGrowth { player, .. }
            | KernelCommand::ChooseCityName { player, .. }
            | KernelCommand::EndTurn { player } => Some(player),
        }
    }
}

// The journal's layout of the kernel's orders (`replay::codec`): a tag keeps
// its number for good; a new order takes the next one.
crate::command_field!(KernelCommand {
    0 => MoveUnit { unit, destination },
    1 => MarchTo { unit, destination },
    2 => FoundCity { settler },
    3 => Fortify { unit },
    4 => BuildImprovement { unit, improvement },
    5 => BuildRoad { unit },
    6 => ChopForest { unit },
    7 => ChooseTechnology { player, technology },
    8 => PlanResearch { player, target },
    9 => EnqueueProduction { player, city, item },
    10 => ClearProduction { player, city },
    11 => HurryProduction { player, city },
    12 => PurchaseTile { player, city, tile },
    13 => ToggleWorkedTile { player, city, tile },
    14 => LockWorkedTile { player, city, tile },
    15 => SetCityFocus { player, city, focus },
    16 => SetSpecialists { player, city, specialist, count },
    17 => EndTurn { player },
    18 => SetAvoidGrowth { player, city, avoid },
    19 => ChooseCityName { player, city, name },
    20 => CancelOrder { unit },
});
