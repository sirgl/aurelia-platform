//! The rules module's fields on the kernel's table rows (platform/PLATFORM.md stage
//! 5c, the row extension).
//!
//! A kernel row (a unit, a building, a technology, ...) holds what any hex
//! 4X reads; a rules module adds its own fields to it as one value, the
//! row's `module` field, that the kernel compiles and hashes without knowing
//! its parts (Classic: a unit's faith price, a building's great work slots, a
//! resource's happiness). The module names one extension per kernel table
//! ([`ModuleRows`]), each with the form content writes (references by key)
//! and the compiled form ([`RowExtension`]). The kernel asks an extension
//! only what its own rules need ([`UnitExtension`], [`BuildingExtension`],
//! [`SpecialistExtension`], [`HandicapExtension`]).
//!
//! # Places
//!
//! A row hashes its fields in declaration order (the ruleset fingerprint,
//! `rules::fingerprint`). Before the split the module's fields sat between
//! the kernel's, so every row names the places its extension hashes at
//! (`UnitPlace`, ...) and writes the extension there: the fingerprint keeps
//! its bytes and the saves of every edition keep loading. A module without
//! that history writes its fields at any one place.

use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use crate::ids::ImprovementId;
use crate::module::RulesModule;
use crate::rules::compile::RulesError;
use crate::rules::compiled::{
    BuildingDefinition, NationDefinition, CompiledRuleset, EraDefinition, HandicapDefinition,
    ImprovementDefinition, NaturalWonderDefinition, ResourceDefinition, SpecialistDefinition,
    TechnologyDefinition, UnitDefinition,
};
use crate::rules::key_index::KeyIndex;
use crate::rules::source::HandicapValues;
use crate::rules::tables::ModuleTables;

/// The module's fields on the rows of one kernel table; `P` names the
/// places of that table's rows.
pub trait RowExtension<M: RulesModule, P>: Debug + Clone + Default + PartialEq + Eq + 'static {
    /// The fields as content writes them (references by key).
    type Source: Debug + Clone + Default;

    /// Resolve and check the fields of the row `key`. `rules` holds the
    /// tables compiled before this one: the kernel's tables the row may
    /// refer to, and the module's tables for the specialists.
    fn compile(source: &Self::Source, key: &str, ix: &KeyIndex, rules: &CompiledRuleset<M>) -> Result<Self, RulesError>;

    /// Hash the fields that sit at `place` in the row.
    fn hash_at<H: Hasher>(&self, place: P, state: &mut H);
}

/// What the kernel asks of the module's fields on a unit.
pub trait UnitExtension<M: RulesModule>: RowExtension<M, UnitPlace> {
    /// While a city builds it, the city's food surplus goes to production
    /// and the city neither grows nor starves (Classic: settlers).
    fn built_with_food(&self) -> bool;

    /// Bought, never trained: the city's production never completes it, only
    /// a purchase does Default: no unit is.
    fn purchase_only(&self) -> bool {
        false
    }

    /// Improvements a combat unit builds (a worker's builds do not need
    /// it: the unit's `builder` flag and domain say them). Classic: the
    /// Samurai's fishing boats, the Legion's forts. Default: none.
    fn builds(&self) -> &[ImprovementId] {
        &[]
    }

    /// Whether a combat unit builds roads (Classic: the Legion).
    fn builds_roads(&self) -> bool {
        false
    }
}

/// What the kernel asks of the module's fields on a building.
pub trait BuildingExtension<M: RulesModule>: RowExtension<M, BuildingPlace> {
    /// Enemy land units pay one more move per tile of its owner's land
    /// (`EffectIndex::border_obstacle_buildings`; Classic: the Great Wall).
    fn border_obstacle(&self) -> bool;
}

/// What the kernel asks of the module's fields on a specialist.
pub trait SpecialistExtension<M: RulesModule>: RowExtension<M, SpecialistPlace> {
    /// What one specialist of the kind earns per turn beyond its yields
    /// (Classic: great person points): the citizen manager weighs it.
    fn points(&self) -> u16;
}

/// The module's numbers of a difficulty level (`HandicapValues::module`).
/// Content writes them as they are: no references to resolve.
pub trait HandicapExtension: Debug + Copy + Default + PartialEq + Eq + 'static {
    /// The numbers of content without a handicap table.
    fn neutral() -> Self;

    /// The numbers an AI player plays with: the AI row's, with the game
    /// level's where the AI follows the game's difficulty.
    fn for_ai(level: &Self, ai: &Self) -> Self;

    /// Hash the numbers that sit at `place` in the level.
    fn hash_at<H: Hasher>(&self, place: HandicapPlace, state: &mut H);
}

/// The module's extension of every kernel table.
pub trait ModuleRows<M: RulesModule>: 'static {
    type Unit: UnitExtension<M>;
    type Building: BuildingExtension<M>;
    type Technology: RowExtension<M, TechnologyPlace>;
    type Improvement: RowExtension<M, ImprovementPlace>;
    type Resource: RowExtension<M, ResourcePlace>;
    type Nation: RowExtension<M, NationPlace>;
    type Era: RowExtension<M, EraPlace>;
    type NaturalWonder: RowExtension<M, NaturalWonderPlace>;
    type Specialist: SpecialistExtension<M>;
    type Handicap: HandicapExtension;
}

/// A module that adds no fields to the kernel's rows.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRows;

impl<M: RulesModule> ModuleRows<M> for NoRows {
    type Unit = ();
    type Building = ();
    type Technology = ();
    type Improvement = ();
    type Resource = ();
    type Nation = ();
    type Era = ();
    type NaturalWonder = ();
    type Specialist = ();
    type Handicap = ();
}

impl<M: RulesModule, P> RowExtension<M, P> for () {
    type Source = ();
    fn compile(_: &(), _: &str, _: &KeyIndex, _: &CompiledRuleset<M>) -> Result<(), RulesError> {
        Ok(())
    }
    fn hash_at<H: Hasher>(&self, _: P, _: &mut H) {}
}

impl<M: RulesModule> UnitExtension<M> for () {
    fn built_with_food(&self) -> bool {
        false
    }
}

impl<M: RulesModule> BuildingExtension<M> for () {
    fn border_obstacle(&self) -> bool {
        false
    }
}

impl<M: RulesModule> SpecialistExtension<M> for () {
    fn points(&self) -> u16 {
        0
    }
}

impl HandicapExtension for () {
    fn neutral() {}
    fn for_ai(_: &(), _: &()) {}
    fn hash_at<H: Hasher>(&self, _: HandicapPlace, _: &mut H) {}
}

/// The module's extension of each kernel table, and its source form.
pub type ModuleRowsOf<M> = <<M as RulesModule>::Tables as ModuleTables<M>>::Rows;
pub type ModuleUnit<M> = <ModuleRowsOf<M> as ModuleRows<M>>::Unit;
pub type ModuleBuilding<M> = <ModuleRowsOf<M> as ModuleRows<M>>::Building;
pub type ModuleTechnology<M> = <ModuleRowsOf<M> as ModuleRows<M>>::Technology;
pub type ModuleImprovement<M> = <ModuleRowsOf<M> as ModuleRows<M>>::Improvement;
pub type ModuleResource<M> = <ModuleRowsOf<M> as ModuleRows<M>>::Resource;
pub type ModuleNation<M> = <ModuleRowsOf<M> as ModuleRows<M>>::Nation;
pub type ModuleEra<M> = <ModuleRowsOf<M> as ModuleRows<M>>::Era;
pub type ModuleNaturalWonder<M> = <ModuleRowsOf<M> as ModuleRows<M>>::NaturalWonder;
pub type ModuleSpecialist<M> = <ModuleRowsOf<M> as ModuleRows<M>>::Specialist;
pub type ModuleHandicap<M> = <ModuleRowsOf<M> as ModuleRows<M>>::Handicap;
pub type ModuleSourceUnit<M> = <ModuleUnit<M> as RowExtension<M, UnitPlace>>::Source;
pub type ModuleSourceBuilding<M> = <ModuleBuilding<M> as RowExtension<M, BuildingPlace>>::Source;
pub type ModuleSourceTechnology<M> = <ModuleTechnology<M> as RowExtension<M, TechnologyPlace>>::Source;
pub type ModuleSourceImprovement<M> = <ModuleImprovement<M> as RowExtension<M, ImprovementPlace>>::Source;
pub type ModuleSourceResource<M> = <ModuleResource<M> as RowExtension<M, ResourcePlace>>::Source;
pub type ModuleSourceNation<M> = <ModuleNation<M> as RowExtension<M, NationPlace>>::Source;
pub type ModuleSourceEra<M> = <ModuleEra<M> as RowExtension<M, EraPlace>>::Source;
pub type ModuleSourceNaturalWonder<M> = <ModuleNaturalWonder<M> as RowExtension<M, NaturalWonderPlace>>::Source;
pub type ModuleSourceSpecialist<M> = <ModuleSpecialist<M> as RowExtension<M, SpecialistPlace>>::Source;

// ---- places ----

/// Where a unit's extension hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitPlace {
    AfterSight,
    AfterSpaceshipProject,
}

/// Where a building's extension hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingPlace {
    AfterClass,
    AfterSpecialistSlots,
    AfterReplaces,
}

/// Where a technology's extension hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TechnologyPlace {
    AfterResearchAgreement,
}

/// Where an improvement's extension hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImprovementPlace {
    AfterNation,
    AfterAdjacentSameCulture,
}

/// Where a resource's extension hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourcePlace {
    AfterYields,
    AfterInStandardGame,
    AfterImprovedYields,
}

/// Where a nation's extension hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NationPlace {
    AfterFreeUnits,
}

/// Where an era's extension hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraPlace {
    Last,
}

/// Where a natural wonder's extension hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NaturalWonderPlace {
    AfterDefense,
}

/// Where a specialist's extension hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialistPlace {
    Last,
}

/// Where a difficulty level's extension hashes (in its numbers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandicapPlace {
    First,
    AfterResearchPercent,
}

// ---- the rows' hashes: kernel fields in order, the extension at its places ----

impl<M: RulesModule> Hash for UnitDefinition<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let UnitDefinition {
            key, movement, strength, founder, builder, cost, required_tech, effects, name, class,
            role, domain, ranged_strength, range, sight, unbuildable, obsolete_tech,
            required_resources, upgrades_to, free_promotions, era, nation, replaces,
            requires_buildings, combat, required_project, spaceship_project, inert, module,
        } = self;
        key.hash(h);
        movement.hash(h);
        strength.hash(h);
        founder.hash(h);
        builder.hash(h);
        cost.hash(h);
        required_tech.hash(h);
        effects.hash(h);
        name.hash(h);
        class.hash(h);
        role.hash(h);
        domain.hash(h);
        ranged_strength.hash(h);
        range.hash(h);
        sight.hash(h);
        module.hash_at(UnitPlace::AfterSight, h);
        unbuildable.hash(h);
        obsolete_tech.hash(h);
        required_resources.hash(h);
        upgrades_to.hash(h);
        free_promotions.hash(h);
        era.hash(h);
        nation.hash(h);
        replaces.hash(h);
        requires_buildings.hash(h);
        combat.hash(h);
        required_project.hash(h);
        spaceship_project.hash(h);
        module.hash_at(UnitPlace::AfterSpaceshipProject, h);
        inert.hash(h);
    }
}

impl<M: RulesModule> Hash for BuildingDefinition<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let BuildingDefinition {
            key, cost, required_tech, wonder, effects, name, kind, class, unbuildable,
            maintenance, obsolete_tech, era, hurry_cost_modifier, conquest_survival_pct,
            never_capture, specialist_slots, consumes_resources, requires_buildings,
            requires_buildings_in_empire, cost_per_city_pct, placement, max_start_era,
            free_start_era, nation, replaces, inert, module,
        } = self;
        key.hash(h);
        cost.hash(h);
        required_tech.hash(h);
        wonder.hash(h);
        effects.hash(h);
        name.hash(h);
        kind.hash(h);
        class.hash(h);
        module.hash_at(BuildingPlace::AfterClass, h);
        unbuildable.hash(h);
        maintenance.hash(h);
        obsolete_tech.hash(h);
        era.hash(h);
        hurry_cost_modifier.hash(h);
        conquest_survival_pct.hash(h);
        never_capture.hash(h);
        specialist_slots.hash(h);
        module.hash_at(BuildingPlace::AfterSpecialistSlots, h);
        consumes_resources.hash(h);
        requires_buildings.hash(h);
        requires_buildings_in_empire.hash(h);
        cost_per_city_pct.hash(h);
        placement.hash(h);
        max_start_era.hash(h);
        free_start_era.hash(h);
        nation.hash(h);
        replaces.hash(h);
        module.hash_at(BuildingPlace::AfterReplaces, h);
        inert.hash(h);
    }
}

impl<M: RulesModule> Hash for TechnologyDefinition<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let TechnologyDefinition {
            key, era_name, unlocks_buildings, cost, requires, era, effects, name, content_era,
            grid, unlocks_units, unlocks_improvements, unlocks_projects, unlocks_actions,
            repeatable, allows_embarking, embarked_all_water, allows_embassy, allows_open_borders,
            allows_defensive_pact, allows_research_agreement, map_visible, bridge_building,
            road_move_change, inert, module,
        } = self;
        key.hash(h);
        era_name.hash(h);
        unlocks_buildings.hash(h);
        cost.hash(h);
        requires.hash(h);
        era.hash(h);
        effects.hash(h);
        name.hash(h);
        content_era.hash(h);
        grid.hash(h);
        unlocks_units.hash(h);
        unlocks_improvements.hash(h);
        unlocks_projects.hash(h);
        unlocks_actions.hash(h);
        repeatable.hash(h);
        allows_embarking.hash(h);
        embarked_all_water.hash(h);
        allows_embassy.hash(h);
        allows_open_borders.hash(h);
        allows_defensive_pact.hash(h);
        allows_research_agreement.hash(h);
        module.hash_at(TechnologyPlace::AfterResearchAgreement, h);
        map_visible.hash(h);
        bridge_building.hash(h);
        road_move_change.hash(h);
        inert.hash(h);
    }
}

impl<M: RulesModule> Hash for ImprovementDefinition<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let ImprovementDefinition {
            key, work_turns, required_tech, name, kind, unbuildable, yields, work_time,
            build_action, valid_terrains, valid_features, resources, fresh_water_yields,
            defense_pct, pillage_gold, culture_bomb_radius, nearby_enemy_damage, culture_per_era,
            nation, placement, adjacent_mountain_yields, removes_resource,
            adjacent_same_culture, inert, module,
        } = self;
        key.hash(h);
        work_turns.hash(h);
        required_tech.hash(h);
        name.hash(h);
        kind.hash(h);
        unbuildable.hash(h);
        yields.hash(h);
        work_time.hash(h);
        build_action.hash(h);
        valid_terrains.hash(h);
        valid_features.hash(h);
        resources.hash(h);
        fresh_water_yields.hash(h);
        defense_pct.hash(h);
        pillage_gold.hash(h);
        culture_bomb_radius.hash(h);
        nearby_enemy_damage.hash(h);
        culture_per_era.hash(h);
        nation.hash(h);
        module.hash_at(ImprovementPlace::AfterNation, h);
        placement.hash(h);
        adjacent_mountain_yields.hash(h);
        removes_resource.hash(h);
        adjacent_same_culture.hash(h);
        module.hash_at(ImprovementPlace::AfterAdjacentSameCulture, h);
        inert.hash(h);
    }
}

impl<M: RulesModule> Hash for ResourceDefinition<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let ResourceDefinition {
            key, required_tech, terrains, density, sea_density, name, class, yields, improvements,
            tradeable_with_tech, valid_terrains, valid_features, hills, flatlands,
            in_standard_game, nation, quantities, improved_yields, wonder_production, inert,
            module,
        } = self;
        key.hash(h);
        required_tech.hash(h);
        terrains.hash(h);
        density.hash(h);
        name.hash(h);
        class.hash(h);
        yields.hash(h);
        module.hash_at(ResourcePlace::AfterYields, h);
        improvements.hash(h);
        tradeable_with_tech.hash(h);
        valid_terrains.hash(h);
        valid_features.hash(h);
        hills.hash(h);
        flatlands.hash(h);
        in_standard_game.hash(h);
        module.hash_at(ResourcePlace::AfterInStandardGame, h);
        nation.hash(h);
        quantities.hash(h);
        improved_yields.hash(h);
        module.hash_at(ResourcePlace::AfterImprovedYields, h);
        wonder_production.hash(h);
        inert.hash(h);
        // Added after saves of every module existed: a resource that is not at
        // sea hashes as before, so the fingerprint of rules without sea
        // resources (Lands & Laws, the sandbox) did not move.
        if *sea_density != 0 {
            "sea_density".hash(h);
            sea_density.hash(h);
        }
    }
}

impl<M: RulesModule> Hash for NationDefinition<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.hash_fields(h);
        self.city_names.hash(h);
    }
}

impl<M: RulesModule> NationDefinition<M> {
    /// Every field but the city names, in order (the fingerprint before
    /// city names hashes exactly these).
    pub(crate) fn hash_fields<H: Hasher>(&self, h: &mut H) {
        let NationDefinition {
            key, name, leader, color, start_bias, effects, adjective, ability_name, unique_units,
            unique_buildings, unique_improvements, start_regions, free_techs, free_units, inert,
            city_names: _, module,
        } = self;
        key.hash(h);
        name.hash(h);
        leader.hash(h);
        color.hash(h);
        start_bias.hash(h);
        effects.hash(h);
        adjective.hash(h);
        ability_name.hash(h);
        unique_units.hash(h);
        unique_buildings.hash(h);
        unique_improvements.hash(h);
        start_regions.hash(h);
        free_techs.hash(h);
        free_units.hash(h);
        module.hash_at(NationPlace::AfterFreeUnits, h);
        inert.hash(h);
    }
}

impl<M: RulesModule> Hash for EraDefinition<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let EraDefinition {
            key, name, index, starting_defense_units, starting_worker_units, starting_explore_units,
            starting_gold, research_agreement_cost, embarked_unit_defense, module,
        } = self;
        key.hash(h);
        name.hash(h);
        index.hash(h);
        starting_defense_units.hash(h);
        starting_worker_units.hash(h);
        starting_explore_units.hash(h);
        starting_gold.hash(h);
        research_agreement_cost.hash(h);
        embarked_unit_defense.hash(h);
        module.hash_at(EraPlace::Last, h);
    }
}

impl<M: RulesModule> Hash for NaturalWonderDefinition<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let NaturalWonderDefinition {
            key, name, yields, impassable, movement_cost, defense_pct, first_finder_gold,
            fresh_water, valid_terrains, adjacent_unit_free_promotion, occurrence_frequency, rough,
            no_improvement, placement, inert, module,
        } = self;
        key.hash(h);
        name.hash(h);
        yields.hash(h);
        impassable.hash(h);
        movement_cost.hash(h);
        defense_pct.hash(h);
        module.hash_at(NaturalWonderPlace::AfterDefense, h);
        first_finder_gold.hash(h);
        fresh_water.hash(h);
        valid_terrains.hash(h);
        adjacent_unit_free_promotion.hash(h);
        occurrence_frequency.hash(h);
        rough.hash(h);
        no_improvement.hash(h);
        placement.hash(h);
        inert.hash(h);
    }
}

impl<M: RulesModule> Hash for SpecialistDefinition<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let SpecialistDefinition { key, name, yields, module } = self;
        key.hash(h);
        name.hash(h);
        yields.hash(h);
        module.hash_at(SpecialistPlace::Last, h);
    }
}

impl<M: RulesModule> Hash for HandicapDefinition<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let HandicapDefinition { key, name, index, values } = self;
        key.hash(h);
        name.hash(h);
        index.hash(h);
        values.hash(h);
    }
}

impl<M: RulesModule> Hash for HandicapValues<M> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        let HandicapValues {
            production_free_units, production_free_units_per_city,
            production_free_units_population_percent, gold_free_units, route_cost_percent,
            unit_cost_percent, building_cost_percent, research_percent, ai_growth_percent,
            ai_train_percent, ai_world_train_percent, ai_construct_percent,
            ai_world_construct_percent, ai_create_percent, ai_world_create_percent,
            ai_building_cost_percent, ai_unit_cost_percent, ai_unit_supply_percent,
            ai_unit_upgrade_percent, ai_per_era_modifier, module,
        } = self;
        module.hash_at(HandicapPlace::First, h);
        production_free_units.hash(h);
        production_free_units_per_city.hash(h);
        production_free_units_population_percent.hash(h);
        gold_free_units.hash(h);
        route_cost_percent.hash(h);
        unit_cost_percent.hash(h);
        building_cost_percent.hash(h);
        research_percent.hash(h);
        module.hash_at(HandicapPlace::AfterResearchPercent, h);
        ai_growth_percent.hash(h);
        ai_train_percent.hash(h);
        ai_world_train_percent.hash(h);
        ai_construct_percent.hash(h);
        ai_world_construct_percent.hash(h);
        ai_create_percent.hash(h);
        ai_world_create_percent.hash(h);
        ai_building_cost_percent.hash(h);
        ai_unit_cost_percent.hash(h);
        ai_unit_supply_percent.hash(h);
        ai_unit_upgrade_percent.hash(h);
        ai_per_era_modifier.hash(h);
    }
}
