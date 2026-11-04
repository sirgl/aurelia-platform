//! Human-readable content model (before compilation).
//!
//! Every table has the fields of the first (v1) content plus the Classic
//! fields (`rules::content`). The Classic fields default to "not declared", so
//! the frozen v1 content compiles unchanged.

use crate::module::RulesModule;
use crate::rules::content::{
    BuildingKind, Domain, FreshWater, ImprovementKind, ImprovementPlacement, ResourceClass,
    SourceFeatureRemoval, SourceInertEffect, StartRegions, UnitRole, Yields,
};
use crate::rules::rows::{
    ModuleHandicap, ModuleSourceBuilding, ModuleSourceNation, ModuleSourceEra,
    ModuleSourceImprovement, ModuleSourceNaturalWonder, ModuleSourceResource,
    ModuleSourceSpecialist, ModuleSourceTechnology, ModuleSourceUnit,
};
use crate::rules::effects::{CombatContext, CostTarget, Duration, PerCount, Scope, Selector, Stacking, Trigger, YieldKind};
use crate::rules::tables::{ModuleSourceTables, ModuleTables};
use crate::rules::vocabulary::{ModuleSourceCondition, ModuleSourceModifier};

/// Stable content key: `core:unit/warrior`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(transparent))]
pub struct StableContentKey(pub String);

impl StableContentKey {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// A key with a count (required resources, free units, ...).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyCount {
    pub key: StableContentKey,
    pub count: u16,
}

#[derive(Debug, Clone, Default)]
pub struct SourceUnit<M: RulesModule> {
    pub id: Option<StableContentKey>,
    pub movement: u16,
    /// Combat strength (0 = the compiler default, see DEFAULT_STRENGTH; for
    /// Classic civilian roles 0 stays 0).
    pub strength: u16,
    /// Can found cities (settler). Only these go to FoundCity.
    pub founder: bool,
    /// Worker (builder): only these go to BuildImprovement.
    pub builder: bool,
    /// Cost in production points (must be >0: the compiler rejects 0),
    /// except `unbuildable`.
    pub cost: u32,
    pub effects: Vec<SourceEffect<M>>,
    /// Unlocked by a technology (TechId = index in the second pass);
    /// None: the unit is available from turn one (settler/warrior/worker).
    pub required_tech: Option<StableContentKey>,
    // ---- Classic ----
    pub name: String,
    /// `core:unit-class/<slug>` (None in the v1 content).
    pub class: Option<StableContentKey>,
    pub role: Option<UnitRole>,
    pub domain: Domain,
    pub ranged_strength: u16,
    pub range: u8,
    /// Sight in tiles (0 = not declared).
    pub sight: u8,
    /// Cannot be built with production (great people, faith-only units):
    /// cost must be 0.
    pub unbuildable: bool,
    pub obsolete_tech: Option<StableContentKey>,
    pub required_resources: Vec<KeyCount>,
    pub upgrades_to: Vec<StableContentKey>,
    pub free_promotions: Vec<StableContentKey>,
    pub era: Option<StableContentKey>,
    /// Unique unit of this nation.
    pub nation: Option<StableContentKey>,
    /// The regular unit this unique unit replaces (same class).
    pub replaces: Option<StableContentKey>,
    /// Buildings the training city must have.
    pub requires_buildings: Vec<StableContentKey>,
    /// Combat, air and nuclear abilities.
    pub combat: crate::rules::content::SourceUnitCombat,
    /// Needs this project before it can be built (spaceship parts: Apollo).
    pub required_project: Option<StableContentKey>,
    /// Installed in the capital, it completes this project (spaceship).
    pub spaceship_project: Option<StableContentKey>,
    pub inert: Vec<SourceInertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleSourceUnit<M>,
}

/// A building with yield effects. Unlocked by a technology (required_tech).
#[derive(Debug, Clone, Default)]
pub struct SourceBuilding<M: RulesModule> {
    pub id: Option<StableContentKey>,
    /// Cost in production points (must be >0, except `unbuildable`).
    pub cost: u32,
    pub effects: Vec<SourceEffect<M>>,
    pub required_tech: Option<StableContentKey>,
    /// Wonder: globally unique; the validator rejects an order
    /// if any city of any player has already built it; in a race
    /// of completions the first wins (the second order is silently consumed).
    pub wonder: bool,
    // ---- Classic ----
    pub name: String,
    pub kind: BuildingKind,
    /// `core:building-class/<slug>` (None in the v1 content).
    pub class: Option<StableContentKey>,
    /// Cannot be built with production (palace, faith-only buildings).
    pub unbuildable: bool,
    /// Gold per turn.
    pub maintenance: u32,
    pub obsolete_tech: Option<StableContentKey>,
    pub era: Option<StableContentKey>,
    /// Percent added to the gold price of hurrying it (-1 = cannot hurry).
    pub hurry_cost_modifier: i32,
    pub conquest_survival_pct: u8,
    pub never_capture: bool,
    /// Specialist slots: (`core:specialist/<slug>`, count).
    pub specialist_slots: Option<KeyCount>,
    pub consumes_resources: Vec<KeyCount>,
    /// The city must have these buildings.
    pub requires_buildings: Vec<StableContentKey>,
    /// Every city of the empire must have these (national wonders).
    pub requires_buildings_in_empire: Vec<StableContentKey>,
    /// National wonders: percent added to the cost per city.
    pub cost_per_city_pct: i32,
    pub placement: crate::rules::content::SourceBuildingPlacement,
    pub max_start_era: Option<StableContentKey>,
    pub free_start_era: Option<StableContentKey>,
    pub nation: Option<StableContentKey>,
    pub replaces: Option<StableContentKey>,
    pub inert: Vec<SourceInertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleSourceBuilding<M>,
}

/// Technology: unlocks buildings (unlocks), gives era points later.
#[derive(Debug, Clone, Default)]
pub struct SourceTechnology<M: RulesModule> {
    pub id: Option<StableContentKey>,
    /// Era name of the content ("Ancient"): metadata for visualising
    /// the tree. The game era (DAG depth) is computed by the compiler into `era`.
    pub era_name: Option<String>,
    pub unlocks_buildings: Vec<StableContentKey>,
    /// Cost in beakers (must be >0).
    pub cost: u32,
    /// Prerequisites (keys); a cycle or an unknown key is rejected by the compiler.
    pub requires: Vec<StableContentKey>,
    pub effects: Vec<SourceEffect<M>>,
    // ---- Classic ----
    pub name: String,
    /// `core:era/<slug>`.
    pub era: Option<StableContentKey>,
    /// Column and row in the tech tree.
    pub grid: (u8, u8),
    /// Mirrors of the `required_tech` of these tables (the compiler checks
    /// both directions, like `unlocks_buildings`).
    pub unlocks_units: Vec<StableContentKey>,
    pub unlocks_improvements: Vec<StableContentKey>,
    pub unlocks_projects: Vec<StableContentKey>,
    /// Worker actions and routes (`core:build/…`, `core:route/…`).
    pub unlocks_actions: Vec<StableContentKey>,
    pub repeatable: bool,
    /// Land units may embark (Optics).
    pub allows_embarking: bool,
    /// Embarked units may enter deep water (Astronomy).
    pub embarked_all_water: bool,
    /// Players may receive embassies (Writing).
    pub allows_embassy: bool,
    /// Open borders may be traded (Civil Service).
    pub allows_open_borders: bool,
    /// Defensive pacts may be traded (Chivalry).
    pub allows_defensive_pact: bool,
    /// Research agreements may be traded (Education).
    pub allows_research_agreement: bool,
    /// Learning it reveals the whole map (Satellites).
    pub map_visible: bool,
    /// Roads cross rivers (Engineering).
    pub bridge_building: bool,
    /// Change of the move points of a road step (
    /// Machinery −10, in 1/60 of a move).
    pub road_move_change: i16,
    pub inert: Vec<SourceInertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleSourceTechnology<M>,
}

/// Tile improvement (farm/mine): food/production bonuses, built over turns.
/// Unlocked by a technology (required_tech); terrain suitability is
/// a matrix later (v1: any land, water forbidden).
#[derive(Debug, Clone, Default)]
pub struct SourceImprovement<M: RulesModule> {
    pub id: Option<StableContentKey>,
    /// Turns of work on the tile (must be >0). Classic content gives `work_time`.
    pub work_turns: u16,
    /// Tile bonuses. Classic content gives `yields`.
    pub food_bonus: i32,
    pub prod_bonus: i32,
    /// +food next to a river (a farm by the water, as in Nation; a mine gets 0).
    pub river_food: i32,
    pub required_tech: Option<StableContentKey>,
    // ---- Classic ----
    pub name: String,
    pub kind: ImprovementKind,
    /// Workers cannot build it (great person, barbarian, work boat
    /// improvements): no work time.
    pub unbuildable: bool,
    pub yields: Yields,
    /// Worker time in hundredths of worker-turns (0 =
    /// workers cannot build it).
    pub work_time: u32,
    /// `core:build/<slug>`.
    pub build_action: Option<StableContentKey>,
    pub valid_terrains: Vec<StableContentKey>,
    pub valid_features: Vec<StableContentKey>,
    /// Resources this improvement connects.
    pub resources: Vec<StableContentKey>,
    /// Yields a technology adds only with / without fresh water (the
    /// unconditional ones are effects of the technology).
    pub fresh_water_yields: Vec<SourceTechYield<M>>,
    pub defense_pct: i32,
    pub pillage_gold: u32,
    /// Tiles around it the builder claims (citadel: 1).
    pub culture_bomb_radius: u8,
    /// Damage to adjacent enemy units at their turn's end (citadel: 30).
    pub nearby_enemy_damage: u16,
    /// Culture per era since the tile's history (Landmark).
    pub culture_per_era: u16,
    pub nation: Option<StableContentKey>,
    pub placement: ImprovementPlacement,
    /// Added once per adjacent mountain (terrace farm).
    pub adjacent_mountain_yields: Yields,
    /// Building it removes the tile's resource
    /// (archaeological dig).
    pub removes_resource: bool,
    /// Culture per adjacent improvement of the same kind
    /// (Moai).
    pub adjacent_same_culture: u8,
    pub inert: Vec<SourceInertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleSourceImprovement<M>,
}

/// Yield a technology adds to an improvement under a fresh water condition.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceTechYield<M: RulesModule> {
    pub tech: StableContentKey,
    pub yield_kind: YieldKind<M>,
    pub amount: i32,
    pub fresh_water: FreshWater,
}

/// Tile feature (forest): a permanent bonus while it stands, and clearing by a worker
/// (Nation: a forest gives +1P, chopping removes the feature and gives a one-off production grant).
#[derive(Debug, Clone, Default)]
pub struct SourceFeature {
    pub id: Option<StableContentKey>,
    /// Turns of work on the tile (must be >0: there are no free chops),
    /// except `unremovable`.
    pub chop_turns: u16,
    /// Tile production bonus while the feature stands.
    pub prod_bonus: i32,
    /// One-off production grant to the nearest city when the chop completes.
    pub chop_grant: u32,
    // ---- Classic ----
    pub name: String,
    pub yields: Yields,
    /// The feature replaces the terrain yields instead of adding to them.
    pub yield_not_additive: bool,
    pub movement_cost: u8,
    pub defense_pct: i32,
    pub impassable: bool,
    pub rough: bool,
    pub fresh_water: bool,
    pub no_improvement: bool,
    /// Only on a tile with a river (flood plains).
    pub requires_river: bool,
    /// Only on flat land, not hills or mountains.
    pub requires_flatlands: bool,
    /// How far the feature raises the tile against sight
    /// (forest and jungle 1): it hides the tiles behind it from a lower observer.
    pub see_through: u8,
    pub valid_terrains: Vec<StableContentKey>,
    /// Workers cannot remove it (no removal build): no chop time and no
    /// chop grant.
    pub unremovable: bool,
    /// Classic removal build (time, technology, chop production).
    pub removal: Option<SourceFeatureRemoval>,
    pub inert: Vec<SourceInertEffect>,
}

/// Map resource (wheat/iron/gold): bonuses when developed.
/// Developed = the required improvement stands (and the tech is known, if set).
/// Placed by the generator over terrains with a density (deterministic).
#[derive(Debug, Clone, Default)]
pub struct SourceResource<M: RulesModule> {
    pub id: Option<StableContentKey>,
    pub food_bonus: i32,
    pub prod_bonus: i32,
    /// Gold to the treasury per turn from a worked tile (A1 economy).
    pub gold_bonus: u32,
    /// Which improvement develops it (None = works immediately).
    pub requires_improvement: Option<StableContentKey>,
    pub required_tech: Option<StableContentKey>,
    /// Allowed terrains (empty = any; water excluded: the sea has its own count, `sea_density`).
    pub terrains: Vec<u16>,
    /// Density in percent (0..=100) over allowed tiles.
    pub density: u8,
    /// Percent (0..=100) of the shallow sea (water within `SHALLOW_REACH` of
    /// land) the resource is placed on, in addition to its land `density`;
    /// 0: not at sea. The sea has its own count because a resource at sea
    /// is bounded by the coast, not by the land terrains it lists.
    pub sea_density: u8,
    // ---- Classic ----
    pub name: String,
    pub class: Option<ResourceClass>,
    /// Yields of the tile (Classic content; replaces food/prod/gold bonus).
    pub yields: Yields,
    /// Improvements that connect it (the first one is the usual one).
    pub improvements: Vec<StableContentKey>,
    pub tradeable_with_tech: Option<StableContentKey>,
    /// Terrains it appears on (resolved to map codes for the generator).
    pub valid_terrains: Vec<StableContentKey>,
    pub valid_features: Vec<StableContentKey>,
    pub hills: bool,
    pub flatlands: bool,
    /// False for scenario-only resources (never placed).
    pub in_standard_game: bool,
    pub nation: Option<StableContentKey>,
    /// Strategic quantities per deposit.
    pub quantities: Vec<u8>,
    /// Extra yields with a connecting improvement .
    pub improved_yields: Vec<(StableContentKey, Yields)>,
    /// Wonder production percent in the cities that have it (Marble).
    pub wonder_production: Option<SourceWonderProduction>,
    pub inert: Vec<SourceInertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleSourceResource<M>,
}

/// `WonderProduction` with the era by key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceWonderProduction {
    pub percent: i32,
    pub before_era: Option<StableContentKey>,
}

/// Effect as written in content (see `rules::effects` for the meaning of
/// every field). References are stable keys; the compiler resolves them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEffect<M: RulesModule> {
    pub scope: Scope<M>,
    pub selector: Selector<M>,
    pub condition: SourceCondition<M>,
    /// Must hold together with `condition` (Always for most effects).
    pub second_condition: SourceCondition<M>,
    pub trigger: Trigger<M>,
    pub modifier: SourceModifier<M>,
    pub duration: Duration,
    pub stacking: Stacking,
}

impl<M: RulesModule> SourceEffect<M> {
    /// Plain passive effect (no condition, stacks).
    pub fn passive(scope: Scope<M>, selector: Selector<M>, modifier: SourceModifier<M>) -> Self {
        Self {
            scope,
            selector,
            condition: SourceCondition::Always,
            second_condition: SourceCondition::Always,
            trigger: Trigger::Passive,
            modifier,
            duration: Duration::WhileSourceExists,
            stacking: Stacking::Stack,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum SourceCondition<M: RulesModule> {
    Always,
    HasTechnology(StableContentKey),
    LacksTechnology(StableContentKey),
    MinPopulation(u32),
    HasBuilding(StableContentKey),
    TileImprovement(StableContentKey),
    TileResource(StableContentKey),
    TileFeature(StableContentKey),
    /// Tile scope: the tile is of this terrain; Unit scope: the battle
    /// or entered tile is.
    TileTerrain(StableContentKey),
    /// Unit scope: the tile's terrain, while the owner lacks the technology.
    TileTerrainUntil(StableContentKey, StableContentKey),
    /// Unit scope, combat: the opponent's combat role.
    OpponentRole(crate::rules::content::UnitRole),
    /// Unit scope, combat: the opponent's domain.
    OpponentDomain(crate::rules::content::Domain),
    /// Unit scope, combat: the opponent is of this unit's class.
    OpponentClassOf(StableContentKey),
    UnitDomain(crate::rules::content::Domain),
    UnitRole(crate::rules::content::UnitRole),
    /// OnUnitKilled of a building or a belief: within N tiles of its city.
    EventNearSourceCity(u8),
    TilePlot(crate::rules::effects::PlotFilter),
    TileResourceClass(crate::rules::content::ResourceClass),
    TileFeatureUnimproved(StableContentKey),
    CityAdjacentUnimprovedFeature {
        feature: StableContentKey,
        at_least: u8,
    },
    Producing(SourceProductionFilter),
    CityGarrisoned,
    CityCoastal,
    /// OnTechnologyUnlocked: the learned technology.
    LearnedTechnology(StableContentKey),
    CityOnRiver,
    AtPeace,
    ConnectedToCapital,
    /// Unit scope, combat: an own improvement within `range` tiles.
    NearOwnImprovement { improvement: StableContentKey, range: u8 },
    /// A word of the rules module, as content writes it.
    Module(ModuleSourceCondition<M>),
}

/// `Forbidden` as written in content (references by key).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize))]
pub enum SourceForbidden {
    UnitClassOf(StableContentKey),
    Annexing,
}

/// `ProductionFilter` with references by key.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize))]
pub enum SourceProductionFilter {
    Units,
    UnitDomain(crate::rules::content::Domain),
    UnitRole(crate::rules::content::UnitRole),
    UnitClassOf(StableContentKey),
    MilitaryUnits,
    SpaceshipParts,
    Buildings,
    WorldWonders { before_era: Option<StableContentKey> },
    BuildingClassOf(StableContentKey),
    BuiltInCapital,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum SourceModifier<M: RulesModule> {
    AddYield {
        yield_kind: YieldKind<M>,
        amount: i32,
    },
    AddYieldPercent {
        yield_kind: YieldKind<M>,
        percent: i32,
    },
    MultiplyYield {
        yield_kind: YieldKind<M>,
        percent: i32,
    },
    ModifyGrowth {
        percent: i32,
    },
    ModifyCost {
        target: CostTarget<M>,
        percent: i32,
    },
    AddYieldPerPopulation {
        yield_kind: YieldKind<M>,
        hundredths: i32,
    },
    ModifyCombat {
        context: CombatContext,
        percent: i32,
    },
    ModifyMovement {
        amount: i16,
    },
    UnlockUnit(StableContentKey),
    UnlockBuilding(StableContentKey),
    FreeUnit {
        unit: StableContentKey,
        count: u8,
    },
    FreeBuilding(StableContentKey),
    AddPromotion(StableContentKey),
    AddStat {
        stat: crate::rules::stats::SourceStat<M>,
        amount: i32,
    },
    ModifyUnitStat {
        stat: crate::rules::effects::UnitStat<M>,
        amount: i32,
    },
    GrantAbility(crate::rules::effects::UnitAbility<M>),
    PostCombatPromotion(StableContentKey),
    PassivePromotion(StableContentKey),
    ModifyCityCombat {
        stat: crate::rules::effects::CityCombatStat,
        amount: i32,
    },
    GrantExperience(u16),
    FreeBuildingInCities {
        building: StableContentKey,
        count: u8,
    },
    AddYieldPerCount {
        yield_kind: YieldKind<M>,
        amount: i32,
        per: PerCount<M>,
    },
    AddYieldPercentPerCount {
        yield_kind: YieldKind<M>,
        percent: i32,
        per: PerCount<M>,
    },
    YieldFromKill {
        yield_kind: YieldKind<M>,
        percent: i32,
    },
    Grant(crate::rules::effects::Grant<M>),
    Forbid(SourceForbidden),
    /// A word of the rules module, as content writes it.
    Module(ModuleSourceModifier<M>),
}

/// Promotion: a named bundle of unit effects, granted by AddPromotion.
#[derive(Debug, Clone, Default)]
pub struct SourcePromotion<M: RulesModule> {
    pub id: Option<StableContentKey>,
    pub effects: Vec<SourceEffect<M>>,
    // ---- Classic ----
    pub name: String,
    /// All of these promotions are required first.
    pub requires: Vec<StableContentKey>,
    /// One of these promotions is required first.
    pub requires_any_of: Vec<StableContentKey>,
    pub choosable: bool,
    pub lost_with_upgrade: bool,
    pub required_tech: Option<StableContentKey>,
    /// Unit combat types that may take it.
    pub unit_roles: Vec<UnitRole>,
    /// Civilian units that may take it.
    pub civilian_units: Vec<StableContentKey>,
    pub inert: Vec<SourceInertEffect>,
}

/// Nation start bias: affects the choice of the start tile
/// (the coast for naval nations, as in Classic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StartBias {
    #[default]
    None,
    Coastal,
}

/// Nation (the `Nation` record, classic/assets/rules/nations.ron): identity for the menu/HUD.
#[derive(Debug, Clone, Default)]
pub struct SourceNation<M: RulesModule> {
    pub id: Option<StableContentKey>,
    pub name: String,
    pub leader: String,
    /// Team colour (RGB, as in Classic).
    pub color: (u8, u8, u8),
    pub start_bias: StartBias,
    /// Unique abilities of the nation.
    pub effects: Vec<SourceEffect<M>>,
    // ---- Classic ----
    pub adjective: String,
    /// Name of the unique ability (trait).
    pub ability_name: String,
    pub unique_units: Vec<StableContentKey>,
    pub unique_buildings: Vec<StableContentKey>,
    pub unique_improvements: Vec<StableContentKey>,
    pub start_regions: StartRegions,
    pub free_techs: Vec<StableContentKey>,
    pub free_units: Vec<KeyCount>,
    /// The unit this nation starts with in a starting slot, instead of the
    /// slot's regular unit or its `replaces` variant (the Shoshone's
    /// Pathfinder in the defense slot). The unit must be the nation's own.
    pub start_units: Vec<(crate::rules::roles::StartingUnit, StableContentKey)>,
    pub inert: Vec<SourceInertEffect>,
    /// Ordered city names; an empty pool uses a readable generic fallback.
    pub city_names: Vec<String>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleSourceNation<M>,
}

/// Era (Classic): order and the starting parameters of a game started in it.
#[derive(Debug, Clone, Default)]
pub struct SourceEra<M: RulesModule> {
    pub id: Option<StableContentKey>,
    pub name: String,
    /// Order of the era (0 = Ancient).
    pub index: u8,
    pub starting_defense_units: u8,
    pub starting_worker_units: u8,
    pub starting_explore_units: u8,
    pub starting_gold: u32,
    pub research_agreement_cost: u32,
    pub embarked_unit_defense: u16,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleSourceEra<M>,
}

/// Difficulty level (classic/docs/classic/rules.md "Economy and
/// society", difficulty table): the player's own fields and the `ai_*`
/// fields the AI players take from the game's difficulty.
#[derive(Debug, Clone, Default)]
pub struct SourceHandicap<M: RulesModule> {
    pub id: Option<StableContentKey>,
    pub name: String,
    /// Order of the level (0 = Settler … 7 = Deity; the AI row is 8).
    pub index: u8,
    pub values: HandicapValues<M>,
}

/// The numbers of one difficulty level (field names in snake case). Content writes them as they are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HandicapValues<M: RulesModule> {
    pub production_free_units: i32,
    pub production_free_units_per_city: i32,
    pub production_free_units_population_percent: i32,
    pub gold_free_units: i32,
    pub route_cost_percent: i32,
    pub unit_cost_percent: i32,
    pub building_cost_percent: i32,
    pub research_percent: i32,
    pub ai_growth_percent: i32,
    pub ai_train_percent: i32,
    pub ai_world_train_percent: i32,
    pub ai_construct_percent: i32,
    pub ai_world_construct_percent: i32,
    pub ai_create_percent: i32,
    pub ai_world_create_percent: i32,
    pub ai_building_cost_percent: i32,
    pub ai_unit_cost_percent: i32,
    pub ai_unit_supply_percent: i32,
    pub ai_unit_upgrade_percent: i32,
    pub ai_per_era_modifier: i32,
    /// The rules module's numbers (`rules::rows::HandicapExtension`).
    pub module: ModuleHandicap<M>,
}

/// Terrain type (Classic). `map_code` links it to the map's terrain codes.
#[derive(Debug, Clone, Default)]
pub struct SourceTerrain {
    pub id: Option<StableContentKey>,
    pub name: String,
    pub yields: Yields,
    /// Change of yields on hills of this terrain.
    pub hills_yields: Yields,
    pub movement_cost: u8,
    pub impassable: bool,
    pub defense_pct: i32,
    pub water: bool,
    pub can_found_city: bool,
    /// The map terrain code the generator writes for it (None: the map has
    /// no such terrain yet).
    pub map_code: Option<u16>,
}

/// Natural wonder (Classic): a unique map feature.
#[derive(Debug, Clone, Default)]
pub struct SourceNaturalWonder<M: RulesModule> {
    pub id: Option<StableContentKey>,
    pub name: String,
    pub yields: Yields,
    pub impassable: bool,
    pub movement_cost: u8,
    pub defense_pct: i32,
    pub first_finder_gold: u32,
    pub fresh_water: bool,
    pub valid_terrains: Vec<StableContentKey>,
    pub adjacent_unit_free_promotion: Option<StableContentKey>,
    pub occurrence_frequency: u16,
    /// Rough ground: combat treats the tile as rough.
    pub rough: bool,
    /// No improvement may be built on it.
    pub no_improvement: bool,
    /// Where the map generator places it.
    pub placement: crate::rules::natural_wonders::SourcePlacement,
    pub inert: Vec<SourceInertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleSourceNaturalWonder<M>,
}

/// Specialist (Classic): the yields of a specialist slot.
#[derive(Debug, Clone, Default)]
pub struct SourceSpecialist<M: RulesModule> {
    pub id: Option<StableContentKey>,
    pub name: String,
    pub yields: Yields,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleSourceSpecialist<M>,
}

#[derive(Debug, Clone, Default)]
pub struct SourceRules<M: RulesModule> {
    pub units: Vec<SourceUnit<M>>,
    pub buildings: Vec<SourceBuilding<M>>,
    pub technologies: Vec<SourceTechnology<M>>,
    pub improvements: Vec<SourceImprovement<M>>,
    pub features: Vec<SourceFeature>,
    pub resources: Vec<SourceResource<M>>,
    pub promotions: Vec<SourcePromotion<M>>,
    pub nations: Vec<SourceNation<M>>,
    // ---- Classic ----
    pub eras: Vec<SourceEra<M>>,
    pub terrains: Vec<SourceTerrain>,
    pub natural_wonders: Vec<SourceNaturalWonder<M>>,
    pub specialists: Vec<SourceSpecialist<M>>,
    pub handicaps: Vec<SourceHandicap<M>>,
    /// Game speeds (`rules::speeds`); none: every game plays at Standard.
    pub speeds: Vec<crate::rules::speeds::SourceSpeed>,
    /// What plays each role the kernel names (`rules::roles`).
    pub roles: crate::rules::roles::SourceRoles<M>,
    /// The rules module's tables (`ModuleTables::Source`).
    pub module: ModuleSourceTables<M>,
}

impl<M: RulesModule> SourceRules<M> {
    /// Append every table of `other` (a ruleset split over several files).
    pub fn merge(&mut self, other: SourceRules<M>) {
        self.units.extend(other.units);
        self.buildings.extend(other.buildings);
        self.technologies.extend(other.technologies);
        self.improvements.extend(other.improvements);
        self.features.extend(other.features);
        self.resources.extend(other.resources);
        self.promotions.extend(other.promotions);
        self.nations.extend(other.nations);
        self.eras.extend(other.eras);
        self.terrains.extend(other.terrains);
        self.natural_wonders.extend(other.natural_wonders);
        self.specialists.extend(other.specialists);
        self.handicaps.extend(other.handicaps);
        self.speeds.extend(other.speeds);
        self.roles.merge(other.roles);
        M::Tables::merge_source(&mut self.module, other.module);
    }
}
