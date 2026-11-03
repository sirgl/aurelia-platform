//! client: orchestration layer (§34-37).
//! Here input/simulation/AI/render-projection/renderer/UI come together.
//! Forbidden: calculate_city_yield/resolve_combat/can_unit_enter (§36).
//! Write only through Command; core stays synchronous.
//! `shell` is the application shell (frame, screen stack, HUD, settings, sound, saves)
//! and the contract of the interface module `shell::module::ModuleUi`; not a single classic-module mechanic.

pub mod ai_round;
pub mod controller;
pub mod game;
pub mod human;
pub mod input;
pub mod overlays;
pub mod perf;
pub mod selection;
pub mod simulation_driver;
pub mod ui_model;

pub use game::GameClient;
pub mod shell;
