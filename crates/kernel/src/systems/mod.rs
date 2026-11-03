//! The kernel's gameplay systems: generic 4X mechanics as methods of the
//! transaction, with the rules module's part at fixed points
//! ([`module::ModuleSystems`]). Systems call each other explicitly — no
//! callback or event bus (platform/project.md §10, §56); the module's reactions are
//! the one named contract.
//!
//! | module        | what                                               |
//! |---------------|----------------------------------------------------|
//! | turn          | the end-turn driver, moves, growth, timed effects  |
//! | units         | healing, fortification                             |
//! | worked_tiles  | citizen orders of a city                           |
//! | purchase      | hurry production, buy a tile                       |
//! | movement      | walks, marches and move orders                     |
//! | city_founding | a settler founds a city                            |
//! | city_naming   | the player picks a city name from its list         |
//! | production    | the city queue and its completion                  |
//! | borders       | culture grows the borders                          |
//! | economy       | gold, deficit, capital connections                 |
//! | research      | the research queue, science, learning              |
//! | improvements  | worker jobs                                        |
//! | elimination   | a player without cities and units is gone          |

pub mod borders;
pub mod city_founding;
pub mod city_naming;
pub mod economy;
pub mod elimination;
pub mod improvements;
pub mod module;
pub mod movement;
pub mod production;
pub mod purchase;
pub mod research;
pub mod turn;
pub mod units;
pub mod worked_tiles;

pub use module::{CityGrowth, KernelEvent, ModuleSystems};
