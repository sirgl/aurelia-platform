//! Simulation save schema (§24 platform/structure.md, §26 platform/project.md).
//!
//! A separate versioned schema, NOT a dump of internal structures: fields are simple
//! values (u16/u32/Option/Vec), without handles/generations/slot indexing.
//! Handles are not persisted (§17: runtime handle != persistent identity):
//! units and cities carry their EntityId, the vectors are in ascending id
//! order (the canonical order), and `next_entity` continues the counter.
//! Byte encoding (serde/postcard) is the next layer over these structures.

use crate::module::RulesModule;

#[derive(Debug, Clone)]
pub struct SaveHeader {
    pub save_schema_version: u32,
    pub simulation_version: u32,
    pub ruleset_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedUnit<M: RulesModule> {
    pub id: u64,
    pub tile: u32,
    pub owner: u8,
    pub kind: u16,
    pub hp: u16,
    pub moves_left: u16,
    pub moves_max: u16,
    /// Stance: (tag, fortify turns) — 0 ready, 1 fortified, 2 set up,
    /// 3 intercept.
    pub stance: (u8, u8),
    /// Construction: (tile, tag, id, work_left), tag 0 improvement, 1 road, 2 chop; work_left in hundredths of a worker turn.
    pub work: Option<(u32, u8, u16, u16)>,
    /// Promotion ids, ascending.
    pub promotions: Vec<u16>,
    /// Multi-turn move order: the target tile.
    pub goal: Option<u32>,
    pub experience: u32,
    pub level: u8,
    pub attacks_made: u8,
    pub interceptions_made: u8,
    /// The rules module's state of the unit (`world::module::ModuleUnit`):
    /// last in the record, from the module's `SAVED_SINCE` codec version.
    pub module: crate::world::module::SavedUnitOf<M>,
}

/// Timed effect (Duration::Turns). target_tag 0 = player id, 1 = city
/// position in `cities`; source_tag: 0 nation, 1 policy, 2 belief, 3 tech,
/// 4 building, 5 unit, 6 promotion; trigger = Trigger::index().
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedEffect {
    pub owner: u8,
    pub target_tag: u8,
    pub target: u32,
    pub source_tag: u8,
    pub source_id: u16,
    pub trigger: u8,
    pub slot: u16,
    pub turns_left: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedCity {
    pub id: u64,
    pub tile: u32,
    pub owner: u8,
    pub name: String,
    pub population: u32,
    /// Food in store, hundredths.
    pub food: u32,
    pub buildings: Vec<u16>,
    pub production: u32,
    /// Order queue: (tag, id), 0 = unit, 1 = building.
    pub queue: Vec<(u8, u16)>,
    /// Tiles taken off work (sorted tile ids).
    pub unworked: Vec<u32>,
    pub culture: u32,
    /// Tiles acquired (culture + purchases), the argument of culture_cost.
    pub acquired: u16,
    pub radius: u8,
    /// Damage taken (HP = max − damage).
    pub damage: u16,
    pub original_owner: u8,
    pub original_capital: bool,
    /// CityStatus: 0 own, 1 awaiting decision, 2 occupied, 3 puppet, 4 razing.
    pub status: u8,
    pub resistance_turns: u16,
    pub strike_made: bool,
    /// `CapitalConnection::Connected`.
    pub connected_to_capital: bool,
    /// Round the owner founded or acquired the city.
    pub acquired_round: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedWorldV1<M: RulesModule> {
    pub width: u32,
    pub height: u32,
    pub players: u8,
    pub terrain: Vec<u16>,
    pub feature: Vec<Option<u16>>,
    pub resource: Vec<Option<u16>>,
    /// Copies of each tile's resource (0 without one).
    pub resource_amount: Vec<u8>,
    pub owner: Vec<Option<u8>>,
    /// Owning city of each tile: position in `cities` (Some exactly on
    /// owned tiles, a city of the tile's owner).
    pub tile_city: Vec<Option<u32>>,
    pub improvement: Vec<Option<u16>>,
    /// Tiles whose improvement is pillaged, ascending.
    pub pillaged: Vec<u32>,
    /// Routes by tile: 0 none, 1 road, 2 railroad (map::Route::code).
    pub road: Vec<u8>,
    /// Natural wonder per tile (content index); one entry per tile.
    pub natural_wonder: Vec<Option<u16>>,
    /// Rivers (0/1 by tile).
    pub river: Vec<u8>,
    /// River beds: canonical edges (tile-id pairs, sorted).
    pub river_edges: Vec<(u32, u32)>,
    pub units: Vec<SavedUnit<M>>,
    pub cities: Vec<SavedCity>,
    pub diplomacy: Vec<u8>,
    /// Open-borders matrix (players x players; shorter = all closed).
    pub diplomacy_open: Vec<u8>,
    pub turn_round: u32,
    /// Discriminant of TurnPhase (0..=3); anything else is Corrupt.
    pub turn_phase: u8,
    pub active_player: u8,
    pub players_alive: Vec<bool>,
    /// Learned technologies by player (empty = none; the order is history).
    pub player_techs: Vec<Vec<u16>>,
    /// Treasury by player (empty/shorter = 0, so old saves still read).
    pub player_gold: Vec<u32>,
    /// Beakers by player + research queues (tech ids).
    pub player_science: Vec<u32>,
    pub player_research: Vec<Vec<u16>>,
    /// Nations by player (empty/shorter = 0, so old saves still read).
    pub player_nations: Vec<u8>,
    /// One explored vec per player, the length of each = tiles.
    pub knowledge_explored: Vec<Vec<u8>>,
    /// Tile memory per player, one entry per explored tile in tile order:
    /// (last seen round, improvement, feature, owner, flags) — see
    /// knowledge::state::TileMemory.
    pub knowledge_memory: Vec<Vec<(u32, u16, u16, u8, u8)>>,
    /// Contacts per player, one per player: (met round, last seen round).
    pub knowledge_contacts: Vec<Vec<(Option<u32>, Option<u32>)>>,
    /// The deposit each explored tile last showed (`u16::MAX`: none), per
    /// player, in the order of `knowledge_memory`. Empty before city sightings
    /// (`ModuleSections::SIGHTINGS_SINCE`): such a save remembers the deposit
    /// the map holds at load.
    pub knowledge_resource: Vec<Vec<u16>>,
    /// The cities each player last saw, per player: (tile, owner, name, the
    /// numbers then: population, era, buildings). Empty before sightings: a
    /// remembered city of such a save keeps its name and owner, its numbers
    /// are unknown.
    pub knowledge_cities: Vec<Vec<SavedSighting>>,
    /// States of the RNG streams (§24). Without them load desynchronises the future.
    pub rng_combat: u64,
    pub rng_events: u64,
    /// Timed effects in store order.
    pub active_effects: Vec<SavedEffect>,
    /// Next entity id (above every saved unit and city id).
    pub next_entity: u64,
    /// Economy and society section (citizen orders, golden ages, great
    /// people counters).
    pub economy: crate::persistence::economy::SavedEconomy<M>,
    /// Player kinds: (tag, city-state id) — 0 major, 1 city-state, 2 barbarians.
    pub player_kinds: Vec<(u8, u16)>,
    /// The rules module's sections (`world::module`).
    pub module: crate::world::module::SavedModule<M>,
}

/// A sighted city: tile, owner, name, and its numbers when seen (None: unknown).
pub type SavedSighting = (u32, u8, String, Option<(u32, u8, Vec<u16>)>);

#[derive(Debug, Clone)]
pub struct SaveGameV1<M: RulesModule> {
    pub header: SaveHeader,
    pub world: SavedWorldV1<M>,
    /// The AI players' memory (its own version, `persistence::ai`).
    pub ai: crate::persistence::ai::SavedAiStates,
}
