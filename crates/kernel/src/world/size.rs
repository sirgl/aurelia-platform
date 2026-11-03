//! Classic world size (`Worlds` table, classic/docs/classic/rules.md "Economy and
//! society", table Worlds). The map is generated with arbitrary
//! dimensions; its size class is the `Worlds` row whose grid area is the
//! nearest to the map's (ties: the smaller one). Pure function of the map
//! dimensions — nothing to store.

use crate::math::MapDimensions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorldSize {
    Duel,
    Tiny,
    Small,
    Standard,
    Large,
    Huge,
}

pub const WORLD_SIZES: [WorldSize; 6] = [
    WorldSize::Duel,
    WorldSize::Tiny,
    WorldSize::Small,
    WorldSize::Standard,
    WorldSize::Large,
    WorldSize::Huge,
];

impl WorldSize {
    pub fn of(dims: MapDimensions) -> Self {
        let area = i64::from(dims.width) * i64::from(dims.height);
        let mut best = WorldSize::Duel;
        let mut best_gap = i64::MAX;
        for size in WORLD_SIZES {
            let (w, h) = size.grid();
            let gap = (i64::from(w) * i64::from(h) - area).abs();
            if gap < best_gap {
                best = size;
                best_gap = gap;
            }
        }
        best
    }

    /// `GridWidth × GridHeight`.
    pub fn grid(self) -> (u32, u32) {
        match self {
            WorldSize::Duel => (40, 24),
            WorldSize::Tiny => (56, 36),
            WorldSize::Small => (66, 42),
            WorldSize::Standard => (80, 52),
            WorldSize::Large => (104, 64),
            WorldSize::Huge => (128, 80),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_is_the_nearest_grid() {
        let dims = |width, height| MapDimensions { width, height };
        assert_eq!(WorldSize::of(dims(80, 52)), WorldSize::Standard);
        assert_eq!(WorldSize::of(dims(48, 40)), WorldSize::Tiny);
        assert_eq!(WorldSize::of(dims(10, 10)), WorldSize::Duel);
        assert_eq!(WorldSize::of(dims(200, 200)), WorldSize::Huge);
    }
}
