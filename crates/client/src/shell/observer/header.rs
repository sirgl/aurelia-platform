//! A panel's header: the power's colour and name, and its figures in columns (label
//! over value). Text that does not fit its column is cut with an ellipsis, never
//! drawn over its neighbour.
use egui::{pos2, vec2, Align2, Color32, FontId, Rect};

use super::contract::Stat;
use super::scene::{elide, View};
use crate::shell::theme::Theme;

/// The narrowest a column of figures gets.
pub const COLUMN_MIN: f32 = 52.0;
/// Padding inside the header.
const PAD: f32 = 10.0;
/// The line of the name.
const NAME_SIZE: f32 = 15.0;
const LABEL_SIZE: f32 = 10.5;
const VALUE_SIZE: f32 = 14.0;

/// The colour the renderer draws a power's borders, cities and units in.
pub fn power_color(player: u8) -> Color32 {
    let [r, g, b] = render::gpu::buffers::owner_color(player);
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// How many of `stats` fit in a header of `width`: as many as there are columns.
pub fn columns(width: f32, stats: usize) -> usize {
    (((width - 2.0 * PAD) / COLUMN_MIN).floor() as usize).clamp(1, stats.max(1))
}

pub fn draw(ui: &mut egui::Ui, r: Rect, view: &View<'_>, power: usize) {
    let colors = Theme::of(ui.ctx()).colors;
    let Some(info) = view.obs.powers().get(power) else { return };
    let alive = view.obs.alive(power);
    let ink = if alive { colors.text } else { colors.muted };
    let p = ui.painter();
    let middle = r.top() + 15.0;
    p.circle_filled(pos2(r.left() + PAD + 6.0, middle), 6.0, power_color(info.player));
    let tag = (!alive).then_some(view.words.fallen);
    let tag_width = tag.map_or(0.0, |t| p.layout_no_wrap(t.to_owned(), FontId::proportional(12.0), ink).size().x + 8.0);
    let room = (r.width() - 2.0 * PAD - 18.0 - tag_width).max(1.0);
    let name = elide(p, &info.name, NAME_SIZE, room);
    p.text(pos2(r.left() + PAD + 18.0, middle), Align2::LEFT_CENTER, name, FontId::proportional(NAME_SIZE), ink);
    if let Some(tag) = tag {
        p.text(pos2(r.right() - PAD, middle), Align2::RIGHT_CENTER, tag, FontId::proportional(12.0), colors.muted);
    }
    figures(ui, Rect::from_min_max(pos2(r.left() + PAD, r.top() + 28.0), pos2(r.right() - PAD, r.bottom())), view.obs.standing(power), ink);
}

fn figures(ui: &egui::Ui, r: Rect, stats: &[Stat], ink: Color32) {
    let colors = Theme::of(ui.ctx()).colors;
    let n = columns(r.width() + 2.0 * PAD, stats.len());
    let column = r.width() / n as f32;
    for (i, stat) in stats.iter().take(n).enumerate() {
        let x = r.left() + i as f32 * column;
        let label = elide(ui.painter(), stat.label, LABEL_SIZE, column - 4.0);
        let value = elide(ui.painter(), &stat.value, VALUE_SIZE, column - 4.0);
        ui.painter().text(pos2(x, r.top()), Align2::LEFT_TOP, label, FontId::proportional(LABEL_SIZE), colors.muted);
        ui.painter().text(pos2(x, r.top() + 12.0), Align2::LEFT_TOP, value, FontId::proportional(VALUE_SIZE), ink);
    }
}

/// The rect each figure of a header takes (for the layout tests).
pub fn figure_rects(r: Rect, stats: usize) -> Vec<Rect> {
    let inner = Rect::from_min_max(pos2(r.left() + PAD, r.top() + 28.0), pos2(r.right() - PAD, r.bottom()));
    let n = columns(r.width(), stats);
    let column = inner.width() / n as f32;
    (0..n.min(stats)).map(|i| Rect::from_min_size(pos2(inner.left() + i as f32 * column, inner.top()), vec2(column - 4.0, inner.height()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_narrow_header_keeps_fewer_columns_but_at_least_one() {
        assert_eq!(columns(600.0, 5), 5);
        assert_eq!(columns(300.0, 5), 5);
        assert_eq!(columns(250.0, 5), 4);
        assert_eq!(columns(120.0, 5), 1);
        assert_eq!(columns(300.0, 2), 2);
        assert_eq!(columns(300.0, 0), 1);
    }

    #[test]
    fn the_eight_power_grid_at_720p_keeps_four_figures() {
        let f = super::super::frame::frame(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 720.0)), 8, None);
        for cell in f.cells.iter().map(|c| c.rect) {
            assert!(columns(cell.width(), 5) >= 4, "{cell:?}");
        }
    }
}
