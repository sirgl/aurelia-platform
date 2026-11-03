//! The observer's state in the shell: the clock, which panel is enlarged, which panels
//! still owe a refresh of their view, and the pace of the frame. The rounds are played
//! by the match (`Observed::advance`); this decides when, and which panels redraw.
//!
//! A panel redraws only when its power's picture changed (the projection's revision) or
//! its size did, and at most `budget` panels per frame, so six or eight panels cost
//! about what one map does. At the greatest speed the panels refresh a few times a
//! second and the rounds in between are not drawn.
use std::time::Duration;

use super::contract::{Observed, Progress};
use super::panels::CameraMode;
use super::select::{self, Way};
use super::frame::{self, Frame, Size};
use super::inspect::Inspect;
use super::panels::{Panels, TerrainLayers};
use super::scene::Action;
use super::speed::{Clock, Hold, Order, Speed};
use render::gpu::context::GpuCore;
use render::gpu::terrain_gpu::TerrainSync;

/// How many panels a frame refreshes and draws, at most.
pub const PANELS_PER_FRAME: usize = 2;
/// At the greatest speed the panels are brought up to date this often.
pub const MAX_SPEED_DISPLAY: Duration = Duration::from_millis(150);

/// What the viewer's keys did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Ignored,
    Used,
}

/// Counters of the frames, for the benches and the tests.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    pub rounds: u32,
    pub renders: u64,
    pub refreshes: u64,
    /// Slices of AI work played, and the milliseconds they took in all.
    pub slices: u64,
    pub slice_ms: f64,
    /// Milliseconds spent refreshing the views and drawing the panels, in all.
    pub refresh_ms: f64,
    pub render_ms: f64,
    /// Milliseconds the last frame spent refreshing views and drawing panels.
    pub last_panels_ms: f64,
}

pub struct ObserverShell {
    pub clock: Clock,
    /// The power the screen shows alone; None: the grid of every power.
    pub focus: Option<usize>,
    /// The power shown alone last, for the way back from the grid.
    last_focus: Option<usize>,
    /// The results card is shown (it opens when the match ends).
    pub finale_open: bool,
    pub panels: Panels,
    /// What the viewer looks at: the tile under the pointer, a chosen unit, an open city.
    pub inspect: Inspect,
    /// The frame as the last draw laid it out (a click is matched against it).
    shown: Option<Frame>,
    /// Panels still wait for a draw of the views the last round brought up to date.
    pending_draw: bool,
    cursor: usize,
    since_display: Duration,
    /// What the frames' elapsed times add up to: the shell's own time, which the panels' cameras
    /// follow (never the wall clock, so a pinned frame time pins them too).
    time: Duration,
    pub budget: usize,
    pub stats: Stats,
    last_frame: Option<web_time::Instant>,
    wants_frames: bool,
    /// Offscreen runs pin the time between frames (the clock never fires by itself).
    fixed_elapsed: Option<Duration>,
}

impl ObserverShell {
    /// The shell of a match of `count` powers; the panels' renderers are made as they
    /// are first drawn.
    pub fn new(obs: &dyn Observed, format: wgpu::TextureFormat, layers: TerrainLayers, sync: TerrainSync, clock: Clock) -> Self {
        let count = obs.powers().len();
        Self {
            clock,
            focus: obs.first_view().filter(|&p| p < count),
            last_focus: obs.first_view(),
            finale_open: false,
            panels: Panels::new(count, format, layers, obs.books(), sync),
            inspect: Inspect::default(),
            shown: None,
            pending_draw: true,
            cursor: 0,
            since_display: Duration::ZERO,
            time: Duration::ZERO,
            budget: PANELS_PER_FRAME,
            stats: Stats::default(),
            last_frame: None,
            wants_frames: true,
            fixed_elapsed: None,
        }
    }

    pub fn frame(&self, screen: egui::Rect, count: usize) -> Frame {
        frame::frame(screen, count, self.focus)
    }

    /// Play the match as the clock orders. `elapsed` is the wall time since the last frame.
    pub fn pace(&mut self, obs: &mut dyn Observed, elapsed: Duration) -> Option<Progress> {
        self.since_display += elapsed;
        self.time += elapsed;
        if obs.is_over() {
            return Some(Progress::Over);
        }
        let playing = obs.mid_round();
        let waiting = self.pending_draw && self.clock.speed() != Speed::Max;
        let hold = if playing || waiting { Hold::Held } else { Hold::Free };
        let order = self.clock.tick(elapsed, hold);
        if !playing && order != Order::Round {
            return None;
        }
        let started = web_time::Instant::now();
        let progress = obs.advance(self.clock.speed().slice());
        self.stats.slices += 1;
        self.stats.slice_ms += started.elapsed().as_secs_f64() * 1000.0;
        if progress != Progress::Working {
            self.round_ended(obs);
        }
        Some(progress)
    }

    fn round_ended(&mut self, obs: &mut dyn Observed) {
        self.stats.rounds += 1;
        let due = self.clock.speed() != Speed::Max || self.since_display >= MAX_SPEED_DISPLAY || self.clock.is_paused();
        if due || obs.is_over() {
            self.catch_up(obs);
            self.pending_draw = true;
            self.since_display = Duration::ZERO;
        }
        if obs.is_over() {
            self.finale_open = true;
        }
    }

    /// The wall time since the previous frame (zero at the first).
    pub fn elapsed(&mut self, now: web_time::Instant) -> Duration {
        if let Some(fixed) = self.fixed_elapsed {
            return fixed;
        }
        let gap = self.last_frame.map_or(Duration::ZERO, |last| now.saturating_duration_since(last));
        self.last_frame = Some(now);
        gap
    }

    /// Offscreen runs: every frame lasts `elapsed`, whatever the wall clock says.
    pub fn pin_time(&mut self, elapsed: Duration) {
        self.fixed_elapsed = Some(elapsed);
    }

    /// Play rounds at once, without the clock, until the match is in round `round` or
    /// over (offscreen runs and tests). The views are owed a refresh afterwards.
    pub fn play_to(&mut self, obs: &mut dyn Observed, round: u32) {
        while !obs.is_over() && obs.round().now < round {
            while obs.advance(Duration::from_secs(3600)) == Progress::Working {}
            self.round_ended(obs);
        }
        self.catch_up(obs);
        self.pending_draw = true;
    }

    /// After a frame: does the window need the next one at once?
    pub fn settle(&mut self, obs: &dyn Observed) {
        let running = !self.clock.is_paused() && !obs.is_over();
        let hurry = running && self.clock.speed() == Speed::Max;
        self.wants_frames = obs.mid_round() || self.busy() || self.clock.step_pending() || hurry;
    }

    /// The observer screen is not shown (the menu is over it): nothing runs, and the time
    /// spent away is not counted when the screen is back.
    pub fn rest(&mut self) {
        self.last_frame = None;
        self.wants_frames = false;
    }

    pub fn wants_frames(&self) -> bool {
        self.wants_frames
    }

    /// The last views wait to be drawn.
    pub fn owes(&self) -> bool {
        self.pending_draw
    }

    /// The renderers still have work, or views wait: the window must draw again.
    pub fn busy(&self) -> bool {
        self.panels.working() || self.pending_draw
    }

    /// Draw the panels whose picture is out of date, `budget` of them at most per frame
    /// (in rotation, so none starves).
    pub fn draw_panels(&mut self, obs: &dyn Observed, core: &GpuCore, ui: &mut egui_wgpu::Renderer, frame: &Frame, points_to_pixels: f32) {
        let started = web_time::Instant::now();
        let scale = frame::resolution(self.focus);
        let n = frame.cells.len();
        let mut spent = 0;
        let mut next = self.cursor;
        for step in 0..n {
            let cell = frame.cells[(self.cursor + step) % n];
            if spent >= self.budget {
                break;
            }
            next = (self.cursor + step + 1) % n;
            let size = frame::pixels(frame::picture(cell.rect, frame.head), points_to_pixels, scale);
            spent += self.update(obs, core, ui, cell.power, size);
        }
        self.cursor = if spent >= self.budget { next } else { 0 };
        self.pending_draw = spent >= self.budget;
        self.stats.last_panels_ms = started.elapsed().as_secs_f64() * 1000.0;
    }

    /// Draw one panel if its picture is out of date; how much work it took (0 or 1).
    fn update(&mut self, obs: &dyn Observed, core: &GpuCore, ui: &mut egui_wgpu::Renderer, power: usize, size: Size) -> usize {
        let world = obs.world(power);
        if !self.panels.stale(power, world, size) {
            return 0;
        }
        let started = web_time::Instant::now();
        self.panels.render(core, ui, power, world, size, self.time);
        self.stats.render_ms += started.elapsed().as_secs_f64() * 1000.0;
        self.stats.renders += 1;
        1
    }

    /// Free the panels' textures.
    pub fn release(&mut self, ui: &mut egui_wgpu::Renderer) {
        self.panels.release(ui);
    }
}

mod views;
#[cfg(test)]
mod tests;
