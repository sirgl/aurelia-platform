//! The contract between the kernel's AI driver and a rules module's
//! planners ([`AiMind`]): the driver runs the turn (budgets, order, trace,
//! memory, commands through the game's boundary); the mind observes and
//! decides. The decisions read only what the player knows.
//!
//! The boundary (classic/docs/ai-truth-reads.md): the two `sense*` methods are the
//! ONLY place a mind touches the game. They build what the player sees
//! (`Sight` for a whole turn step, `Near` around one unit); every planning
//! method receives that observation and never the game, so a mind that
//! implements `Sight` with a real observation cannot read the truth by
//! construction. A mind may implement `Sight` as the game itself (the
//! L&L and sandbox minds do: those are held by the guard test, not by types).

use crate::ai::plan::{AiSettings, Decision, PlanTools};
use crate::ai::state::AiState;
use crate::ids::{EntityId, PlayerId, UnitHandle, UnitKindId};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::world::state::GameState;

/// How an AI player plays its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnKind {
    /// Strategy, units one by one, then the cities.
    Full,
    /// No strategy of its own (Classic: a city-state neither declares war
    /// nor makes peace); units and cities as usual.
    NoStrategy,
    /// The opening decisions are the whole turn (Classic: the barbarians).
    Scripted,
}

/// What the mind makes of the observation at the start of a player's turn.
#[derive(Debug)]
pub struct TurnStart<M: RulesModule, B> {
    /// The owned brief every later decision of the turn reads.
    pub brief: B,
    pub kind: TurnKind,
    /// The player's units the turn decides for (the driver orders them).
    pub units: Vec<(EntityId, UnitHandle, UnitKindId)>,
    /// The player's cities (the decision budget grows with them).
    pub cities: u32,
    /// Decisions before the units move (after the strategic ones).
    pub opening: Vec<Decision<M>>,
}

/// A rules module's AI planners.
pub trait AiMind<M: RulesModule>: std::fmt::Debug {
    /// The owned summary of the player's turn.
    type Brief: std::fmt::Debug;

    /// What the player sees of the whole world at one moment (its own
    /// state, what is visible now, the map as known, the public facts).
    type Sight<'g>;

    /// What the player sees around one of its units.
    type Near<'g>;

    /// The sensor of the turn: the one place besides `sense_unit` where the
    /// mind reads the game. The driver calls it at the start of the turn
    /// and again before the cities are planned.
    fn sense<'g>(&self, game: &'g GameState<M>, rules: &'g CompiledRuleset<M>, player: PlayerId) -> Self::Sight<'g>;

    /// The sensor around a unit, rebuilt after every command (None: the
    /// unit is gone or not the player's).
    fn sense_unit<'g>(
        &self,
        game: &'g GameState<M>,
        rules: &'g CompiledRuleset<M>,
        settings: &AiSettings,
        player: PlayerId,
        unit: UnitHandle,
    ) -> Option<Self::Near<'g>>;

    /// The players that play themselves (the driver's `humans`), told at the
    /// start and whenever they change: a mind may keep its new aggressive
    /// aims off them. It is the game's configuration, not an observation of
    /// the world, and the default ignores it.
    fn note_humans(&mut self, _humans: &[PlayerId]) {}

    /// Observe at the start of the turn: the memory update, the brief, the
    /// units and the opening decisions.
    fn start_turn(
        &self,
        sight: &Self::Sight<'_>,
        rules: &CompiledRuleset<M>,
        player: PlayerId,
        state: &mut AiState,
    ) -> TurnStart<M, Self::Brief>;

    /// The order the turn's units decide in. The driver's order comes in
    /// (settlers, workers, other civilians, then the army, each by id) and a
    /// mind may reorder it from its brief (Classic: the `unciv` mind sorts by
    /// Unciv's unit priority). The default keeps the driver's order.
    fn order_units(&self, _brief: &Self::Brief, _rules: &CompiledRuleset<M>, _order: &mut Vec<(EntityId, UnitHandle)>) {}

    /// Does the mind pick its turn's next unit itself (`next_unit`)? Asked
    /// once a turn, after `order_units`. False (the default): the driver's
    /// queue as above — its order, one deferral a unit
    /// (`driver::DEFERS_PER_UNIT`) — and `next_unit` is never asked, so a
    /// mind that does not implement it plays exactly as before.
    fn picks_next(&self, _brief: &Self::Brief) -> bool {
        false
    }

    /// "Who is next": before each unit decision of a turn whose mind
    /// `picks_next`, which of `remaining` (the turn's units not done, in
    /// the queue's order; at least two) decides now. `near` is a fresh
    /// observation around the queue's head (`remaining[0]`), taken after
    /// the last executed command, so the answer can follow the turn as it
    /// goes (a shot that missed, a unit that died). The answer goes to the
    /// front of the queue; None, or a unit not in `remaining`, keeps the
    /// head. Such a mind may defer a unit `driver::REORDERS_PER_UNIT` times
    /// a turn, so its turn stays finite. The answer must depend only on
    /// what the mind observes (the game stays deterministic).
    fn next_unit(
        &self,
        _near: &Self::Near<'_>,
        _brief: &Self::Brief,
        _state: &AiState,
        _remaining: &[(EntityId, UnitHandle)],
    ) -> Option<EntityId> {
        None
    }

    /// Strategy, diplomacy and research of the turn (the brief and the
    /// memory only).
    fn plan_strategic(&self, brief: &Self::Brief, state: &mut AiState, rules: &CompiledRuleset<M>) -> Vec<Decision<M>>;

    /// One decision of one unit from a fresh observation around it. No
    /// command with `Reason::Deferred` asks to decide again after the rest
    /// of the turn's queue (`driver::DEFERS_PER_UNIT` times a turn,
    /// `driver::REORDERS_PER_UNIT` for a mind that `picks_next`; the mind
    /// counts its own requests).
    #[allow(clippy::too_many_arguments)]
    fn decide_unit(
        &self,
        near: &Self::Near<'_>,
        player: PlayerId,
        unit: UnitHandle,
        brief: &Self::Brief,
        state: &mut AiState,
        tools: &mut PlanTools<'_, M>,
    ) -> Option<Decision<M>>;

    /// May the planned command still be sent? Asked at the moment of
    /// sending, against the game as it is then (an earlier command of the
    /// same batch may have changed it). A mind that answers false withholds
    /// the command: it is counted (`AiCounters::orders_withheld`), traced
    /// as `Reason::Forbidden` and never reaches the game's command
    /// boundary, so the game refuses nothing of this mind's.
    fn permits(&self, _game: &GameState<M>, _rules: &CompiledRuleset<M>, _player: PlayerId, _command: &M::Command) -> bool {
        true
    }

    /// The cities' production and the end-of-turn decisions from a fresh
    /// full observation.
    fn plan_end_of_turn(
        &self,
        sight: &Self::Sight<'_>,
        player: PlayerId,
        brief: &Self::Brief,
        state: &AiState,
        tools: &mut PlanTools<'_, M>,
    ) -> Vec<Decision<M>>;
}
