//! What the viewer looks at in the view of the power shown: the pointer, the tile under it
//! and its tooltip, the unit chosen with a click, the city whose card is open, and the
//! sight outline of the unit or city. Nothing here changes the match: the facts come from
//! the views (`Observed`), and a view shows only what its power knows (all of it, with the
//! whole map open).
use egui::{Pos2, Rect};

use super::card::Card;
use super::contract::{CityEntry, Observed, Sees, Sight, UnitFacts};
use super::frame::{picture, Frame};
use super::panels::Panels;

/// The tooltip of the tile under the pointer.
#[derive(Debug, Clone, PartialEq)]
pub struct Hover {
    pub power: usize,
    pub tile: u32,
    pub pointer: Pos2,
    /// What the module says of the tile (none: the tile is not on the power's map).
    pub card: Option<Card>,
    pub units: Vec<UnitFacts>,
    pub city: Option<CityEntry>,
}

/// The unit the viewer chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chosen {
    pub power: usize,
    pub id: u64,
}

/// The city whose card is open.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenCity {
    pub power: usize,
    pub tile: u32,
    pub card: Card,
    /// Its place among the power's own cities (from 0), when it is one of them, and how many
    /// there are.
    pub place: Option<(usize, usize)>,
}

/// What an offscreen run does as the viewer would (`AURELIA_OBSERVER_LOOK`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// Open the card of the power's biggest city.
    City,
    /// Choose the power's first unit that stands in the open.
    Unit,
    /// Hold the pointer over the power's biggest city, over its first unit, or over the first
    /// unit of another power that it sees.
    HoverCity,
    HoverUnit,
    HoverStranger,
}

impl Look {
    pub fn named(name: &str) -> Option<Look> {
        Some(match name.trim().to_lowercase().as_str() {
            "city" => Look::City,
            "unit" => Look::Unit,
            "hover-city" => Look::HoverCity,
            "hover-unit" => Look::HoverUnit,
            "hover-stranger" => Look::HoverStranger,
            _ => return None,
        })
    }
}

/// What a cached pick was asked: the power, the pointer's place on its picture, the
/// camera, the picture's revision and the size of its texture's unit (one frame's key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PickKey {
    power: usize,
    at: [u32; 2],
    camera: [u32; 3],
    revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CardKey {
    power: usize,
    tile: u32,
    revision: u64,
    revealed: bool,
    serial: u64,
}

#[derive(Default)]
pub struct Inspect {
    pointer: Option<Pos2>,
    pressed: bool,
    pub chosen: Option<Chosen>,
    /// What the view knows of the chosen unit.
    pub unit: Option<UnitFacts>,
    pub city: Option<OpenCity>,
    /// Where the open card and the chosen unit's card stood on the last frame: the pointer
    /// over them is not over the map.
    pub covered: Vec<Rect>,
    /// The sight of the chosen unit, or of the city whose card is open.
    pub sight: Option<Sight>,
    pub hover: Option<Hover>,
    pick: Option<(PickKey, Option<u32>)>,
    looked: Option<(CardKey, Option<Card>, Vec<UnitFacts>, Option<CityEntry>)>,
}

impl Inspect {
    /// The pointer as the frame's events left it.
    pub fn track(&mut self, raw: &egui::RawInput) {
        for event in &raw.events {
            match event {
                egui::Event::PointerMoved(at) => self.pointer = Some(*at),
                egui::Event::PointerGone => self.pointer = None,
                egui::Event::PointerButton { pos, pressed, .. } => {
                    self.pointer = Some(*pos);
                    self.pressed = *pressed;
                }
                egui::Event::WindowFocused(false) => {
                    self.pointer = None;
                    self.pressed = false;
                }
                _ => {}
            }
        }
    }

    /// Put the pointer somewhere (offscreen runs and tests).
    pub fn set_pointer(&mut self, at: Option<Pos2>) {
        self.pointer = at;
    }

    pub fn pointer(&self) -> Option<Pos2> {
        self.pointer
    }

    /// Nothing is chosen, no card is open.
    pub fn clear(&mut self) {
        self.chosen = None;
        self.unit = None;
        self.city = None;
        self.sight = None;
        self.hover = None;
        self.covered.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.chosen.is_none() && self.city.is_none()
    }

    /// Esc: the city card first, then the chosen unit. False when there was neither.
    pub fn escape(&mut self) -> bool {
        if self.city.take().is_some() {
            self.sight = None;
            self.look_after_choice();
            return true;
        }
        if self.chosen.take().is_some() {
            self.unit = None;
            self.sight = None;
            return true;
        }
        false
    }

    /// The tile under `pointer` on the picture of one of the frame's cells, with the power
    /// of that cell and the point's place on the picture (shares across and down).
    pub fn cell_at(frame: &Frame, pointer: Pos2) -> Option<(usize, Rect)> {
        frame.cells.iter().find_map(|cell| {
            let shown = picture(cell.rect, frame.head).shrink(1.0);
            shown.contains(pointer).then_some((cell.power, shown))
        })
    }

    fn pick(&mut self, obs: &dyn Observed, panels: &Panels, power: usize, share: Pos2) -> Option<u32> {
        let world = obs.world(power);
        let key = PickKey { power, at: [share.x.to_bits(), share.y.to_bits()], camera: panels.pose(power), revision: world.revision() };
        if let Some((last, tile)) = self.pick {
            if last == key {
                return tile;
            }
        }
        let tile = panels.pick(power, world, share);
        self.pick = Some((key, tile));
        tile
    }

    /// The tile under the pointer, if it is over a picture and not over one of `covered`.
    pub fn tile_under(&mut self, obs: &dyn Observed, panels: &Panels, frame: &Frame, covered: &[Rect]) -> Option<(usize, u32, Pos2)> {
        let pointer = self.pointer?;
        let (power, shown) = Self::cell_at(frame, pointer)?;
        if covered.iter().any(|r| r.contains(pointer)) {
            return None;
        }
        let share = Pos2::new((pointer.x - shown.left()) / shown.width().max(1.0), (pointer.y - shown.top()) / shown.height().max(1.0));
        let tile = self.pick(obs, panels, power, share)?;
        Some((power, tile, pointer))
    }

    /// Work out the tooltip for this frame (none while a button is down, or the pointer is
    /// off the map). The module's card is asked once per tile and picture.
    pub fn update_hover(&mut self, obs: &dyn Observed, panels: &Panels, frame: &Frame, covered: &[Rect]) {
        self.hover = None;
        if self.pressed {
            return;
        }
        let Some((power, tile, pointer)) = self.tile_under(obs, panels, frame, covered) else { return };
        if !obs.current(power) {
            return;
        }
        let key = CardKey { power, tile, revision: obs.world(power).revision(), revealed: obs.revealed(), serial: obs.serial() };
        let fresh = !matches!(&self.looked, Some((last, ..)) if *last == key);
        if fresh {
            self.looked = Some((key, obs.tile_card(power, tile), obs.units_on(power, tile), obs.city_at(power, tile)));
        }
        if let Some((_, card, units, city)) = &self.looked {
            self.hover = Some(Hover { power, tile, pointer, card: card.clone(), units: units.clone(), city: city.clone() });
        }
    }

    /// A click on the picture of `power` over `tile` (none: over no known land). A city opens
    /// its card, a unit is chosen (a second click on a stack takes the next unit), anything
    /// else clears the choice.
    pub fn click(&mut self, obs: &dyn Observed, power: usize, tile: Option<u32>) {
        if !obs.current(power) {
            return;
        }
        let Some(tile) = tile else {
            self.clear();
            return;
        };
        if obs.city_at(power, tile).is_some() {
            self.open_city(obs, power, tile);
            return;
        }
        let units = obs.units_on(power, tile);
        if units.is_empty() {
            self.clear();
            return;
        }
        let after = self.chosen.filter(|c| c.power == power).and_then(|c| units.iter().position(|u| u.id == c.id)).map_or(0, |i| i + 1);
        let next = &units[after % units.len()];
        self.city = None;
        self.chosen = Some(Chosen { power, id: next.id });
        self.unit = Some(next.clone());
        self.sight = obs.sight(power, Sees::Unit(next.id));
    }

    /// Open the card of the city on `tile` in the power's view.
    pub fn open_city(&mut self, obs: &dyn Observed, power: usize, tile: u32) {
        if !obs.current(power) {
            return;
        }
        let Some(card) = obs.city_card(power, tile) else { return };
        let own = obs.cities_of(power);
        let place = own.iter().position(|c| c.tile == tile).map(|i| (i, own.len()));
        self.chosen = None;
        self.unit = None;
        self.city = Some(OpenCity { power, tile, card, place });
        self.sight = obs.sight(power, Sees::City(tile));
    }

    /// Open the card of the power's `index`-th own city (the biggest first).
    pub fn open_nth(&mut self, obs: &dyn Observed, power: usize, index: usize) {
        if let Some(city) = obs.cities_of(power).get(index) {
            self.open_city(obs, power, city.tile);
        }
    }

    /// The card of the next (or the previous) own city of the power, going round.
    pub fn step_city(&mut self, obs: &dyn Observed, power: usize, forward: bool) {
        let cities = obs.cities_of(power);
        if cities.is_empty() {
            return;
        }
        let at = self.city.as_ref().and_then(|c| cities.iter().position(|e| e.tile == c.tile));
        let next = match (at, forward) {
            (None, _) => 0,
            (Some(i), true) => (i + 1) % cities.len(),
            (Some(i), false) => (i + cities.len() - 1) % cities.len(),
        };
        self.open_city(obs, power, cities[next].tile);
    }

    /// The `Cities` key: open the card of the biggest city, or close the card that is open.
    pub fn toggle_cities(&mut self, obs: &dyn Observed, power: usize) {
        if self.city.is_some() {
            self.escape();
        } else {
            self.open_nth(obs, power, 0);
        }
    }

    /// The match moved on, or the map was opened or closed: the card, the chosen unit and
    /// their sight are worked out again from the views (a unit that is gone, or no longer
    /// in the view, is dropped; so is a city that is no longer on the map).
    pub fn refresh(&mut self, obs: &dyn Observed) {
        self.looked = None;
        self.pick = None;
        if let Some(chosen) = self.chosen {
            self.unit = obs.unit_facts(chosen.power, chosen.id);
            if self.unit.is_some() {
                self.sight = obs.sight(chosen.power, Sees::Unit(chosen.id));
            } else {
                self.chosen = None;
                self.sight = None;
            }
        }
        if let Some(open) = self.city.take() {
            self.open_city(obs, open.power, open.tile);
        }
    }

    fn look_after_choice(&mut self) {
        self.looked = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_are_named_by_the_environment() {
        assert_eq!(Look::named("City"), Some(Look::City));
        assert_eq!(Look::named("hover-stranger"), Some(Look::HoverStranger));
        assert_eq!(Look::named("somewhere"), None);
    }

    #[test]
    fn the_pointer_follows_the_events_and_goes_when_the_window_loses_focus() {
        let mut inspect = Inspect::default();
        let raw = |events| egui::RawInput { events, ..Default::default() };
        inspect.track(&raw(vec![egui::Event::PointerMoved(Pos2::new(10.0, 20.0))]));
        assert_eq!(inspect.pointer(), Some(Pos2::new(10.0, 20.0)));
        inspect.track(&raw(vec![egui::Event::PointerButton { pos: Pos2::new(11.0, 21.0), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]));
        assert!(inspect.pressed);
        inspect.track(&raw(vec![egui::Event::PointerGone]));
        assert_eq!(inspect.pointer(), None);
        inspect.track(&raw(vec![egui::Event::PointerMoved(Pos2::new(1.0, 2.0)), egui::Event::WindowFocused(false)]));
        assert_eq!(inspect.pointer(), None);
    }

    #[test]
    fn escape_closes_the_card_then_the_chosen_unit_and_then_says_there_was_nothing() {
        let mut inspect = Inspect::default();
        inspect.chosen = Some(Chosen { power: 0, id: 7 });
        inspect.city = Some(OpenCity { power: 0, tile: 3, card: Card::new("Ur"), place: Some((0, 2)) });
        assert!(inspect.escape());
        assert!(inspect.city.is_none() && inspect.chosen.is_some());
        assert!(inspect.escape());
        assert!(inspect.is_empty());
        assert!(!inspect.escape());
    }
}
