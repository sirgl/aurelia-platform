//! A list of notices on a HUD panel: a marker, a line of text and what a click does —
//! the whole row (show it on the map) or a button at its end (an agenda item). The module
//! fills the rows and places the panel (an `egui::Area`); the widget returns the row
//! acted on.
use crate::shell::hud::{icon, panel, text, Icon, GOLD, INK, LIGHT};
use crate::shell::observer::scene::elide;
use egui::{vec2, Align2, Color32, Rect, Sense, Stroke};

/// The height of a row.
pub const ROW: f32 = 22.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Marker {
    /// A dot in the colour of the notice's kind.
    Dot(Color32),
    Icon(Icon, Color32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Read only.
    None,
    /// The whole row is a button; the hint shows on hover.
    Row { hint: String },
    /// A button at the end of the row.
    Button { label: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub marker: Marker,
    pub text: String,
    pub action: Action,
}

/// The panel's height for `rows` rows.
pub fn height(rows: usize) -> f32 {
    14. + rows as f32 * ROW
}

/// A panel `width` wide with every row of `notices` (the caller keeps the list short);
/// returns the row whose action was taken. A long line runs on past the panel's edge;
/// `show_clipped` cuts it.
pub fn show(ui: &mut egui::Ui, width: f32, notices: &[Notice]) -> Option<usize> {
    show_rows(ui, width, notices, false)
}

/// `show` with every line cut to the panel's width by an ellipsis; the hover text of a
/// cut row holds the whole line (before the row's hint).
pub fn show_clipped(ui: &mut egui::Ui, width: f32, notices: &[Notice]) -> Option<usize> {
    show_rows(ui, width, notices, true)
}

fn show_rows(ui: &mut egui::Ui, width: f32, notices: &[Notice], clip: bool) -> Option<usize> {
    let (r, _) = ui.allocate_exact_size(vec2(width, height(notices.len())), Sense::hover());
    panel(ui.painter(), r, 4.0);
    let mut acted = None;
    for (i, notice) in notices.iter().enumerate() {
        let at = r.min + vec2(12., 8. + i as f32 * ROW);
        let row = Rect::from_min_size(at - vec2(6., 2.), vec2(width - 12., ROW - 1.));
        if draw_row(ui, row, at, i, notice, clip) {
            acted = Some(i);
        }
    }
    acted
}

/// The text the row shows: the notice's line, cut to `room` when clipping.
fn shown(ui: &egui::Ui, notice: &Notice, room: f32, clip: bool) -> String {
    match clip {
        true => elide(ui.painter(), &notice.text, 12., room),
        false => notice.text.clone(),
    }
}

fn draw_row(ui: &mut egui::Ui, row: Rect, at: egui::Pos2, index: usize, notice: &Notice, clip: bool) -> bool {
    let response = ui.interact(row, ui.id().with(("notice", index)), Sense::click());
    if matches!(notice.action, Action::Row { .. }) && response.hovered() {
        ui.painter().rect_filled(row, 3.0, Color32::from_rgb(40, 77, 97));
    }
    match notice.marker {
        Marker::Dot(color) => {
            ui.painter().circle_filled(at + vec2(3., 8.), 3., color);
        }
        Marker::Icon(kind, color) => icon(ui.painter(), at + vec2(3., 8.), 5., kind, color),
    }
    let start = at.x + TEXT_INDENT;
    match &notice.action {
        Action::None => {
            let value = shown(ui, notice, row.right() - start - 4., clip);
            line(ui.painter(), at, &value);
            if value != notice.text {
                response.on_hover_text(&notice.text);
            }
            false
        }
        Action::Row { hint } => {
            let value = shown(ui, notice, row.right() - start - 4., clip);
            line(ui.painter(), at, &value);
            let tip = if value != notice.text { format!("{}\n{hint}", notice.text) } else { hint.clone() };
            response.on_hover_text(tip).clicked()
        }
        Action::Button { label } => {
            let button = Rect::from_min_max(row.right_top() - vec2(76., -1.), row.right_bottom() - vec2(2., 1.));
            let words = Rect::from_min_max(row.min, egui::pos2(button.left() - 4., row.bottom()));
            let value = shown(ui, notice, words.right() - start, clip);
            line(&ui.painter().with_clip_rect(words), at, &value);
            if value != notice.text {
                response.on_hover_text(&notice.text);
            }
            row_button(ui, button, index, label)
        }
    }
}

/// Where the line starts, right of the marker.
const TEXT_INDENT: f32 = 12.;

fn line(p: &egui::Painter, at: egui::Pos2, value: &str) {
    text(p, at + vec2(TEXT_INDENT, 0.), Align2::LEFT_TOP, value, 12., LIGHT);
}

fn row_button(ui: &mut egui::Ui, r: Rect, index: usize, label: &str) -> bool {
    let response = ui.interact(r, ui.id().with(("notice_button", index)), Sense::click());
    let fill = if response.hovered() { Color32::from_rgb(40, 77, 97) } else { INK };
    ui.painter().rect_filled(r, 3., fill);
    ui.painter().rect_stroke(r, 3., Stroke::new(1., GOLD), egui::StrokeKind::Inside);
    text(ui.painter(), r.center(), Align2::CENTER_CENTER, label, 11., LIGHT);
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(notices: &[Notice], at: impl Fn(Rect) -> egui::Pos2) -> Option<usize> {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(400., 300.));
        let mut acted = None;
        let mut frame = |events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                egui::Area::new("notices".into()).fixed_pos(egui::pos2(10., 10.)).show(ui.ctx(), |ui| {
                    acted = acted.or(show(ui, 300., notices));
                });
            });
            output.textures_delta.clear();
        };
        frame(vec![]);
        let panel = Rect::from_min_size(egui::pos2(10., 10.), vec2(300., height(notices.len())));
        let point = at(panel);
        frame(vec![egui::Event::PointerMoved(point)]);
        for pressed in [true, false] {
            frame(vec![egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }]);
        }
        acted
    }

    /// The text shapes of one frame of `show`/`show_clipped` with a long line, and the widest.
    fn painted(clipped: bool, width: f32) -> (Vec<String>, f32) {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(600., 300.));
        let rows = [Notice { marker: Marker::Dot(GOLD), text: "A very long notice about the great city of Rome growing beyond all measure".into(), action: Action::Row { hint: "Show".into() } }];
        let frame = || {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                egui::Area::new("notices".into()).fixed_pos(egui::pos2(10., 10.)).show(ui.ctx(), |ui| {
                    let _ = if clipped { show_clipped(ui, width, &rows) } else { show(ui, width, &rows) };
                });
            });
            output.textures_delta.clear();
            output
        };
        frame();
        let output = frame();
        let texts: Vec<(String, f32)> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_owned(), t.pos.x + t.galley.size().x)),
                _ => None,
            })
            .collect();
        let right = texts.iter().map(|(_, r)| *r).fold(0., f32::max);
        (texts.into_iter().map(|(t, _)| t).collect(), right)
    }

    #[test]
    fn clipped_lines_stay_inside_the_panel() {
        let (plain, plain_right) = painted(false, 200.);
        assert!(plain_right > 10. + 200., "the plain list runs past the panel");
        assert!(plain.iter().any(|t| t.ends_with("measure")));
        let (cut, cut_right) = painted(true, 200.);
        assert!(cut_right <= 10. + 200., "the cut line ends inside the panel, not at {cut_right}");
        assert!(cut.iter().any(|t| t.ends_with('…')), "{cut:?}");
        let (whole, _) = painted(true, 600.);
        assert!(whole.iter().any(|t| t.ends_with("measure")), "a line that fits is not cut");
    }

    fn notice(action: Action) -> Notice {
        Notice { marker: Marker::Dot(GOLD), text: "Rome grew".into(), action }
    }

    #[test]
    fn a_row_acts_by_its_kind() {
        let rows = [notice(Action::None), notice(Action::Row { hint: "Show".into() }), notice(Action::Button { label: "Go".into() })];
        let second_row = |r: Rect| r.min + vec2(60., 8. + ROW + 8.);
        assert_eq!(click(&rows, second_row), Some(1));
        let first_row = |r: Rect| r.min + vec2(60., 16.);
        assert_eq!(click(&rows, first_row), None, "a read-only row does nothing");
        let button = |r: Rect| egui::pos2(r.right() - 40., r.top() + 8. + 2. * ROW + 8.);
        assert_eq!(click(&rows, button), Some(2));
        let text_of_button_row = |r: Rect| r.min + vec2(60., 8. + 2. * ROW + 8.);
        assert_eq!(click(&rows, text_of_button_row), None, "only the button of an agenda row acts");
    }
}
