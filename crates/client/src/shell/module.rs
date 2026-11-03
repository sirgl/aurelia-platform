//! The contract between the app shell and a game module's user interface.
//!
//! The shell owns the window-independent frame: the map (renderer, camera, picking,
//! hover), the screen stack with its header and Empire menu frame, the main menu,
//! settings, sound cues, saves and the HUD frame (top bar, city labels, unit badges,
//! minimap). A module owns its match and everything that names a mechanic: its
//! screens and their view models, its HUD panels, its keyboard, tooltips and notices.
//!
//! Calls go one way. The shell asks the module (`ModuleUi`) for data and drawing; the
//! module never reaches into the shell, it queues `ShellAction`s that the shell applies
//! after the frame. One module runs per app; the app names it in one place.
use egui::{vec2, Color32, Rect, Vec2};
use std::borrow::Cow;
use kernel::ids::UnitKindId;
pub use kernel::world::module::ModuleId;
use render::camera::CameraController;
use render::scene::terrain::surface::SurfaceMesh;
use render::unit_models::UnitModelBook;
use render::world::state::RenderWorld;
use render::world_models::WorldBook;

use super::hud::Icon;
pub use super::hud::HudFrame;
use super::layout::Panel;
pub use super::hud::map_paint::{MapPaint, MapWarning, RouteLine, Spot, TileIcon, TileTint};
pub use super::theme::Theme;
pub use super::words::ShellWords;
use super::presentation::{Frame, Settings as PresentationSettings};
pub use super::saves::{CommandJournal, SaveProblem, SaveSummary};

/// A screen of the module, by the module's own numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScreenId(pub u8);

/// What fills the window: the map, a shell screen or a module screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Screen {
    #[default]
    Map,
    Menu,
    Settings,
    Module(ScreenId),
}

/// Music the shell plays while a screen is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenMusic {
    /// Menu music (before a match).
    Menu,
    /// Game music (the map and the game screens).
    Game,
    /// Whatever played before (reference screens).
    Keep,
}

/// How the shell frames a module screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenSpec {
    /// Title in the header and the Empire menu.
    pub title: &'static str,
    /// The action id of the shortcut that opens it (or a fixed key such as `Esc`), for the
    /// menu button's tooltip; the keymap says the key the player has now.
    pub hotkey: Option<&'static str>,
    /// Exists only with a running match (hidden from the menu before one).
    pub needs_game: bool,
    /// The world stays visible around its panels (the shell draws the map under it).
    pub shows_world: bool,
    pub music: ScreenMusic,
    /// The footer line of the screen in the showcase (the game shows none).
    pub footer: &'static str,
}

/// A titled group of the Empire menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuGroup {
    pub title: &'static str,
    pub screens: Vec<Screen>,
}

/// The main menu's artwork and title block, the module's face before a match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuCover {
    /// Screen art by name (`art::ScreenArt`): `<art>-v1.png` or `<art>.png` under
    /// `classic/assets/visual/generated`.
    pub art: &'static str,
    /// The small line above the title.
    pub kicker: &'static str,
    pub title: &'static str,
    pub tagline: &'static str,
}

impl MenuCover {
    /// The platform's own cover (the classic module and the sandbox show it), in the active language.
    pub fn aurelia() -> MenuCover {
        MenuCover { art: "menu", kicker: i18n::t!("shell.cover.kicker"), title: "AURELIA", tagline: i18n::t!("shell.cover.tagline") }
    }
}

/// The shell's state a module reads to build its menu and screens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellFlags {
    /// The game binary (screens show the match), not the showcase.
    pub game_mode: bool,
    /// A match runs (or was loaded).
    pub has_session: bool,
    /// Developer screens and samples (`AURELIA_VISUAL_DEV=1`).
    pub dev_mode: bool,
    /// The screen shown this frame.
    pub screen: Screen,
    /// Art brightness of the settings (multiplies screen art).
    pub brightness: f32,
}

/// What the shell does for a module, applied after the frame that asked.
#[derive(Debug, Clone, PartialEq)]
pub enum ShellAction {
    Open(Screen),
    /// Centre the map on a tile.
    CenterOn(u32),
    /// Centre the map on the selected unit or city.
    CenterOnSelection,
    /// The quick save, its load, and the newest of the saves.
    Save,
    Load,
    Continue,
    /// The menu's "Load game": open the list of saves.
    LoadDialog,
    /// A pick of that list: load this save.
    LoadSave(std::path::PathBuf),
    /// The list's Delete after its confirmation: remove this save.
    DeleteSave(std::path::PathBuf),
    /// The menu's "Save game": ask for a name, with the match's own as the default.
    SaveDialog,
    /// The naming dialog's Save: write the match under this name.
    SaveAs(String),
    /// A new match runs: the shell resets the map and shows it.
    MatchStarted { summary: String },
    ToggleGrid,
    /// Show or hide the yield icons on the map.
    ToggleYields,
    /// Camera zoom by a factor (> 1 zooms in).
    Zoom(f32),
    /// End the turn: the other players act in frame slices from here.
    EndTurn,
    /// An observed match runs (`ModuleUi::observed`): the shell shows the powers' panels
    /// and keeps the clock.
    ObserverStarted { summary: String },
    /// The menu's "Report a bug": open the dialog with the facts of this moment.
    ReportBug,
    /// The dialog's "Save report file": make the file and hand it to the player.
    ReportSave,
}

/// Where the match stands for the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Playing,
    Victory,
    Defeat,
}

/// Where the AI turns stand after a frame's slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProgress {
    /// No AI turn runs.
    Idle,
    /// AI turns continue next frame.
    Busy,
    /// The AI turns ended in this frame: the human plays again.
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapButton {
    Primary,
    Secondary,
}

/// A top-bar value with its hover breakdown ("Cities +12", "Unit upkeep -4").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YieldTip {
    /// A static word or a line the module built (`Cow::Owned`).
    pub title: Cow<'static, str>,
    pub total: String,
    pub lines: Vec<(String, String)>,
    pub note: Option<Cow<'static, str>>,
}

/// One value of the top bar.
#[derive(Debug, Clone, PartialEq)]
pub struct TopBarValue {
    pub icon: Icon,
    pub tint: Color32,
    /// "312 (+18) Gold".
    pub text: String,
    /// The same on a narrow window: "312 (+18)".
    pub compact: String,
    pub tip: Option<YieldTip>,
    /// A click opens something of the module (None: the value is only shown).
    pub click: Option<TopBarClick>,
    /// The player can act on this value now (a policy can be adopted): the cell is lit.
    pub ready: bool,
}

/// What a click on a top-bar value does: the module's own id of it (given back to
/// `ModuleUi::top_bar_clicked`) and the tooltip line that tells the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopBarClick {
    pub id: &'static str,
    pub hint: String,
}

/// A module's own cell at the right end of the top bar, in place of the nation
/// and turn (`HudFeed::status`). It is a button: a click reaches `ModuleUi::status_clicked`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusLine {
    /// The full text on a wide window.
    pub text: String,
    /// The same on a narrow window, or when the full text does not fit.
    pub short: String,
    /// The hover text (empty: none).
    pub tip: String,
    /// The panel the cell opens is open: the cell is drawn pressed, with the chevron up.
    pub open: bool,
}

/// Banner data of a city label (own cities show growth and production).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityBanner {
    pub tile: u32,
    pub own: bool,
    pub strength: i32,
    /// Hit points left and the maximum (a bar shows a damaged city; 0 max:
    /// the module has no city hit points).
    pub hp: u16,
    pub max_hp: u16,
    /// An own city whose ranged strike is ready with a target in range
    /// (the banner shows the bow).
    pub can_strike: bool,
    /// Food stored / needed and turns to grow (own cities).
    pub growth: Option<(u32, u32, Option<u32>)>,
    /// Item, progress / cost and turns (own cities with a queue).
    pub production: Option<(String, u32, u32, Option<u32>)>,
    /// The hover text in the module's words (None: the shell's own lines).
    pub tip: Option<String>,
    /// Names of the agents the viewer keeps in the city (spies), one line each under the name.
    pub visitors: Vec<String>,
}

/// One unit as the observer sees it on a hovered map badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitTip {
    pub kind: UnitKindId,
    pub name: String,
    pub owner: String,
    pub player: u8,
    pub own: bool,
    pub hp: u16,
    pub attack: i32,
    pub defense: i32,
    pub ranged: u16,
    pub range: u8,
    /// Own units only.
    pub moves: Option<String>,
    /// The radius the unit sees with where it stands (own units only).
    pub sight: Option<i32>,
    pub status: Option<String>,
    pub promotions: Vec<String>,
}

/// A unit kind as a map badge shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindLabel {
    pub name: String,
    /// Never fights: the badge shows a leaf, not a shield.
    pub civilian: bool,
}

/// The route of the selected unit as the ground marks show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteFeed {
    pub tiles: Vec<u32>,
    /// Number of leading tiles (including the start) reachable this turn.
    pub this_turn: usize,
    pub turns: u32,
}

/// What the shell's HUD frame draws of the running match, refreshed every frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HudFeed {
    pub turn: u32,
    /// The player's nation, beside the turn on wide windows.
    pub nation: String,
    pub top_bar: Vec<TopBarValue>,
    pub banners: Vec<CityBanner>,
    /// Units on the badge hovered in the previous frame.
    pub units: Option<(u32, Vec<UnitTip>)>,
    pub route: Option<RouteFeed>,
    /// The end-turn button while a match runs.
    pub end_turn: Option<EndTurn>,
    /// The top bar's book button: the reference screen it opens, and its tooltip.
    pub reference: Option<(Screen, &'static str)>,
    /// The module's own cell in place of "nation | Turn N" (None: the shell's text).
    pub status: Option<StatusLine>,
}

/// What the end-turn button offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndTurn {
    /// The turn can end now.
    Ready,
    /// A choice is required first (the classic module's «Choose research»): the button
    /// names it, and pressing it still reaches `ModuleUi::begin_end_turn`, where
    /// the module opens the choice.
    Decide { label: String, tip: String },
    /// The other players act (or the AI plays for the player): the turn ends by itself.
    Waiting,
    /// The turn cannot end: the game is over.
    Unavailable,
}

impl EndTurn {
    pub fn ready_if(ready: bool) -> Self {
        if ready { EndTurn::Ready } else { EndTurn::Unavailable }
    }
}

/// What a module reads at the start of a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameInput {
    pub hovered_tile: Option<u32>,
    /// The unit badge under the pointer in the previous frame.
    pub hovered_badge: Option<u32>,
    pub shell: ShellFlags,
}

/// Where a module HUD layer is drawn in the shell's HUD (egui stacks layers in the
/// order they are first drawn, so the order is part of the look).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudLayer {
    /// Under the top bar, before the city labels (status boxes).
    Status,
    /// Over everything else (orders, notices, modal choices).
    Panels,
}

/// The map as a HUD layer sees it.
pub struct HudView<'a> {
    /// The world the shell shows when it is not the module's match (the showcase's
    /// hand-built scene); None in a match, whose view the module has.
    pub scene: Option<&'a RenderWorld>,
    pub screen: Rect,
    pub selected_tile: Option<u32>,
}

/// The camera and ground a module reads while it draws over its world.
pub struct MapView<'a> {
    pub camera: &'a CameraController,
    pub surface: Option<&'a SurfaceMesh>,
    pub screen: Rect,
}

/// The camera and ground a module screen may fit while it shows its world.
pub struct CameraFit<'a> {
    pub camera: &'a mut CameraController,
    pub surface: Option<&'a SurfaceMesh>,
    pub screen: Rect,
}

/// A game module's user interface (see the module docs).
pub trait ModuleUi {
    // --- Screens -------------------------------------------------------------------

    fn screen_spec(&self, screen: ScreenId) -> ScreenSpec;
    /// The Empire menu (may list shell screens: Menu, Settings).
    fn menu(&self, shell: ShellFlags) -> Vec<MenuGroup>;
    /// The screen the menu's New game opens.
    fn new_game_screen(&self) -> ScreenId;
    /// The main menu's art and title.
    fn menu_cover(&self) -> MenuCover {
        MenuCover::aurelia()
    }
    /// The shell's words in the module's language.
    fn words(&self) -> &'static ShellWords {
        super::words::for_scope(self.i18n_scope())
    }
    /// The module's name in the catalogue: its `key@scope` messages override the shell's
    /// (None: the shell's own words).
    fn i18n_scope(&self) -> Option<&'static str> {
        None
    }
    /// The art gallery the showcase menu offers in developer mode.
    fn gallery_screen(&self) -> Option<ScreenId>;
    /// The shell switched to a module screen (from `from`).
    fn screen_opened(&mut self, screen: ScreenId, from: Screen);
    /// Draw a module screen into `body` (the shell scales the management canvas).
    /// `scene`: the shell's world when it is not the module's match (the showcase).
    fn draw_screen(&mut self, ui: &mut egui::Ui, body: Rect, screen: ScreenId, shell: ShellFlags, scene: Option<&RenderWorld>);
    /// Where the header's Back leads from a reference screen (None: an ordinary
    /// screen, Back leads to the map).
    fn back_from(&self, screen: ScreenId) -> Option<Screen>;
    /// Where Esc leads from a module screen (None: the map, or the menu before a match).
    fn escape_to(&self, screen: ScreenId) -> Option<Screen>;
    /// A match is being created: navigation waits.
    fn creating_match(&self) -> bool;
    /// Pointer and touch controls and what they do (the keyboard is the keymap's), for the
    /// Settings screen.
    fn key_help(&self) -> Vec<(&'static str, &'static str)>;
    /// Decode the art the shown screen needs now (offscreen captures).
    fn preload_screen_art(&mut self, ctx: &egui::Context, screen: Screen);

    // --- Input ---------------------------------------------------------------------

    /// The module's keyboard shortcuts: its name in `settings.cfg` and its rows (defaults,
    /// scope, group). The shell lists, remaps and routes them (`shell::keymap`).
    fn shortcuts(&self) -> super::keymap::ShortcutSet {
        super::keymap::ShortcutSet::NONE
    }
    /// A shortcut of the module's rows was pressed (outside text fields; Esc is the shell's).
    /// True when the module did it; false when it cannot now (a unit without that order),
    /// and the key goes on to the next action that has it.
    fn shortcut(&mut self, _id: &str, _shell: ShellFlags) -> bool {
        false
    }
    /// Esc first cancels a pending choice of the module (true when it did).
    fn cancel(&mut self) -> bool;
    fn click(&mut self, tile: Option<u32>, button: MapButton);
    /// A city label was clicked, whoever owns the city (the module checks whose it is).
    fn select_city(&mut self, tile: u32);
    /// The bow of an own city's banner was clicked (`CityBanner::can_strike`):
    /// the city aims its ranged strike, the next map click picks the target.
    fn strike_city(&mut self, _tile: u32) {}

    // --- The match -----------------------------------------------------------------

    /// The observer's map, while a match runs.
    fn view(&self) -> Option<&RenderWorld>;
    /// Poll work that finishes outside the frame (a match being created).
    fn update(&mut self);
    /// Unit and world model books of the running ruleset.
    fn art_books(&self) -> Option<(UnitModelBook, WorldBook)>;
    fn turn(&self) -> u32;
    /// The observer's era (music).
    fn era(&self) -> u8;
    fn outcome(&self) -> Outcome;
    /// The Hall of Fame line of an ended match.
    fn hall_of_fame_line(&self) -> String;
    /// The Hall of Fame the end screen shows.
    fn set_hall_of_fame(&mut self, lines: &[String]);
    /// The match just ended (after its Hall of Fame line was written).
    fn game_ended(&mut self);
    /// The rules module's id: the shell keeps its saves apart under its name
    /// (`saves::ModuleSaves`) and refuses a save another module wrote.
    fn module_id(&self) -> ModuleId;
    /// The saves of the layout before per-module saves name no module: the one module
    /// that wrote them adopts them at start (they move under its name).
    fn adopts_shared_saves(&self) -> bool {
        false
    }
    /// The save of the match; None while other players act (or no match runs).
    fn save_bytes(&self) -> Option<Vec<u8>>;
    /// Replace the match with a save.
    fn load_bytes(&mut self, bytes: &[u8]) -> Result<(), String>;
    /// What the list of saves shows about a save: read from the module's `payload` (the bytes after
    /// the shell's tag) without building the game. The list asks once per file and keeps the
    /// answer. The default knows nothing, and the list shows the file only.
    fn describe_save(&self, payload: &[u8]) -> Result<SaveSummary, SaveProblem> {
        let _ = payload;
        Ok(SaveSummary::default())
    }
    /// The running match in the same terms (the default name of a new save: the nation and the
    /// turn). None: no match runs, or the module says nothing of it.
    fn describe_match(&self) -> Option<SaveSummary> {
        None
    }

    // --- Turns ---------------------------------------------------------------------

    /// The turn's other players still act or their actions still play.
    fn turn_running(&self) -> bool;
    fn begin_end_turn(&mut self);
    /// Where to ask for a repaint when the AI turns have an answer: the host has a worker
    /// thread for them (native targets). Without it, or when a module does not use workers,
    /// the AI turns run in the frame slices of `tick_ai`.
    fn set_ai_wake(&mut self, _wake: Option<crate::ai_round::AiWake>) {}
    /// One frame's slice of the AI turns.
    fn tick_ai(&mut self, slice: std::time::Duration) -> AiProgress;
    /// The AI turns that are left, at once (offscreen runs).
    fn finish_ai_turns(&mut self);

    // --- The map -------------------------------------------------------------------

    fn selected_tile(&self) -> Option<u32>;
    /// The presentation id of the selected unit.
    fn selected_unit_id(&self) -> Option<u64>;
    /// The selected unit has orders (WASD then gives orders, not camera moves).
    fn unit_selected(&self) -> bool;
    fn advance_presentation(&mut self, seconds: f32, settings: PresentationSettings) -> Frame;
    fn presentation_idle(&self) -> bool;
    /// Before the UI of a frame: fit the camera of a world-showing screen. Returns
    /// true when the module moved the camera (the shell restores it on leaving).
    fn fit_camera(&mut self, screen: Screen, fit: CameraFit<'_>) -> bool;

    // --- The HUD -------------------------------------------------------------------

    /// Unit kinds by index, for map badges (also without a match).
    fn unit_kinds(&self) -> Vec<KindLabel>;
    /// Rebuild the data of this frame: the HUD feed (None without a match) and the
    /// shown screen's view.
    fn refresh(&mut self, input: FrameInput) -> Option<HudFeed>;
    /// Before the UI: the map as this frame draws it (screens that project tiles).
    fn prepare_ui(&mut self, map: MapView<'_>, shell: ShellFlags);
    /// The screen area the module's order panel covers (labels hide under it).
    fn hud_reserved(&self, view: &HudView<'_>) -> Option<Rect>;
    fn draw_hud(&mut self, ctx: &egui::Context, layer: HudLayer, view: &HudView<'_>);
    /// The tooltip of the hovered map tile, after all other UI of the frame.
    fn draw_tile_hover(&mut self, ctx: &egui::Context, scene: Option<&RenderWorld>, tile: Option<u32>);
    /// After the UI of a frame: links and keys the module reads from the context.
    fn after_ui(&mut self, ctx: &egui::Context, hovered_tile: Option<u32>, shell: ShellFlags);

    /// The actions queued since the last call.
    fn take_actions(&mut self) -> Vec<ShellAction>;

    // --- The look (new methods go at the end, each with a default) ------------------

    /// The module's look: colour tokens, shapes, frames, fonts and its art folder. The
    /// app installs it once at start (`theme::install`); the default is the shell's own.
    fn theme(&self) -> &'static Theme {
        &Theme::AURELIA
    }
    /// What the module paints on the map this frame (lenses and overlays, in tiles):
    /// the shell places it on the ground under the HUD. None: nothing.
    fn map_paint(&self) -> Option<&MapPaint> {
        None
    }

    // --- HUD layout ------------------------------------------------------------------

    /// The HUD panels of this frame whose size the module knows before it draws them:
    /// the shell lays them out first (`layout`), so they hold their spot from the first
    /// frame and the map's labels and yields keep clear of them. A panel finds its spot
    /// again with `layout::place` as it draws; panels not declared here ask only then.
    fn hud_panels(&self, view: &HudView<'_>) -> Vec<Panel> {
        let _ = view;
        Vec::new()
    }
    /// The size a module screen is laid out on when it is shown full-size on a touch screen
    /// smaller than that (the canvas is scaled to fit the window, down to this size's scale):
    /// a screen that adapts to a short, narrow canvas names a smaller one, so its text stays
    /// readable on a phone held sideways (`window` is the room in points: upright it is narrow and
    /// tall). The default is the design canvas of 1150 x 640.
    fn phone_canvas(&self, _screen: ScreenId, _window: Vec2) -> Vec2 {
        vec2(1150., 640.)
    }

    /// Where the shell's controls stand over the map and whether the interface grows
    /// with the window (`HudFrame`).
    fn hud_frame(&self) -> HudFrame {
        HudFrame::Classic
    }

    /// The module's top-bar status cell was clicked (`HudFeed::status`).
    fn status_clicked(&mut self) {}
    /// A top-bar value with a click target was clicked (`TopBarValue::click`): the module
    /// opens what the id names, the way its shortcut would.
    fn top_bar_clicked(&mut self, _id: &str, _shell: ShellFlags) {}
    /// The module's own yields of a map tile, shown instead of the render world's:
    /// food, production, gold, science, culture, faith. None: the world's yields.
    fn tile_yields(&self, tile: u32) -> Option<[i32; 6]> {
        let _ = tile;
        None
    }
    /// False: the module shows no yield icons on the map at all.
    fn yields_shown(&self) -> bool {
        true
    }
    /// True: the city banners, their strike bows and the unit badges shrink with the camera
    /// (`hud::label_scale`: a hex's width in pixels over 110, kept within 0.6..1) and stand
    /// above the city's model instead of over it. False (the classic module): one size at any zoom,
    /// as before.
    fn labels_follow_zoom(&self) -> bool {
        false
    }

    // --- Saves (new methods go at the end, each with a default) ---------------------

    /// The running match's command journal (`saves::journal`): the shell starts one at
    /// every autosave and writes it beside the save. None: no match runs, or the module
    /// keeps no journal.
    fn command_journal(&mut self) -> Option<&mut dyn CommandJournal> {
        None
    }

    /// The seed the running match was made from; None when the module does not know it (a match
    /// loaded from a save) or keeps none. A bug report states it (`shell::report`).
    fn world_seed(&self) -> Option<u64> {
        None
    }

    // --- Observer mode (new methods go at the end, each with a default) ----------------

    /// The match of AI powers the viewer watches (`observer`): the shell draws a panel
    /// per power from it and paces its rounds. None: no observed match runs, or the
    /// module has no observer mode. A module that returns Some shows no map of its own
    /// (`view` is None), and queues `ShellAction::ObserverStarted` when it starts one.
    fn observed(&mut self) -> Option<&mut dyn super::observer::Observed> {
        None
    }
}
