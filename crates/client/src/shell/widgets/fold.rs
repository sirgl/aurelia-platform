//! The header row of a collapsible HUD panel: a title, a one-line summary and a chevron.
//! The module keeps the open state (`shell::fold`) and draws the body under the header
//! when the panel is open; the header only reports the click.
use crate::shell::hud::{panel, text, LIGHT, MUTED};
use crate::shell::observer::scene::elide;
use egui::{vec2, Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke};

/// The title's size and the room kept right of the summary for the chevron.
const TITLE_SIZE: f32 = 13.;
const SUMMARY_SIZE: f32 = 12.;
const PAD: f32 = 10.;
const CHEVRON_ROOM: f32 = 26.;

/// A chevron drawn with two lines: pointing down when folded, up when open (no glyph,
/// so it never depends on the font).
pub fn chevron(painter: &Painter, center: Pos2, open: bool, color: Color32) {
    let (half, rise) = (4.5, 2.5);
    let tip = if open { -rise } else { rise };
    let stroke = Stroke::new(1.6, color);
    painter.line_segment([center + vec2(-half, -tip), center + vec2(0., tip)], stroke);
    painter.line_segment([center + vec2(0., tip), center + vec2(half, -tip)], stroke);
}

/// The panel's header in the HUD's style over `rect`: the ink panel with the gold edge,
/// `title` in light text, `summary` right of it in muted text (cut to fit), the chevron at
/// the end. `id` keeps the click apart from other headers of the `ui`. True when clicked.
pub fn header(ui: &mut egui::Ui, rect: Rect, title: &str, summary: &str, open: bool, id: &str) -> bool {
    let response = ui.interact(rect, ui.id().with(("fold_header", id)), Sense::click());
    let p = ui.painter();
    panel(p, rect, 4.0);
    if response.hovered() {
        p.rect_filled(rect.shrink(1.), 3., Color32::from_rgba_unmultiplied(40, 77, 97, 150));
    }
    let middle = rect.center().y;
    let title_end = p.layout_no_wrap(title.to_owned(), FontId::proportional(TITLE_SIZE), LIGHT).size().x;
    text(p, Pos2::new(rect.left() + PAD, middle), Align2::LEFT_CENTER, title, TITLE_SIZE, LIGHT);
    let room = rect.width() - PAD - title_end - PAD - CHEVRON_ROOM;
    if !summary.is_empty() && room > 12. {
        let shown = elide(p, summary, SUMMARY_SIZE, room);
        text(p, Pos2::new(rect.right() - CHEVRON_ROOM, middle), Align2::RIGHT_CENTER, shown, SUMMARY_SIZE, MUTED);
    }
    chevron(p, Pos2::new(rect.right() - CHEVRON_ROOM * 0.5 - 2., middle), open, LIGHT);
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `header` for two frames, clicking at `point` in the second; returns the click
    /// and the shapes of the last frame.
    fn run(point: Option<Pos2>, summary: &str) -> (bool, Vec<egui::epaint::ClippedShape>) {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(400., 200.));
        let rect = Rect::from_min_size(Pos2::new(20., 20.), vec2(240., 28.));
        let clicked = std::cell::Cell::new(false);
        let frame = |events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                egui::Area::new("fold".into()).fixed_pos(Pos2::ZERO).show(ui.ctx(), |ui| {
                    clicked.set(clicked.get() | header(ui, rect, "Events", summary, false, "events"));
                });
            });
            output.textures_delta.clear();
            output.shapes
        };
        frame(vec![]);
        let Some(point) = point else { return (clicked.get(), frame(vec![])) };
        frame(vec![egui::Event::PointerMoved(point)]);
        let mut shapes = Vec::new();
        for pressed in [true, false] {
            shapes = frame(vec![egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }]);
        }
        (clicked.get(), shapes)
    }

    #[test]
    fn a_click_on_the_header_is_reported_and_outside_is_not() {
        assert!(run(Some(Pos2::new(100., 34.)), "3 new").0);
        assert!(!run(Some(Pos2::new(100., 80.)), "3 new").0);
        assert!(!run(None, "3 new").0);
    }

    #[test]
    fn title_and_summary_are_painted_and_a_long_summary_is_cut() {
        let texts = |shapes: Vec<egui::epaint::ClippedShape>| -> Vec<String> {
            shapes.iter().filter_map(|s| match &s.shape { egui::Shape::Text(t) => Some(t.galley.text().to_owned()), _ => None }).collect()
        };
        let short = texts(run(None, "3 new").1);
        assert!(short.contains(&"Events".to_owned()) && short.contains(&"3 new".to_owned()), "{short:?}");
        let long = texts(run(None, "Rome grew, Athens fell, Memphis declared war on everybody").1);
        assert!(long.iter().any(|t| t.ends_with('…')), "{long:?}");
    }

    #[test]
    fn the_chevron_flips_with_the_state() {
        let ctx = egui::Context::default();
        let lines = |open: bool| {
            let mut out = Vec::new();
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(100., 100.))), ..Default::default() }, |ui| {
                chevron(ui.painter(), Pos2::new(50., 50.), open, Color32::WHITE);
            });
            output.textures_delta.clear();
            for s in output.shapes {
                if let egui::Shape::LineSegment { points, .. } = s.shape {
                    out.push(points);
                }
            }
            out
        };
        let (closed, open) = (lines(false), lines(true));
        assert_eq!(closed.len(), 2);
        // Folded: the point is below the arms; open: above them.
        assert!(closed[0][1].y > closed[0][0].y);
        assert!(open[0][1].y < open[0][0].y);
    }
}
