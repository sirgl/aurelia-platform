//! The observer's screen in egui: the panels with their headers, the bar of the clock,
//! the hint line and, after the end, the standard results card. It draws what the
//! shell hands it and answers with what the viewer asked for (`Action`); it owns no
//! state and reads no rules.
use egui::{pos2, Color32, FontId, Rect, Sense, TextureId};

use super::{clock_bar, header as panel_header};
use super::contract::{Observed, Round};
use super::frame::{header, picture, Frame};
use super::panels::{CameraMode, Label};
use super::inspect::Inspect;
use super::overlay::{self, Marks};
use super::{chips, single};
use super::speed::{Clock, Pace, Speed};
use crate::shell::kit::{panel, text};
use crate::shell::layout::{self, Anchor, Panel};
use crate::shell::theme::Theme;
use crate::shell::words::ObserverWords;

/// What the viewer asked for with the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TogglePause,
    Step,
    Slower,
    Faster,
    Speed(Speed),
    /// Show a power alone.
    Focus(usize),
    /// The grid becomes one power, and one power the grid.
    ToggleView,
    /// How the camera of the power shown alone moves.
    Camera(CameraMode),
    /// Open the card of the end again.
    OpenFinale,
    /// Close the card of the end.
    CloseFinale,
    Menu,
    /// Open the whole map to every view, or close it again.
    ToggleReveal,
    /// The cards of the shown power's cities: open the first, or close the open one.
    ToggleCities,
    /// The next (true) or the previous city on the open card.
    CardStep(bool),
    /// Close the city card.
    CardClose,
}

/// Everything one frame of the scene draws.
pub struct View<'a> {
    pub obs: &'a dyn Observed,
    pub words: &'a ObserverWords,
    pub clock: &'a Clock,
    pub frame: &'a Frame,
    pub textures: &'a [Option<TextureId>],
    /// City names over the enlarged panel (empty in the grid).
    pub labels: &'a [Label],
    pub focus: Option<usize>,
    /// How the camera of the power shown alone moves.
    pub camera: CameraMode,
    pub finale_open: bool,
    /// What the viewer looks at: the tile under the pointer, the unit, the city card.
    pub inspect: &'a Inspect,
    /// The outlines to draw over the picture of the power shown alone.
    pub marks: &'a Marks,
}

/// What one frame of the scene asked for, and where its cards stood.
#[derive(Default)]
pub struct Drawn {
    pub actions: Vec<Action>,
    /// The cards over the map: the pointer over one of them is not over the map.
    pub covered: Vec<Rect>,
}

/// Draw the screen; the actions the pointer asked for.
pub fn draw(ctx: &egui::Context, view: &View<'_>) -> Drawn {
    layout::begin(ctx, ctx.content_rect(), Vec::new());
    layout::place(ctx, Panel::at("observer_bar", Anchor::Top, view.frame.bar));
    let mut actions = Vec::new();
    egui::Area::new("observer_panels".into()).order(egui::Order::Middle).fixed_pos(pos2(0.0, 0.0)).constrain(false).show(ctx, |ui| {
        for cell in &view.frame.cells {
            panel_cell(ui, view, cell.power, cell.rect, &mut actions);
        }
        let line = elide(ui.painter(), hint(view), 12.0, view.frame.hint.width());
        text(ui.painter(), view.frame.hint.min, line, 12.0, Theme::of(ui.ctx()).colors.muted);
    });
    egui::Area::new("observer_bar".into()).order(egui::Order::Foreground).fixed_pos(view.frame.bar.min).show(ctx, |ui| {
        ui.set_min_size(view.frame.bar.size());
        actions.extend(clock_bar::draw(ui, view));
    });
    if let Some(row) = view.frame.chips {
        egui::Area::new("observer_chips".into()).order(egui::Order::Foreground).fixed_pos(row.min).show(ctx, |ui| {
            ui.set_min_size(row.size());
            actions.extend(chips::draw(ui, view));
        });
    }
    let covered = cards(ctx, view, &mut actions);
    if let Some(hover) = &view.inspect.hover {
        overlay::tooltip(ctx, ctx.content_rect(), hover, view.words, view.focus.is_some());
    }
    Drawn { actions, covered }
}

/// The card of the chosen unit or of the open city, over the picture of the power shown
/// alone; where it stands.
fn cards(ctx: &egui::Context, view: &View<'_>, actions: &mut Vec<Action>) -> Vec<Rect> {
    let Some(power) = view.focus else { return Vec::new() };
    let Some(cell) = view.frame.cell(power) else { return Vec::new() };
    let shown = picture(cell, view.frame.head).shrink(1.0);
    if let Some(open) = view.inspect.city.as_ref().filter(|c| c.power == power) {
        let placed = overlay::place_city(ctx, shown, open);
        egui::Area::new("observer_city_card".into()).order(egui::Order::Foreground).fixed_pos(placed.rect.min).show(ctx, |ui| {
            actions.extend(overlay::draw_city(ui, &placed, open, view.words));
        });
        return vec![placed.rect];
    }
    if let Some(unit) = view.inspect.unit.as_ref().filter(|_| view.inspect.chosen.is_some_and(|c| c.power == power)) {
        let placed = overlay::place_unit(ctx, shown, unit, view.words);
        egui::Area::new("observer_unit_card".into()).order(egui::Order::Foreground).fixed_pos(placed.rect.min).show(ctx, |ui| overlay::draw_unit(ui, &placed));
        return vec![placed.rect];
    }
    Vec::new()
}

fn hint(view: &View<'_>) -> &'static str {
    match view.focus {
        Some(_) => view.words.zoom_hint,
        None => view.words.grid_hint,
    }
}

/// One power's panel: the header, the picture, a click to enlarge or to go back.
fn panel_cell(ui: &mut egui::Ui, view: &View<'_>, power: usize, cell: Rect, actions: &mut Vec<Action>) {
    let colors = Theme::of(ui.ctx()).colors;
    panel(ui.painter(), cell);
    let alive = view.obs.alive(power);
    let head = header(cell, view.frame.head);
    if view.focus == Some(power) {
        single::draw(ui, head, view, power);
    } else {
        panel_header::draw(ui, head, view, power);
    }
    let shown = picture(cell, view.frame.head).shrink(1.0);
    match view.textures.get(power).copied().flatten() {
        Some(texture) => {
            ui.painter().image(texture, shown, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(shown, 0.0, colors.background);
        }
    }
    if !alive {
        ui.painter().rect_filled(shown, 0.0, Color32::from_black_alpha(120));
    }
    if view.focus == Some(power) {
        overlay::paint_marks(ui.painter(), shown, view.marks);
        city_names(ui.painter(), shown, view.labels);
    }
    if view.obs.revealed() && alive {
        let tag = if view.focus == Some(power) { view.words.reveal_tag } else { view.words.reveal };
        overlay::paint_reveal_tag(ui.painter(), shown, tag);
    }
    // The grid's panels are buttons; the panel of one power is a map to drag.
    if view.focus.is_none() && ui.interact(cell, ui.id().with(("observer_panel", power)), Sense::click()).clicked() {
        actions.push(Action::Focus(power));
    }
}

/// The names of the cities, over the picture, each on a dark shadow so it reads on any ground.
fn city_names(painter: &egui::Painter, shown: Rect, labels: &[Label]) {
    let font = FontId::proportional(13.0);
    for label in labels {
        let at = shown.min + label.at.to_vec2() * shown.size() - egui::vec2(0.0, 10.0);
        painter.text(at + egui::vec2(1.0, 1.0), egui::Align2::CENTER_BOTTOM, &label.name, font.clone(), Color32::from_black_alpha(220));
        painter.text(at, egui::Align2::CENTER_BOTTOM, &label.name, font.clone(), Color32::WHITE);
    }
}

/// The round line of the bar: "Round 41 of 200", and "paused" when paused.
pub fn round_line(words: &ObserverWords, round: Round, pace: Pace) -> String {
    let line = match round.cap {
        Some(cap) => i18n::tf!("shell.observer.turn-of", round = round.now, limit = cap),
        None => i18n::tf!("shell.observer.turn", round = round.now),
    };
    if pace == Pace::Paused {
        format!("{line} · {}", words.paused)
    } else {
        line
    }
}

/// Text cut to `width` with an ellipsis (the whole text when it fits).
pub fn elide(painter: &egui::Painter, value: &str, size: f32, width: f32) -> String {
    let measure = |s: &str| painter.layout_no_wrap(s.to_owned(), FontId::proportional(size), Color32::WHITE).size().x;
    if measure(value) <= width {
        return value.to_owned();
    }
    let chars: Vec<char> = value.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        keep -= 1;
        let cut: String = chars[..keep].iter().collect::<String>().trim_end().to_owned() + "…";
        if measure(&cut) <= width {
            return cut;
        }
    }
    String::new()
}
