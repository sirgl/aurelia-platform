//! A player's subjective knowledge (§21, §45 platform/structure.md).
//! World truth / Visibility / Knowledge are three different concepts.
//!
//! Knowledge is real state (it cannot be rebuilt from the current world),
//! saved and hashed:
//! - `explored` — the tile was visible at least once;
//! - tile memory — what the player last saw of the tile's changing layers
//!   (owner, improvement, road, feature, resource, a city) and the round it
//!   last saw it. Terrain and river placement never change during a game,
//!   so a remembered tile reads them from the map (`PlayerView`). A resource
//!   does change (a rules module may create a deposit), so the memory keeps
//!   the one the player last saw, whatever technology it knew then;
//! - city sightings — the name, owner, population, era and buildings of each
//!   city the player last saw, by tile. A remembered city is drawn from its
//!   sighting, never from the city list of the world now;
//! - contacts — when the player met each other player and last saw one of
//!   its units or cities (diplomacy reads this, never the truth).
//!
//! The one writer is `knowledge::sync`, run after every journal.

use std::collections::BTreeMap;

use crate::ids::{FeatureId, ImprovementId, PlayerId, ResourceId, TileId};
use crate::tile_set::TileSet;
use crate::world::map::Route;

const NONE_U16: u16 = u16::MAX;
const NONE_U8: u8 = u8::MAX;
const ROAD: u8 = 1;
const CITY: u8 = 2;
/// With ROAD: the route is a railroad (saves from before railroads never set it).
const RAILROAD: u8 = 4;
/// Every flag a saved memory may carry.
const FLAGS_MASK: u8 = ROAD | CITY | RAILROAD;

/// The changing layers of one tile as the player last saw them, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileMemory {
    /// Last round the tile was visible (meaningful once explored).
    pub(crate) seen_round: u32,
    improvement: u16,
    feature: u16,
    resource: u16,
    owner: u8,
    flags: u8,
}

impl Default for TileMemory {
    fn default() -> Self {
        Self {
            seen_round: 0,
            improvement: NONE_U16,
            feature: NONE_U16,
            resource: NONE_U16,
            owner: NONE_U8,
            flags: 0,
        }
    }
}

/// The remembered layers of a tile (unpacked).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RememberedLayers {
    pub improvement: Option<ImprovementId>,
    pub feature: Option<FeatureId>,
    /// The deposit as seen. Kept for the save layout only: `KnowledgeView::tile` reads the deposit of an
    /// explored tile from the live map (R355), never from here.
    pub resource: Option<ResourceId>,
    pub owner: Option<PlayerId>,
    pub route: Route,
    pub city: bool,
}

impl TileMemory {
    pub(crate) fn from_layers(layers: RememberedLayers, seen_round: u32) -> Self {
        let route = match layers.route {
            Route::None => 0,
            Route::Road => ROAD,
            Route::Railroad => ROAD | RAILROAD,
        };
        let flags = route | if layers.city { CITY } else { 0 };
        Self {
            seen_round,
            improvement: layers.improvement.map(|i| i.0).unwrap_or(NONE_U16),
            feature: layers.feature.map(|f| f.0).unwrap_or(NONE_U16),
            resource: layers.resource.map(|r| r.0).unwrap_or(NONE_U16),
            owner: layers.owner.map(|p| p.0).unwrap_or(NONE_U8),
            flags,
        }
    }

    pub fn layers(&self) -> RememberedLayers {
        RememberedLayers {
            improvement: (self.improvement != NONE_U16).then_some(ImprovementId(self.improvement)),
            feature: (self.feature != NONE_U16).then_some(FeatureId(self.feature)),
            resource: (self.resource != NONE_U16).then_some(ResourceId(self.resource)),
            owner: (self.owner != NONE_U8).then_some(PlayerId(self.owner)),
            route: match (self.flags & ROAD != 0, self.flags & RAILROAD != 0) {
                (false, _) => Route::None,
                (true, false) => Route::Road,
                (true, true) => Route::Railroad,
            },
            city: self.flags & CITY != 0,
        }
    }

    pub fn seen_round(&self) -> u32 {
        self.seen_round
    }

    /// The remembered city is gone (a save from before sightings kept the flag of a
    /// city that no longer stands).
    pub(crate) fn clear_city(&mut self) {
        self.flags &= !CITY;
    }

    /// Whether saved flags hold only the flags above (a railroad is road and rail).
    pub(crate) fn flags_valid(flags: u8) -> bool {
        flags & !FLAGS_MASK == 0 && (flags & RAILROAD == 0 || flags & ROAD != 0)
    }

    /// Save/hash form: (round, improvement, feature, owner, flags).
    pub(crate) fn packed(&self) -> (u32, u16, u16, u8, u8) {
        (self.seen_round, self.improvement, self.feature, self.owner, self.flags)
    }

    /// The last seen deposit (`NONE_U16`: none), saved beside the packed entry.
    pub(crate) fn resource_code(&self) -> u16 {
        self.resource
    }

    /// The saved entry and the deposit saved beside it.
    pub(crate) fn from_packed(p: (u32, u16, u16, u8, u8), resource: u16) -> Self {
        Self {
            seen_round: p.0,
            improvement: p.1,
            feature: p.2,
            resource,
            owner: p.3,
            flags: p.4,
        }
    }
}

/// The numbers of a city when the player last saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SightedCity {
    pub population: u32,
    /// The owner's era then (the city kit's style).
    pub era: u8,
    /// Building kinds, ascending: the silhouette.
    pub buildings: Vec<u16>,
}

/// A city as the player last saw it. A save from before sightings knows the
/// city's owner and name only: `seen` is None, "unknown", never a zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitySighting {
    pub owner: PlayerId,
    pub name: String,
    pub seen: Option<SightedCity>,
}

/// What a player knows about another player (Classic: met; last seen).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Contact {
    /// Round of the first meeting (None: never met).
    pub met_round: Option<u32>,
    /// Last round a unit or city of the other player was in sight.
    pub last_seen_round: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerKnowledge {
    /// The tiles that were in sight at least once (explored); the map's tile count is
    /// the length of `memory`.
    explored: TileSet,
    memory: crate::cow::Chunked<TileMemory>,
    /// The cities last seen, by tile (ascending: saved and hashed in this order).
    sightings: BTreeMap<u32, CitySighting>,
    contacts: Vec<Contact>,
}

impl PlayerKnowledge {
    pub fn new(tiles: usize, players: usize) -> Self {
        Self {
            explored: TileSet::with_tile_count(tiles),
            memory: crate::cow::Chunked::filled(tiles, TileMemory::default()),
            sightings: BTreeMap::new(),
            contacts: vec![Contact::default(); players],
        }
    }

    pub(crate) fn from_parts(
        explored: TileSet,
        memory: Vec<TileMemory>,
        sightings: BTreeMap<u32, CitySighting>,
        contacts: Vec<Contact>,
    ) -> Self {
        Self {
            explored,
            memory: crate::cow::Chunked::from_vec_sharing_blank(memory, TileMemory::default()),
            sightings,
            contacts,
        }
    }

    pub fn is_explored(&self, tile: TileId) -> bool {
        self.explored.contains(tile)
    }

    /// Every explored tile, ascending.
    pub fn explored_tiles(&self) -> &TileSet {
        &self.explored
    }

    /// Number of explored tiles.
    pub fn explored_count(&self) -> usize {
        self.explored.len()
    }

    /// One byte per tile, 1 for an explored tile (the save's form).
    pub fn explored_flags(&self) -> Vec<u8> {
        let mut flags = vec![0u8; self.memory.len()];
        for tile in self.explored.iter() {
            if let Some(flag) = flags.get_mut(tile.0 as usize) {
                *flag = 1;
            }
        }
        flags
    }

    /// The memory of an explored tile (None for an unexplored one).
    pub fn memory(&self, tile: TileId) -> Option<&TileMemory> {
        if !self.is_explored(tile) {
            return None;
        }
        self.memory.get(tile.0 as usize)
    }

    pub fn contact(&self, other: PlayerId) -> Contact {
        self.contacts.get(other.0 as usize).copied().unwrap_or_default()
    }

    pub fn contacts(&self) -> &[Contact] {
        &self.contacts
    }

    pub(crate) fn memories(&self) -> &crate::cow::Chunked<TileMemory> {
        &self.memory
    }

    /// The city last seen on `tile` (None: no city seen there, or a save before
    /// sightings whose city is gone).
    pub fn sighting(&self, tile: TileId) -> Option<&CitySighting> {
        self.sightings.get(&tile.0)
    }

    /// Every city last seen, by tile, ascending.
    pub fn sightings(&self) -> impl Iterator<Item = (TileId, &CitySighting)> {
        self.sightings.iter().map(|(tile, sighting)| (TileId(*tile), sighting))
    }

    /// The tile is visible now: remember its layers as of `round`, and the
    /// city on it (`city`: its sighting, None when the layers hold no city).
    pub(crate) fn remember(&mut self, tile: TileId, layers: RememberedLayers, city: Option<CitySighting>, round: u32) {
        let index = tile.0 as usize;
        if let Some(slot) = self.memory.get_mut(index) {
            self.explored.insert(tile);
            *slot = TileMemory::from_layers(layers, round);
            match city {
                Some(sighting) if layers.city => self.sightings.insert(tile.0, sighting),
                _ => self.sightings.remove(&tile.0),
            };
        }
    }

    /// The tile was in sight until now: a remembered city on it is what stands
    /// there now (`Some`), or is gone from the memory (`None`: the observer saw it vanish).
    /// A tile without a remembered city is left alone.
    pub(crate) fn refresh_sighting(&mut self, tile: TileId, city: Option<CitySighting>) {
        let Some(slot) = self.sightings.get_mut(&tile.0) else {
            return;
        };
        match city {
            Some(city) => *slot = city,
            None => {
                self.sightings.remove(&tile.0);
                if let Some(memory) = self.memory.get_mut(tile.0 as usize) {
                    memory.clear_city();
                }
            }
        }
    }

    /// The tile just left sight: it was last seen in `round`.
    pub(crate) fn stamp(&mut self, tile: TileId, round: u32) {
        if let Some(slot) = self.memory.get_mut(tile.0 as usize) {
            slot.seen_round = round;
        }
    }

    /// A unit or city of `other` is in sight in `round`.
    pub(crate) fn saw(&mut self, other: PlayerId, round: u32) {
        if let Some(contact) = self.contacts.get_mut(other.0 as usize) {
            contact.met_round.get_or_insert(round);
            contact.last_seen_round = Some(round);
        }
    }

    /// `other` met this player (meeting is mutual, sight is not).
    pub(crate) fn met(&mut self, other: PlayerId, round: u32) {
        if let Some(contact) = self.contacts.get_mut(other.0 as usize) {
            contact.met_round.get_or_insert(round);
        }
    }
}

#[cfg(test)]
mod flag_tests {
    use super::*;

    #[test]
    fn a_saved_railroad_memory_is_valid_and_a_rail_without_road_is_not() {
        assert!(TileMemory::flags_valid(ROAD | RAILROAD | CITY));
        assert!(TileMemory::flags_valid(0));
        assert!(!TileMemory::flags_valid(RAILROAD));
        assert!(!TileMemory::flags_valid(8));
    }

    fn seen_city(population: u32) -> CitySighting {
        CitySighting {
            owner: PlayerId(1),
            name: "Rome".into(),
            seen: Some(SightedCity { population, era: 0, buildings: vec![3] }),
        }
    }

    fn knowing_a_city() -> (PlayerKnowledge, TileId) {
        let mut knowledge = PlayerKnowledge::new(4, 2);
        let layers = RememberedLayers {
            improvement: None,
            feature: None,
            resource: None,
            owner: Some(PlayerId(1)),
            route: Route::None,
            city: true,
        };
        knowledge.remember(TileId(2), layers, Some(seen_city(3)), 5);
        (knowledge, TileId(2))
    }

    #[test]
    fn a_remembered_city_follows_its_last_sight_and_vanishes_when_seen_gone() {
        let (mut knowledge, tile) = knowing_a_city();
        knowledge.refresh_sighting(tile, Some(seen_city(5)));
        assert_eq!(knowledge.sighting(tile), Some(&seen_city(5)));
        knowledge.refresh_sighting(tile, None);
        assert_eq!(knowledge.sighting(tile), None, "the observer saw the city go");
        assert!(!knowledge.memory(tile).unwrap().layers().city, "the tile no longer remembers a city");
    }

    #[test]
    fn a_tile_without_a_remembered_city_is_not_given_one() {
        let (mut knowledge, _) = knowing_a_city();
        knowledge.refresh_sighting(TileId(1), Some(seen_city(9)));
        assert_eq!(knowledge.sighting(TileId(1)), None);
        assert_eq!(knowledge.sightings().count(), 1);
    }
}
