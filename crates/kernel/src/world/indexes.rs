//! Authoritative secondary indexes (§9).
//!
//! Part of the world invariant, updated inside transaction operations,
//! never by downstream listeners. Committed atomically with the primary storage.
//!
//! Order is part of the contract: the per-player lists are in entity order
//! (the order of creation), so a world restored from a save iterates them
//! exactly like the live world. A move touches only the tile index.

use crate::fx::FxHashMap;
use crate::ids::{CityHandle, PlayerId, TileId, UnitHandle};
use crate::tile_set::TileSet;

#[derive(Debug, Clone, Default)]
pub struct WorldIndexes {
    /// Units per owner, entity order (created units are the newest).
    pub(crate) units_by_player: FxHashMap<u8, Vec<UnitHandle>>,
    /// Units per tile. Order unspecified: rules must not depend on it.
    pub(crate) unit_by_tile: FxHashMap<u32, Vec<UnitHandle>>,
    /// Cities per owner, entity order. Cities are never removed, so the
    /// slot index orders them like their entity ids.
    pub cities_by_player: FxHashMap<u8, Vec<CityHandle>>,
    /// One city per tile (FoundCity checks the place).
    pub(crate) city_by_tile: FxHashMap<u32, CityHandle>,
    /// The tiles `city_by_tile` holds, as bits: "no city here" (almost every tile) is one load.
    city_tiles: TileSet,
}

impl WorldIndexes {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// A new unit (the newest entity: appended keeps entity order).
    #[inline]
    pub(crate) fn index_unit(&mut self, owner: PlayerId, tile: TileId, h: UnitHandle) {
        self.units_by_player.entry(owner.0).or_default().push(h);
        self.unit_by_tile.entry(tile.0).or_default().push(h);
    }

    /// A removed unit: an ordered remove keeps entity order of the owner.
    #[inline]
    pub(crate) fn unindex_unit(&mut self, owner: PlayerId, tile: TileId, h: UnitHandle) {
        if let Some(v) = self.units_by_player.get_mut(&owner.0) {
            if let Some(pos) = v.iter().position(|x| *x == h) {
                v.remove(pos);
            }
        }
        self.untile_unit(tile, h);
    }

    /// A unit changed tiles; its owner list does not change.
    #[inline]
    pub fn move_unit(&mut self, h: UnitHandle, from: TileId, to: TileId) {
        self.untile_unit(from, h);
        self.unit_by_tile.entry(to.0).or_default().push(h);
    }

    fn untile_unit(&mut self, tile: TileId, h: UnitHandle) {
        if let Some(v) = self.unit_by_tile.get_mut(&tile.0) {
            if let Some(pos) = v.iter().position(|x| *x == h) {
                v.swap_remove(pos);
            }
            if v.is_empty() {
                self.unit_by_tile.remove(&tile.0);
            }
        }
    }

    #[inline]
    pub fn units_of(&self, owner: PlayerId) -> &[UnitHandle] {
        self.units_by_player
            .get(&owner.0)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Units standing on `tile` (order unspecified).
    #[inline]
    pub fn units_on(&self, tile: TileId) -> &[UnitHandle] {
        self.unit_by_tile
            .get(&tile.0)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    #[inline]
    pub fn cities_of(&self, owner: PlayerId) -> &[CityHandle] {
        self.cities_by_player
            .get(&owner.0)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// A city gains an owner (founding or capture): inserted in entity
    /// (= slot) order.
    #[inline]
    pub fn index_city(&mut self, owner: PlayerId, tile: TileId, h: CityHandle) {
        let list = self.cities_by_player.entry(owner.0).or_default();
        let position = list.partition_point(|x| x.index < h.index);
        list.insert(position, h);
        self.city_by_tile.insert(tile.0, h);
        self.city_tiles.insert(tile);
    }

    /// Remove a city from the owner's list (capture): city_by_tile is untouched
    /// (the key is the tile, the owner takes no part in it).
    #[inline]
    pub fn unindex_city_owner(&mut self, owner: PlayerId, h: CityHandle) {
        if let Some(v) = self.cities_by_player.get_mut(&owner.0) {
            if let Some(pos) = v.iter().position(|x| *x == h) {
                v.remove(pos);
            }
        }
    }

    /// A razed city leaves its tile.
    #[inline]
    pub fn unindex_city_tile(&mut self, tile: TileId) {
        self.city_by_tile.remove(&tile.0);
        self.city_tiles.remove(tile);
    }

    #[inline]
    pub fn city_on_tile(&self, tile: TileId) -> Option<CityHandle> {
        if !self.city_tiles.contains(tile) {
            return None;
        }
        self.city_by_tile.get(&tile.0).copied()
    }
}
