//! kernel: the generic 4X simulation (platform/PLATFORM.md), generic over the game's
//! rules module (`module::RulesModule`).
//!
//! Map and tiles, players, cities and units, war and open borders, the turn,
//! knowledge and fog, navigation, the effect engine with the kernel's
//! vocabulary, the rules compiler, the spatial derived state, saves, the
//! world hash and replay. A rules module brings everything else (its state,
//! commands, mechanics, words and tables) and instantiates the kernel with
//! itself: `GameState<Classic>`. The kernel names no module.
//!
//! Forbidden here: rendering, platform APIs, files, clocks, randomness
//! outside `rng`, and any dependency on a rules module or its content.


pub mod agent;
pub mod ai;
pub mod command;
pub mod cow;
pub mod derived;
pub mod effects;
pub mod empty;
pub mod fx;
pub mod ids;
pub mod knowledge;
pub mod math;
pub mod metrics;
pub mod module;
pub mod navigation;
pub mod numeric;
pub mod persistence;
pub mod replay;
pub mod rng;
pub mod rules;
pub mod scenario;
pub mod systems;
pub mod tile_set;
pub mod transaction;
pub mod turn;
pub mod validation;
pub mod world;

pub use fx::{FxHashMap, FxHashSet};
pub use tile_set::TileSet;

#[cfg(test)]
mod test_module;
