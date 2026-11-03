//! Navigation is a query service, not persistent derived (§18).
//! A simple A* first; persistent caches may live in DerivedWorld later.

pub mod astar;
pub mod cost;
pub mod profile;
pub mod scratch;
pub mod terrain;

pub use astar::{find_path, find_path_cost, reachable_this_turn, MoveBudget, Path, PathCost, PathRequest};
pub use profile::{Embark, MoveProfile};
pub use scratch::PathScratch;
