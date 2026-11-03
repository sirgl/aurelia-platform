//! Melee combat and city capture: the sandbox's own formula, deterministic
//! and without dice.
//!
//! Power is strength × hit points × (100 + defence percent): the attacker's
//! plain, the defender's with the terrain's defence and, in a city, the
//! garrison bonus. Each side loses `BASE_DAMAGE` × the other's power / its
//! own, within `MIN_DAMAGE..=MAX_DAMAGE`. A city without a military unit
//! defends itself with `city_strength`; at zero hit points the attacker takes
//! it. A winner with no enemy soldier left on the tile advances into it and
//! captures what stands there (civilians die, a city changes hands). An
//! attack ends the attacker's moves.

use kernel::command::CommandError;
use kernel::ids::{CityHandle, PlayerId, TileId, UnitHandle};
use kernel::navigation::MoveProfile;
use kernel::transaction::journal::{transfer_city_tiles, CityChange, PlayerChange, UnitChange};
use kernel::transaction::WorldTransaction;
use kernel::world::city::{CityCore, CityWrite};
use kernel::world::state::AuthWorld;

use crate::{Rules, Sandbox};

/// Damage of a battle between equal powers.
pub const BASE_DAMAGE: u64 = 30;
pub const MIN_DAMAGE: u64 = 5;
pub const MAX_DAMAGE: u64 = 60;
/// Defence percent of a soldier in a city.
pub const GARRISON_PERCENT: i32 = 25;
/// Hit points of a city.
pub const CITY_MAX_HP: u16 = 100;

/// What an attack hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Defender {
    /// The strongest enemy soldier on the tile.
    Unit(UnitHandle),
    /// A city without soldiers defends itself.
    City(CityHandle),
    /// Only civilians stand there: they are taken without a fight.
    Civilians,
}

/// A validated attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battle {
    pub attacker: UnitHandle,
    pub owner: PlayerId,
    pub from: TileId,
    pub target: TileId,
    pub defender: Defender,
}

fn violation(reason: &str) -> CommandError {
    CommandError::RuleViolation(reason.into())
}

/// Validate an attack of `unit` on the adjacent tile `target`.
pub fn plan(world: &AuthWorld<Sandbox>, rules: &Rules, unit: UnitHandle, target: TileId) -> Result<Battle, CommandError> {
    let core = *world.units().get(unit).ok_or(CommandError::UnknownUnit)?.core;
    if core.owner != world.active_player() {
        return Err(violation("not your turn"));
    }
    if strength(rules, core.kind.0) == 0 {
        return Err(violation("not a soldier"));
    }
    if core.moves_left == 0 || core.attacks_made > 0 {
        return Err(violation("exhausted"));
    }
    if !world.map().dimensions.neighbors(core.tile).into_iter().any(|t| t == target) {
        return Err(CommandError::BadDestination);
    }
    let defender = defender_on(world, rules, core.owner, target).ok_or_else(|| violation("no enemy there"))?;
    Ok(Battle { attacker: unit, owner: core.owner, from: core.tile, target, defender })
}

/// Content strength of a unit kind (0: a civilian).
pub fn strength(rules: &Rules, kind: u16) -> u16 {
    let unit = rules.units.get(kind as usize);
    unit.filter(|u| !u.founder && !u.builder).map_or(0, |u| u.strength)
}

/// A city's own strength: it grows with its people.
pub fn city_strength(city: &CityCore) -> u64 {
    4 + u64::from(city.population)
}

/// An enemy of `owner` is anyone it is at war with.
fn enemy(world: &AuthWorld<Sandbox>, owner: PlayerId, other: PlayerId) -> bool {
    other != owner && world.diplomacy().at_war(owner, other)
}

/// What an attack of `owner` on `tile` hits (None: no enemy there).
pub fn defender_on(world: &AuthWorld<Sandbox>, rules: &Rules, owner: PlayerId, tile: TileId) -> Option<Defender> {
    let mut soldier: Option<(u16, u16, UnitHandle)> = None;
    let mut civilians = false;
    for &handle in world.indexes().units_on(tile) {
        let Some(view) = world.units().get(handle) else { continue };
        if !enemy(world, owner, view.core.owner) {
            return None;
        }
        let power = strength(rules, view.core.kind.0);
        match power {
            0 => civilians = true,
            _ if soldier.is_none_or(|(s, hp, _)| (power, view.core.hp) > (s, hp)) => {
                soldier = Some((power, view.core.hp, handle))
            }
            _ => {}
        }
    }
    if let Some((_, _, unit)) = soldier {
        return Some(Defender::Unit(unit));
    }
    if let Some(city) = world.indexes().city_on_tile(tile) {
        let city_owner = world.cities().get(city)?.owner;
        return enemy(world, owner, city_owner).then_some(Defender::City(city));
    }
    civilians.then_some(Defender::Civilians)
}

/// Resolve a validated battle.
pub fn fight(tx: &mut WorldTransaction<'_, Sandbox>, rules: &Rules, battle: Battle) -> Result<(), CommandError> {
    let attacker = *tx.world.units.get(battle.attacker).ok_or(CommandError::UnknownUnit)?.core;
    let attack = power(u64::from(strength(rules, attacker.kind.0)), attacker.hp, 0);
    let defence = match battle.defender {
        Defender::Unit(unit) => {
            let core = *tx.world.units.get(unit).ok_or(CommandError::UnknownUnit)?.core;
            let percent = terrain_defence(tx.world, rules, battle.target) + garrison_percent(tx.world, battle.target);
            power(u64::from(strength(rules, core.kind.0)), core.hp, percent)
        }
        Defender::City(city) => {
            let core = tx.world.cities.get(city).ok_or(CommandError::UnknownCity)?;
            power(city_strength(core), CITY_MAX_HP.saturating_sub(core.damage), 0)
        }
        Defender::Civilians => 0,
    };
    let (dealt, taken) = match battle.defender {
        Defender::Civilians => (0, 0),
        _ => (damage(attack, defence), damage(defence, attack)),
    };
    let fell = match battle.defender {
        Defender::Unit(unit) => wound(tx, unit, dealt)?,
        Defender::City(city) => batter(tx, city, dealt)?,
        Defender::Civilians => true,
    };
    if wound(tx, battle.attacker, taken)? {
        return Ok(());
    }
    if fell && no_soldier_left(tx.world, rules, battle) {
        advance(tx, rules, battle)?;
    }
    if let Some(core) = tx.world.units.get_mut_core(battle.attacker) {
        core.moves_left = 0;
        core.attacks_made += 1;
    }
    Ok(())
}

fn power(strength: u64, hp: u16, percent: i32) -> u64 {
    strength * u64::from(hp) * (100 + percent.max(-90)) as u64
}

/// Damage a side of power `from` deals to a side of power `to`.
fn damage(from: u64, to: u64) -> u16 {
    let raw = BASE_DAMAGE * from / to.max(1);
    raw.clamp(MIN_DAMAGE, MAX_DAMAGE) as u16
}

fn terrain_defence(world: &AuthWorld<Sandbox>, rules: &Rules, tile: TileId) -> i32 {
    let kind = world.map().terrain(tile).and_then(|code| rules.terrain_kind_of(code));
    kind.and_then(|k| rules.terrains.get(k.0 as usize)).map_or(0, |t| t.defense_pct)
}

fn garrison_percent(world: &AuthWorld<Sandbox>, tile: TileId) -> i32 {
    match world.indexes().city_on_tile(tile) {
        Some(_) => GARRISON_PERCENT,
        None => 0,
    }
}

/// The unit loses `damage` hit points; true when it died.
fn wound(tx: &mut WorldTransaction<'_, Sandbox>, unit: UnitHandle, damage: u16) -> Result<bool, CommandError> {
    let core = tx.world.units.get_mut_core(unit).ok_or(CommandError::UnknownUnit)?;
    if damage >= core.hp {
        tx.destroy_unit(unit)?;
        return Ok(true);
    }
    if damage > 0 {
        core.hp -= damage;
        tx.journal.unit_changes.push(UnitChange::Damaged { unit, damage });
    }
    Ok(false)
}

/// The city takes `damage`; true when its hit points are gone.
fn batter(tx: &mut WorldTransaction<'_, Sandbox>, city: CityHandle, damage: u16) -> Result<bool, CommandError> {
    let core = tx.world.cities.get_mut(city).ok_or(CommandError::UnknownCity)?;
    core.damage = (core.damage + damage).min(CITY_MAX_HP);
    Ok(core.damage >= CITY_MAX_HP)
}

/// No enemy soldier is left on the target (another may stand behind the
/// one that fell).
fn no_soldier_left(world: &AuthWorld<Sandbox>, rules: &Rules, battle: Battle) -> bool {
    !matches!(defender_on(world, rules, battle.owner, battle.target), Some(Defender::Unit(_)))
}

/// The winner takes the tile: civilians there die, a city changes hands,
/// then the attacker steps in.
fn advance(tx: &mut WorldTransaction<'_, Sandbox>, rules: &Rules, battle: Battle) -> Result<(), CommandError> {
    let mut civilians: Vec<UnitHandle> = tx.world.indexes.units_on(battle.target).to_vec();
    civilians.sort_by_key(|u| tx.world.units.get(*u).map(|v| v.core.id));
    for unit in civilians {
        tx.destroy_unit(unit)?;
    }
    if let Some(city) = tx.world.indexes.city_on_tile(battle.target) {
        capture(tx, rules, city, battle.owner)?;
    }
    let core = *tx.world.units.get(battle.attacker).ok_or(CommandError::UnknownUnit)?.core;
    let profile = MoveProfile::of_unit(tx.world, rules, &core);
    tx.move_unit(battle.attacker, battle.from, battle.target, 1, &profile, rules)
}

/// The conqueror takes the city and its tiles; it loses a citizen, its queue
/// and its seat of government (a captured capital is nobody's capital).
pub fn capture(tx: &mut WorldTransaction<'_, Sandbox>, rules: &Rules, city: CityHandle, conqueror: PlayerId) -> Result<(), CommandError> {
    let round = tx.world.turn.round;
    let (from, tile) = {
        let core = tx.world.cities.get(city).ok_or(CommandError::UnknownCity)?;
        (core.owner, core.tile)
    };
    if let CityWrite::TargetGone | CityWrite::QueueFull = tx.world.cities.set_owner(city, conqueror) {
        return Err(CommandError::UnknownCity);
    }
    let seat = kernel::systems::city_founding::palace(rules);
    let core = tx.world.cities.get_mut(city).ok_or(CommandError::UnknownCity)?;
    core.acquired_round = round;
    core.population = core.population.saturating_sub(1).max(1);
    core.damage = CITY_MAX_HP / 2;
    core.queue.clear();
    core.production = 0;
    core.buildings.retain(|b| Some(*b) != seat);
    tx.world.indexes.unindex_city_owner(from, city);
    tx.world.indexes.index_city(conqueror, tile, city);
    tx.journal.city_changes.push(CityChange::OwnerChanged { city, from, to: conqueror });
    transfer_city_tiles(tx.world, &mut tx.journal, city);
    tx.journal.city_changes.push(CityChange::PopulationChanged { city });
    for player in [from, conqueror] {
        tx.journal.player_changes.push(PlayerChange::EffectsChanged { player });
    }
    Ok(())
}
