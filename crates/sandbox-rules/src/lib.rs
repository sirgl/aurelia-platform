//! sandbox-rules: the second rules module of the platform (platform/PLATFORM.md
//! stage 7, decision D3) and the template for a new game on it.
//!
//! A minimal 4X built only on the kernel's public contracts: territory and
//! borders, cities founded by settlers, two yields (food and production),
//! growth, production of a few units and buildings, melee combat with its
//! own formula, city capture and one victory (`victory`). The kernel's
//! systems do the generic work (moves, founding, growth, the queue, borders,
//! the turn, saves, replay, the AI driver); this crate supplies the rules at
//! the points the contracts name:
//!
//! | contract (kernel)                 | here                       |
//! |-----------------------------------|----------------------------|
//! | `module::RulesModule`             | [`Sandbox`]                |
//! | `world::module::ModuleWorld`      | `world::SandboxWorld`      |
//! | `rules::tables::ModuleTables`     | `world::SandboxTables`     |
//! | `rules::vocabulary::Vocabulary`   | `kernel::empty::NoVocabulary` |
//! | `systems::ModuleSystems`          | `systems::SandboxSystems`  |
//! | `derived::yields::YieldTerms`     | `systems::SandboxYields`   |
//! | `ai::mind::AiMind`                | `ai::SandboxMind`          |
//!
//! The module never depends on another rules module or its content
//! (`platform/tools/platform-lint.sh`, `sandbox_deps`); its content pack is
//! `sandbox-content`.

pub mod ai;
pub mod combat;
pub mod command;
pub mod game;
pub mod systems;
pub mod victory;
pub mod world;

use kernel::derived::DerivedWorld;
use kernel::empty::{NoVocabulary, NoWord};
use kernel::ids::PlayerId;
use kernel::module::{Rejected, RulesModule};
use kernel::navigation::PathScratch;
use kernel::rules::CompiledRuleset;
use kernel::transaction::journal::ChangeJournal;
use kernel::turn::resolution::TurnSchedule;
use kernel::world::state::AuthWorld;

pub use command::Command;

/// The sandbox rules module (the marker the kernel's types take).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sandbox;

/// The kernel's types instantiated with the sandbox module.
pub type Rules = CompiledRuleset<Sandbox>;
pub type Game = kernel::world::state::GameState<Sandbox>;

impl RulesModule for Sandbox {
    type World = world::SandboxWorld;
    type Vocabulary = NoVocabulary;
    type Tables = world::SandboxTables;
    type Yields = systems::SandboxYields;
    type Command = Command;
    type Systems = systems::SandboxSystems;
    type Ai = kernel::ai::driver::AiDriver<Sandbox, ai::SandboxMind>;
    type Step = NoWord;
    type CityStep = systems::CityStep;

    const SCHEDULE: TurnSchedule<NoWord, systems::CityStep> = systems::SCHEDULE;

    fn execute(
        world: &mut AuthWorld<Self>,
        derived: &DerivedWorld<Self>,
        rules: &CompiledRuleset<Self>,
        command: Command,
        scratch: &mut PathScratch,
    ) -> Result<ChangeJournal<Self>, Rejected<Self>> {
        command::execute(world, derived, rules, command, scratch)
    }

    fn victor(world: &AuthWorld<Self>) -> Option<PlayerId> {
        victory::victor(world)
    }

    fn is_over(world: &AuthWorld<Self>) -> bool {
        victory::victor(world).is_some()
    }

    fn end_turn(player: PlayerId) -> Command {
        Command::Kernel(kernel::command::KernelCommand::EndTurn { player })
    }
}
