//! AiAgent: the module's AI players (`AiPlayers`) as a `PlayerAgent`.

use super::{AgentContext, AgentEvent, AgentPoll, PlayerAgent};
use crate::ai::{Advance, AiPlayers};
use crate::module::RulesModule;
use crate::world::state::GameState;

/// The AI of every seat it is assigned to. One agent holds the AI machine and
/// its memory (`AiStates`, saved with the game); it serves all those seats, one
/// seat at a time as the turn passes.
///
/// A poll is one `AiPlayers::advance`: the machine reads the game (through its
/// module's sensors: the observation is built there, the planners never see
/// the game) and either did a piece of work (`Working`), has a command for the
/// host to execute, or has nothing to do (`Pending`). The result of the command
/// comes back through `observe`.
///
/// The seats the table gives this agent must be the players the machine plays:
/// `M::Ai::for_game(.., humans)` names the others, and the two are kept equal
/// by whoever builds both (`SeatTable::replace` moves a seat out; the machine's
/// `set_humans` follows, between turns).
pub struct AiAgent<M: RulesModule> {
    ai: M::Ai,
}

impl<M: RulesModule> AiAgent<M> {
    pub fn new(ai: M::Ai) -> Self {
        Self { ai }
    }

    pub fn ai(&self) -> &M::Ai {
        &self.ai
    }

    pub fn ai_mut(&mut self) -> &mut M::Ai {
        &mut self.ai
    }

    pub fn into_inner(self) -> M::Ai {
        self.ai
    }
}

impl<M: RulesModule> PlayerAgent<M> for AiAgent<M> {
    fn poll(&mut self, cx: &AgentContext<'_, M>) -> AgentPoll<M::Command> {
        match self.ai.advance(cx.game, cx.rules) {
            Advance::Worked | Advance::RoundDone => AgentPoll::Working,
            Advance::Command(command) => AgentPoll::Command(command),
            Advance::EndTurn(_) => AgentPoll::EndTurn,
            Advance::Waiting => AgentPoll::Pending,
        }
    }

    fn observe(&mut self, event: AgentEvent<M>) {
        match event {
            AgentEvent::Executed { result, .. } => {
                self.ai.executed(result);
            }
            AgentEvent::TurnsPassed => self.ai.turns_passed(),
        }
    }

    fn configure_game(&self, game: &mut GameState<M>) {
        self.ai.configure_game(game);
    }
}
