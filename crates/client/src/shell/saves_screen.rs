//! The dialogs over the saves: "Load game", the list of every save (manual, quick, auto) newest
//! first with what each says about its game, and "Save game", the name a new save gets. Both are
//! drawn over whatever screen is shown (the main menu, the game menu), like the report dialog.
//!
//! The list holds files and times at once (`ModuleSaves::entries`) and reads what a save says
//! about its game (`ModuleUi::describe_save`) a few rows per frame, once per file: the answer is
//! kept for the session under the file's path and time, so an overwritten save is read again and
//! an untouched one never. The dialog asks for work through `ShellAction`s (load, delete, save
//! as); the app does it and hands the new list back.
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use egui::{pos2, vec2, Align2, FontId, Key, Rect, Sense};
use kernel::world::module::ModuleId;

use super::kit::{button, fit_line, panel, text};
use super::module::{ModuleUi, ShellAction};
use super::saves::{file_stem, read_header, SaveEntry, SaveKind, SaveProblem, SaveSummary, Unreadable};
use super::screens::ScreenState;
use super::theme::Theme;
use i18n::{t, tf};

/// What one row of the list knows of its save.
pub type Header = Result<SaveSummary, Unreadable>;

/// Height of one row of the list.
const ROW_HEIGHT: f32 = 58.0;
/// Wall time per frame spent reading headers.
const READ_BUDGET: Duration = Duration::from_millis(6);

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub entry: SaveEntry,
    /// None while the save waits to be read.
    pub header: Option<Header>,
}

#[derive(Debug, Default)]
struct LoadView {
    rows: Vec<Row>,
    selected: usize,
    /// The save whose deletion waits for the player's yes.
    confirm: Option<PathBuf>,
    /// A line over the buttons: what went wrong, or what was done (true: it went wrong).
    notice: Option<(String, bool)>,
    /// The selection moved by key: scroll it into view.
    reveal: bool,
}

#[derive(Debug, Default)]
struct NameView {
    name: String,
    /// The names of the manual saves that exist (a taken name is replaced; the button says so).
    taken: Vec<String>,
    notice: Option<String>,
    /// The field was given the focus (once, with the default name selected).
    focused: bool,
}

#[derive(Debug, Default)]
enum View {
    #[default]
    Closed,
    Load(LoadView),
    Name(NameView),
}

/// The state of the two dialogs; closed until the menu opens one.
#[derive(Debug, Default)]
pub struct SavesUi {
    view: View,
    /// What each save said when it was read, by path and file time (in milliseconds).
    headers: HashMap<(PathBuf, Option<u64>), Header>,
}

fn stamp(entry: &SaveEntry) -> (PathBuf, Option<u64>) {
    (entry.path.clone(), entry.modified.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64))
}

impl SavesUi {
    pub fn is_open(&self) -> bool {
        !matches!(self.view, View::Closed)
    }

    pub fn close(&mut self) {
        self.view = View::Closed;
    }

    /// Esc: a pending deletion first, then the dialog; true when something closed.
    pub fn cancel(&mut self) -> bool {
        if let View::Load(view) = &mut self.view {
            if view.confirm.take().is_some() {
                return true;
            }
        }
        !matches!(std::mem::take(&mut self.view), View::Closed)
    }

    /// Open the list of `entries` (newest first).
    pub fn open_list(&mut self, entries: Vec<SaveEntry>) {
        self.view = View::Load(LoadView::default());
        self.reload(entries);
    }

    /// Show a new list in the open one: the selection stays on the same save, or on its place.
    pub fn reload(&mut self, entries: Vec<SaveEntry>) {
        let View::Load(view) = &mut self.view else { return };
        let kept = view.rows.get(view.selected).map(|r| r.entry.path.clone());
        let rows: Vec<Row> = entries
            .into_iter()
            .map(|entry| {
                let header = self.headers.get(&stamp(&entry)).cloned();
                Row { entry, header }
            })
            .collect();
        view.selected = kept.and_then(|path| rows.iter().position(|r| r.entry.path == path)).unwrap_or(view.selected).min(rows.len().saturating_sub(1));
        view.rows = rows;
        view.confirm = None;
        view.reveal = true;
    }

    /// Open the naming dialog with `default` in the field; `taken` are the names that exist.
    pub fn open_name(&mut self, default: String, taken: Vec<String>) {
        self.view = View::Name(NameView { name: default, taken, ..Default::default() });
    }

    /// Say something in the open dialog (an error of the last action).
    pub fn set_notice(&mut self, notice: String, error: bool) {
        match &mut self.view {
            View::Load(view) => view.notice = Some((notice, error)),
            View::Name(view) => view.notice = Some(notice),
            View::Closed => {}
        }
    }

    /// The rows of the open list.
    pub fn rows(&self) -> &[Row] {
        match &self.view {
            View::Load(view) => &view.rows,
            _ => &[],
        }
    }

    pub fn selected(&self) -> Option<&Row> {
        match &self.view {
            View::Load(view) => view.rows.get(view.selected),
            _ => None,
        }
    }

    pub fn selection_index(&self) -> Option<usize> {
        matches!(self.view, View::Load(_)).then(|| match &self.view {
            View::Load(view) => view.selected,
            _ => 0,
        })
    }

    pub fn select(&mut self, index: usize) {
        if let View::Load(view) = &mut self.view {
            view.selected = index.min(view.rows.len().saturating_sub(1));
            view.confirm = None;
        }
    }

    /// The name in the field of the naming dialog.
    pub fn name(&self) -> Option<&str> {
        match &self.view {
            View::Name(view) => Some(&view.name),
            _ => None,
        }
    }

    pub fn set_name(&mut self, name: &str) {
        if let View::Name(view) = &mut self.view {
            view.name = name.to_owned();
        }
    }

    /// The path whose deletion waits for a yes.
    pub fn confirming(&self) -> Option<&PathBuf> {
        match &self.view {
            View::Load(view) => view.confirm.as_ref(),
            _ => None,
        }
    }

    /// Read the headers still missing, until the time is up. True when rows are still waiting.
    pub fn read_pending(&mut self, module: ModuleId, describe: &dyn Fn(&[u8]) -> Result<SaveSummary, SaveProblem>, budget: Duration) -> bool {
        let View::Load(view) = &mut self.view else { return false };
        let start = web_time::Instant::now();
        // The selected row first, then the others in order.
        let mut order: Vec<usize> = (0..view.rows.len()).collect();
        let first = view.selected.min(order.len());
        order.rotate_left(first);
        for index in order {
            if view.rows[index].header.is_some() {
                continue;
            }
            if start.elapsed() >= budget {
                return true;
            }
            let entry = &view.rows[index].entry;
            let header = read_header(&entry.path, module, describe);
            self.headers.insert(stamp(entry), header.clone());
            view.rows[index].header = Some(header);
        }
        false
    }

    /// A key while the list is open (the dialog is modal: every key is its own). The action
    /// it asks for, if any.
    pub fn key(&mut self, key: Key) -> Option<ShellAction> {
        match &mut self.view {
            View::Closed => None,
            View::Name(view) => match key {
                Key::Enter => name_action(view),
                _ => None,
            },
            View::Load(view) => {
                let last = view.rows.len().saturating_sub(1);
                let go = |view: &mut LoadView, to: usize| {
                    view.selected = to.min(last);
                    view.confirm = None;
                    view.reveal = true;
                };
                match key {
                    Key::ArrowUp => go(view, view.selected.saturating_sub(1)),
                    Key::ArrowDown => go(view, view.selected + 1),
                    Key::Home => go(view, 0),
                    Key::End => go(view, last),
                    Key::PageUp => go(view, view.selected.saturating_sub(5)),
                    Key::PageDown => go(view, view.selected + 5),
                    Key::Enter => {
                        let row = view.rows.get(view.selected)?;
                        return match view.confirm.take() {
                            Some(path) if path == row.entry.path => Some(ShellAction::DeleteSave(path)),
                            Some(_) => None,
                            None => loadable(row).then(|| ShellAction::LoadSave(row.entry.path.clone())),
                        };
                    }
                    Key::Delete | Key::Backspace => {
                        view.confirm = view.rows.get(view.selected).map(|r| r.entry.path.clone());
                    }
                    _ => {}
                }
                None
            }
        }
    }
}

/// A save the list may try to load: not known to be unreadable.
fn loadable(row: &Row) -> bool {
    !matches!(row.header, Some(Err(_)))
}

fn name_action(view: &NameView) -> Option<ShellAction> {
    file_stem(&view.name).map(|_| ShellAction::SaveAs(view.name.trim().to_owned()))
}

/// The name a save shows in the list: the player's own, or the words for the quick and the
/// auto saves.
pub fn display_name(entry: &SaveEntry) -> String {
    match entry.kind {
        SaveKind::Manual => entry.name.clone(),
        SaveKind::Quick => t!("shell.saves.name-quick").to_owned(),
        SaveKind::Auto => t!("shell.saves.name-auto").to_owned(),
    }
}

/// The default name of a new save: "Brazil turn 120", or "Turn 120" when the module names no
/// nation.
pub fn default_name(summary: Option<&SaveSummary>, turn: u32) -> String {
    match summary.and_then(|s| s.nation.as_deref()) {
        Some(nation) => tf!("shell.saves.default-name", civ = nation, turn = turn),
        None => tf!("shell.saves.default-name-plain", turn = turn),
    }
}

/// How long ago a save was written: "5 minutes ago", and the date from a month on.
pub fn age_text(now: u64, modified: u64) -> String {
    let age = now.saturating_sub(modified);
    match age {
        0..=59 => t!("shell.saves.age-now").to_owned(),
        60..=3599 => tf!("shell.saves.age-minutes", n = age / 60),
        3600..=86_399 => tf!("shell.saves.age-hours", n = age / 3600),
        86_400..=2_591_999 => tf!("shell.saves.age-days", n = age / 86_400),
        _ => {
            let (year, month, day) = civil_date(modified);
            tf!("shell.saves.age-date", date = i18n::Arg::Date(year, month, day))
        }
    }
}

/// The calendar date (UTC) of a time in seconds since 1970.
fn civil_date(seconds: u64) -> (i32, u8, u8) {
    let z = (seconds / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u8;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 } as u8;
    let year = (year_of_era + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
}

fn seconds_of(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Draw the open dialog over everything; the actions it asks for join the shell's requests.
pub fn overlay(ctx: &egui::Context, state: &mut ScreenState, module: &dyn ModuleUi) {
    if !state.saves.is_open() {
        return;
    }
    if state.saves.read_pending(module.module_id(), &|payload| module.describe_save(payload), READ_BUDGET) {
        ctx.request_repaint();
    }
    let c = Theme::of(ctx).colors;
    let words = module.words();
    let screen = ctx.content_rect();
    let now = super::report::unix_seconds();
    let mut asked: Vec<ShellAction> = Vec::new();
    let mut close = false;
    egui::Area::new("saves_overlay".into()).order(egui::Order::Tooltip).fixed_pos(screen.min).constrain(false).show(ctx, |ui| {
        let (full, _) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(full, 0.0, egui::Color32::from_black_alpha(170));
        let wanted = match &state.saves.view {
            View::Name(_) => vec2(560.0, 250.0),
            _ => vec2(860.0, 640.0),
        };
        let rect = Rect::from_center_size(screen.center(), vec2((screen.width() - 24.0).min(wanted.x), (screen.height() - 24.0).min(wanted.y)));
        panel(ui.painter(), rect);
        let inner = rect.shrink(18.0);
        let title = if matches!(state.saves.view, View::Name(_)) { t!("shell.saves.save-title") } else { t!("shell.saves.load-title") };
        text(ui.painter(), inner.min, title, 22.0, c.text);
        let close_rect = Rect::from_min_size(pos2(inner.right() - 110.0, inner.top() - 2.0), vec2(110.0, 34.0));
        close |= button(ui, close_rect, t!("shell.saves.close"), false, true);
        match &mut state.saves.view {
            View::Load(view) => draw_load(ui, inner, view, &words.saves, now, &mut asked),
            View::Name(view) => close |= draw_name(ui, inner, view, &mut asked),
            View::Closed => {}
        }
    });
    state.requests.extend(asked);
    if close {
        state.saves.close();
    }
}

fn draw_load(ui: &mut egui::Ui, inner: Rect, view: &mut LoadView, words: &super::words::SaveWords, now: u64, asked: &mut Vec<ShellAction>) {
    let c = Theme::of(ui.ctx()).colors;
    let buttons_top = inner.bottom() - 38.0;
    let list = Rect::from_min_max(pos2(inner.left(), inner.top() + 46.0), pos2(inner.right(), buttons_top - 30.0));
    if view.rows.is_empty() {
        text(ui.painter(), list.min + vec2(4.0, 12.0), t!("shell.saves.empty"), 15.0, c.muted);
    }
    let mut clicked = None;
    let mut open = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        ui.set_clip_rect(list);
        egui::ScrollArea::vertical().id_salt("saves_list").auto_shrink([false, false]).show(ui, |ui| {
            for (index, row) in view.rows.iter().enumerate() {
                let (r, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT - 4.0), Sense::click());
                ui.add_space(4.0);
                if view.reveal && index == view.selected {
                    ui.scroll_to_rect(r.expand2(vec2(0.0, 4.0)), None);
                }
                draw_row(ui, r, row, index == view.selected, response.hovered(), words, now);
                let hint = match &row.header {
                    Some(Err(why)) => why.text(words),
                    _ => row.entry.path.display().to_string(),
                };
                response.clone().on_hover_text(hint);
                if response.clicked() {
                    clicked = Some(index);
                }
                if response.double_clicked() {
                    open = Some(index);
                }
            }
        });
    });
    view.reveal = false;
    if let Some(index) = clicked {
        view.selected = index;
        view.confirm = None;
    }
    if let Some(index) = open {
        view.selected = index;
        if let Some(row) = view.rows.get(index).filter(|r| loadable(r)) {
            asked.push(ShellAction::LoadSave(row.entry.path.clone()));
        }
    }

    if let Some((notice, error)) = &view.notice {
        let (shown, size) = fit_line(ui.painter(), notice, inner.width(), 13.0);
        text(ui.painter(), pos2(inner.left(), buttons_top - 24.0), shown, size, if *error { c.danger } else { c.accent });
    }
    let width = ((inner.width() - 8.0) / 2.0).min(160.0);
    let right = Rect::from_min_size(pos2(inner.right() - width, buttons_top), vec2(width, 38.0));
    let left = Rect::from_min_size(pos2(right.left() - width - 8.0, buttons_top), vec2(width, 38.0));
    let selected = view.rows.get(view.selected);
    let confirm = view.confirm.clone();
    let mut cancelled = false;
    let mut deleted = false;
    let mut ask_delete = false;
    match (&confirm, selected) {
        (Some(path), Some(row)) if *path == row.entry.path => {
            let ask = tf!("shell.saves.delete-ask", name = display_name(&row.entry));
            let (shown, size) = fit_line(ui.painter(), &ask, (left.left() - inner.left() - 12.0).max(40.0), 14.0);
            ui.painter().text(pos2(inner.left(), buttons_top + 19.0), Align2::LEFT_CENTER, shown, FontId::proportional(size), c.text);
            cancelled = button(ui, left, t!("shell.saves.cancel"), false, true);
            if button(ui, right, t!("shell.saves.delete-yes"), true, true) {
                asked.push(ShellAction::DeleteSave(path.clone()));
                deleted = true;
            }
        }
        _ => {
            ask_delete = button(ui, left, t!("shell.saves.delete"), false, selected.is_some());
            if button(ui, right, t!("shell.saves.load"), true, selected.is_some_and(loadable)) {
                if let Some(row) = selected {
                    asked.push(ShellAction::LoadSave(row.entry.path.clone()));
                }
            }
        }
    }
    if cancelled || deleted {
        view.confirm = None;
    }
    if ask_delete {
        view.confirm = view.rows.get(view.selected).map(|r| r.entry.path.clone());
    }
}

fn draw_row(ui: &mut egui::Ui, r: Rect, row: &Row, selected: bool, hovered: bool, words: &super::words::SaveWords, now: u64) {
    let theme = Theme::of(ui.ctx());
    let c = theme.colors;
    let painter = ui.painter();
    let fill = if selected {
        c.selected
    } else if hovered {
        c.hover
    } else {
        c.raised
    };
    painter.rect_filled(r, theme.shapes.button_radius, fill);
    let edge = if selected || hovered { c.selected_edge } else { c.edge };
    painter.rect_stroke(r, theme.shapes.button_radius, egui::Stroke::new(theme.shapes.edge_width, edge), egui::StrokeKind::Inside);

    let pad = 14.0;
    // The facts stay readable on the selected row's lighter fill.
    let quiet = if selected { c.text } else { c.muted };
    let wide = |value: &str, size: f32| painter.layout_no_wrap(value.to_owned(), FontId::proportional(size), c.text).size().x;
    // The right end: when it was written.
    let age = row.entry.modified.map(|m| age_text(now, seconds_of(m))).unwrap_or_default();
    let age_width = wide(&age, 12.0);
    painter.text(pos2(r.right() - pad, r.top() + 10.0), Align2::RIGHT_TOP, &age, FontId::proportional(12.0), quiet);

    // The left end: the name, its mark, and "unreadable" when it is.
    let mark = match row.entry.kind {
        SaveKind::Manual => None,
        SaveKind::Quick => Some(t!("shell.saves.mark-quick")),
        SaveKind::Auto => Some(t!("shell.saves.mark-auto")),
    };
    let mark_width = mark.map_or(0.0, |m| wide(m, 11.0) + 14.0 + 8.0);
    let unreadable = matches!(row.header, Some(Err(_)));
    let tag_width = if unreadable { wide(t!("shell.saves.unreadable"), 11.0) + 14.0 + 8.0 } else { 0.0 };
    let room = (r.width() - 2.0 * pad - age_width - 16.0 - mark_width - tag_width).max(60.0);
    let (name, size) = fit_line(painter, &display_name(&row.entry), room, 16.0);
    let name_width = painter.layout_no_wrap(name.clone(), FontId::proportional(size), c.text).size().x;
    painter.text(pos2(r.left() + pad, r.top() + 8.0), Align2::LEFT_TOP, name, FontId::proportional(size), c.text);
    let mut x = r.left() + pad + name_width + 8.0;
    let mut chip = |label: &str, ink: egui::Color32| {
        let width = wide(label, 11.0) + 14.0;
        let chip = Rect::from_min_size(pos2(x, r.top() + 9.0), vec2(width, 18.0));
        painter.rect_stroke(chip, 9.0, egui::Stroke::new(1.0, ink), egui::StrokeKind::Inside);
        painter.text(chip.center(), Align2::CENTER_CENTER, label, FontId::proportional(11.0), ink);
        x += width + 8.0;
    };
    if let Some(mark) = mark {
        chip(mark, c.accent);
    }
    if unreadable {
        chip(t!("shell.saves.unreadable"), c.danger);
    }

    // The second line: the facts of the game, or why there are none.
    let second = r.top() + 34.0;
    let inner = r.width() - 2.0 * pad;
    match &row.header {
        None => text(painter, pos2(r.left() + pad, second), t!("shell.saves.reading"), 13.0, quiet),
        Some(Err(why)) => {
            let (shown, size) = fit_line(painter, &why.text(words), inner, 13.0);
            text(painter, pos2(r.left() + pad, second), shown, size, c.danger);
        }
        Some(Ok(summary)) => {
            let turn = summary.turn.or(row.entry.named_turn).map(|turn| tf!("shell.saves.row-turn", turn = turn));
            let cells = [(turn, 0.0, 0.22), (summary.nation.clone(), 0.22, 0.5), (summary.era.clone(), 0.5, 1.0)];
            for (value, from, to) in cells {
                let Some(value) = value else { continue };
                let (shown, size) = fit_line(painter, &value, inner * (to - from) - 10.0, 13.0);
                text(painter, pos2(r.left() + pad + inner * from, second), shown, size, quiet);
            }
        }
    }
}

/// The naming dialog; true when it closes by itself (it asked for a save).
fn draw_name(ui: &mut egui::Ui, inner: Rect, view: &mut NameView, asked: &mut Vec<ShellAction>) -> bool {
    let c = Theme::of(ui.ctx()).colors;
    text(ui.painter(), pos2(inner.left(), inner.top() + 54.0), t!("shell.saves.name-label"), 13.0, c.muted);
    let field = Rect::from_min_size(pos2(inner.left(), inner.top() + 76.0), vec2(inner.width(), 40.0));
    let id = egui::Id::new("saves_name_field");
    let output = ui
        .scope_builder(egui::UiBuilder::new().max_rect(field), |ui| {
            egui::TextEdit::singleline(&mut view.name)
                .id(id)
                .char_limit(super::saves::NAME_LIMIT)
                .font(FontId::proportional(16.0))
                .text_color(Theme::of(ui.ctx()).colors.text)
                .margin(vec2(10.0, 9.0))
                .desired_width(field.width())
                .show(ui)
        })
        .inner;
    if !view.focused {
        view.focused = true;
        output.response.request_focus();
        // The default name is selected: typing replaces it.
        let mut edit = output.state.clone();
        let end = egui::text::CCursor::new(view.name.chars().count());
        edit.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), end)));
        edit.store(ui.ctx(), id);
    }
    let entered = output.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    let escaped = ui.input(|i| i.key_pressed(Key::Escape));

    let stem = file_stem(&view.name);
    let replaces = stem.as_ref().is_some_and(|stem| view.taken.iter().any(|name| name.to_lowercase() == stem.to_lowercase()));
    let note = match (&view.notice, replaces) {
        (Some(error), _) => Some((error.as_str(), c.danger)),
        (None, true) => Some((t!("shell.saves.name-replaces"), c.accent)),
        (None, false) => None,
    };
    if let Some((note, ink)) = note {
        let (shown, size) = fit_line(ui.painter(), note, inner.width(), 13.0);
        text(ui.painter(), pos2(inner.left(), field.bottom() + 10.0), shown, size, ink);
    }

    let buttons_top = inner.bottom() - 38.0;
    let width = ((inner.width() - 8.0) / 2.0).min(160.0);
    let save = Rect::from_min_size(pos2(inner.right() - width, buttons_top), vec2(width, 38.0));
    let cancel = Rect::from_min_size(pos2(save.left() - width - 8.0, buttons_top), vec2(width, 38.0));
    let label = if replaces { t!("shell.saves.name-replace") } else { t!("shell.saves.name-save") };
    let mut done = escaped;
    if button(ui, cancel, t!("shell.saves.cancel"), false, true) {
        done = true;
    }
    if (button(ui, save, label, true, stem.is_some()) || entered) && stem.is_some() {
        if let Some(action) = name_action(view) {
            asked.push(action);
        }
    }
    done
}

#[cfg(test)]
mod tests;
