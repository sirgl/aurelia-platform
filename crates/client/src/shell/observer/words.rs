//! The observer's plain words (`shell.observer.*` in the catalogue); `ShellWords` carries them
//! (`ShellWords::observer`). Words with names or numbers (`shell.observer.round-of`, the event
//! lines, the winner) are asked from the catalogue with named arguments where they are shown.

/// The observer mode: the panels of the powers, the speed bar and the end of the match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObserverWords {
    /// The entry on the new game screen, and its tooltip.
    pub entry: &'static str,
    pub entry_tip: &'static str,
    pub pause: &'static str,
    pub resume: &'static str,
    pub step: &'static str,
    /// Next to the round while the match is paused.
    pub paused: &'static str,
    /// The speeds, slowest first, the last is «as fast as possible».
    pub speeds: [&'static str; 8],
    pub slower_tip: &'static str,
    pub faster_tip: &'static str,
    pub step_tip: &'static str,
    pub pause_tip: &'static str,
    /// A fallen power's tag.
    pub fallen: &'static str,
    /// The bar's hint of the grid, and of one power alone.
    pub grid_hint: &'static str,
    pub zoom_hint: &'static str,
    /// The buttons that go to one power and back to the grid, with their tips.
    pub view_single: &'static str,
    pub view_grid: &'static str,
    pub view_single_tip: &'static str,
    pub view_grid_tip: &'static str,
    /// The camera of one power: it follows the power's land, or the viewer moves it.
    pub camera_follow: &'static str,
    pub camera_free: &'static str,
    pub camera_follow_tip: &'static str,
    pub camera_free_tip: &'static str,
    /// The new game screen's choice of what to watch.
    pub watch_all: &'static str,
    pub watch_one: &'static str,
    pub watch_tip: &'static str,
    /// The events of one power: the title, the empty list, `{}` round and what happened.
    pub events_title: &'static str,
    pub no_events: &'static str,
    pub ev_fell: &'static str,
    /// The match ended: the card's title, then the lines.
    pub finale_title: &'static str,
    pub nobody: &'static str,
    pub cannot_play: &'static str,
    pub keep_playing: &'static str,
    pub to_menu: &'static str,
    pub final_table: &'static str,
    /// The menu's note for a saved match, which an observed match cannot be.
    pub no_save: &'static str,
    /// The whole map open: the button, its tip, and the tag over every panel while it is.
    pub reveal: &'static str,
    pub reveal_tip: &'static str,
    pub reveal_tag: &'static str,
    /// The button that opens the cards of a power's cities, and its tip.
    pub cities: &'static str,
    pub cities_tip: &'static str,
    /// Under a city's card: `{}` the city's place among the power's cities, then how many; the
    /// keys; the tip of the close button.
    pub card_hint: &'static str,
    pub card_close: &'static str,
    pub card_close_tip: &'static str,
    pub card_prev_tip: &'static str,
    pub card_next_tip: &'static str,
    /// On the tooltip of a tile with a city.
    pub city_click: &'static str,
    /// The units on a tile: the section's title, `{}` kind and power, `{}` hit points and
    /// their most, the sight, the stance.
    pub units_title: &'static str,
    /// The value on the health bar of a unit's card: `{}` of `{}`.
    pub sight_unknown: &'static str,
    pub unit_fortified: &'static str,
    /// The card of a chosen unit: the labels, and the line that says how to drop it.
    pub sight_label: &'static str,
    pub health_label: &'static str,
    pub nation_label: &'static str,
    pub unit_hint: &'static str,
    /// The outline round the sight of the chosen unit or the open city is the whole area
    /// it sees; shown in the tip of the unit and city cards.
    pub sight_note: &'static str,
    /// The keys the observer answers to, for the settings' list of controls.
    pub keys: &'static [(&'static str, &'static str)],
}
