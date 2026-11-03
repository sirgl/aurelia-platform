//! The sandbox's world state and tables: none of its own. Everything the
//! sandbox plays with is the kernel's (map, players, cities, units, war);
//! the victory is read from the kernel's world (`victory`). The types below
//! are the empty answers to the kernel's contracts.

use kernel::empty::NoWord;
use kernel::ids::EntityId;
use kernel::persistence::decode::DecodeError;
use kernel::rules::compile::{RulesError, SourceKeys};
use kernel::rules::content::ContentTable;
use kernel::rules::effects::EffectSet;
use kernel::rules::fingerprint::Fingerprint;
use kernel::rules::key_index::{KeyIndex, TableKeys};
use kernel::rules::source::SourceRules;
use kernel::rules::tables::ModuleTables;
use kernel::validation::world::ValidationError;
use kernel::world::module::{HashPoint, KernelPlayerRecord, ModuleId, ModulePlayer, ModuleWorld, NewModuleWorld};
use kernel::world::state::AuthWorld;

use crate::{Rules, Sandbox};

/// The sandbox's world state beside the kernel's: empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxWorld;

/// The sandbox's state of a player: empty (its save record is the kernel's).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxPlayer;

/// The sandbox's compiled tables: none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SandboxTables;

impl ModuleWorld<Sandbox> for SandboxWorld {
    const ID: ModuleId = ModuleId { name: "sandbox", version: 1 };
    type Saved = ();
    type Change = NoWord;
    type Player = SandboxPlayer;
    type PlayerKind = kernel::world::player::NoKind;
    type City = ();
    type Unit = ();

    fn new(_: NewModuleWorld<'_, Sandbox>) -> Self {
        SandboxWorld
    }
    fn hash(&self, _: HashPoint, _: &AuthWorld<Sandbox>, _: &mut dyn FnMut(u64)) {}
    fn validate(&self, _: &AuthWorld<Sandbox>, _: &Rules) -> Result<(), ValidationError> {
        Ok(())
    }
    fn unit_removed(&mut self, _: EntityId) {}
    fn encode(&self, _: &AuthWorld<Sandbox>) {}
    fn decode(_: &(), _: &AuthWorld<Sandbox>, _: &Rules) -> Result<Self, DecodeError> {
        Ok(SandboxWorld)
    }
}

impl ModulePlayer<Sandbox> for SandboxPlayer {
    type Saved = KernelPlayerRecord;
    fn encode(&self, kernel: KernelPlayerRecord) -> KernelPlayerRecord {
        kernel
    }
    fn kernel_part(saved: &KernelPlayerRecord) -> KernelPlayerRecord {
        *saved
    }
    fn decode(_: &KernelPlayerRecord, _: usize, _: &AuthWorld<Sandbox>) -> Result<Self, String> {
        Ok(SandboxPlayer)
    }
    fn hash(saved: &KernelPlayerRecord, mix: &mut dyn FnMut(u64)) {
        saved.hash(mix)
    }
}

impl ModuleTables<Sandbox> for SandboxTables {
    type Source = ();
    type Rows = kernel::rules::rows::NoRows;
    type SourceRoles = ();
    type Roles = ();
    fn merge_source(_: &mut (), _: ()) {}
    fn source_keys(_: &()) -> Vec<(ContentTable, TableKeys)> {
        Vec::new()
    }

    fn compile(_: &SourceRules<Sandbox>, _: &SourceKeys, _: &KeyIndex, _: &Rules) -> Result<Self, RulesError> {
        Ok(SandboxTables)
    }
    fn check(_: &Rules) -> Result<(), RulesError> {
        Ok(())
    }
    fn attach_effects(_: &SourceRules<Sandbox>, _: &SourceKeys, _: &KeyIndex, _: &mut Rules) -> Result<(), RulesError> {
        Ok(())
    }
    fn effect_set(&self, source: NoWord) -> Option<&EffectSet<Sandbox>> {
        match source {}
    }
    fn effect_sets(&self) -> Vec<(NoWord, &EffectSet<Sandbox>)> {
        Vec::new()
    }
    fn merge_roles(_: &mut (), _: ()) {}
    fn compile_roles(_: &(), _: &KeyIndex, _: &Rules) -> Result<(), RulesError> {
        Ok(())
    }
    fn hash_after(&self, _: &str, _: &mut Fingerprint) {}
}
