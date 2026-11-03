//! Content checks of an untrusted save (§26): every content id names an
//! entry of the rules, every value is one the game can produce, and the
//! invariants the live world keeps hold. Decode runs these after the
//! structural checks (lengths, tiles, players); a failure is Corrupt.
//! Accepting garbage would let it reach the turn code and the UI, which
//! index content tables by these ids.

use crate::module::RulesModule;
use crate::persistence::decode::DecodeError;
use crate::persistence::schema::{SavedCity, SavedUnit, SavedWorldV1};
use crate::rules::tables::ModuleTables;
use crate::rules::CompiledRuleset;
use crate::world::city::CULTURE_RADIUS_MAX;
use crate::world::unit::UNIT_MAX_HP;

/// Largest city population a save may hold (the turn code multiplies it;
/// a real game stays far below).
pub const MAX_CITY_POPULATION: u32 = 1_000;

fn corrupt(msg: impl Into<String>) -> DecodeError {
    DecodeError::Corrupt(msg.into())
}

pub(crate) fn check_content<M: RulesModule>(w: &SavedWorldV1<M>, rules: &CompiledRuleset<M>) -> Result<(), DecodeError> {
    check_map_layers(w, rules)?;
    for (i, unit) in w.units.iter().enumerate() {
        check_unit(unit, rules, w.terrain.len())
            .map_err(|what| corrupt(format!("unit[{i}] {what}")))?;
    }
    for (i, city) in w.cities.iter().enumerate() {
        check_city(city, rules, w.terrain.len())
            .map_err(|what| corrupt(format!("city[{i}] {what}")))?;
    }
    check_city_tiles(w)?;
    check_players(w, rules)?;
    check_flags(w)
}

fn check_map_layers<M: RulesModule>(w: &SavedWorldV1<M>, rules: &CompiledRuleset<M>) -> Result<(), DecodeError> {
    let kinds = rules.terrain_kinds;
    if w.terrain.iter().any(|t| !kinds.knows(*t)) {
        return Err(corrupt("terrain id"));
    }
    let outside =
        |ids: &[Option<u16>], len: usize| ids.iter().flatten().any(|id| *id as usize >= len);
    if outside(&w.feature, rules.features.len()) {
        return Err(corrupt("feature id"));
    }
    if outside(&w.resource, rules.resources.len()) {
        return Err(corrupt("resource id"));
    }
    if outside(&w.improvement, rules.improvements.len()) {
        return Err(corrupt("improvement id"));
    }
    let pillaged_ok = w.pillaged.iter().all(|t| w.improvement.get(*t as usize).is_some_and(|i| i.is_some()));
    if !pillaged_ok || !w.pillaged.windows(2).all(|p| p[0] < p[1]) {
        return Err(corrupt("pillaged tile"));
    }
    if w.road.iter().any(|code| crate::world::map::Route::from_code(*code).is_none()) {
        return Err(corrupt("route code"));
    }
    if w.river.iter().any(|flag| *flag > 1) {
        return Err(corrupt("river flag"));
    }
    if outside(&w.natural_wonder, rules.natural_wonders.len()) {
        return Err(corrupt("natural wonder id"));
    }
    Ok(())
}

fn check_unit<M: RulesModule>(unit: &SavedUnit<M>, rules: &CompiledRuleset<M>, tiles: usize) -> Result<(), &'static str> {
    // A kind outside the table is a debug unit (rules::compiled
    // DEFAULT_STRENGTH / MOVEMENT_DEFAULT): legal in harness worlds.
    if unit.hp == 0 || unit.hp > UNIT_MAX_HP {
        return Err("hp");
    }
    if unit.moves_left > unit.moves_max {
        return Err("moves");
    }
    if unit.goal.is_some_and(|goal| goal as usize >= tiles) {
        return Err("goal tile");
    }
    if unit.level == 0 {
        return Err("level");
    }
    let Some((tile, tag, id, _)) = unit.work else {
        return Ok(());
    };
    if tile as usize >= tiles {
        return Err("work tile");
    }
    let known = match tag {
        0 => (id as usize) < rules.improvements.len(),
        1 => id == 0,
        2 => (id as usize) < rules.features.len(),
        _ => false,
    };
    if known {
        Ok(())
    } else {
        Err("work target")
    }
}

/// Bound of a saved city's damage (garbage guard, not a rule).
const MAX_CITY_DAMAGE: u16 = 10_000;

fn check_city<M: RulesModule>(city: &SavedCity, rules: &CompiledRuleset<M>, tiles: usize) -> Result<(), &'static str> {
    if city.population == 0 || city.population > MAX_CITY_POPULATION {
        return Err("population");
    }
    // Damage never exceeds the maximum hit points: 200 plus the buildings'
    // extra hit points (a few hundred); the bound only rejects garbage.
    if city.damage > MAX_CITY_DAMAGE || city.radius == 0 || city.radius > CULTURE_RADIUS_MAX {
        return Err("damage/radius");
    }
    let mut buildings = city.buildings.clone();
    buildings.sort_unstable();
    buildings.dedup();
    let unknown = city
        .buildings
        .iter()
        .any(|b| *b as usize >= rules.buildings.len());
    if unknown || buildings.len() != city.buildings.len() {
        return Err("buildings");
    }
    for (tag, id) in &city.queue {
        let known = match tag {
            0 => (*id as usize) < rules.units.len(),
            1 => (*id as usize) < rules.buildings.len(),
            // A project of the module's tables, as `decode` reads it.
            2 => rules.module.project(crate::rules::content::ProjectId(*id)).is_some(),
            _ => false,
        };
        if !known {
            return Err("queue");
        }
    }
    if city.queue.len() > crate::world::city::MAX_QUEUE {
        return Err("queue length");
    }
    if city.unworked.iter().any(|t| *t as usize >= tiles) {
        return Err("unworked tile");
    }
    Ok(())
}

/// One city per tile.
fn check_city_tiles<M: RulesModule>(w: &SavedWorldV1<M>) -> Result<(), DecodeError> {
    let mut tiles: Vec<u32> = w.cities.iter().map(|c| c.tile).collect();
    tiles.sort_unstable();
    if tiles.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(corrupt("two cities on one tile"));
    }
    Ok(())
}

/// Per-player vectors are complete, nations known, and an eliminated
/// player owns nothing (a player with nothing may still be alive: a world
/// in setup has not eliminated anybody yet).
fn check_players<M: RulesModule>(w: &SavedWorldV1<M>, rules: &CompiledRuleset<M>) -> Result<(), DecodeError> {
    let players = w.players as usize;
    let lengths = [
        w.player_gold.len(),
        w.player_science.len(),
        w.player_research.len(),
        w.player_nations.len(),
    ];
    if lengths.iter().any(|len| *len != players) {
        return Err(corrupt("player vectors size"));
    }
    let nation_known =
        |nation: u8| (nation as usize) < rules.nations.len() || (rules.nations.is_empty() && nation == 0);
    if !w.player_nations.iter().all(|nation| nation_known(*nation)) {
        return Err(corrupt("player nation"));
    }
    for p in 0..players {
        let has_unit = w.units.iter().any(|u| u.owner as usize == p);
        let has_city = w.cities.iter().any(|c| c.owner as usize == p);
        if !w.players_alive[p] && (has_unit || has_city) {
            return Err(corrupt(format!("eliminated player {p} owns entities")));
        }
    }
    Ok(())
}

/// Boolean layers hold 0/1 only; nobody is at war with itself.
fn check_flags<M: RulesModule>(w: &SavedWorldV1<M>) -> Result<(), DecodeError> {
    let players = w.players as usize;
    let flags = w.diplomacy.iter().chain(&w.diplomacy_open);
    if flags.copied().any(|v| v > 1) {
        return Err(corrupt("diplomacy value"));
    }
    if (0..players).any(|p| w.diplomacy[p * players + p] != 0) {
        return Err(corrupt("diplomacy with self"));
    }
    if w.knowledge_explored.iter().flatten().any(|v| *v > 1) {
        return Err(corrupt("knowledge value"));
    }
    Ok(())
}
