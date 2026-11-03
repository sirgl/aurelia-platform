//! Debug/test validator (§26). May be expensive.

use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::world::module::ModuleWorld;
use crate::world::state::GameState;

#[derive(Debug, Clone)]
pub enum ValidationError {
    DanglingUnitIndex,
    DanglingCityIndex,
    IndexMismatch(String),
    /// The rules module's state breaks one of its invariants.
    Module(String),
}

pub(crate) fn validate_world<M: RulesModule>(state: &GameState<M>, rules: &CompiledRuleset<M>) -> Result<(), ValidationError> {
    // Check that all indexed handles resolve in the stores.
    let view = state.view();
    for h in view
        .auth
        .indexes()
        .units_by_player
        .values()
        .flat_map(|v| v.iter())
    {
        if view.auth.units().get(*h).is_none() {
            return Err(ValidationError::DanglingUnitIndex);
        }
    }
    for h in view
        .auth
        .indexes()
        .cities_by_player
        .values()
        .flat_map(|v| v.iter())
    {
        if view.auth.cities().get(*h).is_none() {
            return Err(ValidationError::DanglingCityIndex);
        }
    }
    validate_entity_order(state)?;
    // One city per tile: every city is reachable through its tile.
    if view.auth.indexes().city_by_tile.len() != view.auth.cities().len() {
        return Err(ValidationError::IndexMismatch(format!(
            "{} cities but {} city tiles",
            view.auth.cities().len(),
            view.auth.indexes().city_by_tile.len()
        )));
    }
    validate_tile_cities(state)?;
    for (tile, h) in view.auth.indexes().city_by_tile.iter() {
        let Some(core) = view.auth.cities().get(*h) else {
            return Err(ValidationError::DanglingCityIndex);
        };
        if core.tile.0 != *tile {
            return Err(ValidationError::IndexMismatch(format!(
                "city_by_tile {} points at city on {}",
                tile, core.tile.0
            )));
        }
    }
    view.auth.module().validate(view.auth, rules)
}

/// Units and cities iterate in ascending entity id below `next_entity`, and
/// the per-player indexes list exactly the owner's entities in that order
/// (the canonical order a loaded world reproduces). The tile index holds
/// every unit once, on its tile.
fn validate_entity_order<M: RulesModule>(state: &GameState<M>) -> Result<(), ValidationError> {
    let auth = state.view().auth;
    let next = auth.next_entity();
    let mismatch = |what: String| Err(ValidationError::IndexMismatch(what));
    let unit_ids: Vec<_> = auth.units().iter_cores().map(|u| u.id).collect();
    let city_ids: Vec<_> = auth.cities().iter().map(|(_, c)| c.id).collect();
    for ids in [&unit_ids, &city_ids] {
        if ids.windows(2).any(|w| w[0] >= w[1]) || ids.last().is_some_and(|id| *id >= next) {
            return mismatch(format!("entity ids out of order: {ids:?}"));
        }
    }
    for p in 0..auth.players().count() {
        let player = crate::ids::PlayerId(p as u8);
        let units: Vec<_> = auth
            .units()
            .iter_handles()
            .filter(|(_, core)| core.owner == player)
            .map(|(handle, _)| handle)
            .collect();
        if auth.indexes().units_of(player) != units.as_slice() {
            return mismatch(format!("units_of({p}) is not the owner's units in entity order"));
        }
        let cities: Vec<_> = auth
            .cities()
            .iter()
            .filter(|(_, core)| core.owner == player)
            .map(|(handle, _)| handle)
            .collect();
        if auth.indexes().cities_of(player) != cities.as_slice() {
            return mismatch(format!("cities_of({p}) is not the owner's cities in entity order"));
        }
    }
    let indexed: usize = auth.indexes().unit_by_tile.values().map(Vec::len).sum();
    let all_on_their_tiles = auth
        .units()
        .iter_handles()
        .all(|(handle, core)| auth.indexes().units_on(core.tile).contains(&handle));
    if indexed != auth.units().len() || !all_on_their_tiles {
        return mismatch("unit tile index differs from unit positions".into());
    }
    Ok(())
}

/// Every owned tile belongs to one live city of its owner, every unowned
/// tile to none, and every city owns its own tile.
fn validate_tile_cities<M: RulesModule>(state: &GameState<M>) -> Result<(), ValidationError> {
    let auth = state.view().auth;
    let map = auth.map();
    for i in 0..map.tile_count() {
        let tile = crate::ids::TileId(i as u32);
        let owner = map.owner(tile).flatten();
        let city_owner = map
            .owning_city(tile)
            .map(|city| auth.cities().get(city).map(|core| core.owner));
        let consistent = match (owner, city_owner) {
            (None, None) => true,
            (Some(owner), Some(Some(city_owner))) => owner == city_owner,
            _ => false,
        };
        if !consistent {
            return Err(ValidationError::IndexMismatch(format!(
                "tile {i}: owner {owner:?}, owning city {city_owner:?}"
            )));
        }
    }
    for (handle, core) in auth.cities().iter() {
        if map.owning_city(core.tile) != Some(handle) {
            return Err(ValidationError::IndexMismatch(format!(
                "city {handle:?} does not own its tile"
            )));
        }
    }
    Ok(())
}
