//! Where key presses go: the routing of a chord to an action, the "Keyboard shortcuts" overlay
//! and the remapping rows of the Settings screen (Controls page). The table and the
//! matching are `keymap`; this file is the shell's one dispatcher and the screens on top of it.
use super::kit::{button, panel, text};
use super::keymap::{Binding, Chord, KeyContext, Keymap, Overrides, SHELL_NAME};
use super::module::{ModuleUi, Screen, ShellAction};
use super::screens::ScreenState;
use super::theme::Theme;
use egui::{pos2, vec2, Align2, FontId, Rect, Sense, Stroke};
use i18n::t;
use std::sync::Arc;

/// Camera zoom factor of one key press.
pub const KEY_ZOOM_STEP: f32 = 1.15;

/// What a key press did.
#[derive(Debug, Clone, PartialEq)]
pub enum Routed {
    /// No action has this key here.
    None,
    /// The shell used it (the help, a camera key held down elsewhere); nothing to apply.
    Used,
    /// The shell asks to carry out this action.
    Shell(ShellAction),
    /// The module did it (its queued `ShellAction`s are to be applied).
    Module,
}

/// The remapping of one action in progress.
#[derive(Debug, Clone, Default, PartialEq)]
enum Edit {
    #[default]
    Idle,
    /// The next key press becomes the keys of the action.
    Listening(&'static str),
    /// The key is used by other actions: replace them or cancel.
    Replace { id: &'static str, chord: Chord, with: Vec<&'static str> },
}

/// State of the shortcuts screens inside `ScreenState`.
#[derive(Default)]
pub struct ShortcutsUi {
    /// The "Keyboard shortcuts" overlay is open.
    pub open: bool,
    edit: Edit,
    cache: Option<Cached>,
}

struct Cached {
    set: &'static str,
    saved: Overrides,
    map: Arc<Keymap>,
}

impl ShortcutsUi {
    /// Whether the next key press is meant for the remapping.
    pub fn listening(&self) -> bool {
        matches!(self.edit, Edit::Listening(_))
    }

    /// Starts a remapping: the next key press becomes the keys of `id`.
    pub fn listen(&mut self, id: &'static str) {
        self.edit = Edit::Listening(id);
    }

    /// The key a remapping waits to give away from other actions, and who has it now.
    pub fn pending_replace(&self) -> Option<(Chord, Vec<&'static str>)> {
        match &self.edit {
            Edit::Replace { chord, with, .. } => Some((*chord, with.clone())),
            _ => None,
        }
    }

    /// Esc: ends a remapping, else closes the overlay. True when it did something.
    pub fn cancel(&mut self) -> bool {
        if self.edit != Edit::Idle {
            self.edit = Edit::Idle;
            return true;
        }
        std::mem::take(&mut self.open)
    }
}

impl ScreenState {
    /// The keymap of the running module with the player's changes (kept until either changes).
    pub fn keymap(&mut self, module: &dyn ModuleUi) -> Arc<Keymap> {
        let set = module.shortcuts();
        if let Some(cached) = &self.shortcuts.cache {
            if cached.set == set.name && cached.saved == self.settings.keys {
                return cached.map.clone();
            }
        }
        let map = Arc::new(Keymap::build(set, &self.settings.keys));
        self.shortcuts.cache = Some(Cached { set: set.name, saved: self.settings.keys.clone(), map: map.clone() });
        map
    }

    /// Routes a key press to the action that has it where the player is, and carries out
    /// what the shell does itself; the rest is for the caller (`Routed`).
    pub fn route_key(&mut self, module: &mut dyn ModuleUi, chord: Chord) -> Routed {
        let map = self.keymap(module);
        let context = KeyContext {
            in_game: module.view().is_some(),
            screen: self.screen,
            unit_selected: module.unit_selected(),
            overlay: self.shortcuts.open,
        };
        for binding in map.resolve(chord, &context) {
            if !binding.shell {
                if module.shortcut(binding.def.id, self.flags()) {
                    return Routed::Module;
                }
                continue;
            }
            return match binding.def.id {
                "shortcuts" => {
                    self.shortcuts.open = !self.shortcuts.open;
                    Routed::Used
                }
                id if id.starts_with("pan-") => Routed::Used,
                "end-turn" => Routed::Shell(ShellAction::EndTurn),
                "center-selection" => Routed::Shell(ShellAction::CenterOnSelection),
                "toggle-grid" => Routed::Shell(ShellAction::ToggleGrid),
                "toggle-yields" => Routed::Shell(ShellAction::ToggleYields),
                "zoom-in" => Routed::Shell(ShellAction::Zoom(KEY_ZOOM_STEP)),
                "zoom-out" => Routed::Shell(ShellAction::Zoom(1.0 / KEY_ZOOM_STEP)),
                "quick-save" => Routed::Shell(ShellAction::Save),
                "quick-load" => Routed::Shell(ShellAction::Load),
                "settings" => Routed::Shell(ShellAction::Open(Screen::Settings)),
                _ => continue,
            };
        }
        Routed::None
    }

    /// While an action waits for its new key: takes the chord of a key press. True when the
    /// press was meant for the remapping.
    pub fn capture_key(&mut self, module: &dyn ModuleUi, chord: Chord) -> bool {
        let Edit::Listening(id) = self.shortcuts.edit else { return false };
        let map = self.keymap(module);
        let conflicts: Vec<&'static str> = map.conflicts(id, chord).into_iter().map(|c| c.id).collect();
        if conflicts.is_empty() {
            self.rebind(module, id, chord, false);
            self.shortcuts.edit = Edit::Idle;
        } else {
            self.shortcuts.edit = Edit::Replace { id, chord, with: conflicts };
        }
        true
    }

    /// Takes the pending key from the actions that have it.
    pub fn confirm_replace(&mut self, module: &dyn ModuleUi) {
        if let Edit::Replace { id, chord, .. } = std::mem::take(&mut self.shortcuts.edit) {
            self.rebind(module, id, chord, true);
        }
    }

    fn rebind(&mut self, module: &dyn ModuleUi, id: &'static str, chord: Chord, replace: bool) {
        let mut map = (*self.keymap(module)).clone();
        map.rebind(id, chord, replace);
        map.write_overrides(&mut self.settings.keys);
    }

    fn edit_keymap(&mut self, module: &dyn ModuleUi, change: impl FnOnce(&mut Keymap)) {
        let mut map = (*self.keymap(module)).clone();
        change(&mut map);
        map.write_overrides(&mut self.settings.keys);
    }
}

fn group_label(group: &str) -> String {
    // i18n-keys: shortcut.group.*
    i18n::tr_dynamic(&format!("shortcut.group.{group}"), &[]).unwrap_or_else(|| group.to_owned())
}

/// A keycap: the chord in a small box, left edge at `x`; returns its right edge.
fn keycap(p: &egui::Painter, x: f32, centre_y: f32, caption: &str, colors: &super::theme::Colors) -> f32 {
    let galley = p.layout_no_wrap(caption.to_owned(), FontId::proportional(13.0), colors.accent);
    let size = galley.size() + vec2(14.0, 6.0);
    let r = Rect::from_min_size(pos2(x, centre_y - size.y * 0.5), size);
    p.rect_filled(r, 4.0, colors.panel);
    p.rect_stroke(r, 4.0, Stroke::new(1.0, colors.edge), egui::StrokeKind::Inside);
    p.galley(r.min + vec2(7.0, 3.0), galley, colors.accent);
    r.right() + 6.0
}

const ROW: f32 = 34.0;

fn heading(ui: &mut egui::Ui, width: f32, title: &str) {
    let c = Theme::of(ui.ctx()).colors;
    let (rect, _) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
    text(ui.painter(), rect.min + vec2(4.0, 8.0), title, 15.0, c.accent);
}

/// Paints the keys of a row from `x`; returns where they end.
fn keys_of(p: &egui::Painter, x: f32, centre_y: f32, binding: &Binding, colors: &super::theme::Colors) -> f32 {
    if binding.chords.is_empty() {
        p.text(pos2(x, centre_y), Align2::LEFT_CENTER, t!("shortcut.ui.unbound"), FontId::proportional(13.0), colors.muted);
        return x;
    }
    let mut at = x;
    for chord in &binding.chords {
        at = keycap(p, at, centre_y, &chord.caption(), colors);
    }
    at
}

/// The table of all actions by group. With `editable` each row has Change, Clear and Reset.
fn table(ui: &mut egui::Ui, state: &mut ScreenState, module: &dyn ModuleUi, keymap: &Keymap, editable: bool) {
    let theme = Theme::of(ui.ctx());
    let c = theme.colors;
    let width = ui.available_width();
    for group in keymap.groups() {
        heading(ui, width, &group_label(group));
        for binding in keymap.bindings().iter().filter(|b| b.def.group == group) {
            let id = binding.def.id;
            let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
            ui.painter().rect_filled(rect, 4.0, c.raised);
            let centre = rect.center().y;
            let label_width = (width * 0.42).max(120.0);
            let label = binding.def.label();
            ui.painter().with_clip_rect(Rect::from_min_size(rect.min, vec2(label_width, ROW))).text(
                pos2(rect.min.x + 12.0, centre),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(15.0),
                c.text,
            );
            let keys_x = rect.min.x + label_width + 16.0;
            if !editable {
                keys_of(ui.painter(), keys_x, centre, binding, &c);
                continue;
            }
            let edit = state.shortcuts.edit.clone();
            let cell = |from_right: f32, w: f32| Rect::from_min_size(pos2(rect.max.x - from_right - w, rect.min.y + 3.0), vec2(w, ROW - 6.0));
            match edit {
                Edit::Listening(listening) if listening == id => {
                    ui.painter().text(pos2(keys_x, centre), Align2::LEFT_CENTER, t!("shortcut.ui.press-key"), FontId::proportional(14.0), c.warning);
                    if button(ui, cell(8.0, 96.0), t!("shortcut.ui.cancel"), false, true) {
                        state.shortcuts.edit = Edit::Idle;
                    }
                }
                Edit::Replace { id: replacing, chord, ref with } if replacing == id => {
                    let others: Vec<String> = with
                        .iter()
                        .filter_map(|other| keymap.binding(other))
                        .map(|b| b.def.label())
                        .collect();
                    let message = i18n::tf!("shortcut.ui.used-by", key = chord.caption(), actions = others.join(", "));
                    ui.painter().text(pos2(keys_x, centre), Align2::LEFT_CENTER, message, FontId::proportional(13.0), c.warning);
                    if button(ui, cell(8.0, 96.0), t!("shortcut.ui.cancel"), false, true) {
                        state.shortcuts.edit = Edit::Idle;
                    }
                    if button(ui, cell(112.0, 96.0), t!("shortcut.ui.replace"), true, true) {
                        state.confirm_replace(module);
                    }
                }
                _ => {
                    keys_of(ui.painter(), keys_x, centre, binding, &c);
                    if button(ui, cell(8.0, 88.0), t!("shortcut.ui.change"), false, true) {
                        state.shortcuts.edit = Edit::Listening(id);
                    }
                    if button(ui, cell(104.0, 72.0), t!("shortcut.ui.clear"), false, !binding.chords.is_empty()) {
                        state.edit_keymap(module, |map| map.unbind(id));
                    }
                    if button(ui, cell(184.0, 72.0), t!("shortcut.ui.reset"), false, !keymap.is_default(id)) {
                        state.edit_keymap(module, |map| map.reset(id));
                    }
                }
            }
        }
        ui.add_space(6.0);
    }
}

/// The Controls page of the Settings screen: pointer help, then every action with its keys.
pub(super) fn controls(ui: &mut egui::Ui, state: &mut ScreenState, module: &dyn ModuleUi, pointer_help: &[(&'static str, &'static str)], intro: &str) {
    let c = Theme::of(ui.ctx()).colors;
    let keymap = state.keymap(module);
    ui.label(egui::RichText::new(intro).size(16.).color(c.text));
    ui.add_space(12.);
    for &(keys, what) in pointer_help {
        let width = ui.available_width();
        let key_width = width.min(230.) * 0.72;
        let description = ui.painter().layout(what.to_owned(), FontId::proportional(15.), c.text, (width - key_width - 40.).max(80.));
        let height = description.size().y.max(22.) + 20.;
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        ui.painter().rect_filled(rect, 4., c.raised);
        text(ui.painter(), rect.min + vec2(12., 10.), keys, 14., c.accent);
        ui.painter().galley(rect.min + vec2(key_width + 24., 10.), description, c.text);
        ui.add_space(6.);
    }
    ui.add_space(10.);
    ui.label(egui::RichText::new(t!("shortcut.ui.settings-note")).size(13.).color(c.muted));
    ui.add_space(8.);
    let width = ui.available_width();
    let (bar, _) = ui.allocate_exact_size(vec2(width, 38.0), Sense::hover());
    let any_changed = !state.settings.keys.is_empty();
    if button(ui, Rect::from_min_size(bar.min, vec2(220.0, 36.0)), t!("shortcut.ui.reset-all"), false, any_changed) {
        state.edit_keymap(module, |map| map.reset_all());
        // Lines of keys that no longer exist or of other modules are not the player's to keep.
        state.settings.keys.retain(|key, _| !key.starts_with(&format!("{SHELL_NAME}.")) && !key.starts_with(&format!("{}.", module.shortcuts().name)));
    }
    if button(ui, Rect::from_min_size(bar.min + vec2(232.0, 0.0), vec2(220.0, 36.0)), t!("shortcut.ui.show-overlay"), false, true) {
        state.shortcuts.open = true;
    }
    ui.add_space(10.0);
    table(ui, state, module, &keymap, true);
}

/// The "Keyboard shortcuts" overlay over any screen: all actions and their keys.
pub(super) fn overlay(ctx: &egui::Context, state: &mut ScreenState, module: &dyn ModuleUi) {
    if !state.shortcuts.open {
        return;
    }
    let keymap = state.keymap(module);
    let theme = Theme::of(ctx);
    let c = theme.colors;
    let screen = ctx.content_rect();
    egui::Area::new("shortcuts_overlay".into())
        .order(egui::Order::Tooltip)
        .fixed_pos(screen.min)
        .constrain(false)
        .show(ctx, |ui| {
            let (full, _) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(full, 0.0, egui::Color32::from_black_alpha(170));
            let width = (screen.width() - 32.0).min(760.0);
            let height = (screen.height() - 32.0).min(760.0);
            let rect = Rect::from_center_size(screen.center(), vec2(width, height));
            panel(ui.painter(), rect);
            let inner = rect.shrink(18.0);
            text(ui.painter(), inner.min, t!("shortcut.ui.title"), 22.0, c.text);
            text(ui.painter(), inner.min + vec2(0.0, 30.0), t!("shortcut.ui.overlay-note"), 13.0, c.muted);
            let close = Rect::from_min_size(pos2(inner.right() - 110.0, inner.top()), vec2(110.0, 34.0));
            let mut close_now = button(ui, close, t!("shortcut.ui.close"), false, true);
            let body = Rect::from_min_max(inner.min + vec2(0.0, 58.0), inner.max);
            ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                ui.set_clip_rect(body);
                egui::ScrollArea::vertical().id_salt("shortcuts_overlay_scroll").auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_min_width(body.width() - 12.0);
                    table(ui, state, module, &keymap, false);
                });
            });
            close_now |= ui.input(|i| i.pointer.any_click()) && ui.input(|i| i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)));
            if close_now {
                state.shortcuts.open = false;
            }
        });
}
