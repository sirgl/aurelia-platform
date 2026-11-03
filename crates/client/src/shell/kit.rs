//! The look of the full screens, shared by the shell's and the modules' screens: the
//! palette and the small widgets every screen draws with. The widgets take their colours
//! and shapes from the running module's theme (`theme::Theme::of`); the constants are
//! the shell's own palette, the default theme's tokens.
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

use super::theme::{NineSlice, Theme};

pub const BG: Color32 = Color32::from_rgb(9, 22, 28);
pub const PANEL: Color32 = Color32::from_rgb(17, 36, 43);
pub const RAISED: Color32 = Color32::from_rgb(25, 47, 52);
pub const GOLD: Color32 = Color32::from_rgb(193, 162, 99);
pub const CREAM: Color32 = Color32::from_rgb(239, 233, 211);
pub const MUTED: Color32 = Color32::from_rgb(146, 168, 170);
pub const TEAL: Color32 = Color32::from_rgb(63, 125, 124);

/// Whether a button can be pressed now; the reason is shown on hover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Blocked(String),
}

impl Availability {
    pub fn is_ready(&self) -> bool {
        *self == Availability::Ready
    }
}

pub fn text(p: &egui::Painter, at: egui::Pos2, value: impl ToString, size: f32, color: Color32) {
    p.text(at, Align2::LEFT_TOP, value, FontId::proportional(size), color);
}

pub fn paragraph(p: &egui::Painter, at: egui::Pos2, value: &str, size: f32, color: Color32, width: f32) {
    let galley = p.layout(value.to_owned(), FontId::proportional(size), color, width);
    p.galley(at, galley, color);
}

pub fn panel(p: &egui::Painter, r: Rect) {
    let theme = Theme::of(p.ctx());
    p.rect_filled(r, theme.shapes.panel_radius, theme.colors.panel);
    p.rect_stroke(r, theme.shapes.panel_radius, theme.edge(), egui::StrokeKind::Inside);
    frame(p, r, theme.frames.panel);
}

/// The theme's frame art over a panel or button (nothing without one, or while it loads).
fn frame(p: &egui::Painter, r: Rect, slice: Option<NineSlice>) {
    if let Some(slice) = slice {
        let shared = super::art::ScreenArt::shared(p.ctx());
        if let Ok(mut art) = shared.lock() {
            art.paint_nine_slice(p.ctx(), p, r, &slice, Color32::WHITE);
        };
    }
}

pub fn button(ui: &mut egui::Ui, r: Rect, label: &str, selected: bool, enabled: bool) -> bool {
    note_button(ui, r, label, selected, enabled, None)
}

/// A button that says why it is blocked: the reason stands under the label in small type
/// (cut to the button's width), the hover shows it whole.
pub fn note_button(ui: &mut egui::Ui, r: Rect, label: &str, selected: bool, enabled: bool, note: Option<&str>) -> bool {
    let id = ui.id().with(("screen_button", r.min.x.to_bits(), r.min.y.to_bits()));
    let response = ui.interact(super::theme::touch_hit(r, 5.), id, if enabled { Sense::click() } else { Sense::hover() });
    let theme = Theme::of(ui.ctx());
    let colors = &theme.colors;
    let fill = if !enabled {
        colors.raised
    } else if selected {
        colors.selected
    } else if response.hovered() && enabled {
        colors.hover
    } else {
        colors.raised
    };
    let radius = theme.shapes.button_radius;
    ui.painter().rect_filled(r, radius, fill);
    let edge = if selected || response.hovered() { colors.selected_edge } else { colors.edge };
    ui.painter().rect_stroke(r, radius, Stroke::new(theme.shapes.edge_width, edge), egui::StrokeKind::Inside);
    frame(ui.painter(), r, theme.frames.button);
    let ink = if enabled { colors.text } else { colors.muted };
    match note.filter(|_| r.height() >= NOTE_MIN_HEIGHT) {
        Some(note) => {
            ui.painter().text(pos2(r.center().x, r.top() + r.height() * 0.34), Align2::CENTER_CENTER, label, FontId::proportional(13.0), ink);
            let width = r.width() - 8.0;
            let (shown, size) = fit_line(ui.painter(), note, width, 11.0);
            ui.painter().text(pos2(r.center().x, r.top() + r.height() * 0.74), Align2::CENTER_CENTER, shown, FontId::proportional(size), colors.danger);
        }
        None => {
            // A label wider than its button (a narrow screen, a long word) shrinks, then is cut.
            let (shown, size) = fit_line(ui.painter(), label, r.width() - 8.0, 14.0);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, shown, FontId::proportional(size), ink);
        }
    }
    response.clicked() && enabled
}

/// A button must be this tall to carry a reason line.
const NOTE_MIN_HEIGHT: f32 = 28.0;

/// `text` on one line within `width`: the size shrinks from `size` to 8, then the end is cut with
/// an ellipsis. Returns what to draw and at which size.
pub fn fit_line(p: &egui::Painter, text: &str, width: f32, size: f32) -> (String, f32) {
    let wide = |value: &str, size: f32| p.layout_no_wrap(value.to_owned(), FontId::proportional(size), Color32::WHITE).size().x;
    let mut size = size;
    while size > 8.0 && wide(text, size) > width {
        size -= 0.5;
    }
    if wide(text, size) <= width {
        return (text.to_owned(), size);
    }
    let mut cut: String = text.to_owned();
    while cut.pop().is_some() && wide(&format!("{cut}…"), size) > width {}
    (format!("{}…", cut.trim_end()), size)
}

/// A game button greyed by the rules' verdict; the reason stands under the label and shows whole
/// on hover.
pub fn ready_button(ui: &mut egui::Ui, r: Rect, label: &str, availability: &Availability) -> bool {
    let reason = match availability {
        Availability::Blocked(reason) => Some(reason.as_str()),
        Availability::Ready => None,
    };
    let clicked = note_button(ui, r, label, false, availability.is_ready(), reason);
    if let Some(reason) = reason {
        let id = ui.id().with(("blocked", r.min.x.to_bits(), r.min.y.to_bits()));
        ui.interact(r, id, Sense::hover()).on_hover_text(reason);
    }
    clicked
}

/// Title band of the world screens: name, one line of facts, the last rejected order.
pub fn world_banner(ui: &mut egui::Ui, r: Rect, title: &str, line: &str, message: Option<&str>) {
    let theme = Theme::of(ui.ctx());
    panel(ui.painter(), r);
    ui.painter().text(r.min + vec2(24.0, 12.0), Align2::LEFT_TOP, title, theme.title(31.0), theme.colors.text);
    text(ui.painter(), r.min + vec2(26.0, 58.0), line, 14.0, theme.colors.accent);
    if let Some(message) = message {
        let at = pos2(r.right() - 24.0, r.top() + 24.0);
        ui.painter().text(at, Align2::RIGHT_TOP, message, FontId::proportional(13.0), theme.colors.danger);
    }
}

/// A panel's small heading in the accent colour.
pub fn heading(ui: &egui::Ui, r: Rect, label: &str) {
    text(ui.painter(), r.min + vec2(16.0, 14.0), label, 10.0, Theme::of(ui.ctx()).colors.accent);
}

/// A vertical shade over art: `top` and `bottom` black alpha.
pub fn darken(p: &egui::Painter, r: Rect, top: u8, bottom: u8) {
    let mut mesh = egui::Mesh::default();
    for (pos, alpha) in [(r.left_top(), top), (r.right_top(), top), (r.right_bottom(), bottom), (r.left_bottom(), bottom)] {
        mesh.vertices.push(egui::epaint::Vertex { pos, uv: egui::epaint::WHITE_UV, color: Color32::from_black_alpha(alpha) });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    p.add(egui::Shape::mesh(mesh));
}

/// The tint art is painted with at a settings brightness.
pub fn art_tint(brightness: f32) -> Color32 {
    Color32::from_gray((brightness * 255.0).min(255.0) as u8)
}

/// Paint `texture` over `r` as a nine-slice frame: the corners keep their size, the
/// edges stretch along one axis and the middle along both. Borders wider than `r`
/// shrink together so the frame never folds over itself.
pub fn nine_slice(p: &egui::Painter, texture: egui::TextureId, r: Rect, slice: &NineSlice, tint: Color32) {
    let [left, top, right, bottom] = slice.borders;
    let fit = (r.width() / (left + right).max(1e-3)).min(r.height() / (top + bottom).max(1e-3)).min(1.0);
    let xs = [r.left(), r.left() + left * fit, r.right() - right * fit, r.right()];
    let ys = [r.top(), r.top() + top * fit, r.bottom() - bottom * fit, r.bottom()];
    let [u_left, v_top, u_right, v_bottom] = slice.cuts.map(|cut| cut.clamp(0.0, 0.5));
    let us = [0.0, u_left, 1.0 - u_right, 1.0];
    let vs = [0.0, v_top, 1.0 - v_bottom, 1.0];
    let mut mesh = egui::Mesh::with_texture(texture);
    for row in 0..3 {
        for column in 0..3 {
            let cell = Rect::from_min_max(pos2(xs[column], ys[row]), pos2(xs[column + 1], ys[row + 1]));
            let uv = Rect::from_min_max(pos2(us[column], vs[row]), pos2(us[column + 1], vs[row + 1]));
            if cell.width() > 0.0 && cell.height() > 0.0 {
                mesh.add_rect_with_uv(cell, uv, tint);
            }
        }
    }
    p.add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh_of(r: Rect, slice: &NineSlice) -> egui::Mesh {
        let ctx = egui::Context::default();
        let texture = egui::TextureId::User(7);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            nine_slice(ui.painter(), texture, r, slice, Color32::WHITE);
        });
        output.textures_delta.clear();
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some((**mesh).clone()),
            _ => None,
        }).expect("a textured mesh")
    }

    #[test]
    fn nine_slice_keeps_the_corners_and_stretches_the_middle() {
        let slice = NineSlice { art: "frame", cuts: [0.25, 0.125, 0.25, 0.125], borders: [8.0, 4.0, 8.0, 4.0] };
        let r = Rect::from_min_size(pos2(10.0, 20.0), vec2(200.0, 100.0));
        let mesh = mesh_of(r, &slice);
        assert_eq!(mesh.vertices.len(), 9 * 4, "nine quads");
        assert_eq!(mesh.calc_bounds(), r);
        let corner = |pos: egui::Pos2| mesh.vertices.iter().find(|v| v.pos == pos).map(|v| v.uv);
        assert_eq!(corner(pos2(18.0, 24.0)), Some(pos2(0.25, 0.125)), "the corner is 8 x 4 px on screen");
        assert_eq!(corner(pos2(202.0, 116.0)), Some(pos2(0.75, 0.875)));
    }

    #[test]
    fn nine_slice_borders_shrink_on_a_small_rect() {
        let slice = NineSlice { art: "frame", cuts: [0.3; 4], borders: [20.0; 4] };
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(20.0, 80.0));
        let mesh = mesh_of(r, &slice);
        assert_eq!(mesh.calc_bounds(), r);
        assert!(mesh.vertices.iter().all(|v| r.expand(0.01).contains(v.pos)));
        assert_eq!(mesh.vertices.len(), 6 * 4, "the middle column is empty, the borders meet");
    }
}
