use crate::module::RulesModule;
use crate::ids::TileId;
use crate::persistence::schema::{SaveGameV1, SaveHeader, SavedCity, SavedUnit, SavedWorldV1};
use crate::rules::CompiledRuleset;
use crate::persistence::codec::ModuleSections;
use crate::world::module::{ModuleUnit, ModuleWorld};
use crate::world::state::GameState;

/// Save schema version. Content indexes (buildings/units/...) shift
/// when rules are added; an old save must fail loudly instead of
/// reading granary as bank. 16: +11 city buildings; 17: +3 tile improvements
/// (mine 1->3 in sorted order); 18: +3 units with tech gates
/// (settler 0->2, warrior 1->4); 19: +3 wonders (shrine 8->11);
/// 20: unit promotions + timed effects (effect system M3.0);
/// 21: turn_round is the global round (was a per-EndTurn counter);
/// 22: tile_city — the owning city of every owned tile;
/// 23: entity ids of units and cities, next_entity (identity survives load);
/// 24: unit moves in move points (MOVE_DENOMINATOR = 60 per point);
/// 25: unit goal (multi-turn move order);
/// 26: player knowledge — tile memory and contacts;
/// 27: route kinds (road code 2 = railroad) and natural wonders per tile;
/// 28: tundra (terrain code 7) from the Classic-style generator;
/// 29: culture pools and social policies (`culture`), city culture level
/// counts culture growths only;
/// 30: faith, pantheons, religions, religious pressure (`faith`);
/// 31: great works, influence, public opinion, archaeology (`tourism`);
/// 32: M4 track C on top of track B — combat (unit stance, experience,
/// level, attack counters; city damage, original owner and capital,
/// conquest status, resistance; player kinds; great general points),
/// diplomacy (treaties, embassies, pacts, research agreements,
/// friendships, denunciations, warmonger memory, trades, offers),
/// barbarians, city-states, victories;
/// 33: the World Congress;
/// 34: espionage;
/// 35: M4-A economy section (citizen orders, food store in hundredths,
/// great people, golden ages, happiness snapshot, game settings,
/// processes) and resource deposit amounts;
/// 36: trade routes (in the economy section);
/// 37: worker jobs keep their work left in hundredths of worker-turns;
/// 38: natural wonders found per player (economy section);
/// 39: owed free technologies and great people (in the economy section);
/// 40: World Congress projects, their rewards, league project processes;
/// 41: a city's capital connection and its paid first-conversion bonus;
/// 42: the Patronage finisher's countdown per city-state friend;
/// 43: the round each city was founded or acquired;
/// 44: pillaged improvements (tiles).
pub const SAVE_SCHEMA_VERSION: u32 = 44;
pub const SIMULATION_VERSION: u32 = 1;

/// The module's sections, with the digest of the rules when the module keeps one.
fn module_sections<M: RulesModule>(
    auth: &crate::world::state::AuthWorld<M>,
    rules: &CompiledRuleset<M>,
) -> crate::world::module::SavedModule<M> {
    let mut sections = auth.module().encode(auth);
    sections.keep_rules_digest(|| rules.digest());
    sections
}

pub fn encode<M: RulesModule>(game: &GameState<M>, rules: &CompiledRuleset<M>) -> SaveGameV1<M> {
    let view = game.view();
    let auth = view.auth;
    let n = auth.map().tile_count();
    let mut terrain = Vec::with_capacity(n);
    let mut feature = Vec::with_capacity(n);
    let mut resource = Vec::with_capacity(n);
    let mut resource_amount = Vec::with_capacity(n);
    let mut owner = Vec::with_capacity(n);
    let mut tile_city = Vec::with_capacity(n);
    let city_ordinals = city_ordinals(auth);
    let mut improvement = Vec::with_capacity(n);
    let mut road = Vec::with_capacity(n);
    let mut pillaged = Vec::new();
    let mut natural_wonder = Vec::with_capacity(n);
    let mut river = Vec::with_capacity(n);
    for i in 0..n {
        let t = TileId(i as u32);
        terrain.push(auth.map().terrain(t).map(|x| x.0).unwrap_or(u16::MAX));
        feature.push(auth.map().feature(t).and_then(|o| o).map(|x| x.0));
        resource.push(auth.map().resource(t).and_then(|o| o).map(|x| x.0));
        resource_amount.push(auth.map().resource_amount(t));
        owner.push(auth.map().owner(t).and_then(|o| o).map(|p| p.0));
        tile_city.push(auth.map().owning_city(t).map(|c| city_ordinals[c.index as usize]));
        improvement.push(auth.map().improvement(t).and_then(|o| o).map(|x| x.0));
        road.push(auth.map().route(t).code());
        if auth.map().pillaged(t) {
            pillaged.push(t.0);
        }
        natural_wonder.push(auth.map().natural_wonder(t).map(|w| w.0));
        river.push(auth.map().river(t) as u8);
    }
    // The order is the slot iteration order (deterministic).
    let units = auth
        .units()
        .iter_handles()
        .map(|(h, c)| SavedUnit {
            id: c.id.0,
            tile: c.tile.0,
            owner: c.owner.0,
            kind: c.kind.0,
            hp: c.hp,
            moves_left: c.moves_left,
            moves_max: c.moves_max,
            stance: encode_stance(c.stance),
            work: c.work.map(|w| {
                let (tag, id) = match w.target {
                    crate::ids::WorkTarget::Improvement(imp) => (0u8, imp.0),
                    crate::ids::WorkTarget::Road => (1u8, 0u16),
                    crate::ids::WorkTarget::Chop(feat) => (2u8, feat.0),
                };
                (w.tile.0, tag, id, w.work_left)
            }),
            promotions: crate::persistence::effects::encode_promotions(&c.promotions),
            goal: c.goal.map(|t| t.0),
            experience: c.experience,
            level: c.level,
            attacks_made: c.attacks_made,
            interceptions_made: c.interceptions_made,
            module: auth.units().module_of(h).map(ModuleUnit::encode).unwrap_or_default(),
        })
        .collect();
    let cities = auth
        .cities()
        .iter()
        .map(|(_, c)| SavedCity {
            id: c.id.0,
            tile: c.tile.0,
            owner: c.owner.0,
            name: c.name.clone(),
            population: c.population,
            food: c.food_times100,
            buildings: c.buildings.iter().map(|b| b.0).collect(),
            production: c.production,
            queue: c
                .queue
                .iter()
                .map(|item| match item {
                    crate::ids::ProductionItem::Unit(k) => (0u8, k.0),
                    crate::ids::ProductionItem::Building(b) => (1u8, b.0),
                    crate::ids::ProductionItem::Project(p) => (2u8, p.0),
                })
                .collect(),
            unworked: c.unworked.iter().map(|t| t.0).collect(),
            culture: c.culture,
            acquired: c.acquired,
            radius: c.radius,
            damage: c.damage,
            original_owner: c.original_owner.0,
            original_capital: c.original_capital,
            status: encode_status(c.status),
            resistance_turns: c.resistance_turns,
            strike_made: c.strike_made,
            connected_to_capital: c.capital_connection
                == crate::world::city::CapitalConnection::Connected,
            acquired_round: c.acquired_round,
        })
        .collect();
    let players = auth.players().count();
    SaveGameV1 {
        header: SaveHeader {
            save_schema_version: SAVE_SCHEMA_VERSION,
            simulation_version: SIMULATION_VERSION,
            ruleset_hash: rules.hash(),
        },
        world: SavedWorldV1 {
            width: auth.map().dimensions.width,
            height: auth.map().dimensions.height,
            players: players as u8,
            terrain,
            feature,
            resource,
            resource_amount,
            owner,
            tile_city,
            improvement,
            pillaged,
            road,
            natural_wonder,
            river,
            river_edges: auth.map().river_edges().to_vec(),
            units,
            cities,
            diplomacy: auth.diplomacy().matrix().to_vec(),
            diplomacy_open: auth.diplomacy().open_matrix().to_vec(),
            turn_round: auth.turn().round,
            turn_phase: match auth.turn().phase {
                crate::turn::TurnPhase::Start => 0,
                crate::turn::TurnPhase::PlayerActions => 1,
                crate::turn::TurnPhase::Resolution => 2,
                crate::turn::TurnPhase::End => 3,
            },
            active_player: auth.active_player().0,
            players_alive: (0..players)
                .map(|i| auth.players().is_alive(crate::ids::PlayerId(i as u8)))
                .collect(),
            player_techs: (0..players)
                .map(|i| {
                    auth.players()
                        .techs_of(crate::ids::PlayerId(i as u8))
                        .iter()
                        .map(|t| t.0)
                        .collect()
                })
                .collect(),
            player_gold: (0..players)
                .map(|i| auth.players().treasury_of(crate::ids::PlayerId(i as u8)))
                .collect(),
            player_science: (0..players)
                .map(|i| auth.players().science_of(crate::ids::PlayerId(i as u8)))
                .collect(),
            player_research: (0..players)
                .map(|i| {
                    auth.players()
                        .research_queue_of(crate::ids::PlayerId(i as u8))
                        .iter()
                        .map(|t| t.0)
                        .collect()
                })
                .collect(),
            player_nations: (0..players)
                .map(|i| auth.players().nation_of(crate::ids::PlayerId(i as u8)))
                .collect(),
            knowledge_explored: auth
                .knowledge()
                .iter()
                .map(|k| k.explored_flags())
                .collect(),
            knowledge_memory: auth.knowledge().iter().map(encode_memory).collect(),
            knowledge_resource: auth.knowledge().iter().map(encode_resources).collect(),
            knowledge_cities: auth.knowledge().iter().map(encode_sightings).collect(),
            knowledge_contacts: auth
                .knowledge()
                .iter()
                .map(|k| k.contacts().iter().map(|c| (c.met_round, c.last_seen_round)).collect())
                .collect(),
            rng_combat: auth.rng().combat.state(),
            rng_events: auth.rng().events.state(),
            active_effects: crate::persistence::effects::encode_effects(auth),
            next_entity: auth.next_entity().0,
            economy: crate::persistence::economy::encode(auth),
            player_kinds: (0..players)
                .map(|i| auth.players().kind_of(crate::ids::PlayerId(i as u8)).code())
                .collect(),
            module: module_sections(auth, rules),
        },
        // The AI memory belongs to the AI driver, which fills it in.
        ai: crate::persistence::ai::SavedAiStates::default(),
    }
}

pub(crate) fn encode_stance(stance: crate::world::unit::Stance) -> (u8, u8) {
    use crate::world::unit::Stance;
    match stance {
        Stance::Ready => (0, 0),
        Stance::Fortified { turns } => (1, turns),
        Stance::SetUp => (2, 0),
        Stance::Intercept => (3, 0),
    }
}

pub(crate) fn encode_status(status: crate::world::city::CityStatus) -> u8 {
    use crate::world::city::CityStatus;
    match status {
        CityStatus::Own => 0,
        CityStatus::AwaitingDecision => 1,
        CityStatus::Occupied => 2,
        CityStatus::Puppet => 3,
        CityStatus::Razing => 4,
    }
}

/// Position of every live city in store order, indexed by the handle's
/// slot (the save refers to cities by this position).
pub fn city_ordinals<M: RulesModule>(auth: &crate::world::state::AuthWorld<M>) -> Vec<u32> {
    let mut ordinals = vec![u32::MAX; auth.cities().capacity()];
    for (ordinal, (handle, _)) in auth.cities().iter().enumerate() {
        ordinals[handle.index as usize] = ordinal as u32;
    }
    ordinals
}

/// Memory of the explored tiles, in tile order.
fn encode_memory(knowledge: &crate::knowledge::PlayerKnowledge) -> Vec<(u32, u16, u16, u8, u8)> {
    let memories = knowledge.memories();
    knowledge
        .explored_tiles()
        .iter()
        .filter_map(|tile| memories.get(tile.0 as usize))
        .map(|memory| memory.packed())
        .collect()
}

/// The deposit each explored tile last showed, in the order of `encode_memory`.
fn encode_resources(knowledge: &crate::knowledge::PlayerKnowledge) -> Vec<u16> {
    let memories = knowledge.memories();
    knowledge
        .explored_tiles()
        .iter()
        .filter_map(|tile| memories.get(tile.0 as usize))
        .map(|memory| memory.resource_code())
        .collect()
}

/// The cities the player last saw, by tile.
fn encode_sightings(knowledge: &crate::knowledge::PlayerKnowledge) -> Vec<crate::persistence::schema::SavedSighting> {
    knowledge
        .sightings()
        .map(|(tile, city)| {
            let seen = city.seen.as_ref().map(|s| (s.population, s.era, s.buildings.clone()));
            (tile.0, city.owner.0, city.name.clone(), seen)
        })
        .collect()
}
