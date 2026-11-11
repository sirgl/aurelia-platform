//! Production: the city queue (orders), completion of units and buildings.
//! Journal: UnitChange::Created, CityChange::BuildingAdded. Fires OnBuilt.

use crate::command::CommandError;
use crate::effects::city::CityModifiers;
use crate::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TileId, UnitKindId};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::systems::module::{KernelEvent, ModuleSystems};
use crate::transaction::journal::{CityChange, OrderRemoval};
use crate::world::state::AuthWorld;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::{CityWrite, SpendOutcome};

/// A unit only bought with gold, never trained.
pub(crate) fn purchase_only<M: RulesModule>(rules: &CompiledRuleset<M>, item: ProductionItem) -> bool {
    match item {
        ProductionItem::Unit(kind) => rules
            .units
            .get(kind.0 as usize)
            .is_some_and(|d| crate::rules::rows::UnitExtension::purchase_only(&d.module)),
        ProductionItem::Building(_) | ProductionItem::Project(_) => false,
    }
}

/// Why the rules can never complete `item` in `city` as the world stands
/// (None: it can). Structural reasons only: they hold for every module.
/// Validation of a purchase, the commit and the queue drain all ask this one
/// question before any production or gold is spent.
pub(crate) fn order_impossible<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    city: CityHandle,
    item: ProductionItem,
) -> Option<OrderRemoval> {
    let ProductionItem::Building(kind) = item else {
        return None;
    };
    let core = auth.cities().get(city)?;
    if core.buildings.contains(&kind) {
        return Some(OrderRemoval::AlreadyBuilt);
    }
    let def = rules.buildings.get(kind.0 as usize)?;
    let elsewhere = |same_owner: bool| {
        auth.cities()
            .iter()
            .any(|(h, c)| h != city && c.buildings.contains(&kind) && (!same_owner || c.owner == core.owner))
    };
    if def.wonder && elsewhere(false) {
        return Some(OrderRemoval::WonderBuilt);
    }
    if def.kind == crate::rules::content::BuildingKind::NationalWonder && elsewhere(true) {
        return Some(OrderRemoval::NationalWonderBuilt);
    }
    None
}

/// `order_impossible` plus the module's veto (`queue_item_blocked`).
pub(crate) fn order_lost<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    city: CityHandle,
    item: ProductionItem,
) -> Option<OrderRemoval> {
    order_impossible(auth, rules, city, item)
        .or_else(|| M::Systems::queue_item_blocked(auth, rules, owner, item).then_some(OrderRemoval::Blocked))
}

/// The refusal that tells a player why a purchase of a lost order is not taken.
pub(crate) fn refusal_for(reason: OrderRemoval) -> crate::command::Refusal {
    use crate::command::Refusal;
    match reason {
        OrderRemoval::WonderBuilt => Refusal::WonderBuilt,
        OrderRemoval::NationalWonderBuilt | OrderRemoval::AlreadyBuilt => Refusal::AlreadyBuilt,
        OrderRemoval::Blocked => Refusal::NotProducible,
    }
}

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// Commit safety net mirrors validate (the world may have changed):
    /// a foreign/stale city, duplicate buildings, overflow.
    pub(crate) fn enqueue_production(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        item: ProductionItem,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), CommandError> {
        let core = self
            .world
            .cities
            .get(city)
            .ok_or(CommandError::UnknownCity)?;
        if core.owner != player {
            return Err(CommandError::RuleViolation("not your city".into()));
        }
        if let ProductionItem::Building(kind) = item {
            if core.buildings.contains(&kind) || core.queue.contains(&item) {
                return Err(CommandError::RuleViolation(
                    "already built or queued".into(),
                ));
            }
        }
        if order_impossible(&*self.world, rules, city, item) == Some(OrderRemoval::WonderBuilt) {
            return Err(CommandError::RuleViolation("wonder already built".into()));
        }
        let new_head = core.queue.is_empty();
        match self.world.cities.enqueue(city, item) {
            CityWrite::Applied => {}
            CityWrite::QueueFull => return Err(CommandError::RuleViolation("queue full".into())),
            CityWrite::TargetGone => return Err(CommandError::UnknownCity),
        }
        if new_head {
            self.journal.city_changes.push(CityChange::QueueChanged { city });
        }
        Ok(())
    }

    pub(crate) fn clear_production(
        &mut self,
        player: PlayerId,
        city: CityHandle,
    ) -> Result<(), CommandError> {
        let core = self
            .world
            .cities
            .get(city)
            .ok_or(CommandError::UnknownCity)?;
        if core.owner != player {
            return Err(CommandError::RuleViolation("not your city".into()));
        }
        let had_head = !core.queue.is_empty();
        match self.world.cities.clear_queue(city) {
            CityWrite::Applied => {}
            CityWrite::QueueFull => unreachable!("clear never queues"),
            CityWrite::TargetGone => return Err(CommandError::UnknownCity),
        }
        if had_head {
            self.journal.city_changes.push(CityChange::QueueChanged { city });
        }
        Ok(())
    }

    /// Remove the head of the queue; the city's production percents toward
    /// the item change with it (journaled).
    fn pop_head(&mut self, city: CityHandle) {
        if self.world.cities.pop_queue(city).is_some() {
            self.journal.city_changes.push(CityChange::QueueChanged { city });
        }
    }

    /// The head of the queue is taken out because the rules cannot complete
    /// it: journaled as `OrderRemoved`, no production or gold is spent.
    fn remove_head_lost(&mut self, owner: PlayerId, city: CityHandle, item: ProductionItem, reason: OrderRemoval) {
        self.pop_head(city);
        self.journal.city_changes.push(CityChange::OrderRemoved { city, owner, item, reason });
    }

    /// A wonder has just been completed in `city`: its order leaves the queue
    /// of every other city that holds it, at any position (a world wonder: of
    /// every player; a national wonder: of the same owner). The production stock
    /// of those cities is not touched. Cities are visited in slot order.
    fn sweep_lost_orders(&mut self, city: CityHandle, building: BuildingKindId, rules: &CompiledRuleset<M>) {
        let Some(def) = rules.buildings.get(building.0 as usize) else {
            return;
        };
        let national = def.kind == crate::rules::content::BuildingKind::NationalWonder;
        if !def.wonder && !national {
            return;
        }
        let Some(owner) = self.world.cities.get(city).map(|c| c.owner) else {
            return;
        };
        let item = ProductionItem::Building(building);
        let holders: Vec<(CityHandle, PlayerId)> = self
            .world
            .cities
            .iter()
            .filter(|(h, c)| *h != city && c.queue.contains(&item) && (def.wonder || c.owner == owner))
            .map(|(h, c)| (h, c.owner))
            .collect();
        let reason = if def.wonder { OrderRemoval::WonderBuilt } else { OrderRemoval::NationalWonderBuilt };
        for (other, other_owner) in holders {
            if let Some(at) = self.world.cities.remove_queued(other, item) {
                if at == 0 {
                    self.journal.city_changes.push(CityChange::QueueChanged { city: other });
                }
                self.journal.city_changes.push(CityChange::OrderRemoved { city: other, owner: other_owner, item, reason });
            }
        }
    }

    /// Build everything affordable from the head of the queue (prices after the city's
    /// ModifyCost at the start of the step). Loops terminate: the stockpile melts at prices
    /// >= 1 (the compiler rejects zero ones, caps do not let it go lower).
    /// An empty queue builds nothing (Classic has no default order).
    pub fn drain_queue(
        &mut self,
        owner: PlayerId,
        city: CityHandle,
        tile: TileId,
        rules: &CompiledRuleset<M>,
        mods: &CityModifiers<M>,
    ) {
        loop {
            let head = match self.world.cities.get(city) {
                Some(core) => core.queue.first().copied(),
                None => return,
            };
            let Some(item) = head else {
                return;
            };
            if let Some(reason) = order_lost(&*self.world, rules, owner, city, item) {
                // An order the rules cannot complete (a wonder built elsewhere, a
                // duplicate, resources gone) leaves the queue with a record; the
                // production stays stored for the next order.
                self.remove_head_lost(owner, city, item, reason);
                continue;
            }
            if purchase_only(rules, item) {
                // Bought, never trained: production waits at the head.
                return;
            }
            let Some(cost) = mods.production_cost(rules, item) else {
                return;
            };
            if !self.complete_head(owner, city, tile, cost, rules) {
                return;
            }
        }
    }

    /// Spend `cost` of the city's production on the head of its queue and
    /// complete it; false when the production does not reach it.
    pub(crate) fn complete_head(
        &mut self,
        owner: PlayerId,
        city: CityHandle,
        tile: TileId,
        cost: u32,
        rules: &CompiledRuleset<M>,
    ) -> bool {
        let Some(item) = self.world.cities.get(city).and_then(|c| c.queue.first().copied()) else {
            return false;
        };
        // The check comes before the spend: an order that can no longer be
        // completed costs nothing and is removed with a record.
        if let Some(reason) = order_lost(&*self.world, rules, owner, city, item) {
            self.remove_head_lost(owner, city, item, reason);
            return true;
        }
        match self.world.cities.spend_production(city, cost) {
            SpendOutcome::Spent => {}
            SpendOutcome::NotEnough | SpendOutcome::TargetGone => return false,
        }
        self.pop_head(city);
        self.complete_item(owner, city, tile, item, rules);
        true
    }

    /// A completed order goes into the world: the unit spawns, the building stands
    /// (+ journal: Created / BuildingAdded, so derived knows).
    fn complete_item(
        &mut self,
        owner: PlayerId,
        city: CityHandle,
        tile: TileId,
        item: ProductionItem,
        rules: &CompiledRuleset<M>,
    ) {
        match item {
            ProductionItem::Unit(kind) => self.complete_unit(owner, city, tile, kind, rules),
            ProductionItem::Building(building) => self.complete_building(city, building, rules),
            ProductionItem::Project(project) => {
                M::Systems::on_event(self, rules, KernelEvent::ProjectCompleted { player: owner, city, project })
            }
        }
    }

    fn complete_unit(
        &mut self,
        owner: PlayerId,
        city: CityHandle,
        tile: TileId,
        kind: UnitKindId,
        rules: &CompiledRuleset<M>,
    ) {
        let unit = self.spawn_at_city(owner, tile, kind, rules);
        M::Systems::on_event(self, rules, KernelEvent::UnitTrained { city, unit, kind });
    }

    /// Harness-only: the building completed as by production (OnBuilt fires).
    #[cfg(any(test, feature = "harness"))]
    pub fn debug_complete_building(&mut self, city: CityHandle, building: BuildingKindId, rules: &CompiledRuleset<M>) {
        self.complete_building(city, building, rules);
    }

    fn complete_building(
        &mut self,
        city: CityHandle,
        building: BuildingKindId,
        rules: &CompiledRuleset<M>,
    ) {
        // Defence in depth: `complete_head` has asked `order_impossible` before it
        // spent anything, so this does not happen on that path. A duplicate or a
        // lost wonder that gets here anyway (the harness) is not built; nothing
        // is spent here and nothing is consumed.
        let already = match self.world.cities.get(city) {
            Some(core) => core.buildings.contains(&building),
            None => true,
        };
        if already || self.lost_wonder_race(city, building, rules) {
            return;
        }
        match self.world.cities.add_building(city, building) {
            CityWrite::Applied => {}
            CityWrite::TargetGone => return,
            CityWrite::QueueFull => unreachable!("add_building never queues"),
        }
        self.journal
            .city_changes
            .push(CityChange::BuildingAdded { city, building });
        self.sweep_lost_orders(city, building, rules);
        M::Systems::on_event(self, rules, KernelEvent::BuildingCompleted { city, building });
    }

    /// Wonder race: another city has already built this wonder.
    fn lost_wonder_race(
        &self,
        city: CityHandle,
        building: BuildingKindId,
        rules: &CompiledRuleset<M>,
    ) -> bool {
        let wonder = match rules.buildings.get(building.0 as usize) {
            Some(def) => def.wonder,
            None => false,
        };
        if !wonder {
            return false;
        }
        let mut cities = self.world.cities.iter();
        cities.any(|(h, c)| h != city && c.buildings.contains(&building))
    }
}
