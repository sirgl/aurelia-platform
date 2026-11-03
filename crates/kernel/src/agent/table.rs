//! SeatTable: seat -> agent, and the sans-IO loop that plays the active seat.

use std::collections::BTreeMap;

use super::{AgentContext, AgentEvent, AgentPoll, PlayerAgent};
use crate::ai::driver::RunReport;
use crate::ai::StepOutcome;
use crate::ids::PlayerId;
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::world::state::GameState;

/// Index of an agent in the table (stable while the agent is in it).
pub type AgentSlot = usize;

struct Entry<M: RulesModule> {
    agent: Box<dyn PlayerAgent<M>>,
}

/// Who plays which seat. A seat without an agent is not played by the table:
/// the host leaves it to whoever drives it from outside, and `step` waits.
pub struct SeatTable<M: RulesModule> {
    entries: Vec<Option<Entry<M>>>,
    seats: BTreeMap<PlayerId, AgentSlot>,
    /// Plays every seat that has no agent of its own (the AI of a game whose
    /// number of players the host does not know yet).
    rest: Option<AgentSlot>,
    /// A seat has just ended its turn, in this round, played by this agent
    /// (the next `step` says whether that agent's turns are over).
    ended_in: Option<(u32, AgentSlot)>,
}

impl<M: RulesModule> Default for SeatTable<M> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            seats: BTreeMap::new(),
            rest: None,
            ended_in: None,
        }
    }
}

impl<M: RulesModule> std::fmt::Debug for SeatTable<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeatTable")
            .field("seats", &self.seats)
            .field("rest", &self.rest)
            .finish_non_exhaustive()
    }
}

impl<M: RulesModule> SeatTable<M> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `agent` plays all of `seats` (taking them from whoever played them).
    pub fn assign(&mut self, seats: &[PlayerId], agent: Box<dyn PlayerAgent<M>>) -> AgentSlot {
        let slot = self.entries.len();
        self.entries.push(Some(Entry { agent }));
        for seat in seats {
            self.release(*seat);
            self.seats.insert(*seat, slot);
        }
        slot
    }

    /// `agent` plays every seat that no other agent is assigned to.
    pub fn assign_rest(&mut self, agent: Box<dyn PlayerAgent<M>>) -> AgentSlot {
        let slot = self.entries.len();
        self.entries.push(Some(Entry { agent }));
        self.rest = Some(slot);
        slot
    }

    /// `agent` plays `seat` from now on (the player left and the AI took the
    /// seat over, or came back). Only between turns of the seat: a turn in
    /// progress is not handed over (the old agent's state would be lost).
    pub fn replace(&mut self, seat: PlayerId, agent: Box<dyn PlayerAgent<M>>) -> AgentSlot {
        self.assign(&[seat], agent)
    }

    /// Nobody plays `seat` from now on.
    pub fn remove(&mut self, seat: PlayerId) {
        self.release(seat);
    }

    fn release(&mut self, seat: PlayerId) {
        if let Some(slot) = self.seats.remove(&seat) {
            if self.rest != Some(slot) && !self.seats.values().any(|s| *s == slot) {
                self.entries[slot] = None;
            }
        }
    }

    pub fn has_agent(&self, seat: PlayerId) -> bool {
        self.slot_of(seat).is_some()
    }

    pub fn slot_of(&self, seat: PlayerId) -> Option<AgentSlot> {
        self.seats.get(&seat).copied().or(self.rest)
    }

    /// The agent in `slot` (to read what it keeps: a journal, a counter).
    pub fn agent(&self, slot: AgentSlot) -> Option<&dyn PlayerAgent<M>> {
        self.entries.get(slot)?.as_ref().map(|e| e.agent.as_ref())
    }

    /// The agent in `slot` as its own type (to read what it keeps).
    pub fn agent_ref<A: PlayerAgent<M>>(&self, slot: AgentSlot) -> Option<&A> {
        (self.agent(slot)? as &dyn std::any::Any).downcast_ref()
    }

    pub fn agent_mut_as<A: PlayerAgent<M>>(&mut self, slot: AgentSlot) -> Option<&mut A> {
        (self.agent_mut(slot)? as &mut dyn std::any::Any).downcast_mut()
    }

    pub fn agent_mut(&mut self, slot: AgentSlot) -> Option<&mut (dyn PlayerAgent<M> + 'static)> {
        self.entries
            .get_mut(slot)?
            .as_mut()
            .map(|e| e.agent.as_mut())
    }

    /// The active seat has an agent and may act.
    fn active_seat(&self, game: &GameState<M>) -> Option<(PlayerId, AgentSlot)> {
        if game.is_over() {
            return None;
        }
        let auth = game.view().auth;
        let active = auth.active_player();
        if !auth.players().is_alive(active) {
            return None;
        }
        Some((active, self.slot_of(active)?))
    }

    /// One bounded step of the active seat's agent, with the outcomes of
    /// `AiPlayers::step`, so a host loop does not care which of the two plays.
    ///
    /// `RoundDone` comes after the `PlayerDone` of an agent that plays
    /// unattended (`!is_interactive`: the AI, a script), once the turn has
    /// passed to someone who waits for input (a human, a remote player) or to
    /// nobody, or the round has moved on: the host's cue to give control back
    /// and redraw. A human's own end of turn gives none. An agent whose turn
    /// passed to a seat it does not play is told (`AgentEvent::TurnsPassed`).
    ///
    /// `Worked`: something happened (a command was executed, the agent
    /// worked). `PlayerDone(seat)`: a seat ended its turn, or an agent asked
    /// for a boundary. `RoundDone`: the turns of seats with agents are over
    /// (the round moved on, or the next seat has none). `Waiting`: nothing to
    /// do until something arrives (a human's turn, an agent that is
    /// `Pending`, the end of the game).
    pub fn step(&mut self, game: &mut GameState<M>, rules: &CompiledRuleset<M>) -> StepOutcome {
        if let Some((round, ender)) = self.ended_in.take() {
            let next = self.active_seat(game);
            let next_slot = next.map(|(_, slot)| slot);
            let moved_on = game.view().auth.turn().round != round;
            if moved_on || next_slot != Some(ender) {
                let mut unattended = false;
                if let Some(Some(entry)) = self.entries.get_mut(ender) {
                    unattended = !entry.agent.is_interactive();
                    entry.agent.observe(AgentEvent::TurnsPassed);
                }
                // The end of a round of unattended turns: the turn went on to
                // someone who waits for input, or to nobody, or the round
                // moved on. A human's own end of turn is not one.
                let waits = next_slot.is_none_or(|slot| {
                    self.entries[slot]
                        .as_ref()
                        .is_some_and(|e| e.agent.is_interactive())
                });
                if unattended && (moved_on || waits) {
                    return StepOutcome::RoundDone;
                }
            }
        }
        let Some((seat, slot)) = self.active_seat(game) else {
            return StepOutcome::Waiting;
        };
        let entry = self.entries[slot]
            .as_mut()
            .expect("a seat's slot is filled");
        // A game nobody configured gets its setup from its first agent (a no-op
        // for a game that has its settings, as it was in `AiDriver::step`).
        entry.agent.configure_game(game);
        let poll = {
            let cx = AgentContext::new(game, rules, seat);
            entry.agent.poll(&cx)
        };
        match poll {
            AgentPoll::Working => StepOutcome::Worked,
            AgentPoll::Pending => StepOutcome::Waiting,
            AgentPoll::Boundary => StepOutcome::PlayerDone(seat),
            AgentPoll::Command(command) => {
                Self::execute(entry, game, rules, command);
                StepOutcome::Worked
            }
            AgentPoll::EndTurn => {
                let round = game.view().auth.turn().round;
                let applied = Self::execute(entry, game, rules, M::end_turn(seat));
                if applied {
                    self.ended_in = Some((round, slot));
                    StepOutcome::PlayerDone(seat)
                } else {
                    StepOutcome::Worked
                }
            }
        }
    }

    /// The command goes through the game's boundary; the agent hears the
    /// result. True when the game applied it.
    fn execute(
        entry: &mut Entry<M>,
        game: &mut GameState<M>,
        rules: &CompiledRuleset<M>,
        command: M::Command,
    ) -> bool {
        let result = game.execute(rules, command.clone());
        let applied = result.is_ok();
        entry
            .agent
            .observe(AgentEvent::Executed { command, result });
        applied
    }

    /// Run at most `max_steps` steps; stops early when the round's turns are
    /// over or nothing can move.
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
}
