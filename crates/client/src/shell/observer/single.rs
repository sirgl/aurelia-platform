//! The header of one power shown alone: besides its figures, what it is researching and
//! earning, and its last events. The words of the events are the shell's (`ObserverWords`),
//! the facts come from the match (`Observed::events`, `Observed::details`).
use egui::{pos2, vec2, Align2, FontId, Rect};

use super::contract::{Event, Happened, Stat};
use super::scene::{elide, View};
use super::{frame, header};
use crate::shell::theme::Theme;
use crate::shell::words::ObserverWords;

const PAD: f32 = 10.0;
/// The share of the header the figures and details have; the events have the rest.
const FIGURES_SHARE: f32 = 0.58;
/// Height of one line of events.
const LINE: f32 = 15.0;
const LABEL_SIZE: f32 = 10.5;
const VALUE_SIZE: f32 = 14.0;
const BAR_HEIGHT: f32 = 5.0;
/// A figure with a progress bar takes this many times the room of another.
const BAR_WEIGHT: f32 = 2.2;

/// The two parts of a header: figures and details on the left, events on the right.
pub fn split(r: Rect) -> (Rect, Rect) {
    let cut = r.left() + r.width() * FIGURES_SHARE;
    (Rect::from_min_max(r.min, pos2(cut, r.bottom())), Rect::from_min_max(pos2(cut, r.top()), r.max))
}

/// The room of the figures' row (name and figures) and of the details' row, in the left part.
pub fn rows(left: Rect) -> (Rect, Rect) {
    let figures = Rect::from_min_max(left.min, pos2(left.right(), (left.top() + frame::HEADER_HEIGHT).min(left.bottom())));
    let details = Rect::from_min_max(pos2(left.left() + PAD, figures.bottom() + 2.0), pos2(left.right() - PAD, left.bottom() - 6.0));
    (figures, details)
}

pub fn draw(ui: &mut egui::Ui, r: Rect, view: &View<'_>, power: usize) {
    let (left, events) = split(r);
    let (figures, details) = rows(left);
    header::draw(ui, figures, view, power);
    draw_details(ui, details, view.obs.details(power));
    draw_events(ui, events, view, power);
}

/// The details as columns, the ones with a progress wider, each with its label over its
/// value and, for a progress, a bar under the value.
fn draw_details(ui: &egui::Ui, r: Rect, stats: &[Stat]) {
    let colors = Theme::of(ui.ctx()).colors;
    let weight = |s: &Stat| if s.percent.is_some() { BAR_WEIGHT } else { 1.0 };
    let total: f32 = stats.iter().map(weight).sum::<f32>().max(1.0);
    let mut x = r.left();
    for stat in stats {
        let width = r.width() * weight(stat) / total;
        let room = width - 6.0;
        let label = elide(ui.painter(), stat.label, LABEL_SIZE, room);
        let value = elide(ui.painter(), &stat.value, VALUE_SIZE, room);
        ui.painter().text(pos2(x, r.top()), Align2::LEFT_TOP, label, FontId::proportional(LABEL_SIZE), colors.muted);
        ui.painter().text(pos2(x, r.top() + 12.0), Align2::LEFT_TOP, value, FontId::proportional(VALUE_SIZE), colors.text);
        if let Some(percent) = stat.percent {
            let track = Rect::from_min_size(pos2(x, r.top() + 31.0), vec2(room.max(1.0), BAR_HEIGHT));
            ui.painter().rect_filled(track, 2.0, colors.track);
            let done = Rect::from_min_size(track.min, vec2(track.width() * f32::from(percent) / 100.0, BAR_HEIGHT));
            ui.painter().rect_filled(done, 2.0, colors.progress);
        }
        x += width;
    }
}

fn draw_events(ui: &egui::Ui, r: Rect, view: &View<'_>, power: usize) {
    let colors = Theme::of(ui.ctx()).colors;
    let inner = Rect::from_min_max(pos2(r.left() + PAD, r.top() + 8.0), pos2(r.right() - PAD, r.bottom()));
    ui.painter().text(inner.min, Align2::LEFT_TOP, view.words.events_title, FontId::proportional(LABEL_SIZE), colors.muted);
    let events = view.obs.events(power);
    if events.is_empty() {
        ui.painter().text(pos2(inner.left(), inner.top() + 14.0), Align2::LEFT_TOP, view.words.no_events, FontId::proportional(13.0), colors.muted);
        return;
    }
    for (i, happened) in events.iter().rev().enumerate() {
        let line = elide(ui.painter(), &event_line(view.words, happened), 13.0, inner.width());
        ui.painter().text(pos2(inner.left(), inner.top() + 14.0 + i as f32 * LINE), Align2::LEFT_TOP, line, FontId::proportional(13.0), colors.text);
    }
}

/// An event in words, with its round in front.
pub fn event_line(words: &ObserverWords, happened: &Happened) -> String {
    i18n::tf!("shell.observer.event-line", round = happened.round, text = event_text(words, &happened.event))
}

fn event_text(words: &ObserverWords, event: &Event) -> String {
    match event {
        Event::Founded(city) => i18n::tf!("shell.observer.ev-founded", city = city),
        Event::Captured { city, from: Some(from) } => i18n::tf!("shell.observer.ev-captured", city = city, nation = from),
        Event::Captured { city, from: None } => i18n::tf!("shell.observer.ev-captured-plain", city = city),
        Event::Lost { city, to: Some(to) } => i18n::tf!("shell.observer.ev-lost", city = city, nation = to),
        Event::Lost { city, to: None } => i18n::tf!("shell.observer.ev-lost-plain", city = city),
        Event::War(with) => i18n::tf!("shell.observer.ev-war", nation = with),
        Event::Peace(with) => i18n::tf!("shell.observer.ev-peace", nation = with),
        Event::Learned(tech) => i18n::tf!("shell.observer.ev-learned", tech = tech),
        Event::Fell => words.ev_fell.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn happened(event: Event) -> Happened {
        Happened { round: 41, event }
    }

    #[test]
    fn events_read_in_both_languages() {
        let line = |tag: &str, event: Event| i18n::with_language(tag, || event_line(&crate::shell::words::current().observer, &happened(event)));
        let captured = || Event::Captured { city: "Ur".into(), from: Some("Kish".into()) };
        assert_eq!(line("en", captured()), "T41: Captured Ur from Kish");
        assert_eq!(line("ru", captured()), "\u{425}\u{43e}\u{434} 41: \u{412}\u{437}\u{44f}\u{442} \u{433}\u{43e}\u{440}\u{43e}\u{434} Ur: \u{443} \u{434}\u{435}\u{440}\u{436}\u{430}\u{432}\u{44b} Kish"); // Russian: "Turn 41: City Ur captured: from the realm Kish"
        assert_eq!(line("en", Event::Lost { city: "Ur".into(), to: None }), "T41: Lost Ur");
        assert_eq!(line("ru", Event::Fell), "\u{425}\u{43e}\u{434} 41: \u{414}\u{435}\u{440}\u{436}\u{430}\u{432}\u{430} \u{43f}\u{430}\u{43b}\u{430}"); // Russian: "Turn 41: The realm has fallen"
    }

    #[test]
    fn the_header_splits_into_figures_and_events_without_overlap() {
        for width in [520.0, 1000.0, 1664.0] {
            let r = Rect::from_min_size(pos2(5.0, 7.0), vec2(width, frame::HEADER_SINGLE));
            let (left, events) = split(r);
            let (figures, details) = rows(left);
            assert!(left.right() <= events.left() + 0.01 && r.contains_rect(left) && r.contains_rect(events));
            assert!(figures.bottom() <= details.top() + 0.01 && left.contains_rect(details), "{width}");
            assert!(details.height() >= 30.0, "room for a label, a value and a bar: {details:?}");
        }
    }

    #[test]
    fn five_event_lines_fit_under_the_title() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, frame::HEADER_SINGLE));
        let (_, events) = split(r);
        let last = events.top() + 8.0 + 14.0 + 4.0 * LINE + 13.0;
        assert!(last <= events.bottom(), "{last} below {}", events.bottom());
    }
}
