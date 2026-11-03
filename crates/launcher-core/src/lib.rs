//! launcher-core: everything a game launcher needs that is not a rule set. The window and the frame
//! loop (desktop), the browser page (web), the Android activity, music and sound, the `--replay`
//! report. A launcher is a thin crate that registers ONE module (its UI crate, its terrain layers,
//! its product name and developer commands) and calls into this one; there is a launcher per game
//! (`lands/launcher`, `classic/launcher`) and one for development with every module
//! (`integration/crates/dev-desktop`). This crate depends on the platform only, never on a module.

pub use client;

/// Terrain texture layers of a module's content (width, height, RGBA).
pub type Layers = Vec<(u32, u32, Vec<u8>)>;

#[cfg(not(target_arch = "wasm32"))]
pub mod audio;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub mod desktop;
pub mod replay;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub mod log_tail;

#[cfg(target_arch = "wasm32")]
pub mod web;

#[cfg(target_os = "android")]
pub mod android;
