//! The AI turn driver (PLAN M6 stage C, R-07 AI §4): a step-wise state
//! machine over the AI players' turns.
//!
//! ```text
//! Idle → PlayerStart → Observe → PlanStrategic → PlanNext ⇄ Execute
//!      → PlayerEnd → NextPlayer → (PlayerStart | RoundDone → Idle)
//! ```
//!
//! One `step` does one bounded piece of work: build one observation, make
//! one decision, or execute one command. Every decision reads a fresh
//! observation of its own boundary (the full one at Observe and at the end
//! of the turn, a local one around the unit otherwise) and every command
//! goes through `GameState::execute`, the same boundary as a human's.
//!
//! Two budgets (R-07 §4): the deterministic one lives in the decisions
//! (`DecisionBudget`: search nodes, candidates, commands per unit,
//! decisions per turn) and is the same on every machine; the wall-time one
//! belongs to the host, which calls `step` until its frame slice is spent.
//! Stopping between steps changes nothing: the next step continues from the
//! same machine state, so any slicing gives the same commands and hashes.
//!
//! The driver is the one writer of the AI memory (`AiStates`), saved with
//! the game at player boundaries (`at_player_boundary`). The decisions are
//! the rules module's planners' ([`AiMind`]).

use std::collections::VecDeque;

use crate::ai::map::MapScratch;
use crate::ai::mind::{AiMind, TurnKind};
use crate::ai::plan::{role_of, AiRole, AiSettings, Decision, PlanTools};
use crate::ai::state::AiStates;
use crate::ai::trace::{Actor, AiCounters, DecisionTrace, Outcome, Reason, TraceEntry};
use crate::ai::{Advance, AiPlayers, DriverPhase, StepOutcome};
use crate::command::CommandError;
use crate::ids::{EntityId, PlayerId, UnitHandle, UnitKindId};
use crate::module::RulesModule;
use crate::persistence::ai::{decode_ai, encode_ai, SavedAiStates};
use crate::persistence::decode::DecodeError;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{AppliedCommand, CommandResult};
use crate::world::state::GameState;

/// What a run of steps did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunReport {
    pub steps: u32,
    pub players_done: Vec<PlayerId>,
    pub round_done: bool,
    pub waiting: bool,
}

/// Transient state of one AI player's turn (never saved: it is empty at
/// every player boundary).
#[derive(Debug)]
struct PlayerTurn<M: RulesModule, B> {
    player: PlayerId,
    brief: Option<B>,
    round: u32,
    /// The system advisors' start-of-turn decisions (from the Observe
    /// observation), executed after the strategic ones.
    opening: Vec<Decision<M>>,
    pending: VecDeque<Decision<M>>,
    current: Option<Decision<M>>,
    /// Units in decision order: civilians first (an escort follows its
    /// settler), then by id; a deferred unit moves to the end.
    order: Vec<(EntityId, UnitHandle)>,
    /// Sorted.
    done: Vec<EntityId>,
    /// Deferrals granted per unit this turn, sorted by unit.
    deferred: Vec<(EntityId, u32)>,
    /// Commands per unit this turn, sorted by unit.
    commands: Vec<(EntityId, u32)>,
    decisions: u32,
    limit: u32,
    cities_planned: bool,
    kind: TurnKind,
    /// The mind picks the next unit itself (`AiMind::picks_next`).
    picks: bool,
}

impl<M: RulesModule, B> PlayerTurn<M, B> {
    fn new(player: PlayerId, round: u32) -> Self {
        Self {
            player,
            brief: None,
            round,
            opening: Vec::new(),
            pending: VecDeque::new(),
            current: None,
            order: Vec::new(),
            done: Vec::new(),
            deferred: Vec::new(),
            commands: Vec::new(),
            decisions: 0,
            limit: 0,
            cities_planned: false,
            kind: TurnKind::Full,
            picks: false,
        }
    }

    fn next_unit(&self) -> Option<(EntityId, UnitHandle)> {
        self.order
            .iter()
            .find(|(id, _)| self.done.binary_search(id).is_err())
            .copied()
    }

    /// The units not done, in the queue's order.
    fn remaining(&self) -> Vec<(EntityId, UnitHandle)> {
        self.order.iter().filter(|(id, _)| self.done.binary_search(id).is_err()).copied().collect()
    }

    /// `unit` to the front of the queue (it decides next); false when it is
    /// not in the queue or done.
    fn bring_forward(&mut self, unit: EntityId) -> bool {
        if self.done.binary_search(&unit).is_ok() {
            return false;
        }
        let Some(pos) = self.order.iter().position(|(id, _)| *id == unit) else {
            return false;
        };
        let entry = self.order.remove(pos);
        self.order.insert(0, entry);
        true
    }

    fn mark_done(&mut self, unit: EntityId) {
        if let Err(at) = self.done.binary_search(&unit) {
            self.done.insert(at, unit);
        }
    }

    /// The unit to the end of the queue, if it has a deferral left this
    /// turn (`DEFERS_PER_UNIT`, `REORDERS_PER_UNIT` when the mind picks the
    /// next unit); false: refused, nothing changed.
    fn defer(&mut self, unit: EntityId) -> bool {
        let limit = if self.picks { REORDERS_PER_UNIT } else { DEFERS_PER_UNIT };
        let at = self.deferred.binary_search_by_key(&unit, |d| d.0);
        if at.is_ok_and(|at| self.deferred[at].1 >= limit) {
            return false;
        }
        let Some(pos) = self.order.iter().position(|(id, _)| *id == unit) else {
            return false;
        };
        match at {
            Ok(at) => self.deferred[at].1 += 1,
            Err(at) => self.deferred.insert(at, (unit, 1)),
        }
        let entry = self.order.remove(pos);
        self.order.push(entry);
        true
    }

    /// Count one more command of the unit; returns the new count.
    fn count_command(&mut self, unit: EntityId) -> u32 {
        match self.commands.binary_search_by_key(&unit, |c| c.0) {
            Ok(at) => {
                self.commands[at].1 += 1;
                self.commands[at].1
            }
            Err(at) => {
                self.commands.insert(at, (unit, 1));
                1
            }
        }
    }
}

/// The AI of every non-human player of one game, deciding with the
/// module's planners `P`.
#[derive(Debug)]
pub struct AiDriver<M: RulesModule, P: AiMind<M>> {
    settings: AiSettings,
    humans: Vec<PlayerId>,
    states: AiStates,
    phase: DriverPhase,
    turn: Option<PlayerTurn<M, P::Brief>>,
    last_round: u32,
    scratch: MapScratch,
    mind: P,
    counters: AiCounters,
    trace: DecisionTrace<M>,
    log: Option<Vec<M::Command>>,
    /// Applied commands with their journals, kept for the host while it
    /// records them (its presentation); never read by the AI.
    journal: Option<Vec<AppliedCommand<M>>>,
    /// The command handed to the host by `advance`, until `executed` brings
    /// its result back.
    awaiting: Option<Awaiting<M>>,
}

/// A command the host is executing for the driver.
#[derive(Debug)]
struct Awaiting<M: RulesModule> {
    round: u32,
    player: PlayerId,
    decision: Decision<M>,
    command: M::Command,
    /// The end of the player's turn (not a decision of a unit or a player).
    end_turn: bool,
}

impl<M: RulesModule, P: AiMind<M> + Default> AiDriver<M, P> {
    /// A driver for the players not in `humans`.
    pub fn new(settings: AiSettings, humans: Vec<PlayerId>) -> Self {
        Self::with_mind(settings, humans, P::default())
    }
}

impl<M: RulesModule, P: AiMind<M>> AiDriver<M, P> {
    /// A driver deciding with `mind` for the players not in `humans`.
    pub fn with_mind(settings: AiSettings, mut humans: Vec<PlayerId>, mind: P) -> Self {
        humans.sort();
        humans.dedup();
        let mut driver = Self {
            settings,
            humans,
            states: AiStates::default(),
            phase: DriverPhase::Idle,
            turn: None,
            last_round: 0,
            scratch: MapScratch::default(),
            mind,
            counters: AiCounters::default(),
            trace: DecisionTrace::default(),
            log: None,
            journal: None,
            awaiting: None,
        };
        driver.mind.note_humans(&driver.humans);
        driver
    }

    pub fn phase(&self) -> DriverPhase {
        self.phase
    }

    pub fn settings(&self) -> &AiSettings {
        &self.settings
    }

    pub fn is_human(&self, player: PlayerId) -> bool {
        self.humans.binary_search(&player).is_ok()
    }

    /// Who plays themselves from now on. The memory of a player the AI
    /// takes over starts fresh (`AiStates::get_or_create`).
    pub fn set_humans(&mut self, mut humans: Vec<PlayerId>) {
        debug_assert!(self.turn.is_none(), "the AI turn in progress is not handed over");
        humans.sort();
        humans.dedup();
        self.humans = humans;
        self.mind.note_humans(&self.humans);
    }

    /// No AI player is in the middle of its turn: the memory is complete
    /// and the game may be saved.
    pub fn at_player_boundary(&self) -> bool {
        self.turn.is_none()
    }

    pub fn states(&self) -> &AiStates {
        &self.states
    }

    /// The AI memory, for a scene that gives a player a plan it would form
    /// by itself only after many rounds (tests and probes).
    #[cfg(any(test, feature = "harness"))]
    pub fn state_mut(&mut self, player: PlayerId, round: u32) -> &mut crate::ai::state::AiState {
        self.states.get_or_create(player, round)
    }

    pub fn counters(&self) -> &AiCounters {
        &self.counters
    }

    pub fn trace(&self) -> &DecisionTrace<M> {
        &self.trace
    }

    /// Keep the newest `capacity` trace entries (0: totals only).
    pub fn set_trace_capacity(&mut self, capacity: usize) {
        self.trace = DecisionTrace::with_capacity(capacity);
    }

    /// The module's planners (a module plugs its parts in here).
    pub fn mind_mut(&mut self) -> &mut P {
        &mut self.mind
    }

    /// Record every applied AI command (tests, replay).
    pub fn record_commands(&mut self) {
        self.log = Some(Vec::new());
    }

    pub fn commands(&self) -> &[M::Command] {
        self.log.as_deref().unwrap_or(&[])
    }

    /// Keep every applied AI command with its journal until `take_journal`
    /// (the host's presentation). Recording changes no decision.
    pub fn record_journal(&mut self) {
        self.journal.get_or_insert_with(Vec::new);
    }

    pub fn records_journal(&self) -> bool {
        self.journal.is_some()
    }

    /// The commands applied since the last call, in order.
    pub fn take_journal(&mut self) -> Vec<AppliedCommand<M>> {
        match self.journal.as_mut() {
            Some(journal) => std::mem::take(journal),
            None => Vec::new(),
        }
    }

    /// The AI memory in its save form (call at a player boundary).
    pub fn save(&self) -> SavedAiStates {
        encode_ai(&self.states, &self.settings.difficulty)
    }

    /// Restore the memory of a loaded game; the difficulty comes with it.
    pub fn load(
        &mut self,
        saved: &SavedAiStates,
        tiles: usize,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), DecodeError> {
        let states = decode_ai(saved, tiles)?;
        let budget = self.settings.budget;
        let difficulty = if saved.difficulty.is_empty() {
            self.settings.difficulty.clone()
        } else {
            saved.difficulty.clone()
        };
        self.settings = AiSettings::for_rules(rules, &difficulty);
        self.settings.budget = budget;
        self.states = states;
        self.phase = DriverPhase::Idle;
        self.turn = None;
        Ok(())
    }

    /// Run at most `max_steps` steps; stops early when there is nothing to
    /// do or the round's AI turns are over.
    /// A game the driver plays and nobody configured yet gets its setup:
    /// the driver's difficulty, the players not in `humans` played by the
    /// AI (they then play with the AI handicap row, M4-A). A configured game
    /// is left as it is.
    pub fn configure_game(&self, game: &mut GameState<M>) {
        if !game.view().auth.settings().ai.is_empty() {
            return;
        }
        let players = game.view().auth.players().count();
        let ai: Vec<PlayerId> = (0..players as u8)
            .map(PlayerId)
            .filter(|p| !self.is_human(*p))
            .collect();
        game.configure_players(&self.settings.difficulty, &ai);
    }

    pub fn run(
        &mut self,
        game: &mut GameState<M>,
        rules: &CompiledRuleset<M>,
        max_steps: u32,
    ) -> RunReport {
        let mut report = RunReport::default();
        while report.steps < max_steps {
            let outcome = self.step(game, rules);
            report.steps += 1;
            match outcome {
                StepOutcome::Worked => {}
                StepOutcome::PlayerDone(player) => report.players_done.push(player),
                StepOutcome::RoundDone => {
                    report.round_done = true;
                    return report;
                }
                StepOutcome::Waiting => {
                    report.waiting = true;
                    return report;
                }
            }
        }
        report
    }

    /// One step of the machine: `advance`, and the command it asks for goes
    /// through `GameState::execute` on the spot.
    pub fn step(&mut self, game: &mut GameState<M>, rules: &CompiledRuleset<M>) -> StepOutcome {
        self.configure_game(game);
        match self.advance(game, rules) {
            Advance::Worked => StepOutcome::Worked,
            Advance::RoundDone => StepOutcome::RoundDone,
            Advance::Waiting => StepOutcome::Waiting,
            Advance::Command(command) | Advance::EndTurn(command) => {
                let result = game.execute(rules, command);
                self.executed(result)
            }
        }
    }

    /// The host passed the turn on to someone the AI does not play (see
    /// `AiPlayers::turns_passed`).
    pub fn turns_passed(&mut self) {
        if self.turn.is_none() && self.awaiting.is_none() {
            self.phase = DriverPhase::Idle;
        }
    }

    /// One step of the machine that only reads the game: when the next piece of
    /// work is a command, it is handed to the caller, who executes it through
    /// `GameState::execute` (or `execute_as`) and gives the result to
    /// `executed` before the next call. The setup of a game nobody configured
    /// (`configure_game`) is the caller's, before the first call.
    pub fn advance(&mut self, game: &GameState<M>, rules: &CompiledRuleset<M>) -> Advance<M::Command> {
        debug_assert!(self.awaiting.is_none(), "the result of the last command is still owed");
        let outcome = match self.phase {
            DriverPhase::Idle => self.idle(game),
            DriverPhase::PlayerStart => self.player_start(game),
            DriverPhase::Observe => self.observe(game, rules),
            DriverPhase::PlanStrategic => self.plan_strategic(rules),
            DriverPhase::PlanNext => self.plan_next(game, rules),
            DriverPhase::Execute => {
                return match self.take_decision_command(game, rules) {
                    Some(command) => Advance::Command(command),
                    None => Advance::Worked,
                }
            }
            DriverPhase::PlayerEnd => {
                return match self.take_end_turn(game) {
                    Some(command) => Advance::EndTurn(command),
                    None => Advance::Worked,
                }
            }
            DriverPhase::NextPlayer => self.next_player(game),
            DriverPhase::RoundDone => {
                self.phase = DriverPhase::Idle;
                StepOutcome::RoundDone
            }
        };
        match outcome {
            StepOutcome::Worked | StepOutcome::PlayerDone(_) => Advance::Worked,
            StepOutcome::RoundDone => Advance::RoundDone,
            StepOutcome::Waiting => Advance::Waiting,
        }
    }

    fn ai_to_act(&self, game: &GameState<M>) -> Option<PlayerId> {
        if game.is_over() {
            return None;
        }
        let auth = game.view().auth;
        let active = auth.active_player();
        if self.is_human(active) || !auth.players().is_alive(active) {
            return None;
        }
        Some(active)
    }

    fn idle(&mut self, game: &GameState<M>) -> StepOutcome {
        if self.ai_to_act(game).is_none() {
            return StepOutcome::Waiting;
        }
        self.phase = DriverPhase::PlayerStart;
        StepOutcome::Worked
    }

    fn player_start(&mut self, game: &GameState<M>) -> StepOutcome {
        let Some(player) = self.ai_to_act(game) else {
            self.phase = DriverPhase::Idle;
            return StepOutcome::Worked;
        };
        let round = game.view().auth.turn().round;
        self.turn = Some(PlayerTurn::new(player, round));
        self.last_round = round;
        self.counters.player_turns += 1;
        self.phase = DriverPhase::Observe;
        StepOutcome::Worked
    }

    fn observe(&mut self, game: &GameState<M>, rules: &CompiledRuleset<M>) -> StepOutcome {
        let Some(turn) = self.turn.as_mut() else {
            self.phase = DriverPhase::Idle;
            return StepOutcome::Worked;
        };
        self.counters.observations += 1;
        let state = self.states.get_or_create(turn.player, turn.round);
        let _span = prof::span("ai/observe (mind.start_turn)");
        let sight = self.mind.sense(game, rules, turn.player);
        let start = self.mind.start_turn(&sight, rules, turn.player, state);
        let budget = self.settings.budget;
        turn.limit = budget.decisions_base + 4 * start.units.len() as u32 + 2 * start.cities;
        turn.order = decision_order(&start.units, rules);
        self.mind.order_units(&start.brief, rules, &mut turn.order);
        turn.picks = self.mind.picks_next(&start.brief);
        turn.opening = start.opening;
        turn.kind = start.kind;
        if turn.kind == TurnKind::Scripted {
            turn.order.clear();
            turn.cities_planned = true;
        }
        turn.brief = Some(start.brief);
        self.phase = DriverPhase::PlanStrategic;
        StepOutcome::Worked
    }

    fn plan_strategic(&mut self, rules: &CompiledRuleset<M>) -> StepOutcome {
        let Some(turn) = self.turn.as_mut() else {
            self.phase = DriverPhase::Idle;
            return StepOutcome::Worked;
        };
        let Some(brief) = turn.brief.as_ref() else {
            self.phase = DriverPhase::PlayerEnd;
            return StepOutcome::Worked;
        };
        let state = self.states.get_or_create(turn.player, turn.round);
        let _span = prof::span("ai/plan_strategic");
        let mut decisions = match turn.kind {
            TurnKind::Full => self.mind.plan_strategic(brief, state, rules),
            TurnKind::NoStrategy | TurnKind::Scripted => Vec::new(),
        };
        decisions.append(&mut turn.opening);
        let round = turn.round;
        queue_decisions(turn, &mut self.trace, round, decisions);
        self.phase = DriverPhase::PlanNext;
        StepOutcome::Worked
    }

    fn plan_next(&mut self, game: &GameState<M>, rules: &CompiledRuleset<M>) -> StepOutcome {
        let Some(turn) = self.turn.as_mut() else {
            self.phase = DriverPhase::Idle;
            return StepOutcome::Worked;
        };
        let round = turn.round;
        if turn.decisions >= turn.limit {
            let entry = refusal(
                round,
                turn.player,
                Actor::Player,
                Reason::TurnBudgetExhausted,
            );
            self.trace.push(entry);
            self.phase = DriverPhase::PlayerEnd;
            return StepOutcome::Worked;
        }
        turn.decisions += 1;
        self.counters.decisions += 1;
        if let Some(decision) = turn.pending.pop_front() {
            turn.current = Some(decision);
            self.phase = DriverPhase::Execute;
            return StepOutcome::Worked;
        }
        if turn.picks {
            self.pick_next_unit(game, rules);
        }
        let Some(turn) = self.turn.as_mut() else {
            self.phase = DriverPhase::Idle;
            return StepOutcome::Worked;
        };
        if let Some((id, handle)) = turn.next_unit() {
            self.decide_unit(game, rules, id, handle);
            return StepOutcome::Worked;
        }
        if !turn.cities_planned {
            turn.cities_planned = true;
            self.plan_end_of_turn(game, rules);
            return StepOutcome::Worked;
        }
        self.phase = DriverPhase::PlayerEnd;
        StepOutcome::Worked
    }

    /// "Who is next" (`AiMind::next_unit`): the mind's answer, from a fresh
    /// observation around the queue's head, goes to the front of the queue.
    /// Asked only of a mind that `picks_next`, when two units or more remain.
    fn pick_next_unit(&mut self, game: &GameState<M>, rules: &CompiledRuleset<M>) {
        let Self { settings, states, turn, mind, .. } = self;
        let Some(turn) = turn.as_mut() else { return };
        let remaining = turn.remaining();
        if remaining.len() < 2 {
            return;
        }
        let Some(brief) = turn.brief.as_ref() else { return };
        let state = states.get_or_create(turn.player, turn.round);
        let answer = {
            let _span = prof::span("ai/next_unit");
            let Some(near) = mind.sense_unit(game, rules, settings, turn.player, remaining[0].1) else { return };
            mind.next_unit(&near, brief, state, &remaining)
        };
        if let Some(unit) = answer.filter(|u| *u != remaining[0].0) {
            turn.bring_forward(unit);
        }
    }

    /// One decision of one unit: a command goes to Execute; no command ends
    /// the unit's turn (traced); a unit that is gone is skipped.
    fn decide_unit(
        &mut self,
        game: &GameState<M>,
        rules: &CompiledRuleset<M>,
        id: EntityId,
        handle: UnitHandle,
    ) {
        let decision = {
            let _span = prof::span("ai/decide_unit");
            self.unit_decision(game, rules, handle)
        };
        let Some(turn) = self.turn.as_mut() else {
            return;
        };
        let Some(decision) = decision else {
            turn.mark_done(id);
            return;
        };
        if decision.command.is_some() {
            turn.current = Some(decision);
            self.phase = DriverPhase::Execute;
            return;
        }
        let round = game.view().auth.turn().round;
        let entry = refusal(round, turn.player, decision.actor, decision.reason);
        self.trace.push(entry);
        // A deferral the unit still has: it decides again after the queue.
        if decision.reason == Reason::Deferred && turn.defer(id) {
            return;
        }
        turn.mark_done(id);
    }

    /// The planner's decision for a unit from a fresh local observation
    /// (None when the unit is gone).
    fn unit_decision(
        &mut self,
        game: &GameState<M>,
        rules: &CompiledRuleset<M>,
        handle: UnitHandle,
    ) -> Option<Decision<M>> {
        let Self {
            settings,
            states,
            turn,
            scratch,
            counters,
            mind,
            ..
        } = self;
        let turn = turn.as_ref()?;
        let brief = turn.brief.as_ref()?;
        let state = states.get_or_create(turn.player, turn.round);
        let mut tools = PlanTools {
            rules,
            settings,
            scratch,
            counters,
        };
        let near = mind.sense_unit(game, rules, tools.settings, turn.player, handle)?;
        tools.counters.local_observations += 1;
        mind.decide_unit(&near, turn.player, handle, brief, state, &mut tools)
    }

    /// Production and the end-of-turn decisions from a fresh full
    /// observation.
    fn plan_end_of_turn(&mut self, game: &GameState<M>, rules: &CompiledRuleset<M>) {
        let Self {
            settings,
            states,
            turn,
            scratch,
            counters,
            trace,
            mind,
            ..
        } = self;
        let Some(turn) = turn.as_mut() else {
            return;
        };
        let Some(brief) = turn.brief.as_ref() else {
            return;
        };
        counters.observations += 1;
        let state = states.get_or_create(turn.player, turn.round);
        let mut tools = PlanTools {
            rules,
            settings,
            scratch,
            counters,
        };
        let decisions = {
            let _span = prof::span("ai/plan_end_of_turn");
            let sight = mind.sense(game, rules, turn.player);
            mind.plan_end_of_turn(&sight, turn.player, brief, state, &mut tools)
        };
        let round = turn.round;
        queue_decisions(turn, trace, round, decisions);
    }

    /// The command of the decision to execute (None: nothing to execute).
    fn take_decision_command(&mut self, game: &GameState<M>, rules: &CompiledRuleset<M>) -> Option<M::Command> {
        self.phase = DriverPhase::PlanNext;
        let Some(turn) = self.turn.as_mut() else {
            self.phase = DriverPhase::Idle;
            return None;
        };
        let decision = turn.current.take()?;
        let command = decision.command.clone()?;
        if !self.mind.permits(game, rules, turn.player, &command) {
            // Planned against an earlier state of the turn and refused by
            // the game as it is now: not sent. A unit's task counts it as a
            // failed attempt, as a refusal would.
            let (round, player, actor) = (turn.round, turn.player, decision.actor);
            self.counters.orders_stale += 1;
            self.trace.push(TraceEntry {
                round,
                player,
                actor,
                reason: Reason::Forbidden,
                command: Some(command),
                outcome: Outcome::NoCommand,
            });
            if let Actor::Unit(unit) = actor {
                self.after_unit_command(round, player, unit, false);
            }
            return None;
        }
        self.awaiting = Some(Awaiting {
            round: game.view().auth.turn().round,
            player: turn.player,
            decision,
            command: command.clone(),
            end_turn: false,
        });
        Some(command)
    }

    /// The result of the command `advance` handed out: the journal, the
    /// trace and the unit's budget follow it. `PlayerDone` after the end of a
    /// player's turn.
    pub fn executed(&mut self, result: Result<CommandResult<M>, CommandError>) -> StepOutcome {
        let Some(Awaiting {
            round,
            player,
            decision,
            command,
            end_turn,
        }) = self.awaiting.take()
        else {
            return StepOutcome::Worked;
        };
        let applied = result.is_ok();
        let _span = prof::span("ai/keep_journal_and_trace");
        let outcome = self.keep_journal(&command, result);
        self.record(round, player, &decision, command, outcome);
        if end_turn {
            return StepOutcome::PlayerDone(player);
        }
        if let Actor::Unit(unit) = decision.actor {
            self.after_unit_command(round, player, unit, applied);
        }
        StepOutcome::Worked
    }

    /// The outcome of an executed command; its journal is kept while the
    /// host records them.
    fn keep_journal(
        &mut self,
        command: &M::Command,
        result: Result<CommandResult<M>, CommandError>,
    ) -> Outcome {
        match result {
            Ok(result) => {
                if let Some(journal) = self.journal.as_mut() {
                    journal.push(AppliedCommand {
                        command: command.clone(),
                        changes: result.into_journal(),
                    });
                }
                Outcome::Applied
            }
            Err(error) => Outcome::Rejected(error),
        }
    }

    fn record(
        &mut self,
        round: u32,
        player: PlayerId,
        decision: &Decision<M>,
        command: M::Command,
        outcome: Outcome,
    ) {
        let applied = outcome == Outcome::Applied;
        if applied {
            self.counters.commands_applied += 1;
            if let Some(log) = self.log.as_mut() {
                log.push(command.clone());
            }
        } else {
            self.counters.commands_rejected += 1;
        }
        self.trace.push(TraceEntry {
            round,
            player,
            actor: decision.actor,
            reason: decision.reason,
            command: Some(command),
            outcome,
        });
    }

    /// A rejected command ends the unit's turn and counts against its task;
    /// a unit stops after `commands_per_unit` commands.
    fn after_unit_command(&mut self, round: u32, player: PlayerId, unit: EntityId, applied: bool) {
        let Some(turn) = self.turn.as_mut() else {
            return;
        };
        let count = turn.count_command(unit);
        if !applied {
            turn.mark_done(unit);
            let state = self.states.get_or_create(player, round);
            if state.note_failure(unit) {
                self.counters.tasks_dropped += 1;
            }
            return;
        }
        if count >= self.settings.budget.commands_per_unit {
            turn.mark_done(unit);
            self.trace.push(refusal(
                round,
                player,
                Actor::Unit(unit),
                Reason::AttemptsExhausted,
            ));
        }
    }

    /// The end of the player's turn: its command (None: no turn to end).
    fn take_end_turn(&mut self, game: &GameState<M>) -> Option<M::Command> {
        let Some(turn) = self.turn.take() else {
            self.phase = DriverPhase::Idle;
            return None;
        };
        let player = turn.player;
        let command = M::end_turn(player);
        self.awaiting = Some(Awaiting {
            round: game.view().auth.turn().round,
            player,
            decision: Decision::act(Actor::Player, Reason::EndTurn, command.clone()),
            command: command.clone(),
            end_turn: true,
        });
        self.phase = DriverPhase::NextPlayer;
        Some(command)
    }

    fn next_player(&mut self, game: &GameState<M>) -> StepOutcome {
        let new_round = game.view().auth.turn().round != self.last_round;
        self.phase = match self.ai_to_act(game) {
            Some(_) if !new_round => DriverPhase::PlayerStart,
            _ => DriverPhase::RoundDone,
        };
        StepOutcome::Worked
    }
}

/// How many times a turn a unit's decision may be `Reason::Deferred` and the
/// unit moved to the end of the turn's queue: the mind asks (no command,
/// reason `Deferred`), the driver grants it while the unit has deferrals
/// left; a request beyond them ends the unit's turn as no command does. A
/// mind that never asks keeps the order of `decision_order` (and
/// `AiMind::order_units`). One deferral a unit keeps every turn finite: the
/// queue grows by at most its length.
pub const DEFERS_PER_UNIT: u32 = 1;

/// Deferrals a unit has a turn when its mind picks the next unit itself
/// (`AiMind::picks_next`, "who is next"): the mind may hold a unit back while
/// others act and call it again later, so it needs more than one; a bound
/// keeps every turn finite (the queue grows by at most this many times its
/// length).
pub const REORDERS_PER_UNIT: u32 = 3;

/// Units in decision order: settlers, workers, other civilians, then the
/// army (an escort moves after its settler), each by id.
fn decision_order<M: RulesModule>(
    units: &[(EntityId, UnitHandle, UnitKindId)],
    rules: &CompiledRuleset<M>,
) -> Vec<(EntityId, UnitHandle)> {
    let rank = |role: AiRole| match role {
        AiRole::Settler => 0,
        AiRole::Worker => 1,
        AiRole::Civilian | AiRole::Naval => 2,
        AiRole::Military => 3,
    };
    let mut order: Vec<(u8, EntityId, UnitHandle)> = units
        .iter()
        .map(|&(id, handle, kind)| (rank(role_of(rules, kind)), id, handle))
        .collect();
    order.sort_by_key(|(rank, id, _)| (*rank, *id));
    order
        .into_iter()
        .map(|(_, id, handle)| (id, handle))
        .collect()
}

/// Commands go to the pending queue; choices without a command go to the
/// trace.
fn queue_decisions<M: RulesModule, B>(
    turn: &mut PlayerTurn<M, B>,
    trace: &mut DecisionTrace<M>,
    round: u32,
    decisions: Vec<Decision<M>>,
) {
    for decision in decisions {
        if decision.command.is_some() {
            turn.pending.push_back(decision);
        } else {
            trace.push(refusal(round, turn.player, decision.actor, decision.reason));
        }
    }
}

fn refusal<M: RulesModule>(round: u32, player: PlayerId, actor: Actor, reason: Reason) -> TraceEntry<M> {
    TraceEntry {
        round,
        player,
        actor,
        reason,
        command: None,
        outcome: Outcome::NoCommand,
    }
}

/// The AI players as the kernel's host drives them.
impl<M: RulesModule, P: AiMind<M> + Default> AiPlayers<M> for AiDriver<M, P> {
    fn for_game(rules: &CompiledRuleset<M>, difficulty: &str, humans: Vec<PlayerId>) -> Self {
        AiDriver::new(AiSettings::for_rules(rules, difficulty), humans)
    }

    fn phase(&self) -> DriverPhase {
        AiDriver::phase(self)
    }

    fn is_human(&self, player: PlayerId) -> bool {
        AiDriver::is_human(self, player)
    }

    fn set_humans(&mut self, humans: Vec<PlayerId>) {
        AiDriver::set_humans(self, humans)
    }

    fn step(&mut self, game: &mut GameState<M>, rules: &CompiledRuleset<M>) -> StepOutcome {
        AiDriver::step(self, game, rules)
    }

    fn advance(&mut self, game: &GameState<M>, rules: &CompiledRuleset<M>) -> Advance<M::Command> {
        AiDriver::advance(self, game, rules)
    }

    fn executed(&mut self, result: Result<CommandResult<M>, CommandError>) -> StepOutcome {
        AiDriver::executed(self, result)
    }

    fn turns_passed(&mut self) {
        AiDriver::turns_passed(self)
    }

    fn record_journal(&mut self) {
        AiDriver::record_journal(self)
    }

    fn records_journal(&self) -> bool {
        AiDriver::records_journal(self)
    }

    fn take_journal(&mut self) -> Vec<AppliedCommand<M>> {
        AiDriver::take_journal(self)
    }

    fn command_counts(&self) -> (u64, u64) {
        let counters = self.counters();
        (counters.commands_applied, counters.commands_rejected)
    }

    fn save(&self) -> SavedAiStates {
        AiDriver::save(self)
    }

    fn load(&mut self, saved: &SavedAiStates, tiles: usize, rules: &CompiledRuleset<M>) -> Result<(), DecodeError> {
        AiDriver::load(self, saved, tiles, rules)
    }

    fn configure_game(&self, game: &mut GameState<M>) {
        AiDriver::configure_game(self, game)
    }

    fn difficulty(&self) -> &str {
        &self.settings().difficulty
    }
}
