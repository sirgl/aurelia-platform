//! New geometric architecture of the terrain (v28):
//! immutable staged pipeline Relief → Boundary → Patch → Features.
//! For now only the exact integer lattice is here (v28a); fields and patches are v28b.

pub mod evaluator;
pub mod features;
pub mod fields;
pub mod lattice;
pub use lattice::corner_lattice;
pub use lattice::{edge_lattice, key_from_world, lattice_to_world, tile_lattice, LatticeKey};
pub use lattice::{CORNER_OFFSETS, EDGE_OFFSETS};

pub mod surface;
pub(crate) mod mountains;
