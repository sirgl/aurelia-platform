//! Semantic compiler: SourceRules -> CompiledRuleset.
//!
//! Boundary: invalid content does not penetrate the simulation.
//! Steps: keys (ids, duplicates) → rows of every table with their references
//! resolved (`KeyIndex`: ids are known before any row) → tech tree (cycles,
//! eras) → consistency checks (`compile_tables`) → effects (references,
//! validity, buckets) and inert effects → indexes.

use crate::module::RulesModule;
use crate::ids::{
    BuildingKindId, FeatureId, ImprovementId, PromotionId, ResourceId, TechId, UnitKindId,
};
use crate::rules::compile_effects::{
    build_effect_index, compile_effect_set, compile_inert, SourceKind,
};
use crate::rules::compile_tables::{check_consistency, compile_classic_tables};
use crate::rules::compiled::{BuildingDefinition, NationDefinition, CompiledRuleset, FeatureDefinition, FeatureRemoval, ImprovementDefinition, PromotionDefinition, ResourceDefinition, TechYield, TechnologyDefinition, UnitDefinition, WonderProduction, DEFAULT_STRENGTH, MAX_PROMOTIONS};
use crate::rules::tables::ModuleTables;
use crate::rules::content::{ContentTable, EraId, ProjectId, SpecialistId, TerrainKindId, Yields};
use crate::rules::roles::StartingUnit;
use crate::rules::rows::RowExtension;
use crate::rules::effects::EffectSet;
use crate::rules::key_index::{KeyIndex, TableKeys};
use crate::rules::source::{KeyCount, SourceEffect, SourceRules, StableContentKey};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesError {
    DuplicateId(String),
    UnknownReference(String),
    InvalidModifier(String),
    InvalidSelector(String),
    /// Condition does not fit the effect scope.
    InvalidCondition(String),
    InvalidTrigger(String),
    /// Duration does not fit the trigger/modifier (passive vs once vs timed).
    InvalidDuration(String),
    /// Forbidden stacking (compounding multipliers, deduplicated one-shots).
    InvalidStacking(String),
    ForbiddenCycle(String),
    /// Zero production cost: content must name the cost explicitly.
    ZeroCost(String),
    /// Empty name (a nation without a name cannot be shown in the menu).
    EmptyName(String),
    /// A table exceeds a runtime limit (promotions bitset).
    TooMany(String),
    /// Two records disagree (unlock lists vs required_tech, unique units vs
    /// their nation, a cost on an unbuildable item, ...).
    Inconsistent(String),
}

impl std::fmt::Display for RulesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for RulesError {}

/// Resolve stable IDs, check refs/duplicates/modifiers,
/// build runtime indexes + deterministic ordering.
pub fn compile_rules<M: RulesModule>(source: SourceRules<M>) -> Result<CompiledRuleset<M>, RulesError> {
    const { crate::rules::vocabulary::assert_fits::<M>() };
    let keys = SourceKeys::of(&source);
    let index = keys.index(&source)?;
    let mut rules = CompiledRuleset {
        version: 1,
        ..CompiledRuleset::default()
    };
    rules.technologies = compile_technologies(&source, &keys, &index, &rules)?;
    compute_eras(&mut rules)?;
    rules.units = compile_units(&source, &keys, &index, &rules)?;
    rules.buildings = compile_buildings(&source, &keys, &index, &rules)?;
    rules.improvements = compile_improvements(&source, &keys, &index, &rules)?;
    rules.features = compile_features(&source, &keys, &index)?;
    rules.nations = compile_nations(&source, &keys, &index, &rules)?;
    compile_effect_only_tables(&source, &keys, &index, &mut rules)?;
    compile_classic_tables(&source, &keys, &index, &mut rules)?;
    rules.handicaps = crate::rules::compile_handicaps::compile_handicaps(&source)?;
    rules.speeds = crate::rules::speeds::compile_speeds(&source.speeds)?;
    rules.resources = compile_resources(&source, &keys, &index, &rules)?;
    rules.terrain_by_code = terrain_codes(&rules)?;
    rules.roles = crate::rules::roles::compile_roles(&source.roles, &index, &rules)?;
    rules.terrain_kinds = crate::navigation::cost::TerrainKinds::from_rules(&rules);
    check_specialist_count(&rules)?;
    check_consistency(&source, &keys, &rules)?;
    attach_effects(&source, &keys, &index, &mut rules)?;
    rules.effect_index = build_effect_index(&rules)?;
    Ok(rules)
}

// ---- keys ----

/// Keys of every table (see `TableKeys`).
pub struct SourceKeys {
    pub units: TableKeys,
    pub buildings: TableKeys,
    pub technologies: TableKeys,
    pub improvements: TableKeys,
    pub features: TableKeys,
    pub resources: TableKeys,
    pub nations: TableKeys,
    pub promotions: TableKeys,
    pub eras: TableKeys,
    pub terrains: TableKeys,
    pub natural_wonders: TableKeys,
    pub specialists: TableKeys,
    /// The rules module's tables (`ModuleTables::source_keys`).
    pub module: Vec<(ContentTable, TableKeys)>,
}

impl SourceKeys {
    fn of<M: RulesModule>(s: &SourceRules<M>) -> Self {
        Self {
            units: TableKeys::of(s.units.iter().map(|x| &x.id), "unit"),
            buildings: TableKeys::of(s.buildings.iter().map(|x| &x.id), "building"),
            technologies: TableKeys::of(s.technologies.iter().map(|x| &x.id), "tech"),
            improvements: TableKeys::of(s.improvements.iter().map(|x| &x.id), "improvement"),
            features: TableKeys::of(s.features.iter().map(|x| &x.id), "feature"),
            resources: TableKeys::of(s.resources.iter().map(|x| &x.id), "resource"),
            nations: TableKeys::of(s.nations.iter().map(|x| &x.id), "civ"),
            promotions: TableKeys::of(s.promotions.iter().map(|x| &x.id), "promotion"),
            eras: TableKeys::of(s.eras.iter().map(|x| &x.id), "era"),
            terrains: TableKeys::of(s.terrains.iter().map(|x| &x.id), "terrain"),
            natural_wonders: TableKeys::of(
                s.natural_wonders.iter().map(|x| &x.id),
                "natural-wonder",
            ),
            specialists: TableKeys::of(s.specialists.iter().map(|x| &x.id), "specialist"),
            module: M::Tables::source_keys(&s.module),
        }
    }

    /// The keys of the module's table `table` (the module declared it).
    pub fn module_table(&self, table: ContentTable) -> &TableKeys {
        let declared = self.module.iter().find(|(t, _)| *t == table);
        &declared.expect("the module declares its tables").1
    }

    /// Register every table (duplicates across tables are errors) and the
    /// unit and building classes the rows declare.
    fn index<M: RulesModule>(&self, s: &SourceRules<M>) -> Result<KeyIndex, RulesError> {
        let mut index = KeyIndex::new();
        let tables = [
            (ContentTable::Unit, &self.units),
            (ContentTable::Building, &self.buildings),
            (ContentTable::Technology, &self.technologies),
            (ContentTable::Improvement, &self.improvements),
            (ContentTable::Feature, &self.features),
            (ContentTable::Resource, &self.resources),
            (ContentTable::Nation, &self.nations),
            (ContentTable::Promotion, &self.promotions),
            (ContentTable::Era, &self.eras),
            (ContentTable::Terrain, &self.terrains),
            (ContentTable::NaturalWonder, &self.natural_wonders),
            (ContentTable::Specialist, &self.specialists),
        ];
        let module = self.module.iter().map(|(table, keys)| (*table, keys));
        for (table, keys) in tables.into_iter().chain(module) {
            index.add_table(table, keys)?;
        }
        if self.nations.keys.len() > u8::MAX as usize + 1 {
            return Err(RulesError::TooMany("nations (max 256)".into()));
        }
        for unit in &s.units {
            if let Some(class) = &unit.class {
                index.add_unit_class(&class.0);
            }
        }
        for building in &s.buildings {
            if let Some(class) = &building.class {
                index.add_building_class(&class.0);
            }
        }
        Ok(index)
    }
}

// ---- reference helpers ----

pub fn tech(
    ix: &KeyIndex,
    key: &Option<StableContentKey>,
) -> Result<Option<TechId>, RulesError> {
    Ok(ix.opt_id(ContentTable::Technology, key)?.map(TechId))
}

pub fn era(
    ix: &KeyIndex,
    key: &Option<StableContentKey>,
) -> Result<Option<EraId>, RulesError> {
    Ok(ix.opt_id(ContentTable::Era, key)?.map(EraId))
}

pub(crate) fn nation(ix: &KeyIndex, key: &Option<StableContentKey>) -> Result<Option<u8>, RulesError> {
    Ok(ix
        .opt_id(ContentTable::Nation, key)?
        .map(|id| id as u8))
}

pub(crate) fn terrains(
    ix: &KeyIndex,
    keys: &[StableContentKey],
) -> Result<Vec<TerrainKindId>, RulesError> {
    Ok(ix
        .ids(ContentTable::Terrain, keys)?
        .into_iter()
        .map(TerrainKindId)
        .collect())
}

pub(crate) fn features(
    ix: &KeyIndex,
    keys: &[StableContentKey],
) -> Result<Vec<FeatureId>, RulesError> {
    Ok(ix
        .ids(ContentTable::Feature, keys)?
        .into_iter()
        .map(FeatureId)
        .collect())
}

pub(crate) fn buildings(
    ix: &KeyIndex,
    keys: &[StableContentKey],
) -> Result<Vec<BuildingKindId>, RulesError> {
    Ok(ix
        .ids(ContentTable::Building, keys)?
        .into_iter()
        .map(BuildingKindId)
        .collect())
}

pub(crate) fn units(
    ix: &KeyIndex,
    keys: &[StableContentKey],
) -> Result<Vec<UnitKindId>, RulesError> {
    Ok(ix
        .ids(ContentTable::Unit, keys)?
        .into_iter()
        .map(UnitKindId)
        .collect())
}

pub fn improvements(
    ix: &KeyIndex,
    keys: &[StableContentKey],
) -> Result<Vec<ImprovementId>, RulesError> {
    Ok(ix
        .ids(ContentTable::Improvement, keys)?
        .into_iter()
        .map(ImprovementId)
        .collect())
}

pub(crate) fn promotions(
    ix: &KeyIndex,
    keys: &[StableContentKey],
) -> Result<Vec<PromotionId>, RulesError> {
    Ok(ix
        .ids(ContentTable::Promotion, keys)?
        .into_iter()
        .map(PromotionId)
        .collect())
}

fn building_placement(
    ix: &KeyIndex,
    p: &crate::rules::content::SourceBuildingPlacement,
) -> Result<crate::rules::compiled::BuildingPlacement, RulesError> {
    let terrain = |key: &Option<StableContentKey>| -> Result<Option<TerrainKindId>, RulesError> {
        Ok(match key {
            Some(k) => Some(TerrainKindId(ix.id(ContentTable::Terrain, &k.0)?)),
            None => None,
        })
    };
    Ok(crate::rules::compiled::BuildingPlacement {
        water: p.water,
        river: p.river,
        mountain: p.mountain,
        nearby_mountain: p.nearby_mountain,
        fresh_water: p.fresh_water,
        flat: p.flat,
        nearby_terrain: terrain(&p.nearby_terrain)?,
        prohibited_city_terrain: terrain(&p.prohibited_city_terrain)?,
        local_resources_any: resources(ix, &p.local_resources_any)?,
        local_resources_all: resources(ix, &p.local_resources_all)?,
        exclusive_group: p.exclusive_group,
        capital: p.capital,
        allows_water_routes: p.allows_water_routes,
    })
}

fn resource_counts(
    ix: &KeyIndex,
    items: &[KeyCount],
) -> Result<Vec<(ResourceId, u16)>, RulesError> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let id = ix.id(ContentTable::Resource, &item.key.0)?;
        out.push((ResourceId(id), item.count));
    }
    Ok(out)
}

/// Production cost rule: buildable items name a cost, unbuildable ones none.
fn check_cost(key: &str, cost: u32, unbuildable: bool) -> Result<(), RulesError> {
    if unbuildable && cost != 0 {
        return Err(RulesError::Inconsistent(format!(
            "{key}: unbuildable with a cost"
        )));
    }
    if !unbuildable && cost == 0 {
        return Err(RulesError::ZeroCost(key.to_string()));
    }
    Ok(())
}

/// Legacy single bonus fields or Classic yields, never both.
fn check_one_yield_source(key: &str, legacy: bool, classic: bool) -> Result<(), RulesError> {
    if legacy && classic {
        let reason = format!("{key}: both v1 bonuses and Classic yields");
        return Err(RulesError::Inconsistent(reason));
    }
    Ok(())
}

// ---- tables ----

fn compile_units<M: RulesModule>(
    source: &SourceRules<M>,
    keys: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset<M>,
) -> Result<Vec<UnitDefinition<M>>, RulesError> {
    let mut out = Vec::with_capacity(source.units.len());
    for (id, row) in keys.units.order.iter().enumerate() {
        let u = &source.units[*row];
        let key = keys.units.key_of(id);
        check_cost(key, u.cost, u.unbuildable)?;
        // Pre-Classic content (no role) used 0 for "default strength"; Classic
        // content declares 0 for units that do not fight.
        let strength = if u.strength == 0 && u.role.is_none() {
            DEFAULT_STRENGTH
        } else {
            u.strength
        };
        let replaces = ix.opt_id(ContentTable::Unit, &u.replaces)?.map(UnitKindId);
        out.push(UnitDefinition {
            key: key.to_string(),
            movement: u.movement,
            strength,
            founder: u.founder,
            builder: u.builder,
            cost: u.cost,
            required_tech: tech(ix, &u.required_tech)?,
            effects: EffectSet::default(),
            name: u.name.clone(),
            class: u.class.as_ref().map(|c| c.0.clone()),
            role: u.role,
            domain: u.domain,
            ranged_strength: u.ranged_strength,
            range: u.range,
            sight: u.sight,
            unbuildable: u.unbuildable,
            obsolete_tech: tech(ix, &u.obsolete_tech)?,
            required_resources: resource_counts(ix, &u.required_resources)?,
            upgrades_to: units(ix, &u.upgrades_to)?,
            free_promotions: promotions(ix, &u.free_promotions)?,
            era: era(ix, &u.era)?,
            nation: nation(ix, &u.nation)?,
            replaces,
            requires_buildings: buildings(ix, &u.requires_buildings)?,
            combat: unit_combat(ix, &u.combat)?,
            required_project: project(ix, &u.required_project)?,
            spaceship_project: project(ix, &u.spaceship_project)?,
            inert: compile_inert(ix, key, &u.inert)?,
            module: RowExtension::compile(&u.module, key, ix, rules)?,
        });
    }
    Ok(out)
}

fn project(ix: &KeyIndex, key: &Option<StableContentKey>) -> Result<Option<ProjectId>, RulesError> {
    match key {
        Some(k) => Ok(Some(ProjectId(ix.id(ContentTable::Project, &k.0)?))),
        None => Ok(None),
    }
}

fn unit_combat(
    ix: &KeyIndex,
    c: &crate::rules::content::SourceUnitCombat,
) -> Result<crate::rules::content::UnitCombatTraits, RulesError> {
    Ok(crate::rules::content::UnitCombatTraits {
        capture: ix.opt_id(ContentTable::Unit, &c.capture)?.map(UnitKindId),
        ignore_building_defense: c.ignore_building_defense,
        immobile: c.immobile,
        air_intercept_range: c.air_intercept_range,
        suicide: c.suicide,
        nuke_level: c.nuke_level,
        ranged_only_in_domain: c.ranged_only_in_domain,
        carries: c.carries,
        carried_as: c.carried_as,
        pillage: c.pillage,
    })
}

fn compile_buildings<M: RulesModule>(
    source: &SourceRules<M>,
    keys: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset<M>,
) -> Result<Vec<BuildingDefinition<M>>, RulesError> {
    use crate::rules::content::BuildingKind;
    let mut out = Vec::with_capacity(source.buildings.len());
    for (id, row) in keys.buildings.order.iter().enumerate() {
        let b = &source.buildings[*row];
        let key = keys.buildings.key_of(id);
        check_cost(key, b.cost, b.unbuildable)?;
        // `wonder` (v1 flag) and the Classic kind must agree.
        let world_wonder = b.kind == BuildingKind::WorldWonder;
        if b.wonder && !matches!(b.kind, BuildingKind::Regular | BuildingKind::WorldWonder) {
            let reason = format!("{key}: wonder flag on a {:?} building", b.kind);
            return Err(RulesError::Inconsistent(reason));
        }
        let specialist_slots = match &b.specialist_slots {
            Some(slots) => {
                let id = ix.id(ContentTable::Specialist, &slots.key.0)?;
                Some((SpecialistId(id), slots.count))
            }
            None => None,
        };
        out.push(BuildingDefinition {
            key: key.to_string(),
            cost: b.cost,
            required_tech: tech(ix, &b.required_tech)?,
            wonder: b.wonder || world_wonder,
            effects: EffectSet::default(),
            name: b.name.clone(),
            kind: b.kind,
            class: b.class.as_ref().map(|c| c.0.clone()),
            unbuildable: b.unbuildable,
            maintenance: b.maintenance,
            obsolete_tech: tech(ix, &b.obsolete_tech)?,
            era: era(ix, &b.era)?,
            hurry_cost_modifier: b.hurry_cost_modifier,
            conquest_survival_pct: b.conquest_survival_pct,
            never_capture: b.never_capture,
            specialist_slots,
            consumes_resources: resource_counts(ix, &b.consumes_resources)?,
            requires_buildings: buildings(ix, &b.requires_buildings)?,
            requires_buildings_in_empire: buildings(ix, &b.requires_buildings_in_empire)?,
            cost_per_city_pct: b.cost_per_city_pct,
            placement: building_placement(ix, &b.placement)?,
            max_start_era: era(ix, &b.max_start_era)?,
            free_start_era: era(ix, &b.free_start_era)?,
            nation: nation(ix, &b.nation)?,
            replaces: ix
                .opt_id(ContentTable::Building, &b.replaces)?
                .map(BuildingKindId),
            inert: compile_inert(ix, key, &b.inert)?,
            module: RowExtension::compile(&b.module, key, ix, rules)?,
        });
    }
    Ok(out)
}

/// Technologies: unlocks and requires resolve by the index of keys
/// (unknown -> error); eras and cycles come in the next step.
fn compile_technologies<M: RulesModule>(
    source: &SourceRules<M>,
    keys: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset<M>,
) -> Result<Vec<TechnologyDefinition<M>>, RulesError> {
    let mut technologies = Vec::with_capacity(source.technologies.len());
    for (id, row) in keys.technologies.order.iter().enumerate() {
        let t = &source.technologies[*row];
        let key = keys.technologies.key_of(id);
        if t.cost == 0 {
            return Err(RulesError::ZeroCost(key.to_string()));
        }
        let mut requires = Vec::with_capacity(t.requires.len());
        for r in &t.requires {
            requires.push(TechId(ix.id(ContentTable::Technology, &r.0)?));
        }
        let mut actions = Vec::with_capacity(t.unlocks_actions.len());
        for action in &t.unlocks_actions {
            ix.any(&action.0)?;
            actions.push(action.0.clone());
        }
        let projects = ix.ids(ContentTable::Project, &t.unlocks_projects)?;
        technologies.push(TechnologyDefinition {
            key: key.to_string(),
            era_name: t.era_name.clone(),
            unlocks_buildings: buildings(ix, &t.unlocks_buildings)?,
            cost: t.cost,
            requires,
            era: 0,
            effects: EffectSet::default(),
            name: t.name.clone(),
            content_era: era(ix, &t.era)?,
            grid: t.grid,
            unlocks_units: units(ix, &t.unlocks_units)?,
            unlocks_improvements: improvements(ix, &t.unlocks_improvements)?,
            unlocks_projects: projects.into_iter().map(ProjectId).collect(),
            unlocks_actions: actions,
            repeatable: t.repeatable,
            allows_embarking: t.allows_embarking,
            embarked_all_water: t.embarked_all_water,
            allows_embassy: t.allows_embassy,
            allows_open_borders: t.allows_open_borders,
            allows_defensive_pact: t.allows_defensive_pact,
            allows_research_agreement: t.allows_research_agreement,
            map_visible: t.map_visible,
            bridge_building: t.bridge_building,
            road_move_change: t.road_move_change,
            inert: compile_inert(ix, key, &t.inert)?,
            module: RowExtension::compile(&t.module, key, ix, rules)?,
        });
    }
    Ok(technologies)
}

/// Cycles + eras (DAG depth): DFS with colours (0 new, 1 in-stack, 2 done).
fn compute_eras<M: RulesModule>(rules: &mut CompiledRuleset<M>) -> Result<(), RulesError> {
    let technologies = &rules.technologies;
    let n = technologies.len();
    let mut color = vec![0u8; n];
    let mut era = vec![0u8; n];
    for start in 0..n {
        let mut stack: Vec<(usize, bool)> = vec![(start, false)];
        while let Some((v, processed)) = stack.pop() {
            if processed {
                let mut depth = 0u8;
                for req in &technologies[v].requires {
                    depth = depth.max(era[req.0 as usize].saturating_add(1));
                }
                era[v] = depth;
                color[v] = 2;
                continue;
            }
            if color[v] == 2 {
                continue;
            }
            if color[v] == 1 {
                return Err(RulesError::ForbiddenCycle(technologies[v].key.clone()));
            }
            color[v] = 1;
            stack.push((v, true));
            for req in &technologies[v].requires {
                stack.push((req.0 as usize, false));
            }
        }
    }
    for (t, e) in rules.technologies.iter_mut().zip(era) {
        t.era = e;
    }
    Ok(())
}

/// Improvements: work_turns 0 is an honest error (there are no free constructions),
/// except `unbuildable`. Classic content gives `work_time` (hundredths); the v1
/// food/production bonuses become yields and the v1 river bonus a fresh
/// water yield without a technology.
fn compile_improvements<M: RulesModule>(
    source: &SourceRules<M>,
    keys: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset<M>,
) -> Result<Vec<ImprovementDefinition<M>>, RulesError> {
    let mut improvements = Vec::with_capacity(source.improvements.len());
    for (id, row) in keys.improvements.order.iter().enumerate() {
        let imp = &source.improvements[*row];
        let key = keys.improvements.key_of(id);
        let work_turns = improvement_work_turns(key, imp)?;
        let legacy = imp.food_bonus != 0 || imp.prod_bonus != 0 || imp.river_food != 0;
        check_one_yield_source(key, legacy, !imp.yields.is_zero())?;
        let mut yields = imp.yields;
        yields.food += imp.food_bonus;
        yields.production += imp.prod_bonus;
        let fresh_water_yields = improvement_fresh_water_yields(ix, imp)?;
        let build_action = match &imp.build_action {
            Some(action) => {
                ix.any(&action.0)?;
                Some(action.0.clone())
            }
            None => None,
        };
        improvements.push(ImprovementDefinition {
            key: key.to_string(),
            work_turns,
            required_tech: tech(ix, &imp.required_tech)?,
            name: imp.name.clone(),
            kind: imp.kind,
            unbuildable: imp.unbuildable,
            yields,
            work_time: if imp.work_time != 0 {
                imp.work_time
            } else {
                u32::from(work_turns) * 100
            },
            build_action,
            valid_terrains: terrains(ix, &imp.valid_terrains)?,
            valid_features: features(ix, &imp.valid_features)?,
            resources: resources(ix, &imp.resources)?,
            fresh_water_yields,
            defense_pct: imp.defense_pct,
            pillage_gold: imp.pillage_gold,
            culture_bomb_radius: imp.culture_bomb_radius,
            nearby_enemy_damage: imp.nearby_enemy_damage,
            culture_per_era: imp.culture_per_era,
            nation: nation(ix, &imp.nation)?,
            placement: imp.placement,
            adjacent_mountain_yields: imp.adjacent_mountain_yields,
            removes_resource: imp.removes_resource,
            adjacent_same_culture: imp.adjacent_same_culture,
            inert: compile_inert(ix, key, &imp.inert)?,
            module: RowExtension::compile(&imp.module, key, ix, rules)?,
        });
    }
    Ok(improvements)
}

fn resources(ix: &KeyIndex, keys: &[StableContentKey]) -> Result<Vec<ResourceId>, RulesError> {
    Ok(ix
        .ids(ContentTable::Resource, keys)?
        .into_iter()
        .map(ResourceId)
        .collect())
}

/// Classic technology yields under a fresh water condition, plus the v1 river
/// bonus (no technology, fresh water required).
fn improvement_fresh_water_yields<M: RulesModule>(
    ix: &KeyIndex,
    imp: &crate::rules::source::SourceImprovement<M>,
) -> Result<Vec<TechYield<M>>, RulesError> {
    let mut out = Vec::with_capacity(imp.fresh_water_yields.len() + 1);
    for y in &imp.fresh_water_yields {
        out.push(TechYield {
            tech: Some(TechId(ix.id(ContentTable::Technology, &y.tech.0)?)),
            yield_kind: y.yield_kind,
            amount: y.amount,
            fresh_water: y.fresh_water,
        });
    }
    if imp.river_food != 0 {
        out.push(TechYield {
            tech: None,
            yield_kind: crate::rules::effects::YieldKind::Food,
            amount: imp.river_food,
            fresh_water: crate::rules::content::FreshWater::Required,
        });
    }
    Ok(out)
}

/// Worker turns: v1 `work_turns`, or Classic `work_time` rounded up to whole
/// turns; unbuildable improvements have neither.
fn improvement_work_turns<M: RulesModule>(
    key: &str,
    imp: &crate::rules::source::SourceImprovement<M>,
) -> Result<u16, RulesError> {
    if imp.work_turns != 0 && imp.work_time != 0 {
        let reason = format!("{key}: both work_turns and work_time");
        return Err(RulesError::Inconsistent(reason));
    }
    let turns = if imp.work_time != 0 {
        work_time_turns(imp.work_time)
    } else {
        imp.work_turns
    };
    check_cost(key, u32::from(turns), imp.unbuildable)?;
    Ok(turns)
}

/// Worker time (hundredths of worker-turns, worker rate 100) in whole turns.
fn work_time_turns(work_time: u32) -> u16 {
    work_time.div_ceil(100).min(u16::MAX as u32) as u16
}

/// Features: yield (v1 production bonus folded in) and chopping. A removable
/// feature names its removal: v1 `chop_turns`/`chop_grant` (no technology)
/// or the Classic removal build; `unremovable` features have neither.
fn compile_features<M: RulesModule>(
    source: &SourceRules<M>,
    keys: &SourceKeys,
    ix: &KeyIndex,
) -> Result<Vec<FeatureDefinition>, RulesError> {
    let mut out = Vec::with_capacity(source.features.len());
    for (id, row) in keys.features.order.iter().enumerate() {
        let f = &source.features[*row];
        let key = keys.features.key_of(id);
        check_one_yield_source(key, f.prod_bonus != 0, !f.yields.is_zero())?;
        let mut yields = f.yields;
        yields.production += f.prod_bonus;
        out.push(FeatureDefinition {
            key: key.to_string(),
            name: f.name.clone(),
            yields,
            yield_not_additive: f.yield_not_additive,
            movement_cost: f.movement_cost,
            defense_pct: f.defense_pct,
            impassable: f.impassable,
            rough: f.rough,
            fresh_water: f.fresh_water,
            no_improvement: f.no_improvement,
            requires_river: f.requires_river,
            requires_flatlands: f.requires_flatlands,
            see_through: f.see_through,
            valid_terrains: terrains(ix, &f.valid_terrains)?,
            removal: feature_removal(key, f, ix)?,
            inert: compile_inert(ix, key, &f.inert)?,
        });
    }
    Ok(out)
}

fn feature_removal(
    key: &str,
    f: &crate::rules::source::SourceFeature,
    ix: &KeyIndex,
) -> Result<Option<FeatureRemoval>, RulesError> {
    let legacy = f.chop_turns != 0 || f.chop_grant != 0;
    if legacy && f.removal.is_some() {
        let reason = format!("{key}: both v1 chop fields and a Classic removal");
        return Err(RulesError::Inconsistent(reason));
    }
    if f.unremovable && (legacy || f.removal.is_some()) {
        let reason = format!("{key}: unremovable with a removal");
        return Err(RulesError::Inconsistent(reason));
    }
    if f.unremovable {
        return Ok(None);
    }
    let removal = match &f.removal {
        Some(r) => FeatureRemoval {
            tech: tech(ix, &r.tech)?,
            work_time: r.work_time,
            production: r.production,
        },
        None => FeatureRemoval {
            tech: None,
            work_time: u32::from(f.chop_turns) * 100,
            production: f.chop_grant,
        },
    };
    if removal.work_time == 0 {
        return Err(RulesError::ZeroCost(key.to_string()));
    }
    Ok(Some(removal))
}

/// Resources: density > 100 is an honest error (a silent clamp would hide typos).
/// Map codes: the v1 `terrains` plus the codes of the Classic `valid_terrains`.
/// The v1 bonuses become yields (no improvement needed) or improved yields
/// of the v1 `requires_improvement`.
fn compile_resources<M: RulesModule>(
    source: &SourceRules<M>,
    keys: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset<M>,
) -> Result<Vec<ResourceDefinition<M>>, RulesError> {
    let mut out = Vec::with_capacity(source.resources.len());
    for (id, row) in keys.resources.order.iter().enumerate() {
        let res = &source.resources[*row];
        let key = keys.resources.key_of(id);
        if res.density > 100 {
            return Err(RulesError::InvalidModifier(format!("density>100: {}", key)));
        }
        if res.sea_density > 100 {
            return Err(RulesError::InvalidModifier(format!("sea_density>100: {}", key)));
        }
        let legacy = res.food_bonus != 0 || res.prod_bonus != 0 || res.gold_bonus != 0;
        check_one_yield_source(key, legacy, !res.yields.is_zero())?;
        if res.yields.gold < 0 {
            return Err(RulesError::Inconsistent(format!("{key}: negative gold")));
        }
        let valid_terrains = terrains(ix, &res.valid_terrains)?;
        let map_codes = resource_map_codes(key, res, &valid_terrains, rules)?;
        let (yields, improved_yields) = resource_yields(ix, res)?;
        let mut connecting = improvements(ix, &res.improvements)?;
        for (improvement, _) in &improved_yields {
            if !connecting.contains(improvement) {
                connecting.push(*improvement);
            }
        }
        out.push(ResourceDefinition {
            key: key.to_string(),
            required_tech: tech(ix, &res.required_tech)?,
            terrains: map_codes,
            density: res.density,
            sea_density: res.sea_density,
            name: res.name.clone(),
            class: res.class,
            yields,
            improvements: connecting,
            tradeable_with_tech: tech(ix, &res.tradeable_with_tech)?,
            valid_terrains,
            valid_features: features(ix, &res.valid_features)?,
            hills: res.hills,
            flatlands: res.flatlands,
            in_standard_game: res.in_standard_game,
            nation: nation(ix, &res.nation)?,
            quantities: res.quantities.clone(),
            improved_yields,
            wonder_production: wonder_production(ix, key, res)?,
            inert: compile_inert(ix, key, &res.inert)?,
            module: RowExtension::compile(&res.module, key, ix, rules)?,
        });
    }
    Ok(out)
}

/// A resource's wonder production percent; a zero percent is an error (it
/// would mark the resource for nothing).
fn wonder_production<M: RulesModule>(
    ix: &KeyIndex,
    key: &str,
    res: &crate::rules::source::SourceResource<M>,
) -> Result<Option<WonderProduction>, RulesError> {
    let Some(source) = &res.wonder_production else {
        return Ok(None);
    };
    if source.percent == 0 {
        return Err(RulesError::InvalidModifier(format!("{key}: wonder production of 0 %")));
    }
    let before_era = match &source.before_era {
        Some(era) => Some(EraId(ix.id(ContentTable::Era, &era.0)?)),
        None => None,
    };
    Ok(Some(WonderProduction {
        percent: source.percent,
        before_era,
    }))
}

type ResourceYields = (Yields, Vec<(ImprovementId, Yields)>);

/// Base yields and the yields an improvement adds (Classic), or the v1 bonuses
/// (behind `requires_improvement` when it is set).
fn resource_yields<M: RulesModule>(
    ix: &KeyIndex,
    res: &crate::rules::source::SourceResource<M>,
) -> Result<ResourceYields, RulesError> {
    let mut improved = Vec::with_capacity(res.improved_yields.len());
    for (key, yields) in &res.improved_yields {
        let id = ImprovementId(ix.id(ContentTable::Improvement, &key.0)?);
        improved.push((id, *yields));
    }
    let legacy = Yields {
        food: res.food_bonus,
        production: res.prod_bonus,
        gold: res.gold_bonus.min(i32::MAX as u32) as i32,
        ..Yields::default()
    };
    if legacy.is_zero() {
        return Ok((res.yields, improved));
    }
    match ix.opt_id(ContentTable::Improvement, &res.requires_improvement)? {
        Some(id) => {
            improved.push((ImprovementId(id), legacy));
            Ok((res.yields, improved))
        }
        None => Ok((legacy, improved)),
    }
}

/// Cities track at most MAX_SPECIALIST_KINDS kinds.
fn check_specialist_count<M: RulesModule>(rules: &CompiledRuleset<M>) -> Result<(), RulesError> {
    let max = crate::world::city::MAX_SPECIALIST_KINDS;
    if rules.specialists.len() > max {
        return Err(RulesError::TooMany(format!("specialists (max {max})")));
    }
    Ok(())
}

/// Index of terrain definitions by map code (a code used twice is an error).
fn terrain_codes<M: RulesModule>(rules: &CompiledRuleset<M>) -> Result<Vec<Option<TerrainKindId>>, RulesError> {
    let mut by_code: Vec<Option<TerrainKindId>> = Vec::new();
    for (i, terrain) in rules.terrains.iter().enumerate() {
        let Some(code) = terrain.map_code else {
            continue;
        };
        let slot = code as usize;
        if by_code.len() <= slot {
            by_code.resize(slot + 1, None);
        }
        if by_code[slot].is_some() {
            let reason = format!("{}: map code {code} used twice", terrain.key);
            return Err(RulesError::Inconsistent(reason));
        }
        by_code[slot] = Some(TerrainKindId(i as u16));
    }
    Ok(by_code)
}

/// Map codes a resource may be placed on. A placed Classic resource (density
/// above 0) needs at least one valid terrain with a map code: an empty
/// list would mean "any terrain" to the generator.
fn resource_map_codes<M: RulesModule>(
    key: &str,
    res: &crate::rules::source::SourceResource<M>,
    valid: &[TerrainKindId],
    rules: &CompiledRuleset<M>,
) -> Result<Vec<u16>, RulesError> {
    let mut codes = res.terrains.clone();
    for terrain in valid {
        if let Some(code) = rules.terrains[terrain.0 as usize].map_code {
            codes.push(code);
        }
    }
    codes.sort_unstable();
    codes.dedup();
    let unmapped = !valid.is_empty() && codes.is_empty();
    if unmapped && res.density > 0 {
        let reason = format!("{key}: placed but no valid terrain is on the map");
        return Err(RulesError::Inconsistent(reason));
    }
    Ok(codes)
}

/// Nations: an empty name (shown in the menu/HUD) is an error.
fn compile_nations<M: RulesModule>(
    source: &SourceRules<M>,
    keys: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset<M>,
) -> Result<Vec<NationDefinition<M>>, RulesError> {
    let mut nations = Vec::with_capacity(source.nations.len());
    for (id, row) in keys.nations.order.iter().enumerate() {
        let c = &source.nations[*row];
        let key = keys.nations.key_of(id);
        if c.name.trim().is_empty() {
            return Err(RulesError::EmptyName(key.to_string()));
        }
        let mut city_names = std::collections::HashSet::new();
        for name in &c.city_names {
            if name.is_empty() || name.trim() != name {
                return Err(RulesError::Inconsistent(format!(
                    "{key}: city names must be nonempty and have no surrounding whitespace"
                )));
            }
            if !city_names.insert(name.to_lowercase()) {
                return Err(RulesError::Inconsistent(format!(
                    "{key}: duplicate city name {name:?}"
                )));
            }
        }
        let mut free_units = Vec::with_capacity(c.free_units.len());
        for unit in &c.free_units {
            let kind = UnitKindId(ix.id(ContentTable::Unit, &unit.key.0)?);
            free_units.push((kind, unit.count));
        }
        let mut start_units: Vec<(StartingUnit, UnitKindId)> = Vec::with_capacity(c.start_units.len());
        for (slot, unit) in &c.start_units {
            if start_units.iter().any(|(taken, _)| taken == slot) {
                return Err(RulesError::Inconsistent(format!("{key}: two start units for the {slot:?} slot")));
            }
            let kind = UnitKindId(ix.id(ContentTable::Unit, &unit.0)?);
            start_units.push((*slot, kind));
        }
        nations.push(NationDefinition {
            key: key.to_string(),
            name: c.name.clone(),
            leader: c.leader.clone(),
            color: c.color,
            start_bias: c.start_bias,
            effects: EffectSet::default(),
            adjective: c.adjective.clone(),
            ability_name: c.ability_name.clone(),
            unique_units: units(ix, &c.unique_units)?,
            unique_buildings: buildings(ix, &c.unique_buildings)?,
            unique_improvements: improvements(ix, &c.unique_improvements)?,
            start_regions: c.start_regions.clone(),
            free_techs: ix
                .ids(ContentTable::Technology, &c.free_techs)?
                .into_iter()
                .map(TechId)
                .collect(),
            free_units,
            start_units,
            inert: compile_inert(ix, key, &c.inert)?,
            city_names: c.city_names.clone(),
            module: RowExtension::compile(&c.module, key, ix, rules)?,
        });
    }
    Ok(nations)
}

/// Promotions: effects are attached later.
fn compile_effect_only_tables<M: RulesModule>(
    s: &SourceRules<M>,
    k: &SourceKeys,
    ix: &KeyIndex,
    rules: &mut CompiledRuleset<M>,
) -> Result<(), RulesError> {
    if s.promotions.len() > MAX_PROMOTIONS {
        let count = s.promotions.len();
        let reason = format!("{count} promotions (max {MAX_PROMOTIONS})");
        return Err(RulesError::TooMany(reason));
    }
    for (id, row) in k.promotions.order.iter().enumerate() {
        let p = &s.promotions[*row];
        let key = k.promotions.key_of(id);
        rules.promotions.push(PromotionDefinition {
            key: key.to_string(),
            effects: EffectSet::default(),
            name: p.name.clone(),
            requires: promotions(ix, &p.requires)?,
            requires_any_of: promotions(ix, &p.requires_any_of)?,
            choosable: p.choosable,
            lost_with_upgrade: p.lost_with_upgrade,
            required_tech: tech(ix, &p.required_tech)?,
            unit_roles: p.unit_roles.clone(),
            civilian_units: units(ix, &p.civilian_units)?,
            inert: compile_inert(ix, key, &p.inert)?,
        });
    }
    Ok(())
}

// ---- effects ----

/// Effects of every source, compiled against the finished tables (all keys
/// are known, so references resolve), then stored on their definitions.
fn attach_effects<M: RulesModule>(
    s: &SourceRules<M>,
    k: &SourceKeys,
    ix: &KeyIndex,
    rules: &mut CompiledRuleset<M>,
) -> Result<(), RulesError> {
    let sets = effect_sets(rules, ix, SourceKind::Unit, &k.units, |i| {
        &s.units[i].effects
    })?;
    for (def, set) in rules.units.iter_mut().zip(sets) {
        def.effects = set;
    }
    let sets = effect_sets(rules, ix, SourceKind::Building, &k.buildings, |i| {
        &s.buildings[i].effects
    })?;
    for (def, set) in rules.buildings.iter_mut().zip(sets) {
        def.effects = set;
    }
    let sets = effect_sets(rules, ix, SourceKind::Technology, &k.technologies, |i| {
        &s.technologies[i].effects
    })?;
    for (def, set) in rules.technologies.iter_mut().zip(sets) {
        def.effects = set;
    }
    let sets = effect_sets(rules, ix, SourceKind::Nation, &k.nations, |i| {
        &s.nations[i].effects
    })?;
    for (def, set) in rules.nations.iter_mut().zip(sets) {
        def.effects = set;
    }
    let sets = effect_sets(rules, ix, SourceKind::Promotion, &k.promotions, |i| {
        &s.promotions[i].effects
    })?;
    for (def, set) in rules.promotions.iter_mut().zip(sets) {
        def.effects = set;
    }
    M::Tables::attach_effects(s, k, ix, rules)
}

/// Compiled effect sets of one table, in id order.
pub fn effect_sets<'a, M: RulesModule>(
    rules: &CompiledRuleset<M>,
    ix: &KeyIndex,
    kind: SourceKind<M>,
    keys: &TableKeys,
    effects_of: impl Fn(usize) -> &'a [SourceEffect<M>],
) -> Result<Vec<EffectSet<M>>, RulesError> {
    let mut sets = Vec::with_capacity(keys.order.len());
    for (id, row) in keys.order.iter().enumerate() {
        let key = keys.key_of(id);
        sets.push(compile_effect_set(rules, ix, kind, key, effects_of(*row))?);
    }
    Ok(sets)
}

