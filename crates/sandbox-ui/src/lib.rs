//! sandbox-ui: the user interface of the sandbox rules module, plugged into
//! the app shell through `client::shell::module::ModuleUi` (platform/PLATFORM.md
//! stage 7). The shell draws the map, the top bar, city labels, the end-turn
//! button, menus, settings and saves; this crate adds the match (`session`),
//! map orders, a small unit panel, the new-game and city screens
//! (`screens`) and the offscreen capture (`headless`).
//!
//! Map orders: left click selects an own unit or opens an own city; right
//! click sends the selected unit there (or attacks the enemy next to it);
//! F founds a city; Enter ends the turn.

pub mod headless;
pub mod screens;
pub mod shortcuts;
pub mod session;

use std::sync::Arc;
use std::time::Duration;

use client::shell::kit::{button, panel, text, CREAM, MUTED};
use client::shell::layout;
use client::shell::module::{
    AiProgress, CameraFit, FrameInput, HudFeed, HudLayer, HudView, KindLabel, MapButton, MapView, MenuGroup, ModuleId, ModuleUi,
    Outcome, Screen, ScreenId, ScreenSpec, ShellAction, ShellFlags,
};
use client::shell::presentation::{Frame, Settings as PresentationSettings};
use egui::{pos2, vec2, Rect};
use i18n::{t, tf};
use render::unit_models::{model_by_id, UnitModelBook};
use render::world::state::RenderWorld;
use render::world_models::WorldBook;
use sandbox_content::pack::Pack;
use sandbox_rules::game::{new_game, Setup};
use sandbox_rules::Rules;

use screens::{Request, CITY, NEW_GAME};
use session::Session;

/// The seed of a new game from the menu (fixed: the same world every time).
pub const MENU_SEED: u64 = 2026;

/// The sandbox module's user interface and its match.
pub struct SandboxUi {
    pack: Pack,
    rules: Arc<Rules>,
    pub session: Option<Session>,
    hall_of_fame: Vec<String>,
    actions: Vec<ShellAction>,
    /// What a screen or panel asked for this frame, applied after the UI.
    requests: Vec<Request>,
    found_clicked: bool,
}

impl SandboxUi {
    pub fn game() -> Self {
        let rules = sandbox_content::rules().expect("the sandbox pack compiles");
        Self {
            pack: sandbox_content::pack(),
            rules: Arc::new(rules),
            session: None,
            hall_of_fame: Vec::new(),
            actions: Vec::new(),
            requests: Vec::new(),
            found_clicked: false,
        }
    }

    /// The renderer's ground texture layers of the pack.
    pub fn terrain_layers() -> Vec<(u32, u32, Vec<u8>)> {
        sandbox_content::art::terrain_layers(&sandbox_content::pack())
    }

    /// Start a new match on `setup`; `humans` players are played by hand.
    pub fn start(&mut self, setup: Setup, humans: u8) {
        let game = new_game(&self.rules, setup);
        let session = Session::new(game, self.rules.clone(), humans);
        let summary = tf!("sandbox.lib.match-summary", faction = session.faction(), width = setup.width, height = setup.height);
        self.session = Some(session);
        self.actions.push(ShellAction::MatchStarted { summary });
    }

    fn apply(&mut self, request: Request) {
        match request {
            Request::StartGame => self.start(Setup::standard(MENU_SEED), 1),
            Request::Produce(item) => {
                if let Some(session) = &mut self.session {
                    session.produce(item);
                }
            }
        }
    }

    /// The selected unit's panel: name, health, moves, and Found city for a settler.
    fn unit_panel(&mut self, ctx: &egui::Context, screen: Rect) {
        let Some(session) = &self.session else { return };
        let Some(unit) = session.selected_unit() else { return };
        let (name, civilian) = session.unit_label(unit.kind.0);
        let founder = self.rules.units.get(unit.kind.0 as usize).is_some_and(|u| u.founder);
        let home = unit_panel_rect(screen);
        let Some(r) = layout::place(ctx, layout::Panel::at("sandbox_unit_panel", layout::Anchor::BottomLeft, home).priority(100)) else { return };
        let message = session.message.clone();
        egui::Area::new("sandbox_unit_panel".into()).fixed_pos(r.min).show(ctx, |ui| {
            ui.set_min_size(r.size());
            let p = ui.painter();
            panel(p, r);
            text(p, r.min + vec2(16., 12.), &name, 20., CREAM);
            let moves = f32::from(unit.moves_left) / f32::from(kernel::world::unit::MOVE_DENOMINATOR);
            let line = if civilian {
                tf!("sandbox.lib.unit-line-civilian", hp = unit.hp, moves = moves.round() as i32)
            } else {
                tf!("sandbox.lib.unit-line", hp = unit.hp, moves = moves.round() as i32)
            };
            text(p, r.min + vec2(16., 42.), line, 14., MUTED);
            if let Some(message) = message {
                text(p, r.min + vec2(16., 64.), message, 13., egui::Color32::from_rgb(214, 96, 84));
            }
            let found = Rect::from_min_size(pos2(r.right() - 136., r.top() + 12.), vec2(120., 34.));
            if founder && button(ui, found, t!("sandbox.lib.found-city"), false, unit.moves_left > 0) {
                self.found_clicked = true;
            }
        });
    }
}

fn unit_panel_rect(screen: Rect) -> Rect {
    Rect::from_min_size(pos2(screen.left() + 16., screen.bottom() - 110.), vec2(360., 92.))
}

impl ModuleUi for SandboxUi {
    fn screen_spec(&self, screen: ScreenId) -> ScreenSpec {
        screens::spec(screen)
    }

    fn menu(&self, shell: ShellFlags) -> Vec<MenuGroup> {
        let mut game = vec![Screen::Menu, Screen::Settings];
        if shell.has_session {
            game.insert(0, Screen::Module(CITY));
        }
        vec![MenuGroup { title: t!("sandbox.lib.title"), screens: game }]
    }

    fn new_game_screen(&self) -> ScreenId {
        NEW_GAME
    }

    fn gallery_screen(&self) -> Option<ScreenId> {
        None
    }

    fn screen_opened(&mut self, _: ScreenId, _: Screen) {}

    fn draw_screen(&mut self, ui: &mut egui::Ui, body: Rect, screen: ScreenId, _: ShellFlags, _: Option<&RenderWorld>) {
        let request = match (screen, &self.session) {
            (CITY, Some(session)) => screens::draw_city(ui, body, session),
            (CITY, None) => None,
            _ => screens::draw_new_game(ui, body),
        };
        self.requests.extend(request);
    }

    fn back_from(&self, screen: ScreenId) -> Option<Screen> {
        (screen == NEW_GAME).then_some(Screen::Menu)
    }

    fn escape_to(&self, _: ScreenId) -> Option<Screen> {
        None
    }

    fn creating_match(&self) -> bool {
        false
    }

    fn key_help(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            (t!("sandbox.lib.key-left-click"), t!("sandbox.lib.key-left-click-help")),
            (t!("sandbox.lib.key-right-click"), t!("sandbox.lib.key-right-click-help")),
        ]
    }

    fn preload_screen_art(&mut self, _: &egui::Context, _: Screen) {}

    fn shortcuts(&self) -> client::shell::keymap::ShortcutSet {
        shortcuts::SET
    }

    fn shortcut(&mut self, id: &str, _shell: ShellFlags) -> bool {
        match (id, self.session.as_mut()) {
            ("order-found-city", Some(session)) => session.found_city(),
            _ => return false,
        }
        true
    }

    fn cancel(&mut self) -> bool {
        false
    }

    fn click(&mut self, tile: Option<u32>, button: MapButton) {
        let Some(session) = &mut self.session else { return };
        if session.click(tile, button) {
            self.actions.push(ShellAction::Open(Screen::Module(CITY)));
        }
    }

    fn select_city(&mut self, tile: u32) {
        if self.session.as_mut().is_some_and(|s| s.select_city_on(tile)) {
            self.actions.push(ShellAction::Open(Screen::Module(CITY)));
        }
    }

    fn view(&self) -> Option<&RenderWorld> {
        self.session.as_ref().map(|s| &s.client.render_world)
    }

    fn update(&mut self) {}

    fn art_books(&self) -> Option<(UnitModelBook, WorldBook)> {
        let rules = self.session.as_ref()?.rules();
        let models = sandbox_content::art::unit_models(&self.pack, rules);
        let units = UnitModelBook::from_models(models.into_iter().map(|m| m.and_then(model_by_id)));
        Some((units, WorldBook::from_rules(rules)))
    }

    fn turn(&self) -> u32 {
        self.session.as_ref().map_or(0, Session::round)
    }

    fn era(&self) -> u8 {
        0
    }

    fn outcome(&self) -> Outcome {
        self.session.as_ref().map_or(Outcome::Playing, Session::outcome)
    }

    fn hall_of_fame_line(&self) -> String {
        let Some(session) = &self.session else { return String::new() };
        let outcome = match session.outcome() {
            Outcome::Playing => t!("sandbox.lib.outcome-playing"),
            Outcome::Victory => t!("sandbox.lib.outcome-victory"),
            Outcome::Defeat => t!("sandbox.lib.outcome-defeat"),
        };
        tf!("sandbox.lib.hall-of-fame", faction = session.faction(), outcome = outcome, round = session.round())
    }

    fn set_hall_of_fame(&mut self, lines: &[String]) {
        self.hall_of_fame = lines.to_vec();
    }

    fn game_ended(&mut self) {}

    fn module_id(&self) -> ModuleId {
        <sandbox_rules::Sandbox as kernel::module::RulesModule>::ID
    }

    fn save_bytes(&self) -> Option<Vec<u8>> {
        self.session.as_ref().filter(|s| !s.ai_busy()).map(Session::save_bytes)
    }

    fn command_journal(&mut self) -> Option<&mut dyn client::shell::saves::CommandJournal> {
        self.session.as_mut().map(|s| &mut s.client as _)
    }

    fn load_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.session = Some(Session::from_save(bytes, self.rules.clone())?);
        Ok(())
    }

    fn turn_running(&self) -> bool {
        self.session.as_ref().is_some_and(Session::ai_busy)
    }

    fn begin_end_turn(&mut self) {
        if let Some(session) = &mut self.session {
            session.end_turn();
        }
    }

    fn tick_ai(&mut self, slice: Duration) -> AiProgress {
        self.session.as_mut().map_or(AiProgress::Idle, |s| s.tick_ai(slice))
    }

    fn finish_ai_turns(&mut self) {
        if let Some(session) = &mut self.session {
            session.finish_ai_turns();
        }
    }

    fn selected_tile(&self) -> Option<u32> {
        self.session.as_ref().and_then(Session::selected_tile)
    }

    fn selected_unit_id(&self) -> Option<u64> {
        self.session.as_ref().and_then(Session::selected_unit).map(|u| u.id.0)
    }

    fn unit_selected(&self) -> bool {
        self.selected_unit_id().is_some()
    }

    /// The sandbox plays no animations: the map shows the world as it is.
    fn advance_presentation(&mut self, _: f32, _: PresentationSettings) -> Frame {
        Frame::default()
    }

    fn presentation_idle(&self) -> bool {
        true
    }

    fn fit_camera(&mut self, _: Screen, _: CameraFit<'_>) -> bool {
        false
    }

    fn unit_kinds(&self) -> Vec<KindLabel> {
        let Some(session) = &self.session else { return Vec::new() };
        (0..self.rules.units.len() as u16)
            .map(|kind| {
                let (name, civilian) = session.unit_label(kind);
                KindLabel { name, civilian }
            })
            .collect()
    }

    fn refresh(&mut self, _: FrameInput) -> Option<HudFeed> {
        let session = self.session.as_ref()?;
        Some(HudFeed {
            turn: session.round(),
            nation: session.faction(),
            top_bar: session.top_bar(),
            banners: session.banners(),
            units: None,
            route: None,
            end_turn: Some(client::shell::module::EndTurn::ready_if(session.outcome() == Outcome::Playing)),
            reference: None,
            status: None,
        })
    }

    fn prepare_ui(&mut self, _: MapView<'_>, _: ShellFlags) {}

    fn hud_reserved(&self, view: &HudView<'_>) -> Option<Rect> {
        let session = self.session.as_ref()?;
        session.selected_unit().map(|_| unit_panel_rect(view.screen))
    }

    fn draw_hud(&mut self, ctx: &egui::Context, layer: HudLayer, view: &HudView<'_>) {
        if layer == HudLayer::Panels {
            self.unit_panel(ctx, view.screen);
        }
    }

    fn draw_tile_hover(&mut self, _: &egui::Context, _: Option<&RenderWorld>, _: Option<u32>) {}

    fn after_ui(&mut self, _: &egui::Context, _: Option<u32>, _: ShellFlags) {
        if std::mem::take(&mut self.found_clicked) {
            if let Some(session) = &mut self.session {
                session.found_city();
            }
        }
        for request in std::mem::take(&mut self.requests) {
            self.apply(request);
        }
    }

    fn take_actions(&mut self) -> Vec<ShellAction> {
        std::mem::take(&mut self.actions)
    }
}
