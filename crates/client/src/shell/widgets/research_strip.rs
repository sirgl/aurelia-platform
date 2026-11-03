//! The research strip: the current technology as an image card (`cards::STRIP`, 249 × 87)
//! on the HUD's dark ink. The classic module pins it under the top bar; Lands & Laws opens it
//! from a dock button. The module fills the card (words, progress) and places the area;
//! the strip paints it and reports the click.
use super::cards::{self, ImageCard};
use crate::shell::art::ArtPen;
use crate::shell::hud::GOLD;
use egui::{vec2, Color32, Rect, Sense, Stroke, Vec2};

/// The strip's size.
pub const SIZE: Vec2 = vec2(249., 87.);

/// The strip's fill: translucent ink over the map.
const FILL: Color32 = Color32::from_rgba_premultiplied(13, 26, 36, 204);

/// Paint `card` into `bounds` (inside an `egui::Area` the caller placed) and show `tip` on
/// hover; true when clicked. `framed` adds the gold edge a free-standing popup needs (a strip
/// pinned under the top bar goes without). With a `pen` an art picture of the card is painted;
/// without one its slot stays empty.
pub fn research_strip(ui: &mut egui::Ui, bounds: Rect, card: &ImageCard, tip: &str, framed: bool, pen: Option<&mut ArtPen>) -> bool {
    let (r, response) = ui.allocate_exact_size(bounds.size(), Sense::click());
    ui.painter().rect_filled(r, 0.0, FILL);
    cards::paint(ui, r, card, &cards::STRIP, pen);
    if framed {
        ui.painter().rect_stroke(r, 0.0, Stroke::new(1., GOLD.gamma_multiply(0.8)), egui::StrokeKind::Inside);
    }
    response.on_hover_text(tip).clicked()
}
