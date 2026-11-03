//! Save -> GameState with version checks and range validation (§26).
//!
//! Untrusted data: any out-of-range index is DecodeError::Corrupt,
//! not a panic. Versions: an incompatible sim/ruleset is an explicit error
//! (replay between versions is not guaranteed, §27).

use crate::rules::tables::ModuleTables;
use crate::module::RulesModule;
use crate::ids::{
    BuildingKindId, EntityId, FeatureId, ImprovementId, PlayerId, ResourceId, TerrainId, TileId,
    UnitKindId,
};
use crate::math::MapDimensions;
use crate::persistence::codec::ModuleSections;
use crate::persistence::encode::{SAVE_SCHEMA_VERSION, SIMULATION_VERSION};
use crate::persistence::schema::SaveGameV1;
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::module::{ModuleUnit, ModuleUnitOf, ModuleWorld, NewModuleWorld};
use crate::world::player::PlayerKind;
use crate::world::state::{AuthWorld, GameState};
use crate::world::unit::UnitCore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    SchemaMismatch(u32),
    SimMismatch(u32),
    RulesetMismatch,
    Corrupt(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SchemaMismatch(v) => write!(f, "the save has another schema ({v})"),
            Self::SimMismatch(v) => write!(f, "the save has another simulation version ({v})"),
            Self::RulesetMismatch => f.write_str("the save was made with other rules"),
            Self::Corrupt(what) => write!(f, "the save is damaged ({what})"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn corrupt(msg: impl Into<String>) -> DecodeError {
    DecodeError::Corrupt(msg.into())
}

/// What a load does when the save's rules fingerprint is not the current
/// rules' (platform/docs/saves.md, "Saving other rules").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RulesMatch {
    /// Replay and tests: only the current fingerprint.
    Exact,
    /// A player's game: the current fingerprint, or the fingerprint of
    /// schema 44 / simulation 1 before the role bindings, before the rules
    /// module's id or before the city-name catalog (same world bytes, same
    /// behaviour; saved city names are never rewritten).
    SameRules,
    /// Development: any fingerprint. The game goes on under the current
    /// rules and the load reports the change ([`RulesCheck::Changed`]).
    AnyRules,
}

/// The rules a loaded game was saved with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesCheck {
    /// The current rules (or an earlier fingerprint of them).
    Same,
    /// Other rules: the game now runs under the current ones.
    Changed(RulesDrift),
}

/// How the rules changed since a save (`RulesMatch::AnyRules`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulesDrift {
    /// The fingerprint in the save's header.
    pub saved: u64,
    /// The current rules' fingerprint.
    pub current: u64,
    /// The parts of the rules that changed (`RulesDigest::changed_parts`);
    /// None: the save keeps no digest, the parts are unknown.
    pub parts: Option<Vec<String>>,
    /// The kernel tables whose rows may have moved (`RulesDigest::shifted_tables`): a world
    /// points into them by row number. None: the save keeps no digest.
    pub shifted: Option<Vec<String>>,
}

/// Strict content guard, including for replay snapshots.
pub fn decode<M: RulesModule>(save: &SaveGameV1<M>, rules: &CompiledRuleset<M>) -> Result<GameState<M>, DecodeError> {
    decode_with(save, rules, RulesMatch::Exact).map(|(game, _)| game)
}

/// Load a saved snapshot of the same rules (`RulesMatch::SameRules`).
/// This is not a compatibility promise for historical replay command logs.
pub fn decode_snapshot<M: RulesModule>(save: &SaveGameV1<M>, rules: &CompiledRuleset<M>) -> Result<GameState<M>, DecodeError> {
    decode_with(save, rules, RulesMatch::SameRules).map(|(game, _)| game)
}

/// Load a save accepting its rules by `accept`: the game and the rules it was saved with.
pub fn decode_with<M: RulesModule>(
    save: &SaveGameV1<M>,
    rules: &CompiledRuleset<M>,
    accept: RulesMatch,
) -> Result<(GameState<M>, RulesCheck), DecodeError> {
    if save.header.save_schema_version != SAVE_SCHEMA_VERSION {
        return Err(DecodeError::SchemaMismatch(save.header.save_schema_version));
    }
    if save.header.simulation_version != SIMULATION_VERSION {
        return Err(DecodeError::SimMismatch(save.header.simulation_version));
    }
    let check = rules_check(save, rules, accept)?;
    Ok((decode_world(save, rules)?, check))
}

/// The save's fingerprint against the current rules, accepted by `accept`.
fn rules_check<M: RulesModule>(
    save: &SaveGameV1<M>,
    rules: &CompiledRuleset<M>,
    accept: RulesMatch,
) -> Result<RulesCheck, DecodeError> {
    let saved = save.header.ruleset_hash;
    let current = rules.hash();
    let earlier = || {
        [rules.hash_before_roles(), rules.hash_before_module_id(), rules.hash_before_city_names()].contains(&saved)
            || M::EARLIER_EDITIONS.contains(&(saved, current))
    };
    match accept {
        _ if saved == current => Ok(RulesCheck::Same),
        RulesMatch::SameRules if earlier() => Ok(RulesCheck::Same),
        RulesMatch::Exact | RulesMatch::SameRules => Err(DecodeError::RulesetMismatch),
        RulesMatch::AnyRules => {
            let now = rules.digest();
            let digest = save.world.module.rules_digest();
            let parts = digest.map(|digest| digest.changed_parts(&now));
            let shifted = digest.map(|digest| digest.shifted_tables(&now));
            Ok(RulesCheck::Changed(RulesDrift { saved, current, parts, shifted }))
        }
    }
}

/// The world of a save whose versions and rules were accepted.
fn decode_world<M: RulesModule>(save: &SaveGameV1<M>, rules: &CompiledRuleset<M>) -> Result<GameState<M>, DecodeError> {
    let w = &save.world;
    if w.width == 0 || w.height == 0 || w.players == 0 {
        return Err(corrupt("empty dimensions/players"));
    }
    let tiles = (w.width as usize)
        .checked_mul(w.height as usize)
        .ok_or_else(|| corrupt("dims overflow"))?;
    if tiles > 1_000_000 {
        return Err(corrupt("map too large"));
    }
    for (name, len) in [
        ("terrain", w.terrain.len()),
        ("feature", w.feature.len()),
        ("resource", w.resource.len()),
        ("resource_amount", w.resource_amount.len()),
        ("owner", w.owner.len()),
        ("improvement", w.improvement.len()),
        ("road", w.road.len()),
        ("natural_wonder", w.natural_wonder.len()),
        ("river", w.river.len()),
    ] {
        if len != tiles {
            return Err(corrupt(format!("{} len {} != tiles {}", name, len, tiles)));
        }
    }
    let players = w.players as usize;
    if w.diplomacy.len() != players * players {
        return Err(corrupt("diplomacy matrix size"));
    }
    if w.diplomacy_open.len() != players * players {
        return Err(corrupt("diplomacy open matrix size"));
    }
    if w.players_alive.len() != players
        || w.knowledge_explored.len() != players
        || w.player_techs.len() != players
    {
        return Err(corrupt("players/knowledge/techs size"));
    }
    for (i, k) in w.knowledge_explored.iter().enumerate() {
        if k.len() != tiles {
            return Err(corrupt(format!("knowledge[{}] len", i)));
        }
    }
    let check_tile = |t: u32| -> Result<(), DecodeError> {
        if (t as usize) < tiles {
            Ok(())
        } else {
            Err(corrupt(format!("tile {} out of {}", t, tiles)))
        }
    };
    let check_player = |p: u8| -> Result<(), DecodeError> {
        if (p as usize) < players {
            Ok(())
        } else {
            Err(corrupt(format!("player {} out of {}", p, players)))
        }
    };
    for (i, u) in w.units.iter().enumerate() {
        check_tile(u.tile).map_err(|_| corrupt(format!("unit[{}] tile", i)))?;
        check_player(u.owner).map_err(|_| corrupt(format!("unit[{}] owner", i)))?;
    }
    for (i, c) in w.cities.iter().enumerate() {
        check_tile(c.tile).map_err(|_| corrupt(format!("city[{}] tile", i)))?;
        check_player(c.owner).map_err(|_| corrupt(format!("city[{}] owner", i)))?;
        check_player(c.original_owner).map_err(|_| corrupt(format!("city[{}] original owner", i)))?;
    }
    if w.player_kinds.len() != players {
        return Err(corrupt("player kinds size"));
    }
    for (i, techs) in w.player_techs.iter().enumerate() {
        for t in techs {
            if (*t as usize) >= rules.technologies.len() {
                return Err(corrupt(format!("player[{}] tech {}", i, t)));
            }
        }
    }
    // Rivers: ends within the map, canonicality (a<=b), sortedness.
    // The river tile cache is built by MapState from the edges by the same rule as the
    // live map (water is never a river); w.river is only the length above.
    let mut prev: Option<(u32, u32)> = None;
    for (a, b) in &w.river_edges {
        if (*a as usize) >= tiles || (*b as usize) >= tiles || a > b {
            return Err(corrupt(format!("river edge ({},{})", a, b)));
        }
        if let Some(p) = prev {
            if (*a, *b) <= p {
                return Err(corrupt("river edges not sorted/unique"));
            }
        }
        prev = Some((*a, *b));
    }
    if (w.active_player as usize) >= players {
        return Err(corrupt("active_player"));
    }
    check_entity_ids(w)?;
    crate::persistence::check::check_content(w, rules)?;
    let phase = match w.turn_phase {
        0 => crate::turn::TurnPhase::Start,
        1 => crate::turn::TurnPhase::PlayerActions,
        2 => crate::turn::TurnPhase::Resolution,
        3 => crate::turn::TurnPhase::End,
        x => return Err(corrupt(format!("turn_phase {}", x))),
    };

    let dims = MapDimensions {
        width: w.width,
        height: w.height,
    };
    let mut units = crate::world::unit::UnitStore::<M>::new();
    let mut indexes = crate::world::indexes::WorldIndexes::new();
    for (i, u) in w.units.iter().enumerate() {
        let promotions = crate::persistence::effects::decode_promotions(&u.promotions, rules, i)?;
        let h = units.insert(UnitCore {
            id: EntityId(u.id),
            tile: TileId(u.tile),
            owner: PlayerId(u.owner),
            kind: UnitKindId(u.kind),
            hp: u.hp,
            moves_left: u.moves_left,
            moves_max: u.moves_max,
            stance: decode_stance(u.stance).ok_or_else(|| corrupt(format!("unit[{}] stance", i)))?,
            work: u.work.map(|(tile, tag, id, left)| crate::ids::UnitWork {
                tile: TileId(tile),
                target: match tag {
                    1 => crate::ids::WorkTarget::Road,
                    2 => crate::ids::WorkTarget::Chop(crate::ids::FeatureId(id)),
                    _ => crate::ids::WorkTarget::Improvement(crate::ids::ImprovementId(id)),
                },
                work_left: left,
            }),
            goal: u.goal.map(TileId),
            promotions,
            experience: u.experience,
            level: u.level,
            attacks_made: u.attacks_made,
            interceptions_made: u.interceptions_made,
        });
        let state = ModuleUnitOf::<M>::decode(&u.module, rules).map_err(|what| corrupt(format!("unit[{i}] {what}")))?;
        if let Some(slot) = units.module_mut(h) {
            *slot = state;
        }
        indexes.index_unit(PlayerId(u.owner), TileId(u.tile), h);
    }
    let mut cities = crate::world::city::CityStore::new();
    let mut city_order = Vec::with_capacity(w.cities.len());
    for c in &w.cities {
        let mut queue = Vec::with_capacity(c.queue.len());
        for (tag, id) in &c.queue {
            let item = match tag {
                0 => crate::ids::ProductionItem::Unit(crate::ids::UnitKindId(*id)),
                1 => crate::ids::ProductionItem::Building(crate::ids::BuildingKindId(*id)),
                2 if rules.module.project(crate::rules::content::ProjectId(*id)).is_some() => {
                    crate::ids::ProductionItem::Project(crate::rules::content::ProjectId(*id))
                }
                _ => return Err(corrupt(format!("city queue tag {}", tag))),
            };
            queue.push(item);
        }
        let h = cities.insert(CityCore {
            id: EntityId(c.id),
            tile: TileId(c.tile),
            owner: PlayerId(c.owner),
            name: c.name.clone(),
            population: c.population,
            food_times100: c.food,
            buildings: c.buildings.iter().map(|b| BuildingKindId(*b)).collect(),
            production: c.production,
            queue,
            unworked: c.unworked.iter().map(|t| TileId(*t)).collect(),
            culture: c.culture,
            acquired: c.acquired,
            radius: c.radius,
            damage: c.damage,
            original_owner: PlayerId(c.original_owner),
            original_capital: c.original_capital,
            status: decode_status(c.status).ok_or_else(|| corrupt("city status"))?,
            resistance_turns: c.resistance_turns,
            strike_made: c.strike_made,
            capital_connection: if c.connected_to_capital {
                crate::world::city::CapitalConnection::Connected
            } else {
                crate::world::city::CapitalConnection::Disconnected
            },
            acquired_round: c.acquired_round,
            citizens: Default::default(),
            economy: Default::default(),
        });
        indexes.index_city(PlayerId(c.owner), TileId(c.tile), h);
        city_order.push(h);
    }
    let owning_city = decode_tile_cities(w, &city_order)?;
    let routes = w
        .road
        .iter()
        .map(|code| crate::world::map::Route::from_code(*code))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| corrupt("route code"))?;
    let natural_wonders = w
        .natural_wonder
        .iter()
        .map(|o| match o {
            Some(id) if (*id as usize) < rules.natural_wonders.len() => {
                Ok(Some(crate::rules::content::NaturalWonderId(*id)))
            }
            Some(id) => Err(corrupt(format!("natural wonder {}", id))),
            None => Ok(None),
        })
        .collect::<Result<Vec<_>, _>>()?;
    // `check` verified every pillaged tile is on the map with an improvement.
    let mut map = crate::world::map::MapState::from_parts(
        dims,
        w.terrain.iter().map(|t| TerrainId(*t)).collect(),
        w.feature.iter().map(|o| o.map(FeatureId)).collect(),
        w.resource.iter().map(|o| o.map(ResourceId)).collect(),
        resource_amounts(w),
        w.owner.iter().map(|o| o.map(PlayerId)).collect(),
        owning_city,
        w.improvement.iter().map(|o| o.map(ImprovementId)).collect(),
        pillaged_tiles(w),
        routes,
        natural_wonders,
        w.river_edges.clone(),
    );
    map.set_terrain_kinds(crate::navigation::cost::TerrainKinds::from_rules(rules));
    let effects = crate::persistence::effects::decode_effects(
        &w.active_effects,
        rules,
        players,
        &city_order,
    )?;
    let techs: Vec<Vec<crate::ids::TechId>> = w
        .player_techs
        .iter()
        .map(|v| v.iter().map(|t| crate::ids::TechId(*t)).collect())
        .collect();
    for (i, rq) in w.player_research.iter().enumerate() {
        for t in rq {
            if (*t as usize) >= rules.technologies.len() {
                return Err(corrupt(format!("player[{}] research {}", i, t)));
            }
        }
    }
    let research: Vec<Vec<crate::ids::TechId>> = w
        .player_research
        .iter()
        .map(|v| v.iter().map(|t| crate::ids::TechId(*t)).collect())
        .collect();
    let mut player_store = crate::world::player::PlayerStore::from_parts(
        w.players_alive.clone(),
        techs,
        w.player_gold.clone(),
        w.player_science.clone(),
        research,
        w.player_nations.clone(),
    );
    for (i, kind) in w.player_kinds.iter().enumerate() {
        let kind = PlayerKind::from_code(*kind, rules).ok_or_else(|| corrupt(format!("player[{}] kind", i)))?;
        player_store.set_kind(PlayerId(i as u8), kind);
    }
    let mut auth = AuthWorld {
        map,
        players: player_store,
        cities,
        units,
        diplomacy: crate::world::diplomacy::DiplomacyState::from_parts(
            players,
            w.diplomacy.clone(),
            w.diplomacy_open.clone(),
        ),
        turn: crate::turn::TurnState::restore(w.turn_round, phase, PlayerId(w.active_player)),
        indexes,
        knowledge: decode_knowledge(w, rules)?,
        effects,
        rng: crate::rng::RngState {
            combat: crate::rng::StreamRng::restore(w.rng_combat),
            events: crate::rng::StreamRng::restore(w.rng_events),
        },
        next_entity: w.next_entity,
        settings: Default::default(),
        module: M::World::new(NewModuleWorld {
            players,
            seed: 0,
            rules,
        }),
        memo: Default::default(),
    };
    auth.module = M::World::decode(&w.module, &auth, rules)?;
    crate::persistence::economy::restore(&mut auth, &w.economy, rules)?;
    Ok(GameState::restore(auth, rules))
}

fn decode_stance((tag, turns): (u8, u8)) -> Option<crate::world::unit::Stance> {
    use crate::world::unit::{Stance, MAX_FORTIFY_TURNS};
    match tag {
        0 => Some(Stance::Ready),
        1 if (1..=MAX_FORTIFY_TURNS).contains(&turns) => Some(Stance::Fortified { turns }),
        2 => Some(Stance::SetUp),
        3 => Some(Stance::Intercept),
        _ => None,
    }
}

fn decode_status(tag: u8) -> Option<crate::world::city::CityStatus> {
    use crate::world::city::CityStatus;
    match tag {
        0 => Some(CityStatus::Own),
        1 => Some(CityStatus::AwaitingDecision),
        2 => Some(CityStatus::Occupied),
        3 => Some(CityStatus::Puppet),
        4 => Some(CityStatus::Razing),
        _ => None,
    }
}

/// Units and cities are saved in ascending entity order, ids are unique
/// across both and below `next_entity` (identity and canonical order).
/// Copies per tile, kept in step with the resources (1 or more on a
/// resource, 0 without one).
fn resource_amounts<M: RulesModule>(w: &crate::persistence::schema::SavedWorldV1<M>) -> Vec<u8> {
    w.resource
        .iter()
        .zip(&w.resource_amount)
        .map(|(resource, amount)| if resource.is_some() { (*amount).max(1) } else { 0 })
        .collect()
}

fn check_entity_ids<M: RulesModule>(w: &crate::persistence::schema::SavedWorldV1<M>) -> Result<(), DecodeError> {
    let ascending = |ids: &mut dyn Iterator<Item = u64>| {
        let mut previous: Option<u64> = None;
        for id in ids {
            if previous.is_some_and(|p| p >= id) || id >= w.next_entity {
                return false;
            }
            previous = Some(id);
        }
        true
    };
    if !ascending(&mut w.units.iter().map(|u| u.id)) {
        return Err(corrupt("unit ids"));
    }
    if !ascending(&mut w.cities.iter().map(|c| c.id)) {
        return Err(corrupt("city ids"));
    }
    let mut all: Vec<u64> = w.units.iter().map(|u| u.id).collect();
    all.extend(w.cities.iter().map(|c| c.id));
    all.sort_unstable();
    if all.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(corrupt("entity id shared by a unit and a city"));
    }
    Ok(())
}

/// Player knowledge: explored flags, the memory of every explored tile (in
/// tile order) and the contacts. Every remembered id must be known to the
/// rules, every round no later than the save's round.
fn decode_knowledge<M: RulesModule>(
    w: &crate::persistence::schema::SavedWorldV1<M>,
    rules: &CompiledRuleset<M>,
) -> Result<Vec<crate::knowledge::PlayerKnowledge>, DecodeError> {
    let players = w.players as usize;
    // The deposits and the cities are absent from a save of the formats before sightings.
    let sized = |len: usize| len == 0 || len == players;
    if w.knowledge_memory.len() != players
        || w.knowledge_contacts.len() != players
        || !sized(w.knowledge_resource.len())
        || !sized(w.knowledge_cities.len())
    {
        return Err(corrupt("knowledge size"));
    }
    let mut out = Vec::with_capacity(players);
    for p in 0..players {
        let flags = &w.knowledge_explored[p];
        let mut memory = decode_memory(w, rules, p, flags)?;
        let mut explored = crate::tile_set::TileSet::with_tile_count(flags.len());
        explored.extend(flags.iter().enumerate().filter(|(_, f)| **f == 1).map(|(i, _)| crate::ids::TileId(i as u32)));
        let sightings = decode_sightings(w, rules, p, &memory)?;
        if w.knowledge_cities.is_empty() {
            forget_vanished_cities(&mut memory, &sightings);
        }
        let contacts = decode_contacts(w, &w.knowledge_contacts[p])?;
        out.push(crate::knowledge::PlayerKnowledge::from_parts(explored, memory, sightings, contacts));
    }
    Ok(out)
}

fn decode_memory<M: RulesModule>(
    w: &crate::persistence::schema::SavedWorldV1<M>,
    rules: &CompiledRuleset<M>,
    player: usize,
    explored: &[u8],
) -> Result<Vec<crate::knowledge::state::TileMemory>, DecodeError> {
    use crate::knowledge::state::TileMemory;
    let saved = &w.knowledge_memory[player];
    if explored.iter().filter(|e| **e == 1).count() != saved.len() {
        return Err(corrupt("knowledge memory count"));
    }
    // A save from before sightings has no deposits: the map's are remembered.
    let resources = w.knowledge_resource.get(player);
    if resources.is_some_and(|resources| resources.len() != saved.len()) {
        return Err(corrupt("knowledge resource count"));
    }
    let mut entries = saved.iter().enumerate();
    let mut memory = vec![TileMemory::default(); explored.len()];
    for (tile, (slot, flag)) in memory.iter_mut().zip(explored).enumerate() {
        if *flag == 1 {
            let (index, entry) = entries.next().expect("count checked above");
            let resource = resources.map_or_else(|| w.resource[tile].unwrap_or(u16::MAX), |resources| resources[index]);
            *slot = memory_entry(w, rules, *entry, resource)?;
        }
    }
    Ok(memory)
}

/// One remembered tile: its ids must be known to the rules, its round no later than the save's.
fn memory_entry<M: RulesModule>(
    w: &crate::persistence::schema::SavedWorldV1<M>,
    rules: &CompiledRuleset<M>,
    entry: (u32, u16, u16, u8, u8),
    resource: u16,
) -> Result<crate::knowledge::state::TileMemory, DecodeError> {
    use crate::knowledge::state::TileMemory;
    let (round, improvement, feature, owner, flags) = entry;
    let known = |id: u16, len: usize| id == u16::MAX || (id as usize) < len;
    let owner_ok = owner == u8::MAX || owner < w.players;
    let ids_ok = known(improvement, rules.improvements.len())
        && known(feature, rules.features.len())
        && known(resource, rules.resources.len());
    if round > w.turn_round || !owner_ok || !ids_ok || !TileMemory::flags_valid(flags) {
        return Err(corrupt("knowledge memory entry"));
    }
    Ok(TileMemory::from_packed(entry, resource))
}

/// The cities the player last saw, by tile. A save from before sightings gets one for each
/// remembered city that still stands (its name, its remembered owner), with no numbers: they
/// were never recorded.
fn decode_sightings<M: RulesModule>(
    w: &crate::persistence::schema::SavedWorldV1<M>,
    rules: &CompiledRuleset<M>,
    player: usize,
    memory: &[crate::knowledge::state::TileMemory],
) -> Result<std::collections::BTreeMap<u32, crate::knowledge::CitySighting>, DecodeError> {
    match w.knowledge_cities.get(player) {
        Some(saved) => saved_sightings(w, rules, saved, memory),
        None => Ok(legacy_sightings(w, memory)),
    }
}

/// A save from before sightings may remember a city that has since vanished: with no sighting
/// to draw it from, the memory forgets it.
fn forget_vanished_cities(
    memory: &mut [crate::knowledge::state::TileMemory],
    sightings: &std::collections::BTreeMap<u32, crate::knowledge::CitySighting>,
) {
    for (tile, slot) in memory.iter_mut().enumerate() {
        if slot.layers().city && !sightings.contains_key(&(tile as u32)) {
            slot.clear_city();
        }
    }
}

fn legacy_sightings<M: RulesModule>(
    w: &crate::persistence::schema::SavedWorldV1<M>,
    memory: &[crate::knowledge::state::TileMemory],
) -> std::collections::BTreeMap<u32, crate::knowledge::CitySighting> {
    use crate::knowledge::CitySighting;
    let mut out = std::collections::BTreeMap::new();
    for city in &w.cities {
        let remembered = memory.get(city.tile as usize).map(|m| m.layers()).filter(|layers| layers.city);
        if let Some(layers) = remembered {
            let owner = layers.owner.unwrap_or(PlayerId(city.owner));
            out.insert(city.tile, CitySighting { owner, name: city.name.clone(), seen: None });
        }
    }
    out
}

/// Every saved sighting stands on a tile whose memory holds a city, in ascending tile order,
/// and every such tile has one.
fn saved_sightings<M: RulesModule>(
    w: &crate::persistence::schema::SavedWorldV1<M>,
    rules: &CompiledRuleset<M>,
    saved: &[crate::persistence::schema::SavedSighting],
    memory: &[crate::knowledge::state::TileMemory],
) -> Result<std::collections::BTreeMap<u32, crate::knowledge::CitySighting>, DecodeError> {
    use crate::knowledge::{CitySighting, SightedCity};
    let building_ok = |b: &u16| (*b as usize) < rules.buildings.len();
    let mut out = std::collections::BTreeMap::new();
    for (tile, owner, name, seen) in saved {
        let ordered = out.keys().next_back().is_none_or(|last| last < tile);
        let remembered = memory.get(*tile as usize).is_some_and(|m| m.layers().city);
        let seen_ok = seen.as_ref().is_none_or(|(_, _, buildings)| {
            buildings.iter().all(building_ok) && buildings.windows(2).all(|pair| pair[0] < pair[1])
        });
        if !ordered || *owner >= w.players || !remembered || !seen_ok {
            return Err(corrupt("knowledge city"));
        }
        let seen = seen.as_ref().map(|(population, era, buildings)| SightedCity {
            population: *population,
            era: *era,
            buildings: buildings.clone(),
        });
        out.insert(*tile, CitySighting { owner: PlayerId(*owner), name: name.clone(), seen });
    }
    if memory.iter().filter(|m| m.layers().city).count() != out.len() {
        return Err(corrupt("knowledge city missing"));
    }
    Ok(out)
}

fn decode_contacts<M: RulesModule>(
    w: &crate::persistence::schema::SavedWorldV1<M>,
    saved: &[(Option<u32>, Option<u32>)],
) -> Result<Vec<crate::knowledge::Contact>, DecodeError> {
    if saved.len() != w.players as usize {
        return Err(corrupt("contacts size"));
    }
    let late = |round: Option<u32>| round.is_some_and(|r| r > w.turn_round);
    let mut out = Vec::with_capacity(saved.len());
    for (met_round, last_seen_round) in saved {
        if late(*met_round) || late(*last_seen_round) {
            return Err(corrupt("contact round"));
        }
        out.push(crate::knowledge::Contact {
            met_round: *met_round,
            last_seen_round: *last_seen_round,
        });
    }
    Ok(out)
}

/// The owning city of every tile (city positions in `cities`). An owned
/// tile belongs to exactly one city of its owner; an unowned tile to none.
fn decode_tile_cities<M: RulesModule>(
    w: &crate::persistence::schema::SavedWorldV1<M>,
    city_order: &[crate::ids::CityHandle],
) -> Result<Vec<Option<crate::ids::CityHandle>>, DecodeError> {
    if w.tile_city.len() != w.owner.len() {
        return Err(corrupt("tile_city len"));
    }
    let mut out = Vec::with_capacity(w.tile_city.len());
    for (i, (city, owner)) in w.tile_city.iter().zip(&w.owner).enumerate() {
        let handle = match (city, owner) {
            (None, None) => None,
            (Some(ordinal), Some(owner)) => {
                let saved = w
                    .cities
                    .get(*ordinal as usize)
                    .ok_or_else(|| corrupt(format!("tile {i} city {ordinal}")))?;
                if saved.owner != *owner {
                    return Err(corrupt(format!("tile {i} owner differs from its city")));
                }
                Some(city_order[*ordinal as usize])
            }
            _ => return Err(corrupt(format!("tile {i} owner without city"))),
        };
        out.push(handle);
    }
    Ok(out)
}


/// One flag per tile from the pillaged tile list.
fn pillaged_tiles<M: RulesModule>(w: &crate::persistence::schema::SavedWorldV1<M>) -> crate::tile_set::TileSet {
    let tiles = w.improvement.len();
    let mut flags = crate::tile_set::TileSet::with_tile_count(tiles);
    for t in &w.pillaged {
        if (*t as usize) < tiles {
            flags.insert(crate::ids::TileId(*t));
        }
    }
    flags
}
