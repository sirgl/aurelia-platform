//! Commands: the kernel's orders ([`KernelCommand`]), their validation and
//! why one is rejected. A rules module's command type adds its own orders
//! and routes the kernel's here.

// Module names follow platform/structure.md (§2: command/command.rs etc.);
// module_inception is deliberate here: structure beats the lint.
#![allow(clippy::module_inception)]

pub mod command;
pub mod error;
pub mod resolved;
pub mod validate;

pub use command::KernelCommand;
pub use error::{CommandError, Refusal, SiteProblem};
pub use resolved::ResolvedKernel;
pub use validate::validate_kernel;
