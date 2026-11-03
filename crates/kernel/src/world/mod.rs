//! Authoritative state (§6 platform/structure.md).
//!
//! Contract: `&mut UnitStore/CityStore/MapState` cannot be obtained from
//! ordinary gameplay code. Reading goes through read-only views, writing
//! only through the transaction API.

pub mod building_rules;
pub mod builder;
pub mod builds;
pub mod citizens;
pub mod city;
pub mod city_economy;
pub(crate) mod construction;
pub mod diplomacy;
pub mod effects;
pub mod facts;
#[cfg(any(test, feature = "harness"))]
mod harness;
pub mod game_settings;
pub mod indexes;
pub mod map;
pub mod module;
pub mod player;
pub mod stacking;
pub mod size;
pub mod state;
pub mod territory;
pub mod unit;

pub use builder::WorldBuilder;
pub use city::CityStore;
pub use diplomacy::DiplomacyState;
pub use indexes::WorldIndexes;
pub use map::MapState;
pub use player::PlayerStore;
pub use state::{AuthWorld, GameConfig, GameState, GameView, Trial};
pub use unit::{UnitStore, UnitView};
