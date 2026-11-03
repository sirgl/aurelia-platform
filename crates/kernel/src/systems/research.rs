//! Research: the player's queue of technologies and its completion.
//! Journal: PlayerChange::TechnologyChosen. Fires OnTechnologyUnlocked.

#[path = "research_plan.rs"]
mod planning;
pub use planning::{plan_research, ResearchPlanError};

use crate::command::CommandError;
use crate::ids::{PlayerId, TechId};
use crate::module::RulesModule;
use crate::rules::modifiers::Caps;
use crate::rules::CompiledRuleset;
use crate::systems::module::{KernelEvent, ModuleSystems};
use crate::transaction::journal::PlayerChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::player::LearnOutcome;

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    pub(crate) fn apply_research_plan(&mut self, player: PlayerId, target: TechId, queue: &[TechId], rules: &CompiledRuleset<M>) -> Result<(), CommandError> {
        if !self.world.players.is_alive(player) { return Err(CommandError::UnknownPlayer); }
        if self.world.active_player() != player { return Err(CommandError::RuleViolation("not your turn".into())); }
        let expected = plan_research(rules, self.world.players.techs_of(player), target)
            .map_err(|e| CommandError::RuleViolation(e.to_string()))?;
        if expected != queue { return Err(CommandError::RuleViolation("research plan changed".into())); }
        if !self.world.players.replace_research(player, expected) { return Err(CommandError::RuleViolation("research plan invalid".into())); }
        Ok(())
    }

    /// Join the queue (there is no instant learning, A2: beakers accumulate,
    /// completion is in `research`). The commit safety net mirrors validate:
    /// duplicates and overflow are precise errors, the world is untouched.
    pub(crate) fn choose_technology(
        &mut self,
        player: PlayerId,
        technology: TechId,
    ) -> Result<(), CommandError> {
        if self.world.players.techs_of(player).contains(&technology) {
            return Err(CommandError::RuleViolation("already researched".into()));
        }
        let queue = self.world.players.research_queue_of(player);
        if queue.contains(&technology) {
            return Err(CommandError::RuleViolation("already queued".into()));
        }
        if self.world.players.enqueue_research(player, technology) {
            Ok(())
        } else {
            Err(CommandError::RuleViolation("research queue full".into()))
        }
    }

    /// The empire's science of the turn from all cities (`cities_total`,
    /// the module's `ModuleSystems::science`), less the gold `deficit` of
    /// the turn, at least 0.
    pub(crate) fn collect_science(
        &mut self,
        player: PlayerId,
        cities_total: i32,
        deficit: i32,
        rules: &CompiledRuleset<M>,
    ) {
        let science = M::Systems::science(self.world, rules, player, cities_total);
        let science = (science + deficit.min(0)).max(0);
        self.world.players.earn_science(player, science as u32);
        let event = KernelEvent::ScienceCollected { player, science: science.max(0) as u32 };
        M::Systems::on_event(self, rules, event);
    }

    /// A player's research per turn: the queue head completes when the
    /// price is accumulated, overflow goes to the next. The loop terminates: the stockpile
    /// melts at prices > 0 (ZeroCost rejected, ModifyCost not below 1).
    pub fn research(&mut self, owner: PlayerId, rules: &CompiledRuleset<M>) {
        while let Some(head) = self.world.players.research_queue_of(owner).first().copied() {
            let Some(def) = rules.technologies.get(head.0 as usize) else {
                return;
            };
            if self.world.players.techs_of(owner).contains(&head) {
                // A duplicate in the queue (bypassing validate): silently consumed.
                self.world.players.pop_research(owner);
                continue;
            }
            // Queued prerequisites must actually be learned before science is spent.
            if !def.requires.iter().all(|t| self.world.players.techs_of(owner).contains(t)) { return; }
            let cost = M::Systems::research_cost(self.world, rules, owner, head);
            if !self.world.players.spend_science(owner, cost) {
                return;
            }
            self.world.players.pop_research(owner);
            if !self.learn(owner, head, rules) {
                return;
            }
        }
    }

    /// Learn and announce; false only for an unknown player.
    /// A technology of a later era fires OnEraEntered after its own
    /// OnTechnologyUnlocked; owed free technologies nothing fits any more
    /// are dropped.
    pub fn learn(&mut self, owner: PlayerId, tech: TechId, rules: &CompiledRuleset<M>) -> bool {
        let era_before = crate::world::player::player_era(self.world, rules, owner);
        match self.world.players.learn_tech(owner, tech) {
            LearnOutcome::Learned => {
                self.journal
                    .player_changes
                    .push(PlayerChange::TechnologyChosen { player: owner });
                let reveals = rules.technologies.get(tech.0 as usize).is_some_and(|d| d.map_visible);
                if reveals {
                    self.journal
                        .player_changes
                        .push(PlayerChange::MapRevealed { player: owner });
                }
                let new_era = crate::world::player::player_era(self.world, rules, owner) > era_before;
                let event = KernelEvent::TechnologyLearned { player: owner, tech, new_era };
                M::Systems::on_event(self, rules, event);
                true
            }
            LearnOutcome::AlreadyKnown => true,
            LearnOutcome::UnknownPlayer => false,
        }
    }

    /// Research cost after the player's ModifyCost(Technologies).
    pub fn technology_cost(&self, rules: &CompiledRuleset<M>, owner: PlayerId, base: u32) -> u32 {
        technology_cost(self.world, rules, owner, base)
    }
}

/// The same modified science cost used by paid research completion and UI.
pub fn technology_cost<M: RulesModule>(auth: &crate::world::state::AuthWorld<M>, rules: &CompiledRuleset<M>, owner: PlayerId, base: u32) -> u32 {
    let mods = crate::effects::player::player_modifiers(auth, rules, owner);
    let base = base.min(i32::MAX as u32) as i32;
    mods.tech_cost.evaluate(base, Caps::at_least(1)) as u32
}
