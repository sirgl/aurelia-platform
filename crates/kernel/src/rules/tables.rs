//! The rules module's compiled tables (platform/PLATFORM.md stage 4): the
//! `CompiledRuleset` holds the kernel's tables and the module's part
//! (`CompiledRuleset::module`) without knowing its rows.
//!
//! The module compiles its tables from the content once the kernel's keys
//! are final, checks them against the kernel's tables, attaches the
//! effects of its rows, answers which rows carry effects, and hashes its
//! tables into the ruleset fingerprint at the places they held before the
//! split (`rules::fingerprint`), so saves keep loading.

use std::fmt::Debug;

use crate::module::RulesModule;
use crate::rules::compile::{RulesError, SourceKeys};
use crate::rules::compiled::CompiledRuleset;
use crate::rules::effects::EffectSet;
use crate::rules::fingerprint::Fingerprint;
use crate::rules::key_index::{KeyIndex, TableKeys};
use crate::rules::source::SourceRules;
use crate::ids::TechId;
use crate::rules::content::{ContentTable, ProjectId};
use crate::rules::rows::ModuleRows;
use crate::rules::vocabulary::ModuleSource;

/// The compiled tables of a rules module.
pub trait ModuleTables<M: RulesModule>: Debug + Clone + Default + 'static {
    /// The module's tables as content writes them (`SourceRules::module`).
    type Source: Debug + Clone + Default;

    /// Append the tables of `other` (a ruleset split over files).
    fn merge_source(source: &mut Self::Source, other: Self::Source);

    /// The keys of the module's tables, each with its table: the kernel
    /// registers them beside its own (a key is unique across all tables)
    /// and hands them back in `SourceKeys::module`.
    fn source_keys(source: &Self::Source) -> Vec<(ContentTable, TableKeys)>;

    /// The module's tables from the content; the kernel's tables they
    /// refer to are compiled (units, technologies, eras, terrains).
    fn compile(
        source: &SourceRules<M>,
        keys: &SourceKeys,
        ix: &KeyIndex,
        rules: &CompiledRuleset<M>,
    ) -> Result<Self, RulesError>;

    /// The module's rules across tables, every table compiled.
    fn check(rules: &CompiledRuleset<M>) -> Result<(), RulesError>;

    /// Compile the effects of the module's rows and store them on the rows.
    fn attach_effects(
        source: &SourceRules<M>,
        keys: &SourceKeys,
        ix: &KeyIndex,
        rules: &mut CompiledRuleset<M>,
    ) -> Result<(), RulesError>;

    /// The effects of one row (None: no such row).
    fn effect_set(&self, source: ModuleSource<M>) -> Option<&EffectSet<M>>;

    /// Every row with effects, in the module's order.
    fn effect_sets(&self) -> Vec<(ModuleSource<M>, &EffectSet<M>)>;

    /// The module's fields on the kernel's rows (`rules::rows`).
    type Rows: ModuleRows<M>;

    /// The module's role bindings as the content writes them.
    type SourceRoles: Debug + Clone + Default;

    /// The module's role bindings resolved to rows; they hash last in the
    /// ruleset's roles (`rules::roles::Roles`).
    type Roles: Debug + Clone + Default + PartialEq + Eq + std::hash::Hash;

    /// Take the bindings `other` writes (a ruleset split over files).
    fn merge_roles(roles: &mut Self::SourceRoles, other: Self::SourceRoles);

    /// Resolve and check the module's bindings against the compiled rows.
    fn compile_roles(source: &Self::SourceRoles, ix: &KeyIndex, rules: &CompiledRuleset<M>) -> Result<Self::Roles, RulesError>;

    /// Hash the module's tables that follow the kernel's table `kernel_table`
    /// in the fingerprint.
    fn hash_after(&self, kernel_table: &str, f: &mut Fingerprint);

    // ---- what the kernel's rows and state refer to in the module's tables
    // (Classic notions still named by the kernel: production of projects,
    // city-state players, specialists' great people, AI branch roles) ----

    /// A project a city may produce or contribute to (None: no such row;
    /// a module without projects keeps the default).
    fn project(&self, _id: ProjectId) -> Option<ProjectFacts> {
        None
    }

    /// The projects technology `tech` unlocks, in table order.
    fn projects_of_tech(&self, _tech: TechId) -> Vec<ProjectId> {
        Vec::new()
    }
}

/// The module's tables as content writes them.
pub type ModuleSourceTables<M> = <<M as RulesModule>::Tables as ModuleTables<M>>::Source;

/// What the kernel reads of a module's project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectFacts {
    /// Production cost.
    pub cost: u32,
    /// Production cannot build it (unbuildable, or built by contributions).
    pub unbuildable: bool,
    /// Cities contribute to it by a process (Classic: a World Congress project).
    pub contributed: bool,
}
