//! Effect system (platform/project.md §20–21): typed, small, not Turing-complete.
//!
//! An effect is attached to a content *source* (building, technology, unit
//! kind, promotion, nation, policy, belief) and says:
//!
//! ```text
//! scope      what kind of entity receives the modifier: Player, City, Unit, Tile
//! selector   which entities of that scope, relative to the source
//! condition  gate checked when the effect is evaluated
//! trigger    Passive (while the source exists) or a typed event
//! modifier   what changes (yield stage, cost, combat, unlock, free unit, ...)
//! duration   WhileSourceExists (passive), Once (instant) or Turns(n) (timed)
//! stacking   Stack (every source instance counts) or Unique (counted once)
//! ```
//!
//! Ordering: numeric modifiers are combined by stage, never by source order
//! (see `rules::modifiers` for base → flat → additive % → multiplicative →
//! caps → rounding). Instant effects of one event run in canonical source
//! order: nation, technologies (learning order), buildings (city order),
//! unit kind, promotions; inside a source — declaration order.
//!
//! The compiler (`rules::compile_effects`) rejects every invalid combination
//! before the game starts, so runtime code matches on these enums without
//! fallbacks. Rules never read the world (platform/structure.md §47): the gameplay
//! systems interpret these values (`crate::effects`, `crate::systems`).

use crate::module::RulesModule;
use std::hash::{Hash, Hasher};

use crate::ids::{
    BuildingKindId, FeatureId, ImprovementId, PromotionId, ResourceId, TechId, UnitKindId,
};
use crate::rules::vocabulary::{
    hash_tag, ConditionWord, CostWord, CountWord, GrantWord, ModifierWord,
    ModuleCondition, ModuleCost, ModuleCount, ModuleGrant, ModuleModifier, ModuleScope,
    ModuleSelector, ModuleSource, ModuleTrigger, ModuleUnitAbility, ModuleUnitStat, ModuleYield,
    ModuleIndex, SelectorWord, Vocabulary, Word, YieldWord,
};

/// `Hash` of a family split into kernel and module words: the word's tag
/// (`rules::vocabulary`), then the fields of a kernel word; a module word
/// hashes itself the same way.
macro_rules! hash_by_tag {
    ($family:ident) => {
        impl<M: RulesModule> Hash for $family<M> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                match self {
                    Self::Module(word) => word.hash(state),
                    kernel => hash_tag(kernel.tag(), state),
                }
            }
        }
    };
}

/// What kind of entity receives the modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum Scope<M: RulesModule> {
    Player,
    City,
    Unit,
    Tile,
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleScope<M>),
}

impl<M: RulesModule> Scope<M> {
    pub fn tag(self) -> u8 {
        match self {
            Scope::Player => 0,
            Scope::City => 1,
            Scope::Unit => 2,
            Scope::Tile => 3,
            Scope::Module(scope) => scope.tag(),
        }
    }
}

hash_by_tag!(Scope);

/// Which entities of the scope receive the modifier, relative to the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum Selector<M: RulesModule> {
    /// Player scope: the player who owns the source.
    Owner,
    /// City scope: the city that holds the source building.
    SourceCity,
    /// City scope: every city of the owner.
    OwnerCities,
    /// City scope: the city of the trigger event (OnCityFounded).
    EventCity,
    /// Unit scope: the unit that is the source (unit kind or promotion).
    SourceUnit,
    /// Unit scope: every unit of the owner.
    OwnerUnits,
    /// Unit scope: the unit of the trigger event (OnUnitKilled: the killer).
    EventUnit,
    /// Tile scope: tiles worked by the city that holds the source building.
    SourceCityTiles,
    /// Tile scope: tiles worked by every city of the owner.
    OwnerCityTiles,
    /// City scope: the owner's capital (the city with the Palace; Classic
    /// `capital` selector). Owner-wide: a building source reaches it too.
    Capital,
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleSelector<M>),
}

impl<M: RulesModule> Selector<M> {
    /// The scope this selector addresses (the compiler requires a match).
    pub fn scope(self) -> Scope<M> {
        match self {
            Selector::Owner => Scope::Player,
            Selector::SourceCity
            | Selector::OwnerCities
            | Selector::EventCity
            | Selector::Capital => Scope::City,
            Selector::SourceUnit | Selector::OwnerUnits | Selector::EventUnit => Scope::Unit,
            Selector::SourceCityTiles | Selector::OwnerCityTiles => Scope::Tile,
            Selector::Module(selector) => selector.scope(),
        }
    }

    pub fn tag(self) -> u8 {
        match self {
            Selector::Owner => 0,
            Selector::SourceCity => 1,
            Selector::OwnerCities => 2,
            Selector::EventCity => 3,
            Selector::SourceUnit => 4,
            Selector::OwnerUnits => 5,
            Selector::EventUnit => 6,
            Selector::SourceCityTiles => 7,
            Selector::OwnerCityTiles => 8,
            Selector::Capital => 9,
            Selector::Module(selector) => selector.tag(),
        }
    }

    /// Reaches entities of the owner beyond the source itself (empire-wide).
    pub fn is_owner_wide(self) -> bool {
        matches!(
            self,
            Selector::Owner
                | Selector::OwnerCities
                | Selector::OwnerUnits
                | Selector::OwnerCityTiles
                | Selector::Capital
        )
    }
}

hash_by_tag!(Selector);

/// When the effect applies. Passive effects are evaluated continuously while
/// the source exists; the others fire in the turn phase of their event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum Trigger<M: RulesModule> {
    Passive,
    /// The source building is completed / the source unit kind is trained
    /// (production completion; free buildings and units do not fire it).
    OnBuilt,
    /// Start of the owner's turn (first step of the end-turn sequence).
    OnTurnStart,
    /// The owner learns a technology. A technology source fires only when it
    /// is the technology being learned.
    OnTechnologyUnlocked,
    /// The owner founds a city.
    OnCityFounded,
    /// A unit of the owner kills an enemy unit. Unit kind and promotion
    /// sources fire only for the killer itself.
    OnUnitKilled,
    /// A city of the owner trains a unit: building
    /// sources fire only for units trained in their own city; the unit is
    /// the EventUnit (barracks-like promotions, free experience).
    OnUnitTrained,
    /// The owner captures a city in combat (buyouts and deals do not
    /// count). The captured city is
    /// the EventCity.
    OnCityConquered,
    /// The owner enters a new era by learning a technology of a later era
    /// (Classic, rules.md, espionage).
    OnEraEntered,
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleTrigger<M>),
}

impl<M: RulesModule> Trigger<M> {
    /// The kernel's triggers.
    const KERNEL: [Trigger<M>; 9] = [
        Trigger::Passive,
        Trigger::OnBuilt,
        Trigger::OnTurnStart,
        Trigger::OnTechnologyUnlocked,
        Trigger::OnCityFounded,
        Trigger::OnUnitKilled,
        Trigger::OnUnitTrained,
        Trigger::OnCityConquered,
        Trigger::OnEraEntered,
    ];

    /// Every trigger, the kernel's first (`index()` is the tag, not the
    /// position here).
    pub fn all() -> impl Iterator<Item = Trigger<M>> {
        let module = M::Vocabulary::TRIGGERS.iter().map(|&t| Trigger::Module(t));
        Self::KERNEL.into_iter().chain(module)
    }

    /// The length of per-trigger arrays: one past the highest tag (the
    /// kernel's and the module's tags together cover `0..slots` once when
    /// the module fills the kernel's gaps, as Classic does).
    pub fn slots() -> usize {
        Self::all().map(|t| usize::from(t.tag()) + 1).max().unwrap_or(0)
    }

    pub fn tag(self) -> u8 {
        match self {
            Trigger::Passive => 0,
            Trigger::OnBuilt => 1,
            Trigger::OnTurnStart => 2,
            Trigger::OnTechnologyUnlocked => 3,
            Trigger::OnCityFounded => 4,
            Trigger::OnUnitKilled => 5,
            Trigger::OnUnitTrained => 6,
            Trigger::OnCityConquered => 10,
            Trigger::OnEraEntered => 11,
            Trigger::Module(trigger) => trigger.tag(),
        }
    }

    /// The slot of the trigger in per-trigger arrays: its tag.
    pub fn index(self) -> usize {
        usize::from(self.tag())
    }

    /// The trigger with this tag (saves keep triggers by tag).
    pub fn from_tag(tag: u8) -> Option<Trigger<M>> {
        Self::all().find(|t| t.tag() == tag)
    }
}

hash_by_tag!(Trigger);

/// How long the modifier lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize))]
pub enum Duration {
    /// Passive effects: as long as the source exists.
    WhileSourceExists,
    /// Triggered instant effects: applied once (grants, free units, ...).
    Once,
    /// Triggered timed effects: active for N turns of the owner.
    Turns(u16),
}

/// How several instances of the same effect combine on one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize))]
pub enum Stacking {
    /// Every source instance adds its modifier.
    Stack,
    /// Counted once per target, however many source instances exist
    /// (and a re-triggered timed effect refreshes instead of stacking).
    Unique,
}

/// Yield channels. `Output` is the v1 combined city output (tile food and
/// production plus 2 per citizen) that feeds the production stock and the
/// treasury; per-yield channels replace it with the M4 economy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum YieldKind<M: RulesModule> {
    Output,
    Food,
    Production,
    Gold,
    Science,
    Culture,
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleYield<M>),
}

/// Capacity of per-yield arrays: the kernel's 6 channels and room for the
/// module's (`Vocabulary::YIELDS`, at most one).
pub const MAX_YIELD_KINDS: usize = 8;

impl<M: RulesModule> YieldKind<M> {
    const KERNEL: [YieldKind<M>; 6] = [
        YieldKind::Output,
        YieldKind::Food,
        YieldKind::Production,
        YieldKind::Gold,
        YieldKind::Science,
        YieldKind::Culture,
    ];

    /// Every yield channel in `index()` order.
    pub fn all() -> impl Iterator<Item = YieldKind<M>> {
        let module = M::Vocabulary::YIELDS.iter().map(|&y| YieldKind::Module(y));
        Self::KERNEL.into_iter().chain(module)
    }

    /// The slot of the yield in per-yield arrays: its tag.
    pub fn index(self) -> usize {
        usize::from(self.tag())
    }

    pub fn tag(self) -> u8 {
        match self {
            YieldKind::Output => 0,
            YieldKind::Food => 1,
            YieldKind::Production => 2,
            YieldKind::Gold => 3,
            YieldKind::Science => 4,
            YieldKind::Culture => 5,
            YieldKind::Module(kind) => kind.tag(),
        }
    }

    /// Player-level stock exists (empire treasury, science, a module's).
    pub fn is_player_yield(self) -> bool {
        match self {
            YieldKind::Gold | YieldKind::Science => true,
            YieldKind::Module(kind) => kind.is_player_yield(),
            _ => false,
        }
    }

    /// A one-shot grant has a stock to land in: the treasury, the research,
    /// the city's food, production and culture, a module's pool. Output is
    /// a combined rate without a stock.
    pub(crate) fn has_stock(self) -> bool {
        self != YieldKind::Output
    }

    /// The player has a pool of it (treasury, research, culture for
    /// policies, a module's): instant player grants land there.
    pub fn has_player_stock(self) -> bool {
        match self {
            YieldKind::Gold | YieldKind::Science | YieldKind::Culture => true,
            YieldKind::Module(kind) => kind.has_player_stock(),
            _ => false,
        }
    }
}

hash_by_tag!(YieldKind);

impl<M: RulesModule> PartialOrd for YieldKind<M> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// In tag order, as the closed enum derived it.
impl<M: RulesModule> Ord for YieldKind<M> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.tag().cmp(&other.tag())
    }
}

/// What a cost modifier changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum CostTarget<M: RulesModule> {
    /// Production cost of units in the target city.
    Units,
    /// Production cost of buildings (wonders included) in the target city.
    Buildings,
    /// Research cost of technologies for the target player.
    Technologies,
    /// Gold price of a tile bought by the target city (`tile_purchase.cost`).
    TilePurchase,
    /// Gold price of hurrying production in the target city (`purchase.cost`).
    Hurry,
    /// Culture needed for the next border tile of the target city
    /// (`border.culture_cost`).
    BorderGrowth,
    /// Percent of the exponent of the border tile cost of the target city
    /// (`border.culture_cost_exponent`).
    BorderGrowthExponent,
    /// Gold price of upgrading a unit, for the target player
    /// (`unit.upgrade_cost.pct`).
    UnitUpgrade,
    /// Gold price of buying a unit in the target city, after the hurry
    /// percents (`purchase.units.cost.pct`, Classic
    /// Mobilization).
    UnitPurchase,
    /// Gold price of buying a building in the target city, after the hurry
    /// percents (`purchase.buildings.cost.pct`, Classic
    /// Skyscrapers).
    BuildingPurchase,
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleCost<M>),
}

/// Capacity of per-cost arrays: the kernel's 10 costs and room for the
/// module's (`Vocabulary::COSTS`).
pub const MAX_COST_TARGETS: usize = 16;

impl<M: RulesModule> CostTarget<M> {
    /// The slot of the cost in per-cost arrays: its tag.
    pub fn index(self) -> usize {
        usize::from(self.tag())
    }

    pub fn tag(self) -> u8 {
        match self {
            CostTarget::Units => 0,
            CostTarget::Buildings => 1,
            CostTarget::Technologies => 2,
            CostTarget::TilePurchase => 3,
            CostTarget::Hurry => 4,
            CostTarget::BorderGrowth => 5,
            CostTarget::BorderGrowthExponent => 6,
            CostTarget::UnitUpgrade => 10,
            CostTarget::UnitPurchase => 11,
            CostTarget::BuildingPurchase => 12,
            CostTarget::Module(cost) => cost.tag(),
        }
    }

    /// Player-level costs (the rest are city costs).
    pub fn is_player_cost(self) -> bool {
        match self {
            CostTarget::Technologies => true,
            CostTarget::Module(cost) => cost.is_player_cost(),
            _ => false,
        }
    }
}

hash_by_tag!(CostTarget);

/// When a combat modifier applies (`combat.<context>.pct`,
/// classic/docs/classic/rules.md). The combat system (`crate::combat`)
/// decides per strike which contexts hold; the percents of all holding
/// contexts are summed in the additive stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize))]
pub enum CombatContext {
    /// Every strike, attacking or defending (`combat.strength`).
    Any,
    /// The unit attacks, melee or ranged (`combat.attack`).
    Attack,
    /// The unit defends (`combat.defense`).
    Defense,
    /// The unit attacks a city (`combat.attack_vs_city`).
    VsCity,
    /// Melee attack on open ground (Shock).
    AttackOpen,
    /// Melee attack on rough ground (Drill).
    AttackRough,
    /// Defense on open ground.
    DefenseOpen,
    /// Defense on rough ground.
    DefenseRough,
    /// Ranged attack on a target on open ground (Accuracy).
    RangedAttackOpen,
    /// Ranged attack on a target on rough ground (Barrage).
    RangedAttackRough,
    /// Defense against a ranged or air attack (Cover).
    DefenseVsRanged,
    /// Any ranged attack of the unit.
    RangedAttack,
    /// Attack on a wounded unit.
    VsWounded,
    /// Attack on a fortified unit (Volley).
    VsFortified,
    /// The battle tile is friendly territory.
    FriendlyLand,
    /// The battle tile is not friendly territory.
    OutsideFriendlyLand,
    /// Melee attack into friendly territory.
    AttackFriendlyLand,
    /// Bonus of the flanking bonus, percent of it.
    Flanking,
    /// A friendly combat unit stands next to the unit (Discipline).
    AdjacentFriendly,
    /// Within the falloff range of the owner's capital.
    NearCapital,
    /// Defense while embarked, percent of the era value.
    EmbarkedDefense,
    /// Strength of an interception.
    Interception,
    /// Strength of an air sweep.
    AirSweep,
}

/// Numeric unit properties a promotion, policy or building changes (flat
/// stage; percent-valued stats are named `…Percent`). Read by the system
/// that owns the property (combat, movement, visibility, healing, air).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum UnitStat<M: RulesModule> {
    /// Sight radius in tiles.
    Sight,
    /// Sight radius while embarked.
    EmbarkedSight,
    /// Ranged attack range.
    Range,
    /// Attacks per turn.
    AttacksPerTurn,
    /// Aircraft or missiles the unit carries.
    Cargo,
    /// Chance to intercept an air attack, percent.
    InterceptChance,
    /// Interceptions per turn.
    InterceptionsPerTurn,
    /// Chance to evade an interception, percent.
    EvasionPercent,
    /// Damage taken from interceptions, percent.
    InterceptionDamageTakenPercent,
    /// Paradrop range.
    ParadropRange,
    /// Extra moves at sea.
    NavalMoves,
    /// Experience gained, percent (`ExperiencePercent`).
    ExperiencePercent,
    /// Chance to withdraw before a melee attack, percent.
    WithdrawChance,
    /// Gold plundered from a city, percent of the damage dealt.
    CityAttackPlunderPercent,
    /// Healing in enemy territory.
    HealEnemyLand,
    /// Healing in neutral territory.
    HealNeutralLand,
    /// Healing in friendly territory.
    HealFriendlyLand,
    /// Healing given to own units on adjacent tiles.
    HealAdjacent,
    /// HP healed after destroying an enemy unit.
    HealOnKill,
    /// Combat percent of enemy units within `AuraRange`.
    EnemyAuraPercent,
    /// Range of the enemy aura.
    AuraRange,
    /// Damage dealt to enemies next to the target when attacking.
    AdjacentDamageOnAttack,
    /// NearCapital percent change per tile of distance (`CapitalDefenseFalloff`).
    CapitalDefenseFalloff,
    /// Extra moves while embarked.
    EmbarkedMoves,
    /// Change of the wounded damage multiplier, percent points.
    WoundedDamagePercent,
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleUnitStat<M>),
}

impl<M: RulesModule> UnitStat<M> {
    /// Also the slot of the stat in per-stat arrays.
    pub fn tag(self) -> u8 {
        match self {
            UnitStat::Sight => 0,
            UnitStat::EmbarkedSight => 1,
            UnitStat::Range => 2,
            UnitStat::AttacksPerTurn => 3,
            UnitStat::Cargo => 4,
            UnitStat::InterceptChance => 5,
            UnitStat::InterceptionsPerTurn => 6,
            UnitStat::EvasionPercent => 7,
            UnitStat::InterceptionDamageTakenPercent => 8,
            UnitStat::ParadropRange => 9,
            UnitStat::NavalMoves => 10,
            UnitStat::ExperiencePercent => 11,
            UnitStat::WithdrawChance => 12,
            UnitStat::CityAttackPlunderPercent => 13,
            UnitStat::HealEnemyLand => 14,
            UnitStat::HealNeutralLand => 15,
            UnitStat::HealFriendlyLand => 16,
            UnitStat::HealAdjacent => 17,
            UnitStat::HealOnKill => 18,
            UnitStat::EnemyAuraPercent => 19,
            UnitStat::AuraRange => 20,
            UnitStat::AdjacentDamageOnAttack => 21,
            UnitStat::CapitalDefenseFalloff => 24,
            UnitStat::EmbarkedMoves => 25,
            UnitStat::WoundedDamagePercent => 26,
            UnitStat::Module(stat) => stat.tag(),
        }
    }
}

hash_by_tag!(UnitStat);

/// Combat properties of a city that buildings and policies change
/// (classic/docs/classic/rules.md). Read by `crate::combat::city`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize))]
pub enum CityCombatStat {
    /// Building defense in hundredths of strength
    /// (walls 500 = +5).
    Defense,
    /// Extra hit points.
    MaxHitPoints,
    /// Percent of the building defense (Red Fort).
    DefensePercent,
    /// Ranged strike percent of a garrisoned city
    ///.
    GarrisonRangedStrikePercent,
    /// Damage of nuclear attacks on the city, percent.
    NukeDamagePercent,
    /// Extra aircraft the city holds.
    AirCapacity,
    /// Ranged strike percent of a city, garrisoned or not (beliefs).
    RangedStrikePercent,
}


/// Boolean unit abilities (Classic promotion flags). Each has one consumer
/// in the system that owns the behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum UnitAbility<M: RulesModule> {
    /// Attacks do not end the turn (Blitz).
    MoveAfterAttacking,
    /// Heals even after moving or attacking (`AlwaysHeal`, March).
    AlwaysHeal,
    /// Land unit may enter water.
    Embarkation,
    /// Embarked unit may enter deep water (`EmbarkedAllWater`).
    EmbarkedAllWater,
    /// Enters impassable terrain.
    MoveImpassable,
    /// Free half-strength ranged strike before melee (`RangedSupportFire`).
    RangedSupportFire,
    /// Hills cost half (`HillsDoubleMove`).
    HillsDoubleMove,
    /// Entering the condition's terrain or feature costs half.
    DoubleMove,
    /// Cannot enter the condition's terrain or feature.
    Impassable,
    /// Choosing the promotion heals 50 HP instead of being kept (`InstaHeal`).
    InstaHeal,
    /// No penalty for attacking from water or across a river (`Amphib`).
    Amphibious,
    /// No penalty for attacking across a river.
    RiverCrossing,
    /// Ranged attacks ignore line of sight.
    IndirectFire,
    /// Heals outside friendly territory at sea (`HealOutsideFriendly`).
    HealOutsideFriendly,
    /// Air sweeps.
    AirSweep,
    /// Enters rival territory without open borders (`RivalTerritory`).
    RivalTerritory,
    /// Entering rough terrain ends the move (`RoughTerrainEndsTurn`).
    RoughTerrainEndsTurn,
    /// Every step costs one point (`IgnoreTerrainCost`).
    IgnoreTerrainCost,
    /// Every step costs exactly one point, roads included (`FlatMovementCost`).
    FlatMovementCost,
    /// Moves over land and water.
    Hovering,
    /// Never captures cities or units (`NoCapture`).
    NoCapture,
    /// Cannot melee attack (`OnlyDefensive`).
    OnlyDefensive,
    /// No terrain defense bonus, no fortification (`NoDefensiveBonus`).
    NoDefensiveBonus,
    /// Must set up before a ranged attack.
    MustSetUp,
    /// Pillaging costs no moves (`FreePillageMoves`).
    FreePillageMoves,
    /// Only attacks cities (`CityAttackOnly`).
    CityAttackOnly,
    /// Captures a defeated unit of its own kind (`CaptureDefeatedEnemy`).
    CaptureDefeatedEnemy,
    /// ×1.5 attack against a unit that cannot withdraw (`HeavyCharge`).
    HeavyCharge,
    /// +50% city attack for own units next to a city it is next to (`Sapper`).
    Sapper,
    /// Ignores zones of control.
    IgnoreZoneOfControl,
    /// Stepping from water onto land costs one move point instead of all
    /// of them (a landing that does not end the move).
    EmbarkedToLandFlatCost,
    /// Seen by other players only next to their units and cities or by
    /// their units that see invisible units (submarines).
    Invisible,
    /// Sees invisible units within its sight.
    SeeInvisible,
    /// Entering hills costs one point, whatever the terrain and the feature.
    HillsIgnoreTerrainCost,
    /// Enters mountains, at the price of a mountain.
    CrossMountains,
    /// The condition's tile counts as a road for the unit's own moves while
    /// it lies in the unit's own territory.
    RoadInFriendlyLand,
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleUnitAbility<M>),
}

impl<M: RulesModule> UnitAbility<M> {
    pub fn tag(self) -> u8 {
        match self {
            UnitAbility::MoveAfterAttacking => 0,
            UnitAbility::AlwaysHeal => 1,
            UnitAbility::Embarkation => 2,
            UnitAbility::EmbarkedAllWater => 3,
            UnitAbility::MoveImpassable => 4,
            UnitAbility::RangedSupportFire => 5,
            UnitAbility::HillsDoubleMove => 9,
            UnitAbility::DoubleMove => 10,
            UnitAbility::Impassable => 11,
            UnitAbility::InstaHeal => 12,
            UnitAbility::Amphibious => 13,
            UnitAbility::RiverCrossing => 14,
            UnitAbility::IndirectFire => 15,
            UnitAbility::HealOutsideFriendly => 16,
            UnitAbility::AirSweep => 17,
            UnitAbility::RivalTerritory => 18,
            UnitAbility::RoughTerrainEndsTurn => 19,
            UnitAbility::IgnoreTerrainCost => 20,
            UnitAbility::FlatMovementCost => 21,
            UnitAbility::Hovering => 22,
            UnitAbility::NoCapture => 23,
            UnitAbility::OnlyDefensive => 24,
            UnitAbility::NoDefensiveBonus => 25,
            UnitAbility::MustSetUp => 26,
            UnitAbility::FreePillageMoves => 27,
            UnitAbility::CityAttackOnly => 28,
            UnitAbility::CaptureDefeatedEnemy => 29,
            UnitAbility::HeavyCharge => 30,
            UnitAbility::Sapper => 31,
            UnitAbility::IgnoreZoneOfControl => 32,
            UnitAbility::EmbarkedToLandFlatCost => 33,
            UnitAbility::Invisible => 34,
            UnitAbility::SeeInvisible => 35,
            UnitAbility::HillsIgnoreTerrainCost => 38,
            UnitAbility::CrossMountains => 39,
            UnitAbility::RoadInFriendlyLand => 40,
            UnitAbility::Module(ability) => ability.tag(),
        }
    }
}

hash_by_tag!(UnitAbility);

/// What a per-count modifier counts on its target (`PerCount`). Each count
/// names the scopes it is defined for; `crate::effects::count` evaluates it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum Count<M: RulesModule> {
    /// City: its citizens.
    Population,
    /// City: the world wonders standing in it.
    WorldWonders,
    /// Player: its cities connected to its capital, the capital not
    /// counted (Meritocracy).
    ConnectedCities,
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleCount<M>),
}

impl<M: RulesModule> Count<M> {
    /// The scopes the count is defined for.
    pub fn fits(self, scope: Scope<M>) -> bool {
        match self {
            Count::Population | Count::WorldWonders => scope == Scope::City,
            Count::ConnectedCities => scope == Scope::Player,
            Count::Module(count) => count.fits(scope),
        }
    }

    pub fn tag(self) -> u8 {
        match self {
            Count::Population => 0,
            Count::WorldWonders => 2,
            Count::ConnectedCities => 3,
            Count::Module(count) => count.tag(),
        }
    }
}

hash_by_tag!(Count);

/// A count and its step: the modifier's amount applies once per `every`
/// counted things (rounded down), up to `max` in the modifier's own unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub struct PerCount<M: RulesModule> {
    pub count: Count<M>,
    /// At least 1 (the compiler checks).
    pub every: u16,
    /// Cap of the resulting amount (None: uncapped).
    pub max: Option<i32>,
}

impl<M: RulesModule> PerCount<M> {
    /// `amount` × ⌊counted / every⌋, capped at `max` (by magnitude).
    pub fn scale(self, amount: i32, counted: u32) -> i32 {
        let times = (counted / u32::from(self.every.max(1))).min(i32::MAX as u32) as i32;
        let value = amount.saturating_mul(times);
        match self.max {
            Some(max) if value.abs() > max.abs() => max,
            _ => value,
        }
    }
}

/// What kind of plot a Tile-scope effect reaches (river, lake and sea
/// plots, sea resources, natural-wonder yield of a belief; rules.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize))]
pub enum PlotFilter {
    /// A land tile on a river.
    River,
    /// Water that is not a lake (coast and ocean).
    Sea,
    /// A lake tile.
    Lake,
    /// A water tile (sea or lake) with a resource its owner sees.
    SeaResource,
    /// A natural wonder tile.
    NaturalWonder,
}

/// Which items a production percent is for (classic
/// `getProductionModifier(item)`, classic/docs/classic/rules.md): the percent is
/// part of the city's production rate while the head of its queue matches.
/// `effects::production` matches an item against a filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductionFilter {
    /// Every unit.
    Units,
    /// Units of a domain (Forge,
    /// Seaport).
    UnitDomain(crate::rules::content::Domain),
    /// Units of a combat role (`*_UnitCombatProductionModifiers`: Stable,
    /// Warrior Code).
    UnitRole(crate::rules::content::UnitRole),
    /// Units of the class of this unit; the unique replacements of the
    /// class match too (Collective Rule: settlers).
    UnitClassOf(crate::ids::UnitKindId),
    /// Units of a military role.
    MilitaryUnits,
    /// Spaceship parts.
    SpaceshipParts,
    /// Every building, wonders included
    /// (Windmill, Republic). Unverified: whether wonders should be excluded.
    Buildings,
    /// World wonders; with `before_era`, only
    /// wonders of an earlier era (Monument to the Gods, Marble).
    WorldWonders {
        before_era: Option<crate::rules::content::EraId>,
    },
    /// Buildings of the class of this building, unique replacements
    /// included.
    BuildingClassOf(BuildingKindId),
    /// Buildings the owner's capital already has (Rome).
    BuiltInCapital,
}

/// Gate checked at evaluation time (references resolved by the compiler).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition<M: RulesModule> {
    Always,
    /// The owner knows the technology.
    HasTechnology(TechId),
    /// The owner does not know the technology yet.
    LacksTechnology(TechId),
    /// The target city (or the working city of a tile) has at least this
    /// many citizens.
    MinPopulation(u32),
    /// The target city (City scope) or the working city (Tile scope) has it.
    HasBuilding(BuildingKindId),
    /// Tile scope: the tile has this improvement.
    TileImprovement(ImprovementId),
    /// Tile scope: the tile has this resource.
    TileResource(ResourceId),
    /// Tile scope: the tile has this feature. Unit scope: the battle tile
    /// (combat) or the entered tile (movement) has it.
    TileFeature(FeatureId),
    /// Tile scope: the tile's terrain is this terrain type. Unit scope: the
    /// battle or entered tile is of this terrain (the hills terrain matches
    /// hills).
    TileTerrain(crate::rules::content::TerrainKindId),
    /// Unit scope: the tile is of this terrain and the owner does not know
    /// the technology yet (Classic trireme: ocean impassable until Astronomy).
    TileTerrainUntil(crate::rules::content::TerrainKindId, TechId),
    /// Unit scope, combat: the opponent is a unit of this combat role.
    OpponentRole(crate::rules::content::UnitRole),
    /// Unit scope, combat: the opponent is a unit of this domain.
    OpponentDomain(crate::rules::content::Domain),
    /// Unit scope, combat: the opponent is of the class of this unit
    /// (unique replacements of the class match too).
    OpponentClassOf(UnitKindId),
    /// Unit scope: the target unit is of this domain (kind-level: checked
    /// when the unit's kind is known; Classic filters of building XP).
    UnitDomain(crate::rules::content::Domain),
    /// Unit scope: the target unit is of this combat role (kind-level).
    UnitRole(crate::rules::content::UnitRole),
    /// OnUnitKilled effects of a building or a belief: the kill happened
    /// within this many tiles of the source's city (the building's city;
    /// for a belief, the killer's city nearest to the kill whose majority
    /// holds the belief — Classic God of War).
    EventNearSourceCity(u8),
    /// Tile scope: the tile is of this kind of plot (river, sea, lake, ...).
    TilePlot(PlotFilter),
    /// Tile scope: the tile has a resource of this class that its owner
    /// sees (Russia: `Trait_YieldChangesStrategicResources`).
    TileResourceClass(crate::rules::content::ResourceClass),
    /// Tile scope: the tile has this feature and no improvement (Japan).
    TileFeatureUnimproved(FeatureId),
    /// City scope: at least `at_least` tiles next to the city centre have
    /// `feature` and no improvement (Celts).
    CityAdjacentUnimprovedFeature { feature: FeatureId, at_least: u8 },
    /// City scope, production percents only: the head of the city's queue
    /// matches the filter.
    Producing(ProductionFilter),
    /// City scope: a land combat unit of the owner stands in the city
    /// (`combat::city::garrison`).
    CityGarrisoned,
    /// City scope: the city is next to a sea, not only a lake (Classic
    /// `world::map::coastal_land`).
    CityCoastal,
    /// OnTechnologyUnlocked only: the learned technology is this one (Babylon at Writing).
    LearnedTechnology(TechId),
    /// City or Tile scope: the city's tile touches a river (Classic
    /// Sacred Waters).
    CityOnRiver,
    /// The owner is at war with no major nation (Swords into Plowshares; wars with city-states
    /// and barbarians do not count). Player, City or Tile scope.
    AtPeace,
    /// City scope: the city is the capital or was connected to it at the
    /// start of its owner's turn (`CityCore::capital_connection`; Messenger
    /// of the Gods). Unverified: the capital itself counts as connected.
    ConnectedToCapital,
    /// Unit scope, combat: an improvement of this kind of the unit's owner
    /// stands within `range` tiles of the unit's own tile (a combat bonus
    /// near a particular improvement).
    NearOwnImprovement { improvement: ImprovementId, range: u8 },
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleCondition<M>),
}

impl<M: RulesModule> Condition<M> {
    /// Conditions on the opponent: in a battle (Unit scope, passive) or on
    /// the victim of an OnUnitKilled event (any scope).
    pub fn is_opponent(self) -> bool {
        match self {
            Condition::OpponentRole(_)
            | Condition::OpponentDomain(_)
            | Condition::OpponentClassOf(_) => true,
            Condition::Module(condition) => condition.is_opponent(),
            _ => false,
        }
    }

    /// Conditions that hold only in a situation (a battle tile, an
    /// opponent, an entered tile), not for the owner as a whole: the system
    /// of the situation evaluates them (`crate::combat`, navigation).
    pub fn is_situational(self) -> bool {
        match self {
            Condition::TileFeature(_)
            | Condition::NearOwnImprovement { .. }
            | Condition::TileTerrain(_)
            | Condition::TileTerrainUntil(..)
            | Condition::OpponentRole(_)
            | Condition::OpponentDomain(_)
            | Condition::OpponentClassOf(_) => true,
            Condition::Module(condition) => condition.is_situational(),
            _ => false,
        }
    }

    /// Conditions on the target unit's kind (domain, role): evaluated when
    /// the unit is known, never for the owner as a whole.
    pub(crate) fn is_kind_level(self) -> bool {
        matches!(self, Condition::UnitDomain(_) | Condition::UnitRole(_))
    }

    pub fn tag(self) -> u8 {
        match self {
            Condition::Always => 0,
            Condition::HasTechnology(_) => 1,
            Condition::LacksTechnology(_) => 2,
            Condition::MinPopulation(_) => 3,
            Condition::HasBuilding(_) => 4,
            Condition::TileImprovement(_) => 5,
            Condition::TileResource(_) => 6,
            Condition::TileFeature(_) => 7,
            Condition::TileTerrain(_) => 8,
            Condition::TileTerrainUntil(..) => 9,
            Condition::OpponentRole(_) => 10,
            Condition::OpponentDomain(_) => 11,
            Condition::OpponentClassOf(_) => 12,
            Condition::UnitDomain(_) => 14,
            Condition::UnitRole(_) => 15,
            Condition::EventNearSourceCity(_) => 17,
            Condition::TilePlot(_) => 19,
            Condition::TileResourceClass(_) => 20,
            Condition::TileFeatureUnimproved(_) => 21,
            Condition::CityAdjacentUnimprovedFeature { .. } => 22,
            Condition::Producing(_) => 23,
            Condition::CityGarrisoned => 25,
            Condition::CityCoastal => 26,
            Condition::LearnedTechnology(_) => 27,
            Condition::CityOnRiver => 28,
            Condition::AtPeace => 29,
            Condition::ConnectedToCapital => 30,
            Condition::NearOwnImprovement { .. } => 34,
            Condition::Module(condition) => condition.tag(),
        }
    }
}

impl<M: RulesModule> Hash for Condition<M> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let Condition::Module(condition) = self {
            return condition.hash(state);
        }
        hash_tag(self.tag(), state);
        match self {
            Condition::HasTechnology(t)
            | Condition::LacksTechnology(t)
            | Condition::LearnedTechnology(t) => t.hash(state),
            Condition::MinPopulation(n) => n.hash(state),
            Condition::HasBuilding(b) => b.hash(state),
            Condition::TileImprovement(i) => i.hash(state),
            Condition::TileResource(r) => r.hash(state),
            Condition::TileFeature(f) | Condition::TileFeatureUnimproved(f) => f.hash(state),
            Condition::TileTerrain(terrain) => terrain.hash(state),
            Condition::TileTerrainUntil(terrain, tech) => {
                terrain.hash(state);
                tech.hash(state);
            }
            Condition::OpponentRole(role) | Condition::UnitRole(role) => role.hash(state),
            Condition::OpponentDomain(domain) | Condition::UnitDomain(domain) => domain.hash(state),
            Condition::OpponentClassOf(unit) => unit.hash(state),
            Condition::EventNearSourceCity(tiles) => tiles.hash(state),
            Condition::TilePlot(plot) => plot.hash(state),
            Condition::TileResourceClass(class) => class.hash(state),
            Condition::CityAdjacentUnimprovedFeature { feature, at_least } => {
                feature.hash(state);
                at_least.hash(state);
            }
            Condition::Producing(filter) => filter.hash(state),
            Condition::NearOwnImprovement { improvement, range } => {
                improvement.hash(state);
                range.hash(state);
            }
            Condition::Always
            | Condition::CityGarrisoned
            | Condition::CityCoastal
            | Condition::CityOnRiver
            | Condition::AtPeace
            | Condition::ConnectedToCapital
            | Condition::Module(_) => {}
        }
    }
}

/// What an instant grant gives (`Modifier::Grant`): the one-shot Classic
/// columns of wonders, policies and traits. Choices (technologies, great
/// people) are owed to the player and chosen by command
/// (`ChooseFreeTechnology`, `ChooseFreeGreatPerson`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum Grant<M: RulesModule> {
    /// Player: technologies of the player's choice among those it may
    /// research now.
    FreeTechnologies(u8),
    /// Player, OnCityConquered: a technology of the player's choice that the
    /// city's previous owner knows, once per city (Assyria).
    ConquestTechnology,
    /// City: one unit of each land combat kind the owner has appears in the
    /// city (Terracotta Army).
    MilitaryUnitCopies,
    /// City: citizens added (buildings and policies).
    Population(u8),
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleGrant<M>),
}

impl<M: RulesModule> Grant<M> {
    /// The scope the grant targets.
    pub fn scope(self) -> Scope<M> {
        match self {
            Grant::MilitaryUnitCopies | Grant::Population(_) => Scope::City,
            Grant::FreeTechnologies(_) | Grant::ConquestTechnology => Scope::Player,
            Grant::Module(grant) => grant.scope(),
        }
    }

    pub fn tag(self) -> u8 {
        match self {
            Grant::FreeTechnologies(_) => 0,
            Grant::ConquestTechnology => 3,
            Grant::MilitaryUnitCopies => 5,
            Grant::Population(_) => 6,
            Grant::Module(grant) => grant.tag(),
        }
    }
}

impl<M: RulesModule> Hash for Grant<M> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let Grant::Module(grant) = self {
            return grant.hash(state);
        }
        hash_tag(self.tag(), state);
        if let Grant::FreeTechnologies(n) | Grant::Population(n) = self {
            n.hash(state);
        }
    }
}

/// What a player may not do while the source exists (`Modifier::Forbid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Forbidden {
    /// Train or buy units of the class of this unit (Venice: settlers).
    UnitClassOf(UnitKindId),
    /// Annex a captured city (Venice): it becomes a puppet.
    Annexing,
}

/// Typed modifier set (Level 2). Extending it means one variant here, one
/// compiler rule and one consumer — never a string kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier<M: RulesModule> {
    /// Flat stage.
    AddYield {
        yield_kind: YieldKind<M>,
        amount: i32,
    },
    /// Additive percentage stage (all percents of one target are summed).
    AddYieldPercent {
        yield_kind: YieldKind<M>,
        percent: i32,
    },
    /// Multiplicative stage (percent of the value: 150 = ×1.5). Unique only.
    MultiplyYield {
        yield_kind: YieldKind<M>,
        percent: i32,
    },
    /// Additive percentage of the city food surplus.
    ModifyGrowth {
        percent: i32,
    },
    /// Additive percentage of a cost (negative = cheaper).
    ModifyCost {
        target: CostTarget<M>,
        percent: i32,
    },
    /// Flat yield per citizen, in hundredths (library: 50 = +1 per 2 pop). Truncated once per evaluation.
    AddYieldPerPopulation {
        yield_kind: YieldKind<M>,
        hundredths: i32,
    },
    /// Additive percentage of unit combat strength in a context.
    ModifyCombat {
        context: CombatContext,
        percent: i32,
    },
    /// Flat movement points.
    ModifyMovement {
        amount: i16,
    },
    /// The player may train the unit without its required technology.
    UnlockUnit(UnitKindId),
    /// The player may build the building without its required technology.
    UnlockBuilding(BuildingKindId),
    /// Instant: `count` units appear in the target city.
    FreeUnit {
        unit: UnitKindId,
        count: u8,
    },
    /// Instant: the building appears in the target city.
    FreeBuilding(BuildingKindId),
    /// Instant: the target unit gains the promotion.
    AddPromotion(PromotionId),
    /// A numeric stat of the economy and society systems
    /// (`rules::stats`): great people rates, food kept, specialist yields.
    AddStat {
        stat: crate::rules::stats::Stat<M>,
        amount: i32,
    },
    /// Flat change of a unit property (Unit scope).
    ModifyUnitStat {
        stat: UnitStat<M>,
        amount: i32,
    },
    /// A boolean unit ability (Unit scope).
    GrantAbility(UnitAbility<M>),
    /// After its first combat the unit gains one of its candidate
    /// promotions at random (Mystic Blade).
    PostCombatPromotion(PromotionId),
    /// While the source exists, the target units have the promotion's
    /// effects (units of a combat role the promotion allows; Classic free
    /// promotions of policies and wonders).
    PassivePromotion(PromotionId),
    /// Flat change of a city's combat property (City scope).
    ModifyCityCombat {
        stat: CityCombatStat,
        amount: i32,
    },
    /// Instant: the target unit gains experience (`unit.free_xp`).
    GrantExperience(u16),
    /// Instant: the building appears for free in the first `count` cities
    /// of the target player that lack it; what is left is given to the
    /// next cities founded (Aristocracy-style free buildings).
    FreeBuildingInCities { building: BuildingKindId, count: u8 },
    /// Flat yield per counted thing of the target (Divine Inspiration: +2
    /// faith per world wonder). Becomes `AddYield` when evaluated.
    AddYieldPerCount {
        yield_kind: YieldKind<M>,
        amount: i32,
        per: PerCount<M>,
    },
    /// Additive yield percent per counted thing (Religious Community: +1 %
    /// production per follower, at most 15). Becomes `AddYieldPercent`.
    AddYieldPercentPerCount {
        yield_kind: YieldKind<M>,
        percent: i32,
        per: PerCount<M>,
    },
    /// Instant, OnUnitKilled, Player scope: the player gains `percent` % of
    /// the victim's strength (the greater of melee and ranged) as a yield
    /// with a player stock (classic/docs/classic/rules.md).
    YieldFromKill { yield_kind: YieldKind<M>, percent: i32 },
    /// Instant: what `Grant` names (Player or City scope by the grant).
    Grant(Grant<M>),
    /// While the source exists the player may not do this (Player scope).
    Forbid(Forbidden),
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleModifier<M>),
}

impl<M: RulesModule> Modifier<M> {
    /// Instant modifiers change the world once; the rest are evaluated.
    pub fn is_instant(&self) -> bool {
        match self {
            Modifier::YieldFromKill { .. }
            | Modifier::FreeUnit { .. }
            | Modifier::FreeBuilding(_)
            | Modifier::AddPromotion(_)
            | Modifier::GrantExperience(_)
            | Modifier::FreeBuildingInCities { .. }
            | Modifier::Grant(_) => true,
            Modifier::Module(modifier) => modifier.is_instant(),
            _ => false,
        }
    }

    pub fn tag(&self) -> u8 {
        match self {
            Modifier::AddYield { .. } => 0,
            Modifier::AddYieldPercent { .. } => 1,
            Modifier::MultiplyYield { .. } => 2,
            Modifier::ModifyGrowth { .. } => 3,
            Modifier::ModifyCost { .. } => 4,
            Modifier::AddYieldPerPopulation { .. } => 5,
            Modifier::ModifyCombat { .. } => 6,
            Modifier::ModifyMovement { .. } => 7,
            Modifier::UnlockUnit(_) => 10,
            Modifier::UnlockBuilding(_) => 11,
            Modifier::FreeUnit { .. } => 12,
            Modifier::FreeBuilding(_) => 13,
            Modifier::AddPromotion(_) => 14,
            Modifier::AddStat { .. } => 15,
            Modifier::ModifyUnitStat { .. } => 16,
            Modifier::GrantAbility(_) => 17,
            Modifier::PostCombatPromotion(_) => 18,
            Modifier::PassivePromotion(_) => 19,
            Modifier::ModifyCityCombat { .. } => 20,
            Modifier::GrantExperience(_) => 21,
            Modifier::FreeBuildingInCities { .. } => 24,
            Modifier::AddYieldPerCount { .. } => 34,
            Modifier::AddYieldPercentPerCount { .. } => 35,
            Modifier::YieldFromKill { .. } => 37,
            Modifier::Grant(_) => 38,
            Modifier::Forbid(_) => 39,
            Modifier::Module(modifier) => modifier.tag(),
        }
    }
}

impl<M: RulesModule> Hash for Modifier<M> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let Modifier::Module(modifier) = self {
            return modifier.hash(state);
        }
        hash_tag(self.tag(), state);
        match *self {
            Modifier::AddYield { yield_kind, amount } => (yield_kind, amount).hash(state),
            Modifier::AddYieldPercent { yield_kind, percent }
            | Modifier::MultiplyYield { yield_kind, percent }
            | Modifier::YieldFromKill { yield_kind, percent } => (yield_kind, percent).hash(state),
            Modifier::ModifyGrowth { percent } => percent.hash(state),
            Modifier::ModifyCost { target, percent } => (target, percent).hash(state),
            Modifier::AddYieldPerPopulation { yield_kind, hundredths } => {
                (yield_kind, hundredths).hash(state)
            }
            Modifier::ModifyCombat { context, percent } => (context, percent).hash(state),
            Modifier::ModifyMovement { amount } => amount.hash(state),
            Modifier::UnlockUnit(unit) => unit.hash(state),
            Modifier::UnlockBuilding(building) | Modifier::FreeBuilding(building) => {
                building.hash(state)
            }
            Modifier::FreeUnit { unit, count } => (unit, count).hash(state),
            Modifier::AddPromotion(promotion)
            | Modifier::PostCombatPromotion(promotion)
            | Modifier::PassivePromotion(promotion) => promotion.hash(state),
            Modifier::AddStat { stat, amount } => (stat, amount).hash(state),
            Modifier::ModifyUnitStat { stat, amount } => (stat, amount).hash(state),
            Modifier::GrantAbility(ability) => ability.hash(state),
            Modifier::ModifyCityCombat { stat, amount } => (stat, amount).hash(state),
            Modifier::GrantExperience(amount) => amount.hash(state),
            Modifier::FreeBuildingInCities { building, count } => (building, count).hash(state),
            Modifier::AddYieldPerCount { yield_kind, amount, per } => {
                (yield_kind, amount, per).hash(state)
            }
            Modifier::AddYieldPercentPerCount { yield_kind, percent, per } => {
                (yield_kind, percent, per).hash(state)
            }
            Modifier::Grant(grant) => grant.hash(state),
            Modifier::Forbid(what) => what.hash(state),
            Modifier::Module(_) => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CompiledEffect<M: RulesModule> {
    pub scope: Scope<M>,
    pub selector: Selector<M>,
    pub condition: Condition<M>,
    /// A second gate that must hold too (Always for most effects): a
    /// belief on a building of a following city with a number of
    /// followers (Asceticism). The compiler keeps it off Unit scope and off
    /// situational conditions, so owner-level evaluation alone reads it.
    pub second_condition: Condition<M>,
    pub trigger: Trigger<M>,
    pub modifier: Modifier<M>,
    pub duration: Duration,
    pub stacking: Stacking,
}

/// Effects of one source, bucketed by the compiler so that runtime lookups
/// touch only what they need: passive effects by target scope, triggered
/// effects by trigger. Declaration order is kept inside every bucket.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectSet<M: RulesModule> {
    pub passive_player: Vec<CompiledEffect<M>>,
    /// Passive effects of the module's scopes (`Scope::Module`).
    pub passive_module: Vec<CompiledEffect<M>>,
    pub passive_city: Vec<CompiledEffect<M>>,
    pub passive_unit: Vec<CompiledEffect<M>>,
    pub passive_tile: Vec<CompiledEffect<M>>,
    /// Index = `Trigger::index()` (`Trigger::slots` long); slot
    /// 0 (Passive) stays empty.
    pub triggered: Vec<Vec<CompiledEffect<M>>>,
}

impl<M: RulesModule> Default for EffectSet<M> {
    fn default() -> Self {
        Self {
            passive_player: Vec::new(),
            passive_module: Vec::new(),
            passive_city: Vec::new(),
            passive_unit: Vec::new(),
            passive_tile: Vec::new(),
            triggered: vec![Vec::new(); Trigger::<M>::slots()],
        }
    }
}

impl<M: RulesModule> EffectSet<M> {
    pub fn triggered(&self, trigger: Trigger<M>) -> &[CompiledEffect<M>] {
        &self.triggered[trigger.index()]
    }

    pub fn is_empty(&self) -> bool {
        self.passive_player.is_empty()
            && self.passive_module.is_empty()
            && self.passive_city.is_empty()
            && self.passive_unit.is_empty()
            && self.passive_tile.is_empty()
            && self.triggered.iter().all(|t| t.is_empty())
    }

    /// Any passive effect reaching beyond the source (owner-wide selector).
    pub(crate) fn has_owner_wide_passive(&self) -> bool {
        let buckets = [
            &self.passive_player,
            &self.passive_city,
            &self.passive_unit,
            &self.passive_tile,
        ];
        buckets
            .iter()
            .any(|b| b.iter().any(|e| e.selector.is_owner_wide()))
    }
}

/// Content source of an effect (persisted for timed effects).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRef<M: RulesModule> {
    Nation(u8),
    Technology(TechId),
    Building(BuildingKindId),
    Unit(UnitKindId),
    Promotion(PromotionId),
    /// A word of the rules module (`rules::vocabulary`).
    Module(ModuleSource<M>),
}

impl<M: RulesModule> SourceRef<M> {
    pub fn tag(self) -> u8 {
        match self {
            SourceRef::Nation(_) => 0,
            SourceRef::Technology(_) => 3,
            SourceRef::Building(_) => 4,
            SourceRef::Unit(_) => 5,
            SourceRef::Promotion(_) => 6,
            SourceRef::Module(source) => source.tag(),
        }
    }
}

impl<M: RulesModule> Hash for SourceRef<M> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let SourceRef::Module(source) = self {
            return source.hash(state);
        }
        hash_tag(self.tag(), state);
        match self {
            SourceRef::Nation(nation) => nation.hash(state),
            SourceRef::Technology(tech) => tech.hash(state),
            SourceRef::Building(building) => building.hash(state),
            SourceRef::Unit(unit) => unit.hash(state),
            SourceRef::Promotion(promotion) => promotion.hash(state),
            SourceRef::Module(_) => {}
        }
    }
}

/// Sources that have at least one effect of a trigger (compile-time index:
/// firing an event visits only these, never every source of the ruleset).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TriggerSources<M: RulesModule> {
    pub nations: Vec<u8>,
    pub technologies: Vec<TechId>,
    pub buildings: Vec<BuildingKindId>,
    pub units: Vec<UnitKindId>,
    pub promotions: Vec<PromotionId>,
    /// Rows of the module's tables, in the module's order.
    pub module: Vec<ModuleSource<M>>,
}

impl<M: RulesModule> TriggerSources<M> {
    pub fn is_empty(&self) -> bool {
        self.nations.is_empty()
            && self.technologies.is_empty()
            && self.buildings.is_empty()
            && self.units.is_empty()
            && self.promotions.is_empty()
            && self.module.is_empty()
    }
}

/// Runtime indexes over all effects, built once by the compiler.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectIndex<M: RulesModule> {
    /// Index = `Trigger::index()` (`Trigger::slots` long); slot
    /// 0 (Passive) stays empty.
    pub by_trigger: Vec<TriggerSources<M>>,
    /// Buildings whose passive effects reach other cities/units of the owner:
    /// evaluation of one target scans only these, not every building.
    pub owner_wide_buildings: Vec<BuildingKindId>,
    /// Index = `BuildingKindId`: position of the building in
    /// `owner_wide_buildings`, `u16::MAX` when it is not owner-wide (one
    /// lookup per city building when counting them).
    pub owner_wide_slot: Vec<u16>,
    /// Any source has passive City-scope effects (else a city evaluation
    /// only looks at timed effects).
    pub has_passive_city: bool,
    /// Any source has passive Tile-scope effects (else worked tiles are not
    /// matched against effects at all).
    pub has_passive_tile: bool,
    /// Any source has passive or timed Unit-scope effects (else unit strength and
    /// movement are the content values).
    pub has_passive_unit: bool,
    /// Any source has passive Player-scope effects.
    pub has_passive_player: bool,
    /// Buildings with `BuildingAbility::BorderObstacle` (the Great Wall):
    /// movement profiles look only for these.
    pub border_obstacle_buildings: Vec<BuildingKindId>,
    /// Any passive effect depends on a city's garrison (`CityGarrisoned`):
    /// a combat unit entering or leaving a city refreshes its yields.
    pub has_garrison_condition: bool,
    /// Any effect has a `Producing` condition, or a resource speeds wonders:
    /// a city's production rate depends on the head of its queue (queue
    /// changes refresh its yields).
    pub has_production_percents: bool,
    /// Any effect has `Producing(BuiltInCapital)`: a building added to a
    /// capital refreshes the owner's cities.
    pub has_capital_production: bool,
    /// Resources with a wonder production percent (Marble), ascending.
    pub wonder_resources: Vec<crate::ids::ResourceId>,
    /// Some effect asks whether a city is connected to its capital: the
    /// turn keeps `CityCore::capital_connection` up to date only then.
    pub uses_capital_connection: bool,
    /// Some effect is gated by peace (`Condition::AtPeace`): a war or a
    /// peace refreshes the yields of both players.
    pub uses_at_peace: bool,
    /// The module's indexes over its words (`Vocabulary::index`).
    pub module: ModuleIndex<M>,
}

impl<M: RulesModule> Default for EffectIndex<M> {
    fn default() -> Self {
        Self {
            by_trigger: vec![TriggerSources::default(); Trigger::<M>::slots()],
            owner_wide_buildings: Vec::new(),
            owner_wide_slot: Vec::new(),
            has_passive_city: false,
            has_passive_tile: false,
            has_passive_unit: false,
            has_passive_player: false,
            border_obstacle_buildings: Vec::new(),
            has_garrison_condition: false,
            has_production_percents: false,
            has_capital_production: false,
            wonder_resources: Vec::new(),
            uses_capital_connection: false,
            uses_at_peace: false,
            module: Default::default(),
        }
    }
}

impl<M: RulesModule> EffectIndex<M> {
    pub fn sources(&self, trigger: Trigger<M>) -> &TriggerSources<M> {
        &self.by_trigger[trigger.index()]
    }
}
