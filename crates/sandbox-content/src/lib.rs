//! sandbox-content: the content pack of the sandbox rules module — its
//! terrains, one feature, units, buildings and factions as data
//! (`data/pack.ron`) in the pack's own small schema ([`pack`]), compiled
//! into the kernel's source rules and roles ([`source_rules`]).
//!
//! The pack also carries its art catalog: the unit model each unit is drawn
//! with and the colours of the terrain layers (`art`).

pub mod art;
pub mod pack;

use kernel::rules::compile::{compile_rules, RulesError};
use kernel::rules::source::SourceRules;
use sandbox_rules::{Rules, Sandbox};

/// The pack's text (compiled in: the pack needs no file system).
pub const PACK: &str = include_str!("../data/pack.ron");

/// The pack as written.
pub fn pack() -> pack::Pack {
    ron::from_str(PACK).expect("the sandbox pack parses")
}

/// The pack as the kernel's source rules.
pub fn source_rules() -> SourceRules<Sandbox> {
    pack().source_rules()
}

/// The compiled sandbox rules.
pub fn rules() -> Result<Rules, RulesError> {
    compile_rules(source_rules())
}
