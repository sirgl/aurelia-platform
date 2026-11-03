//! What a game module gives the observer: the powers of an AI-only match, each power's
//! picture of the world and its figures, the round, and the end. The shell draws and
//! paces; it never reads a rule. `ObserverMatch` (in `matches`) implements the contract
//! for any rules module, the module only describes its powers (`Describe`).
use std::time::Duration;

use render::unit_models::UnitModelBook;
use super::card::Card;
use render::world::state::RenderWorld;
use render::world_models::WorldBook;

/// The figures of a rules module's units and world objects, for the panels' renderers.
#[derive(Debug, Clone)]
pub struct Books {
    pub units: UnitModelBook,
    pub world: WorldBook,
}

/// A power of the match: its player number (the renderer colours its borders, cities and
/// units by it) and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Power {
    pub player: u8,
    pub name: String,
}

/// One figure of a power's panel header: "Score 120", "Gold 412".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub label: &'static str,
    pub value: String,
    /// How far along it is, when the figure is a progress (research): a bar is drawn.
    pub percent: Option<u8>,
}

impl Stat {
    pub fn new(label: &'static str, value: impl ToString) -> Self {
        Self { label, value: value.to_string(), percent: None }
    }

    /// A figure with a progress bar, `percent` full.
    pub fn with_percent(mut self, percent: u8) -> Self {
        self.percent = Some(percent.min(100));
        self
    }
}

/// The round the match is in, and the round it ends at when the rules cap it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Round {
    pub now: u32,
    pub cap: Option<u32>,
}

/// How the match ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finale {
    /// The names of the winning powers (none: nobody won).
    pub winners: Vec<String>,
    /// What decided it, in the module's words.
    pub cause: String,
    pub round: u32,
}

/// Something that happened to a power in a round, in names (the shell words it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The power founded a city.
    Founded(String),
    /// The power took a city; `from` is the power it was taken from, when it is a power
    /// of the match.
    Captured { city: String, from: Option<String> },
    /// The power lost a city (to `to`, when that is a power of the match).
    Lost { city: String, to: Option<String> },
    War(String),
    Peace(String),
    /// The power learned a technology.
    Learned(String),
    /// The power is gone from the match.
    Fell,
}

/// An event and the round it happened in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Happened {
    pub round: u32,
    pub event: Event,
}

/// Where the AI turns stand after a slice of work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// The round goes on.
    Working,
    /// A round just ended: the views may be refreshed.
    RoundDone,
    /// The match is over.
    Over,
}

/// What the observer opens on: the grid of every power, or one power alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Watch {
    #[default]
    All,
    One,
}

/// A unit as a power's view knows it: for a tooltip and the card of a selected unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFacts {
    /// The unit's persistent identity (`UnitMarker::id`).
    pub id: u64,
    pub tile: u32,
    /// The unit's kind, in the module's words.
    pub kind: String,
    /// The name of the power the unit serves, and its player number.
    pub owner: String,
    pub player: u8,
    /// The unit serves the power whose view is shown.
    pub own: bool,
    pub hp: u16,
    pub max_hp: u16,
    pub fortified: bool,
    /// How far the unit sees (rings of tiles). Known for the shown power's own units, and
    /// for every unit when the whole map is open; None for a stranger's unit.
    pub sight: Option<u8>,
}

/// A city as a power's view knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityEntry {
    pub tile: u32,
    pub name: String,
    /// The name of the power the city belongs to, and its player number.
    pub owner: String,
    pub player: u8,
    /// The city belongs to the power whose view is shown.
    pub own: bool,
    /// None: not known (a remembered city of an old save).
    pub population: Option<u32>,
}

/// What a unit or a city sees: the tile it stands on, how far, and the tiles it covers
/// (the rules' own sight: rings of tiles, less what a higher tile hides).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sight {
    pub at: u32,
    pub radius: u8,
    pub tiles: Vec<u32>,
}

/// What a sight is asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sees {
    /// A unit, by its identity.
    Unit(u64),
    /// A city, by its tile.
    City(u32),
}

/// A match played by the AI alone, as the observer sees it.
pub trait Observed {
    /// The powers, in panel order; fixed for the match.
    fn powers(&self) -> &[Power];
    fn alive(&self, power: usize) -> bool;
    /// The figures of the power's header, as the last round left them.
    fn standing(&self, power: usize) -> &[Stat];
    fn round(&self) -> Round;
    /// Play the AI turns for about `slice` of wall time (at least one step). The match
    /// plays out the same whatever the slices are.
    fn advance(&mut self, slice: Duration) -> Progress;
    /// A round is being played: its turns have begun and not ended.
    fn mid_round(&self) -> bool;
    fn is_over(&self) -> bool;
    fn finale(&self) -> Option<Finale>;
    /// Grows with every round that ended: what changed on screen since a number was
    /// taken.
    fn serial(&self) -> u64;
    /// Bring the power's view up to date with the world (between rounds only). Returns
    /// true when the picture changed.
    fn refresh(&mut self, power: usize) -> bool;
    /// The world through the power's eyes: only what it explored, sees and remembers.
    fn world(&self, power: usize) -> &RenderWorld;
    /// Unit and world models of the match's rules, for the panels' renderers.
    fn books(&self) -> Books;
    /// The hash of the whole world (the match's identity: it must not depend on how the
    /// match was watched).
    fn state_hash(&self) -> u64;
    /// One line for the menu: who plays, how long.
    fn summary(&self) -> String;
    /// The power the view opens on alone (None: all powers in the grid).
    fn first_view(&self) -> Option<usize> {
        None
    }
    /// What the power is working on, beyond the header's figures: its research and
    /// progress, its income. Only the power shown alone has them.
    fn details(&self, _power: usize) -> &[Stat] {
        &[]
    }
    /// Work out `details` of the power as the last round left it (between rounds only).
    fn refresh_details(&mut self, _power: usize) {}
    /// The last events of the power, the newest last.
    fn events(&self, _power: usize) -> &[Happened] {
        &[]
    }

    // --- The whole map, tooltips and cards (new methods go at the end, each with a default) ---

    /// Every power's view shows the whole map: no fog, every unit and city as it is now.
    /// The match is the same either way (`state_hash`).
    fn revealed(&self) -> bool {
        false
    }
    /// Open the whole map to every view, or close it again. The views are rebuilt at the
    /// next `refresh` (the shell calls it for the views it shows at once).
    fn set_revealed(&mut self, _on: bool) {}
    /// The power's view is as old as the world: no round is being played and the view has
    /// followed the last round. Tooltips, cards and sight read the world, so they are given
    /// only while this holds (they would otherwise tell more than the picture shows).
    fn current(&self, _power: usize) -> bool {
        true
    }
    /// The tooltip of a tile in the power's view: what the module's own tooltip says of a
    /// tile the power sees or remembers (none for a tile it has not explored), the
    /// whole of it when the map is open.
    fn tile_card(&self, _power: usize, _tile: u32) -> Option<Card> {
        None
    }
    /// The units the power's view shows on a tile.
    fn units_on(&self, _power: usize, _tile: u32) -> Vec<UnitFacts> {
        Vec::new()
    }
    /// One unit of the power's view, by identity.
    fn unit_facts(&self, _power: usize, _id: u64) -> Option<UnitFacts> {
        None
    }
    /// The power's own cities, the biggest first.
    fn cities_of(&self, _power: usize) -> Vec<CityEntry> {
        Vec::new()
    }
    /// The city the power's view shows on a tile.
    fn city_at(&self, _power: usize, _tile: u32) -> Option<CityEntry> {
        None
    }
    /// The card of the city on a tile, as the module's own city screen shows it, with no
    /// orders: the whole card for the power's own city (or any city when the map is
    /// open), only what the power knows of a foreign one.
    fn city_card(&self, _power: usize, _tile: u32) -> Option<Card> {
        None
    }
    /// What a unit or a city of the power sees (none when the view does not know it).
    fn sight(&self, _power: usize, _target: Sees) -> Option<Sight> {
        None
    }
}
