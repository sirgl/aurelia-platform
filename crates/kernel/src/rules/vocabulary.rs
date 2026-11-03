//! The effect vocabulary contract (platform/PLATFORM.md stage 4, "Extensible
//! vocabulary of effects"): the kernel's words plus the words of the rules module.
//!
//! The kernel's vocabulary (`rules::effects`) names what any hex 4X has:
//! yields, costs, movement, sight, combat, unit stats and abilities, map
//! conditions, the triggers of cities, units and technologies. Each family
//! has one typed extension point, a `Module(..)` variant holding the
//! module's word of that family ([`Vocabulary`]). The module's words and
//! what they mean live in the module (Classic: `classic::effects`).
//!
//! # Classification (Classic vocabulary before the split)
//!
//! | Family | Kernel | Module | Module words |
//! |---|---|---|---|
//! | `Scope` | 4 | 1 | Religion |
//! | `Selector` | 10 | 5 | FollowingCities, FollowingCityTiles, HolyCity, Founder, OwnReligion |
//! | `Trigger` | 9 | 8 | OnAdopt, OnNaturalWonderFoundFirst, OnNaturalWonderFound, OnLongCountCycle, OnGreatPersonExpended, OnCityConverted, OnReligionSpread, OnGreatPersonBorn |
//! | `Condition` | 28 | 8 | OpponentIsBarbarian, MinFollowers, NearFollowingCity, GoldenAge, GreatPersonIs, OpponentIsCityState, OpponentHasMoreCities, GreatGeneralEarned |
//! | `Modifier` | 26 | 21 | AddHappiness, AddGreatPersonPoints, ModifyCityState, FreePolicies, AddYieldPerFollowingCity, AddYieldPerFollowers, AddHappinessPerFollowingCity, ModifyReligion, UnlockFaithBuilding, UnlockFaithUnits, AllowReformationBelief, ModifyTourism, TourismToward, AddHappinessPerCount, GrantSpies, PromoteSpies, GrantReligionAbility, ExtraFoundingBelief, SecondReligionPantheon, InstantTourism, GrantExperiencePerGreatWork |
//! | `UnitStat` | 25 | 10 | GreatGeneralPointsPercent, StackedGeneralPercent, MaintenancePercent, ExperienceNeededPercent, GeneralAuraPercent, TradeMissionGoldPercent, TradeMissionInfluencePercent, AdjacentDamageChance, GoldenAgePointsOnKillPercent, RivalLandStrengthLossPercent |
//! | `UnitAbility` | 34 | 7 | GreatGeneral, GreatAdmiral, HealOnKillNotBarbarians, PostCombatPromotions, ExoticCargo, LendsMovementToGeneral, ChoosesRuinsReward |
//! | `CombatContext` | 23 | 0 | |
//! | `YieldKind` | 6 | 1 | Faith |
//! | `CostTarget` | 10 | 3 | Policies, PolicyCityPenalty, FaithPurchase |
//! | `Count` | 3 | 7 | Policies, MajorityFollowers, ReligionsPresent, PeacefulForeignFollowers, OtherReligionFollowers, TradePartners, CityStateRoutes |
//! | `Grant` | 4 | 3 | FreeGreatPeople, LongCountGreatPerson, GoldenAge |
//! | `SourceRef` | 5 | 2 | Policy, Belief |
//! | `CityCombatStat`, `PlotFilter`, `ProductionFilter`, `Forbidden`, `Duration`, `Stacking` | all | 0 | |
//! | `GreatPersonKind`, `CityStateStat`, `ReligionStat`, `ReligionAbility`, `TourismStat`, `TourismCondition`, `CityRelation` | 0 | all | the whole enum |
//!
//! Kernel: what a hex wargame or another 4X has without Classic — yields of
//! food, production, gold, science and culture; costs of units, buildings,
//! technologies, tiles, hurrying and borders; combat contexts; unit stats of
//! sight, range, moves, healing and air combat; terrain, feature, resource,
//! building, technology, owner and war/peace conditions; triggers of turn
//! start, built, trained, founded, conquered, killed, technology and era.
//! Module: faith, happiness, golden ages, great people (generals and
//! admirals included), policies, religion and beliefs, tourism, city-states,
//! spies, trade routes, barbarians, natural-wonder finders, the Maya Long
//! Count. `rules::stats` (the `AddStat` numbers) is split the same way:
//! `Stat` 9 kernel, 19 module (happiness, great people, golden ages, trade,
//! plunder, rule exceptions, society, espionage, the World Congress,
//! connections, research agreements).
//!
//! # Tags
//!
//! Every word of a family has a tag: its position in the one closed enum
//! the family was before the split. The tag is the word's persisted form
//! (timed effects and triggers in saves), the index of per-family arrays
//! (yields, costs, triggers) and the discriminant the ruleset fingerprint
//! hashes (`rules::fingerprint`), so moving a word between the kernel and
//! the module changes none of them. The kernel keeps its tags; a module's
//! words take the numbers the kernel leaves free, and together they cover
//! `0..count` once (`vocabulary_tests`).

use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use crate::module::RulesModule;
use crate::rules::compile::RulesError;
use crate::rules::compile_effects::SourceKind;
use crate::rules::effects::{CompiledEffect, Scope};
use crate::rules::key_index::KeyIndex;
use crate::rules::CompiledRuleset;

/// A word of a module's vocabulary family. Its `Hash` writes it as the
/// closed enum did: [`hash_tag`] of its tag, then its fields.
pub trait Word: Copy + Eq + Hash + Debug + 'static {
    /// The word's tag in its family (see the module docs).
    fn tag(self) -> u8;
}

/// What the kernel asks of a module's selector.
pub trait SelectorWord<M: RulesModule>: Word {
    /// The scope the selector addresses.
    fn scope(self) -> Scope<M>;
}

/// What the kernel asks of a module's trigger.
pub trait TriggerWord: Word {
    /// Its event has a unit (`Selector::EventUnit` may address it).
    fn has_event_unit(self) -> bool;
}

/// What the kernel asks of a module's condition.
pub trait ConditionWord: Word {
    /// A condition on the opponent of a battle or the victim of a kill.
    fn is_opponent(self) -> bool;
    /// It holds only in a situation (a battle, an entered tile, a place).
    fn is_situational(self) -> bool;
    /// It holds by the city whose territory the unit stands in: healing by
    /// place reads it, movement does not.
    fn is_territory(self) -> bool;
}

/// What the kernel asks of a module's modifier.
pub trait ModifierWord: Word {
    /// It changes the world once (applied by the module's trigger code).
    fn is_instant(self) -> bool;
}

/// What the kernel asks of a module's stat (`rules::stats`). Its `Ord`
/// follows the tags.
pub trait StatWord<M: RulesModule>: Word + Ord {
    /// The scopes it may target.
    fn fits(self, scope: Scope<M>) -> bool;
}

/// What the kernel asks of a module's yield channel.
pub trait YieldWord: Word {
    /// The player has a stock of it that instant player grants land in.
    fn has_player_stock(self) -> bool;
    /// A player-level rate exists (Player-scope yield modifiers).
    fn is_player_yield(self) -> bool;
}

/// What the kernel asks of a module's cost.
pub trait CostWord: Word {
    /// A player cost (the others are city costs).
    fn is_player_cost(self) -> bool;
}

/// What the kernel asks of a module's per-count modifier count.
pub trait CountWord<M: RulesModule>: Word {
    /// The scopes it is defined for.
    fn fits(self, scope: Scope<M>) -> bool;
}

/// What the kernel asks of a module's table whose rows carry effects.
pub trait TableWord: Copy + Eq + Debug + 'static {
    /// A row has a city of its own (a belief: the following city nearest
    /// to the event), as a building has.
    fn has_source_city(self) -> bool;
}

/// What the kernel asks of a row of a module's table that carries effects.
pub trait SourceWord: Word {
    /// The row's id in its table (saved with the tag).
    fn row(self) -> u16;
    /// The row of the table with this tag (None: no table has the tag).
    fn from_parts(tag: u8, row: u16) -> Option<Self>;
}

/// What the kernel asks of a module's instant grant.
pub trait GrantWord<M: RulesModule>: Word {
    /// The scope it targets.
    fn scope(self) -> Scope<M>;
}

/// The words a rules module adds to the kernel's effect vocabulary, one
/// type per family. Every slice lists all words of its family.
pub trait Vocabulary<M: RulesModule>: 'static {
    type Scope: Word;
    type Selector: SelectorWord<M>;
    type Trigger: TriggerWord;
    type Condition: ConditionWord;
    type Modifier: ModifierWord;
    type UnitStat: Word;
    type UnitAbility: Word;
    /// The module's stats (`rules::stats`): a module stat names no row, so
    /// content writes it as it is.
    type Stat: StatWord<M>;
    type Yield: YieldWord;
    type Cost: CostWord;
    type Count: CountWord<M>;
    type Grant: GrantWord<M>;
    /// The module's tables whose rows carry effects, and one row of them.
    type Table: TableWord;
    type Source: SourceWord;
    /// The module's modifiers and conditions as content writes them
    /// (references by key, `rules::source`).
    type SourceModifier: Clone + Debug + PartialEq + Eq;
    type SourceCondition: Clone + Debug + PartialEq + Eq;

    /// The module's runtime indexes over its words (`EffectIndex::module`).
    type Index: Debug + Clone + Default + PartialEq + Eq + Hash;

    const TRIGGERS: &'static [Self::Trigger];
    /// At most one: `Yields` keeps the module's yield in its one extra
    /// channel (Classic: faith).
    const YIELDS: &'static [Self::Yield];
    const COSTS: &'static [Self::Cost];
    const UNIT_STATS: &'static [Self::UnitStat];

    /// Build the module's indexes once every effect is compiled.
    fn index(rules: &CompiledRuleset<M>) -> Self::Index;

    // ---- the compiler's questions (`rules::compile_effects`) ----

    /// The module's modifier with its references resolved.
    fn resolve_modifier(ix: &KeyIndex, source: &Self::SourceModifier) -> Result<Self::Modifier, RulesError>;
    /// The module's condition with its references resolved.
    fn resolve_condition(ix: &KeyIndex, source: &Self::SourceCondition) -> Result<Self::Condition, RulesError>;
    /// The module's selector is available to this effect of a `kind` source.
    fn selector_fits(selector: Self::Selector, kind: SourceKind<M>, effect: &CompiledEffect<M>) -> bool;
    /// The module's condition fits this effect of a `kind` source.
    fn condition_fits(condition: Self::Condition, kind: SourceKind<M>, effect: &CompiledEffect<M>) -> bool;
    /// The module's modifier may target the scope.
    fn modifier_fits_scope(modifier: Self::Modifier, scope: Scope<M>) -> bool;
    /// The module's trigger is available to a `kind` source.
    fn trigger_fits(trigger: Self::Trigger, kind: SourceKind<M>) -> bool;
    /// The module's other rules on an effect of a `kind` source (amounts,
    /// the trigger or the source kind a word needs). Runs after the
    /// kernel's checks.
    fn check_effect(kind: SourceKind<M>, effect: &CompiledEffect<M>) -> Result<(), RulesError>;
}

/// The words of a module's vocabulary, family by family.
pub type ModuleVocabulary<M> = <M as RulesModule>::Vocabulary;
pub type ModuleScope<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Scope;
pub type ModuleSelector<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Selector;
pub type ModuleTrigger<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Trigger;
pub type ModuleCondition<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Condition;
pub type ModuleModifier<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Modifier;
pub type ModuleUnitStat<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::UnitStat;
pub type ModuleStat<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Stat;
pub type ModuleUnitAbility<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::UnitAbility;
pub type ModuleYield<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Yield;
pub type ModuleCost<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Cost;
pub type ModuleCount<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Count;
pub type ModuleGrant<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Grant;
pub type ModuleTable<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Table;
pub type ModuleSource<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Source;
pub type ModuleSourceModifier<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::SourceModifier;
pub type ModuleSourceCondition<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::SourceCondition;
pub type ModuleIndex<M> = <<M as RulesModule>::Vocabulary as Vocabulary<M>>::Index;

/// The module's words fit the kernel's per-family arrays (checked when a
/// game's rules compile; a module that does not fit fails its build).
pub const fn assert_fits<M: RulesModule>() {
    use crate::rules::effects::{MAX_COST_TARGETS, MAX_YIELD_KINDS};
    let vocabulary_yields = <M::Vocabulary as Vocabulary<M>>::YIELDS.len();
    assert!(vocabulary_yields <= 1, "Yields keeps one module yield");
    assert!(6 + vocabulary_yields <= MAX_YIELD_KINDS);
    assert!(10 + <M::Vocabulary as Vocabulary<M>>::COSTS.len() <= MAX_COST_TARGETS);
    assert!(25 + <M::Vocabulary as Vocabulary<M>>::UNIT_STATS.len() <= crate::effects::unit::MAX_UNIT_STATS);
}

/// Hash a word as the derived `Hash` of the closed enum hashed its
/// variant: the tag as an `isize` discriminant.
pub fn hash_tag<H: Hasher>(tag: u8, state: &mut H) {
    (tag as isize).hash(state);
}

