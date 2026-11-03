//! The "Report a bug" dialog: what goes into the issue (read-only, the player reads it first),
//! the report file, and the buttons. It is drawn over any screen; the app fills it
//! (`ReportUi::show`), saves the file when asked and reports where it went (`ReportUi::saved`).
use egui::{pos2, vec2, FontId, Rect, RichText, Sense};

use super::{Delivered, Prepared, ReportInfo};
use crate::shell::kit::{button, panel, text};
use crate::shell::module::ShellAction;
use crate::shell::screens::ScreenState;
use crate::shell::theme::Theme;
use i18n::{t, tf};

/// The dialog's state; closed until the menu button opens it.
#[derive(Debug, Default)]
pub struct ReportUi {
    shown: Option<Shown>,
}

#[derive(Debug)]
struct Shown {
    info: ReportInfo,
    prepared: Prepared,
    /// Where the report file went, or why it did not.
    file: Option<Result<Delivered, String>>,
    copied: bool,
}

impl ReportUi {
    /// Open the dialog with the report of `info`.
    pub fn show(&mut self, info: ReportInfo, prepared: Prepared) {
        self.shown = Some(Shown { info, prepared, file: None, copied: false });
    }

    pub fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    /// Esc closes the dialog; true when it was open.
    pub fn cancel(&mut self) -> bool {
        self.shown.take().is_some()
    }

    /// The facts and the text the dialog shows (the file is made from the same).
    pub fn prepared(&self) -> Option<(&ReportInfo, &Prepared)> {
        self.shown.as_ref().map(|shown| (&shown.info, &shown.prepared))
    }

    /// The report file was written or handed to the browser (or could not be).
    pub fn saved(&mut self, result: Result<Delivered, String>) {
        if let Some(shown) = &mut self.shown {
            shown.file = Some(result);
            shown.copied = false;
        }
    }
}

/// The words the crash dialog of the page needs, in the language in force.
pub fn dialog_words() -> super::DialogWords {
    super::DialogWords {
        title: t!("shell.report.crashed-title").to_owned(),
        intro: t!("shell.report.crashed-intro").to_owned(),
        preview: t!("shell.report.preview").to_owned(),
        open_issue: t!("shell.report.open-issue").to_owned(),
        save_file: t!("shell.report.save-file").to_owned(),
        reload: t!("shell.report.reload").to_owned(),
        saved: t!("shell.report.crashed-saved").to_owned(),
    }
}

/// Draw the dialog over everything when it is open.
pub fn overlay(ctx: &egui::Context, state: &mut ScreenState) {
    let Some(shown) = state.report.shown.as_mut() else { return };
    let c = Theme::of(ctx).colors;
    let screen = ctx.content_rect();
    let mut close = false;
    let mut save = false;
    egui::Area::new("report_overlay".into()).order(egui::Order::Tooltip).fixed_pos(screen.min).constrain(false).show(ctx, |ui| {
        let (full, _) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(full, 0.0, egui::Color32::from_black_alpha(170));
        let rect = Rect::from_center_size(screen.center(), vec2((screen.width() - 24.0).min(780.0), (screen.height() - 24.0).min(620.0)));
        panel(ui.painter(), rect);
        let inner = rect.shrink(18.0);
        let narrow = inner.width() < 560.0;
        text(ui.painter(), inner.min, t!("shell.report.title"), 22.0, c.text);
        let close_rect = Rect::from_min_size(pos2(inner.right() - 110.0, inner.top()), vec2(110.0, 34.0));
        close |= button(ui, close_rect, t!("shell.report.close"), false, true);

        let intro = ui.painter().layout(t!("shell.report.intro").to_owned(), FontId::proportional(13.0), c.muted, inner.width());
        let intro_at = inner.min + vec2(0.0, 40.0);
        let mut y = intro_at.y + intro.size().y + 10.0;
        ui.painter().galley(intro_at, intro, c.muted);
        text(ui.painter(), pos2(inner.left(), y), t!("shell.report.preview"), 12.0, c.accent);
        y += 20.0;

        // The buttons stand in one row, or in two on a narrow window; the status line is above them.
        let rows = if narrow { 2.0 } else { 1.0 };
        let buttons_top = inner.bottom() - rows * 44.0 + 6.0;
        let status_top = buttons_top - 28.0;
        let preview = Rect::from_min_max(pos2(inner.left(), y), pos2(inner.right(), (status_top - 6.0).max(y + 40.0)));
        ui.painter().rect_filled(preview, 4.0, c.raised);
        ui.painter().rect_stroke(preview, 4.0, egui::Stroke::new(1.0, c.edge), egui::StrokeKind::Inside);
        ui.scope_builder(egui::UiBuilder::new().max_rect(preview.shrink(8.0)), |ui| {
            ui.set_clip_rect(preview.shrink(2.0));
            egui::ScrollArea::vertical().id_salt("report_preview").auto_shrink([false, false]).show(ui, |ui| {
                ui.set_min_width(preview.width() - 28.0);
                // A selectable label: the player can copy it, and a touch screen shows no keyboard.
                ui.add(egui::Label::new(RichText::new(shown.prepared.body.as_str()).monospace().size(12.0).color(c.text)).selectable(true));
            });
        });

        let (note, ink) = match &shown.file {
            None => (t!("shell.report.hint").to_owned(), c.muted),
            Some(Ok(done)) if done.path.is_some() => (tf!("shell.report.saved-path", place = done.place.as_str()), c.accent),
            Some(Ok(done)) => (tf!("shell.report.saved-download", place = done.place.as_str()), c.accent),
            Some(Err(error)) => (error.clone(), c.danger),
        };
        let (shown_note, size) = crate::shell::kit::fit_line(ui.painter(), &note, inner.width(), 13.0);
        text(ui.painter(), pos2(inner.left(), status_top), shown_note, size, ink);

        let mut cells = vec![Cell::Open, Cell::Save];
        if matches!(&shown.file, Some(Ok(done)) if done.path.is_some()) {
            cells.push(Cell::Copy);
        }
        let per_row = if narrow { 2 } else { cells.len().max(3) };
        let gap = 8.0;
        let width = (inner.width() - gap * (per_row as f32 - 1.0)) / per_row as f32;
        for (i, cell) in cells.iter().enumerate() {
            let (row, column) = (i / per_row, i % per_row);
            let at = pos2(inner.left() + column as f32 * (width + gap), buttons_top + row as f32 * 44.0);
            let r = Rect::from_min_size(at, vec2(width, 38.0));
            let label = match cell {
                Cell::Open => t!("shell.report.open-issue"),
                Cell::Save => t!("shell.report.save-file"),
                Cell::Copy if shown.copied => t!("shell.report.copied"),
                Cell::Copy => t!("shell.report.copy-path"),
            };
            if button(ui, r, label, matches!(cell, Cell::Open), true) {
                match cell {
                    Cell::Open => ui.ctx().open_url(egui::OpenUrl::new_tab(shown.prepared.url.clone())),
                    Cell::Save => save = true,
                    Cell::Copy => {
                        if let Some(Ok(Delivered { path: Some(path), .. })) = &shown.file {
                            ui.ctx().copy_text(path.clone());
                            shown.copied = true;
                        }
                    }
                }
            }
        }
    });
    if save {
        state.requests.push(ShellAction::ReportSave);
    }
    if close {
        state.report.shown = None;
    }
}

#[derive(Clone, Copy)]
enum Cell {
    Open,
    Save,
    Copy,
}
