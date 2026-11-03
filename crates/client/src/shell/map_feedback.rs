//! Map feedback over the world: floating damage numbers and words, combat flashes,
//! city puffs, building sparkles, projectiles in flight, the selection ring and the
//! route preview. Everything stands on the displayed ground (heights sampled by the
//! game from the renderer's surface) and is drawn with the egui painter: no GPU work.
use render::camera::CameraController;
use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Stroke};

use super::module::ShellWords;
use super::presentation::motion::{Floating, Mark, MarkKind};

/// What the map shows this frame besides the world itself.
#[derive(Debug, Default, Clone)]
pub struct MapOverlay {
    pub marks: Vec<ScreenMark>,
    /// Units the presentation draws away from their tile: (id, feet position).
    pub posed: Vec<(u64, [f32; 3])>,
    /// Feet of the selected unit.
    pub selection: Option<[f32; 3]>,
    /// Route preview tiles on the ground.
    pub route: Vec<[f32; 3]>,
    /// Seconds, for pulses and marching dashes.
    pub time: f32,
    pub reduced_motion: bool,
    pub observer: u8,
}

/// A live mark with the ground under it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenMark {
    pub mark: Mark,
    pub progress: f32,
    pub ground: f32,
}

const GOLD: Color32 = Color32::from_rgb(214, 178, 96);
const LATER: Color32 = Color32::from_rgb(150, 160, 166);
const LOSS: Color32 = Color32::from_rgb(236, 92, 72);
const HIT: Color32 = Color32::from_rgb(250, 214, 120);
const CREAM: Color32 = Color32::from_rgb(244, 236, 212);

struct View<'a> {
    camera: &'a CameraController,
    screen: Rect,
}

impl View<'_> {
    fn project(&self, p: [f32; 3]) -> Option<Pos2> {
        let (x, y) = self.camera.project_to_screen(p, self.screen.aspect_ratio(), self.screen.width(), self.screen.height())?;
        Some(pos2(x, y))
    }

    /// A ring of radius `r` world units on the ground around `c`.
    fn ring(&self, c: [f32; 3], r: f32) -> Vec<Pos2> {
        (0..=32)
            .filter_map(|i| {
                let a = i as f32 / 32.0 * std::f32::consts::TAU;
                self.project([c[0] + r * a.cos(), c[1], c[2] + r * a.sin()])
            })
            .collect()
    }
}

fn painter(ctx: &egui::Context, name: &'static str) -> egui::Painter {
    ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new(name)))
}

/// The layer of everything that lies on the ground under the HUD widgets: a module's
/// map paint first, then the selection ring and the route preview.
pub fn ground_painter(ctx: &egui::Context) -> egui::Painter {
    painter(ctx, "map_feedback_ground")
}

/// Selection ring and route preview: under the HUD widgets.
pub fn draw_ground(ctx: &egui::Context, camera: &CameraController, overlay: &MapOverlay, this_turn: usize, turns: u32, words: &ShellWords) {
    let view = View { camera, screen: ctx.content_rect() };
    let painter = ground_painter(ctx);
    if let Some(feet) = overlay.selection {
        selection_ring(&painter, &view, feet, overlay);
    }
    route(&painter, &view, overlay, (this_turn, turns), words);
}

/// Numbers, flashes, puffs, sparkles and projectiles: over the world, under the HUD.
pub fn draw_marks(ctx: &egui::Context, camera: &CameraController, overlay: &MapOverlay, words: &ShellWords) {
    let view = View { camera, screen: ctx.content_rect() };
    let painter = painter(ctx, "map_feedback_marks");
    for mark in &overlay.marks {
        draw_mark(&painter, &view, mark, overlay, words);
    }
}

fn selection_ring(painter: &egui::Painter, view: &View<'_>, feet: [f32; 3], overlay: &MapOverlay) {
    let lift = [feet[0], feet[1] + 0.03, feet[2]];
    let ring = view.ring(lift, 0.62);
    if ring.len() < 3 {
        return;
    }
    painter.add(egui::Shape::line(ring.clone(), Stroke::new(4.0, Color32::from_black_alpha(110))));
    painter.add(egui::Shape::line(ring, Stroke::new(2.0, GOLD)));
    if overlay.reduced_motion {
        return;
    }
    let pulse = (overlay.time * 1.6).fract();
    let outer = view.ring(lift, 0.62 + pulse * 0.28);
    let alpha = ((1.0 - pulse) * 150.0) as u8;
    painter.add(egui::Shape::line(outer, Stroke::new(1.5, Color32::from_rgba_unmultiplied(214, 178, 96, alpha))));
}

fn route(painter: &egui::Painter, view: &View<'_>, overlay: &MapOverlay, (this_turn, turns): (usize, u32), words: &ShellWords) {
    let lifted: Vec<Option<Pos2>> = overlay.route.iter().map(|p| view.project([p[0], p[1] + 0.06, p[2]])).collect();
    let march = if overlay.reduced_motion { 0.0 } else { (overlay.time * 1.5).fract() };
    for (i, pair) in lifted.windows(2).enumerate() {
        let (Some(a), Some(b)) = (pair[0], pair[1]) else {
            continue;
        };
        let color = if i + 1 < this_turn { GOLD } else { LATER };
        painter.line_segment([a, b], Stroke::new(5.0, Color32::from_black_alpha(110)));
        dashes(painter, a, b, march, color);
    }
    let Some(Some(end)) = lifted.last() else {
        return;
    };
    let beat = if overlay.reduced_motion { 0.0 } else { (overlay.time * 3.0).sin() * 1.2 };
    painter.circle_filled(*end, 6.5 + beat, Color32::from_rgba_premultiplied(10, 20, 29, 230));
    painter.circle_stroke(*end, 6.5 + beat, Stroke::new(1.8, GOLD));
    let label = if turns <= 1 { words.hud.this_turn.to_owned() } else { i18n::tf!("shell.hud.turns", n = turns) };
    outlined(painter, *end + vec2(11.0, -9.0), Align2::LEFT_TOP, &label, 12.0, CREAM);
}

/// Dashes from `a` to `b` that march towards `b` (`phase` 0..1).
fn dashes(painter: &egui::Painter, a: Pos2, b: Pos2, phase: f32, color: Color32) {
    const DASH: f32 = 9.0;
    const GAP: f32 = 5.0;
    let length = a.distance(b);
    if length < 1.0 {
        return;
    }
    let dir = (b - a) / length;
    let mut s = (phase - 1.0) * (DASH + GAP);
    while s < length {
        let (from, to) = (s.max(0.0), (s + DASH).min(length));
        if to > from {
            painter.line_segment([a + dir * from, a + dir * to], Stroke::new(2.4, color));
        }
        s += DASH + GAP;
    }
}

fn draw_mark(painter: &egui::Painter, view: &View<'_>, mark: &ScreenMark, overlay: &MapOverlay, words: &ShellWords) {
    let at = [mark.mark.at[0], mark.ground, mark.mark.at[1]];
    let t = mark.progress;
    match &mark.mark.kind {
        MarkKind::Damage { amount, owner } => {
            let color = if *owner == overlay.observer { LOSS } else { HIT };
            float_text(painter, view, at, t, &format!("-{amount}"), 20.0, color, overlay.reduced_motion);
        }
        MarkKind::Word(word) => {
            let word = match word {
                Floating::Grew => "+1",
                Floating::Captured => words.hud.captured,
                Floating::Destroyed => words.hud.destroyed,
            };
            float_text(painter, view, at, t, word, 16.0, CREAM, overlay.reduced_motion)
        }
        MarkKind::Flash => flash(painter, view, at, t),
        MarkKind::Puff { large } => puff(painter, view, at, t, *large),
        MarkKind::Sparkle { large } => sparkle(painter, view, at, t, *large),
        MarkKind::Warning { radius } => warning(painter, view, at, t, *radius, overlay.reduced_motion),
    }
}

/// Text that rises and fades (or only fades with reduced motion).
#[allow(clippy::too_many_arguments)]
fn float_text(painter: &egui::Painter, view: &View<'_>, at: [f32; 3], t: f32, text: &str, size: f32, color: Color32, reduced: bool) {
    let rise = if reduced { 0.0 } else { t * 0.7 };
    let Some(p) = view.project([at[0], at[1] + 0.9 + rise, at[2]]) else {
        return;
    };
    let alpha = if t < 0.6 { 1.0 } else { 1.0 - (t - 0.6) / 0.4 };
    let pop = if reduced { 1.0 } else { 1.0 + 0.35 * (1.0 - (t / 0.12).min(1.0)) };
    outlined(painter, p, Align2::CENTER_CENTER, text, size * pop, color.gamma_multiply(alpha));
}

fn outlined(painter: &egui::Painter, p: Pos2, align: Align2, text: &str, size: f32, color: Color32) {
    let font = FontId::proportional(size);
    let shadow = Color32::from_black_alpha((color.a() as f32 * 0.85) as u8);
    for offset in [vec2(-1.2, 0.0), vec2(1.2, 0.0), vec2(0.0, -1.2), vec2(0.0, 1.2), vec2(1.0, 1.6)] {
        painter.text(p + offset, align, text, font.clone(), shadow);
    }
    painter.text(p, align, text, font, color);
}

fn flash(painter: &egui::Painter, view: &View<'_>, at: [f32; 3], t: f32) {
    let Some(p) = view.project([at[0], at[1] + 0.35, at[2]]) else {
        return;
    };
    let alpha = ((1.0 - t) * 220.0) as u8;
    painter.circle_filled(p, 10.0 + 34.0 * t, Color32::from_rgba_unmultiplied(255, 236, 170, alpha / 2));
    painter.circle_stroke(p, 6.0 + 44.0 * t, Stroke::new(3.0 * (1.0 - t), Color32::from_rgba_unmultiplied(255, 250, 225, alpha)));
}

/// The warning of a nuclear launch: the blast area as a red ring that pulses faster as the
/// warhead comes down (steady with reduced motion), and a mark on the aim point.
fn warning(painter: &egui::Painter, view: &View<'_>, at: [f32; 3], t: f32, radius: f32, reduced: bool) {
    let pulse = if reduced { 0.75 } else { 0.55 + 0.45 * (t * (6.0 + 22.0 * t)).sin().abs() };
    let alpha = (pulse * 210.0) as u8;
    let ring = view.ring([at[0], at[1] + 0.05, at[2]], radius);
    painter.add(egui::Shape::line(ring, Stroke::new(2.6, Color32::from_rgba_unmultiplied(235, 60, 40, alpha))));
    let inner = view.ring([at[0], at[1] + 0.05, at[2]], radius * 0.45);
    painter.add(egui::Shape::line(inner, Stroke::new(1.6, Color32::from_rgba_unmultiplied(255, 150, 60, alpha / 2))));
    if let Some(p) = view.project([at[0], at[1] + 0.05, at[2]]) {
        let arm = 9.0;
        let stroke = Stroke::new(2.2, Color32::from_rgba_unmultiplied(255, 90, 60, alpha));
        painter.line_segment([p - vec2(arm, 0.0), p + vec2(arm, 0.0)], stroke);
        painter.line_segment([p - vec2(0.0, arm), p + vec2(0.0, arm)], stroke);
    }
}

fn puff(painter: &egui::Painter, view: &View<'_>, at: [f32; 3], t: f32, large: bool) {
    let reach = if large { 1.9 } else { 1.1 };
    for (delay, width) in [(0.0, 3.0), (0.22, 2.0)] {
        let u = ((t - delay) / (1.0 - delay)).clamp(0.0, 1.0);
        if u <= 0.0 || u >= 1.0 {
            continue;
        }
        let ring = view.ring([at[0], at[1] + 0.05, at[2]], 0.25 + reach * u);
        let alpha = ((1.0 - u) * 200.0) as u8;
        painter.add(egui::Shape::line(ring, Stroke::new(width, Color32::from_rgba_unmultiplied(244, 236, 212, alpha))));
    }
}

fn sparkle(painter: &egui::Painter, view: &View<'_>, at: [f32; 3], t: f32, large: bool) {
    let count = if large { 14 } else { 7 };
    let spread = if large { 1.2 } else { 0.7 };
    for i in 0..count {
        let a = i as f32 * 2.399_963; // golden angle
        let r = spread * (0.35 + 0.65 * ((i * 7 % count) as f32 / count as f32));
        let height = 0.4 + t * (0.9 + 0.08 * i as f32);
        let Some(p) = view.project([at[0] + r * a.cos(), at[1] + height, at[2] + r * a.sin()]) else {
            continue;
        };
        let twinkle = ((t * 9.0 + i as f32).sin() * 0.5 + 0.5) * (1.0 - t);
        let size = if large { 4.5 } else { 3.2 } * (0.5 + twinkle);
        let color = Color32::from_rgba_unmultiplied(255, 220, 120, (twinkle * 255.0) as u8);
        painter.line_segment([p - vec2(size, 0.0), p + vec2(size, 0.0)], Stroke::new(1.6, color));
        painter.line_segment([p - vec2(0.0, size), p + vec2(0.0, size)], Stroke::new(1.6, color));
    }
}
