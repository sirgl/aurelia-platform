//! The row under the bar in the view of one power: a chip for every power (its colour
//! and name; the one shown is lit, a fallen one is grey) and the two camera buttons.
//! A click on a chip shows that power.
use egui::{pos2, vec2, Align2, FontId, Rect, Sense, Stroke};

use super::header::power_color;
use super::panels::CameraMode;
use super::scene::{elide, Action, View};
use crate::shell::kit::button;
use crate::shell::theme::Theme;

const GAP: f32 = 6.0;
const CHIP_MAX: f32 = 190.0;
const FOLLOW_WIDTH: f32 = 96.0;
const FREE_WIDTH: f32 = 150.0;
const CITIES_WIDTH: f32 = 100.0;
const DOT: f32 = 6.0;

/// Where the row's parts stand.
#[derive(Debug, Clone, PartialEq)]
pub struct Parts {
    pub chips: Vec<Rect>,
    pub cities: Rect,
    pub follow: Rect,
    pub free: Rect,
}

/// The parts of a row of `count` chips in `row`: the camera buttons at the right end, the
/// chips side by side in what is left.
pub fn parts(row: Rect, count: usize) -> Parts {
    let free = Rect::from_min_max(pos2(row.right() - FREE_WIDTH, row.top()), row.max);
    let follow = Rect::from_min_max(pos2(free.left() - GAP - FOLLOW_WIDTH, row.top()), pos2(free.left() - GAP, row.bottom()));
    let cities = Rect::from_min_max(pos2(follow.left() - GAP - CITIES_WIDTH, row.top()), pos2(follow.left() - GAP, row.bottom()));
    let room = (cities.left() - GAP - row.left()).max(1.0);
    let each = ((room - GAP * count.saturating_sub(1) as f32) / count.max(1) as f32).clamp(1.0, CHIP_MAX);
    let chips = (0..count).map(|i| Rect::from_min_size(pos2(row.left() + i as f32 * (each + GAP), row.top()), vec2(each, row.height()))).collect();
    Parts { chips, cities, follow, free }
}

pub fn draw(ui: &mut egui::Ui, view: &View<'_>) -> Vec<Action> {
    let Some(row) = view.frame.chips else { return Vec::new() };
    let parts = parts(row, view.obs.powers().len());
    let mut actions: Vec<Action> = (0..parts.chips.len()).filter(|&power| chip(ui, parts.chips[power], view, power)).map(Action::Focus).collect();
    let words = view.words;
    let mode = view.camera;
    let any = view.focus.is_some_and(|power| {
        let world = view.obs.world(power);
        world.cities.iter().any(|c| c.owner == world.observer)
    });
    let open = view.inspect.city.is_some();
    let clicked = button(ui, parts.cities, words.cities, open, any);
    ui.interact(parts.cities, ui.id().with("observer_cities_tip"), Sense::hover()).on_hover_text(words.cities_tip);
    if clicked {
        actions.push(Action::ToggleCities);
    }
    if tipped(ui, parts.follow, words.camera_follow, mode == CameraMode::Follow, words.camera_follow_tip) {
        actions.push(Action::Camera(CameraMode::Follow));
    }
    if tipped(ui, parts.free, words.camera_free, mode == CameraMode::Free, words.camera_free_tip) {
        actions.push(Action::Camera(CameraMode::Free));
    }
    actions
}

fn tipped(ui: &mut egui::Ui, r: Rect, label: &str, selected: bool, tip: &str) -> bool {
    let clicked = button(ui, r, label, selected, true);
    ui.interact(r, ui.id().with(("observer_camera_tip", r.min.x.to_bits())), Sense::hover()).on_hover_text(tip);
    clicked
}

/// One power's chip; true when it was clicked.
fn chip(ui: &mut egui::Ui, r: Rect, view: &View<'_>, power: usize) -> bool {
    let Some(info) = view.obs.powers().get(power) else { return false };
    let theme = Theme::of(ui.ctx());
    let colors = &theme.colors;
    let response = ui.interact(r, ui.id().with(("observer_chip", power)), Sense::click());
    let shown = view.focus == Some(power);
    let fill = if shown { colors.selected } else if response.hovered() { colors.hover } else { colors.raised };
    let edge = if shown || response.hovered() { colors.selected_edge } else { colors.edge };
    ui.painter().rect_filled(r, theme.shapes.button_radius, fill);
    ui.painter().rect_stroke(r, theme.shapes.button_radius, Stroke::new(theme.shapes.edge_width, edge), egui::StrokeKind::Inside);
    let alive = view.obs.alive(power);
    let ink = if alive { colors.text } else { colors.muted };
    let dot = if alive { power_color(info.player) } else { colors.muted };
    ui.painter().circle_filled(pos2(r.left() + 10.0 + DOT, r.center().y), DOT, dot);
    let room = (r.width() - 22.0 - DOT * 2.0).max(1.0);
    let name = elide(ui.painter(), &info.name, 14.0, room);
    ui.painter().text(pos2(r.left() + 20.0 + DOT * 2.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(14.0), ink);
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::observer::frame::frame;

    fn overlap(a: Rect, b: Rect) -> bool {
        a.min.x < b.max.x - 0.5 && b.min.x < a.max.x - 0.5 && a.min.y < b.max.y - 0.5 && b.min.y < a.max.y - 0.5
    }

    #[test]
    fn the_chips_and_the_camera_buttons_do_not_overlap_and_stay_in_the_row() {
        for size in [(1024.0, 600.0), (1280.0, 720.0), (1664.0, 936.0), (2560.0, 1440.0)] {
            for count in 1..=8 {
                let f = frame(Rect::from_min_size(egui::Pos2::ZERO, vec2(size.0, size.1)), count, Some(0));
                let row = f.chips.expect("one power has its chips");
                let p = parts(row, count);
                let all: Vec<Rect> = p.chips.iter().copied().chain([p.cities, p.follow, p.free]).collect();
                for (i, a) in all.iter().enumerate() {
                    assert!(row.expand(0.01).contains_rect(*a), "{size:?} {count}: {a:?} leaves {row:?}");
                    for b in &all[i + 1..] {
                        assert!(!overlap(*a, *b), "{size:?} {count}: {a:?} overlaps {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn eight_chips_stay_wide_enough_for_a_name_at_the_smallest_window() {
        let f = frame(Rect::from_min_size(egui::Pos2::ZERO, vec2(1024.0, 600.0)), 8, Some(0));
        let p = parts(f.chips.unwrap(), 8);
        assert!(p.chips.iter().all(|c| c.width() >= 60.0), "{:?}", p.chips);
    }

    #[test]
    fn the_grid_has_no_chips() {
        assert!(frame(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 720.0)), 6, None).chips.is_none());
    }
}
