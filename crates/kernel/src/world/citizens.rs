//! A city's standing orders for its citizens (authoritative, saved and
//! hashed): the focus of the automatic assignment, tiles the player makes the
//! city work first, and specialists the player assigns by hand. Which tiles
//! are actually worked is derived from these orders (`derived::citizens`).

use crate::ids::TileId;
use crate::rules::content::SpecialistId;

/// What the automatic citizen assignment favours (Classic city focus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CityFocus {
    #[default]
    Default,
    Food,
    Production,
    Gold,
    Science,
    Culture,
    /// The specialists' points (Classic: great people).
    SpecialistPoints,
    /// The module's yield (Classic: faith).
    ModuleYield,
}

impl CityFocus {
    pub const ALL: [CityFocus; 8] = [
        CityFocus::Default,
        CityFocus::Food,
        CityFocus::Production,
        CityFocus::Gold,
        CityFocus::Science,
        CityFocus::Culture,
        CityFocus::SpecialistPoints,
        CityFocus::ModuleYield,
    ];

    /// Stable code for the save and the hash.
    pub fn code(self) -> u8 {
        match self {
            CityFocus::Default => 0,
            CityFocus::Food => 1,
            CityFocus::Production => 2,
            CityFocus::Gold => 3,
            CityFocus::Science => 4,
            CityFocus::Culture => 5,
            CityFocus::SpecialistPoints => 6,
            CityFocus::ModuleYield => 7,
        }
    }

    pub fn from_code(code: u8) -> Option<CityFocus> {
        CityFocus::ALL.get(code as usize).copied()
    }
}

/// The bit of the saved focus byte that holds `CitizenPlan::avoid_growth`.
pub const AVOID_GROWTH_BIT: u8 = 0x80;

/// Upper bound of hand-assigned specialists of one kind (Classic slots per
/// building are 1–3; a city has a few buildings per kind).
pub const MAX_FORCED_SPECIALISTS: u8 = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CitizenPlan {
    pub focus: CityFocus,
    /// The city does not grow (Classic «Avoid Growth»): at the threshold the
    /// store stays full, and the automatic assignment asks for no food
    /// beyond what the citizens eat. Saved in the focus byte
    /// (`AVOID_GROWTH_BIT`), so a save without it reads as before.
    pub avoid_growth: bool,
    /// Tiles worked before any automatic choice, ascending tile id.
    pub locked: Vec<TileId>,
    /// Hand-assigned specialists (kind, count > 0), ascending kind.
    pub specialists: Vec<(SpecialistId, u8)>,
}

impl CitizenPlan {
    /// Lock a tile, or unlock it when it is locked. Returns the new state.
    pub(crate) fn toggle_lock(&mut self, tile: TileId) -> bool {
        match self.locked.binary_search_by_key(&tile.0, |t| t.0) {
            Ok(at) => {
                self.locked.remove(at);
                false
            }
            Err(at) => {
                self.locked.insert(at, tile);
                true
            }
        }
    }

    pub fn unlock(&mut self, tile: TileId) {
        if let Ok(at) = self.locked.binary_search_by_key(&tile.0, |t| t.0) {
            self.locked.remove(at);
        }
    }

    /// Hand-assigned count of one specialist kind.
    pub fn forced(&self, kind: SpecialistId) -> u8 {
        match self.specialists.binary_search_by_key(&kind, |(k, _)| *k) {
            Ok(at) => self.specialists[at].1,
            Err(_) => 0,
        }
    }

    pub(crate) fn set_forced(&mut self, kind: SpecialistId, count: u8) {
        match self.specialists.binary_search_by_key(&kind, |(k, _)| *k) {
            Ok(at) if count == 0 => {
                self.specialists.remove(at);
            }
            Ok(at) => self.specialists[at].1 = count,
            Err(_) if count == 0 => {}
            Err(at) => self.specialists.insert(at, (kind, count)),
        }
    }
}
