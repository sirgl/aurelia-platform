//! Runtime representation of the rules (after compilation).
//!
//! Every reference is resolved to an id (index into the table sorted by
//! key). Classic fields that no system reads yet are compiled and validated all
//! the same; `classic/docs/classic/implemented.md` lists which ones wait for M4.

use crate::module::RulesModule;
use crate::ids::{
    BuildingKindId, FeatureId, ImprovementId, PromotionId, ResourceId, TechId, UnitKindId,
};
use crate::rules::content::{
    BuildingKind, Domain, EraId, FreshWater, ImprovementKind, ImprovementPlacement, InertEffect,
    ProjectId, ResourceClass, SpecialistId, StartRegions, TerrainKindId, UnitRole, Yields,
};
use crate::rules::effects::{EffectIndex, EffectSet, YieldKind};
use crate::rules::roles::StartingUnit;
use crate::rules::rows::{
    ModuleBuilding, ModuleNation, ModuleEra, ModuleImprovement, ModuleNaturalWonder,
    ModuleResource, ModuleSpecialist, ModuleTechnology, ModuleUnit,
};
use crate::rules::tables::ModuleTables;

/// Strength of debug/default units not described by content (UnitKindId(0) with
/// an empty SourceRules). An explicit fallback, not a hidden one: combat code documents it.
pub const DEFAULT_STRENGTH: u16 = 10;
/// Movement points of default units (content sets them via movement).
pub const MOVEMENT_DEFAULT: u16 = 2;

/// Runtime definition of a unit, indexed by UnitKindId.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitDefinition<M: RulesModule> {
    pub key: String,
    pub movement: u16,
    pub strength: u16,
    pub founder: bool,
    pub builder: bool,
    /// Production cost (0 when `unbuildable`).
    pub cost: u32,
    /// Unlocking technology (None = available at once). Resolved in
    /// a second pass, as for buildings; an unknown key is an error.
    pub required_tech: Option<TechId>,
    pub effects: EffectSet<M>,
    // ---- Classic ----
    pub name: String,
    /// Class key (`core:unit-class/<slug>`); None in the v1 content.
    pub class: Option<String>,
    pub role: Option<UnitRole>,
    pub domain: Domain,
    pub ranged_strength: u16,
    pub range: u8,
    pub sight: u8,
    pub unbuildable: bool,
    pub obsolete_tech: Option<TechId>,
    pub required_resources: Vec<(ResourceId, u16)>,
    pub upgrades_to: Vec<UnitKindId>,
    pub free_promotions: Vec<PromotionId>,
    pub era: Option<EraId>,
    /// Index of the nation of a unique unit.
    pub nation: Option<u8>,
    pub replaces: Option<UnitKindId>,
    pub requires_buildings: Vec<BuildingKindId>,
    pub combat: crate::rules::content::UnitCombatTraits,
    pub required_project: Option<ProjectId>,
    pub spaceship_project: Option<ProjectId>,
    pub inert: Vec<InertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleUnit<M>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildingDefinition<M: RulesModule> {
    pub key: String,
    /// Production cost (0 when `unbuildable`).
    pub cost: u32,
    /// Unlocking technology (None = available at once). Resolved in
    /// a second pass (technologies refer to buildings and vice versa).
    pub required_tech: Option<TechId>,
    /// Wonder: global uniqueness (see SourceBuilding.wonder).
    pub wonder: bool,
    /// Typed effects, bucketed by the compiler (see `rules::effects`).
    pub effects: EffectSet<M>,
    // ---- Classic ----
    pub name: String,
    pub kind: BuildingKind,
    pub class: Option<String>,
    pub unbuildable: bool,
    pub maintenance: u32,
    pub obsolete_tech: Option<TechId>,
    pub era: Option<EraId>,
    pub hurry_cost_modifier: i32,
    pub conquest_survival_pct: u8,
    pub never_capture: bool,
    pub specialist_slots: Option<(SpecialistId, u16)>,
    pub consumes_resources: Vec<(ResourceId, u16)>,
    pub requires_buildings: Vec<BuildingKindId>,
    pub requires_buildings_in_empire: Vec<BuildingKindId>,
    pub cost_per_city_pct: i32,
    pub placement: BuildingPlacement,
    pub max_start_era: Option<EraId>,
    pub free_start_era: Option<EraId>,
    pub nation: Option<u8>,
    pub replaces: Option<BuildingKindId>,
    pub inert: Vec<InertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleBuilding<M>,
}

/// Where a building may be built (see `SourceBuildingPlacement`), with
/// resolved references.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct BuildingPlacement {
    pub water: bool,
    pub river: bool,
    pub mountain: bool,
    pub nearby_mountain: bool,
    pub fresh_water: bool,
    pub flat: bool,
    pub nearby_terrain: Option<TerrainKindId>,
    pub prohibited_city_terrain: Option<TerrainKindId>,
    pub local_resources_any: Vec<ResourceId>,
    pub local_resources_all: Vec<ResourceId>,
    pub exclusive_group: Option<u8>,
    pub capital: bool,
    pub allows_water_routes: bool,
}

/// Technology: unlocks buildings; the bonuses are its effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnologyDefinition<M: RulesModule> {
    pub key: String,
    /// Content era ("Ancient"): a column caption in the tree; plays no part
    /// in the simulation (that uses `era`).
    pub era_name: Option<String>,
    pub unlocks_buildings: Vec<BuildingKindId>,
    pub cost: u32,
    pub requires: Vec<TechId>,
    /// Era = depth in the prerequisite DAG (0 = root). Computed by the compiler.
    pub era: u8,
    pub effects: EffectSet<M>,
    // ---- Classic ----
    pub name: String,
    /// Content era.
    pub content_era: Option<EraId>,
    pub grid: (u8, u8),
    pub unlocks_units: Vec<UnitKindId>,
    pub unlocks_improvements: Vec<ImprovementId>,
    pub unlocks_projects: Vec<ProjectId>,
    pub unlocks_actions: Vec<String>,
    pub repeatable: bool,
    pub allows_embarking: bool,
    pub embarked_all_water: bool,
    pub allows_embassy: bool,
    pub allows_open_borders: bool,
    pub allows_defensive_pact: bool,
    pub allows_research_agreement: bool,
    /// Learning it reveals the whole map to the player
    /// (Satellites).
    pub map_visible: bool,
    /// Roads of its owner cross rivers (Engineering).
    pub bridge_building: bool,
    /// Change of the move points of a road step of its owner
    /// (Machinery −10, in 1/60 of a move).
    pub road_move_change: i16,
    pub inert: Vec<InertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleTechnology<M>,
}

/// Tile improvement: yield, where it is built, cost in work turns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImprovementDefinition<M: RulesModule> {
    pub key: String,
    /// Worker turns (0 = workers cannot build it).
    pub work_turns: u16,
    /// Unlocking technology (None = available at once).
    pub required_tech: Option<TechId>,
    pub name: String,
    pub kind: ImprovementKind,
    /// Workers cannot build it (`work_turns` is 0).
    pub unbuildable: bool,
    /// Yields of the improvement (v1 food/production bonuses folded in).
    pub yields: Yields,
    pub work_time: u32,
    pub build_action: Option<String>,
    pub valid_terrains: Vec<TerrainKindId>,
    pub valid_features: Vec<FeatureId>,
    pub resources: Vec<ResourceId>,
    /// Yields that depend on fresh water (with a technology, or always for
    /// the v1 river bonus).
    pub fresh_water_yields: Vec<TechYield<M>>,
    pub defense_pct: i32,
    pub pillage_gold: u32,
    pub culture_bomb_radius: u8,
    pub nearby_enemy_damage: u16,
    /// Culture per era between the landmark's record and its owner's era.
    pub culture_per_era: u16,
    pub nation: Option<u8>,
    /// Placement flags; with no listed terrain, feature or resource and no
    /// flag (the v1 content) the improvement fits any land.
    pub placement: ImprovementPlacement,
    /// Added once per adjacent mountain (terrace farm).
    pub adjacent_mountain_yields: Yields,
    /// Building it removes the tile's resource
    /// (archaeological dig).
    pub removes_resource: bool,
    /// Culture per adjacent improvement of the same kind
    /// (Moai).
    pub adjacent_same_culture: u8,
    pub inert: Vec<InertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleImprovement<M>,
}

/// Yield a technology adds to an improvement under a fresh water condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TechYield<M: RulesModule> {
    /// None: no technology needed (the v1 river bonus of a farm).
    pub tech: Option<TechId>,
    pub yield_kind: YieldKind<M>,
    pub amount: i32,
    pub fresh_water: FreshWater,
}

/// Tile feature (forest, jungle, swamp...): yield, movement, chopping.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FeatureDefinition {
    pub key: String,
    pub name: String,
    /// Yields (v1 production bonus folded in, additive).
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
    /// Only on flat land, not hills or mountains
    /// (flood plains, marsh, oasis).
    pub requires_flatlands: bool,
    /// Sight level the feature adds to its tile (forest and
    /// jungle 1); see `derived::visibility`.
    pub see_through: u8,
    pub valid_terrains: Vec<TerrainKindId>,
    /// How a worker removes it (None: it cannot be removed).
    pub removal: Option<FeatureRemoval>,
    pub inert: Vec<InertEffect>,
}

impl FeatureDefinition {
    /// May the feature lie on a tile of this shape (its placement rule)?
    pub fn fits_site(&self, flat: bool, river: bool) -> bool {
        (!self.requires_river || river) && (!self.requires_flatlands || flat)
    }
}

/// Removal of a feature by a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FeatureRemoval {
    pub tech: Option<TechId>,
    /// Worker time in hundredths of worker-turns (also added to an
    /// improvement built over the feature).
    pub work_time: u32,
    /// Chop production before the distance fall-off.
    pub production: u32,
}

/// Map resource: bonuses when developed by an improvement (and a tech).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDefinition<M: RulesModule> {
    pub key: String,
    /// Technology that reveals it: its yields count and
    /// the player sees it only with this technology.
    pub required_tech: Option<TechId>,
    /// Map terrain codes the generator may place it on.
    pub terrains: Vec<u16>,
    pub density: u8,
    /// Percent of the shallow sea it is placed on (`SourceResource::sea_density`).
    pub sea_density: u8,
    // ---- Classic ----
    pub name: String,
    pub class: Option<ResourceClass>,
    /// Yields of the tile once revealed (v1 bonuses without an improvement
    /// requirement folded in).
    pub yields: Yields,
    pub improvements: Vec<ImprovementId>,
    pub tradeable_with_tech: Option<TechId>,
    pub valid_terrains: Vec<TerrainKindId>,
    pub valid_features: Vec<FeatureId>,
    pub hills: bool,
    pub flatlands: bool,
    pub in_standard_game: bool,
    pub nation: Option<u8>,
    pub quantities: Vec<u8>,
    /// Extra yields with a connecting improvement (v1 bonuses that needed an
    /// improvement folded in).
    pub improved_yields: Vec<(ImprovementId, Yields)>,
    /// Production percent toward world wonders in a city that has the
    /// resource locally (Marble).
    pub wonder_production: Option<WonderProduction>,
    pub inert: Vec<InertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleResource<M>,
}

/// A resource's wonder bonus: a city with the resource connected on its own tiles
/// builds world wonders of an era before `before_era` `percent` % faster
/// (`effects::production`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WonderProduction {
    pub percent: i32,
    pub before_era: Option<EraId>,
}

/// Immutable after compilation. The simulation takes only `&CompiledRuleset`.
/// Nation (`Nation`): the key is required, name/leader are non-empty (checked by the compiler).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NationDefinition<M: RulesModule> {
    pub key: String,
    pub name: String,
    pub leader: String,
    pub color: (u8, u8, u8),
    pub start_bias: crate::rules::source::StartBias,
    pub effects: EffectSet<M>,
    // ---- Classic ----
    pub adjective: String,
    pub ability_name: String,
    pub unique_units: Vec<UnitKindId>,
    pub unique_buildings: Vec<BuildingKindId>,
    pub unique_improvements: Vec<ImprovementId>,
    pub start_regions: StartRegions,
    pub free_techs: Vec<TechId>,
    pub free_units: Vec<(UnitKindId, u16)>,
    /// The nation's own unit for a starting slot (`starting_units`); a slot
    /// not listed here takes the slot's unit or its `replaces` variant.
    pub start_units: Vec<(StartingUnit, UnitKindId)>,
    pub inert: Vec<InertEffect>,
    /// Ordered, validated names for newly founded cities.
    pub city_names: Vec<String>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleNation<M>,
}

/// Promotion: unit effects granted by AddPromotion (PromotionId = index).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PromotionDefinition<M: RulesModule> {
    pub key: String,
    pub effects: EffectSet<M>,
    // ---- Classic ----
    pub name: String,
    pub requires: Vec<PromotionId>,
    pub requires_any_of: Vec<PromotionId>,
    pub choosable: bool,
    pub lost_with_upgrade: bool,
    pub required_tech: Option<TechId>,
    pub unit_roles: Vec<UnitRole>,
    pub civilian_units: Vec<UnitKindId>,
    pub inert: Vec<InertEffect>,
}

impl<M: RulesModule> PromotionDefinition<M> {
    /// Choosing it heals the unit instead of staying on it (`InstaHeal`).
    pub fn heals_instantly(&self) -> bool {
        let heal = crate::rules::effects::Modifier::GrantAbility(crate::rules::effects::UnitAbility::InstaHeal);
        self.effects.passive_unit.iter().any(|e| e.modifier == heal)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EraDefinition<M: RulesModule> {
    pub key: String,
    pub name: String,
    pub index: u8,
    pub starting_defense_units: u8,
    pub starting_worker_units: u8,
    pub starting_explore_units: u8,
    pub starting_gold: u32,
    pub research_agreement_cost: u32,
    pub embarked_unit_defense: u16,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleEra<M>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TerrainDefinition {
    pub key: String,
    pub name: String,
    pub yields: Yields,
    pub hills_yields: Yields,
    pub movement_cost: u8,
    pub impassable: bool,
    pub defense_pct: i32,
    pub water: bool,
    pub can_found_city: bool,
    pub map_code: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NaturalWonderDefinition<M: RulesModule> {
    pub key: String,
    pub name: String,
    pub yields: Yields,
    pub impassable: bool,
    pub movement_cost: u8,
    pub defense_pct: i32,
    pub first_finder_gold: u32,
    pub fresh_water: bool,
    pub valid_terrains: Vec<TerrainKindId>,
    /// A unit that enters a tile next to the wonder gains this promotion
    ///, when its role may take it.
    pub adjacent_unit_free_promotion: Option<PromotionId>,
    pub occurrence_frequency: u16,
    /// Rough ground for combat.
    pub rough: bool,
    /// Workers may not improve the tile.
    pub no_improvement: bool,
    pub placement: crate::rules::natural_wonders::NaturalWonderPlacement,
    pub inert: Vec<InertEffect>,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleNaturalWonder<M>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecialistDefinition<M: RulesModule> {
    pub key: String,
    pub name: String,
    pub yields: Yields,
    /// The rules module's fields (`rules::rows`).
    pub module: ModuleSpecialist<M>,
}

/// Difficulty level, ordered by `index`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandicapDefinition<M: RulesModule> {
    pub key: String,
    pub name: String,
    pub index: u8,
    pub values: crate::rules::source::HandicapValues<M>,
}

/// Upper bound of the promotion table (units keep promotions in a bitset).
pub const MAX_PROMOTIONS: usize = 256;

#[derive(Debug, Clone, Default)]
pub struct CompiledRuleset<M: RulesModule> {
    pub units: Vec<UnitDefinition<M>>,
    pub buildings: Vec<BuildingDefinition<M>>,
    pub technologies: Vec<TechnologyDefinition<M>>,
    pub improvements: Vec<ImprovementDefinition<M>>,
    pub features: Vec<FeatureDefinition>,
    pub resources: Vec<ResourceDefinition<M>>,
    pub nations: Vec<NationDefinition<M>>,
    pub promotions: Vec<PromotionDefinition<M>>,
    // ---- Classic ----
    pub eras: Vec<EraDefinition<M>>,
    pub terrains: Vec<TerrainDefinition>,
    pub natural_wonders: Vec<NaturalWonderDefinition<M>>,
    pub specialists: Vec<SpecialistDefinition<M>>,
    /// Difficulty levels by `index` (Settler … Deity, then the AI row).
    pub handicaps: Vec<HandicapDefinition<M>>,
    /// Game speeds by `index`, the fastest first (`rules::speeds`).
    pub speeds: Vec<crate::rules::speeds::SpeedDefinition>,
    /// Compile-time indexes over all effects (by trigger, owner-wide).
    pub effect_index: EffectIndex<M>,
    /// Index = map terrain code (`TerrainId`): the terrain definition of
    /// that code (None: no definition, the code yields nothing).
    pub terrain_by_code: Vec<Option<TerrainKindId>>,
    /// What plays each role the kernel names (`rules::roles`).
    pub roles: crate::rules::roles::Roles<M>,
    /// What each map terrain code is (derived from the terrains and roles).
    pub terrain_kinds: crate::navigation::cost::TerrainKinds,
    pub version: u64,
    /// The rules module's tables (`rules::tables`).
    pub module: M::Tables,
}

/// Why the content forbids producing an item (see `production_gate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionBlock {
    /// Cannot be built with production (great people, palace, faith-only).
    Unbuildable,
    /// Unique to another nation.
    OtherNation,
}

impl<M: RulesModule> CompiledRuleset<M> {
    /// The terrain definition of a map terrain code (None: the ruleset does
    /// not define that code).
    pub(crate) fn terrain_of(&self, code: crate::ids::TerrainId) -> Option<&TerrainDefinition> {
        let kind = (*self.terrain_by_code.get(code.0 as usize)?)?;
        self.terrains.get(kind.0 as usize)
    }

    /// Terrain kind of a map terrain code.
    pub fn terrain_kind_of(&self, code: crate::ids::TerrainId) -> Option<TerrainKindId> {
        *self.terrain_by_code.get(code.0 as usize)?
    }

    /// Civilian units (settlers, workers) do not attack, exert no zone of
    /// control and share a tile with one military unit (Classic: Combat = 0).
    /// The unit schema has no combat flag yet, so founders and builders are
    /// the civilians; kinds outside the table are military debug units.
    pub fn is_civilian(&self, kind: crate::ids::UnitKindId) -> bool {
        match self.units.get(kind.0 as usize) {
            Some(def) => {
                let civilian_role = def.role.is_some_and(|r| !r.is_military());
                def.founder || def.builder || civilian_role
            }
            None => false,
        }
    }

    /// Effects of a content source (None for an id outside the tables).
    pub fn effect_set(&self, source: crate::rules::effects::SourceRef<M>) -> Option<&EffectSet<M>> {
        use crate::rules::effects::SourceRef;
        match source {
            SourceRef::Nation(i) => self.nations.get(i as usize).map(|d| &d.effects),
            SourceRef::Technology(id) => self.technologies.get(id.0 as usize).map(|d| &d.effects),
            SourceRef::Building(id) => self.buildings.get(id.0 as usize).map(|d| &d.effects),
            SourceRef::Unit(id) => self.units.get(id.0 as usize).map(|d| &d.effects),
            SourceRef::Promotion(id) => self.promotions.get(id.0 as usize).map(|d| &d.effects),
            SourceRef::Module(source) => self.module.effect_set(source),
        }
    }

    /// Content rules on what a player of nation `nation` may produce:
    /// None when allowed (technology and wonder checks stay in validation).
    pub fn production_gate(
        &self,
        item: crate::ids::ProductionItem,
        nation: u8,
    ) -> Option<ProductionBlock> {
        let (unbuildable, owner) = match item {
            crate::ids::ProductionItem::Unit(kind) => {
                let def = self.units.get(kind.0 as usize)?;
                (def.unbuildable, def.nation)
            }
            crate::ids::ProductionItem::Building(kind) => {
                let def = self.buildings.get(kind.0 as usize)?;
                (def.unbuildable, def.nation)
            }
            crate::ids::ProductionItem::Project(id) => (self.module.project(id)?.unbuildable, None),
        };
        if unbuildable {
            return Some(ProductionBlock::Unbuildable);
        }
        match owner {
            Some(owner) if owner != nation => Some(ProductionBlock::OtherNation),
            _ => None,
        }
    }

    /// Units a player of nation `nation` starts with in the first era
    /// (Classic: the nation's free units, then the era's defense, worker
    /// and explore units: the nation's own unit for the slot when its
    /// `start_units` name one, else the nation's variant of the unit playing
    /// that `StartingUnit` role). Content without eras or free units: one
    /// founder.
    pub fn starting_units(&self, nation: u8) -> Vec<UnitKindId> {
        let mut units = Vec::new();
        if let Some(def) = self.nations.get(nation as usize) {
            for (kind, count) in &def.free_units {
                for _ in 0..*count {
                    units.push(*kind);
                }
            }
        }
        if units.is_empty() {
            return self.first_founder().into_iter().collect();
        }
        let Some(era) = self.eras.iter().min_by_key(|e| e.index) else {
            return units;
        };
        let extra = [
            (StartingUnit::Defense, era.starting_defense_units),
            (StartingUnit::Worker, era.starting_worker_units),
            (StartingUnit::Explore, era.starting_explore_units),
        ];
        for (role, count) in extra {
            let Some(regular) = self.roles.starting_unit(role) else {
                continue;
            };
            let own = self.nations.get(nation as usize).and_then(|def| {
                def.start_units.iter().find(|(slot, _)| *slot == role).map(|(_, kind)| *kind)
            });
            let kind = own.unwrap_or_else(|| self.nation_variant_of(nation, regular));
            for _ in 0..count {
                units.push(kind);
            }
        }
        units
    }

    /// Does `nation` have a unique unit that replaces the regular unit `kind`?
    /// Such a nation does not build `kind` (the unit counterpart of the rule
    /// on buildings, `world::building_rules::nation_rule`).
    pub fn replaced_for(&self, nation: u8, kind: UnitKindId) -> bool {
        self.units.iter().any(|unit| unit.nation == Some(nation) && unit.replaces == Some(kind))
    }

    /// The unit of `nation` for the regular unit `key`: its unique replacement
    /// when it has one.
    pub fn nation_variant(&self, nation: u8, key: &str) -> Option<UnitKindId> {
        let regular = self.units.iter().position(|u| u.key == key)?;
        Some(self.nation_variant_of(nation, UnitKindId(regular as u16)))
    }

    /// The unit of `nation` for the regular unit `regular`.
    pub(crate) fn nation_variant_of(&self, nation: u8, regular: UnitKindId) -> UnitKindId {
        let unique = self
            .units
            .iter()
            .position(|unit| unit.nation == Some(nation) && unit.replaces == Some(regular));
        unique.map_or(regular, |i| UnitKindId(i as u16))
    }

    /// The difficulty level with this key.
    pub fn handicap(&self, key: &str) -> Option<&HandicapDefinition<M>> {
        self.handicaps.iter().find(|h| h.key == key)
    }

    /// The game speed with this key.
    pub fn speed(&self, key: &str) -> Option<&crate::rules::speeds::SpeedDefinition> {
        self.speeds.iter().find(|s| s.key == key)
    }

    /// The game speed whose key or name is `name` (case ignored; the key's
    /// last part, `quick` of `core:speed/quick`, too).
    pub fn speed_named(&self, name: &str) -> Option<&crate::rules::speeds::SpeedDefinition> {
        let wanted = name.trim();
        self.speeds.iter().find(|s| {
            let short = s.key.rsplit('/').next().unwrap_or(&s.key);
            s.key.eq_ignore_ascii_case(wanted) || s.name.eq_ignore_ascii_case(wanted) || short.eq_ignore_ascii_case(wanted)
        })
    }

    /// Key of the level a game plays at unless it chooses one (the
    /// `default_difficulty` role; empty without handicaps).
    pub fn default_difficulty(&self) -> &str {
        self.roles.default_difficulty(self).map_or("", |h| h.key.as_str())
    }

    /// The handicap every AI player plays with (the `ai_handicap` role);
    /// its `ai_*` fields come from the game's level.
    pub fn ai_handicap(&self) -> Option<&HandicapDefinition<M>> {
        self.roles.ai_handicap(self)
    }

    /// The first unit (in id order) that founds cities.
    pub(crate) fn first_founder(&self) -> Option<UnitKindId> {
        let index = self.units.iter().position(|u| u.founder)?;
        Some(UnitKindId(index as u16))
    }
}
