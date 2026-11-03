//! The end of an observed match on the shell's standard results card
//! (`widgets::game_end`): who won, how and when, the final figures of every power, the
//! menu, and «play on» that is shown and cannot be pressed — nobody plays here.
use egui::{pos2, vec2, Rect};

use super::contract::Observed;
use super::scene::{round_line, Action};
use super::speed::Pace;
use crate::shell::kit::Availability;
use crate::shell::widgets::game_end::{self, Choice, HallOfFame, Results, Verdict};
use crate::shell::words::ObserverWords;

/// The widest the card gets, and the least height it needs for the standings.
const CARD_WIDTH: f32 = 760.0;
const CARD_HEIGHT: f32 = 600.0;

/// The card in `screen`, over a dimmed window; what the viewer chose.
pub fn draw(ctx: &egui::Context, screen: Rect, obs: &dyn Observed, words: &ObserverWords) -> Option<Action> {
    game_end::backdrop(ctx, screen, "observer_finale_backdrop");
    let size = vec2(CARD_WIDTH.min(screen.width() - 24.0), CARD_HEIGHT.min(screen.height() - 24.0));
    let card = Rect::from_center_size(screen.center(), size);
    let results = results(obs, words);
    let mut chosen = None;
    egui::Area::new("observer_finale".into()).order(egui::Order::Tooltip).fixed_pos(card.min).constrain(false).show(ctx, |ui| {
        ui.set_min_size(card.size());
        let local = Rect::from_min_size(pos2(card.left(), card.top()), card.size());
        chosen = game_end::show(ui, local, &results);
    });
    chosen.map(|choice| match choice {
        Choice::Menu | Choice::KeepPlaying => Action::Menu,
    })
}

/// The results card of a finished match.
pub fn results(obs: &dyn Observed, words: &ObserverWords) -> Results {
    let finale = obs.finale();
    let (winners, cause, round) = finale.map_or((Vec::new(), String::new(), obs.round().now), |f| (f.winners, f.cause, f.round));
    let verdict_line = match winners.len() {
        0 => words.nobody.to_owned(),
        1 => i18n::tf!("shell.observer.winner", name = &winners[0]),
        _ => i18n::tf!("shell.observer.winners", names = winners.join(", ")),
    };
    Results {
        verdict: Verdict::Won,
        title: words.finale_title.to_owned(),
        verdict_line,
        line: i18n::tf!("shell.observer.finale-line", round = round, reason = &cause),
        score: round_line(words, obs.round(), Pace::Running),
        keep_playing_label: words.keep_playing.to_owned(),
        keep_playing: Availability::Blocked(words.cannot_play.to_owned()),
        menu_label: words.to_menu.to_owned(),
        hall_of_fame: HallOfFame { heading: words.final_table.to_owned(), empty: String::new(), games: standings(obs, &winners) },
    }
}

/// One line per power, the last place first (the card lists the newest line on top).
fn standings(obs: &dyn Observed, winners: &[String]) -> Vec<String> {
    let mut rows: Vec<(bool, usize)> = (0..obs.powers().len()).map(|p| (winners.contains(&obs.powers()[p].name), p)).collect();
    rows.sort_by_key(|(won, p)| (std::cmp::Reverse(*won), *p));
    rows.iter()
        .rev()
        .map(|(_, p)| {
            let figures: Vec<String> = obs.standing(*p).iter().map(|s| format!("{} {}", s.label, s.value)).collect();
            format!("{} · {}", obs.powers()[*p].name, figures.join(" · "))
        })
        .collect()
}
