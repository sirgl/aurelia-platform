//! The observer mode: a match played by the AI alone, watched through every power's eyes
//! at once. The screen is a grid of panels, one per power, each drawing the map as that
//! power knows it (its fog, its explored and remembered land, the units and cities it
//! sees); a click or a digit enlarges one. Time is the viewer's: pause, eight speeds and
//! a step of one round.
//!
//! The mode belongs to the shell and runs over any rules module through the contract in
//! `contract` (`ModuleUi::observed`). The platform implements the match for every module
//! (`matches::ObserverMatch`), the module names its powers and figures (`Describe`).
//! Watching changes nothing in the match: the speed is only the pause between rounds,
//! and the views are projections that read the world.
pub mod capture;
pub mod card;
pub mod chips;
pub mod chronicle;
pub mod clock_bar;
pub mod contract;
pub mod finale;
pub mod fit;
pub mod frame;
pub mod grid;
pub mod header;
pub mod inspect;
pub mod matches;
pub mod overlay;
pub mod panels;
pub mod scene;
pub mod select;
pub mod shell;
pub mod single;
pub mod speed;
pub mod widget;
pub mod words;

pub use card::{Card, Line, Row, Section, Tone};
pub use contract::{Books, CityEntry, Event, Finale, Happened, Observed, Power, Progress, Round, Sees, Sight, Stat, UnitFacts, Watch};
pub use matches::{study, Describe, ObserverMatch, Study};
