//! Occupancy cache for navigation (§18 platform/structure.md: persistent navigation caches
//! live in DerivedWorld; §33: a fast dynamic occupancy/ZOC layer).
//!
//! Two layers per (player, tile), row-major like visibility: all units and
//! military units (zones of control come from military units only, classic
//! rules.md). Exact O(1) increments per event; rebuild is the oracle;
//! equivalence is checked by assert_matches_rebuild.
//! Memory: 2 x P*T*2B = 1.6MB @50k*8.

use crate::module::RulesModule;
use crate::ids::{PlayerId, TileId, UnitKindId};
use crate::metrics::WorkCounters;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::ChangeJournal;
use crate::world::state::AuthWorld;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MovementState {
    /// Units of player p on tile t: units[p * tiles + t].
    pub units: Vec<u16>,
    /// Military units of player p on tile t (same layout).
    pub military: Vec<u16>,
    /// Military ships of player p on tile t (zones of control at sea).
    pub naval: Vec<u16>,
    pub players: usize,
    pub tiles: usize,
}

impl MovementState {
    pub fn rebuild<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> Self {
        Self::rebuild_counted(auth, rules).0
    }

    pub fn rebuild_counted<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> (Self, WorkCounters) {
        let players = auth.players().count();
        let tiles = auth.map().tile_count();
        let mut state = Self {
            units: vec![0u16; players * tiles],
            military: vec![0u16; players * tiles],
            naval: vec![0u16; players * tiles],
            players,
            tiles,
        };
        let mut c = WorkCounters::default();
        for (_, core) in auth.units().iter_handles() {
            state.bump(rules, core.owner, core.tile, core.kind, 1);
            c.entities_visited += 1;
        }
        c.bytes_allocated = (state.units.len() * 4) as u64;
        (state, c)
    }

    /// Units of `owner` on `tile`.
    pub fn units_of(&self, owner: PlayerId, tile: TileId) -> u16 {
        self.slot(owner, tile)
            .and_then(|i| self.units.get(i).copied())
            .unwrap_or(0)
    }

    /// Military units of `owner` on `tile` (zone of control sources).
    pub fn military_of(&self, owner: PlayerId, tile: TileId) -> u16 {
        self.slot(owner, tile)
            .and_then(|i| self.military.get(i).copied())
            .unwrap_or(0)
    }

    /// Military ships of `owner` on `tile`.
    pub(crate) fn naval_of(&self, owner: PlayerId, tile: TileId) -> u16 {
        self.slot(owner, tile)
            .and_then(|i| self.naval.get(i).copied())
            .unwrap_or(0)
    }

    fn slot(&self, owner: PlayerId, tile: TileId) -> Option<usize> {
        let (p, t) = (owner.0 as usize, tile.0 as usize);
        if p < self.players && t < self.tiles {
            Some(p * self.tiles + t)
        } else {
            None
        }
    }

    fn bump<M: RulesModule>(&mut self, rules: &CompiledRuleset<M>, owner: PlayerId, tile: TileId, kind: UnitKindId, delta: i16) {
        let Some(i) = self.slot(owner, tile) else {
            return;
        };
        self.units[i] = self.units[i].saturating_add_signed(delta);
        if !rules.is_civilian(kind) {
            self.military[i] = self.military[i].saturating_add_signed(delta);
            let sea = rules
                .units
                .get(kind.0 as usize)
                .is_some_and(|d| d.domain == crate::rules::content::Domain::Sea);
            if sea {
                self.naval[i] = self.naval[i].saturating_add_signed(delta);
            }
        }
    }

    pub fn apply_changes<M: RulesModule>(
        &mut self,
        changes: &ChangeJournal<M>,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
    ) {
        let _ = Self::apply_changes_counted(self, changes, auth, rules);
    }

    /// Exact O(1) increment: owner/tile/kind are either in the event (Destroyed)
    /// or resolved from the post-commit world (Moved/Created: the unit is alive).
    pub fn apply_changes_counted<M: RulesModule>(
        ms: &mut MovementState,
        changes: &ChangeJournal<M>,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
    ) -> WorkCounters {
        use crate::transaction::journal::UnitChange;
        let mut c = WorkCounters::default();
        for uc in &changes.unit_changes {
            match uc {
                UnitChange::Created(h) => {
                    if let Some(v) = auth.units().get(*h) {
                        ms.bump(rules, v.core.owner, v.core.tile, v.core.kind, 1);
                        c.entities_visited += 1;
                    }
                }
                UnitChange::Moved { unit, from, to } => {
                    if let Some(v) = auth.units().get(*unit) {
                        ms.bump(rules, v.core.owner, *from, v.core.kind, -1);
                        ms.bump(rules, v.core.owner, *to, v.core.kind, 1);
                        c.entities_visited += 1;
                    }
                }
                UnitChange::Damaged { .. } => {
                    // HP does not change occupancy: zero work.
                }
                UnitChange::Promoted { .. } | UnitChange::StanceChanged { .. } => {
                    // Promotions and stances do not change occupancy.
                }
                UnitChange::Destroyed { owner, tile, kind, .. } => {
                    ms.bump(rules, *owner, *tile, *kind, -1);
                    c.entities_visited += 1;
                }
            }
        }
        c
    }
}
