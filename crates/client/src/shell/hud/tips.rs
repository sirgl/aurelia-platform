//! Tooltip bodies shared by the HUD: a yield with its breakdown, a unit with
//! its combat numbers. Data comes from `overview_model`; nothing is computed here.
use crate::shell::module::{ShellWords, UnitTip, YieldTip};
use egui::{Color32, RichText};

const GOLD: Color32 = Color32::from_rgb(193, 162, 99);
const CREAM: Color32 = Color32::from_rgb(239, 233, 211);
const MUTED: Color32 = Color32::from_rgb(146, 168, 170);
const HURT: Color32 = Color32::from_rgb(214, 96, 84);

pub fn yield_tip(ui: &mut egui::Ui, tip: &YieldTip) {
    ui.set_max_width(320.);
    ui.label(RichText::new(tip.title.as_ref()).strong().color(GOLD));
    ui.label(RichText::new(&tip.total).color(CREAM));
    ui.separator();
    egui::Grid::new(("yield_tip", tip.title.as_ref())).num_columns(2).spacing([24., 2.]).show(ui, |ui| {
        for (label, value) in &tip.lines {
            ui.label(RichText::new(label).color(MUTED));
            ui.label(RichText::new(value).color(CREAM));
            ui.end_row();
        }
    });
    if let Some(note) = &tip.note {
        ui.add_space(4.);
        ui.label(RichText::new(note.as_ref()).size(11.).color(MUTED));
    }
}

/// The line under a top-bar value's tip that says a click opens something.
pub fn click_hint(ui: &mut egui::Ui, hint: &str) {
    ui.add_space(4.);
    ui.label(RichText::new(hint).size(11.).color(GOLD));
}

pub fn unit_tip(ui: &mut egui::Ui, unit: &UnitTip, _words: &ShellWords) {
    ui.set_max_width(300.);
    ui.label(RichText::new(&unit.name).strong().color(CREAM));
    if !unit.own {
        ui.label(RichText::new(&unit.owner).color(MUTED));
    }
    let health = if unit.hp < kernel::world::unit::UNIT_MAX_HP { HURT } else { CREAM };
    ui.label(RichText::new(i18n::tf!("shell.hud.health", current = unit.hp, max = kernel::world::unit::UNIT_MAX_HP)).color(health));
    if unit.attack > 0 || unit.defense > 0 {
        ui.label(RichText::new(i18n::tf!("shell.hud.strength", attack = unit.attack, defense = unit.defense)).color(CREAM));
    }
    if unit.ranged > 0 {
        ui.label(RichText::new(i18n::tf!("shell.hud.ranged", strength = unit.ranged, range = unit.range)).color(CREAM));
    }
    if let Some(moves) = &unit.moves {
        ui.label(RichText::new(i18n::tf!("shell.hud.moves", moves = moves)).color(CREAM));
    }
    if let Some(sight) = unit.sight {
        ui.label(RichText::new(i18n::tf!("shell.hud.sight", sight = sight)).color(CREAM));
    }
    if let Some(status) = &unit.status {
        ui.label(RichText::new(status).color(MUTED));
    }
    if !unit.promotions.is_empty() {
        ui.label(RichText::new(unit.promotions.join(", ")).size(11.).color(GOLD));
    }
    ui.add_space(4.);
}
