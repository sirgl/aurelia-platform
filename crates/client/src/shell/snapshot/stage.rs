//! `Stage`: the shell's frame over a module without a window, a GPU, saves or art. It
//! draws what the app draws (`app::draw_shell_ui`: the shown full screen, else the HUD
//! and the Empire navigation) and carries out the module's screen and camera actions;
//! saving, loading and the presentation are the app's and do not run here.
use render::camera::CameraController;

use super::super::app::{center_camera, draw_shell_ui, map_camera, ShellUi};
use super::super::hud::{VisualHudActions, VisualHudState};
use super::super::module::{FrameInput, ModuleUi, Screen, ShellAction};
use super::super::keymap::Chord;
use super::super::screens::ScreenState;
use super::super::shortcuts::Routed;

/// Frames drawn before a snapshot: egui sizes new areas in a first pass, and a screen
/// opened by the previous frame shows from the next.
const SETTLE_FRAMES: u32 = 3;

pub struct Stage<U: ModuleUi> {
    pub ctx: egui::Context,
    pub module: U,
    pub screens: ScreenState,
    pub hud: VisualHudState,
    pub camera: CameraController,
    /// The window's size in points.
    size: egui::Vec2,
    frame: u32,
}

impl<U: ModuleUi> Stage<U> {
    /// The module in the game's shell at `size` points, on the main menu like the app
    /// starts, with the module's theme and no art loaded.
    pub fn new(mut module: U, size: egui::Vec2) -> Self {
        let ctx = egui::Context::default();
        super::super::theme::install(&ctx, module.theme());
        super::super::art::stay_offline(&ctx);
        ctx.enable_accesskit();
        let mut screens = ScreenState::default();
        screens.game_mode = true;
        screens.dev_mode = false;
        screens.screen = Screen::Menu;
        module.set_hall_of_fame(&[]);
        let hud = VisualHudState { unit_kinds: module.unit_kinds(), ..Default::default() };
        Self { ctx, module, screens, hud, camera: CameraController::fit_map(1, 1), size, frame: 0 }
    }

    pub fn open(&mut self, screen: Screen) {
        self.screens.open(screen);
    }

    /// Centre the map on `tile`.
    pub fn center_on(&mut self, tile: u32) {
        let width = self.module.view().map_or(1, |world| world.width);
        center_camera(&mut self.camera, width, tile);
    }

    /// One frame with `events` (pointer and text input, in points; keys reach egui's
    /// widgets, not the app's hotkeys); returns its output.
    pub fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.module.update();
        self.apply_module_actions();
        self.refresh();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            time: Some(f64::from(self.frame) / 60.0),
            focused: true,
            events,
            ..Default::default()
        };
        self.frame += 1;
        let mut actions = VisualHudActions::default();
        let ctx = self.ctx.clone();
        let mut output = ctx.run_ui(raw, |ui| {
            let shell = ShellUi { screens: &mut self.screens, hud: &mut self.hud, camera: &self.camera, surface: None };
            actions = draw_shell_ui(ui.ctx(), shell, &mut self.module);
            self.module.draw_tile_hover(ui.ctx(), None, None);
        });
        // Nothing is painted: the texture changes are dropped on purpose.
        output.textures_delta.clear();
        self.apply(actions);
        output
    }

    /// A key press, like the app's: `chord` is `B`, `Ctrl+B`, `F6`... Esc closes the shortcuts
    /// overlay or ends a remapping. What the shell does itself (camera, grid, saves) is
    /// carried out as far as a stage can (saves and loads do nothing); returns what the
    /// press did.
    pub fn press(&mut self, chord: &str) -> Routed {
        let chord = Chord::parse(chord).unwrap_or_else(|| panic!("not a chord: {chord}"));
        let modifiers = egui::Modifiers { ctrl: chord.ctrl, command: chord.ctrl, shift: chord.shift, alt: chord.alt, ..Default::default() };
        self.press_key(chord.key, &modifiers)
    }

    /// A key press with the modifiers the window reported (`press` for the common case).
    pub fn press_key(&mut self, key: egui::Key, modifiers: &egui::Modifiers) -> Routed {
        // The app presses keys between frames: the module has refreshed its panels (the
        // orders of the selected unit) by then.
        self.frame(Vec::new());
        if key == egui::Key::Escape {
            let used = self.screens.shortcuts.cancel() || self.module.cancel() || self.screens.close_navigation();
            return if used { Routed::Used } else { Routed::None };
        }
        let Some(chord) = Chord::of_event(key, modifiers) else { return Routed::None };
        if self.screens.shortcuts.listening() {
            self.screens.capture_key(&self.module, chord);
            return Routed::Used;
        }
        let routed = self.screens.route_key(&mut self.module, chord);
        match &routed {
            Routed::Shell(action) => self.carry_out(action.clone()),
            Routed::Module => self.apply_module_actions(),
            Routed::Used | Routed::None => {}
        }
        routed
    }

    /// A primary click at `at` (points): press and release in two frames.
    pub fn click(&mut self, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![egui::Event::PointerMoved(at), button(true)]);
        self.frame(vec![button(false), egui::Event::PointerGone]);
    }

    /// The text of the shown screen after it settled (`describe`), under a header line
    /// that names the screen and the size.
    pub fn snapshot(&mut self) -> String {
        for _ in 1..SETTLE_FRAMES {
            self.frame(Vec::new());
        }
        // Only the art the last frame asks for is listed.
        super::super::art::take_offline_requests(&self.ctx);
        let output = self.frame(Vec::new());
        let header = format!("screen {} {}x{}\n", self.screen_name(), self.size.x, self.size.y);
        header + &super::describe(&self.ctx, &output)
    }

    fn screen_name(&self) -> String {
        match self.screens.screen {
            Screen::Module(id) => self.module.screen_spec(id).title.to_owned(),
            screen => format!("{screen:?}"),
        }
    }

    /// The HUD feed and the screens' data of this frame (the app's `refresh_models`).
    fn refresh(&mut self) {
        let input = FrameInput { hovered_tile: None, hovered_badge: None, shell: self.screens.flags() };
        self.hud.feed = self.module.refresh(input);
        if self.module.view().is_some() {
            self.hud.selected_tile = self.module.selected_tile();
            self.screens.has_session = true;
        }
    }

    fn apply(&mut self, actions: VisualHudActions) {
        if let Some(screen) = actions.open {
            self.screens.open(screen);
        }
        if let Some(tile) = actions.select_city {
            self.module.select_city(tile);
        }
        if let Some(tile) = actions.strike_city {
            self.module.strike_city(tile);
        }
        if actions.status_clicked {
            self.module.status_clicked();
        }
        if let Some(id) = actions.top_clicked {
            self.module.top_bar_clicked(id, self.screens.flags());
        }
        if actions.end_turn {
            self.end_turn();
        }
        self.module.after_ui(&self.ctx, None, self.screens.flags());
        for request in std::mem::take(&mut self.screens.requests) {
            self.carry_out(request);
        }
        self.apply_module_actions();
    }

    /// Carry out what the module queued (tests call it after driving the module).
    pub fn apply_module_actions(&mut self) {
        for action in self.module.take_actions() {
            self.carry_out(action);
        }
    }

    /// The screen and camera actions; saves and loads do nothing on a stage.
    fn carry_out(&mut self, action: ShellAction) {
        match action {
            ShellAction::Open(screen) => self.screens.open(screen),
            ShellAction::CenterOn(tile) => self.center_on(tile),
            ShellAction::CenterOnSelection => self.center_on_selection(),
            ShellAction::MatchStarted { summary } => {
                self.after_new_world();
                self.screens.match_ready(summary);
            }
            ShellAction::ToggleGrid => self.hud.show_grid = !self.hud.show_grid,
            ShellAction::ToggleYields => self.hud.show_yields = !self.hud.show_yields,
            ShellAction::Zoom(factor) => self.camera.zoom_by(factor),
            ShellAction::EndTurn => self.end_turn(),
            ShellAction::ObserverStarted { summary } => self.screens.match_ready(summary),
            ShellAction::Save
            | ShellAction::Load
            | ShellAction::Continue
            | ShellAction::LoadDialog
            | ShellAction::LoadSave(_)
            | ShellAction::DeleteSave(_)
            | ShellAction::SaveDialog
            | ShellAction::SaveAs(_)
            | ShellAction::ReportBug
            | ShellAction::ReportSave => {}
        }
    }

    /// The whole turn of the other players at once.
    fn end_turn(&mut self) {
        if self.module.view().is_some() {
            self.module.begin_end_turn();
            self.module.finish_ai_turns();
            self.center_on_selection();
        }
    }

    fn after_new_world(&mut self) {
        self.hud.unit_kinds = self.module.unit_kinds();
        if let Some(world) = self.module.view() {
            self.camera = map_camera(world);
        }
        self.center_on_selection();
    }

    fn center_on_selection(&mut self) {
        if let Some(tile) = self.module.selected_tile() {
            self.center_on(tile);
        }
    }
}
