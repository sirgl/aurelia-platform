//! Save section of the economy and society systems: city citizen orders
//! and stocks (the module's state of each city among the kernel's:
//! `world::module::ModuleCity`), and the players' records (the module's
//! state of each player with the kernel's fields:
//! `world::module::ModulePlayer`). One section in the save
//! (`SavedWorldV1::economy`), one encode, one restore, one hash fold (the
//! rules module mixes its own part last in it).

use crate::rules::tables::ModuleTables;
use crate::module::RulesModule;
use crate::ids::{PlayerId, TileId};
use crate::persistence::decode::DecodeError;
use crate::rules::content::SpecialistId;
use crate::rules::CompiledRuleset;
use crate::world::citizens::{CitizenPlan, CityFocus, AVOID_GROWTH_BIT};
use crate::world::city_economy::CityEconomy;
use crate::world::module::{
    HashPoint, KernelPlayerRecord, ModuleCity, ModuleCityOf, ModulePlayer, ModulePlayerOf, ModuleWorld,
    SavedCityOf, SavedPlayerOf,
};
use crate::world::state::AuthWorld;

/// Citizen orders and stocks of one city (cities in save order).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedCityEconomy<M: RulesModule> {
    pub focus: u8,
    /// Locked tiles, ascending.
    pub locked: Vec<u32>,
    /// Hand-assigned specialists (kind, count), ascending kind.
    pub specialists: Vec<(u16, u8)>,
    pub food_kept: u32,
    /// The module's state of the city (`ModuleCity::Saved`).
    pub module: SavedCityOf<M>,
    /// `Process::code`.
    pub process: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SavedEconomy<M: RulesModule> {
    pub cities: Vec<SavedCityEconomy<M>>,
    /// The players' records (the module's, with the kernel's fields:
    /// `ModulePlayer::Saved`), in id order.
    pub players: Vec<SavedPlayerOf<M>>,
    /// The game's difficulty (handicap key; empty: the default level).
    pub difficulty: String,
    /// The game's speed (speed key; empty: Standard). Written from the
    /// module's `ModuleSections::SPEED_SINCE`; older saves read Standard.
    pub speed: String,
}

fn corrupt(msg: impl Into<String>) -> DecodeError {
    DecodeError::Corrupt(msg.into())
}

pub fn encode<M: RulesModule>(auth: &AuthWorld<M>) -> SavedEconomy<M> {
    let cities = auth
        .cities()
        .iter()
        .map(|(handle, core)| {
            let module = auth.cities().module_of(handle).map(ModuleCity::encode).unwrap_or_default();
            encode_city(&core.citizens, &core.economy, module)
        })
        .collect();
    let players = (0..auth.players().count())
        .map(|i| {
            let player = PlayerId(i as u8);
            let kernel = KernelPlayerRecord {
                plots_bought: auth.players().plots_bought(player),
                ai: auth.settings().is_ai(player),
            };
            let state = auth.players().economy_of(player).cloned().unwrap_or_default();
            state.encode(kernel)
        })
        .collect();
    SavedEconomy {
        cities,
        players,
        difficulty: auth.settings().difficulty.clone(),
        speed: auth.settings().speed.clone(),
    }
}

fn encode_city<M: RulesModule>(plan: &CitizenPlan, economy: &CityEconomy, module: SavedCityOf<M>) -> SavedCityEconomy<M> {
    SavedCityEconomy {
        focus: plan.focus.code() | if plan.avoid_growth { AVOID_GROWTH_BIT } else { 0 },
        locked: plan.locked.iter().map(|t| t.0).collect(),
        specialists: plan.specialists.iter().map(|(k, n)| (k.0, *n)).collect(),
        food_kept: economy.food_kept,
        module,
        process: crate::world::city_economy::Process::code(economy.process),
    }
}

/// Put the saved section into a restored world (cities and players are
/// already there, in save order). Untrusted input: errors, no panics.
pub(crate) fn restore<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    saved: &SavedEconomy<M>,
    rules: &CompiledRuleset<M>,
) -> Result<(), DecodeError> {
    let handles: Vec<_> = auth.cities().iter().map(|(h, _)| h).collect();
    if saved.cities.len() != handles.len() {
        return Err(corrupt("economy cities size"));
    }
    if saved.players.len() != auth.players().count() {
        return Err(corrupt("economy players size"));
    }
    let tiles = auth.map().tile_count();
    for (i, (handle, city)) in handles.iter().zip(&saved.cities).enumerate() {
        let (plan, economy, module) =
            decode_city(city, rules, tiles).map_err(|why| corrupt(format!("city[{i}] {why}")))?;
        if let Some(core) = auth.cities.get_mut(*handle) {
            core.citizens = plan;
            core.economy = economy;
        }
        if let Some(slot) = auth.cities.module_mut(*handle) {
            *slot = module;
        }
    }
    for (i, player) in saved.players.iter().enumerate() {
        let players = saved.players.len();
        let state = ModulePlayerOf::<M>::decode(player, players, auth)
            .map_err(|why| corrupt(format!("player[{i}] {why}")))?;
        let kernel = ModulePlayerOf::<M>::kernel_part(player);
        if let Some(slot) = auth.players.economy_mut(PlayerId(i as u8)) {
            *slot = state;
        }
        auth.players.set_plots_bought(PlayerId(i as u8), kernel.plots_bought);
    }
    if !saved.difficulty.is_empty() && rules.handicap(&saved.difficulty).is_none() && !rules.handicaps.is_empty() {
        return Err(corrupt("economy difficulty"));
    }
    if !saved.speed.is_empty() && rules.speed(&saved.speed).is_none() && !rules.speeds.is_empty() {
        return Err(corrupt("economy speed"));
    }
    auth.settings = crate::world::game_settings::GameSettings {
        difficulty: saved.difficulty.clone(),
        ai: saved.players.iter().map(|p| ModulePlayerOf::<M>::kernel_part(p).ai).collect(),
        speed: saved.speed.clone(),
        speed_values: crate::world::game_settings::GameSettings::values_of(rules, &saved.speed),
    };
    Ok(())
}

fn decode_city<M: RulesModule>(
    city: &SavedCityEconomy<M>,
    rules: &CompiledRuleset<M>,
    tiles: usize,
) -> Result<(CitizenPlan, CityEconomy, ModuleCityOf<M>), &'static str> {
    let focus = CityFocus::from_code(city.focus & !AVOID_GROWTH_BIT).ok_or("focus")?;
    let mut locked = Vec::with_capacity(city.locked.len());
    for t in &city.locked {
        if *t as usize >= tiles || locked.last().map(|p: &TileId| p.0 >= *t).unwrap_or(false) {
            return Err("locked tiles");
        }
        locked.push(TileId(*t));
    }
    let mut specialists = Vec::with_capacity(city.specialists.len());
    for (kind, count) in &city.specialists {
        let ascending = specialists
            .last()
            .map(|(k, _): &(SpecialistId, u8)| k.0 < *kind)
            .unwrap_or(true);
        if *kind as usize >= rules.specialists.len() || *count == 0 || !ascending {
            return Err("specialists");
        }
        specialists.push((SpecialistId(*kind), *count));
    }
    let module = ModuleCityOf::<M>::decode(&city.module, rules)?;
    let plan = CitizenPlan {
        focus,
        avoid_growth: city.focus & AVOID_GROWTH_BIT != 0,
        locked,
        specialists,
    };
    let process = crate::world::city_economy::Process::from_code(city.process).map_err(|_| "process")?;
    if let Some(crate::world::city_economy::Process::Contribute(project)) = process {
        let contributed = rules.module.project(project).is_some_and(|p| p.contributed);
        if !contributed {
            return Err("process");
        }
    }
    let economy = CityEconomy {
        food_kept: city.food_kept,
        process,
    };
    Ok((plan, economy, module))
}

/// Fold of the section for the world hash (the saved form is the
/// canonical one: what survives a save is what is hashed).
pub fn hash<M: RulesModule>(auth: &AuthWorld<M>) -> u64 {
    let saved = encode(auth);
    let mut h: u64 = 0x6563_6f6e_6f6d_79;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x100000001b3);
    };
    for city in &saved.cities {
        mix(u64::from(city.focus));
        for t in &city.locked {
            mix(0x1_0000_0000 | u64::from(*t));
        }
        for (k, n) in &city.specialists {
            mix(0x2_0000_0000 | u64::from(*k) << 8 | u64::from(*n));
        }
        mix(u64::from(city.food_kept));
        ModuleCityOf::<M>::hash(&city.module, &mut mix);
        mix(u64::from(city.process));
        mix(0x9e37_79b9);
    }
    for p in &saved.players {
        ModulePlayerOf::<M>::hash(p, &mut mix);
        mix(0x85eb_ca6b);
    }
    for b in saved.difficulty.bytes() {
        mix(u64::from(b));
    }
    // A Standard game keeps no speed and hashes as a game of before speeds.
    if !saved.speed.is_empty() {
        mix(0x7370_6565_6400);
        for b in saved.speed.bytes() {
            mix(u64::from(b));
        }
    }
    auth.module().hash(HashPoint::Economy, auth, &mut mix);
    h
}
