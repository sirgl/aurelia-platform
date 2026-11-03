//! Domain operations over stores (tx.create_unit / tx.destroy_unit / ...).
//! Also here: the centralised update of secondary indexes and the journal:
//! systems do not touch the unit indexes themselves.

use crate::command::CommandError;
use crate::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::systems::{KernelEvent, ModuleSystems};
use crate::transaction::journal::UnitChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::module::ModuleWorld;
use crate::world::unit::UnitCore;

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    pub(crate) fn create_unit(
        &mut self,
        owner: PlayerId,
        tile: TileId,
        kind: UnitKindId,
        moves_max: u16,
    ) -> Result<UnitHandle, CommandError> {
        let id = self.world.allocate_entity();
        let h = self.world.units.insert(UnitCore {
            id,
            tile,
            owner,
            kind,
            hp: crate::world::unit::UNIT_MAX_HP,
            moves_left: moves_max,
            moves_max,
            stance: crate::world::unit::Stance::Ready,
            work: None,
            goal: None,
            promotions: Default::default(),
            experience: 0,
            level: 1,
            attacks_made: 0,
            interceptions_made: 0,
        });
        self.world.indexes.index_unit(owner, tile, h);
        self.journal.unit_changes.push(UnitChange::Created(h));
        Ok(h)
    }

    /// Create a unit of `kind` for `owner` on `tile` (moves and free
    /// promotions of a new unit); the module reacts (`UnitSpawned`).
    pub fn spawn_unit(
        &mut self,
        owner: PlayerId,
        tile: TileId,
        kind: UnitKindId,
        rules: &CompiledRuleset<M>,
    ) -> UnitHandle {
        let movement = crate::effects::unit::new_unit_movement(self.world, rules, owner, kind);
        let handle = self
            .create_unit(owner, tile, kind, movement)
            .expect("create_unit never fails");
        let free = crate::effects::unit::free_promotions(rules, kind);
        if let Some(core) = self.world.units.get_mut_core(handle) {
            core.promotions = free;
        }
        if crate::navigation::terrain::is_water(&self.world.map, tile) {
            // A land unit appearing on water is embarked.
            self.refresh_allowance(handle, rules);
        }
        let event = KernelEvent::UnitSpawned { player: owner, unit: handle };
        M::Systems::on_event(self, rules, event);
        handle
    }

    /// A unit a city trains or receives: on the city tile when the stacking
    /// rule allows, else on the first neighbour (HEX_DIRS order) where the
    /// owner may enter and stop (Classic places it on the nearest valid plot).
    /// With no such tile it stays in the city, over the limit.
    pub fn spawn_at_city(
        &mut self,
        owner: PlayerId,
        city_tile: TileId,
        kind: UnitKindId,
        rules: &CompiledRuleset<M>,
    ) -> UnitHandle {
        let tile = self.spawn_tile(owner, city_tile, kind, rules);
        self.spawn_unit(owner, tile, kind, rules)
    }

    fn spawn_tile(
        &self,
        owner: PlayerId,
        city_tile: TileId,
        kind: UnitKindId,
        rules: &CompiledRuleset<M>,
    ) -> TileId {
        use crate::world::stacking::can_stop_at;
        let auth = &*self.world;
        if can_stop_at(auth, rules, owner, kind, None, city_tile).is_ok() {
            return city_tile;
        }
        let around = auth.map().dimensions.neighbors(city_tile);
        let free = around.into_iter().find(|t| {
            let profile = crate::navigation::MoveProfile::of_kind(auth, rules, owner, kind);
            let passable = crate::navigation::cost::destination_ok(auth, &profile, *t);
            let enter = crate::world::territory::may_enter(auth, owner, *t);
            passable && enter && can_stop_at(auth, rules, owner, kind, None, *t).is_ok()
        });
        free.unwrap_or(city_tile)
    }

    /// Remove a unit (killed, or consumed by founding a city): store,
    /// indexes, the module's state of the unit (`ModuleWorld::unit_removed`)
    /// and the Destroyed journal entry in one place.
    pub fn destroy_unit(&mut self, unit: UnitHandle) -> Result<UnitCore, CommandError> {
        M::Systems::unit_removing(self, unit);
        let core = self
            .world
            .units
            .remove(unit)
            .ok_or(CommandError::UnknownUnit)?;
        self.world.indexes.unindex_unit(core.owner, core.tile, unit);
        self.world.module.unit_removed(core.id);
        self.journal.unit_changes.push(UnitChange::Destroyed {
            unit,
            id: core.id,
            owner: core.owner,
            tile: core.tile,
            kind: core.kind,
            promotions: core.promotions,
        });
        Ok(core)
    }
}
