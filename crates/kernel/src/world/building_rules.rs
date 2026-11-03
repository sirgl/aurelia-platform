//! Where a building may be built (Classic, Classic data):
//! the rules of production orders and purchases beyond technology and
//! world-wonder uniqueness (`command::validate` checks those).
//!
//! ```text
//! unbuildable          palace, faith-only buildings
//! nation         a unique building of another nation, or the
//!                      regular building the own unique one replaces
//! national wonder      one per player (built or in a queue)
//! buildings            in the city; in every non-puppet city (national
//!                      wonders: `requires_buildings_in_empire`)
//! placement            water (a sea of ≥ 10 tiles next to the city), river,
//!                      mountain next to the city, mountain or terrain in
//!                      the city's territory, fresh water, flat city tile,
//!                      prohibited city terrain, local resources (any/all in
//!                      the territory), one building per exclusive group,
//!                      capital only
//! ```
//!
//! Policy branches, holy cities and faith purchases are other tracks'.

use crate::module::RulesModule;
use crate::world::city::MAX_TERRITORY_RADIUS;
use crate::ids::{BuildingKindId, CityHandle, ProductionItem, TileId};
use crate::math::hex_disc;
use crate::rules::compiled::{BuildingDefinition, BuildingPlacement};
use crate::rules::content::BuildingKind;
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::map::MIN_WATER_SIZE_FOR_OCEAN;
use crate::world::state::AuthWorld;

/// May `city` build `kind` (rules beyond technology and world wonders)?
pub(crate) fn building_site<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    city: CityHandle,
    kind: BuildingKindId,
) -> Result<(), &'static str> {
    let core = auth.cities().get(city).ok_or("unknown city")?;
    let def = rules
        .buildings
        .get(kind.0 as usize)
        .ok_or("unknown building")?;
    if def.unbuildable {
        return Err("cannot be built");
    }
    nation_rule(auth, rules, core, kind, def)?;
    national_wonder_rule(auth, core, kind, def)?;
    if first_missing(rules, core, &def.requires_buildings).is_some() {
        return Err("needs a building in the city");
    }
    if !in_every_city(auth, core, &def.requires_buildings_in_empire) {
        return Err("needs a building in every city");
    }
    placement_rule(auth, rules, city, core, &def.placement)
}

/// Does the city have `kind`, or the unique building of its owner that
/// replaces it (Classic: a requirement names the building class)?
pub fn has_class<M: RulesModule>(rules: &CompiledRuleset<M>, core: &CityCore, kind: BuildingKindId) -> bool {
    core.buildings.contains(&kind)
        || core
            .buildings
            .iter()
            .any(|b| rules.buildings.get(b.0 as usize).is_some_and(|def| def.replaces == Some(kind)))
}

/// The first of `required` the city does not have (see `has_class`).
pub fn first_missing<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    required: &[BuildingKindId],
) -> Option<BuildingKindId> {
    required.iter().copied().find(|b| !has_class(rules, core, *b))
}

/// A unique building of another nation, or the regular building the
/// owner's unique one replaces, is refused (production and faith purchase).
pub fn nation_rule<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    kind: BuildingKindId,
    def: &BuildingDefinition<M>,
) -> Result<(), &'static str> {
    let nation = auth.players().nation_of(core.owner);
    if def.nation.is_some_and(|c| c != nation) {
        return Err("another nation's building");
    }
    let replaced = rules
        .buildings
        .iter()
        .any(|b| b.replaces == Some(kind) && b.nation == Some(nation));
    if replaced {
        return Err("replaced by a unique building");
    }
    Ok(())
}

fn national_wonder_rule<M: RulesModule>(
    auth: &AuthWorld<M>,
    core: &CityCore,
    kind: BuildingKindId,
    def: &BuildingDefinition<M>,
) -> Result<(), &'static str> {
    if def.kind != BuildingKind::NationalWonder {
        return Ok(());
    }
    let item = ProductionItem::Building(kind);
    let taken = auth
        .indexes()
        .cities_of(core.owner)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .any(|c| c.buildings.contains(&kind) || c.queue.contains(&item));
    if taken {
        return Err("national wonder already built");
    }
    Ok(())
}

/// Every non-puppet city of the owner has all of `buildings`.
fn in_every_city<M: RulesModule>(auth: &AuthWorld<M>, core: &CityCore, buildings: &[BuildingKindId]) -> bool {
    if buildings.is_empty() {
        return true;
    }
    auth.indexes()
        .cities_of(core.owner)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .filter(|c| c.status != crate::world::city::CityStatus::Puppet)
        .all(|c| buildings.iter().all(|b| c.buildings.contains(b)))
}

fn placement_rule<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    city: CityHandle,
    core: &CityCore,
    p: &BuildingPlacement,
) -> Result<(), &'static str> {
    let map = auth.map();
    let kinds = map.terrain_kinds();
    let mountains = |t: TileId| map.terrain(t).is_some_and(|c| kinds.is_mountains(c.0));
    if p.water && !on_sea(auth, core.tile) {
        return Err("needs the coast");
    }
    if p.river && !map.river(core.tile) {
        return Err("needs a river");
    }
    let next_to_mountain = map
        .dimensions
        .neighbors(core.tile)
        .as_slice()
        .iter()
        .any(|&t| mountains(t));
    if p.mountain && !next_to_mountain {
        return Err("needs a mountain next to the city");
    }
    if p.flat && map.terrain(core.tile).is_some_and(|c| kinds.is_hills(c.0)) {
        return Err("not on hills");
    }
    if p.fresh_water {
        let facts = crate::world::facts::TruthFacts::new(auth, rules, core.owner);
        if !crate::derived::tile_yields::fresh_water(&facts, rules, core.tile) {
            return Err("needs fresh water");
        }
    }
    if p.capital && !is_capital(rules, core) {
        return Err("only in the capital");
    }
    let city_kind = map
        .terrain(core.tile)
        .and_then(|c| rules.terrain_kind_of(c));
    if p.prohibited_city_terrain.is_some() && city_kind == p.prohibited_city_terrain {
        return Err("not on this terrain");
    }
    exclusive_rule(rules, core, p)?;
    territory_rule(auth, rules, city, core, p)
}

/// The city touches a water body big enough for ocean buildings.
fn on_sea<M: RulesModule>(auth: &AuthWorld<M>, tile: TileId) -> bool {
    let map = auth.map();
    map.dimensions.neighbors(tile).as_slice().iter().any(|&t| {
        map.water_area(t)
            .is_some_and(|a| map.water_area_size(a) >= MIN_WATER_SIZE_FOR_OCEAN)
    })
}

fn is_capital<M: RulesModule>(rules: &CompiledRuleset<M>, core: &CityCore) -> bool {
    core.buildings.iter().any(|b| {
        rules
            .buildings
            .get(b.0 as usize)
            .is_some_and(|d| d.kind == BuildingKind::Palace)
    })
}

fn exclusive_rule<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    p: &BuildingPlacement,
) -> Result<(), &'static str> {
    let Some(group) = p.exclusive_group else {
        return Ok(());
    };
    let item_group = |b: &BuildingKindId| {
        rules
            .buildings
            .get(b.0 as usize)
            .and_then(|d| d.placement.exclusive_group)
    };
    let queued = core.queue.iter().filter_map(|i| match i {
        ProductionItem::Building(b) => Some(b),
        ProductionItem::Unit(_) | ProductionItem::Project(_) => None,
    });
    if core
        .buildings
        .iter()
        .chain(queued)
        .any(|b| item_group(b) == Some(group))
    {
        return Err("excludes a building of the city");
    }
    Ok(())
}

/// Mountains, terrain and resources in the city's territory.
fn territory_rule<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    city: CityHandle,
    core: &CityCore,
    p: &BuildingPlacement,
) -> Result<(), &'static str> {
    let needs_territory = p.nearby_mountain
        || p.nearby_terrain.is_some()
        || !p.local_resources_any.is_empty()
        || !p.local_resources_all.is_empty();
    if !needs_territory {
        return Ok(());
    }
    let map = auth.map();
    let tiles: Vec<TileId> = hex_disc(map.dimensions, core.tile, MAX_TERRITORY_RADIUS)
        .filter(|t| map.owning_city(*t) == Some(city))
        .collect();
    let has_terrain = |kind| {
        tiles
            .iter()
            .any(|t| map.terrain(*t).and_then(|c| rules.terrain_kind_of(c)) == Some(kind))
    };
    let has_resource = |r| tiles.iter().any(|t| map.resource(*t).flatten() == Some(r));
    if p.nearby_mountain
        && !tiles
            .iter()
            .any(|t| map.terrain(*t).is_some_and(|c| map.terrain_kinds().is_mountains(c.0)))
    {
        return Err("needs a mountain in the territory");
    }
    if p.nearby_terrain.is_some_and(|k| !has_terrain(k)) {
        return Err("needs a terrain in the territory");
    }
    if !p.local_resources_any.is_empty() && !p.local_resources_any.iter().any(|r| has_resource(*r))
    {
        return Err("needs a resource in the territory");
    }
    if !p.local_resources_all.iter().all(|r| has_resource(*r)) {
        return Err("needs resources in the territory");
    }
    Ok(())
}
