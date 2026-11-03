//! The kernel's part of the AI: the contract of a module's AI players
//! ([`AiPlayers`], which the host steps between frames), the step-wise
//! driver of their turns (`driver`) with its trace (`trace`), the saved
//! memory (`state`), what decisions are made of (`plan`), the map search
//! over the player's knowledge (`map`), the contract of the module's
//! planners (`mind::AiMind`) and minds by seat for the AI bench (`seats`).
//! The decisions are the module's.

pub mod driver;
pub mod map;
pub mod mind;
pub mod observation;
pub mod plan;
pub mod seats;
pub mod state;
pub mod trace;

use crate::ids::PlayerId;
use crate::module::RulesModule;
use crate::persistence::ai::SavedAiStates;
use crate::persistence::decode::DecodeError;
use crate::rules::CompiledRuleset;
use crate::command::CommandError;
use crate::transaction::journal::{AppliedCommand, CommandResult};
use crate::world::state::GameState;

/// Where the driver is in the AI players' turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverPhase {
    /// No AI turn running: a human acts, or the game is over.
    Idle,
    PlayerStart,
    Observe,
    PlanStrategic,
    /// Choose the next decision: a planned command, the next unit, the
    /// cities at the end of the turn, or the end of the turn.
    PlanNext,
    Execute,
    PlayerEnd,
    NextPlayer,
    /// Control returns to a human (or an all-AI round ended).
    RoundDone,
}

/// What one step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// One piece of AI work.
    Worked,
    /// An AI player ended its turn (a presentation boundary).
    PlayerDone(PlayerId),
    /// The AI turns of this round are over.
    RoundDone,
    /// Nothing to do: a human's turn or the end of the game.
    Waiting,
}

/// What the AI machine wants next (`AiPlayers::advance`): the same steps as
/// `StepOutcome`, except that a command is handed to the caller to execute
/// instead of being executed inside the machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advance<C> {
    /// One piece of AI work that needs nothing from the host.
    Worked,
    /// Execute this command, then give its result to `AiPlayers::executed`.
    Command(C),
    /// The same for the command that ends the player's turn (the result
    /// brings `StepOutcome::PlayerDone`).
    EndTurn(C),
    /// The AI turns of this round are over.
    RoundDone,
    /// Nothing to do: a human's turn or the end of the game.
    Waiting,
}

/// The AI players of a game: a deterministic machine the host advances one
/// bounded step at a time (it never reads a clock), with the memory a save
/// keeps.
pub trait AiPlayers<M: RulesModule>: std::fmt::Debug + Sized {
    /// AI players for a game at `difficulty` (a handicap key); `humans`
    /// play themselves.
    fn for_game(rules: &CompiledRuleset<M>, difficulty: &str, humans: Vec<PlayerId>) -> Self;

    fn phase(&self) -> DriverPhase;

    fn is_human(&self, player: PlayerId) -> bool;

    /// Change who plays themselves (a seat's player left and the AI takes
    /// it over, or came back). Only between AI turns (`DriverPhase::Idle`):
    /// a turn in progress is not handed over.
    fn set_humans(&mut self, humans: Vec<PlayerId>);

    /// One bounded step of the AI turns.
    fn step(&mut self, game: &mut GameState<M>, rules: &CompiledRuleset<M>) -> StepOutcome;

    /// `step` in two halves, for a host that executes the commands itself
    /// (`agent::AiAgent`): this one only reads the game and may ask for a
    /// command; `executed` takes the command's result back. `step` is these
    /// two with `GameState::execute` between them. The game's AI setup
    /// (`configure_game`) is the caller's, before the first call.
    fn advance(&mut self, game: &GameState<M>, rules: &CompiledRuleset<M>) -> Advance<M::Command>;

    /// The result of the command `advance` asked for.
    fn executed(&mut self, result: Result<CommandResult<M>, CommandError>) -> StepOutcome;

    /// The host passed the turn on to a player the AI does not play, or the
    /// round moved on: the machine is between turns again (its phase is
    /// `Idle`), as after a `RoundDone` step.
    fn turns_passed(&mut self);

    /// Keep the applied commands with their journals until `take_journal`.
    fn record_journal(&mut self);

    fn records_journal(&self) -> bool;

    /// The commands applied since the last call, in order.
    fn take_journal(&mut self) -> Vec<AppliedCommand<M>>;

    /// Commands applied and rejected since the start.
    fn command_counts(&self) -> (u64, u64);

    /// The AI memory for a save (between turns).
    fn save(&self) -> SavedAiStates;

    /// The AI memory of a loaded game on a map of `tiles` tiles.
    fn load(&mut self, saved: &SavedAiStates, tiles: usize, rules: &CompiledRuleset<M>) -> Result<(), DecodeError>;

    /// A new game takes the players' difficulty and AI players; a game
    /// that has its settings (a save) keeps them.
    fn configure_game(&self, game: &mut GameState<M>);

    /// Content key of the game's difficulty.
    fn difficulty(&self) -> &str;
}
