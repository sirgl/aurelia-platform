//! Simulation save schema, not a filesystem API (§24).
//! GameState <-> save representation; where it is written is the host's
//! choice. The rules module saves its own sections (`ModuleWorld::Saved`).

pub mod ai;
pub mod check;
pub mod codec;
pub mod decode;
pub mod economy;
pub mod effects;
pub mod encode;
pub mod load;
pub mod schema;

pub use decode::DecodeError;
pub use schema::{
    SaveGameV1, SaveHeader, SavedCity, SavedEffect, SavedSighting, SavedUnit, SavedWorldV1,
};
