//! Elimination (Classic): a player lives while it
//! has a city; before it ever had one (its first settlers walking), while
//! it has a unit. A player that loses its last city is eliminated and its
//! remaining units go with it. Runs on every commit (the transaction calls
//! it), never from a command.
//!
//! The module may keep a player alive (`ModuleSystems::stays_alive`) and
//! remembers cities it had (`ModuleSystems::had_city`).
//! Journal: UnitChange::Destroyed for the units of an eliminated player,
//! PlayerChange::EffectsChanged for every major nation (their
//! bonuses from the eliminated player are gone).

use crate::ids::PlayerId;
use crate::module::RulesModule;
use crate::systems::module::ModuleSystems;
use crate::transaction::journal::{ChangeJournal, PlayerChange};
use crate::transaction::transaction::WorldTransaction;

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// Alive reconciliation through the indexes: O(P), nobody touches the flags directly.
    pub(crate) fn refresh_alive(&mut self) {
        let players = self.world.players.count();
        let mut eliminated = Vec::new();
        for i in 0..players {
            let p = PlayerId(i as u8);
            let was_alive = self.world.players.is_alive(p);
            let alive = self.lives(p);
            self.world.players.set_alive(p, alive);
            if was_alive && !alive {
                eliminated.push(p);
            }
        }
        for p in eliminated {
            self.bury(p);
        }
    }

    fn lives(&self, p: PlayerId) -> bool {
        let has_city = match self.world.indexes.cities_by_player.get(&p.0) {
            Some(cities) => !cities.is_empty(),
            None => false,
        };
        let has_unit = !self.world.indexes.units_of(p).is_empty();
        has_city || (has_unit && !self.ever_had_city(p)) || M::Systems::stays_alive(self.world, p)
    }

    /// The player had a city once (the module's memory of it, or a city it
    /// founded that another player holds now).
    fn ever_had_city(&self, p: PlayerId) -> bool {
        M::Systems::had_city(self.world, p)
            || self.world.cities.iter().any(|(_, c)| c.original_owner == p)
    }

    /// The units of an eliminated player are removed; every nation's
    /// effects are recomputed.
    fn bury(&mut self, p: PlayerId) {
        let units = self.world.indexes.units_of(p).to_vec();
        for unit in units {
            let _ = self.destroy_unit(unit);
        }
        let players = &self.world.players;
        let majors = (0..players.count()).map(|i| PlayerId(i as u8));
        let majors: Vec<PlayerId> = majors.filter(|p| players.is_major(*p) && players.is_alive(*p)).collect();
        for major in majors {
            self.journal
                .player_changes
                .push(PlayerChange::EffectsChanged { player: major });
        }
    }

    /// An eliminated active player hands the turn on (nobody else could end
    /// it: EndTurn requires an alive player). Runs after game commands only:
    /// harness setup (debug_*) may populate players in any order.
    pub(crate) fn skip_eliminated_active(&mut self, rules: &crate::rules::CompiledRuleset<M>) {
        let active = self.world.turn.active_player;
        if !self.world.players.is_alive(active) {
            self.pass_turn(active, rules);
        }
    }

    /// Commit invariants that hold after any mutation (elimination), then
    /// the journal for derived systems.
    pub fn commit(mut self) -> ChangeJournal<M> {
        self.refresh_alive();
        self.journal
    }

    /// Commit of a game command: elimination, and the turn moves on when
    /// the active player was eliminated.
    pub fn commit_command(mut self, rules: &crate::rules::CompiledRuleset<M>) -> ChangeJournal<M> {
        self.refresh_alive();
        self.skip_eliminated_active(rules);
        self.journal
    }
}
