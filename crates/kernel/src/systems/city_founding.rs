//! City founding: the settler is consumed, the city and its ring appear.
//! The first city of a player without a capital gets the Palace (Classic
//! a player's first city is its capital); then the module
//! adds its own (`KernelEvent::CityFounded`).
//! Journal: UnitChange::Destroyed, CityChange::Founded, TileChange::
//! OwnerChanged, CityChange::BuildingAdded.

use crate::command::{CommandError, Refusal};
use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::systems::module::{KernelEvent, ModuleSystems};
use crate::transaction::journal::{ChangeJournal, CityChange};
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// A major nation founds its first city: its original capital
    /// (Classic: the first city gets the palace). Never founded a city before
    /// = no city of its own founding exists, alive or captured.
    fn founded_first_city(&self, owner: PlayerId) -> bool {
        if !self.world.players.is_major(owner) {
            return false;
        }
        !self
            .world
            .cities
            .iter()
            .any(|(_, c)| c.original_owner == owner && c.original_capital)
    }

    pub fn found_city(
        &mut self,
        settler: UnitHandle,
        at: TileId,
        owner: PlayerId,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), CommandError> {
        crate::world::territory::city_site_problem(self.world, owner, at)
            .map_err(|problem| CommandError::Refused(Refusal::Site(problem)))?;
        self.destroy_unit(settler)?;
        let id = self.world.allocate_entity();
        let name = crate::world::city::next_city_name(self.world, rules, owner);
        let first = self.founded_first_city(owner);
        let round = self.world.turn.round;
        let city = self.world.cities.insert(CityCore::founded(id, at, owner, name, first, round));
        self.world.indexes.index_city(owner, at, city);
        self.journal.city_changes.push(CityChange::Founded(city));
        // Founding territory: ring = work area (Nation). One helper
        // for both paths (commit + debug); the journal has an OwnerChanged per tile.
        crate::transaction::journal::claim_founding_ring(
            self.world,
            &mut self.journal,
            city,
            crate::derived::yields::CITY_TERRITORY_RADIUS,
        );
        place_palace(self.world, &mut self.journal, rules, owner, city);
        M::Systems::on_event(self, rules, KernelEvent::CityFounded { player: owner, city });
        Ok(())
    }
}

/// The Palace of the ruleset (the building of kind Palace; None in a
/// ruleset without one).
pub fn palace<M: RulesModule>(rules: &CompiledRuleset<M>) -> Option<crate::ids::BuildingKindId> {
    let index = rules
        .buildings
        .iter()
        .position(|b| b.kind == crate::rules::content::BuildingKind::Palace)?;
    Some(crate::ids::BuildingKindId(index as u16))
}

/// A player without a capital makes `city` its capital: the Palace appears
/// there. One helper for the command and the harness path.
pub fn place_palace<M: RulesModule>(
    world: &mut AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    city: crate::ids::CityHandle,
) {
    let Some(building) = palace(rules) else {
        return;
    };
    if crate::world::city::capital_of(world, rules, owner).is_some() {
        return;
    }
    if let crate::world::city::CityWrite::Applied = world.cities.add_building(city, building) {
        journal
            .city_changes
            .push(CityChange::BuildingAdded { city, building });
    }
}
