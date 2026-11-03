//! The sandbox AI: a greedy mind on the kernel's AI driver
//! (`kernel::ai::driver::AiDriver` runs the turn; this decides).
//!
//! - Settlers walk to the best free site they know and found a city.
//! - The first soldier in a city stays as its garrison.
//! - Other soldiers attack an adjacent enemy they can beat, else march on
//!   the nearest enemy city they know, else explore.
//! - Cities build settlers while the empire is small, a garrison when they
//!   have none, then their buildings, then soldiers.
//!
//! Decisions read what the player knows: its own units and cities, the
//! tiles it has seen and the enemy units it sees now. Every choice breaks
//! ties by tile or id, so a game replays exactly.

use kernel::ai::map::{explore, Blockers, KnownMap, SearchStats, Visit};
use kernel::ai::mind::{AiMind, TurnKind, TurnStart};
use kernel::ai::plan::{AiSettings, Decision, PlanTools};
use kernel::ai::state::{AiState, UnitTask};
use kernel::ai::trace::{Actor, Reason};
use kernel::command::KernelCommand;
use kernel::ids::{BuildingKindId, PlayerId, ProductionItem, TileId, UnitHandle, UnitKindId};
use kernel::knowledge::view::PlayerView;
use kernel::math::hex_disc;
use kernel::rules::roles::StartingUnit;
use kernel::scenario::generator::settler_kind;
use kernel::world::territory::city_site_check;

use crate::combat::{city_strength, defender_on, strength, Defender};
use crate::{Command, Game, Rules, Sandbox};

/// The empire stops building settlers at this many cities (settlers on
/// their way included).
const CITY_TARGET: usize = 6;
/// Settlers are built only before this round.
const SETTLING_ROUNDS: u32 = 60;
/// Units the empire keeps: this many per city and a few more.
const ARMY_PER_CITY: usize = 3;
const ARMY_BASE: usize = 2;
/// A soldier below these hit points goes home to heal.
const RETREAT_HP: u16 = 35;
/// Sites farther than this many move points are not considered.
const SITE_REACH: u32 = 12 * 60;

/// What the whole turn reads: the player's cities and the enemy cities it
/// knows.
#[derive(Debug)]
pub struct Brief {
    pub round: u32,
    /// Tiles of own cities, by tile.
    pub cities: Vec<TileId>,
    /// Known enemy city tiles, by tile.
    pub targets: Vec<TileId>,
    /// Own settlers alive.
    pub settlers: usize,
}

/// The sandbox's planners.
#[derive(Debug, Default)]
pub struct SandboxMind;

impl AiMind<Sandbox> for SandboxMind {
    type Brief = Brief;
    // Not enforced by types: the sandbox planners read the game itself (held by
    // the guard test `ai_truth_guard`).
    type Sight<'g> = &'g Game;
    type Near<'g> = &'g Game;

    fn sense<'g>(&self, game: &'g Game, _rules: &'g Rules, _player: PlayerId) -> &'g Game {
        game
    }

    fn sense_unit<'g>(&self, game: &'g Game, _rules: &'g Rules, _settings: &AiSettings, _player: PlayerId, _unit: UnitHandle) -> Option<&'g Game> {
        Some(game)
    }

    fn start_turn(&self, game: &&Game, rules: &Rules, player: PlayerId, state: &mut AiState) -> TurnStart<Sandbox, Brief> {
        let game: &Game = game;
        let auth = game.view().auth;
        let view = game.player_view(rules, player);
        let mut counters = kernel::metrics::WorkCounters::default();
        let (own, _) = kernel::ai::observation::units(auth, &view, &mut counters);
        let mut alive: Vec<_> = own.iter().map(|u| u.id).collect();
        alive.sort();
        state.retain_units(&alive);
        let settler = settler_kind(rules);
        let brief = Brief {
            round: auth.turn().round,
            cities: own_city_tiles(game, player),
            targets: known_enemy_cities(&view),
            settlers: own.iter().filter(|u| u.kind == settler).count(),
        };
        TurnStart {
            cities: brief.cities.len() as u32,
            brief,
            kind: TurnKind::Full,
            units: own.iter().map(|u| (u.id, u.handle, u.kind)).collect(),
            opening: Vec::new(),
        }
    }

    fn plan_strategic(&self, _: &Brief, _: &mut AiState, _: &Rules) -> Vec<Decision<Sandbox>> {
        Vec::new()
    }

    fn decide_unit(
        &self,
        game: &&Game,
        player: PlayerId,
        unit: UnitHandle,
        brief: &Brief,
        state: &mut AiState,
        tools: &mut PlanTools<'_, Sandbox>,
    ) -> Option<Decision<Sandbox>> {
        let game: &Game = game;
        let core = *game.view().auth.units().get(unit)?.core;
        let actor = Actor::Unit(core.id);
        if core.moves_left == 0 {
            return Some(Decision::refuse(actor, Reason::Idle));
        }
        let view = game.player_view(tools.rules, player);
        let decision = if core.kind == settler_kind(tools.rules) {
            settle(game, &view, unit, brief, state, tools)
        } else if strength(tools.rules, core.kind.0) > 0 {
            soldier(game, &view, unit, brief, state, tools)
        } else {
            Decision::refuse(actor, Reason::Idle)
        };
        Some(decision)
    }

    fn plan_end_of_turn(
        &self,
        game: &&Game,
        player: PlayerId,
        brief: &Brief,
        _: &AiState,
        tools: &mut PlanTools<'_, Sandbox>,
    ) -> Vec<Decision<Sandbox>> {
        let game: &Game = game;
        let auth = game.view().auth;
        let mut settlers = brief.settlers;
        let mut decisions = Vec::new();
        let mut cities: Vec<_> = auth.indexes().cities_of(player).to_vec();
        cities.sort_by_key(|c| auth.cities().get(*c).map(|core| core.id));
        for city in cities {
            let Some(core) = auth.cities().get(city) else { continue };
            if !core.queue.is_empty() {
                continue;
            }
            let Some((item, reason)) = production(game, tools.rules, brief, core, &mut settlers) else {
                continue;
            };
            let command = Command::Kernel(KernelCommand::EnqueueProduction { player, city, item });
            decisions.push(Decision::act(Actor::City(core.id), reason, command));
        }
        decisions
    }
}

fn own_city_tiles(game: &Game, player: PlayerId) -> Vec<TileId> {
    let auth = game.view().auth;
    let mut tiles: Vec<TileId> = auth.indexes().cities_of(player).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.tile).collect();
    tiles.sort();
    tiles
}

/// City tiles of other players the player has seen (as it remembers them).
fn known_enemy_cities(view: &PlayerView<'_, Sandbox>) -> Vec<TileId> {
    (0..view.tile_count() as u32)
        .map(TileId)
        .filter(|t| {
            let known = view.tile(*t);
            known.snapshot().is_some_and(|s| s.city && s.owner.is_some_and(|o| o != view.player()))
        })
        .collect()
}

// ---- settlers ----

/// Found here when this is the chosen site (or the first city), else walk to
/// the best site.
fn settle(
    game: &Game,
    view: &PlayerView<'_, Sandbox>,
    unit: UnitHandle,
    brief: &Brief,
    state: &mut AiState,
    tools: &mut PlanTools<'_, Sandbox>,
) -> Decision<Sandbox> {
    let auth = game.view().auth;
    let core = *auth.units().get(unit).expect("decided unit exists").core;
    let actor = Actor::Unit(core.id);
    let found = Command::Kernel(KernelCommand::FoundCity { settler: unit });
    let site_ok = |tile| city_site_check(auth, core.owner, tile).is_ok() && can_found_on(view, tile);
    if brief.cities.is_empty() && site_ok(core.tile) {
        return Decision::act(actor, Reason::FoundCity, found);
    }
    let held = match state.task_of(core.id).map(|t| t.task) {
        Some(UnitTask::Settle { site }) if site_ok(site) => Some(site),
        _ => None,
    };
    let site = held.or_else(|| best_site(view, core.tile, unit, state, tools, &site_ok));
    let Some(site) = site else {
        return Decision::refuse(actor, Reason::NoSettleSite);
    };
    state.set_task(core.id, UnitTask::Settle { site }, brief.round);
    if site == core.tile {
        return Decision::act(actor, Reason::FoundCity, found);
    }
    Decision::act(actor, Reason::MoveToSite, march(unit, site))
}

/// The settler may found a city on the tile's terrain.
fn can_found_on(view: &PlayerView<'_, Sandbox>, tile: TileId) -> bool {
    let rules = view.rules();
    let known = view.tile(tile);
    let terrain = known.snapshot().and_then(|s| rules.terrain_kind_of(s.terrain));
    terrain.and_then(|t| rules.terrains.get(t.0 as usize)).is_some_and(|t| t.can_found_city && !t.water)
}

/// The reachable known site with the most food and production around it,
/// nearer first on a tie.
fn best_site(
    view: &PlayerView<'_, Sandbox>,
    from: TileId,
    unit: UnitHandle,
    state: &AiState,
    tools: &mut PlanTools<'_, Sandbox>,
    site_ok: &dyn Fn(TileId) -> bool,
) -> Option<TileId> {
    let reserved = state.reserved_tiles(site_reservation, None);
    let map = KnownMap::of_unit(view, unit);
    let mut best: Option<(i64, TileId)> = None;
    let mut stats = SearchStats::default();
    explore(&map, from, &Blockers::default(), tools.scratch, tools.settings.budget.search(), &mut stats, &mut |tile, cost| {
        if cost > SITE_REACH {
            return Visit::Stop;
        }
        if reserved.binary_search(&tile).is_err() && site_ok(tile) {
            let score = site_value(view, tile) * 60 - i64::from(cost) / 4;
            if best.is_none_or(|(s, t)| (score, std::cmp::Reverse(tile)) > (s, std::cmp::Reverse(t))) {
                best = Some((score, tile));
            }
        }
        Visit::Continue
    });
    tools.counters.search.searches += stats.searches;
    tools.counters.search.nodes += stats.nodes;
    best.map(|(_, tile)| tile)
}

fn site_reservation(r: kernel::ai::state::Reservation) -> Option<TileId> {
    match r {
        kernel::ai::state::Reservation::Site(tile) => Some(tile),
        _ => None,
    }
}

/// Food counts twice, production once, over the known tiles within two.
fn site_value(view: &PlayerView<'_, Sandbox>, site: TileId) -> i64 {
    let rules = view.rules();
    hex_disc(view.dimensions(), site, 2)
        .filter_map(|t| view.tile(t).snapshot().and_then(|s| rules.terrain_kind_of(s.terrain)))
        .filter_map(|k| rules.terrains.get(k.0 as usize))
        .map(|t| i64::from(2 * t.yields.food + t.yields.production))
        .sum()
}

// ---- soldiers ----

fn soldier(
    game: &Game,
    view: &PlayerView<'_, Sandbox>,
    unit: UnitHandle,
    brief: &Brief,
    state: &mut AiState,
    tools: &mut PlanTools<'_, Sandbox>,
) -> Decision<Sandbox> {
    let auth = game.view().auth;
    let rules = tools.rules;
    let core = *auth.units().get(unit).expect("decided unit exists").core;
    let actor = Actor::Unit(core.id);
    if let Some(target) = beatable_neighbour(game, view, unit) {
        let reason = match auth.indexes().city_on_tile(target) {
            Some(_) => Reason::AttackCity,
            None => Reason::AttackUnit,
        };
        return Decision::act(actor, reason, Command::Attack { unit, target });
    }
    if brief.cities.contains(&core.tile) && is_first_soldier(game, rules, unit) {
        let fortify = Command::Kernel(KernelCommand::Fortify { unit });
        return match core.stance.is_fortified() {
            true => Decision::refuse(actor, Reason::Garrison),
            false => Decision::act(actor, Reason::Garrison, fortify),
        };
    }
    if core.hp < RETREAT_HP {
        return match nearest(core.tile, &brief.cities, view) {
            Some(home) if home != core.tile => Decision::act(actor, Reason::Retreat, march(unit, home)),
            _ => Decision::refuse(actor, Reason::Heal),
        };
    }
    let is_scout = rules.roles.starting_unit(StartingUnit::Explore) == Some(core.kind);
    if !is_scout {
        if let Some(target) = nearest(core.tile, &brief.targets, view) {
            state.set_task(core.id, UnitTask::Assault { target }, brief.round);
            return match approach(game, view, core.tile, target) {
                Some(tile) if tile != core.tile => Decision::act(actor, Reason::Advance, march(unit, tile)),
                _ => Decision::refuse(actor, Reason::Waiting),
            };
        }
    }
    match frontier(game, view, core.tile, unit, tools) {
        Some(target) => Decision::act(actor, Reason::Explore, march(unit, target)),
        None => Decision::refuse(actor, Reason::NoFrontier),
    }
}

/// The adjacent visible enemy with the best odds, when the odds are fair:
/// our power at least two thirds of theirs (a city: any odds once it is
/// half down).
fn beatable_neighbour(game: &Game, view: &PlayerView<'_, Sandbox>, unit: UnitHandle) -> Option<TileId> {
    let auth = game.view().auth;
    let rules = view.rules();
    let core = *auth.units().get(unit)?.core;
    let ours = u64::from(strength(rules, core.kind.0)) * u64::from(core.hp);
    let mut best: Option<(u64, TileId)> = None;
    for tile in view.dimensions().neighbors(core.tile) {
        if !view.is_visible(tile) {
            continue;
        }
        let theirs = match defender_on(auth, rules, core.owner, tile) {
            Some(Defender::Unit(enemy)) => {
                let enemy = auth.units().get(enemy)?.core;
                u64::from(strength(rules, enemy.kind.0)) * u64::from(enemy.hp)
            }
            Some(Defender::City(city)) => {
                let city = auth.cities().get(city)?;
                let hp = crate::combat::CITY_MAX_HP.saturating_sub(city.damage);
                let walls = city_strength(city) * u64::from(hp);
                if hp <= crate::combat::CITY_MAX_HP / 2 { walls / 2 } else { walls }
            }
            Some(Defender::Civilians) => 0,
            None => continue,
        };
        if ours * 3 < theirs * 2 {
            continue;
        }
        if best.is_none_or(|(b, t)| (theirs, tile) < (b, t)) {
            best = Some((theirs, tile));
        }
    }
    best.map(|(_, tile)| tile)
}

/// The unit is the oldest soldier of its owner on its tile.
fn is_first_soldier(game: &Game, rules: &Rules, unit: UnitHandle) -> bool {
    let auth = game.view().auth;
    let Some(core) = auth.units().get(unit).map(|v| *v.core) else { return false };
    let oldest = auth
        .indexes()
        .units_on(core.tile)
        .iter()
        .filter_map(|h| auth.units().get(*h).map(|v| *v.core))
        .filter(|u| u.owner == core.owner && strength(rules, u.kind.0) > 0)
        .map(|u| u.id)
        .min();
    oldest == Some(core.id)
}

/// The nearest of `tiles` by hex distance (lower tile on a tie).
fn nearest(from: TileId, tiles: &[TileId], view: &PlayerView<'_, Sandbox>) -> Option<TileId> {
    let dims = view.dimensions();
    tiles.iter().copied().min_by_key(|t| (dims.distance(from, *t).unwrap_or(i32::MAX), *t))
}

/// Where a soldier waits to strike `target`: the neighbour of it nearest to
/// `from` that it may stand on (`from` itself when it is one).
fn approach(game: &Game, view: &PlayerView<'_, Sandbox>, from: TileId, target: TileId) -> Option<TileId> {
    let dims = view.dimensions();
    let blocked = enemy_tiles(game, view);
    dims.neighbors(target)
        .into_iter()
        .filter(|t| *t == from || (can_stand(view, *t) && blocked.binary_search(t).is_err()))
        .min_by_key(|t| (dims.distance(from, *t).unwrap_or(i32::MAX), *t))
}

/// Known land a soldier may walk onto.
fn can_stand(view: &PlayerView<'_, Sandbox>, tile: TileId) -> bool {
    let rules = view.rules();
    let terrain = view.tile(tile).snapshot().and_then(|s| rules.terrain_kind_of(s.terrain));
    terrain.and_then(|t| rules.terrains.get(t.0 as usize)).is_some_and(|t| !t.water && !t.impassable)
}

/// Tiles of enemy units the player sees and of enemy cities it knows,
/// sorted: a walk never ends there.
fn enemy_tiles(game: &Game, view: &PlayerView<'_, Sandbox>) -> Vec<TileId> {
    let auth = game.view().auth;
    let mut tiles: Vec<TileId> = auth
        .units()
        .iter_cores()
        .filter(|u| u.owner != view.player() && view.is_visible(u.tile))
        .map(|u| u.tile)
        .chain(known_enemy_cities(view))
        .collect();
    tiles.sort();
    tiles.dedup();
    tiles
}

/// The cheapest known tile next to an unknown one.
fn frontier(
    game: &Game,
    view: &PlayerView<'_, Sandbox>,
    from: TileId,
    unit: UnitHandle,
    tools: &mut PlanTools<'_, Sandbox>,
) -> Option<TileId> {
    let map = KnownMap::of_unit(view, unit);
    let mut found = None;
    let mut stats = SearchStats::default();
    let blockers = Blockers::new(enemy_tiles(game, view));
    explore(&map, from, &blockers, tools.scratch, tools.settings.budget.search(), &mut stats, &mut |tile, _| {
        if tile != from && !blockers.contains(tile) && kernel::ai::map::is_frontier(view, tile) {
            found = Some(tile);
            return Visit::Stop;
        }
        Visit::Continue
    });
    tools.counters.search.searches += stats.searches;
    tools.counters.search.nodes += stats.nodes;
    found
}

fn march(unit: UnitHandle, destination: TileId) -> Command {
    Command::Kernel(KernelCommand::MarchTo { unit, destination })
}

// ---- cities ----

/// What an idle city builds next (None: the empire has army enough).
fn production(
    game: &Game,
    rules: &Rules,
    brief: &Brief,
    city: &kernel::world::city::CityCore,
    settlers: &mut usize,
) -> Option<(ProductionItem, Reason)> {
    let auth = game.view().auth;
    let settler = settler_kind(rules);
    let expanding = brief.round < SETTLING_ROUNDS && brief.cities.len() + *settlers < CITY_TARGET;
    if expanding && city.population >= 2 {
        *settlers += 1;
        return Some((ProductionItem::Unit(settler), Reason::ProduceSettler));
    }
    let garrisoned = auth
        .indexes()
        .units_on(city.tile)
        .iter()
        .filter_map(|h| auth.units().get(*h))
        .any(|v| v.core.owner == city.owner && strength(rules, v.core.kind.0) > 0);
    let defender = rules.roles.starting_unit(StartingUnit::Defense).unwrap_or(settler);
    if !garrisoned {
        return Some((ProductionItem::Unit(defender), Reason::ProduceDefender));
    }
    if let Some(building) = next_building(rules, city) {
        return Some((ProductionItem::Building(building), Reason::ProduceBuilding));
    }
    let army = auth.indexes().units_of(city.owner).len();
    if army >= ARMY_PER_CITY * brief.cities.len() + ARMY_BASE {
        return None;
    }
    Some((ProductionItem::Unit(strongest_unit(rules).unwrap_or(defender)), Reason::ProduceArmy))
}

/// The cheapest building the city lacks (buildable ones only).
fn next_building(rules: &Rules, city: &kernel::world::city::CityCore) -> Option<BuildingKindId> {
    rules
        .buildings
        .iter()
        .enumerate()
        .filter(|(i, b)| !b.unbuildable && !city.buildings.contains(&BuildingKindId(*i as u16)))
        .min_by_key(|(i, b)| (b.cost, *i))
        .map(|(i, _)| BuildingKindId(i as u16))
}

/// The buildable soldier with the most strength.
fn strongest_unit(rules: &Rules) -> Option<UnitKindId> {
    (0..rules.units.len() as u16)
        .filter(|k| !rules.units[*k as usize].unbuildable && strength(rules, *k) > 0)
        .max_by_key(|k| (strength(rules, *k), std::cmp::Reverse(*k)))
        .map(UnitKindId)
}
