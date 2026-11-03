//! The empty vocabulary: a rules module that adds no words to the kernel's
//! effect vocabulary takes [`NoVocabulary`] as its `RulesModule::Vocabulary`
//! (the kernel's test module and `sandbox-rules` do). Every family is
//! [`NoWord`], an enum without values, so no content can name a module
//! word and none of the evaluation hooks is ever reached with one.

use crate::effects::count::Counted;
use crate::effects::module::{ModuleEffects, Visit};
use crate::effects::{Bucket, ConditionContext};
use crate::ids::{CityHandle, PlayerId, ResourceId};
use crate::module::RulesModule;
use crate::rules::compile::RulesError;
use crate::rules::compile_effects::SourceKind;
use crate::rules::effects::{CompiledEffect, Count, Scope};
use crate::rules::key_index::KeyIndex;
use crate::rules::vocabulary::{
    ConditionWord, CostWord, CountWord, GrantWord, ModifierWord, SelectorWord, SourceWord, StatWord, TableWord,
    TriggerWord, Vocabulary, Word, YieldWord,
};
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::module::{ModuleChange, StaleYields};
use crate::world::state::AuthWorld;
use crate::world::unit::UnitCore;

/// A word of a family the module leaves empty: no value exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoWord {}

/// The vocabulary of a module without words of its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoVocabulary;

impl Word for NoWord {
    fn tag(self) -> u8 {
        match self {}
    }
}

impl<M: RulesModule> StatWord<M> for NoWord {
    fn fits(self, _: Scope<M>) -> bool {
        match self {}
    }
}

impl<M: RulesModule> SelectorWord<M> for NoWord {
    fn scope(self) -> Scope<M> {
        match self {}
    }
}

impl TriggerWord for NoWord {
    fn has_event_unit(self) -> bool {
        match self {}
    }
}

impl ConditionWord for NoWord {
    fn is_opponent(self) -> bool {
        match self {}
    }
    fn is_situational(self) -> bool {
        match self {}
    }
    fn is_territory(self) -> bool {
        match self {}
    }
}

impl ModifierWord for NoWord {
    fn is_instant(self) -> bool {
        match self {}
    }
}

impl YieldWord for NoWord {
    fn has_player_stock(self) -> bool {
        match self {}
    }
    fn is_player_yield(self) -> bool {
        match self {}
    }
}

impl CostWord for NoWord {
    fn is_player_cost(self) -> bool {
        match self {}
    }
}

impl<M: RulesModule> CountWord<M> for NoWord {
    fn fits(self, _: Scope<M>) -> bool {
        match self {}
    }
}

impl<M: RulesModule> GrantWord<M> for NoWord {
    fn scope(self) -> Scope<M> {
        match self {}
    }
}

impl TableWord for NoWord {
    fn has_source_city(self) -> bool {
        match self {}
    }
}

impl SourceWord for NoWord {
    fn row(self) -> u16 {
        match self {}
    }
    fn from_parts(_: u8, _: u16) -> Option<Self> {
        None
    }
}

/// A module without state changes of its own journals none.
impl ModuleChange for NoWord {
    fn stale_yields(&self) -> StaleYields {
        match *self {}
    }
}

impl<M: RulesModule> Vocabulary<M> for NoVocabulary {
    type Stat = NoWord;
    type Scope = NoWord;
    type Selector = NoWord;
    type Trigger = NoWord;
    type Condition = NoWord;
    type Modifier = NoWord;
    type UnitStat = NoWord;
    type UnitAbility = NoWord;
    type Yield = NoWord;
    type Cost = NoWord;
    type Count = NoWord;
    type Grant = NoWord;
    type Table = NoWord;
    type Source = NoWord;
    type SourceModifier = NoWord;
    type SourceCondition = NoWord;
    type Index = ();

    const TRIGGERS: &'static [NoWord] = &[];
    const YIELDS: &'static [NoWord] = &[];
    const COSTS: &'static [NoWord] = &[];
    const UNIT_STATS: &'static [NoWord] = &[];

    fn index(_: &CompiledRuleset<M>) {}

    fn resolve_modifier(_: &KeyIndex, source: &NoWord) -> Result<NoWord, RulesError> {
        match *source {}
    }
    fn resolve_condition(_: &KeyIndex, source: &NoWord) -> Result<NoWord, RulesError> {
        match *source {}
    }
    fn selector_fits(selector: NoWord, _: SourceKind<M>, _: &CompiledEffect<M>) -> bool {
        match selector {}
    }
    fn condition_fits(condition: NoWord, _: SourceKind<M>, _: &CompiledEffect<M>) -> bool {
        match condition {}
    }
    fn modifier_fits_scope(modifier: NoWord, _: Scope<M>) -> bool {
        match modifier {}
    }
    fn trigger_fits(trigger: NoWord, _: SourceKind<M>) -> bool {
        match trigger {}
    }
    fn check_effect(_: SourceKind<M>, _: &CompiledEffect<M>) -> Result<(), RulesError> {
        Ok(())
    }
}

/// No module sources, conditions or counts: the kernel's effects alone.
impl<M: RulesModule> ModuleEffects<M> for NoVocabulary {
    type CityModifiers = ();
    type PlayerModifiers = ();

    fn apply_city(_: &mut (), modifier: NoWord, _: u32) {
        match modifier {}
    }
    fn apply_player(_: &mut (), modifier: NoWord, _: u32) {
        match modifier {}
    }
    fn condition_holds(_: &AuthWorld<M>, _: &CompiledRuleset<M>, _: ConditionContext<'_>, condition: NoWord) -> bool {
        match condition {}
    }
    fn count(_: &AuthWorld<M>, _: &CompiledRuleset<M>, _: Counted<'_>, count: NoWord) -> u32 {
        match count {}
    }
    fn resolve_count(modifier: NoWord, _: &mut dyn FnMut(Count<M>) -> u32) -> Option<NoWord> {
        match modifier {}
    }
    fn visit_owner_sources(_: &AuthWorld<M>, _: &CompiledRuleset<M>, _: PlayerId, _: Bucket<M>, _: Visit<'_, M>) {}
    fn visit_city_sources(_: &AuthWorld<M>, _: &CompiledRuleset<M>, _: &CityCore, _: Bucket<M>, _: Visit<'_, M>) {}
    fn visit_player_sources(_: &AuthWorld<M>, _: &CompiledRuleset<M>, _: PlayerId, _: Visit<'_, M>) {}
    fn visit_unit_sources(_: &AuthWorld<M>, _: &CompiledRuleset<M>, _: PlayerId, _: Visit<'_, M>) {}
    fn unit_combat_percent(_: &AuthWorld<M>, _: &CompiledRuleset<M>, _: &UnitCore) -> i32 {
        0
    }
    fn city_resources(_: &AuthWorld<M>, _: &CompiledRuleset<M>, _: CityHandle) -> Vec<ResourceId> {
        Vec::new()
    }
}
