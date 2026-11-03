//! The app's side of the observer mode: it starts the panels when the module starts an
//! observed match, draws a frame of the observer screen in place of the map and the
//! HUD, and routes the viewer's keys. The panels, the clock and the scene are
//! `shell::observer`.
use render::gpu::context::GpuCore;

use super::{repaint_delay, App};
use crate::shell::module::{ModuleUi, Screen};
use crate::shell::observer::scene::{self, Action, View};
use crate::shell::observer::shell::{Key, ObserverShell};
use crate::shell::observer::finale;
use crate::shell::observer::frame::Frame;

/// A wall time longer than this between two frames counts as this long (the window slept).
const LONGEST_GAP: std::time::Duration = std::time::Duration::from_secs(5);

impl<U: ModuleUi> App<U> {
    /// The observer's screen is up: an observed match runs and the map screen is shown.
    pub(super) fn observer_shown(&mut self) -> bool {
        if self.observer.is_some() && self.module.observed().is_none() {
            self.release_observer();
        }
        let shown = self.screens.screen == Screen::Map;
        match self.observer.as_mut() {
            Some(shell) if !shown => shell.rest(),
            _ => {}
        }
        self.observer.is_some() && shown
    }

    /// The module started an observed match: panels for its powers.
    pub(super) fn observer_started(&mut self) {
        self.release_observer();
        self.forget_old_match();
        let Some(obs) = self.module.observed() else { return };
        self.journal = None;
        self.ended = false;
        let mut shell = ObserverShell::new(&*obs, self.format, self.terrain.clone(), self.observer_terrain_sync, self.observer_clock);
        // `AURELIA_OBSERVER_REVEAL=1`: the match starts with the whole map open.
        obs.set_revealed(crate::shell::observer::capture::reveal_requested(&|name| std::env::var(name).ok()));
        shell.catch_up(obs);
        self.observer = Some(shell);
    }

    pub(super) fn release_observer(&mut self) {
        if let Some(mut shell) = self.observer.take() {
            shell.release(&mut self.ui_renderer);
        }
    }

    /// The window must draw again soon: the clock runs, a round is being played, or a
    /// panel still has work.
    pub(super) fn observer_wants_frames(&self) -> bool {
        self.observer.as_ref().is_some_and(|shell| shell.wants_frames()) && self.screens.screen == Screen::Map
    }

    /// The observer, for tests and benches.
    pub fn observer(&self) -> Option<&ObserverShell> {
        self.observer.as_ref()
    }

    pub fn observer_mut(&mut self) -> Option<&mut ObserverShell> {
        self.observer.as_mut()
    }

    /// Offscreen runs and tests: show `power` alone, or the grid.
    pub fn observer_show(&mut self, power: Option<usize>) {
        let (Some(shell), Some(obs)) = (self.observer.as_mut(), self.module.observed()) else { return };
        match power {
            Some(power) => shell.show(power, obs),
            None => shell.show_grid(obs),
        }
    }

    /// Offscreen runs: do what the viewer would to see `look`.
    pub fn observer_stage(&mut self, look: crate::shell::observer::inspect::Look) -> bool {
        match (self.observer.as_mut(), self.module.observed()) {
            (Some(shell), Some(obs)) => shell.stage(look, &*obs),
            _ => false,
        }
    }

    /// Offscreen runs: play the observed match at once to round `round`.
    pub fn play_observer_to(&mut self, round: u32) {
        if let (Some(shell), Some(obs)) = (self.observer.as_mut(), self.module.observed()) {
            shell.play_to(obs, round);
        }
    }

    /// The observed match has ended (or there is none).
    pub fn observed_over(&mut self) -> bool {
        self.module.observed().is_none_or(|obs| obs.is_over())
    }

    /// The hash of the observed match's world.
    pub fn observed_hash(&mut self) -> Option<u64> {
        self.module.observed().map(|obs| obs.state_hash())
    }

    /// A key over the observer screen. Esc closes the results card, then the enlarged
    /// panel, then opens the menu.
    pub(super) fn observer_key(&mut self, key: egui::Key, shift: bool) {
        let (Some(shell), Some(obs)) = (self.observer.as_mut(), self.module.observed()) else { return };
        if key == egui::Key::Escape {
            if !shell.escape(obs) {
                self.escape();
            }
            return;
        }
        if shell.key(key, shift, obs) == Key::Used {
            self.ctx.request_repaint();
        }
    }

    /// The cursor stands at `cursor` (physical pixels): a click that follows it is matched
    /// against this place, whether or not a frame was drawn since the cursor moved.
    pub fn note_pointer(&mut self, cursor: (f32, f32)) {
        let scale = self.ctx.pixels_per_point();
        if let Some(shell) = self.observer.as_mut() {
            shell.inspect.set_pointer(Some(egui::pos2(cursor.0 / scale, cursor.1 / scale)));
        }
    }

    /// A click on the map of the observer screen (the primary button): the shell matches it
    /// against the last frame. False when the screen is not the observer's.
    pub(super) fn observer_click(&mut self) -> bool {
        if !self.observer_shown() {
            return false;
        }
        if let (Some(shell), Some(obs)) = (self.observer.as_mut(), self.module.observed()) {
            if shell.click(&*obs) {
                self.ctx.request_repaint();
            }
        }
        true
    }

    /// The viewer drags the map by `dx`, `dy` pixels: over the observer it moves the camera
    /// of the power shown alone, elsewhere the map camera.
    pub fn pan_view(&mut self, dx: f32, dy: f32) {
        match self.observer.as_mut() {
            Some(shell) if self.screens.screen == Screen::Map => shell.pan(dx, dy),
            _ => self.camera.pan_by(dx, dy),
        }
    }

    /// The wheel turns by `factor`: the same routing as `pan_view`.
    pub fn zoom_view(&mut self, factor: f32) {
        match self.observer.as_mut() {
            Some(shell) if self.screens.screen == Screen::Map => shell.zoom(factor),
            _ => self.camera.zoom_by(factor),
        }
    }

    /// Tab switches powers here, not egui's focus between buttons: the key is the viewer's,
    /// and no button keeps the focus (Space and Enter would press it).
    fn keep_keys_for_the_viewer(&mut self, raw: &mut egui::RawInput) {
        raw.events.retain(|event| !matches!(event, egui::Event::Key { key: egui::Key::Tab, .. }));
        self.ctx.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }

    /// One frame of the observer screen: the match advances as the clock orders, the
    /// panels that changed redraw, the scene is painted over them.
    pub(super) fn observer_frame(&mut self, core: &GpuCore, target: &wgpu::TextureView, size: (u32, u32), mut raw: egui::RawInput) -> egui::PlatformOutput {
        let started = web_time::Instant::now();
        self.fit_ui_scale(size, &mut raw);
        self.keep_keys_for_the_viewer(&mut raw);
        let screen = raw.screen_rect.unwrap_or_else(|| {
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32) / self.ctx.pixels_per_point())
        });
        let points_to_pixels = self.ctx.pixels_per_point();
        let (Some(shell), Some(obs)) = (self.observer.as_mut(), self.module.observed()) else {
            return Default::default();
        };
        let elapsed = shell.elapsed(started).min(LONGEST_GAP);
        let frame = shell.frame(screen, obs.powers().len());
        shell.inspect.track(&raw);
        shell.remember(&frame);
        shell.pace(obs, elapsed);
        shell.look(&*obs, &frame);
        shell.draw_panels(obs, core, &mut self.ui_renderer, &frame, points_to_pixels);
        let world_ms = started.elapsed().as_secs_f64() * 1000.0;
        let ui_started = web_time::Instant::now();
        let Some((output, drawn)) = self.run_scene(raw, &frame, screen) else {
            return Default::default();
        };
        if let Some(shell) = self.observer.as_mut() {
            shell.inspect.covered = drawn.covered;
        }
        drawn.actions.into_iter().for_each(|action| self.apply_observer_action(action));
        self.finish_observer_frame(&output);
        let ui_ms = ui_started.elapsed().as_secs_f64() * 1000.0;
        let paint_started = web_time::Instant::now();
        let platform = self.paint_ui(core, target, size, output, false);
        self.stages = super::FrameStages { module_ms: 0.0, world_ms, ui_ms, paint_ms: paint_started.elapsed().as_secs_f64() * 1000.0 };
        platform
    }

    /// Run egui over the scene: what it drew, and what the pointer asked for.
    fn run_scene(&mut self, raw: egui::RawInput, frame: &Frame, screen: egui::Rect) -> Option<(egui::FullOutput, scene::Drawn)> {
        let words = &self.module.words().observer;
        let (Some(shell), Some(obs)) = (self.observer.as_mut(), self.module.observed()) else {
            return None;
        };
        let textures: Vec<_> = (0..obs.powers().len()).map(|power| shell.panels.texture(power)).collect();
        let labels = shell.focus.map(|power| shell.panels.labels(power, obs.world(power))).unwrap_or_default();
        let marks = shell.marks(&*obs);
        let mut drawn = scene::Drawn::default();
        let ctx = self.ctx.clone();
        let output = ctx.run_ui(raw, |ui| {
            let view = View { obs: &*obs, words, clock: &shell.clock, frame, textures: &textures, labels: &labels, focus: shell.focus, camera: shell.camera_mode(), finale_open: shell.finale_open, inspect: &shell.inspect, marks: &marks };
            drawn = scene::draw(ui.ctx(), &view);
            if obs.is_over() && shell.finale_open {
                drawn.actions.extend(finale::draw(ui.ctx(), screen, &*obs, words));
            }
        });
        Some((output, drawn))
    }

    fn apply_observer_action(&mut self, action: Action) {
        match action {
            Action::Menu => self.screens.open(Screen::Menu),
            other => {
                if let (Some(shell), Some(obs)) = (self.observer.as_mut(), self.module.observed()) {
                    shell.apply(other, obs);
                }
            }
        }
    }

    /// When the window draws next: at once while anything runs, else when the pause ends.
    fn finish_observer_frame(&mut self, output: &egui::FullOutput) {
        let (Some(shell), Some(obs)) = (self.observer.as_mut(), self.module.observed()) else { return };
        let over = obs.is_over();
        shell.settle(&*obs);
        let next = if over { None } else { shell.clock.until_next() };
        self.ui_delay = repaint_delay(&self.ctx, output).min(next.unwrap_or(std::time::Duration::MAX));
    }
}
