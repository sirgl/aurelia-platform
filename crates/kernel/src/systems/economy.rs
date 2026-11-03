//! Treasury (classic/docs/classic/rules.md): the
//! empire's gold balance of the turn (the module's income and upkeep,
//! `ModuleSystems::gold_balance`) goes into the treasury. A balance that
//! would go below zero empties the treasury; the shortfall is taken from
//! the turn's science, and the module decides what else a deficit costs
//! (`ModuleSystems::treasury_deficit`). Capital connections are marked here
//! at the start of the turn.

use crate::ids::PlayerId;
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::systems::module::ModuleSystems;
use crate::transaction::journal::CityChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::CapitalConnection;

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// Mark which of the player's cities reach its capital this turn (the
    /// capital counts as connected), only when an effect reads it
    /// (`EffectIndex::uses_capital_connection`). A city whose connection
    /// changes is journaled: its yields follow.
    pub(crate) fn update_capital_connections(&mut self, player: PlayerId, rules: &CompiledRuleset<M>) {
        if !rules.effect_index.uses_capital_connection {
            return;
        }
        let connected = match crate::world::city::capital_of(self.world, rules, player) {
            Some(capital) => {
                // Inside a command the memo is closed: compute.
                crate::derived::connections::compute_connected_cities(self.world, rules, player, capital)
            }
            None => Vec::new(),
        };
        for city in self.world.indexes.cities_of(player).to_vec() {
            let wanted = if connected.contains(&city) {
                CapitalConnection::Connected
            } else {
                CapitalConnection::Disconnected
            };
            let Some(core) = self.world.cities.get_mut(city) else {
                continue;
            };
            if core.capital_connection == wanted {
                continue;
            }
            core.capital_connection = wanted;
            self.journal.city_changes.push(CityChange::StatusChanged { city });
        }
    }

    /// The turn's gold (`cities_total` = the cities' gold). Returns the
    /// shortfall (≤ 0) the science of the turn loses.
    pub(crate) fn collect_gold(
        &mut self,
        player: PlayerId,
        cities_total: i32,
        rules: &CompiledRuleset<M>,
    ) -> i32 {
        let net = M::Systems::gold_balance(self.world, rules, player, cities_total);
        let after = i64::from(self.world.players.treasury_of(player)) + net;
        if after >= 0 {
            self.world
                .players
                .set_treasury(player, after.min(i64::from(u32::MAX)) as u32);
            return 0;
        }
        self.world.players.set_treasury(player, 0);
        M::Systems::treasury_deficit(self, rules, player, after);
        after.max(i64::from(i32::MIN)) as i32
    }
}
