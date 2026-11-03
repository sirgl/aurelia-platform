//! The bar of the clock: the round, pause and step, and the eight speeds with the
//! running one lit. After the end the controls give way to the button of the results.
use egui::{pos2, vec2, Align2, FontId, Rect, Sense};

use super::scene::{elide, round_line, Action, View};
use super::speed::{Pace, Speed};
use crate::shell::kit::{button, panel};
use crate::shell::theme::Theme;

const PAD: f32 = 12.0;
const GAP: f32 = 4.0;
const BUTTON_HEIGHT: f32 = 30.0;
const PAUSE_WIDTH: f32 = 80.0;
const STEP_WIDTH: f32 = 56.0;
const ROUND_SHARE: f32 = 0.19;
const VIEW_WIDTH: f32 = 112.0;
const REVEAL_WIDTH: f32 = 96.0;

/// Where the bar's parts stand.
#[derive(Debug, Clone, PartialEq)]
pub struct Parts {
    pub round: Rect,
    pub pause: Rect,
    pub step: Rect,
    pub speeds: [Rect; 8],
    /// The button between the grid and one power.
    pub view: Rect,
    /// The button that opens the whole map.
    pub reveal: Rect,
}

/// The parts of a bar, left to right.
pub fn parts(bar: Rect) -> Parts {
    let top = bar.center().y - BUTTON_HEIGHT / 2.0;
    let at = |x: f32, w: f32| Rect::from_min_size(pos2(x, top), vec2(w, BUTTON_HEIGHT));
    let round = at(bar.left() + PAD, (bar.width() * ROUND_SHARE).max(80.0));
    let pause = at(round.right() + PAD, PAUSE_WIDTH);
    let step = at(pause.right() + GAP, STEP_WIDTH);
    let view = at(bar.right() - PAD - VIEW_WIDTH, VIEW_WIDTH);
    let reveal = at(view.left() - GAP - REVEAL_WIDTH, REVEAL_WIDTH);
    let from = step.right() + PAD;
    let each = ((reveal.left() - PAD - from - 7.0 * GAP) / 8.0).max(8.0);
    let speeds = std::array::from_fn(|i| at(from + i as f32 * (each + GAP), each));
    Parts { round, pause, step, speeds, view, reveal }
}

pub fn draw(ui: &mut egui::Ui, view: &View<'_>) -> Vec<Action> {
    let bar = view.frame.bar;
    panel(ui.painter(), bar);
    let parts = parts(bar);
    round_text(ui, parts.round, view);
    let mut actions = view_button(ui, parts.view, view);
    if tipped(ui, parts.reveal, view.words.reveal, view.obs.revealed(), true, view.words.reveal_tip) {
        actions.push(Action::ToggleReveal);
    }
    if view.obs.is_over() {
        actions.extend(finale_button(ui, &parts, view));
    } else {
        actions.extend(controls(ui, &parts, view));
    }
    actions
}

/// "One power" in the grid, "Grid" over one power.
fn view_button(ui: &mut egui::Ui, r: Rect, view: &View<'_>) -> Vec<Action> {
    let words = view.words;
    let (label, tip) = if view.focus.is_some() { (words.view_grid, words.view_grid_tip) } else { (words.view_single, words.view_single_tip) };
    tipped(ui, r, label, view.focus.is_some(), true, tip).then_some(Action::ToggleView).into_iter().collect()
}

fn round_text(ui: &egui::Ui, r: Rect, view: &View<'_>) {
    let colors = Theme::of(ui.ctx()).colors;
    let pace = if view.obs.is_over() { Pace::Running } else { view.clock.pace() };
    let line = round_line(view.words, view.obs.round(), pace);
    let line = elide(ui.painter(), &line, 15.0, r.width());
    ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, line, FontId::proportional(15.0), colors.text);
}

fn finale_button(ui: &mut egui::Ui, parts: &Parts, view: &View<'_>) -> Vec<Action> {
    let r = Rect::from_min_max(parts.pause.min, parts.step.max);
    let label = view.words.finale_title;
    let wide = Rect::from_min_size(r.min, vec2(r.width().max(150.0), r.height()));
    let clicked = button(ui, wide, label, view.finale_open, true);
    let action = if view.finale_open { Action::CloseFinale } else { Action::OpenFinale };
    clicked.then_some(action).into_iter().collect()
}

fn controls(ui: &mut egui::Ui, parts: &Parts, view: &View<'_>) -> Vec<Action> {
    let words = view.words;
    let mut actions = Vec::new();
    let paused = view.clock.is_paused();
    let pause_label = if paused { words.resume } else { words.pause };
    if tipped(ui, parts.pause, pause_label, paused, true, words.pause_tip) {
        actions.push(Action::TogglePause);
    }
    if tipped(ui, parts.step, words.step, false, paused, words.step_tip) {
        actions.push(Action::Step);
    }
    for (i, speed) in Speed::ALL.iter().enumerate() {
        let tip = match i {
            0 => words.slower_tip,
            7 => words.faster_tip,
            _ => "",
        };
        if tipped(ui, parts.speeds[i], words.speeds[i], view.clock.speed() == *speed, true, tip) {
            actions.push(Action::Speed(*speed));
        }
    }
    actions
}

/// A button with a hover text.
fn tipped(ui: &mut egui::Ui, r: Rect, label: &str, selected: bool, enabled: bool, tip: &str) -> bool {
    let clicked = button(ui, r, label, selected, enabled);
    if !tip.is_empty() {
        ui.interact(r, ui.id().with(("observer_tip", r.min.x.to_bits())), Sense::hover()).on_hover_text(tip);
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::observer::frame::frame;

    fn overlap(a: Rect, b: Rect) -> bool {
        a.min.x < b.max.x - 0.5 && b.min.x < a.max.x - 0.5 && a.min.y < b.max.y - 0.5 && b.min.y < a.max.y - 0.5
    }

    #[test]
    fn the_parts_of_the_bar_do_not_overlap_and_stay_inside_it() {
        for size in [(1024.0, 600.0), (1280.0, 720.0), (1664.0, 936.0), (2560.0, 1440.0)] {
            let f = frame(Rect::from_min_size(egui::Pos2::ZERO, vec2(size.0, size.1)), 6, None);
            let p = parts(f.bar);
            let all: Vec<Rect> = [p.round, p.pause, p.step, p.view, p.reveal].into_iter().chain(p.speeds).collect();
            for (i, a) in all.iter().enumerate() {
                assert!(f.bar.contains_rect(*a), "{size:?}: {a:?} leaves {:?}", f.bar);
                for b in &all[i + 1..] {
                    assert!(!overlap(*a, *b), "{size:?}: {a:?} overlaps {b:?}");
                }
            }
        }
    }

    /// Every label of the bar fits its button at the small windows in every shipped language
    /// (with that language's fonts: the CJK fallback is installed first). The failures are
    /// collected, so one run lists every label that needs a shorter translation.
    #[test]
    fn every_label_fits_its_button_at_the_small_windows() {
        let failures = std::cell::RefCell::new(Vec::<String>::new());
        // The pseudo-locale is for the panel tests (overlay.rs): its brackets would not fit any button.
        for tag in ["en", "ru", "de", "fr", "es", "ja", "zh-Hans"] {
            let ctx = egui::Context::default();
            crate::shell::theme::install_fonts(&ctx, &crate::shell::theme::Theme::AURELIA, tag);
            // egui applies new fonts at the start of the next pass.
            for _ in 0..2 {
                failures.borrow_mut().clear();
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    for width in [1024.0, 1280.0] {
                        let f = frame(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 720.0)), 6, None);
                        let p = parts(f.bar);
                        i18n::with_language(tag, || {
                            let w = &crate::shell::words::current().observer;
                            let measure = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), FontId::proportional(14.0), egui::Color32::WHITE).size().x;
                            let mut check = |ok: bool, what: String| {
                                if !ok {
                                    failures.borrow_mut().push(format!("{tag} {width}: {what}"));
                                }
                            };
                            for (i, label) in w.speeds.iter().enumerate() {
                                check(measure(label) + 6.0 <= p.speeds[i].width(), format!("speed {label} in {:?}", p.speeds[i]));
                            }
                            check(measure(w.pause) + 6.0 <= p.pause.width() && measure(w.resume) + 6.0 <= p.pause.width(), format!("{} / {} in {:?}", w.pause, w.resume, p.pause));
                            check(measure(w.step) + 6.0 <= p.step.width(), format!("{} in {:?}", w.step, p.step));
                            check(measure(w.view_single) + 6.0 <= p.view.width() && measure(w.view_grid) + 6.0 <= p.view.width(), format!("{} / {} in {:?}", w.view_single, w.view_grid, p.view));
                            check(measure(w.reveal) + 6.0 <= p.reveal.width(), format!("{} in {:?}", w.reveal, p.reveal));
                            let longest = round_line(w, super::super::contract::Round { now: 200, cap: Some(200) }, Pace::Paused);
                            check(measure(&longest) <= p.round.width() + 40.0, format!("{longest} is wider than its slot {:?}", p.round));
                        });
                    }
                });
                output.textures_delta.clear();
            }
            let failed = failures.borrow().clone();
            assert!(failed.is_empty(), "labels wider than their buttons:\n{}", failed.join("\n"));
        }
    }
}
