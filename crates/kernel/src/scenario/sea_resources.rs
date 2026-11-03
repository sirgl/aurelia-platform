//! Resources at sea: the deposits of the shallow water that map generation
//! lays after the land's (fish, whales, pearls, crab and sea oil in Classic).
//!
//! What a map holds is a rule of the content (`ResourceDefinition::sea_density`,
//! the percent of the shallow sea the resource takes); where it may lie and how
//! far apart is the generator's:
//!
//! | Rule | Value |
//! |---|---|
//! | shallow sea | a water tile within `SHALLOW_REACH` of land: a coast tile, a lake, the second ring of a bay |
//! | nearness | the first ring of the shore takes the density in full, the second ring half of it |
//! | never | under ice or an atoll, on a natural wonder tile |
//! | spacing | no two deposits of different resources side by side; a luxury or strategic deposit has none of its own kind beside it (fish may form pairs and fields) |
//! | rolls | a hash of the tile, the resource and the seed, never the map's random stream: the land of a map is the same with or without sea resources |
//! | coastal start | a start with the sea in its work radius and no deposit at sea there gets one of the bonus kind on the nearest shallow tile (`support_starts`), as Classic balances the start of a coastal nation |
//!
//! Integer arithmetic over the seed only, like the rest of the generator.

use super::landscape::{tile_hash, Land};
use crate::ids::{FeatureId, ResourceId, TileId};
use crate::module::RulesModule;
use crate::rules::content::{NaturalWonderId, ResourceClass};
use crate::rules::CompiledRuleset;
use crate::world::builder::WorldBuilder;

/// Hex distance from land within which water is shallow: the coast and the
/// ring behind it. A city works three rings, so the second ring of the shore
/// is the sea its work boats reach first.
pub const SHALLOW_REACH: u8 = 2;

/// Salt of the sea's rolls (the land's own salts differ).
const SALT: u64 = 0x5EA5_0000_0000_5EA5;

/// The shore distance of every tile: 0 on land, 1.. on water up to
/// `SHALLOW_REACH`, `u8::MAX` further out.
pub fn shore_distance(land: &Land) -> Vec<u8> {
    let tiles = land.tiles();
    let mut distance = vec![u8::MAX; tiles];
    let mut ring: Vec<usize> = (0..tiles).filter(|&i| !land.is_water(i)).collect();
    for &i in &ring {
        distance[i] = 0;
    }
    for step in 1..=SHALLOW_REACH {
        let mut next = Vec::new();
        for &i in &ring {
            for n in land.neighbours(i) {
                if distance[n] == u8::MAX {
                    distance[n] = step;
                    next.push(n);
                }
            }
        }
        ring = next;
    }
    distance
}

/// The sea deposits of a map by the rules' `sea_density`: tile and resource
/// in kind order, then tile order. Empty when no resource lies at sea.
pub fn place<M: RulesModule>(
    land: &Land,
    features: &[Option<FeatureId>],
    wonders: &[(TileId, NaturalWonderId)],
    rules: &CompiledRuleset<M>,
    seed: u64,
) -> Vec<(TileId, ResourceId)> {
    let kinds: Vec<usize> = (0..rules.resources.len()).filter(|&k| rules.resources[k].sea_density > 0).collect();
    if kinds.is_empty() {
        return Vec::new();
    }
    let shore = shore_distance(land);
    let mut wonder = vec![false; land.tiles()];
    for (tile, _) in wonders {
        wonder[tile.0 as usize] = true;
    }
    let open = |i: usize| land.is_water(i) && shore[i] <= SHALLOW_REACH && features[i].is_none() && !wonder[i];
    // The deposit on a tile, by kind index.
    let mut taken: Vec<Option<usize>> = vec![None; land.tiles()];
    let mut out = Vec::new();
    for &kind in &kinds {
        let def = &rules.resources[kind];
        let own_neighbour_allowed = def.class == Some(ResourceClass::Bonus);
        // Per mille of a tile of the first ring; the second ring takes half.
        let near = u64::from(def.sea_density) * 10;
        for i in 0..land.tiles() {
            if !open(i) {
                continue;
            }
            let chance = if shore[i] == 1 { near } else { near / 2 };
            if tile_hash(i as u32, SALT ^ (kind as u64).wrapping_mul(0x9E37_79B9), seed) % 1000 >= chance {
                continue;
            }
            let crowded = land.neighbours(i).any(|n| match taken[n] {
                Some(other) => other != kind || !own_neighbour_allowed,
                None => false,
            });
            if crowded {
                continue;
            }
            taken[i] = Some(kind);
            out.push((TileId(i as u32), ResourceId(kind as u16)));
        }
    }
    out
}

/// Tiles around a start that its first city works: the sea within this reach
/// is the start's own.
pub const START_REACH: i32 = 3;

/// A coastal start without a deposit at sea within `START_REACH` gets one:
/// the resource of the bonus class with the highest `sea_density` (Classic:
/// fish) on the shallow tile nearest to the start, the first ring of the
/// shore before the second. Deterministic by the seed's tile hash; a rules
/// module without a sea bonus resource leaves the map as it is.
pub fn support_starts<M: RulesModule>(game: &mut WorldBuilder<'_, M>, starts: &[TileId], seed: u64) {
    let rules = game.rules();
    let bonus = (0..rules.resources.len())
        .filter(|&k| rules.resources[k].sea_density > 0 && rules.resources[k].class == Some(ResourceClass::Bonus))
        .max_by_key(|&k| (rules.resources[k].sea_density, std::cmp::Reverse(k)));
    let Some(kind) = bonus else {
        return;
    };
    let mut added: Vec<(TileId, ResourceId)> = Vec::new();
    {
        let map = game.state().view().auth.map();
        let dims = map.dimensions;
        let tiles = dims.tile_count() as u32;
        // Is there land within `reach` tiles of a tile?
        let land_within = |tile: TileId, reach: u8| -> bool {
            let mut ring = vec![tile];
            for _ in 0..reach {
                ring = ring.iter().flat_map(|&t| dims.neighbors(t)).collect();
                if ring.iter().any(|&t| !map.is_water(t)) {
                    return true;
                }
            }
            false
        };
        for &start in starts {
            let sea: Vec<TileId> = (0..tiles)
                .map(TileId)
                .filter(|&t| map.is_water(t) && dims.distance(start, t).is_some_and(|d| d <= START_REACH))
                .collect();
            let deposit_of = |tile: TileId| -> Option<usize> {
                let placed = added.iter().find(|(t, _)| *t == tile).map(|(_, r)| r.0 as usize);
                placed.or(match map.resource(tile) {
                    Some(Some(r)) if map.is_water(tile) => Some(r.0 as usize),
                    _ => None,
                })
            };
            if sea.is_empty() || sea.iter().any(|&t| deposit_of(t).is_some()) {
                continue;
            }
            let fits = |tile: TileId| {
                map.feature(tile) == Some(None)
                    && map.natural_wonder(tile).is_none()
                    && land_within(tile, SHALLOW_REACH)
                    && !dims.neighbors(tile).into_iter().any(|n| deposit_of(n).is_some_and(|other| other != kind))
            };
            let best = sea.iter().copied().filter(|&t| fits(t)).min_by_key(|&t| {
                let ring = if land_within(t, 1) { 1u8 } else { 2 };
                (ring, dims.distance(start, t).unwrap_or(i32::MAX), tile_hash(t.0, SALT ^ 0x57A27, seed))
            });
            if let Some(tile) = best {
                added.push((tile, ResourceId(kind as u16)));
            }
        }
    }
    if !added.is_empty() {
        game.place_resources(&added);
    }
}
