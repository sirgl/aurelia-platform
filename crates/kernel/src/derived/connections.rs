//! City connections (Classic): the player's cities that
//! a road or railroad network links to the capital, possibly over water
//! between two cities with a harbor (`allows_water_routes`) on the same sea.
//!
//! ```text
//! land   from a reached city, over tiles with a route (city tiles count as
//!        routes) that belong to the player or to nobody; a tile of the player's own
//!        land with a feature its units walk as a road (`RoadInFriendlyLand`, the
//!        Iroquois' forests and jungles) is a route for it
//! water  a reached city with a harbor reaches every own city with a harbor
//!        that touches the same water body
//! ```

use crate::module::RulesModule;
use crate::tile_set::TileSet;

use crate::ids::{CityHandle, FeatureId, PlayerId, TileId};
use crate::rules::CompiledRuleset;
use crate::world::map::Route;
use crate::world::state::AuthWorld;

/// The player's cities connected to `capital` (the capital included).
/// Answered from the world's memo while the world does not change.
pub fn connected_cities<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    capital: CityHandle,
) -> std::sync::Arc<Vec<CityHandle>> {
    auth.memo()
        .connected_cities(player, capital, || compute_connected_cities(auth, rules, player, capital))
}

/// `connected_cities` computed from the world, never from the memo.
pub fn compute_connected_cities<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    capital: CityHandle,
) -> Vec<CityHandle> {
    let Some(start) = auth.cities().get(capital).map(|c| c.tile) else {
        return Vec::new();
    };
    let woods = crate::navigation::profile::own_land_road_features(auth, rules, player);
    let mut reached = vec![capital];
    let mut seen = TileSet::with_tile_count(auth.map().tile_count());
    let mut frontier = vec![start];
    while !frontier.is_empty() {
        for city in flood(auth, player, &woods, &mut seen, std::mem::take(&mut frontier)) {
            if !reached.contains(&city) {
                reached.push(city);
            }
        }
        for city in by_water(auth, rules, player, &reached) {
            reached.push(city);
            if let Some(core) = auth.cities().get(city) {
                frontier.push(core.tile);
            }
        }
    }
    reached
}

/// Own cities reached over routes from `start` tiles.
fn flood<M: RulesModule>(
    auth: &AuthWorld<M>,
    player: PlayerId,
    woods: &[FeatureId],
    seen: &mut TileSet,
    start: Vec<TileId>,
) -> Vec<CityHandle> {
    let map = auth.map();
    let mut found = Vec::new();
    let mut stack = start;
    while let Some(tile) = stack.pop() {
        if !seen.insert(tile) {
            continue;
        }
        if let Some(city) = auth.indexes().city_on_tile(tile) {
            if auth.cities().get(city).is_some_and(|c| c.owner == player) {
                found.push(city);
            }
        }
        for &next in map.dimensions.neighbors(tile).as_slice() {
            let owner_ok = map.owner(next).flatten().is_none_or(|o| o == player);
            let own_woods = !woods.is_empty()
                && map.owner(next).flatten() == Some(player)
                && map.feature(next).flatten().is_some_and(|f| woods.contains(&f));
            let route = map.route(next) != Route::None || own_woods || auth.indexes().city_on_tile(next).is_some();
            if owner_ok && route && !seen.contains(next) {
                stack.push(next);
            }
        }
    }
    found
}

/// Own harbor cities on a sea that a reached harbor city touches, not
/// reached yet.
fn by_water<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    reached: &[CityHandle],
) -> Vec<CityHandle> {
    let seas: Vec<u32> = reached
        .iter()
        .filter(|c| has_harbor(auth, rules, **c))
        .flat_map(|c| seas_of(auth, *c))
        .collect();
    if seas.is_empty() {
        return Vec::new();
    }
    auth.indexes()
        .cities_of(player)
        .iter()
        .copied()
        .filter(|c| !reached.contains(c) && has_harbor(auth, rules, *c))
        .filter(|c| seas_of(auth, *c).iter().any(|s| seas.contains(s)))
        .collect()
}

fn has_harbor<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, city: CityHandle) -> bool {
    auth.cities().get(city).is_some_and(|core| {
        core.buildings.iter().any(|b| {
            rules
                .buildings
                .get(b.0 as usize)
                .is_some_and(|d| d.placement.allows_water_routes)
        })
    })
}

/// Water bodies next to a city.
fn seas_of<M: RulesModule>(auth: &AuthWorld<M>, city: CityHandle) -> Vec<u32> {
    let map = auth.map();
    let Some(tile) = auth.cities().get(city).map(|c| c.tile) else {
        return Vec::new();
    };
    let mut seas: Vec<u32> = map
        .dimensions
        .neighbors(tile)
        .as_slice()
        .iter()
        .filter_map(|t| map.water_area(*t))
        .collect();
    seas.sort_unstable();
    seas.dedup();
    seas
}
