//! Compilation of the kernel tables added for the Classic ruleset (eras,
//! terrains, natural wonders, specialists), the rules module's tables
//! between them, and the cross-table consistency checks.

use crate::module::RulesModule;
use std::collections::HashSet;

use crate::rules::compile::{terrains, RulesError, SourceKeys};
use crate::rules::compile_effects::compile_inert;
use crate::rules::compiled::{
    CompiledRuleset, EraDefinition, NaturalWonderDefinition, SpecialistDefinition,
    TerrainDefinition,
};
use crate::rules::content::ContentTable;
use crate::rules::key_index::KeyIndex;
use crate::rules::rows::RowExtension;
use crate::rules::source::SourceRules;
use crate::rules::tables::ModuleTables;

pub(crate) fn compile_classic_tables<M: RulesModule>(
    s: &SourceRules<M>,
    k: &SourceKeys,
    ix: &KeyIndex,
    rules: &mut CompiledRuleset<M>,
) -> Result<(), RulesError> {
    rules.eras = compile_eras(s, k, ix, rules)?;
    rules.terrains = compile_terrains(s, k)?;
    rules.natural_wonders = compile_natural_wonders(s, k, ix, rules)?;
    rules.module = M::Tables::compile(s, k, ix, rules)?;
    rules.specialists = compile_specialists(s, k, ix, rules)?;
    Ok(())
}

fn compile_eras<M: RulesModule>(
    s: &SourceRules<M>,
    k: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset<M>,
) -> Result<Vec<EraDefinition<M>>, RulesError> {
    let mut out = Vec::with_capacity(s.eras.len());
    let mut seen_index = HashSet::new();
    for (id, row) in k.eras.order.iter().enumerate() {
        let e = &s.eras[*row];
        let key = k.eras.key_of(id);
        if !seen_index.insert(e.index) {
            let reason = format!("{key}: era index {} used twice", e.index);
            return Err(RulesError::Inconsistent(reason));
        }
        out.push(EraDefinition {
            key: key.to_string(),
            name: e.name.clone(),
            index: e.index,
            starting_defense_units: e.starting_defense_units,
            starting_worker_units: e.starting_worker_units,
            starting_explore_units: e.starting_explore_units,
            starting_gold: e.starting_gold,
            research_agreement_cost: e.research_agreement_cost,
            embarked_unit_defense: e.embarked_unit_defense,
            module: RowExtension::compile(&e.module, key, ix, rules)?,
        });
    }
    Ok(out)
}

fn compile_terrains<M: RulesModule>(s: &SourceRules<M>, k: &SourceKeys) -> Result<Vec<TerrainDefinition>, RulesError> {
    let mut out = Vec::with_capacity(s.terrains.len());
    let mut codes = HashSet::new();
    for (id, row) in k.terrains.order.iter().enumerate() {
        let t = &s.terrains[*row];
        let key = k.terrains.key_of(id);
        if let Some(code) = t.map_code {
            if !codes.insert(code) {
                let reason = format!("{key}: map code {code} used twice");
                return Err(RulesError::Inconsistent(reason));
            }
        }
        out.push(TerrainDefinition {
            key: key.to_string(),
            name: t.name.clone(),
            yields: t.yields,
            hills_yields: t.hills_yields,
            movement_cost: t.movement_cost,
            impassable: t.impassable,
            defense_pct: t.defense_pct,
            water: t.water,
            can_found_city: t.can_found_city,
            map_code: t.map_code,
        });
    }
    Ok(out)
}

fn compile_natural_wonders<M: RulesModule>(
    s: &SourceRules<M>,
    k: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset<M>,
) -> Result<Vec<NaturalWonderDefinition<M>>, RulesError> {
    let mut out = Vec::with_capacity(s.natural_wonders.len());
    for (id, row) in k.natural_wonders.order.iter().enumerate() {
        let w = &s.natural_wonders[*row];
        let key = k.natural_wonders.key_of(id);
        let promotion = ix.opt_id(ContentTable::Promotion, &w.adjacent_unit_free_promotion)?;
        out.push(NaturalWonderDefinition {
            key: key.to_string(),
            name: w.name.clone(),
            yields: w.yields,
            impassable: w.impassable,
            movement_cost: w.movement_cost,
            defense_pct: w.defense_pct,
            first_finder_gold: w.first_finder_gold,
            fresh_water: w.fresh_water,
            valid_terrains: terrains(ix, &w.valid_terrains)?,
            adjacent_unit_free_promotion: promotion.map(crate::ids::PromotionId),
            occurrence_frequency: w.occurrence_frequency,
            rough: w.rough,
            no_improvement: w.no_improvement,
            placement: compile_placement(ix, key, &w.placement)?,
            inert: compile_inert(ix, key, &w.inert)?,
            module: RowExtension::compile(&w.module, key, ix, rules)?,
        });
    }
    Ok(out)
}

/// Resolve the placement rules of a natural wonder.
fn compile_placement(
    ix: &KeyIndex,
    key: &str,
    p: &crate::rules::natural_wonders::SourcePlacement,
) -> Result<crate::rules::natural_wonders::NaturalWonderPlacement, RulesError> {
    let mut core_terrains = Vec::with_capacity(p.core_terrains.len());
    for t in &p.core_terrains {
        core_terrains.push(terrain_kind(ix, t)?);
    }
    let core_terrain = match &p.core_terrain {
        Some(t) => Some(terrain_kind(ix, t)?),
        None => None,
    };
    Ok(crate::rules::natural_wonders::Placement {
        method: p.method,
        avoid_ocean_coast: p.avoid_ocean_coast,
        avoid_biggest_landmass: p.avoid_biggest_landmass,
        core_terrains,
        adjacent: compile_adjacent(ix, key, &p.adjacent)?,
        core_shape: p.core_shape,
        core_terrain,
        surround_with_water: p.surround_with_water,
    })
}

fn terrain_kind(
    ix: &KeyIndex,
    key: &crate::rules::source::StableContentKey,
) -> Result<crate::rules::content::TerrainKindId, RulesError> {
    ix.id(ContentTable::Terrain, &key.0).map(crate::rules::content::TerrainKindId)
}

/// The neighbour rules, references resolved; a bound beyond six
/// neighbours and a kind counted twice are errors.
fn compile_adjacent(
    ix: &KeyIndex,
    key: &str,
    rules: &[crate::rules::natural_wonders::AdjacentRule<
        crate::rules::source::StableContentKey,
        crate::rules::source::StableContentKey,
    >],
) -> Result<Vec<crate::rules::natural_wonders::AdjacentRule<crate::rules::content::TerrainKindId, crate::ids::FeatureId>>, RulesError> {
    use crate::rules::natural_wonders::{AdjacentBound, AdjacentKind, AdjacentRule, NEIGHBOURS};
    let mut out: Vec<AdjacentRule<_, _>> = Vec::with_capacity(rules.len());
    for rule in rules {
        let kind = match &rule.kind {
            AdjacentKind::Hills => AdjacentKind::Hills,
            AdjacentKind::Mountain => AdjacentKind::Mountain,
            AdjacentKind::HillsOrMountain => AdjacentKind::HillsOrMountain,
            AdjacentKind::Land => AdjacentKind::Land,
            AdjacentKind::Water => AdjacentKind::Water,
            AdjacentKind::Terrain(t) => AdjacentKind::Terrain(terrain_kind(ix, t)?),
            AdjacentKind::Feature(f) => AdjacentKind::Feature(crate::ids::FeatureId(ix.id(ContentTable::Feature, &f.0)?)),
        };
        let (AdjacentBound::AtLeast(count) | AdjacentBound::AtMost(count)) = rule.bound;
        if count > NEIGHBOURS {
            return Err(RulesError::InvalidModifier(format!("{key}: placement bound {count} > {NEIGHBOURS}")));
        }
        if out.iter().any(|r| r.kind == kind) {
            return Err(RulesError::InvalidModifier(format!("{key}: placement counts {kind:?} twice")));
        }
        out.push(AdjacentRule { kind, bound: rule.bound });
    }
    Ok(out)
}

/// Specialists, after the module's tables (their fields may refer to them).
fn compile_specialists<M: RulesModule>(
    s: &SourceRules<M>,
    k: &SourceKeys,
    ix: &KeyIndex,
    rules: &CompiledRuleset<M>,
) -> Result<Vec<SpecialistDefinition<M>>, RulesError> {
    let mut out = Vec::with_capacity(s.specialists.len());
    for (id, row) in k.specialists.order.iter().enumerate() {
        let sp = &s.specialists[*row];
        let key = k.specialists.key_of(id);
        out.push(SpecialistDefinition {
            key: key.to_string(),
            name: sp.name.clone(),
            yields: sp.yields,
            module: RowExtension::compile(&sp.module, key, ix, rules)?,
        });
    }
    Ok(out)
}

// ---- consistency ----

pub(crate) fn check_consistency<M: RulesModule>(
    _s: &SourceRules<M>,
    _k: &SourceKeys,
    rules: &CompiledRuleset<M>,
) -> Result<(), RulesError> {
    check_unlocks(rules)?;
    check_uniques(rules)?;
    M::Tables::check(rules)?;
    let promotions: Vec<Vec<u16>> = rules
        .promotions
        .iter()
        .map(|p| {
            p.requires
                .iter()
                .chain(&p.requires_any_of)
                .map(|x| x.0)
                .collect()
        })
        .collect();
    check_acyclic(&promotions, |i| &rules.promotions[i].key)?;
    let upgrades: Vec<Vec<u16>> = rules
        .units
        .iter()
        .map(|u| u.upgrades_to.iter().map(|x| x.0).collect())
        .collect();
    check_acyclic(&upgrades, |i| &rules.units[i].key)
}

/// A technology lists exactly the items whose `required_tech` it is. Pre-Classic
/// content lists only buildings: the other lists are checked when declared.
fn check_unlocks<M: RulesModule>(rules: &CompiledRuleset<M>) -> Result<(), RulesError> {
    let lists_units = rules
        .technologies
        .iter()
        .any(|t| !t.unlocks_units.is_empty());
    let lists_improvements = rules
        .technologies
        .iter()
        .any(|t| !t.unlocks_improvements.is_empty());
    for (i, t) in rules.technologies.iter().enumerate() {
        let tech = crate::ids::TechId(i as u16);
        let buildings = rules
            .buildings
            .iter()
            .enumerate()
            .filter(|(_, b)| b.required_tech == Some(tech));
        let expected: Vec<u16> = buildings.map(|(j, _)| j as u16).collect();
        mirror(
            &t.key,
            "buildings",
            &expected,
            t.unlocks_buildings.iter().map(|b| b.0),
        )?;
        let expected: Vec<u16> = rules.module.projects_of_tech(tech).iter().map(|p| p.0).collect();
        mirror(
            &t.key,
            "projects",
            &expected,
            t.unlocks_projects.iter().map(|p| p.0),
        )?;
        if lists_units {
            let units = rules
                .units
                .iter()
                .enumerate()
                .filter(|(_, u)| u.required_tech == Some(tech));
            let expected: Vec<u16> = units.map(|(j, _)| j as u16).collect();
            mirror(
                &t.key,
                "units",
                &expected,
                t.unlocks_units.iter().map(|u| u.0),
            )?;
        }
        if lists_improvements {
            let imps = rules
                .improvements
                .iter()
                .enumerate()
                .filter(|(_, m)| m.required_tech == Some(tech));
            let expected: Vec<u16> = imps.map(|(j, _)| j as u16).collect();
            mirror(
                &t.key,
                "improvements",
                &expected,
                t.unlocks_improvements.iter().map(|m| m.0),
            )?;
        }
    }
    Ok(())
}

fn mirror(
    key: &str,
    what: &str,
    expected: &[u16],
    listed: impl Iterator<Item = u16>,
) -> Result<(), RulesError> {
    let mut listed: Vec<u16> = listed.collect();
    listed.sort_unstable();
    listed.dedup();
    if listed == expected {
        return Ok(());
    }
    let reason = format!(
        "{key}: unlocked {what} {listed:?} differ from the {what} requiring it {expected:?}"
    );
    Err(RulesError::Inconsistent(reason))
}

/// Unique units/buildings/improvements and their civilization name each
/// other; a unique unit or building replaces a regular one of its class.
fn check_uniques<M: RulesModule>(rules: &CompiledRuleset<M>) -> Result<(), RulesError> {
    for (c, nation) in rules.nations.iter().enumerate() {
        let c = c as u8;
        let units = rules
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.nation == Some(c));
        let expected: Vec<u16> = units.map(|(i, _)| i as u16).collect();
        mirror(
            &nation.key,
            "unique units",
            &expected,
            nation.unique_units.iter().map(|u| u.0),
        )?;
        let buildings = rules
            .buildings
            .iter()
            .enumerate()
            .filter(|(_, b)| b.nation == Some(c));
        let expected: Vec<u16> = buildings.map(|(i, _)| i as u16).collect();
        mirror(
            &nation.key,
            "unique buildings",
            &expected,
            nation.unique_buildings.iter().map(|b| b.0),
        )?;
        let imps = rules
            .improvements
            .iter()
            .enumerate()
            .filter(|(_, m)| m.nation == Some(c));
        let expected: Vec<u16> = imps.map(|(i, _)| i as u16).collect();
        mirror(
            &nation.key,
            "unique improvements",
            &expected,
            nation.unique_improvements.iter().map(|m| m.0),
        )?;
    }
    for unit in &rules.units {
        if let Some(replaced) = unit.replaces {
            let regular = &rules.units[replaced.0 as usize];
            check_replacement(
                &unit.key,
                unit.nation,
                &unit.class,
                regular.nation,
                &regular.class,
            )?;
        }
    }
    for building in &rules.buildings {
        if let Some(replaced) = building.replaces {
            let regular = &rules.buildings[replaced.0 as usize];
            check_replacement(
                &building.key,
                building.nation,
                &building.class,
                regular.nation,
                &regular.class,
            )?;
        }
    }
    Ok(())
}

fn check_replacement(
    key: &str,
    nation: Option<u8>,
    class: &Option<String>,
    regular_nation: Option<u8>,
    regular_class: &Option<String>,
) -> Result<(), RulesError> {
    let reason = if nation.is_none() {
        "replaces a unit or building but belongs to no nation"
    } else if regular_nation.is_some() {
        "replaces another unique item"
    } else if class != regular_class {
        "replaces an item of another class"
    } else {
        return Ok(());
    };
    Err(RulesError::Inconsistent(format!("{key}: {reason}")))
}

/// DFS with colors over `edges[i]` (prerequisites of i): a cycle is a
/// `ForbiddenCycle` naming one of its nodes.
pub fn check_acyclic<'a>(edges: &[Vec<u16>], key: impl Fn(usize) -> &'a str) -> Result<(), RulesError> {
    let n = edges.len();
    let mut color = vec![0u8; n];
    for start in 0..n {
        let mut stack: Vec<(usize, bool)> = vec![(start, false)];
        while let Some((v, processed)) = stack.pop() {
            if processed {
                color[v] = 2;
                continue;
            }
            if color[v] == 2 {
                continue;
            }
            if color[v] == 1 {
                return Err(RulesError::ForbiddenCycle(key(v).to_string()));
            }
            color[v] = 1;
            stack.push((v, true));
            for next in &edges[v] {
                stack.push((*next as usize, false));
            }
        }
    }
    Ok(())
}
