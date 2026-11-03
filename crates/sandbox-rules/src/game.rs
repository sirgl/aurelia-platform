//! A new sandbox game: the kernel's map generator and starts (each faction
//! gets its free units and the first era's starting units by role), then
//! every player at war with every other — the sandbox has no diplomacy.

use kernel::ids::PlayerId;
use kernel::scenario::generator::{generate_new_game_builder, ScenarioConfig};

use crate::{Game, Rules};

/// The shape of a new game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setup {
    pub seed: u64,
    pub players: u8,
    pub width: u32,
    pub height: u32,
    /// Percent of the map under water.
    pub water: u8,
}

impl Setup {
    /// The standard game: four factions on a 48 × 32 map.
    pub fn standard(seed: u64) -> Self {
        Self { seed, players: 4, width: 48, height: 32, water: 25 }
    }
}

/// A new game of `setup` on `rules`.
pub fn new_game(rules: &Rules, setup: Setup) -> Game {
    let config = ScenarioConfig {
        seed: setup.seed,
        players: setup.players,
        width: setup.width,
        height: setup.height,
        cities: 0,
        units: 0,
        water_ratio: setup.water,
    };
    let mut game = generate_new_game_builder(rules, &config);
    for a in 0..setup.players {
        for b in (a + 1)..setup.players {
            game.set_war(PlayerId(a), PlayerId(b), true);
        }
    }
    game.build()
}
