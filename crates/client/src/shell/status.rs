//! What the shell tells the player about its own doings (a save that failed, a quick save that
//! worked), drawn as a toast over every screen including the map's HUD. `ScreenState::menu_status`
//! is the main menu's own line and is kept for menu actions only: an error raised during play must
//! not wait in a field that only the menu draws (`errors_during_play_do_not_use_the_menu_line`).
use super::hud::{GOLD, INK, LIGHT};
use super::screens::ScreenState;
use egui::{Align2, Color32, Stroke};

/// How long a message that is not an error stays, in seconds.
pub const SECONDS: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    /// Stays until clicked.
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellStatus {
    pub text: String,
    pub level: Level,
    /// The time (egui seconds) it was first drawn; None before.
    pub since: Option<f64>,
}

impl ShellStatus {
    pub fn new(text: impl Into<String>, level: Level) -> Self {
        Self { text: text.into(), level, since: None }
    }

    /// Whether it is over at `now`: an error never is, until dismissed.
    pub fn expired(&self, now: f64) -> bool {
        self.level == Level::Info && self.since.is_some_and(|since| now - since >= SECONDS)
    }
}

/// The toast, top centre; a click dismisses it.
pub fn overlay(ctx: &egui::Context, state: &mut ScreenState) {
    let now = ctx.input(|i| i.time);
    let Some(status) = &mut state.status else { return };
    status.since.get_or_insert(now);
    if status.expired(now) {
        state.status = None;
        return;
    }
    let (text, level) = (status.text.clone(), status.level);
    let color = if level == Level::Error { Color32::from_rgb(214, 96, 84) } else { GOLD };
    let width = (ctx.content_rect().width() - 24.0).clamp(120.0, 520.0);
    let mut dismissed = false;
    egui::Area::new("shell_status".into())
        .order(egui::Order::Tooltip)
        .anchor(Align2::CENTER_TOP, egui::vec2(0.0, 52.0))
        .show(ctx, |ui| {
            let response = egui::Frame::new()
                .fill(INK)
                .stroke(Stroke::new(1.0, color))
                .inner_margin(egui::Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.set_max_width(width);
                    ui.add(egui::Label::new(egui::RichText::new(text).color(LIGHT).size(13.0)).wrap());
                })
                .response
                .interact(egui::Sense::click());
            dismissed = response.clicked();
        });
    if dismissed {
        state.status = None;
    }
    if state.status.is_some() && level == Level::Info {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_stays_and_an_info_goes_after_its_seconds() {
        let mut error = ShellStatus::new("Cannot save", Level::Error);
        let mut info = ShellStatus::new("Saved", Level::Info);
        (error.since, info.since) = (Some(0.0), Some(0.0));
        assert!(!error.expired(10_000.0));
        assert!(!info.expired(SECONDS - 0.1) && info.expired(SECONDS));
    }

    /// Every write of `menu_status` is in a function of the menu's own actions; an error raised
    /// while playing goes through `ScreenState::status`.
    #[test]
    fn errors_during_play_do_not_use_the_menu_line() {
        const MENU: [&str; 7] = ["with_world_format", "open_save_dialog", "save_named", "continue_latest", "load_from", "open_saves", "new"];
        let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/shell/app.rs")).unwrap();
        let mut current = String::new();
        let mut strays = Vec::new();
        for (number, line) in src.lines().enumerate() {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix("pub fn ").or_else(|| t.strip_prefix("pub(super) fn ")).or_else(|| t.strip_prefix("fn ")) {
                current = rest.split(['(', '<']).next().unwrap_or("").to_owned();
            }
            if t.contains("menu_status =") && !MENU.contains(&current.as_str()) {
                strays.push(format!("app.rs:{} in {current}", number + 1));
            }
        }
        assert!(strays.is_empty(), "menu_status written outside the menu's functions: {strays:?}");
    }
}
