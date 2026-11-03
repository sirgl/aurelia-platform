//! WorldTransaction: the single point of authoritative mutation (§13).
//!
//! A command's systems change the world through one transaction: the world
//! and the journal of what changed. The kernel's systems are its methods
//! (units, cities, the turn); the rules module adds its own systems on top
//! (Classic wraps it in its own transaction type) and joins the kernel's at
//! the points the module contract names (`module::ModuleSystems`).

use crate::command::{CommandError, ResolvedKernel};
use crate::derived::DerivedWorld;
use crate::module::RulesModule;
use crate::navigation::PathScratch;
use crate::rules::CompiledRuleset;
use crate::systems::movement::Watch;
use crate::transaction::journal::ChangeJournal;
use crate::world::state::AuthWorld;

/// The world being changed and the journal of the changes.
pub struct WorldTransaction<'a, M: RulesModule> {
    pub world: &'a mut AuthWorld<M>,
    pub journal: ChangeJournal<M>,
    /// `player_modifiers_scoped`: per player, the journal revision it was
    /// computed at and the answer.
    scoped: std::cell::RefCell<Vec<Option<(usize, std::sync::Arc<crate::effects::player::PlayerModifiers<M>>)>>>,
}

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    pub fn new(world: &'a mut AuthWorld<M>) -> Self {
        Self {
            world,
            journal: ChangeJournal::default(),
            scoped: Default::default(),
        }
    }

    /// `player_modifiers` inside a command, where the world's memo is closed:
    /// the answer is kept while the journal of this transaction records no
    /// change (every change the effect sources are computed from is
    /// journaled, because derived state follows the journal). The loop
    /// over a player's cities asks once instead of once per city.
    pub fn player_modifiers_scoped(
        &self,
        rules: &CompiledRuleset<M>,
        player: crate::ids::PlayerId,
    ) -> std::sync::Arc<crate::effects::player::PlayerModifiers<M>> {
        use crate::effects::memo::MemoMode;
        let mode = self.world.memo().mode();
        let compute = || crate::effects::player::compute_player_modifiers(&*self.world, rules, player);
        if mode == MemoMode::Off {
            return std::sync::Arc::new(compute());
        }
        let (slot, revision) = (player.0 as usize, self.journal.revision());
        let found = {
            let scoped = self.scoped.borrow();
            scoped.get(slot).and_then(|e| e.as_ref()).filter(|(rev, _)| *rev == revision).map(|(_, m)| m.clone())
        };
        if let Some(found) = found {
            if mode == MemoMode::Verify {
                assert_eq!(*found, compute(), "stale scoped modifiers of {player:?} at journal revision {revision}");
            }
            return found;
        }
        let fresh = std::sync::Arc::new(compute());
        let mut scoped = self.scoped.borrow_mut();
        if scoped.len() <= slot {
            scoped.resize(slot + 1, None);
        }
        scoped[slot] = Some((revision, fresh.clone()));
        fresh
    }
}

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// Apply a validated kernel command: dispatch to the system that owns
    /// it. rules is needed by effects; scratch+derived by the march.
    pub fn apply_kernel(
        &mut self,
        resolved: &ResolvedKernel,
        rules: &CompiledRuleset<M>,
        scratch: &mut PathScratch,
        derived: &DerivedWorld<M>,
    ) -> Result<(), CommandError> {
        match *resolved {
            ResolvedKernel::MoveUnit { unit, to } => {
                // An explicit move replaces any move order of the unit.
                self.set_goal(unit, None);
                self.walk(unit, to, rules, scratch, derived, Watch::Ignore).map(|_| ())
            }
            ResolvedKernel::March { unit, to } => self.march(unit, to, rules, scratch, derived).map(|_| ()),
            ResolvedKernel::CancelOrder { unit } => self.cancel_order(unit),
            ResolvedKernel::FoundCity { settler, at, owner } => self.found_city(settler, at, owner, rules),
            ResolvedKernel::Fortify { unit } => self.fortify(unit),
            ResolvedKernel::BuildImprovement { unit, improvement, tile, work_time } => {
                self.start_improvement(unit, improvement, tile, work_time, rules)
            }
            ResolvedKernel::BuildRoad { unit, tile } => self.start_road(unit, tile, rules),
            ResolvedKernel::ChopForest { unit, tile, feature, work_time } => {
                self.start_chop(unit, tile, feature, work_time, rules)
            }
            ResolvedKernel::ChooseTechnology { player, technology } => self.choose_technology(player, technology),
            ResolvedKernel::PlanResearch { player, target, ref queue } => {
                self.apply_research_plan(player, target, queue, rules)
            }
            ResolvedKernel::EnqueueProduction { player, city, item } => self.enqueue_production(player, city, item),
            ResolvedKernel::ClearProduction { player, city } => self.clear_production(player, city),
            ResolvedKernel::HurryProduction { player, city } => self.hurry_production(player, city, rules),
            ResolvedKernel::PurchaseTile { player, city, tile } => self.purchase_tile(player, city, tile, rules),
            ResolvedKernel::ToggleWorkedTile { player, city, tile } => self.toggle_worked_tile(player, city, tile),
            ResolvedKernel::LockWorkedTile { player, city, tile } => self.lock_worked_tile(player, city, tile),
            ResolvedKernel::SetCityFocus { player, city, focus } => self.set_city_focus(player, city, focus),
            ResolvedKernel::SetSpecialists { player, city, specialist, count } => {
                self.set_specialists(player, city, specialist, count, rules)
            }
            ResolvedKernel::SetAvoidGrowth { player, city, avoid } => self.set_avoid_growth(player, city, avoid),
            ResolvedKernel::ChooseCityName { player, city, name } => self.choose_city_name(player, city, name, rules),
            ResolvedKernel::EndTurn { player } => {
                if self.world.turn.is_acting() {
                    return Err(CommandError::RuleViolation("a turn ends in the round's resolution".into()));
                }
                self.end_turn(player, rules, scratch, derived);
                Ok(())
            }
        }
    }
}
