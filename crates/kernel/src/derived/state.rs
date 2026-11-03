use crate::module::RulesModule;
use crate::derived::connectivity::ConnectivityState;
use crate::derived::movement::MovementState;
use crate::derived::visibility::VisibilityState;
use crate::derived::yields::YieldState;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::ChangeJournal;
use crate::world::state::AuthWorld;

#[derive(Debug, Clone, Default)]
pub struct DerivedWorld<M: RulesModule> {
    pub visibility: VisibilityState<M>,
    /// City yields, the module's terms included (`RulesModule::Yields`).
    pub yields: YieldState,
    pub movement: MovementState,
    pub connectivity: ConnectivityState,
    module: std::marker::PhantomData<M>,
}

impl<M: RulesModule> DerivedWorld<M> {
    pub fn rebuild(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> Self {
        Self {
            visibility: VisibilityState::rebuild(auth, rules),
            yields: YieldState::rebuild(auth, rules),
            movement: MovementState::rebuild(auth, rules),
            connectivity: ConnectivityState::rebuild(auth, rules),
            module: std::marker::PhantomData,
        }
    }

    pub fn apply_changes(
        &mut self,
        changes: &ChangeJournal<M>,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
    ) {
        {
            let _span = prof::span("derived/visibility");
            self.visibility.apply_changes(changes, auth, rules);
        }
        {
            let _span = prof::span("derived/yields");
            self.yields.apply_changes(changes, auth, rules);
        }
        {
            let _span = prof::span("derived/movement");
            self.movement.apply_changes(changes, auth, rules);
        }
        let _span = prof::span("derived/connectivity");
        self.connectivity.apply_changes(changes, auth, rules);
    }

    /// Oracle check for correctness tests.
    /// Compares only the recoverable part: visibility.counts + yields.
    /// `visibility.seen` (explored history) accumulates monotonically by increments
    /// and is NOT restored from the current unit positions; per §45 platform/structure.md
    /// it is a PlayerKnowledge candidate (persisted state), not pure derived.
    /// So seen is checked separately: incremental.seen ⊇ rebuild.seen.
    pub(crate) fn assert_matches_rebuild(
        &self,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), String> {
        let rebuilt = Self::rebuild(auth, rules);
        if self.visibility.counts != rebuilt.visibility.counts
            || self.visibility.players != rebuilt.visibility.players
            || self.visibility.tiles != rebuilt.visibility.tiles
        {
            return Err("visibility mismatch rebuild vs incremental".into());
        }
        // What each unit covers (slots past the rebuilt table are empty).
        let slots = self.visibility.unit_cover.len().max(rebuilt.visibility.unit_cover.len());
        for slot in 0..slots {
            let ours = self.visibility.unit_cover.get(slot).copied().unwrap_or_default();
            let fresh = rebuilt.visibility.unit_cover.get(slot).copied().unwrap_or_default();
            if ours != fresh {
                return Err(format!("unit sight mismatch in slot {slot}: {ours:?} vs {fresh:?}"));
            }
        }
        if !rebuilt.visibility.seen.is_subset(&self.visibility.seen) {
            return Err("visibility.seen lost history".into());
        }
        if self.yields != rebuilt.yields {
            return Err("yields mismatch rebuild vs incremental".into());
        }
        if self.movement != rebuilt.movement {
            return Err("movement occupancy mismatch rebuild vs incremental".into());
        }
        Ok(())
    }
}
