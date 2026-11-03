//! The tile as movement and combat see it: water and deep water, hills,
//! mountains, rough ground, the terrain kind of a map tile.
//!
//! The map stores one code per terrain (`navigation::cost`); water has one
//! code. Deep water is a water tile with no land among its neighbours,
//! shallow water the rest (the `deep_water` and `Ground::Water` roles). The
//! terrain table (`rules.terrains`) is linked to the map codes by
//! `map_code`.

use crate::module::RulesModule;
use crate::ids::TileId;
use crate::rules::content::TerrainKindId;
use crate::rules::roles::Ground;
use crate::rules::CompiledRuleset;
use crate::world::map::MapState;

pub fn is_water(map: &MapState, tile: TileId) -> bool {
    map.terrain(tile).is_some_and(|t| map.terrain_kinds().is_water(t.0))
}

/// Water with no land next to it (Classic ocean).
pub(crate) fn is_deep_water(map: &MapState, tile: TileId) -> bool {
    if !is_water(map, tile) {
        return false;
    }
    let around = map.dimensions.neighbors(tile);
    around.into_iter().all(|t| is_water(map, t))
}

pub fn is_hills(map: &MapState, tile: TileId) -> bool {
    map.terrain(tile).is_some_and(|t| map.terrain_kinds().is_hills(t.0))
}

pub fn is_mountain(map: &MapState, tile: TileId) -> bool {
    map.terrain(tile).is_some_and(|t| map.terrain_kinds().is_mountains(t.0))
}

/// Hills, mountains, a rough feature (forest, jungle) or a rough natural
/// wonder. Everything else, water included, is
/// open ground.
pub fn is_rough<M: RulesModule>(rules: &CompiledRuleset<M>, map: &MapState, tile: TileId) -> bool {
    if is_hills(map, tile) || is_mountain(map, tile) {
        return true;
    }
    let wonder = map.natural_wonder(tile).and_then(|w| rules.natural_wonders.get(w.0 as usize));
    if let Some(wonder) = wonder {
        return wonder.rough;
    }
    match map.feature(tile).flatten() {
        Some(feature) => rules
            .features
            .get(feature.0 as usize)
            .is_some_and(|def| def.rough),
        None => false,
    }
}

/// The terrain kind of a map tile: the deep or shallow water role for
/// water (by depth), else the terrain whose `map_code` is the tile's code.
pub fn terrain_kind<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    map: &MapState,
    tile: TileId,
) -> Option<TerrainKindId> {
    let code = map.terrain(tile)?;
    if !map.terrain_kinds().is_water(code.0) {
        return rules.terrain_kind_of(code);
    }
    if is_deep_water(map, tile) {
        rules.roles.deep_water
    } else {
        rules.roles.ground(Ground::Water)
    }
}
