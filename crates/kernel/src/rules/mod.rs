//! Game rules: SourceRules -> compile -> CompiledRuleset (§4-5).
//!
//! Contract: after compilation CompiledRuleset is immutable,
//! the simulation takes `&CompiledRuleset`, there are no string lookups in the hot path.

pub mod compile;
pub mod compile_effects;
mod compile_handicaps;
pub mod compile_tables;
pub mod compiled;
pub mod content;
pub mod effects;
pub mod fingerprint;
pub mod key_index;
pub mod modifiers;
pub mod natural_wonders;
pub mod roles;
pub mod rows;
pub mod source;
pub mod speeds;
pub mod stats;
pub mod tables;
pub mod vocabulary;

pub use compile::{compile_rules, RulesError};
pub use compiled::CompiledRuleset;
pub use source::{SourceRules, StableContentKey};
