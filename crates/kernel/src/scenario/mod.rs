//! Reproducible worlds (§27): the map generator (land, rivers, features,
//! natural wonders, resources) and the start of a game (nations,
//! start sites, starting units by their roles). A rules module adds its own
//! setup on top (Classic: city-states, barbarians, victories).

pub mod generator;
pub mod landscape;
pub mod map_script;
pub mod sea_resources;

pub use generator::{
    deposit_amount, generate, generate_new_game, generate_new_game_builder, generate_player_game, generate_player_game_scripted,
    generate_scripted, generate_scripted_builder, generate_scripted_builder_with, settler_kind, ScenarioConfig,
};
pub use landscape::LandOptions;
pub use map_script::MapScript;
