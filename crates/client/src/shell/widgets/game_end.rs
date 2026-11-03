//! The end of a game: the modal over the map that announces it (`announce`) and the
//! results screen (`show`): who won, how and when, the score, "one more turn" or the
//! menu, and the Hall of Fame of earlier games. The module gives every word.
use crate::shell::hud;
use crate::shell::kit::{button, panel, text, Availability, CREAM, GOLD, MUTED};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

const LOST: Color32 = Color32::from_rgb(214, 96, 84);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Won,
    Lost,
}

/// The finished games, oldest first; the newest shows on top.
#[derive(Debug, Clone, PartialEq)]
pub struct HallOfFame {
    pub heading: String,
    /// Shown when no game finished yet.
    pub empty: String,
    pub games: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Results {
    pub verdict: Verdict,
    /// The kind of victory or defeat, large.
    pub title: String,
    /// "You won" / "You lost".
    pub verdict_line: String,
    /// Who, how and when.
    pub line: String,
    pub score: String,
    /// "One more turn" and whether the game can go on.
    pub keep_playing_label: String,
    pub keep_playing: Availability,
    pub menu_label: String,
    pub hall_of_fame: HallOfFame,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    KeepPlaying,
    Menu,
}

/// The results card, centred in `r`.
pub fn show(ui: &mut egui::Ui, r: Rect, results: &Results) -> Option<Choice> {
    let width = r.width().min(720.0);
    let card = Rect::from_min_size(pos2(r.center().x - width / 2.0, r.top()), vec2(width, r.height()));
    panel(ui.painter(), card);
    let color = match results.verdict {
        Verdict::Won => GOLD,
        Verdict::Lost => LOST,
    };
    let center = card.center_top();
    centered(ui, center + vec2(0.0, 30.0), &results.title, 54.0, color);
    centered(ui, center + vec2(0.0, 100.0), &results.verdict_line, 20.0, CREAM);
    centered(ui, center + vec2(0.0, 132.0), &results.line, 15.0, CREAM);
    centered(ui, center + vec2(0.0, 160.0), &results.score, 14.0, MUTED);
    let choice = buttons(ui, card, results);
    hall_of_fame(ui, Rect::from_min_max(card.min + vec2(30.0, 270.0), card.max - vec2(30.0, 20.0)), &results.hall_of_fame);
    choice
}

fn centered(ui: &egui::Ui, at: egui::Pos2, value: &str, size: f32, color: Color32) {
    ui.painter().text(at, Align2::CENTER_TOP, value, FontId::proportional(size), color);
}

fn buttons(ui: &mut egui::Ui, card: Rect, results: &Results) -> Option<Choice> {
    let size = vec2(220.0, 42.0);
    let more = Rect::from_min_size(pos2(card.center().x - size.x - 8.0, card.top() + 200.0), size);
    let menu = more.translate(vec2(size.x + 16.0, 0.0));
    let mut choice = None;
    if button(ui, more, &results.keep_playing_label, true, results.keep_playing.is_ready()) {
        choice = Some(Choice::KeepPlaying);
    }
    if let Availability::Blocked(reason) = &results.keep_playing {
        ui.interact(more, ui.id().with("keep_playing_blocked"), Sense::hover()).on_hover_text(reason);
    }
    if button(ui, menu, &results.menu_label, false, true) {
        choice = Some(Choice::Menu);
    }
    choice
}

fn hall_of_fame(ui: &egui::Ui, r: Rect, hall: &HallOfFame) {
    ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, GOLD.gamma_multiply(0.4)));
    text(ui.painter(), r.min + vec2(0.0, 14.0), &hall.heading, 11.0, GOLD);
    if hall.games.is_empty() {
        text(ui.painter(), r.min + vec2(0.0, 38.0), &hall.empty, 13.0, MUTED);
        return;
    }
    let rows = ((r.height() - 40.0) / 22.0).max(1.0) as usize;
    for (i, game) in hall.games.iter().rev().take(rows).enumerate() {
        let ink = if i == 0 { CREAM } else { MUTED };
        text(ui.painter(), r.min + vec2(0.0, 38.0 + i as f32 * 22.0), game, 13.0, ink);
    }
}

/// The modal over the map when the game ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Announcement {
    pub title: String,
    pub line: String,
    /// The button that opens the results.
    pub button: String,
}

/// Show the announcement over a dimmed map; true when the player asks for the results.
pub fn announce(ctx: &egui::Context, screen: Rect, announcement: &Announcement) -> bool {
    backdrop(ctx, screen, "outcome_backdrop");
    let mut results = false;
    egui::Area::new("game_outcome".into())
        .order(egui::Order::Tooltip)
        .fixed_pos(screen.center() - vec2(200.0, 90.0))
        .show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(400.0, 180.0), Sense::hover());
            ui.painter().rect_filled(r, 8., Color32::from_rgb(10, 20, 29));
            hud::panel(ui.painter(), r, 8.0);
            hud::text(ui.painter(), r.center_top() + vec2(0.0, 26.0), Align2::CENTER_TOP, &announcement.title, 34.0, hud::GOLD);
            hud::text(ui.painter(), r.center_top() + vec2(0.0, 80.0), Align2::CENTER_TOP, &announcement.line, 14.0, hud::LIGHT);
            let button = Rect::from_center_size(r.center_bottom() - vec2(0.0, 34.0), vec2(200.0, 36.0));
            let response = ui.interact(button, "game_outcome_results".into(), Sense::click());
            let fill = if response.hovered() { Color32::from_rgb(40, 77, 97) } else { hud::INK };
            ui.painter().rect_filled(button, 4.0, fill);
            ui.painter().rect_stroke(button, 4.0, Stroke::new(1.0, hud::GOLD), egui::StrokeKind::Inside);
            hud::text(ui.painter(), button.center(), Align2::CENTER_CENTER, &announcement.button, 14.0, hud::LIGHT);
            results = response.clicked();
        });
    results
}

/// Dim the map and take every click outside a modal's panel.
pub fn backdrop(ctx: &egui::Context, screen: Rect, id: &'static str) {
    egui::Area::new(id.into()).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0., Color32::from_black_alpha(100));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, screen: Rect, events: Vec<egui::Event>, draw: impl FnMut(&mut egui::Ui)) {
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, draw);
        output.textures_delta.clear();
    }

    fn click(ctx: &egui::Context, screen: Rect, at: egui::Pos2, mut draw: impl FnMut(&mut egui::Ui)) {
        frame(ctx, screen, vec![], &mut draw);
        frame(ctx, screen, vec![egui::Event::PointerMoved(at)], &mut draw);
        for pressed in [true, false] {
            let press = egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            frame(ctx, screen, vec![press], &mut draw);
        }
    }

    #[test]
    fn backdrop_blocks_the_buttons_under_it() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(960., 640.));
        let mut clicks = 0;
        click(&ctx, screen, pos2(60., 120.), |ui| {
            egui::Area::new("underlying_city".into()).fixed_pos(pos2(20., 100.)).show(ui.ctx(), |ui| {
                if ui.allocate_exact_size(vec2(100., 40.), Sense::click()).1.clicked() {
                    clicks += 1;
                }
            });
            backdrop(ui.ctx(), screen, "capture_backdrop");
        });
        assert_eq!(clicks, 0);
        assert!(ctx.egui_wants_pointer_input());
    }

    #[test]
    fn keep_playing_acts_only_when_the_game_can_go_on() {
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(960., 640.));
        let results = |keep_playing: Availability| Results {
            verdict: Verdict::Won,
            title: "Science Victory".into(),
            verdict_line: "You won".into(),
            line: "Rome launched a spaceship on turn 300".into(),
            score: "Your score: 900".into(),
            keep_playing,
            keep_playing_label: "One more turn".into(),
            menu_label: "Main menu".into(),
            hall_of_fame: HallOfFame { heading: "HALL OF FAME".into(), empty: "None yet".into(), games: vec![] },
        };
        let at = pos2(screen.center().x - 120., 220.);
        let blocked = Availability::Blocked("Your nation has fallen".into());
        for (keep_playing, expected) in [(Availability::Ready, Some(Choice::KeepPlaying)), (blocked, None)] {
            let ctx = egui::Context::default();
            let view = results(keep_playing);
            let mut choice = None;
            click(&ctx, screen, at, |ui| choice = choice.or(show(ui, screen, &view)));
            assert_eq!(choice, expected);
        }
    }
}
