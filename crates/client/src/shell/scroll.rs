//! Bounded scroll bodies shared by full-screen lists.
use egui::{vec2, Rect, Sense};

/// A bounded scrolling viewport for the existing absolute-positioned cards.
/// The clip belongs to the body: fixed headings and surrounding action bars stay put.
pub fn scroll_panel(
    ui: &mut egui::Ui,
    viewport: Rect,
    salt: &'static str,
    height: f32,
    render: impl FnOnce(&mut egui::Ui, Rect),
) {
    scroll_body(ui, viewport, salt, |ui| {
        let (area, _) = ui.allocate_exact_size(
            vec2(ui.available_width().max(1.0), height.max(0.0)),
            Sense::hover(),
        );
        render(ui, area);
    });
}

pub fn scroll_body(
    ui: &mut egui::Ui,
    viewport: Rect,
    salt: &'static str,
    render: impl FnOnce(&mut egui::Ui),
) {
    if viewport.width() <= 0.0 || viewport.height() <= 0.0 {
        return;
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(viewport), |ui| {
        ui.set_clip_rect(viewport.intersect(ui.clip_rect()));
        egui::ScrollArea::vertical()
            .id_salt(salt)
            .max_height(viewport.height())
            .auto_shrink([false, false])
            .show(ui, render);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, Event, Pos2};

    fn frame(
        ctx: &egui::Context,
        events: Vec<Event>,
        clicked: &mut Option<usize>,
    ) -> (Rect, Vec<egui::epaint::ClippedShape>) {
        let viewport = Rect::from_min_max(pos2(20., 60.), pos2(300., 260.));
        let mut last = Rect::NOTHING;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400., 320.))),
                events,
                ..Default::default()
            },
            |ui| {
                ui.painter().text(
                    pos2(20., 20.),
                    egui::Align2::LEFT_TOP,
                    "FIXED HEADER",
                    egui::FontId::proportional(14.),
                    egui::Color32::WHITE,
                );
                scroll_panel(ui, viewport, "overflow_test", 50. * 40., |ui, area| {
                    for i in 0..50 {
                        let row = Rect::from_min_size(
                            area.min + vec2(0., i as f32 * 40.),
                            vec2(area.width(), 36.),
                        );
                        if i == 49 {
                            last = row;
                        }
                        if !ui.is_rect_visible(row) {
                            continue;
                        }
                        if ui.put(row, egui::Button::new(format!("Row {i}"))).clicked() {
                            *clicked = Some(i);
                        }
                    }
                });
            },
        );
        output.textures_delta.clear();
        (last, output.shapes)
    }

    #[test]
    fn last_row_is_reachable_clickable_and_clipped_at_normal_and_hidpi_scale() {
        for scale in [1., 2.] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(scale);
            let mut clicked = None;
            let (first_last, _) = frame(&ctx, vec![], &mut clicked);
            assert!(first_last.top() > 260.);
            frame(
                &ctx,
                vec![Event::PointerMoved(pos2(150., 160.))],
                &mut clicked,
            );
            frame(
                &ctx,
                vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: vec2(0., -9000.),
                    modifiers: Default::default(),
                }],
                &mut clicked,
            );
            let mut last = Rect::NOTHING;
            let mut shapes = Vec::new();
            for _ in 0..24 {
                (last, shapes) = frame(&ctx, vec![], &mut clicked);
            }
            assert!(
                last.center().y >= 60. && last.center().y <= 260.,
                "last row {last:?}"
            );
            let mut saw_last = false;
            for clipped in shapes {
                if let egui::Shape::Text(text) = clipped.shape {
                    if text.galley.text() == "Row 49" {
                        saw_last = true;
                        assert!(
                            clipped.clip_rect.top() >= 60. && clipped.clip_rect.bottom() <= 260.
                        );
                    }
                    if text.galley.text() == "FIXED HEADER" {
                        assert_eq!(text.pos, pos2(20., 20.));
                    }
                }
            }
            assert!(saw_last);
            let point = last.center();
            frame(
                &ctx,
                vec![
                    Event::PointerMoved(point),
                    Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                &mut clicked,
            );
            frame(
                &ctx,
                vec![Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                &mut clicked,
            );
            assert_eq!(clicked, Some(49));
            clicked = None;
            let header = pos2(150., 30.);
            frame(
                &ctx,
                vec![
                    Event::PointerMoved(header),
                    Event::PointerButton {
                        pos: header,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                &mut clicked,
            );
            frame(
                &ctx,
                vec![Event::PointerButton {
                    pos: header,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                &mut clicked,
            );
            assert_eq!(clicked, None, "clipped rows must not receive header clicks");
        }
    }
}
