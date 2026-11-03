//! Fingers on the map for a host without a mouse (a phone's browser page; Android's loop does
//! the same by hand): the finger events of the window go in, the gestures of
//! `shell::touch` are applied to the shell (pan, pinch, tap, long press).
use web_time::Instant;

use super::App;
use crate::shell::module::{ModuleUi, Screen};
use crate::shell::touch::{Gesture, Phase, Touches};

/// Travel of a finger (density-independent pixels) below which a touch is still a tap.
pub const TAP_SLOP_DP: f32 = 10.0;

/// The touch state machine of one window and what it needs to act on the shell.
pub struct TouchHost {
    touches: Touches,
}

impl TouchHost {
    /// For a screen of `scale` physical pixels per dp.
    pub fn new(scale: f32) -> Self {
        Self { touches: Touches::new(TAP_SLOP_DP * scale) }
    }

    /// Forget the fingers (the window lost the focus: their lifts will never arrive).
    pub fn reset(&mut self, scale: f32) {
        *self = Self::new(scale);
    }

    /// When `tick` next has something to do (the long press): for the loop's wake-up.
    pub fn deadline(&self) -> Option<Instant> {
        self.touches.deadline()
    }

    /// A finger event at `at` (physical pixels). A finger that starts on an egui panel
    /// belongs to egui and is never tracked; `egui_consumed` is egui's answer to the event.
    pub fn event<U: ModuleUi>(
        &mut self,
        app: &mut App<U>,
        id: u64,
        phase: Phase,
        at: (f32, f32),
        egui_consumed: bool,
        size: (u32, u32),
    ) {
        // egui learns a finger's position only in the next frame, so its `consumed` is stale
        // for a first touch: the panels of the last frame are asked directly.
        let free = phase != Phase::Started || !(egui_consumed || app.over_panel(at));
        let gestures = if free && app.touch_on_map() {
            self.touches.event(id, phase, at, Instant::now())
        } else {
            Vec::new()
        };
        self.apply(app, gestures, size);
    }

    /// The loop woke up: a finger held long enough makes the secondary click.
    pub fn tick<U: ModuleUi>(&mut self, app: &mut App<U>, size: (u32, u32)) {
        let gestures = self.touches.tick(Instant::now());
        if !gestures.is_empty() {
            self.apply(app, gestures, size);
        }
    }

    fn apply<U: ModuleUi>(&mut self, app: &mut App<U>, gestures: Vec<Gesture>, size: (u32, u32)) {
        let on_map = app.touch_on_map();
        for gesture in gestures {
            match gesture {
                Gesture::Pan { dx, dy } if on_map => {
                    app.cancel_glide();
                    app.pan_view(dx, dy);
                }
                Gesture::Zoom { factor, .. } if on_map => {
                    app.cancel_glide();
                    app.zoom_view(factor);
                }
                Gesture::Tap { at, button } if on_map => {
                    app.note_pointer(at);
                    let tile = app.pick(at, (size.0 as f32, size.1 as f32));
                    app.click(tile, button);
                }
                _ => {}
            }
        }
        app.dragging = self.touches.dragging();
        app.hud.hovered_tile = None;
        app.ctx.request_repaint();
    }
}

impl<U: ModuleUi> App<U> {
    /// Whether the last frame had an egui panel (not the background) under the physical pixel `at`.
    pub fn over_panel(&self, at: (f32, f32)) -> bool {
        let ppp = self.ctx.pixels_per_point();
        let point = egui::pos2(at.0 / ppp, at.1 / ppp);
        self.ctx.layer_id_at(point).is_some_and(|layer| layer.order != egui::Order::Background)
    }

    /// The map takes touches: it is the screen shown and no list of panels covers it.
    pub fn touch_on_map(&self) -> bool {
        self.screens.screen == Screen::Map && !self.screens.navigation_is_open()
    }
}
