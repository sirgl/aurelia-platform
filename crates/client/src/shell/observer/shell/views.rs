//! What the viewer chooses on the observer's screen: the grid or one power, which power,
//! how its camera follows, and the keys and clicks that say so. A view of one power
//! brings up to date, and draws, that power only; the others are not touched, and they
//! catch up when the grid or they are chosen again.
use super::*;
use crate::shell::observer::inspect::Look;

impl ObserverShell {
    /// Show `power` alone (the match's views of it are brought up to date at once).
    pub fn show(&mut self, power: usize, obs: &mut dyn Observed) {
        if power >= obs.powers().len() {
            return;
        }
        self.focus = Some(power);
        self.last_focus = Some(power);
        self.inspect.clear();
        self.catch_up(obs);
        self.pending_draw = true;
    }

    /// Back to the grid of every power.
    pub fn show_grid(&mut self, obs: &mut dyn Observed) {
        self.focus = None;
        self.inspect.clear();
        self.catch_up(obs);
        self.pending_draw = true;
    }

    /// The grid becomes one power (the one shown last, or the first that stands), and
    /// one power becomes the grid.
    pub fn toggle_view(&mut self, obs: &mut dyn Observed) {
        match self.focus {
            Some(_) => self.show_grid(obs),
            None => {
                let alive: Vec<bool> = (0..obs.powers().len()).map(|p| obs.alive(p)).collect();
                self.show(select::start(self.last_focus, &alive), obs);
            }
        }
    }

    /// The next or the previous power that stands, in the view of one power.
    pub fn switch(&mut self, way: Way, obs: &mut dyn Observed) {
        let Some(from) = self.focus else { return };
        let alive: Vec<bool> = (0..obs.powers().len()).map(|p| obs.alive(p)).collect();
        self.show(select::step(from, &alive, way), obs);
    }

    /// How the camera of the power shown alone moves.
    pub fn camera_mode(&self) -> CameraMode {
        self.focus.map_or_else(CameraMode::default, |power| self.panels.mode(power))
    }

    pub fn set_camera_mode(&mut self, mode: CameraMode) {
        if let Some(power) = self.focus {
            self.panels.set_mode(power, mode);
            self.pending_draw = true;
        }
    }

    /// The viewer drags the map of the power shown alone.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        if let Some(power) = self.focus {
            self.panels.pan(power, dx, dy);
            self.pending_draw = true;
        }
    }

    /// The viewer turns the wheel over the map of the power shown alone.
    pub fn zoom(&mut self, factor: f32) {
        if let Some(power) = self.focus {
            self.panels.zoom(power, factor);
            self.pending_draw = true;
        }
    }

    /// Bring the views that are shown up to date with the world, between rounds only (a
    /// view is a projection of the world as the last round left it). The grid brings
    /// every power's up, one power alone only that power's. Cheap: a few milliseconds.
    pub fn catch_up(&mut self, obs: &mut dyn Observed) {
        if obs.mid_round() {
            return;
        }
        let started = web_time::Instant::now();
        let shown: Vec<usize> = match self.focus {
            Some(power) => vec![power],
            None => (0..obs.powers().len()).collect(),
        };
        for &power in &shown {
            obs.refresh(power);
            self.stats.refreshes += 1;
        }
        if let Some(power) = self.focus {
            obs.refresh_details(power);
        }
        self.inspect.refresh(&*obs);
        self.stats.refresh_ms += started.elapsed().as_secs_f64() * 1000.0;
    }

    /// A key of the viewer; Esc is `escape`.
    pub fn key(&mut self, key: egui::Key, shift: bool, obs: &mut dyn Observed) -> Key {
        use egui::Key::*;
        match key {
            Space => self.clock.toggle_pause(),
            OpenBracket | Minus => self.clock.slower(),
            CloseBracket | Equals | Plus => self.clock.faster(),
            N => self.clock.step(),
            G => self.toggle_view(obs),
            R => self.toggle_reveal(obs),
            C if self.focus.is_some() => self.apply(Action::ToggleCities, obs),
            Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9 if self.inspect.city.is_some() => {
                let power = self.inspect.city.as_ref().map_or(0, |c| c.power);
                self.inspect.open_nth(&*obs, power, usize::from(digit_of(key)) - 1);
            }
            Num1 | Num2 | Num3 | Num4 | Num5 | Num6 | Num7 | Num8 | Num9 => return self.digit(key, obs),
            ArrowRight | ArrowLeft if self.inspect.city.is_some() => {
                let power = self.inspect.city.as_ref().map_or(0, |c| c.power);
                self.inspect.step_city(&*obs, power, key == ArrowRight);
            }
            Tab | ArrowRight | ArrowLeft if self.focus.is_some() => {
                let way = if key == ArrowLeft || (key == Tab && shift) { Way::Previous } else { Way::Next };
                self.switch(way, obs);
            }
            F if self.focus.is_some() => self.set_camera_mode(CameraMode::Follow),
            _ => return Key::Ignored,
        }
        Key::Used
    }

    fn digit(&mut self, key: egui::Key, obs: &mut dyn Observed) -> Key {
        match select::by_digit(digit_of(key), obs.powers().len()) {
            Some(power) => {
                self.show(power, obs);
                Key::Used
            }
            None => Key::Ignored,
        }
    }

    /// Esc closes the results card, then returns to the grid; false when there is
    /// nothing to close (the shell opens its menu).
    pub fn escape(&mut self, obs: &mut dyn Observed) -> bool {
        if obs.is_over() && self.finale_open {
            self.finale_open = false;
            return true;
        }
        if self.inspect.escape() {
            return true;
        }
        if self.focus.is_none() {
            return false;
        }
        self.show_grid(obs);
        true
    }

    /// The whole map open in every view, or the fog back (the views shown are rebuilt now,
    /// or at the round's end when one is being played).
    pub fn toggle_reveal(&mut self, obs: &mut dyn Observed) {
        let on = !obs.revealed();
        obs.set_revealed(on);
        self.catch_up(obs);
        self.pending_draw = true;
    }

    /// The outlines over the picture of the power shown alone: the sight of the chosen unit or
    /// the open city, the tile under the pointer, the chosen unit's place.
    pub fn marks(&self, obs: &dyn Observed) -> super::super::overlay::Marks {
        let mut marks = super::super::overlay::Marks::default();
        let Some(power) = self.focus else { return marks };
        let world = obs.world(power);
        if let Some(sight) = self.inspect.sight.as_ref().filter(|_| self.inspect.chosen.is_some_and(|c| c.power == power) || self.inspect.city.as_ref().is_some_and(|c| c.power == power)) {
            marks.sight = self.panels.hexes(power, world, &sight.tiles);
        }
        if let Some(hover) = self.inspect.hover.as_ref().filter(|h| h.power == power) {
            marks.hover = self.panels.hexes(power, world, &[hover.tile]);
        }
        if let Some(unit) = self.inspect.unit.as_ref().filter(|_| self.inspect.chosen.is_some_and(|c| c.power == power)) {
            marks.unit_at = self.panels.project(power, world, unit.tile);
        }
        marks
    }

    /// Offscreen runs: do what the viewer would to see `look` (false: the picture is not
    /// ready, or there is nothing of the kind to look at).
    pub fn stage(&mut self, look: Look, obs: &dyn Observed) -> bool {
        let Some(power) = self.focus else { return false };
        let world = obs.world(power);
        let own = |u: &&render::world::state::UnitMarker| u.owner == world.observer;
        let tile = match look {
            Look::City => {
                self.inspect.open_nth(obs, power, 0);
                return self.inspect.city.is_some();
            }
            Look::Unit => {
                let open: Vec<u32> = world.units.iter().filter(own).map(|u| u.tile.0).filter(|t| obs.city_at(power, *t).is_none()).collect();
                let Some(&tile) = open.first() else { return false };
                self.inspect.click(obs, power, Some(tile));
                return self.inspect.chosen.is_some();
            }
            Look::HoverCity => obs.cities_of(power).first().map(|c| c.tile),
            Look::HoverUnit => world.units.iter().find(own).map(|u| u.tile.0),
            Look::HoverStranger => world.units.iter().find(|u| u.owner != world.observer).map(|u| u.tile.0),
        };
        let (Some(tile), Some(frame)) = (tile, self.shown.as_ref()) else { return false };
        let (Some(cell), Some(share)) = (frame.cell(power), self.panels.project(power, world, tile)) else { return false };
        let shown = super::super::frame::picture(cell, frame.head).shrink(1.0);
        self.inspect.set_pointer(Some(egui::pos2(shown.left() + share.x * shown.width(), shown.top() + share.y * shown.height())));
        true
    }

    /// Remember how the frame was laid out, for clicks.
    pub fn remember(&mut self, frame: &Frame) {
        if self.shown.as_ref() != Some(frame) {
            self.shown = Some(frame.clone());
        }
    }

    /// The tooltip for the pointer's place, worked out against this frame's layout.
    pub fn look(&mut self, obs: &dyn Observed, frame: &Frame) {
        if obs.is_over() && self.finale_open {
            self.inspect.hover = None;
            return;
        }
        let covered = std::mem::take(&mut self.inspect.covered);
        self.inspect.update_hover(obs, &self.panels, frame, &covered);
        self.inspect.covered = covered;
    }

    /// The viewer clicked the map under the pointer: a city opens its card, a unit is
    /// chosen, bare land drops the choice. False when the click was not over a picture.
    pub fn click(&mut self, obs: &dyn Observed) -> bool {
        let (Some(frame), Some(pointer), Some(_)) = (self.shown.clone(), self.inspect.pointer(), self.focus) else { return false };
        if self.inspect.covered.iter().any(|r| r.contains(pointer)) {
            return true;
        }
        let covered = std::mem::take(&mut self.inspect.covered);
        let under = self.inspect.tile_under(obs, &self.panels, &frame, &covered);
        self.inspect.covered = covered;
        match under {
            Some((power, tile, _)) => {
                self.inspect.click(obs, power, Some(tile));
                true
            }
            None => match Inspect::cell_at(&frame, pointer) {
                Some((power, _)) => {
                    self.inspect.click(obs, power, None);
                    true
                }
                None => false,
            },
        }
    }

    /// What the pointer asked for.
    pub fn apply(&mut self, action: Action, obs: &mut dyn Observed) {
        match action {
            Action::TogglePause => self.clock.toggle_pause(),
            Action::Step => self.clock.step(),
            Action::Slower => self.clock.slower(),
            Action::Faster => self.clock.faster(),
            Action::Speed(speed) => self.clock.set_speed(speed),
            Action::Focus(power) => self.show(power, obs),
            Action::ToggleView => self.toggle_view(obs),
            Action::Camera(mode) => self.set_camera_mode(mode),
            Action::OpenFinale => self.finale_open = true,
            Action::CloseFinale => self.finale_open = false,
            Action::Menu => {}
            Action::ToggleReveal => self.toggle_reveal(obs),
            Action::ToggleCities => {
                if let Some(power) = self.focus {
                    self.inspect.toggle_cities(&*obs, power);
                }
            }
            Action::CardStep(forward) => {
                if let Some(power) = self.focus {
                    self.inspect.step_city(&*obs, power, forward);
                }
            }
            Action::CardClose => {
                self.inspect.escape();
            }
        }
    }
}

fn digit_of(key: egui::Key) -> u8 {
    use egui::Key::*;
    match key {
        Num1 => 1,
        Num2 => 2,
        Num3 => 3,
        Num4 => 4,
        Num5 => 5,
        Num6 => 6,
        Num7 => 7,
        Num8 => 8,
        _ => 9,
    }
}
