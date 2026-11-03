//! An image card: a picture beside lines of text, with an optional progress bar. The
//! module fills `ImageCard` (what is shown); a `CardStyle` (where it goes) fixes the
//! geometry, so a graph node and a HUD strip share one painter at their own sizes.
use crate::shell::art::ArtPen;
use crate::shell::hud::{icon, Icon, GOLD, INK};
use egui::{pos2, vec2, Color32, FontId, Rect};

/// What fills the picture slot.
#[derive(Debug, Clone, PartialEq)]
pub enum Picture {
    /// Screen art by name (`art::ScreenArt`), painted into the slot.
    Art(String),
    /// A HUD icon in a gold ring filling the slot.
    Icon(Icon, Color32),
}

/// One line of text in the colour the module gives its meaning.
#[derive(Debug, Clone, PartialEq)]
pub struct CardLine {
    pub text: String,
    pub color: Color32,
}

impl CardLine {
    pub fn new(text: impl Into<String>, color: Color32) -> Self {
        Self { text: text.into(), color }
    }
}

/// A filled share (0..=1) of the bar under the lines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub share: f32,
    pub color: Color32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageCard {
    pub picture: Picture,
    /// Drawn on the style's rows in order; lines beyond its rows are not drawn.
    pub lines: Vec<CardLine>,
    pub bar: Option<Bar>,
}

/// A text row of a style: its top, font size and how it ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    pub y: f32,
    pub size: f32,
    pub end: RowEnd,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RowEnd {
    /// One line cut this far before the card's right edge.
    Clip(f32),
    /// The whole text, past the card if it is long.
    Free,
}

/// The bar's slot, at the text's left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarSlot {
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The geometry of a card; offsets are from the card's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardStyle {
    pub picture: Rect,
    /// Size of an icon picture inside its ring.
    pub icon: f32,
    pub text_x: f32,
    pub rows: &'static [Row],
    pub bar: Option<BarSlot>,
}

/// A graph node (218 × 62): a square picture, a title and one status line.
pub const NODE: CardStyle = CardStyle {
    picture: Rect { min: pos2(7., 9.), max: pos2(51., 53.) },
    icon: 12.,
    text_x: 59.,
    rows: &[
        Row { y: 8., size: 13., end: RowEnd::Clip(27.) },
        Row { y: 31., size: 11., end: RowEnd::Clip(57.) },
    ],
    bar: None,
};

/// A HUD strip (249 × 87): an icon ring, a small kicker, a title, a detail and a bar.
pub const STRIP: CardStyle = CardStyle {
    picture: Rect { min: pos2(9., 14.5), max: pos2(61., 66.5) },
    icon: 14.,
    text_x: 76.,
    rows: &[
        Row { y: 12., size: 10., end: RowEnd::Free },
        Row { y: 30., size: 18., end: RowEnd::Free },
        Row { y: 53., size: 12., end: RowEnd::Free },
    ],
    bar: Some(BarSlot { y: 73., width: 156., height: 3. }),
};

const BAR_TRACK: Color32 = Color32::from_rgb(48, 67, 77);

/// Paint `card` into `r` (the frame behind it is the caller's). Art needs a pen; without
/// one an art picture leaves its slot empty.
pub fn paint(ui: &egui::Ui, r: Rect, card: &ImageCard, style: &CardStyle, pen: Option<&mut ArtPen>) {
    let slot = style.picture.translate(r.min.to_vec2());
    match (&card.picture, pen) {
        (Picture::Art(name), Some(pen)) => pen.paint(ui, slot, name),
        (Picture::Art(_), None) => {}
        (Picture::Icon(kind, color), _) => ring(ui.painter(), slot, style.icon, *kind, *color),
    }
    for (row, line) in style.rows.iter().zip(&card.lines) {
        let at = r.min + vec2(style.text_x, row.y);
        match row.end {
            RowEnd::Clip(right) => one_line(ui.painter(), at, &line.text, r.width() - style.text_x - right, row.size, line.color),
            RowEnd::Free => {
                ui.painter().text(at, egui::Align2::LEFT_TOP, &line.text, FontId::proportional(row.size), line.color);
            }
        }
    }
    if let (Some(slot), Some(bar)) = (style.bar, card.bar) {
        let track = Rect::from_min_size(r.min + vec2(style.text_x, slot.y), vec2(slot.width, slot.height));
        ui.painter().rect_filled(track, 1.0, BAR_TRACK);
        let filled = Rect::from_min_size(track.min, vec2(slot.width * bar.share.clamp(0., 1.), slot.height));
        ui.painter().rect_filled(filled, 1.0, bar.color);
    }
}

fn ring(p: &egui::Painter, slot: Rect, size: f32, kind: Icon, color: Color32) {
    let (center, radius) = (slot.center(), slot.width().min(slot.height()) * 0.5);
    p.circle_filled(center, radius, INK);
    p.circle_stroke(center, radius, egui::Stroke::new(1.0, GOLD));
    icon(p, center, size, kind, color);
}

/// `value` on one line, cut to `width` with an ellipsis.
pub fn one_line(p: &egui::Painter, at: egui::Pos2, value: &str, width: f32, size: f32, color: Color32) {
    let mut job = egui::text::LayoutJob::simple(value.to_owned(), FontId::proportional(size), color, width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = p.layout_job(job);
    p.galley(at, galley, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(card: &ImageCard, style: &CardStyle) -> Vec<(String, Rect)> {
        let ctx = egui::Context::default();
        let r = Rect::from_min_size(pos2(10., 20.), vec2(249., 87.));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| paint(ui, r, card, style, None));
        output.textures_delta.clear();
        output.shapes.iter().filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some((text.galley.text().to_owned(), text.visual_bounding_rect())),
            _ => None,
        }).collect()
    }

    #[test]
    fn lines_fill_the_rows_in_order_and_clip_where_the_style_says() {
        let long = "A very long title that cannot fit in a node card at all";
        let card = ImageCard {
            picture: Picture::Icon(Icon::Flask, GOLD),
            lines: vec![CardLine::new(long, GOLD), CardLine::new("12 science", GOLD), CardLine::new("dropped", GOLD)],
            bar: None,
        };
        let node = texts(&card, &NODE);
        assert_eq!(node.len(), 2, "a node has two rows");
        assert!(node[0].1.right() <= 10. + 249. - 27. + 1., "the title is cut before the corner");
        let strip = texts(&card, &STRIP);
        assert_eq!(strip.len(), 3);
        assert_eq!(strip[0].0, long, "a free row keeps the whole text");
    }
}
