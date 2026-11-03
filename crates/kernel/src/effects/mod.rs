//! Effect interpretation over the authoritative world (read-only).
//!
//! Rules are data (`rules::effects`); this module answers "which modifiers
//! reach this city / unit / player now" for the gameplay systems, derived
//! yields and validation. It never mutates: triggered effects are applied
//! by `systems::effects` inside a transaction.
//!
//! Lookup cost is bounded by the compile-time buckets and indexes:
//! - a city visits its nation, the owner's known technologies, its own
//!   buildings and only the owner-wide buildings of `EffectIndex`;
//! - a unit visits the same owner sources plus its kind and promotions;
//! - nothing scans every effect of the ruleset.
//!
//! The rules module adds sources of its own (`effects::module`: Classic's
//! adopted policies at the owner level, beliefs for the cities that follow
//! their religion), decides its conditions and keeps its numbers.

pub mod city;
pub mod count;
pub mod memo;
pub mod module;
pub mod player;
pub(crate) mod production;
pub mod unit;

use crate::module::RulesModule;
use crate::effects::module::ModuleEffects;
use crate::ids::{BuildingKindId, PlayerId, TileId};
use crate::rules::effects::{CompiledEffect, Condition, EffectSet, Stacking};
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;

/// One bucket of an effect set (passive effects of one scope).
pub type Bucket<M> = fn(&EffectSet<M>) -> &[CompiledEffect<M>];

/// Where a condition is evaluated: the owner, plus the city and the tile
/// when the scope has them.
#[derive(Clone, Copy)]
pub struct ConditionContext<'a> {
    pub owner: PlayerId,
    pub city: Option<&'a CityCore>,
    pub tile: Option<TileId>,
    /// Domain and role of the target unit (Unit scope with a known kind).
    pub unit: Option<UnitTraits>,
    /// The item the city produces (the head of its queue, or an item the
    /// AI and the HUD estimate): `Producing` conditions look at it.
    pub producing: Option<crate::ids::ProductionItem>,
}

/// What a unit-kind condition looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitTraits {
    pub domain: crate::rules::content::Domain,
    pub role: Option<crate::rules::content::UnitRole>,
}

impl UnitTraits {
    pub fn of<M: RulesModule>(rules: &CompiledRuleset<M>, kind: crate::ids::UnitKindId) -> Self {
        let def = rules.units.get(kind.0 as usize);
        Self {
            domain: def.map(|d| d.domain).unwrap_or_default(),
            role: def.and_then(|d| d.role),
        }
    }
}

pub(crate) fn condition_holds<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    at: ConditionContext<'_>,
    c: Condition<M>,
) -> bool {
    match c {
        Condition::Always => true,
        Condition::HasTechnology(t) => auth.players().techs_of(at.owner).contains(&t),
        Condition::LacksTechnology(t) => !auth.players().techs_of(at.owner).contains(&t),
        Condition::MinPopulation(n) => match at.city {
            Some(city) => city.population >= n,
            None => false,
        },
        Condition::HasBuilding(b) => match at.city {
            Some(city) => city.buildings.contains(&b),
            None => false,
        },
        Condition::TileImprovement(i) => match at.tile {
            Some(tile) => auth.map().working_improvement(tile) == Some(i),
            None => false,
        },
        Condition::TileResource(r) => match at.tile {
            Some(tile) => auth.map().resource(tile) == Some(Some(r)),
            None => false,
        },
        Condition::TileFeature(f) => match at.tile {
            Some(tile) => auth.map().feature(tile) == Some(Some(f)),
            None => false,
        },
        // Situational conditions belong to a battle or a step: the combat
        // profile and the movement profile keep them and evaluate them
        // there (`crate::combat::profile`); owner-level evaluation skips them.
        Condition::UnitDomain(d) => at.unit.is_some_and(|u| u.domain == d),
        Condition::UnitRole(r) => at.unit.is_some_and(|u| u.role == Some(r)),
        Condition::TileTerrainUntil(..)
        | Condition::OpponentRole(_)
        | Condition::OpponentDomain(_)
        | Condition::OpponentClassOf(_) => false,
        Condition::TileTerrain(t) => {
            let code = rules.terrains.get(t.0 as usize).and_then(|d| d.map_code);
            match (at.tile, code) {
                (Some(tile), Some(code)) => auth.map().terrain(tile) == Some(crate::ids::TerrainId(code)),
                _ => false,
            }
        }
        // Situational: the combat profile evaluates it on the unit's tile.
        Condition::NearOwnImprovement { .. } => false,
        // An event condition: the firing of the event evaluates it
        // (`systems::effects`); it never holds outside one.
        Condition::EventNearSourceCity(_) => false,
        Condition::TilePlot(plot) => match at.tile {
            Some(tile) => plot_matches(auth, rules, at.owner, tile, plot),
            None => false,
        },
        Condition::TileResourceClass(class) => match at.tile {
            Some(tile) => seen_resource_class(auth, rules, at.owner, tile) == Some(class),
            None => false,
        },
        Condition::TileFeatureUnimproved(f) => match at.tile {
            Some(tile) => {
                let map = auth.map();
                map.feature(tile) == Some(Some(f)) && map.improvement(tile) == Some(None)
            }
            None => false,
        },
        Condition::CityAdjacentUnimprovedFeature { feature, at_least } => match at.city {
            Some(city) => adjacent_unimproved(auth, city.tile, feature) >= u32::from(at_least),
            None => false,
        },
        Condition::Producing(filter) => match at.producing {
            Some(item) => production::item_matches(auth, rules, at.owner, item, filter),
            None => false,
        },
        Condition::CityGarrisoned => match at.city {
            Some(city) => crate::world::city::garrison(auth, rules, city).is_some(),
            None => false,
        },
        Condition::CityCoastal => match at.city {
            Some(city) => auth.map().coastal_land(city.tile),
            None => false,
        },
        // The event's technology: `systems::effects` checks it when firing.
        Condition::LearnedTechnology(_) => false,
        Condition::CityOnRiver => match at.city {
            Some(city) => auth.map().river(city.tile),
            None => false,
        },
        Condition::AtPeace => at_peace(auth, at.owner),
        Condition::ConnectedToCapital => match at.city {
            Some(city) => city.capital_connection == crate::world::city::CapitalConnection::Connected,
            None => false,
        },
        Condition::Module(condition) => M::Vocabulary::condition_holds(auth, rules, at, condition),
    }
}

/// Is `tile` of the plot kind `plot` (for `owner`, who must see a resource)?
fn plot_matches<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    tile: TileId,
    plot: crate::rules::effects::PlotFilter,
) -> bool {
    use crate::rules::effects::PlotFilter;
    let map = auth.map();
    match plot {
        PlotFilter::River => map.river(tile),
        PlotFilter::Sea => map.is_water(tile) && !map.lake(tile),
        PlotFilter::Lake => map.lake(tile),
        PlotFilter::SeaResource => {
            map.is_water(tile) && seen_resource_class(auth, rules, owner, tile).is_some()
        }
        PlotFilter::NaturalWonder => map.natural_wonder(tile).is_some(),
    }
}

/// The class of the resource on `tile` if `owner` knows its reveal
/// technology ; None otherwise.
fn seen_resource_class<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    tile: TileId,
) -> Option<crate::rules::content::ResourceClass> {
    let resource = auth.map().resource(tile).flatten()?;
    let def = rules.resources.get(resource.0 as usize)?;
    if let Some(tech) = def.required_tech {
        if !auth.players().techs_of(owner).contains(&tech) {
            return None;
        }
    }
    def.class
}

/// Tiles next to `centre` with `feature` and no improvement.
fn adjacent_unimproved<M: RulesModule>(auth: &AuthWorld<M>, centre: TileId, feature: crate::ids::FeatureId) -> u32 {
    let map = auth.map();
    let mut count = 0;
    for near in map.dimensions.neighbors(centre) {
        if map.feature(near) == Some(Some(feature)) && map.improvement(near) == Some(None) {
            count += 1;
        }
    }
    count
}

/// Both gates of an effect hold (the second is Always for most effects).
pub fn effect_holds<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    at: ConditionContext<'_>,
    effect: &CompiledEffect<M>,
) -> bool {
    condition_holds(auth, rules, at, effect.condition)
        && condition_holds(auth, rules, at, effect.second_condition)
}

/// The player is at war with no major nation (city-states and
/// barbarians do not count).
pub(crate) fn at_peace<M: RulesModule>(auth: &AuthWorld<M>, player: PlayerId) -> bool {
    let players = auth.players();
    for p in 0..players.count() as u8 {
        let other = PlayerId(p);
        let major = other != player && players.is_major(other) && players.is_alive(other);
        if major && auth.diplomacy().at_war(player, other) {
            return false;
        }
    }
    true
}

/// How many times an owner-wide effect counts for the owner, given the
/// number of the owner's cities with its building: Unique once, Stack per
/// city.
fn owner_wide_times(cities: u32, stacking: Stacking) -> u32 {
    match stacking {
        Stacking::Unique => cities.min(1),
        Stacking::Stack => cities,
    }
}

/// At most this many owner-wide buildings are counted in one pass over the
/// owner's cities; more are counted one by one.
const OWNER_WIDE_COUNTED: usize = 32;

/// Number of the owner's cities holding each owner-wide building (index =
/// position in `owner_wide_buildings`), in one pass over the owner's
/// cities; None when no owner-wide building has owner-wide effects in
/// `bucket` (nothing to count).
fn owner_wide_counts<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    bucket: Bucket<M>,
) -> Option<[u32; OWNER_WIDE_COUNTED]> {
    let index = &rules.effect_index;
    let relevant = index.owner_wide_buildings.iter().any(|b| {
        let effects = bucket(&rules.buildings[b.0 as usize].effects);
        effects.iter().any(|e| e.selector.is_owner_wide())
    });
    if !relevant {
        return None;
    }
    let mut counts = [0u32; OWNER_WIDE_COUNTED];
    let Some(cities) = auth.indexes().cities_by_player.get(&owner.0) else {
        return Some(counts);
    };
    for handle in cities {
        let Some(core) = auth.cities().get(*handle) else {
            continue;
        };
        for building in &core.buildings {
            let slot = index.owner_wide_slot.get(building.0 as usize).copied();
            if let Some(slot) = slot.filter(|s| (*s as usize) < OWNER_WIDE_COUNTED) {
                counts[slot as usize] += 1;
            }
        }
    }
    Some(counts)
}

fn cities_with_building<M: RulesModule>(auth: &AuthWorld<M>, owner: PlayerId, building: BuildingKindId) -> u32 {
    let Some(cities) = auth.indexes().cities_by_player.get(&owner.0) else {
        return 0;
    };
    let mut count = 0;
    for handle in cities {
        if let Some(core) = auth.cities().get(*handle) {
            if core.buildings.contains(&building) {
                count += 1;
            }
        }
    }
    count
}

/// Visit the passive effects of `bucket` that every source owned by
/// `owner` contributes at the owner level: nation, known
/// technologies, the module's owner sources, owner-wide buildings. `visit` gets the
/// instance count.
pub fn visit_owner_sources<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    bucket: Bucket<M>,
    visit: &mut dyn FnMut(&CompiledEffect<M>, u32),
) {
    let nation = auth.players().nation_of(owner) as usize;
    if let Some(def) = rules.nations.get(nation) {
        for effect in bucket(&def.effects) {
            visit(effect, 1);
        }
    }
    for tech in auth.players().techs_of(owner) {
        if let Some(def) = rules.technologies.get(tech.0 as usize) {
            for effect in bucket(&def.effects) {
                visit(effect, 1);
            }
        }
    }
    M::Vocabulary::visit_owner_sources(auth, rules, owner, bucket, visit);
    let listed = &rules.effect_index.owner_wide_buildings;
    if listed.len() > OWNER_WIDE_COUNTED {
        // Rare: more owner-wide buildings than one pass counts.
        for building in listed {
            let cities = cities_with_building(auth, owner, *building);
            if cities > 0 {
                visit_owner_wide_building(rules, *building, cities, bucket, visit);
            }
        }
        return;
    }
    let Some(counts) = owner_wide_counts(auth, rules, owner, bucket) else {
        return;
    };
    for (building, cities) in listed.iter().zip(counts) {
        if cities > 0 {
            visit_owner_wide_building(rules, *building, cities, bucket, visit);
        }
    }
}

fn visit_owner_wide_building<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    building: BuildingKindId,
    cities: u32,
    bucket: Bucket<M>,
    visit: &mut dyn FnMut(&CompiledEffect<M>, u32),
) {
    let def = &rules.buildings[building.0 as usize];
    for effect in bucket(&def.effects) {
        if !effect.selector.is_owner_wide() {
            continue;
        }
        visit(effect, owner_wide_times(cities, effect.stacking));
    }
}
