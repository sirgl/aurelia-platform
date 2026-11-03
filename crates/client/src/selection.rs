//! What the player selected: unit/city/tile. Pure UI state (§34):
//! selection never mutates the simulation, it only reads for orders.

use kernel::ids::{CityHandle, TileId, UnitHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Selection {
    #[default]
    None,
    Unit(UnitHandle),
    City(CityHandle),
    Tile(TileId),
}

impl Selection {
    pub fn selected_unit(&self) -> Option<UnitHandle> {
        match *self {
            Selection::Unit(u) => Some(u),
            _ => None,
        }
    }

    pub fn clear(&mut self) {
        *self = Selection::None;
    }
}
