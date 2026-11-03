//! What the observer paints over the picture of the power shown alone: the tooltip of the
//! tile under the pointer, the card of the chosen unit, the card of an open city with its
//! buttons, the outline of what a unit or a city sees, and the tag of the open map. The
//! cards' words are the shell's (`ObserverWords`); the module's facts come as `Card`s.
use egui::{pos2, vec2, Color32, Pos2, Rect, Stroke};

use super::card::{Card, Row, Section, Tone};
use super::contract::UnitFacts;
use super::inspect::{Hover, OpenCity};
use super::panels::Hexes;
use super::scene::{elide, Action};
use super::widget;
use crate::shell::kit::{button, panel, text};
use crate::shell::theme::Theme;
use crate::shell::words::ObserverWords;

/// The widest a city card gets, and the width from which it takes two columns.
pub const CARD_WIDTH: f32 = 640.0;
const TWO_COLUMNS: f32 = 520.0;
/// The room under a city card for its buttons and its hint line.
const FOOT: f32 = 66.0;
/// The strip at the bottom of the picture that holds the tag of the open map.
pub const TAG_STRIP: f32 = 34.0;
const UNIT_WIDTH: f32 = 270.0;
const EDGE: f32 = 12.0;

/// Outlines to draw on the picture, as shares of it across and down.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Marks {
    /// What the chosen unit or the open city sees.
    pub sight: Hexes,
    /// The tile under the pointer.
    pub hover: Hexes,
    /// Where the chosen unit stands.
    pub unit_at: Option<Pos2>,
}

/// Where the cards stand: the picture without its margin and the strip of the tag.
pub fn card_area(shown: Rect) -> Rect {
    Rect::from_min_max(shown.min + vec2(EDGE, EDGE), pos2(shown.right() - EDGE, (shown.bottom() - EDGE - TAG_STRIP).max(shown.top() + EDGE + 40.0)))
}

/// The tooltip of a tile: the module's card, the units on it and, over a city, what a
/// click does.
pub fn tooltip_card(hover: &Hover, words: &ObserverWords, can_open: bool) -> Option<Card> {
    let mut card = hover.card.clone()?;
    if !hover.units.is_empty() {
        let rows = hover.units.iter().flat_map(|u| unit_lines(u, words)).collect();
        card = card.section(Section::new(words.units_title, rows));
    }
    if can_open && hover.city.is_some() {
        card = card.section(Section::plain(vec![Row::note(words.city_click)]));
    }
    Some(card)
}

/// "Archer (Rome)", and under it the health, the sight and the stance.
fn unit_lines(unit: &UnitFacts, words: &ObserverWords) -> Vec<Row> {
    let mut details = vec![i18n::tf!("shell.observer.unit-health", current = unit.hp, max = unit.max_hp)];
    if let Some(sight) = unit.sight {
        details.push(i18n::tf!("shell.observer.unit-sight", sight = sight));
    }
    if unit.fortified {
        details.push(words.unit_fortified.to_owned());
    }
    vec![Row::line(i18n::tf!("shell.observer.unit-name", kind = &unit.kind, nation = &unit.owner)), Row::note(details.join(" · "))]
}

/// The card of the chosen unit.
pub fn unit_card(unit: &UnitFacts, words: &ObserverWords) -> Card {
    let sight = match unit.sight {
        Some(n) => Row::toned(words.sight_label, n, Tone::Accent),
        None => Row::toned(words.sight_label, words.sight_unknown, Tone::Muted),
    };
    let mut rows = vec![
        Row::pair(words.nation_label, &unit.owner),
        Row::bar(words.health_label, i18n::tf!("shell.observer.hp-value", current = unit.hp, max = unit.max_hp), percent(unit.hp, unit.max_hp)),
        sight,
    ];
    if unit.fortified {
        rows.push(Row::line(words.unit_fortified));
    }
    Card::new(&unit.kind).section(Section::plain(rows)).section(Section::plain(vec![Row::note(words.sight_note), Row::note(words.unit_hint)]))
}

fn percent(hp: u16, max: u16) -> u8 {
    if max == 0 {
        return 0;
    }
    (u32::from(hp.min(max)) * 100 / u32::from(max)) as u8
}

/// The tooltip at the pointer. The rect it took.
pub fn tooltip(ctx: &egui::Context, screen: Rect, hover: &Hover, words: &ObserverWords, can_open: bool) -> Option<Rect> {
    let card = tooltip_card(hover, words, can_open)?;
    Some(widget::tip(ctx, screen, hover.pointer, &card))
}

/// A card's spot and measured items, kept so the pointer can be kept off the map under it.
pub struct Placed {
    pub rect: Rect,
    laid: widget::Laid,
}

/// Measure the card of the chosen unit in the corner of the picture.
pub fn place_unit(ctx: &egui::Context, shown: Rect, unit: &UnitFacts, words: &ObserverWords) -> Placed {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, "observer_measure".into()));
    let area = card_area(shown);
    let laid = widget::lay(&painter, &unit_card(unit, words), UNIT_WIDTH.min(area.width()), 1, Some(area.height()));
    Placed { rect: Rect::from_min_size(area.min, laid.size), laid }
}

/// Measure the city card in the picture, its buttons' foot included.
pub fn place_city(ctx: &egui::Context, shown: Rect, open: &OpenCity) -> Placed {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, "observer_measure".into()));
    let area = card_area(shown);
    let width = area.width().min(CARD_WIDTH);
    let columns = if width >= TWO_COLUMNS { 2 } else { 1 };
    let laid = widget::lay(&painter, &open.card, width, columns, Some((area.height() - FOOT).max(60.0)));
    let rect = Rect::from_min_size(area.min, vec2(width, (laid.size.y + FOOT).min(area.height())));
    Placed { rect, laid }
}

/// Paint the unit's card (no buttons: Esc or a click elsewhere drops the choice).
pub fn draw_unit(ui: &mut egui::Ui, placed: &Placed) {
    ui.set_min_size(placed.rect.size());
    ui.interact(placed.rect, ui.id().with("observer_unit_card"), egui::Sense::click_and_drag());
    panel(ui.painter(), placed.rect);
    widget::paint(ui.painter(), &placed.laid, placed.rect.min);
}

/// The buttons of the city card's foot.
pub struct Foot {
    pub previous: Rect,
    pub next: Rect,
    pub place: Pos2,
    pub close: Rect,
    pub hint: Rect,
}

pub fn foot(rect: Rect) -> Foot {
    let top = rect.bottom() - FOOT + 6.0;
    let button_at = |x: f32, w: f32| Rect::from_min_size(pos2(x, top), vec2(w, 30.0));
    let previous = button_at(rect.left() + EDGE, 34.0);
    let next = button_at(previous.right() + 4.0, 34.0);
    let close = button_at(rect.right() - EDGE - 100.0, 100.0);
    let hint = Rect::from_min_max(pos2(rect.left() + EDGE, top + 34.0), pos2(rect.right() - EDGE, rect.bottom() - 4.0));
    Foot { previous, next, place: pos2(next.right() + 10.0, top + 8.0), close, hint }
}

/// Paint the city card with its buttons; what they asked for.
pub fn draw_city(ui: &mut egui::Ui, placed: &Placed, open: &OpenCity, words: &ObserverWords) -> Vec<Action> {
    let colors = Theme::of(ui.ctx()).colors;
    ui.set_min_size(placed.rect.size());
    ui.interact(placed.rect, ui.id().with("observer_city_card"), egui::Sense::click_and_drag());
    panel(ui.painter(), placed.rect);
    widget::paint(ui.painter(), &placed.laid, placed.rect.min);
    let foot = foot(placed.rect);
    let mut actions = Vec::new();
    let several = open.place.is_some_and(|(_, n)| n > 1);
    if tipped(ui, foot.previous, "<", several, words.card_prev_tip) {
        actions.push(Action::CardStep(false));
    }
    if tipped(ui, foot.next, ">", several, words.card_next_tip) {
        actions.push(Action::CardStep(true));
    }
    if let Some((i, n)) = open.place {
        text(ui.painter(), foot.place, i18n::tf!("shell.observer.city-place", place = i + 1, count = n), 14.0, colors.muted);
    }
    if tipped(ui, foot.close, words.card_close, true, words.card_close_tip) {
        actions.push(Action::CardClose);
    }
    let hint = elide(ui.painter(), words.card_hint, 12.0, foot.hint.width());
    text(ui.painter(), foot.hint.min, hint, 12.0, colors.muted);
    actions
}

fn tipped(ui: &mut egui::Ui, r: Rect, label: &str, enabled: bool, tip: &str) -> bool {
    let clicked = button(ui, r, label, false, enabled);
    ui.interact(r, ui.id().with(("observer_card_tip", r.min.x.to_bits())), egui::Sense::hover()).on_hover_text(tip);
    clicked
}

/// Draw the outlines over the picture `shown`.
pub fn paint_marks(painter: &egui::Painter, shown: Rect, marks: &Marks) {
    let colors = Theme::of(painter.ctx()).colors;
    let at = |p: Pos2| pos2(shown.left() + p.x * shown.width(), shown.top() + p.y * shown.height());
    let painter = painter.with_clip_rect(shown);
    let wash = Color32::from_rgba_unmultiplied(colors.accent.r(), colors.accent.g(), colors.accent.b(), 28);
    for ring in &marks.sight.cells {
        painter.add(egui::Shape::convex_polygon(ring.iter().map(|p| at(*p)).collect(), wash, Stroke::NONE));
    }
    let shadow = Stroke::new(4.5, Color32::from_black_alpha(140));
    let line = Stroke::new(2.5, colors.accent);
    for edge in &marks.sight.edges {
        painter.line_segment([at(edge[0]), at(edge[1])], shadow);
    }
    for edge in &marks.sight.edges {
        painter.line_segment([at(edge[0]), at(edge[1])], line);
    }
    for edge in &marks.hover.edges {
        painter.line_segment([at(edge[0]), at(edge[1])], Stroke::new(1.5, Color32::from_white_alpha(200)));
    }
    if let Some(unit) = marks.unit_at {
        let centre = at(unit);
        painter.circle_stroke(centre, 16.0, shadow);
        painter.circle_stroke(centre, 16.0, line);
    }
}

/// The tag of the open map at the bottom right of the picture `shown`; its rect.
pub fn paint_reveal_tag(painter: &egui::Painter, shown: Rect, label: &str) -> Rect {
    let colors = Theme::of(painter.ctx()).colors;
    let width = painter.layout_no_wrap(label.to_owned(), egui::FontId::proportional(13.0), colors.text).size().x + 20.0;
    let width = width.min(shown.width() - 2.0 * 6.0).max(20.0);
    let rect = Rect::from_min_size(pos2(shown.right() - 6.0 - width, shown.bottom() - 6.0 - 24.0), vec2(width, 24.0));
    painter.rect_filled(rect, 4.0, Color32::from_black_alpha(190));
    painter.rect_stroke(rect, 4.0, Stroke::new(1.5, colors.accent), egui::StrokeKind::Inside);
    let shown_label = elide(painter, label, 13.0, width - 12.0);
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, shown_label, egui::FontId::proportional(13.0), colors.text);
    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::observer::contract::CityEntry;
    use crate::shell::observer::frame::{frame, picture};

    fn overlap(a: Rect, b: Rect) -> bool {
        a.min.x < b.max.x - 0.5 && b.min.x < a.max.x - 0.5 && a.min.y < b.max.y - 0.5 && b.min.y < a.max.y - 0.5
    }

    fn archer(own: bool, sight: Option<u8>) -> UnitFacts {
        UnitFacts { id: 7, tile: 3, kind: "Archer".into(), owner: "Rome".into(), player: 1, own, hp: 7, max_hp: 10, fortified: true, sight }
    }

    fn hover(units: Vec<UnitFacts>, city: Option<CityEntry>) -> Hover {
        Hover { power: 0, tile: 3, pointer: pos2(10.0, 10.0), card: Some(Card::new("Plains").section(Section::plain(vec![Row::pair("Food", 2)]))), units, city }
    }

    fn rows_text(card: &Card) -> Vec<String> {
        card.rows()
            .map(|r| match r {
                Row::Line(l) => l.text.clone(),
                Row::Pair { label, value, .. } | Row::Bar { label, value, .. } => format!("{label} {value}"),
            })
            .collect()
    }

    #[test]
    fn the_tooltip_adds_the_units_with_their_sight_and_over_a_city_what_a_click_does() {
        i18n::with_language("ru", || {
        let words = &crate::shell::words::current().observer;
        let city = CityEntry { tile: 3, name: "Rome".into(), owner: "Rome".into(), player: 1, own: true, population: Some(4) };
        let card = tooltip_card(&hover(vec![archer(true, Some(2))], Some(city.clone())), words, true).expect("a card");
        let lines = rows_text(&card).join("\n");
        assert!(lines.contains("Archer (Rome)") && lines.contains("\u{43e}\u{431}\u{437}\u{43e}\u{440} 2") && lines.contains("\u{437}\u{434}\u{43e}\u{440}\u{43e}\u{432}\u{44c}\u{435} 7 \u{438}\u{437} 10") && lines.contains("\u{443}\u{43a}\u{440}\u{435}\u{43f}\u{43b}\u{451}\u{43d}"), "{lines}"); // Russian: "sight 2", "health 7 of 10", "fortified"
        assert!(lines.contains(words.city_click));
        let grid = tooltip_card(&hover(vec![archer(false, None)], Some(city)), words, false).expect("a card");
        let lines = rows_text(&grid).join("\n");
        assert!(!lines.contains(words.city_click), "in the grid a click enlarges the panel instead");
        assert!(!lines.contains("\u{43e}\u{431}\u{437}\u{43e}\u{440}"), "a stranger's sight is not known: {lines}"); // Russian: "sight"
        assert!(tooltip_card(&Hover { card: None, ..hover(vec![], None) }, words, true).is_none(), "an unknown tile has no tooltip");
        });
    }

    #[test]
    fn the_card_of_a_unit_says_how_far_it_sees_and_says_so_when_it_is_not_known() {
        i18n::with_language("ru", || {
        let words = &crate::shell::words::current().observer;
        let own = rows_text(&unit_card(&archer(true, Some(3)), words)).join("\n");
        assert!(own.contains(&format!("{} 3", words.sight_label)), "{own}");
        let stranger = rows_text(&unit_card(&archer(false, None), words)).join("\n");
        assert!(stranger.contains(words.sight_unknown), "{stranger}");
        });
    }

    #[test]
    fn the_cards_and_the_tag_stand_inside_the_picture_and_clear_of_each_other_at_every_window() {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for size in [(1024.0, 600.0), (1280.0, 720.0), (1664.0, 936.0), (2560.0, 1440.0)] {
                for lang in ["en", "ru", "de", "en-XA"] {
                  i18n::with_language(lang, || {
                    let words = &crate::shell::words::current().observer;
                    let f = frame(Rect::from_min_size(Pos2::ZERO, vec2(size.0, size.1)), 6, Some(0));
                    let shown = picture(f.cell(0).unwrap(), f.head).shrink(1.0);
                    let rows: Vec<Row> = (0..40).map(|i| Row::pair(format!("Row {i}"), i)).collect();
                    let mut card = Card::new("A very long name of a city that goes on").subtitle("Subtitle");
                    for i in 0..6 {
                        card = card.section(Section::new(format!("Section {i}"), rows.clone()));
                    }
                    let open = OpenCity { power: 0, tile: 1, card, place: Some((1, 4)) };
                    let city = place_city(ui.ctx(), shown, &open);
                    let tag = Rect::from_min_size(pos2(shown.right() - 6.0 - 200.0, shown.bottom() - 6.0 - 24.0), vec2(200.0, 24.0));
                    assert!(shown.contains_rect(city.rect), "{size:?}: {:?} leaves {shown:?}", city.rect);
                    assert!(!overlap(city.rect, tag), "{size:?}: the city card {:?} covers the tag {tag:?}", city.rect);
                    let foot = foot(city.rect);
                    let parts = [foot.previous, foot.next, foot.close, foot.hint];
                    for (i, a) in parts.iter().enumerate() {
                        assert!(city.rect.contains_rect(*a), "{size:?}: {a:?} leaves the card {:?}", city.rect);
                        for b in &parts[i + 1..] {
                            assert!(!overlap(*a, *b), "{size:?}: {a:?} overlaps {b:?}");
                        }
                    }
                    let unit = place_unit(ui.ctx(), shown, &archer(true, Some(2)), words);
                    assert!(shown.contains_rect(unit.rect) && !overlap(unit.rect, tag), "{size:?}: the unit card {:?}", unit.rect);
                    let measure = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), egui::FontId::proportional(14.0), Color32::WHITE).size().x;
                    assert!(measure(words.card_close) + 6.0 <= foot.close.width(), "{}", words.card_close);
                  });
                }
            }
        });
        output.textures_delta.clear();
    }
}
