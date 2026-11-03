//! The rules module's part of effect evaluation (platform/PLATFORM.md stage 4).
//!
//! The engine visits the sources that reach a player, a city or a unit and
//! accumulates their modifiers. A module adds sources of its own (Classic:
//! adopted policies and the beliefs of a religion), decides its
//! conditions and counts, and keeps the numbers its modifiers change in
//! accumulators of its own, which the kernel's hold as `module`.

use std::fmt::Debug;

use crate::effects::count::Counted;
use crate::effects::{Bucket, ConditionContext};
use crate::ids::{CityHandle, PlayerId, ResourceId};
use crate::rules::effects::{CompiledEffect, Count};
use crate::module::RulesModule;
use crate::rules::vocabulary::Vocabulary;
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::unit::UnitCore;
use crate::world::state::AuthWorld;

/// Visits one effect and the number of its instances.
pub type Visit<'v, M> = &'v mut dyn FnMut(&CompiledEffect<M>, u32);

/// How the effect engine evaluates a module's words.
pub trait ModuleEffects<M: RulesModule>: Vocabulary<M> {
    /// What the module's modifiers change on one city and on one player.
    type CityModifiers: Debug + Clone + Default + PartialEq + Eq;
    type PlayerModifiers: Debug + Clone + Default + PartialEq + Eq;

    /// One passive module modifier, `times` instances, on a city.
    fn apply_city(mods: &mut Self::CityModifiers, modifier: Self::Modifier, times: u32);

    /// One passive module modifier, `times` instances, on a player.
    fn apply_player(mods: &mut Self::PlayerModifiers, modifier: Self::Modifier, times: u32);

    /// Whether the module's condition holds where the engine gates owner,
    /// city and tile effects (situational ones never hold there).
    fn condition_holds(
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
        at: ConditionContext<'_>,
        condition: Self::Condition,
    ) -> bool;

    /// The number of the module's counted things on a target.
    fn count(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, target: Counted<'_>, count: Self::Count) -> u32;

    /// A per-count module modifier in its plain form, `count` reading the
    /// target (None: not a per-count modifier).
    fn resolve_count(modifier: Self::Modifier, count: &mut dyn FnMut(Count<M>) -> u32) -> Option<Self::Modifier>;

    /// The module's sources at the owner level (after the owner's
    /// technologies, before its owner-wide buildings).
    fn visit_owner_sources(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, owner: PlayerId, bucket: Bucket<M>, visit: Visit<'_, M>);

    /// The module's sources that reach one city beyond its owner's.
    fn visit_city_sources(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, core: &CityCore, bucket: Bucket<M>, visit: Visit<'_, M>);

    /// The module's Player-scope sources that reach a player beyond its own.
    fn visit_player_sources(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, player: PlayerId, visit: Visit<'_, M>);

    /// The module's Unit-scope sources that reach every unit of `owner`.
    fn visit_unit_sources(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, owner: PlayerId, visit: Visit<'_, M>);

    /// The module's combat percent on every battle of a unit, beyond its
    /// effects (Classic: an unhappy owner, a strategic resource in deficit).
    fn unit_combat_percent(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, unit: &UnitCore) -> i32;

    /// The resources a city has on its own tiles
    /// (connected by an improvement), which wonder production bonuses read.
    fn city_resources(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, city: CityHandle) -> Vec<ResourceId>;
}

pub type ModuleCityModifiers<M> = <<M as RulesModule>::Vocabulary as ModuleEffects<M>>::CityModifiers;
pub type ModulePlayerModifiers<M> = <<M as RulesModule>::Vocabulary as ModuleEffects<M>>::PlayerModifiers;
