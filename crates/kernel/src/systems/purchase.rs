//! Purchase with gold: hurry the head of a city queue, buy a tile.
//! Journal: whatever completion writes (production), TileChange::OwnerChanged.

use crate::command::{CommandError, Refusal};
use crate::ids::{CityHandle, PlayerId, ProductionItem, TileId, UnitHandle};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::systems::module::{KernelEvent, ModuleSystems};
use crate::systems::production::purchase_only;
use crate::transaction::journal::UnitChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::CityWrite;

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// The commit safety net mirrors validate: city, owner, non-empty
    /// head, price by the same formula (after ModifyCost), enough gold.
    pub(crate) fn hurry_production(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), CommandError> {
        let (tile, head, stock) = self.hurry_target(player, city)?;
        let core = self
            .world
            .cities
            .get(city)
            .ok_or(CommandError::UnknownCity)?;
        let mods = crate::effects::city::city_modifiers(self.world, rules, core);
        let cost = mods
            .production_cost(rules, head)
            .ok_or(CommandError::RuleViolation("unknown item".into()))?;
        let price = crate::effects::city::head_hurry_price(self.world, rules, core)
            .ok_or(CommandError::RuleViolation("unknown item".into()))?;
        let treasury = self.world.players.treasury_of(player);
        if !self.world.players.spend(player, price) {
            return Err(CommandError::Refused(Refusal::NotEnoughGold { price, treasury }));
        }
        // The item is bought whole: the stock invested so far stays for the
        // next item (Classic: the price does not count it either).
        let _ = stock;
        let topup = cost;
        match self.world.cities.add_production(city, topup) {
            CityWrite::Applied => {}
            CityWrite::TargetGone => return Err(CommandError::UnknownCity),
            CityWrite::QueueFull => unreachable!("add_production never queues"),
        }
        let created_before = self.journal.unit_changes.len();
        if purchase_only(rules, head) {
            // Production never completes it: the purchase does.
            self.complete_head(player, city, tile, cost, rules);
        }
        // The rest of the stock goes on into the queue as production would.
        self.drain_queue(player, city, tile, rules, &mods);
        if matches!(head, ProductionItem::Unit(_)) {
            self.announce_purchase(city, created_before, rules);
        }
        Ok(())
    }

    /// Tell the module that the unit the purchase created (the first one
    /// the journal shows since `created_before`) was bought.
    fn announce_purchase(&mut self, city: CityHandle, created_before: usize, rules: &CompiledRuleset<M>) {
        let bought: Option<UnitHandle> = self.journal.unit_changes[created_before..].iter().find_map(|c| match c {
            UnitChange::Created(unit) => Some(*unit),
            _ => None,
        });
        let Some(unit) = bought else {
            return;
        };
        let Some(kind) = self.world.units.get(unit).map(|u| u.core.kind) else {
            return;
        };
        M::Systems::on_event(self, rules, KernelEvent::UnitPurchased { city, unit, kind });
    }

    /// (city tile, queue head, production stock) of an own city.
    fn hurry_target(
        &self,
        player: PlayerId,
        city: CityHandle,
    ) -> Result<(TileId, ProductionItem, u32), CommandError> {
        let core = self
            .world
            .cities
            .get(city)
            .ok_or(CommandError::UnknownCity)?;
        if core.owner != player {
            return Err(CommandError::RuleViolation("not your city".into()));
        }
        match core.queue.first() {
            Some(head) => Ok((core.tile, *head, core.production)),
            None => Err(CommandError::Refused(Refusal::NothingQueued)),
        }
    }

    /// The commit safety net mirrors validate: city, owner, the
    /// purchasable_ring rules, price by the same formula, gold.
    pub(crate) fn purchase_tile(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        tile: TileId,
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
        let offer = crate::world::territory::tile_purchase_offer(self.world, rules, city, tile)
            .ok_or(CommandError::Refused(Refusal::TileNotForSale))?;
        let treasury = self.world.players.treasury_of(player);
        if !self.world.players.spend(player, offer.price) {
            return Err(CommandError::Refused(Refusal::NotEnoughGold { price: offer.price, treasury }));
        }
        // A bought tile extends the radius; the culture level is culture's
        // alone (classic/docs/classic/rules.md).
        match self.world.cities.extend_radius(city, offer.ring) {
            CityWrite::Applied => {}
            CityWrite::TargetGone => return Err(CommandError::UnknownCity),
            CityWrite::QueueFull => unreachable!("extend_radius never queues"),
        }
        crate::transaction::journal::claim_tile(self.world, &mut self.journal, city, tile);
        self.world.players.count_plot_bought(player);
        Ok(())
    }
}
