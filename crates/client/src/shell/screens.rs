//! The screen stack: what fills the window (the map, the menu, the settings or a module
//! screen), the header with Back and the Empire menu, and the main menu. Module screens
//! draw themselves (`ModuleUi::draw_screen`) inside the frame drawn here.
use super::art::ScreenArt;
use super::kit::{art_tint, button, darken, panel, paragraph, text};
use super::theme::Theme;
use super::module::{MenuCover, ModuleUi, Screen, ScreenMusic, ShellAction, ShellFlags, ShellWords};
use super::settings::Settings;
use egui::{pos2, vec2, Color32, Rect, Sense, Stroke};
use render::world::state::RenderWorld;

/// The music the shell plays for a screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicMode {
    Menu,
    Game,
}

/// Size of the management canvas module screens are laid out on; smaller windows
/// scale it down as one layer.
const CANVAS: egui::Vec2 = vec2(1150., 640.);

pub struct ScreenState {
    pub screen: Screen,
    navigation_open: bool,
    pub dev_mode: bool,
    pub has_session: bool,
    pub quit_requested: bool,
    /// The game binary: the menu offers save and load, screens show the match.
    pub game_mode: bool,
    /// Some save exists that the Load list can show.
    pub save_available: bool,
    /// A quick save, an autosave or a named save exists: Continue loads the newest, Load lists them.
    pub continue_available: bool,
    /// Player settings; the app applies and persists them.
    pub settings: Settings,
    pub menu_status: Option<String>,
    /// Save, Load and Continue clicked in the menu, applied by the app.
    pub requests: Vec<ShellAction>,
    match_summary: String,
    art: ScreenArt,
    music_error: Option<String>,
    /// The languages the settings screen offers (read again when the catalogue changes).
    pub(super) languages: super::language::LanguageList,
    /// Problems found in the player's language bundles at start-up.
    pub language_notes: Vec<i18n::Diagnostic>,
    /// The screen of the last frame (`notify_opened`).
    last_shown: Screen,
    /// The keyboard shortcuts overlay and the remapping in progress.
    pub shortcuts: super::shortcuts::ShortcutsUi,
    /// The "Report a bug" dialog (`report`).
    pub report: super::report::ReportUi,
    /// The "Load game" list and the "Save game" naming dialog (`saves_screen`).
    pub saves: super::saves_screen::SavesUi,
}

impl Default for ScreenState {
    fn default() -> Self {
        Self {
            screen: Screen::Map,
            navigation_open: false,
            dev_mode: std::env::var("AURELIA_VISUAL_DEV").as_deref() == Ok("1"),
            has_session: false,
            quit_requested: false,
            game_mode: false,
            save_available: false,
            continue_available: false,
            settings: Default::default(),
            menu_status: None,
            requests: Vec::new(),
            match_summary: String::new(),
            art: Default::default(),
            music_error: None,
            languages: Default::default(),
            language_notes: Vec::new(),
            last_shown: Screen::Map,
            shortcuts: Default::default(),
            report: Default::default(),
            saves: Default::default(),
        }
    }
}

impl ScreenState {
    pub fn flags(&self) -> ShellFlags {
        ShellFlags {
            game_mode: self.game_mode,
            has_session: self.has_session,
            dev_mode: self.dev_mode,
            screen: self.screen,
            brightness: self.settings.brightness,
        }
    }
    pub fn navigation_is_open(&self) -> bool {
        self.navigation_open
    }
    pub fn close_navigation(&mut self) -> bool {
        std::mem::take(&mut self.navigation_open)
    }
    /// Offscreen captures: open the Empire menu.
    pub fn capture_navigation(&mut self, open: bool) {
        self.navigation_open = open;
    }
    pub fn preload_atlas_review(&mut self, ctx: &egui::Context, sheet: &str) {
        self.art.preload_sheet(ctx, sheet);
    }
    pub fn draw_atlas_review(&mut self, ui: &mut egui::Ui, sheet: &str) {
        self.art.draw_sheet_review(ui, sheet);
    }
    /// Decode the art of the shown screen now (offscreen captures).
    pub fn preload_screen_art(&mut self, ctx: &egui::Context, module: &mut dyn ModuleUi) {
        if self.screen == Screen::Menu {
            self.art.preload(ctx, &[module.menu_cover().art]);
        }
        module.preload_screen_art(ctx, self.screen);
    }
    /// A match runs: show the map, and the summary in the menu.
    pub fn match_ready(&mut self, summary: String) {
        self.has_session = true;
        self.match_summary = summary;
        self.navigate(Screen::Map);
    }
    pub fn set_music_error(&mut self, error: Option<String>) {
        self.music_error = error;
    }
    /// Music loudness after the master volume.
    pub fn music_volume(&self) -> f32 {
        self.settings.music_level()
    }
    pub fn music_muted(&self) -> bool {
        self.settings.music_muted
    }
    pub fn effects_volume(&self) -> f32 {
        self.settings.effects_level()
    }
    pub fn effects_muted(&self) -> bool {
        self.settings.effects_muted
    }
    pub fn music_error(&self) -> Option<&str> {
        self.music_error.as_deref()
    }
    pub fn presentation(&self) -> super::presentation::Settings {
        self.settings.animation
    }
    fn navigate(&mut self, screen: Screen) {
        self.navigation_open = false;
        self.screen = screen;
    }
    pub fn open(&mut self, screen: Screen) {
        self.navigate(screen);
    }
    /// The music for the shown screen: menu music before a match, game music over the map
    /// and the game screens; settings and reference screens keep what was playing.
    pub fn music(&self, previous: MusicMode, module: &dyn ModuleUi) -> MusicMode {
        match self.screen {
            Screen::Menu => MusicMode::Menu,
            Screen::Settings => previous,
            Screen::Map => MusicMode::Game,
            Screen::Module(id) => match module.screen_spec(id).music {
                ScreenMusic::Menu => MusicMode::Menu,
                ScreenMusic::Game => MusicMode::Game,
                ScreenMusic::Keep => previous,
            },
        }
    }
}

/// The map shows under this screen.
pub fn wants_world(screen: Screen, module: &dyn ModuleUi) -> bool {
    match screen {
        Screen::Map => true,
        Screen::Menu | Screen::Settings => false,
        Screen::Module(id) => module.screen_spec(id).shows_world,
    }
}

fn label(screen: Screen, module: &dyn ModuleUi) -> &'static str {
    let words = module.words();
    match screen {
        Screen::Map => words.world_map,
        Screen::Menu => words.menu,
        Screen::Settings => words.settings,
        Screen::Module(id) => module.screen_spec(id).title,
    }
}

fn hotkey(screen: Screen, module: &dyn ModuleUi) -> Option<&'static str> {
    match screen {
        Screen::Menu => Some("Esc"), // i18n-ok: the name of a key
        Screen::Map | Screen::Settings => None,
        Screen::Module(id) => module.screen_spec(id).hotkey,
    }
}

fn needs_game(screen: Screen, module: &dyn ModuleUi) -> bool {
    match screen {
        Screen::Module(id) => module.screen_spec(id).needs_game,
        _ => false,
    }
}

/// Settings and the module's reference screens go back to the menu (or where the
/// module says), the others to the map.
fn back_target(screen: Screen, module: &dyn ModuleUi) -> Option<Screen> {
    match screen {
        Screen::Settings => Some(Screen::Menu),
        Screen::Module(id) => module.back_from(id),
        Screen::Map | Screen::Menu => None,
    }
}

/// Tell the module which of its screens the shell switched to since the last frame.
fn notify_opened(state: &mut ScreenState, module: &mut dyn ModuleUi) {
    if state.screen == state.last_shown {
        return;
    }
    if let Screen::Module(id) = state.screen {
        module.screen_opened(id, state.last_shown);
    }
    state.last_shown = state.screen;
}

/// A phone held sideways (a touch screen under 480 points high): the header shrinks to one
/// 52-point row and the footer line goes, so the screen's own panels get 311 of 375 points
/// instead of 258. The screens that show the world keep the bars over it as they are.
fn compact_chrome(r: Rect, shows_world: bool) -> bool {
    super::theme::touch_wanted() && r.height() < 480. && !shows_world
}

fn header(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, module: &dyn ModuleUi) {
    let theme = Theme::of(ui.ctx());
    let c = theme.colors;
    let title = module.menu_cover().title;
    let width = ui.painter().layout_no_wrap(title.into(), theme.title(23.), c.accent).size().x;
    let compact = compact_chrome(r, wants_world(state.screen, module));
    // A phone's width: the screen's name takes the place of the title, the buttons are narrower.
    let narrow = r.width() < 520. || compact;
    if compact {
        text(ui.painter(), r.min + vec2(24., 14.), label(state.screen, module), 22., c.text);
    } else if narrow {
        text(ui.painter(), r.min + vec2(20., 21.), label(state.screen, module), 20., c.text);
    } else {
        ui.painter().text(r.min + vec2(26., 19.), egui::Align2::LEFT_TOP, title, theme.title(23.), c.accent);
    }
    if r.width() >= 600. && !compact {
        text(ui.painter(), r.min + vec2(166f32.max(width + 52.), 22.), label(state.screen, module), 20., c.text);
    }
    let (back_width, back_gap) = if narrow { (104., 112.) } else { (132., 128.) };
    let (top, height) = if compact { (6., 40.) } else { (15., 36.) };
    let back = Rect::from_min_size(pos2(r.right() - back_width - if compact { 12. } else if narrow { 14. } else { 26. }, r.top() + top), vec2(back_width, height));
    let reference = back_target(state.screen, module);
    let waiting = module.creating_match();
    let words = module.words();
    if button(ui, back, if reference.is_some() { words.back } else { words.back_to_map }, false, !waiting) {
        state.navigate(reference.unwrap_or(Screen::Map));
    }
    let toggle = Rect::from_min_size(pos2(back.left() - back_gap, back.top()), vec2(if narrow { 100. } else { 116. }, height));
    if !waiting && button(ui, toggle, words.navigate, state.navigation_open, true) {
        state.navigation_open = !state.navigation_open;
    }
    let rule_y = if compact { 52. } else { 65. };
    let rule = [r.min + vec2(if compact { 12. } else { 24. }, rule_y), pos2(r.right() - if compact { 12. } else { 24. }, r.top() + rule_y)];
    ui.painter().line_segment(rule, Stroke::new(1., c.accent.gamma_multiply(0.4)));
}

/// One anchored panel, shared by the map and the management screens.
fn navigation_panel(ctx: &egui::Context, state: &mut ScreenState, module: &dyn ModuleUi) {
    if !state.navigation_open {
        return;
    }
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        state.close_navigation();
        return;
    }
    let screen = ctx.content_rect();
    let width = 376_f32.min((screen.width() - 24.).max(1.));
    let frame = module.hud_frame().rects(screen);
    let top = if state.screen == Screen::Map { frame.empire.bottom().max(frame.top_bar.bottom()) + 9. - screen.top() } else { 59. };
    let height = (screen.height() - top - 12.).clamp(1., 442.);
    let rect = Rect::from_min_size(pos2(screen.right() - width - 12., screen.top() + top), vec2(width, height));
    // Backdrop and buttons share one layer. Separate Foreground Areas can
    // reorder on pointer-down and leave the visible panel behind its shield.
    let id = egui::Id::new("empire_navigation_modal");
    ctx.move_to_top(egui::LayerId::new(egui::Order::Foreground, id));
    let groups = module.menu(state.flags());
    let c = Theme::of(ctx).colors;
    egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(screen.min).constrain(false).show(ctx, |ui| {
        ui.set_min_size(screen.size());
        let (_, backdrop) = ui.allocate_exact_size(screen.size(), Sense::click());
        panel(ui.painter(), rect);
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(12.)), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("empire_navigation_entries")
                .max_height((height - 24.).max(1.))
                .show(ui, |ui| {
                    for group in &groups {
                        ui.label(egui::RichText::new(group.title).color(c.accent).size(11.));
                        let shown = |s: &&Screen| state.game_mode || !needs_game(**s, module);
                        let screens: Vec<Screen> = group.screens.iter().filter(shown).copied().collect();
                        for row in screens.chunks(2) {
                            let (area, _) = ui.allocate_exact_size(vec2(width - 24., 36.), Sense::hover());
                            for (i, target) in row.iter().enumerate() {
                                let at = area.min + vec2(i as f32 * (width - 20.) * 0.5, 0.);
                                let cell = Rect::from_min_size(at, vec2((width - 32.) * 0.5, 32.));
                                let name = label(*target, module);
                                if let Some(key) = hotkey(*target, module) {
                                    let shown = super::keymap::key_text(ui.ctx(), key);
                                    ui.interact(cell, ui.id().with(("nav_tip", name)), Sense::hover()).on_hover_text(shown);
                                }
                                if button(ui, cell, name, state.screen == *target, true) {
                                    state.navigate(*target);
                                }
                            }
                        }
                        ui.add_space(8.);
                    }
                });
        });
        if backdrop.clicked() && backdrop.interact_pointer_pos().is_some_and(|at| !rect.contains(at)) {
            state.close_navigation();
        }
    });
}

pub fn draw_navigation(ctx: &egui::Context, state: &mut ScreenState, module: &mut dyn ModuleUi) {
    notify_opened(state, module);
    if state.screen != Screen::Map {
        return;
    }
    let r = module.hud_frame().rects(ctx.content_rect()).empire;
    egui::Area::new("screen_navigation".into())
        .order(egui::Order::Foreground)
        .fixed_pos(r.min)
        .show(ctx, |ui| {
            let (button_rect, _) = ui.allocate_exact_size(r.size(), Sense::hover());
            if button(ui, button_rect, module.words().empire, state.navigation_open, true) {
                state.navigation_open = !state.navigation_open;
            }
        });
    navigation_panel(ctx, state, module);
}

/// Draw the shown full screen; false over the map (the HUD draws then).
/// `scene`: the world the shell shows when it is not the module's match (showcase).
pub fn draw(ctx: &egui::Context, state: &mut ScreenState, module: &mut dyn ModuleUi, scene: Option<&RenderWorld>) -> bool {
    notify_opened(state, module);
    if state.screen == Screen::Map {
        return false;
    }
    let rendered_screen = state.screen;
    let screen = ctx.content_rect();
    egui::Area::new("visual_full_screen".into())
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_min_size(screen.size());
            let c = Theme::of(ui.ctx()).colors;
            let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            // What this screen paints opaque, for the map overlays to keep out of (D309): the
            // bars over the world, or the whole window.
            if wants_world(rendered_screen, module) {
                let top = Rect::from_min_max(r.min, pos2(r.right(), r.top() + 70.0));
                ui.painter().rect_filled(top, 0.0, c.background);
                let bottom = Rect::from_min_max(pos2(r.left(), r.bottom() - 29.0), r.max);
                ui.painter().rect_filled(bottom, 0.0, c.background);
                super::layout::cover(ui.ctx(), "screen_top_bar", top);
                super::layout::cover(ui.ctx(), "screen_bottom_bar", bottom);
            } else {
                ui.painter().rect_filled(r, 0.0, c.background);
                super::layout::cover(ui.ctx(), "screen_window", r);
            }
            if rendered_screen == Screen::Menu {
                menu(ui, r, state, module);
            } else {
                header(ui, r, state, module);
                let body = if compact_chrome(r, wants_world(rendered_screen, module)) {
                    Rect::from_min_max(pos2(r.left() + 12.0, r.top() + 58.0), pos2(r.right() - 12.0, r.bottom() - 8.0))
                } else {
                    Rect::from_min_max(pos2(r.left() + 24.0, r.top() + 80.0), pos2(r.right() - 24.0, r.bottom() - 37.0))
                };
                draw_body(ui, body, rendered_screen, state, module, scene);
            }
            let footer = match rendered_screen {
                Screen::Module(id) if !state.game_mode => module.screen_spec(id).footer,
                _ => "",
            };
            if !(compact_chrome(r, wants_world(rendered_screen, module)) && rendered_screen != Screen::Menu) {
                text(ui.painter(), pos2(r.left() + 26.0, r.bottom() - 23.0), footer, 11.0, c.muted);
            }
        });
    navigation_panel(ctx, state, module);
    if state.screen != rendered_screen {
        ctx.request_repaint();
    }
    // This frame began as a modal screen, even when a navigation click closes it.
    true
}

fn draw_body(
    ui: &mut egui::Ui,
    body: Rect,
    screen: Screen,
    state: &mut ScreenState,
    module: &mut dyn ModuleUi,
    scene: Option<&RenderWorld>,
) {
    let id = match screen {
        Screen::Settings => {
            // While an observed match runs, the keys of the observer follow the module's own.
            let observing = module.observed().is_some();
            let mut keys = module.key_help();
            if observing {
                keys.extend_from_slice(module.words().observer.keys);
            }
            return super::settings_screen::draw(ui, body, state, module, &keys);
        }
        Screen::Module(id) => id,
        Screen::Map | Screen::Menu => return,
    };
    // A world-showing screen of the match lays its panels around the map at full size;
    // the other module screens scale their canvas down as one layer on small windows.
    let full_size = module.screen_spec(id).shows_world && state.game_mode && state.has_session;
    if full_size || (body.width() >= CANVAS.x && body.height() >= CANVAS.y) {
        return module.draw_screen(ui, body, id, state.flags(), scene);
    }
    // egui applies the same transform to painting and pointer hit-testing.
    // A touch screen: the module's own smaller design canvas, so that the text stays readable.
    let design = if super::theme::touch_wanted() { module.phone_canvas(id, body.size()).min(CANVAS) } else { CANVAS };
    let scale = (body.width() / design.x).min(body.height() / design.y).min(1.);
    let canvas = Rect::from_min_size(egui::Pos2::ZERO, body.size() / scale);
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("responsive_management_canvas"));
    ui.ctx().set_transform_layer(layer, egui::emath::TSTransform::new(body.min.to_vec2(), scale));
    ui.ctx().set_sublayer(ui.layer_id(), layer);
    // ScrollArea checks the topmost registered Area, not only widget hit
    // tests. A bare UiBuilder layer paints and clicks correctly but never
    // receives wheel input because the enclosing fullscreen Area wins.
    egui::Area::new(layer.id)
        .order(layer.order)
        .fixed_pos(egui::Pos2::ZERO)
        .default_size(canvas.size())
        .constrain(false)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(canvas.size());
            ui.set_max_size(canvas.size());
            ui.set_clip_rect(canvas);
            module.draw_screen(ui, canvas, id, state.flags(), scene);
        });
}

fn menu(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, module: &dyn ModuleUi) {
    if state.game_mode {
        game_menu(ui, r, state, module);
    } else {
        illustrated_menu(ui, r, state, module);
    }
}

/// Where the menu's column stands: its left edge, the origin the rows are laid out from, its
/// width and how far the rows are squeezed together so that the last one still fits on a phone
/// held sideways (1 on every other screen).
#[derive(Clone, Copy)]
struct MenuColumn {
    x: f32,
    y: f32,
    width: f32,
    squeeze: f32,
}

impl MenuColumn {
    /// The row that starts `top` points below the origin in the tall layout, `height` high there.
    fn row(&self, top: f32, height: f32) -> Rect {
        Rect::from_min_size(pos2(self.x, self.y + 155. + (top - 155.) * self.squeeze), vec2(self.width, height * self.squeeze))
    }
}

/// The two halves of a row, 8 points apart.
fn halves(row: Rect) -> (Rect, Rect) {
    let half = (row.width() - 8.) / 2.;
    (Rect::from_min_size(row.min, vec2(half, row.height())), Rect::from_min_size(row.min + vec2(half + 8., 0.), vec2(half, row.height())))
}

/// Game menu: Continue resumes the match (or loads the newest save), Save asks for a name and
/// Load lists the saves (the quick-save keys keep the quick save).
fn game_menu(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, module: &dyn ModuleUi) {
    let col = menu_frame(ui, r, state, module.menu_cover(), GAME_MENU_END);
    let words = &module.words().main_menu;
    let resume = state.has_session || state.continue_available;
    if button(ui, col.row(155., 44.), words.resume, true, resume) {
        if state.has_session {
            state.navigate(Screen::Map);
        } else {
            state.requests.push(ShellAction::Continue);
        }
    }
    if button(ui, col.row(211., 44.), words.new_game, false, true) {
        state.navigate(Screen::Module(module.new_game_screen()));
    }
    let (save, load) = halves(col.row(267., 44.));
    if button(ui, save, words.save_game, false, state.has_session) {
        state.requests.push(ShellAction::SaveDialog);
    }
    if button(ui, load, words.load_game, false, state.save_available) {
        state.requests.push(ShellAction::LoadDialog);
    }
    game_menu_footer(ui, r, state, col, module.words());
}

/// Offset of the bottom of the game menu's last row (Report a bug) in the tall layout.
const GAME_MENU_END: f32 = 419. + 34.;

fn game_menu_footer(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, col: MenuColumn, words: &ShellWords) {
    let c = Theme::of(ui.ctx()).colors;
    if button(ui, col.row(323., 38.), words.settings, false, true) {
        state.navigate(Screen::Settings);
    }
    if button(ui, col.row(371., 38.), words.main_menu.quit, false, true) {
        state.quit_requested = true;
    }
    report_button(ui, state, col.row(419., 34.));
    let sideways = r.height() < 520.0;
    if let Some(status) = &state.menu_status {
        if sideways {
            // No room under the rows: the status stands above "your world" on the right.
            let galley = ui.painter().layout(status.clone(), egui::FontId::proportional(13.), c.accent, 290.);
            let at = pos2(r.right() - 310.0, r.bottom() - 112.0 - galley.size().y);
            ui.painter().galley(at, galley, c.accent);
        } else {
            paragraph(ui.painter(), pos2(col.x, col.y + 465.), status, 13., c.accent, col.width);
        }
    }
    if state.has_session {
        text(ui.painter(), pos2(r.right() - 310.0, r.bottom() - 100.0), words.main_menu.your_world, 15.0, c.accent);
        let summary = state.match_summary.clone();
        text(ui.painter(), pos2(r.right() - 310.0, r.bottom() - 72.0), summary, 14.0, c.text);
    }
}

/// "Report a bug": the dialog opens when the app has gathered the facts (`ShellAction::ReportBug`).
fn report_button(ui: &mut egui::Ui, state: &mut ScreenState, row: Rect) {
    if button(ui, row, i18n::t!("shell.main-menu.report-bug"), false, true) {
        state.requests.push(ShellAction::ReportBug);
    }
}

/// Menu artwork, title block and the button column position. `rows_end` is where the bottom of
/// the last row lies in the tall layout (offsets of `MenuColumn::row`).
fn menu_frame(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, cover: MenuCover, rows_end: f32) -> MenuColumn {
    let theme = Theme::of(ui.ctx());
    let c = theme.colors;
    let tint = art_tint(state.settings.brightness);
    state.art.paint(ui.ctx(), ui.painter(), r, cover.art, tint);
    darken(ui.painter(), r, 15, 80);
    // A narrow window (a phone held upright): the column of rows takes the whole width.
    let narrow = r.width() < 520.0;
    let left = Rect::from_min_max(r.min, pos2(r.left() + r.width() * if narrow { 1.0 } else { 0.39 }, r.bottom()));
    ui.painter().rect_filled(left, 0.0, Color32::from_black_alpha(if narrow { 150 } else { 176 }));
    let x = r.left() + r.width() * if narrow { 0.08 } else { 0.065 };
    let width = (left.right() - x - 36.0).min(320.0);
    if r.height() < 520.0 {
        // A phone held sideways (about 390 pt high): only the title, the rows start 72 pt below
        // the top. `y` is the origin the rows are laid out from, so it lies above the screen. The
        // rows are squeezed (down to 70 %) until the last one ends 8 pt above the bottom: the
        // game menu's six rows do not fit at 360 pt otherwise.
        ui.painter().text(pos2(x, r.top() + 10.0), egui::Align2::LEFT_TOP, cover.title, theme.title(40.0), c.text);
        let y = r.top() - 83.0;
        let squeeze = ((r.bottom() - 8.0 - (y + 155.0)) / (rows_end - 155.0)).clamp(0.7, 1.0);
        return MenuColumn { x, y, width, squeeze };
    }
    let y = r.top() + r.height() * 0.17;
    text(ui.painter(), pos2(x, y - 32.0), cover.kicker, 11.0, c.accent);
    let size = r.width().mul_add(0.012, 37.0).min(60.0);
    ui.painter().text(pos2(x, y), egui::Align2::LEFT_TOP, cover.title, theme.title(size), c.text);
    text(ui.painter(), pos2(x, y + 78.0), cover.tagline, 18.0, c.muted);
    ui.painter().line_segment([pos2(x, y + 122.0), pos2(x + 240.0, y + 122.0)], Stroke::new(1.0, c.accent));
    MenuColumn { x, y, width, squeeze: 1.0 }
}

/// The showcase menu: explore the sample world, set up a match, settings.
fn illustrated_menu(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, module: &dyn ModuleUi) {
    let gallery = module.gallery_screen().filter(|_| state.dev_mode);
    // The developer's gallery row keeps its place; the report row goes below it, or under Quit.
    let report_row = if gallery.is_some() { 423. } else { 371. };
    let col = menu_frame(ui, r, state, module.menu_cover(), report_row + 34.);
    let c = Theme::of(ui.ctx()).colors;
    let all = module.words();
    let words = &all.main_menu;
    let explore = if state.has_session { words.continue_journey } else { words.explore };
    let entries = [(explore, Screen::Map), (words.new_game, Screen::Module(module.new_game_screen())), (all.settings, Screen::Settings)];
    for (i, (label, target)) in entries.into_iter().enumerate() {
        if button(ui, col.row(155. + i as f32 * 56., 44.), label, i == 0, true) {
            state.navigate(target);
        }
    }
    if button(ui, col.row(323., 38.), words.quit, false, true) {
        state.quit_requested = true;
    }
    report_button(ui, state, col.row(report_row, 34.));
    if let Some(gallery) = gallery {
        if button(ui, col.row(381., 34.), words.gallery, false, true) {
            state.navigate(Screen::Module(gallery));
        }
    }
    let (title, line) = if state.has_session {
        (words.your_world, state.match_summary.as_str())
    } else {
        (words.showcase_title, words.showcase_line)
    };
    text(ui.painter(), pos2(r.right() - 310.0, r.bottom() - 100.0), title, 15.0, c.accent);
    text(ui.painter(), pos2(r.right() - 310.0, r.bottom() - 72.0), line, 14.0, c.text);
}
