//! The single point of authoritative mutation (§13) and its change journal
//! (§14): what a transaction changed, for derived state, knowledge and the
//! client.
//!
//! After a successful transaction AuthWorld is valid; on reject it is unchanged.
//! Strategy: validate everything -> infallible mutation (without copying the world).

// Module names follow platform/structure.md (§2); module_inception is deliberate.
#![allow(clippy::module_inception)]

pub mod journal;
pub mod operations;
pub mod transaction;

pub use journal::{ChangeJournal, CommandResult};
pub use transaction::WorldTransaction;
