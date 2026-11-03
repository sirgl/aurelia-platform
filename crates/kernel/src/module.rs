//! The contract of a game's rules module (platform/PLATFORM.md, "Model of key
//! contracts"): what a module gives the kernel.
//!
//! A rules module is one type (Classic: `classic_rules::Classic`) that names its
//! world state ([`ModuleWorld`]), its words of the effect vocabulary
//! ([`Vocabulary`]) and how the effect engine evaluates them
//! ([`ModuleEffects`]), its compiled tables ([`ModuleTables`]), its derived
//! city yields ([`YieldTerms`]), its commands and how they change the
//! world, its AI players ([`AiPlayers`]) and the order of a turn
//! ([`TurnSchedule`]). The kernel's types
//! take the module as their parameter (`GameState<M>`, `AuthWorld<M>`,
//! `CompiledRuleset<M>`, ...); the kernel never names a module. One module
//! per game.
//!
//! The module type is a marker: it carries no data, so it derives every
//! trait the kernel's generic types derive.

use std::fmt::Debug;
use std::hash::Hash;

use crate::ai::AiPlayers;
use crate::command::CommandError;
use crate::derived::yields::YieldTerms;
use crate::derived::DerivedWorld;
use crate::effects::module::ModuleEffects;
use crate::ids::PlayerId;
use crate::navigation::PathScratch;
use crate::rules::tables::ModuleTables;
use crate::rules::vocabulary::Vocabulary;
use crate::rules::CompiledRuleset;
use crate::systems::ModuleSystems;
use crate::transaction::journal::ChangeJournal;
use crate::turn::resolution::TurnSchedule;
use crate::world::module::{ModuleId, ModuleWorld};
use crate::world::state::AuthWorld;

/// A rules module: its state, words, tables, commands and turn order.
pub trait RulesModule:
    Copy + Debug + Default + PartialEq + Eq + PartialOrd + Ord + Hash + Send + Sync + 'static
{
    /// The module's world state beside the kernel's.
    type World: ModuleWorld<Self>;

    /// The module's words of the effect vocabulary and their evaluation.
    type Vocabulary: Vocabulary<Self> + ModuleEffects<Self>;

    /// The module's compiled tables (`CompiledRuleset::module`).
    type Tables: ModuleTables<Self>;

    /// The module's terms of the city yields (`derived::yields`).
    type Yields: YieldTerms<Self>;

    /// Everything a player may order.
    type Command: Clone + Debug;

    /// The module's part of the kernel's systems: its reactions to their
    /// events and its formulas.
    type Systems: ModuleSystems<Self>;

    /// The module's AI players.
    type Ai: AiPlayers<Self>;

    /// The module's steps of a player's end turn.
    type Step: Copy + Debug + 'static;

    /// The module's steps of one city's resolution.
    type CityStep: Copy + Debug + 'static;

    /// The module's name and version (its world's `ModuleWorld::ID`): the
    /// app keeps the module's saves under its name.
    const ID: ModuleId = <Self::World as ModuleWorld<Self>>::ID;

    /// The module's content editions a snapshot may come from: (the
    /// fingerprint of an earlier edition, the fingerprint of the edition
    /// that loads its snapshots). An edition changes what rows do, never
    /// which rows exist, so the snapshot loads under exactly that edition
    /// and no other rules. Empty: only the current rules.
    const EARLIER_EDITIONS: &'static [(u64, u64)] = &[];

    /// The full order of an end turn and of a city's resolution, kernel
    /// steps included. The module's turn driver runs it; the kernel checks
    /// it (`TurnSchedule::validate`) when a game starts.
    const SCHEDULE: TurnSchedule<Self::Step, Self::CityStep>;

    /// Validate `command` and apply it: the journal of what changed, or why
    /// it was rejected. A rejected command leaves the world unchanged.
    fn execute(
        world: &mut AuthWorld<Self>,
        derived: &DerivedWorld<Self>,
        rules: &CompiledRuleset<Self>,
        command: Self::Command,
        scratch: &mut PathScratch,
    ) -> Result<ChangeJournal<Self>, Rejected<Self>>;

    /// The winner of a victory the module declared (None: none yet).
    fn victor(world: &AuthWorld<Self>) -> Option<PlayerId>;

    /// How the game ended by the module's rules (None: no result yet):
    /// every winner of a shared result, and what decided it. By default
    /// the victor alone.
    fn outcome(world: &AuthWorld<Self>) -> Option<GameOutcome> {
        let winner = Self::victor(world)?;
        Some(GameOutcome { winners: vec![winner], kind: GameOutcome::VICTORY })
    }

    /// The game has ended (a victory nobody continues past).
    fn is_over(world: &AuthWorld<Self>) -> bool;

    /// Content keys of what the city shows on the map beyond its buildings: the module's
    /// showpieces standing in it (a great project built there). The renderer draws each by
    /// its key as it draws a world wonder; a key it has no model for draws nothing.
    fn city_showpieces(_world: &AuthWorld<Self>, _rules: &CompiledRuleset<Self>, _city: crate::ids::CityHandle) -> Vec<String> {
        Vec::new()
    }

    /// The command that ends `player`'s turn.
    fn end_turn(player: PlayerId) -> Self::Command;

    /// The era `player`'s cities are drawn in: an index into the city kit's styles
    /// (0 = the oldest). None: the kernel's own answer, the latest content era among the
    /// player's technologies (`Players::nation_era_of`). A module whose eras are not content
    /// eras of its technologies answers from its own (`AuthWorld::city_era`).
    fn city_look_era(_world: &AuthWorld<Self>, _rules: &CompiledRuleset<Self>, _player: PlayerId) -> Option<u8> {
        None
    }
}

/// The result of a game (`RulesModule::outcome`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameOutcome {
    /// Several: a shared result; none: nobody won (every player is gone).
    pub winners: Vec<PlayerId>,
    /// The module's name of what decided it ("score", "last power", ...).
    pub kind: &'static str,
}

impl GameOutcome {
    /// The kind of a module's victor without a name of its own.
    pub const VICTORY: &'static str = "victory";
}

/// A command the module rejected, and what it changed anyway (a defect of
/// its system: empty when the system keeps its contract).
#[derive(Debug)]
pub struct Rejected<M: RulesModule> {
    pub error: CommandError,
    pub partial: ChangeJournal<M>,
}
