//! Execution of a player's end-turn sequence. The order is the rules
//! module's schedule (`RulesModule::SCHEDULE`; the Classic table with the
//! reason for each slot: `classic_rules::classic::turn`); the kernel steps run
//! here, every module step is a call into the module
//! (`ModuleSystems::run_step`), and no step decides on its own that "a turn
//! passed".
//!
//! Each city is resolved from one snapshot of its yields taken before any
//! of its steps (base values and modifiers do not shift mid-city). Gold and
//! science of all cities are summed, then the player-level modifiers apply
//! once. Journal: PlayerChange::TurnEnded (+ what the steps write).

use crate::derived::yields::CityBreakdown;
use crate::derived::DerivedWorld;
use crate::ids::{CityHandle, PlayerId, TileId, UnitHandle};
use crate::module::RulesModule;
use crate::navigation::PathScratch;
use crate::rules::CompiledRuleset;
use crate::systems::module::{CityGrowth, KernelEvent, ModuleSystems};
use crate::transaction::journal::{CityChange, PlayerChange};
use crate::transaction::WorldTransaction;
use crate::turn::resolution::{KernelCityStep, KernelStep, StartStep, Step, TurnLedger, TURN_START_SEQUENCE};
use crate::turn::{TurnPhase, TurnState};
use crate::world::city::{CityCore, CityWrite};

/// One city during its resolution: the snapshot and the running population.
pub struct CityTurn<M: RulesModule> {
    pub city: CityHandle,
    pub tile: TileId,
    pub owner: PlayerId,
    /// The population when the snapshot was taken.
    pub population: u32,
    /// The population after the steps so far.
    pub population_now: u32,
    pub yields: CityBreakdown<M>,
    /// The head of the queue is a food unit (settler): the surplus goes to
    /// production, the store does not move.
    pub producing_food: bool,
    /// Gold and science of the city's process this turn.
    pub process_yield: (i32, i32),
}

/// What a module step of a player's end turn reads.
pub struct EndTurnInput<'t, M: RulesModule> {
    pub player: PlayerId,
    pub rules: &'t CompiledRuleset<M>,
    pub derived: &'t DerivedWorld<M>,
    /// What the kernel steps before this one collected.
    pub ledger: &'t TurnLedger,
}

/// What a module step of one city's resolution reads.
pub struct CityStepInput<'t, M: RulesModule> {
    pub owner: PlayerId,
    pub city: CityHandle,
    /// The city's yields, taken once before its first step.
    pub yields: &'t CityBreakdown<M>,
    pub rules: &'t CompiledRuleset<M>,
}

/// The head of the queue is a unit with the food ability (settler): the
/// city converts its surplus into production and neither grows nor starves.
pub fn producing_food<M: RulesModule>(rules: &CompiledRuleset<M>, core: &CityCore) -> bool {
    match core.queue.first() {
        Some(crate::ids::ProductionItem::Unit(kind)) => rules
            .units
            .get(kind.0 as usize)
            .map(|u| crate::rules::rows::UnitExtension::built_with_food(&u.module))
            .unwrap_or(false),
        _ => false,
    }
}

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    pub fn end_turn(
        &mut self,
        player: PlayerId,
        rules: &CompiledRuleset<M>,
        scratch: &mut PathScratch,
        derived: &DerivedWorld<M>,
    ) {
        let mut ledger = TurnLedger {
            timed_before: self.world.effects.len(),
            units: self.world.indexes.units_of(player).to_vec(),
            gold: 0,
            science: 0,
            deficit: 0,
            culture: 0,
            module_yield: 0,
        };
        let _span = prof::span("turn/end_turn");
        for &(phase, step) in M::SCHEDULE.end_turn {
            self.world.turn.enter(phase);
            match step {
                Step::Kernel(step) => {
                    let _span = prof::span_debug("turn/step/", &step);
                    self.run_kernel_step(step, player, rules, &mut ledger)
                }
                Step::Module(step) => {
                    let _span = prof::span_debug("turn/step/", &step);
                    let input = EndTurnInput { player, rules, derived, ledger: &ledger };
                    M::Systems::run_step(self, step, &input);
                }
            }
        }
        self.journal
            .player_changes
            .push(PlayerChange::TurnEnded { player });
        self.pass_turn(player, rules);
        let _span = prof::span("turn/start_turn");
        self.start_turn(rules, scratch, derived);
    }

    /// The new active player's turn begins (TURN_START_SEQUENCE).
    fn start_turn(&mut self, rules: &CompiledRuleset<M>, scratch: &mut PathScratch, derived: &DerivedWorld<M>) {
        let player = self.world.active_player();
        for step in TURN_START_SEQUENCE {
            self.world.turn.enter(TurnPhase::Start);
            match step {
                StartStep::Fortification => self.advance_fortification(player),
                StartStep::MoveOrders => self.continue_move_orders(player, rules, scratch, derived),
            }
        }
        self.world.turn.enter(TurnPhase::PlayerActions);
    }

    /// The next alive player becomes active; after the last one the
    /// module's round end runs and a new round begins.
    pub(crate) fn pass_turn(&mut self, current: PlayerId, rules: &CompiledRuleset<M>) {
        let players = &self.world.players;
        let next = TurnState::next_after(current, players.count(), |p| players.is_alive(p));
        if let crate::turn::state::NextTurn::NewRound(_) = next {
            let _span = prof::span("turn/end_round");
            M::Systems::end_round(self, rules);
        }
        self.world.turn.pass_to(next);
    }

    fn run_kernel_step(
        &mut self,
        step: KernelStep,
        player: PlayerId,
        rules: &CompiledRuleset<M>,
        ledger: &mut TurnLedger,
    ) {
        match step {
            KernelStep::HealUnits => self.heal_units(&ledger.units, rules),
            KernelStep::RestoreMoves => self.restore_moves(player, &ledger.units, rules),
            KernelStep::CapitalConnections => self.update_capital_connections(player, rules),
            KernelStep::TurnStartEffects => {
                M::Systems::on_event(self, rules, KernelEvent::TurnStarted { player })
            }
            KernelStep::Cities => self.resolve_cities(player, rules, ledger),
            KernelStep::Gold => ledger.deficit = self.collect_gold(player, ledger.gold, rules),
            KernelStep::Science => self.collect_science(player, ledger.science, ledger.deficit, rules),
            KernelStep::Research => self.research(player, rules),
            KernelStep::WorkerJobs => self.work_progress(player, rules),
            KernelStep::TimedEffects => self.tick_timed_effects(player, ledger.timed_before),
        }
    }

    /// Cities in list order; each runs the city schedule on its own snapshot.
    fn resolve_cities(
        &mut self,
        owner: PlayerId,
        rules: &CompiledRuleset<M>,
        ledger: &mut TurnLedger,
    ) {
        let cities = match self.world.indexes.cities_by_player.get(&owner.0) {
            Some(cities) => cities.clone(),
            None => Vec::new(),
        };
        for city in cities {
            let snapshot = {
                let _span = prof::span("turn/city_snapshot");
                self.city_snapshot(city, rules)
            };
            let Some(mut turn) = snapshot else {
                continue;
            };
            ledger.gold += turn.yields.rates.gold;
            ledger.science += turn.yields.rates.science;
            ledger.module_yield = ledger.module_yield.saturating_add(turn.yields.rates.module.max(0) as u32);
            for &step in M::SCHEDULE.city {
                let step = match step {
                    Step::Kernel(step) => step,
                    Step::Module(step) => {
                        let _span = prof::span_debug("turn/city_step/", &step);
                        let input = CityStepInput { owner, city, yields: &turn.yields, rules };
                        M::Systems::run_city_step(self, step, &input);
                        continue;
                    }
                };
                let _span = prof::span_debug("turn/city_step/", &step);
                if self.run_city_step(step, &mut turn, rules, ledger).is_none() {
                    break;
                }
            }
            ledger.gold += turn.process_yield.0;
            ledger.science += turn.process_yield.1;
        }
    }

    fn city_snapshot(&self, city: CityHandle, rules: &CompiledRuleset<M>) -> Option<CityTurn<M>> {
        let core = self.world.cities.get(city)?;
        let yields = crate::derived::yields::city_breakdown(self.world, rules, core);
        Some(CityTurn {
            city,
            tile: core.tile,
            owner: core.owner,
            population: core.population,
            population_now: core.population,
            yields,
            producing_food: producing_food(rules, core),
            process_yield: (0, 0),
        })
    }

    /// None: the city is gone (its remaining steps are skipped).
    fn run_city_step(
        &mut self,
        step: KernelCityStep,
        turn: &mut CityTurn<M>,
        rules: &CompiledRuleset<M>,
        ledger: &mut TurnLedger,
    ) -> Option<()> {
        let owner = turn.owner;
        let mods = turn.yields.modifiers.clone();
        match step {
            KernelCityStep::Growth => {
                let growth = M::Systems::city_growth(self.world, rules, turn);
                turn.population_now = self.grow_city(turn.city, growth)?;
            }
            KernelCityStep::Production => {
                // Classic has no default order: a city with an empty queue and
                // no process builds nothing; its production waits in the stock.
                let mut production = turn.yields.rates.production.max(0);
                if turn.producing_food {
                    production += M::Systems::food_production(turn);
                }
                let production = production.max(0) as u32;
                let core = self.world.cities.get(turn.city)?;
                if let (true, Some(process)) = (core.queue.is_empty(), core.economy.process) {
                    turn.process_yield = M::Systems::run_process(self, owner, turn.city, process, production);
                    return Some(());
                }
                if let CityWrite::TargetGone = self.world.cities.add_production(turn.city, production) {
                    return None;
                }
                self.drain_queue(owner, turn.city, turn.tile, rules, &mods);
            }
            KernelCityStep::Borders => {
                let culture = turn.yields.rates.culture.max(0) as u32;
                ledger.culture = ledger.culture.saturating_add(culture);
                self.expand_borders(owner, turn.city, turn.tile, culture, rules, &mods)?;
            }
        }
        Some(())
    }

    /// One turn of growth: the difference fills the
    /// store; at the threshold the city grows by one citizen and keeps its
    /// kept food; below zero it starves (one citizen lost, never below 1).
    /// Returns the population after it (None: the city is gone). Journal:
    /// CityChange::PopulationChanged.
    pub fn grow_city(&mut self, city: CityHandle, growth: CityGrowth) -> Option<u32> {
        let CityGrowth { difference_times100, kept_percent, threshold } = growth;
        let core = self.world.cities.get_mut(city)?;
        let store = i64::from(core.food_times100) + difference_times100;
        let kept = i64::from(core.economy.food_kept) + difference_times100 / 100;
        let kept_max = i64::from(threshold) * i64::from(kept_percent) / 100;
        core.economy.food_kept = kept.clamp(0, kept_max) as u32;
        let changed = if store >= i64::from(threshold) * 100 && core.citizens.avoid_growth {
            // «Avoid Growth»: the store stays full, the city keeps its size.
            core.food_times100 = (i64::from(threshold) * 100) as u32;
            false
        } else if store >= i64::from(threshold) * 100 {
            let used = (i64::from(threshold) - i64::from(core.economy.food_kept)).max(0);
            core.food_times100 = (store - used * 100).max(0) as u32;
            core.population += 1;
            true
        } else if store < 0 {
            core.food_times100 = 0;
            let starved = core.population > 1;
            if starved {
                core.population -= 1;
            }
            starved
        } else {
            core.food_times100 = store as u32;
            false
        };
        let population = core.population;
        if changed {
            self.journal
                .city_changes
                .push(CityChange::PopulationChanged { city });
        }
        Some(population)
    }

    /// Movement points for the new turn (O(own units)). The maximum is
    /// re-evaluated: ModifyMovement may have changed since, and a land unit
    /// on water has its embarked allowance (classic/docs/classic/rules.md).
    fn restore_moves(&mut self, player: PlayerId, handles: &[UnitHandle], rules: &CompiledRuleset<M>) {
        for h in handles {
            if let Some(core) = self.world.units.get_mut_core(*h) {
                core.attacks_made = 0;
                core.interceptions_made = 0;
            }
        }
        if !rules.effect_index.has_passive_unit {
            // No unit modifier exists: the stored maximum is the content
            // value (or the embarked one, set when the unit took to the water).
            for h in handles {
                if let Some(core) = self.world.units.get_mut_core(*h) {
                    core.moves_left = core.moves_max;
                }
            }
            return;
        }
        let owner_part = crate::effects::unit::owner_unit_modifiers(self.world, rules, player);
        for h in handles {
            let Some(view) = self.world.units.get(*h) else {
                continue;
            };
            let unit = *view.core;
            let mods = crate::effects::unit::with_own_sources(self.world, rules, owner_part.clone(), &unit);
            let max = mods.moves_on(rules, &self.world.map, unit.kind, unit.tile);
            if let Some(core) = self.world.units.get_mut_core(*h) {
                core.moves_max = max;
                core.moves_left = max;
            }
        }
    }

    /// End of the owner's resolution: tick timed effects that existed when
    /// the sequence started (`existing`), journal expiry.
    pub(crate) fn tick_timed_effects(&mut self, owner: PlayerId, existing: usize) {
        if self.world.effects.tick(owner, existing) {
            self.journal
                .player_changes
                .push(PlayerChange::EffectsChanged { player: owner });
        }
    }
}
