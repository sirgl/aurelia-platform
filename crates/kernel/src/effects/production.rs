//! Production percents toward the item a city produces (classic
//! classic/docs/classic/rules.md). The
//! percent is part of the city's production rate while the item heads its
//! queue:
//!
//! ```text
//! effects     AddYieldPercent(Production) gated by Producing(filter)
//! resources   a world wonder of an era before the resource's era, in a
//!             city with the resource connected on its own tiles (Marble)
//! ```
//!
//! Overflow and hurried production take no percents (§5.3); the turn's
//! production step reads the rate of the city's snapshot.

use crate::module::RulesModule;
use crate::effects::module::ModuleEffects;
use crate::ids::{PlayerId, ProductionItem};
use crate::rules::content::{BuildingKind, EraId, UnitRole};
use crate::rules::effects::ProductionFilter;
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;

/// `item` is one of the items `filter` names, for a city of `owner`.
pub(crate) fn item_matches<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    item: ProductionItem,
    filter: ProductionFilter,
) -> bool {
    match item {
        ProductionItem::Unit(kind) => {
            let Some(unit) = rules.units.get(kind.0 as usize) else {
                return false;
            };
            unit_matches(rules, unit, filter)
        }
        ProductionItem::Building(kind) => {
            let Some(building) = rules.buildings.get(kind.0 as usize) else {
                return false;
            };
            match filter {
                ProductionFilter::BuiltInCapital => built_in_capital(auth, rules, owner, kind),
                _ => building_matches(rules, building, filter),
            }
        }
        // Projects take no item percents but the spaceship's own parts.
        ProductionItem::Project(_) => false,
    }
}

fn unit_matches<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    unit: &crate::rules::compiled::UnitDefinition<M>,
    filter: ProductionFilter,
) -> bool {
    match filter {
        ProductionFilter::Units => true,
        ProductionFilter::UnitDomain(domain) => unit.domain == domain,
        ProductionFilter::UnitRole(role) => unit.role == Some(role),
        ProductionFilter::UnitClassOf(of) => {
            let class = rules.units.get(of.0 as usize).and_then(|d| d.class.as_ref());
            class.is_some() && class == unit.class.as_ref()
        }
        ProductionFilter::MilitaryUnits => unit.role.is_some_and(UnitRole::is_military),
        ProductionFilter::SpaceshipParts => unit.role == Some(UnitRole::SpaceshipPart),
        ProductionFilter::Buildings
        | ProductionFilter::WorldWonders { .. }
        | ProductionFilter::BuildingClassOf(_)
        | ProductionFilter::BuiltInCapital => false,
    }
}

fn building_matches<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    building: &crate::rules::compiled::BuildingDefinition<M>,
    filter: ProductionFilter,
) -> bool {
    match filter {
        ProductionFilter::Buildings => true,
        ProductionFilter::WorldWonders { before_era } => {
            building.kind == BuildingKind::WorldWonder && built_before(rules, building.era, before_era)
        }
        ProductionFilter::BuildingClassOf(of) => {
            let class = rules.buildings.get(of.0 as usize).and_then(|d| d.class.as_ref());
            class.is_some() && class == building.class.as_ref()
        }
        ProductionFilter::Units
        | ProductionFilter::UnitDomain(_)
        | ProductionFilter::UnitRole(_)
        | ProductionFilter::UnitClassOf(_)
        | ProductionFilter::MilitaryUnits
        | ProductionFilter::SpaceshipParts
        | ProductionFilter::BuiltInCapital => false,
    }
}

/// The building's era is earlier than `before` (no limit: always; a
/// building without an era counts as the earliest).
fn built_before<M: RulesModule>(rules: &CompiledRuleset<M>, era: Option<EraId>, before: Option<EraId>) -> bool {
    let Some(before) = before else {
        return true;
    };
    let order = |e: EraId| rules.eras.get(e.0 as usize).map(|d| d.index).unwrap_or(0);
    match era {
        Some(era) => order(era) < order(before),
        None => true,
    }
}

/// Rome: the owner's capital has this building.
fn built_in_capital<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    building: crate::ids::BuildingKindId,
) -> bool {
    let Some(capital) = crate::world::city::capital_of(auth, rules, owner) else {
        return false;
    };
    auth.cities()
        .get(capital)
        .is_some_and(|core| core.buildings.contains(&building))
}

/// Percent of the resources the city has locally toward `item` (Marble:
/// world wonders before the Medieval era). 0 when no resource speeds
/// wonders or `item` is no world wonder.
pub(crate) fn resource_percent<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    item: ProductionItem,
) -> i32 {
    let wonder_resources = &rules.effect_index.wonder_resources;
    if wonder_resources.is_empty() {
        return 0;
    }
    let ProductionItem::Building(kind) = item else {
        return 0;
    };
    let Some(building) = rules.buildings.get(kind.0 as usize) else {
        return 0;
    };
    if building.kind != BuildingKind::WorldWonder {
        return 0;
    }
    let Some(city) = auth.indexes().city_on_tile(core.tile) else {
        return 0;
    };
    let local = M::Vocabulary::city_resources(auth, rules, city);
    let mut percent = 0;
    for resource in wonder_resources {
        let Some(bonus) = rules.resources[resource.0 as usize].wonder_production else {
            continue;
        };
        if local.contains(resource) && built_before(rules, building.era, bonus.before_era) {
            percent += bonus.percent;
        }
    }
    percent
}
