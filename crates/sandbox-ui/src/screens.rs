//! The sandbox's two screens, drawn with the shell's kit: the new game and
//! the city (its production choice).

use client::shell::kit::{button, heading, panel, paragraph, text, world_banner, CREAM, GOLD, MUTED};
use client::shell::module::{ScreenId, ScreenMusic, ScreenSpec};
use egui::{pos2, vec2, Rect};
use i18n::{t, tf};
use kernel::ids::ProductionItem;

use crate::session::Session;

pub const NEW_GAME: ScreenId = ScreenId(0);
pub const CITY: ScreenId = ScreenId(1);

pub fn spec(screen: ScreenId) -> ScreenSpec {
    let (title, needs_game, music) = match screen {
        CITY => (t!("sandbox.screens.city-title"), true, ScreenMusic::Game),
        _ => (t!("sandbox.screens.new-game-title"), false, ScreenMusic::Menu),
    };
    ScreenSpec { title, hotkey: None, needs_game, shows_world: false, music, footer: "" }
}

/// What a screen asked for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    StartGame,
    Produce(ProductionItem),
}

pub fn draw_new_game(ui: &mut egui::Ui, body: Rect) -> Option<Request> {
    let r = Rect::from_center_size(body.center(), vec2(520., 260.));
    panel(ui.painter(), r);
    text(ui.painter(), r.min + vec2(28., 24.), t!("sandbox.screens.sandbox-heading"), 30., CREAM);
    let about = t!("sandbox.screens.about");
    paragraph(ui.painter(), r.min + vec2(28., 76.), about, 15., MUTED, r.width() - 56.);
    let start = Rect::from_min_size(pos2(r.left() + 28., r.bottom() - 64.), vec2(180., 40.));
    button(ui, start, t!("sandbox.screens.start-game"), true, true).then_some(Request::StartGame)
}

pub fn draw_city(ui: &mut egui::Ui, body: Rect, session: &Session) -> Option<Request> {
    let (Some(city), Some(handle)) = (session.city(), session.city) else {
        text(ui.painter(), body.min + vec2(24., 24.), t!("sandbox.screens.choose-city"), 16., MUTED);
        return None;
    };
    let (food, production) = session.city_rates(handle);
    let line = tf!("sandbox.screens.city-line", population = city.population, food = signed(food), production = signed(production));
    let banner = Rect::from_min_size(body.min + vec2(16., 16.), vec2(body.width() - 32., 92.));
    world_banner(ui, banner, &city.name, &line, session.message.as_deref());

    let current = Rect::from_min_size(pos2(banner.left(), banner.bottom() + 16.), vec2(360., 120.));
    panel(ui.painter(), current);
    heading(ui, current, t!("sandbox.screens.producing"));
    let head = match city.queue.first() {
        Some(item) => tf!("sandbox.screens.producing-item", item = session.item_name(*item), stored = city.production),
        None => t!("sandbox.screens.producing-nothing").to_owned(),
    };
    text(ui.painter(), current.min + vec2(16., 44.), head, 18., CREAM);

    let list = Rect::from_min_size(pos2(current.right() + 16., current.top()), vec2(420., 60. + 46. * 7.));
    panel(ui.painter(), list);
    heading(ui, list, t!("sandbox.screens.build-next"));
    let mut request = None;
    for (i, offer) in session.offers().iter().enumerate() {
        let row = Rect::from_min_size(list.min + vec2(16., 40. + 46. * i as f32), vec2(list.width() - 32., 38.));
        let turns = match production {
            p if p > 0 => tf!("sandbox.screens.turns", n = offer.cost.saturating_sub(stored(city, offer.item)).div_ceil(p as u32)),
            _ => t!("sandbox.screens.never").to_owned(),
        };
        let label = tf!("sandbox.screens.offer", name = offer.name.as_str(), cost = offer.cost, turns = turns);
        let chosen = city.queue.first() == Some(&offer.item);
        if button(ui, row, &label, chosen, true) {
            request = Some(Request::Produce(offer.item));
        }
    }
    text(ui.painter(), pos2(list.left(), list.bottom() + 12.), t!("sandbox.screens.esc-hint"), 13., GOLD);
    request
}

/// Production already stored toward `item` (the head of the queue keeps it).
fn stored(city: &kernel::world::city::CityCore, item: ProductionItem) -> u32 {
    match city.queue.first() {
        Some(head) if *head == item => city.production,
        _ => 0,
    }
}

/// A number with its sign, as the yields are written (`+2`, `-1`, `+0`).
pub(crate) fn signed(value: impl Into<i64>) -> String {
    format!("{:+}", value.into())
}
