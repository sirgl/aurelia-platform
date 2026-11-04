//! The writes of world construction: what lays terrain, rivers, features,
//! deposits, improvements, routes, cities and units into a world without
//! playing to it. One definition each, shared by the two callers that may
//! write outside `execute`:
//!
//! - [`WorldBuilder`](super::builder::WorldBuilder), which builds a world
//!   before the game starts and commits the land layers it wrote in ONE
//!   journal (one derived rebuild);
//! - the test harness (`GameState::debug_*`, behind `cfg(any(test,
//!   feature = "harness"))`), which changes an existing game and commits
//!   the journal of the change at once.
//!
//! Every function writes the world and records what it changed in the
//! journal it is given; none commits. Placement rules are not checked: the
//! constructor decides what stands where.

use crate::ids::{CityHandle, FeatureId, PlayerId, ResourceId, TerrainId, TileId, UnitHandle, UnitKindId};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{ChangeJournal, CityChange, TileChange};
use crate::world::map::Route;
use crate::world::state::AuthWorld;

/// Terrain of tiles; the water areas are drawn again once for the batch.
pub(crate) fn set_terrain<M: RulesModule>(auth: &mut AuthWorld<M>, journal: &mut ChangeJournal<M>, tiles: &[(TileId, TerrainId)]) {
    for (tile, terrain) in tiles {
        auth.map.set_terrain(*tile, *terrain);
        journal.tile_changes.push(TileChange::TerrainSet { tile: *tile });
    }
    auth.map.rebuild_water_areas();
}

/// Rivers on tile edges. The tile cache is built inside `set_river_edges`.
/// The river flag is for land only: a river mouth touches water, but water is
/// not a river (the invariant "rivers never on water").
pub(crate) fn set_river_edges<M: RulesModule>(auth: &mut AuthWorld<M>, journal: &mut ChangeJournal<M>, edges: &[(TileId, TileId)]) {
    auth.map.set_river_edges(edges);
    let mut seen: Vec<TileId> = Vec::new();
    for (a, b) in edges {
        for t in [*a, *b] {
            if seen.contains(&t) {
                continue;
            }
            seen.push(t);
            if auth.map.terrain(t).is_some_and(|c| auth.map.terrain_kinds().is_water(c.0)) {
                continue;
            }
            journal.tile_changes.push(TileChange::RiverSet { tile: t });
        }
    }
}

/// Tiles marked as river tiles WITHOUT a bed: gameplay tests of fords and farms
/// need the flag, not the edges (the renderer draws no river for them).
#[cfg(any(test, feature = "harness"))]
pub(crate) fn set_river_flags<M: RulesModule>(auth: &mut AuthWorld<M>, journal: &mut ChangeJournal<M>, tiles: &[TileId]) {
    for tile in tiles {
        auth.map.set_river(*tile, true);
        journal.tile_changes.push(TileChange::RiverSet { tile: *tile });
    }
}

pub(crate) fn set_features<M: RulesModule>(auth: &mut AuthWorld<M>, journal: &mut ChangeJournal<M>, tiles: &[(TileId, FeatureId)]) {
    for (tile, feature) in tiles {
        auth.map.set_feature(*tile, Some(*feature));
        journal.tile_changes.push(TileChange::FeatureSet { tile: *tile });
    }
}

pub(crate) fn clear_features<M: RulesModule>(auth: &mut AuthWorld<M>, journal: &mut ChangeJournal<M>, tiles: &[TileId]) {
    for tile in tiles {
        auth.map.set_feature(*tile, None);
        journal.tile_changes.push(TileChange::FeatureSet { tile: *tile });
    }
}

/// Deposits of the amount each tile picks (`scenario::deposit_amount`).
pub(crate) fn place_resources<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    rules: &CompiledRuleset<M>,
    tiles: &[(TileId, ResourceId)],
) {
    for (tile, resource) in tiles {
        let amount = crate::scenario::deposit_amount(rules, *resource, *tile);
        auth.map.set_resource_amount(*tile, Some(*resource), amount);
        journal.tile_changes.push(TileChange::ResourceSet { tile: *tile });
    }
}

/// Deposits of the amount each entry names.
pub(crate) fn place_deposits<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    tiles: &[(TileId, ResourceId, u8)],
) {
    for (tile, resource, amount) in tiles {
        auth.map.set_resource_amount(*tile, Some(*resource), *amount);
        journal.tile_changes.push(TileChange::ResourceSet { tile: *tile });
    }
}

/// A deposit of `amount` copies on a tile.
#[cfg(any(test, feature = "harness"))]
pub(crate) fn set_deposit<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    tile: TileId,
    resource: ResourceId,
    amount: u8,
) {
    auth.map.set_resource_amount(tile, Some(resource), amount);
    journal.tile_changes.push(TileChange::ResourceSet { tile });
}

pub(crate) fn set_improvements<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    tiles: &[(TileId, crate::ids::ImprovementId)],
) {
    for (tile, improvement) in tiles {
        auth.map.set_improvement(*tile, Some(*improvement));
        journal.tile_changes.push(TileChange::ImprovementBuilt { tile: *tile });
    }
}

pub(crate) fn set_routes<M: RulesModule>(auth: &mut AuthWorld<M>, journal: &mut ChangeJournal<M>, tiles: &[(TileId, Route)]) {
    for (tile, route) in tiles {
        auth.map.set_route(*tile, *route);
        journal.tile_changes.push(TileChange::RoadBuilt { tile: *tile });
    }
}

#[cfg(any(test, feature = "harness"))]
pub(crate) fn set_roads<M: RulesModule>(auth: &mut AuthWorld<M>, journal: &mut ChangeJournal<M>, tiles: &[TileId]) {
    for tile in tiles {
        auth.map.set_route(*tile, Route::Road);
        journal.tile_changes.push(TileChange::RoadBuilt { tile: *tile });
    }
}

pub(crate) fn set_natural_wonders<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    tiles: &[(TileId, crate::rules::content::NaturalWonderId)],
) {
    for (tile, wonder) in tiles {
        auth.map.set_natural_wonder(*tile, Some(*wonder));
        journal.tile_changes.push(TileChange::NaturalWonderSet { tile: *tile });
    }
}

/// A city of `owner` on `tile` with its founding ring and, for a first city,
/// the Palace (no settler, no module reaction).
pub(crate) fn found_city<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    tile: TileId,
) -> CityHandle {
    assert!(auth.indexes.city_on_tile(tile).is_none(), "found_city: one city per tile");
    let name = crate::world::city::next_city_name(auth, rules, owner);
    let id = auth.allocate_entity();
    let first = auth.indexes.cities_of(owner).is_empty() && auth.players.is_major(owner);
    let round = auth.turn.round;
    let city = auth.cities.insert(crate::world::city::CityCore::founded(id, tile, owner, name, first, round));
    auth.indexes.index_city(owner, tile, city);
    journal.city_changes.push(CityChange::Founded(city));
    // The same ring claim as the command's `found_city`, through the shared helper
    // (two paths, one meaning; `found_city_claims_ring` catches a drift).
    let radius = crate::derived::yields::CITY_TERRITORY_RADIUS;
    crate::transaction::journal::claim_founding_ring(auth, journal, city, radius);
    crate::systems::city_founding::place_palace(auth, journal, rules, owner, city);
    city
}

/// A unit of `kind` for `owner` on `tile`, with the move points and free
/// promotions a trained unit gets. The unit's own journal is returned.
pub(crate) fn spawn_unit<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    tile: TileId,
    kind: UnitKindId,
) -> (UnitHandle, ChangeJournal<M>) {
    let mut tx = crate::transaction::WorldTransaction::new(auth);
    let unit = tx.spawn_unit(owner, tile, kind, rules);
    (unit, tx.commit())
}

/// A unit removed outside a command (a scene cleared for a training
/// ground): `WorldTransaction::destroy_unit`, its journal entry committed.
pub(crate) fn remove_unit<M: RulesModule>(auth: &mut AuthWorld<M>, unit: UnitHandle) -> ((), ChangeJournal<M>) {
    let mut tx = crate::transaction::WorldTransaction::new(auth);
    let _ = tx.destroy_unit(unit);
    ((), tx.commit())
}

/// A building in a city, through the journal (the modifier path of yields).
pub(crate) fn add_building<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    city: CityHandle,
    building: crate::ids::BuildingKindId,
) {
    match auth.cities.add_building(city, building) {
        crate::world::city::CityWrite::Applied => {}
        crate::world::city::CityWrite::TargetGone => panic!("add_building: city handle is stale"),
        crate::world::city::CityWrite::QueueFull => unreachable!("add_building never queues"),
    }
    journal.city_changes.push(CityChange::BuildingAdded { city, building });
}

/// A technology known from the start (free techs of a nation). Not
/// research: no science spent, no trigger fired. Journals only a new one.
pub(crate) fn grant_tech<M: RulesModule>(auth: &mut AuthWorld<M>, journal: &mut ChangeJournal<M>, p: PlayerId, tech: crate::ids::TechId) {
    if auth.players.learn_tech(p, tech) == crate::world::player::LearnOutcome::Learned {
        journal.player_changes.push(crate::transaction::journal::PlayerChange::TechnologyChosen { player: p });
    }
}

/// A player's kind; a kind always at war is at war with everyone.
pub(crate) fn set_player_kind<M: RulesModule>(auth: &mut AuthWorld<M>, player: PlayerId, kind: crate::world::player::PlayerKind<M>) {
    use crate::world::player::ModulePlayerKind;
    auth.players.set_kind(player, kind);
    if kind.module().is_some_and(|k| k.always_at_war()) {
        for p in 0..auth.players.count() {
            let other = PlayerId(p as u8);
            if other != player {
                auth.diplomacy.set_war(player, other, true);
            }
        }
    }
}

/// Difficulty and the players the AI plays (read by happiness and the AI handicap).
pub(crate) fn configure_players<M: RulesModule>(auth: &mut AuthWorld<M>, difficulty: &str, ai: &[PlayerId]) {
    let count = auth.players.count();
    // The speed is chosen apart (`set_speed`) and kept.
    auth.settings.difficulty = difficulty.to_owned();
    auth.settings.ai = (0..count).map(|p| ai.contains(&PlayerId(p as u8))).collect();
}

/// The game's speed (a speed key of the rules; Standard and unknown keys are kept as empty).
pub(crate) fn set_speed<M: RulesModule>(auth: &mut AuthWorld<M>, rules: &CompiledRuleset<M>, speed: &str) {
    auth.settings.speed = crate::world::game_settings::GameSettings::speed_key(rules, speed);
    auth.settings.speed_values = crate::world::game_settings::GameSettings::values_of(rules, &auth.settings.speed);
}
