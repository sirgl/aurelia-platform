//! Derived state: rebuilt from auth + rules (§15).
//! Contract: rebuild(auth) == incremental(auth, prev, changes).

pub mod citizens;
pub mod connections;
pub mod connectivity;
pub mod movement;
pub mod state;
pub mod tile_yields;
pub mod visibility;
pub mod yields;

pub use state::DerivedWorld;
