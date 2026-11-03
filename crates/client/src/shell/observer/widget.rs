//! Paints the observer's cards: the tooltip at the pointer and the body of a city card.
//! One layout serves both (`lay`): the card's rows are measured first (the panel's size
//! is known before anything is painted, so it can be kept on screen and clear of other
//! panels), then painted from the measured items. Colours and fonts are the running
//! module's theme.
use std::sync::Arc;

use egui::{pos2, vec2, Color32, FontId, Galley, Painter, Pos2, Rect, Vec2};

use super::card::{Card, Line, Row, Tone};
use crate::shell::hud::icon;
use crate::shell::kit::panel;
use crate::shell::theme::{Colors, Theme};

/// The width of the tooltip.
pub const TIP_WIDTH: f32 = 300.0;
const PAD: f32 = 12.0;
const GAP: f32 = 18.0;
const ROW_GAP: f32 = 3.0;
const SECTION_GAP: f32 = 8.0;
const TITLE_SIZE: f32 = 18.0;
const SUBTITLE_SIZE: f32 = 11.5;
const SECTION_SIZE: f32 = 11.5;
const ROW_SIZE: f32 = 13.0;
const ICON_ROOM: f32 = 20.0;
const BAR_HEIGHT: f32 = 5.0;
/// The gap between the columns of a card.
const COLUMN_GAP: f32 = 18.0;

/// The colour a tone is painted in.
pub fn tone_color(colors: &Colors, tone: Tone) -> Color32 {
    match tone {
        Tone::Plain => colors.text,
        Tone::Muted => colors.muted,
        Tone::Good => colors.success,
        Tone::Bad => colors.danger,
        Tone::Warn => colors.warning,
        Tone::Accent => colors.accent,
    }
}

enum Item {
    Text { at: Vec2, galley: Arc<Galley>, color: Color32 },
    Icon { at: Vec2, kind: crate::shell::hud::Icon },
    Bar { rect: Rect, percent: u8 },
    Rule { rect: Rect },
}

/// A card measured: its size and what to paint where (relative to its top left).
pub struct Laid {
    pub size: Vec2,
    items: Vec<Item>,
    /// Rows that did not fit the height asked for (cut off at the bottom).
    pub cut: usize,
}

struct Pen<'a> {
    painter: &'a Painter,
    colors: Colors,
    title: FontId,
}

impl Pen<'_> {
    fn galley(&self, text: &str, size: f32, color: Color32, wrap: f32) -> Arc<Galley> {
        self.painter.layout(text.to_owned(), FontId::proportional(size), color, wrap.max(8.0))
    }
}

/// Measure `card` in `width` points (inside its padding) and, when `max_height` is given and
/// `columns` is more than one, flow its sections into columns, a section never split.
pub fn lay(painter: &Painter, card: &Card, width: f32, columns: usize, max_height: Option<f32>) -> Laid {
    let theme = Theme::of(painter.ctx());
    let pen = Pen { painter, colors: theme.colors, title: theme.title(TITLE_SIZE) };
    let mut items = Vec::new();
    let inner = (width - 2.0 * PAD).max(40.0);
    let mut y = PAD;
    let title = painter.layout(card.title.clone(), pen.title.clone(), pen.colors.text, inner);
    let title_height = title.size().y;
    items.push(Item::Text { at: vec2(PAD, y), galley: title, color: pen.colors.text });
    y += title_height + 2.0;
    if let Some(sub) = &card.subtitle {
        let g = pen.galley(sub, SUBTITLE_SIZE, pen.colors.muted, inner);
        y += g.size().y + 4.0;
        let at = vec2(PAD, y - g.size().y - 4.0);
        items.push(Item::Text { at, galley: g, color: pen.colors.muted });
    }
    y += 4.0;
    let head = y;
    let columns = columns.max(1);
    let column_width = (inner - COLUMN_GAP * (columns - 1) as f32) / columns as f32;
    let room = max_height.map(|h| h - PAD);
    let mut column = 0usize;
    let mut cut = 0usize;
    let mut tops = vec![head; columns];
    for section in card.sections.iter().filter(|s| !s.rows.is_empty()) {
        let x_of = |c: usize| PAD + c as f32 * (column_width + COLUMN_GAP);
        let (mut laid, mut height) = section_items(&pen, section, x_of(column), tops[column], column_width);
        let overflows = |top: f32, height: f32| room.is_some_and(|room| top + height > room);
        // A section that does not fit under its column's top goes to the next column
        // (a section is never split); in the last column it is cut off.
        if overflows(tops[column], height) && tops[column] > head && column + 1 < columns {
            column += 1;
            (laid, height) = section_items(&pen, section, x_of(column), tops[column], column_width);
        }
        if overflows(tops[column], height) && tops[column] > head {
            cut += section.rows.len();
            continue;
        }
        items.append(&mut laid);
        tops[column] += height + SECTION_GAP;
    }
    let tallest = tops.iter().copied().fold(head, f32::max);
    let height = if tallest > head { tallest - SECTION_GAP + PAD } else { head + PAD };
    Laid { size: vec2(width, height), items, cut }
}

/// The items of one section with its top left at (`x`, `y`), and the height it takes.
fn section_items(pen: &Pen<'_>, section: &super::card::Section, x: f32, y: f32, width: f32) -> (Vec<Item>, f32) {
    let mut items = Vec::new();
    let mut top = y;
    if let Some(title) = &section.title {
        let g = pen.galley(title, SECTION_SIZE, pen.colors.accent, width);
        let height = g.size().y;
        items.push(Item::Text { at: vec2(x, top), galley: g, color: pen.colors.accent });
        top += height + 2.0;
        items.push(Item::Rule { rect: Rect::from_min_size(pos2(x, top), vec2(width, 1.0)) });
        top += 4.0;
    }
    for row in &section.rows {
        top += place_row(pen, row, x, top, width, &mut items);
    }
    (items, top - y)
}

/// Place one row at (`x`, `y`) in `width`; the height it took.
fn place_row(pen: &Pen<'_>, row: &Row, x: f32, y: f32, width: f32, out: &mut Vec<Item>) -> f32 {
    let colors = pen.colors;
    match row {
        Row::Line(Line { icon, text, tone }) => {
            let indent = if icon.is_some() { ICON_ROOM } else { 0.0 };
            let color = tone_color(&colors, *tone);
            let g = pen.galley(text, ROW_SIZE, color, width - indent);
            if let Some(kind) = icon {
                out.push(Item::Icon { at: vec2(x + 7.0, y + g.size().y.min(16.0) / 2.0 + 1.0), kind: *kind });
            }
            let height = g.size().y;
            out.push(Item::Text { at: vec2(x + indent, y), galley: g, color });
            height + ROW_GAP
        }
        Row::Pair { label, value, tone } => {
            let value_galley = pen.galley(value, ROW_SIZE, tone_color(&colors, *tone), width * 0.5);
            let room = width - value_galley.size().x - 8.0;
            let label_galley = pen.galley(label, ROW_SIZE, colors.muted, room);
            let height = label_galley.size().y.max(value_galley.size().y);
            let value_at = vec2(x + width - value_galley.size().x, y);
            out.push(Item::Text { at: vec2(x, y), galley: label_galley, color: colors.muted });
            out.push(Item::Text { at: value_at, galley: value_galley, color: tone_color(&colors, *tone) });
            height + ROW_GAP
        }
        Row::Bar { label, value, percent } => {
            let value_galley = pen.galley(value, ROW_SIZE, colors.text, width * 0.5);
            let label_galley = pen.galley(label, ROW_SIZE, colors.muted, width - value_galley.size().x - 8.0);
            let line = label_galley.size().y.max(value_galley.size().y);
            let value_at = vec2(x + width - value_galley.size().x, y);
            out.push(Item::Text { at: vec2(x, y), galley: label_galley, color: colors.muted });
            out.push(Item::Text { at: value_at, galley: value_galley, color: colors.text });
            let bar = Rect::from_min_size(pos2(x, y + line + 1.0), vec2(width, BAR_HEIGHT));
            out.push(Item::Bar { rect: bar, percent: *percent });
            line + BAR_HEIGHT + 2.0 + ROW_GAP
        }
    }
}

/// Paint a measured card with its top left at `at`.
pub fn paint(painter: &Painter, laid: &Laid, at: Pos2) {
    let colors = Theme::of(painter.ctx()).colors;
    for item in &laid.items {
        match item {
            Item::Text { at: rel, galley, color } => {
                painter.galley(at + *rel, galley.clone(), *color);
            }
            Item::Icon { at: rel, kind } => icon(painter, at + *rel, 6.0, *kind, colors.accent_icon),
            Item::Bar { rect, percent } => {
                let rect = rect.translate(at.to_vec2());
                painter.rect_filled(rect, 2.0, colors.track);
                let done = Rect::from_min_size(rect.min, vec2(rect.width() * f32::from(*percent) / 100.0, rect.height()));
                painter.rect_filled(done, 2.0, colors.progress);
            }
            Item::Rule { rect } => {
                painter.rect_filled(rect.translate(at.to_vec2()), 0.0, colors.edge);
            }
        }
    }
}

/// Where a tooltip of `size` stands for a pointer at `pointer` on `screen`: right and below
/// the pointer, flipped to the other side when it would leave the screen, then kept inside.
pub fn tip_place(screen: Rect, pointer: Pos2, size: Vec2) -> Rect {
    let mut x = pointer.x + GAP;
    let mut y = pointer.y + GAP;
    if x + size.x > screen.right() - 8.0 {
        x = pointer.x - size.x - GAP;
    }
    if y + size.y > screen.bottom() - 8.0 {
        y = pointer.y - size.y - GAP;
    }
    let max_x = (screen.right() - size.x - 8.0).max(screen.left() + 8.0);
    let max_y = (screen.bottom() - size.y - 8.0).max(screen.top() + 8.0);
    Rect::from_min_size(pos2(x.clamp(screen.left() + 8.0, max_x), y.clamp(screen.top() + 8.0, max_y)), size)
}

/// The tooltip of `card` at the pointer: painted over everything, taking no pointer input.
/// The rect it took.
pub fn tip(ctx: &egui::Context, screen: Rect, pointer: Pos2, card: &Card) -> Rect {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, "observer_tip".into())).with_clip_rect(screen);
    let laid = lay(&painter, card, TIP_WIDTH, 1, None);
    let r = tip_place(screen, pointer, laid.size);
    panel(&painter, r);
    paint(&painter, &laid, r.min);
    r
}
