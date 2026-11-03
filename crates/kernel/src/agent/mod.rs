//! PlayerAgent: who plays a seat (classic/docs/player-agent.md, platform/docs/kernel-audit.md "PlayerAgent").
//!
//! One interface for every way of playing a seat: a human at a screen, the
//! module's AI, a network client, a replay, a scripted test. An agent is
//! polled when its seat may act; it looks at what the seat knows
//! ([`AgentContext::view`], a `PlayerView`, never the authoritative world) and
//! answers with the next [`AgentPoll`]. The host ([`SeatTable`]) executes the
//! command through `GameState::execute`, the one write boundary, and tells the
//! agent what came of it ([`AgentEvent`]).
//!
//! The host never reads a clock. Wall time enters through `Pending` (a human
//! is thinking, a packet is on its way) and through the caller slicing its
//! calls to [`SeatTable::step`]; every step is one bounded piece of work, so
//! any slicing plays the same game (`ai::driver` is documented slicing-
//! invariant, and the AI agent inherits that).
//!
//! What stays out: nothing here is saved or hashed. `GameSettings.ai` is still
//! the saved default of "who drives this seat on load"; the table of live
//! agents is session state.

mod ai;
mod scripted;
mod table;

pub use ai::AiAgent;
pub use scripted::ScriptedAgent;
pub use table::{AgentSlot, SeatTable};

use crate::command::CommandError;
use crate::ids::PlayerId;
use crate::knowledge::view::PlayerView;
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::CommandResult;
use crate::world::state::GameState;

/// What an agent answers when it is polled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentPoll<C> {
    /// Apply this command now through `GameState::execute`.
    Command(C),
    /// End the seat's turn: the host executes `M::end_turn(seat)` (a module's
    /// end of turn is a command like any other, and the agent need not know
    /// its shape).
    EndTurn,
    /// Nothing yet, and nothing until something from outside arrives: a human
    /// is thinking, a packet is on its way, a worker is running. The host
    /// stops asking for now.
    Pending,
    /// The agent did a piece of internal work (the AI observed, planned) and
    /// has no command yet: poll again. This is how a machine that works in
    /// bounded steps (the AI driver) lets the host slice its calls.
    Working,
    /// A presentation boundary: the host may redraw before polling again.
    Boundary,
}

/// What the world did to the seat since the last poll.
#[derive(Debug)]
pub enum AgentEvent<M: RulesModule> {
    /// The seat's command was executed (or refused). The result is the
    /// agent's to keep: its journal, a UI message, a reward.
    Executed {
        command: M::Command,
        result: Result<CommandResult<M>, CommandError>,
    },
    /// The agent ended a seat's turn and the turn passed on to a seat it does
    /// not play, or the round moved on: the table stops stepping it until its
    /// next seat is up. A machine with a turn-passing state of its own (the AI
    /// driver) settles here.
    TurnsPassed,
}

/// What the host shows an agent when it polls it.
///
/// `view()` is the observation of an agent of any kind. The game itself is
/// private to this module: only the kernel's own adapter for the module's AI
/// ([`AiAgent`]) reaches it, because the module's sensors build their
/// observation from the game (classic/docs/ai-truth-reads.md: the driver is the
/// sensor and may read the truth). An agent written anywhere else cannot.
pub struct AgentContext<'a, M: RulesModule> {
    pub seat: PlayerId,
    pub round: u32,
    pub rules: &'a CompiledRuleset<M>,
    view: PlayerView<'a, M>,
    game: &'a GameState<M>,
}

impl<'a, M: RulesModule> AgentContext<'a, M> {
    pub(crate) fn new(
        game: &'a GameState<M>,
        rules: &'a CompiledRuleset<M>,
        seat: PlayerId,
    ) -> Self {
        Self {
            seat,
            round: game.view().auth.turn().round,
            rules,
            view: game.player_view(rules, seat),
            game,
        }
    }

    /// What the seat knows.
    pub fn view(&self) -> PlayerView<'a, M> {
        self.view
    }
}

/// A seat's player: a human, the AI, a remote client, a script.
///
/// One agent may serve several seats (the AI shares one memory and one
/// machine across all its players): `cx.seat` says which seat is asked.
pub trait PlayerAgent<M: RulesModule>: std::any::Any {
    /// The seat may act now: look at what it knows and say what happens next.
    fn poll(&mut self, cx: &AgentContext<'_, M>) -> AgentPoll<M::Command>;

    /// The result of a command this agent asked for.
    fn observe(&mut self, _event: AgentEvent<M>) {}

    /// The agent waits for input from outside (a human, a remote player) and
    /// answers `Pending` while it has none. The AI and scripts do not: they
    /// play on their own, and the table reports the end of their round
    /// (`SeatTable::step`).
    fn is_interactive(&self) -> bool {
        false
    }

    /// Before every poll (cheap and idempotent): the part of the game's setup that
    /// belongs to who plays (the AI marks its seats as AI players in the
    /// game's saved settings and takes the handicap row). A game that has its
    /// settings (a save, a configured game) is left alone.
    fn configure_game(&self, _game: &mut GameState<M>) {}
}
