//! One frame of the game: the world on the continuous visual surface, then the HUD or a
//! full screen, then the actions the UI produced. Shared by the window and the offscreen
//! runs. Everything that names a mechanic is the module's (`ModuleUi`).
mod observer;
mod report_host;
mod touch_host;
pub use touch_host::{TouchHost, TAP_SLOP_DP};

use render::{camera::CameraController, gpu::context::GpuCore, renderer::Renderer, world::state::RenderWorld, FrameFlags};

use super::hover::{hover_allowed, HoverCache, HoverKey};
use super::hud::{self, VisualHudState};
use super::map_feedback::{MapOverlay, ScreenMark};
use super::module::{AiProgress, MapButton, ModuleUi, Outcome, Screen, ShellAction};
use super::pacing::{self, Activity, FrameRequest};
use super::presentation::motion::Sound;
use super::saves::{save_directory, JournalFile, ModuleSaves, SaveError, SaveSlot};
use super::screens::{self, ScreenState};
use super::settings::Settings;

const MAP_ZOOM: f32 = 2.25;
/// Wall time per frame the AI turns may take (R-07 AI §4: 2–4 ms keeps the window
/// responsive; the slice only delays the AI, it never changes a decision).
const AI_FRAME_SLICE: std::time::Duration = std::time::Duration::from_millis(3);
/// Longest frame step the presentation takes (a stalled frame does not skip animations).
const LONGEST_STEP: f32 = 0.1;
/// Seconds of a camera glide to a completed wonder.
const GLIDE_SECONDS: f32 = 0.9;

/// The camera's way to a tile (skippable: any pan or zoom ends it).
struct Glide {
    from: (f32, f32),
    to: (f32, f32),
    t: f32,
}
/// Camera pan of the arrow and WASD keys, logical pixels per second.
const KEY_PAN_SPEED: f32 = 900.0;

pub struct App<U: ModuleUi> {
    pub renderer: Renderer,
    pub camera: CameraController,
    /// The map camera while a world-showing screen moved the camera.
    map_camera: Option<CameraController>,
    pub ctx: egui::Context,
    ui_renderer: egui_wgpu::Renderer,
    /// Ledger tickets of the egui textures the UI renderer holds.
    ui_textures: std::collections::HashMap<egui::TextureId, render::gpu::memory::Allocation>,
    pub hud: VisualHudState,
    pub screens: ScreenState,
    /// The game module's UI and its match.
    pub module: U,
    empty: RenderWorld,
    hover_cache: HoverCache,
    pub dragging: bool,
    /// The page's safe-area insets (left, top, right, bottom) in CSS pixels; None: no notch.
    pub safe_area_css: Option<[f32; 4]>,
    /// The running module's quick save, autosaves and Hall of Fame.
    saves: ModuleSaves,
    /// The command journal of the match since its last autosave (None: no autosave yet).
    journal: Option<JournalFile>,
    /// The end of this game was shown and written to the Hall of Fame.
    ended: bool,
    /// Wall clock of the last frame (presentation time).
    last_frame: Option<web_time::Instant>,
    /// Offscreen runs step the presentation by this fixed time per frame.
    pub presentation_step: Option<f32>,
    /// Sounds of this frame, drained by the window's audio player.
    pub sounds: Vec<Sound>,
    glide: Option<Glide>,
    overlay_time: f32,
    /// Time of the previous frame (egui seconds), for key panning.
    last_frame_time: Option<f64>,
    /// Settings as last applied to the renderer and UI, and the file they persist to
    /// (None: offscreen runs never read or write the player's settings).
    applied: Option<Settings>,
    /// The fallback fonts of the language in force (`theme::language_fonts`); empty while only
    /// the theme's own fonts are installed.
    language_fonts: &'static [&'static str],
    config: Option<std::path::PathBuf>,
    saved: Settings,
    /// Offscreen captures freeze animation at this time whatever the quality preset.
    pub fixed_animation_time: Option<f32>,
    /// The kind of graphics card, for what `Auto` render scale and anti-aliasing choose.
    device_type: wgpu::DeviceType,
    /// `Auto` looks at the graphics card (the game with a settings file, or `AURELIA_AUTO_GRAPHICS=1`);
    /// offscreen runs without one draw the same on every machine: full scale, 4 samples.
    auto_graphics: bool,
    /// Render scale and samples last handed to the renderer.
    scene_quality: Option<super::settings::SceneQuality>,
    /// When egui wants the next frame, from the last frame's output.
    ui_delay: std::time::Duration,
    /// CPU time of the stages of the last frame (the benches' breakdown).
    pub stages: FrameStages,
    /// The observer mode while an observed match runs (`ModuleUi::observed`).
    observer: Option<super::observer::shell::ObserverShell>,
    /// The ground textures and the target format the observer's panel renderers are made with.
    terrain: super::observer::panels::TerrainLayers,
    format: wgpu::TextureFormat,
    /// How the panels' renderers build terrain; offscreen runs set `Immediate`.
    pub observer_terrain_sync: render::gpu::terrain_gpu::TerrainSync,
    /// The clock a new observer starts with.
    pub observer_clock: super::observer::speed::Clock,
    /// The graphics card as a bug report names it (`report_host`), and the frame count that paces its snapshots.
    adapter: report_host::Adapter,
    report_tick: u32,
}

/// CPU milliseconds of the stages of one `App::frame`.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStages {
    /// Input, module update, AI tick and the HUD models.
    pub module_ms: f64,
    /// The renderer: terrain, products, shadows, scene recording and submit.
    pub world_ms: f64,
    /// Running egui (the HUD and screens).
    pub ui_ms: f64,
    /// Tessellating egui, uploading it and recording and submitting its pass.
    pub paint_ms: f64,
}

impl<U: ModuleUi> App<U> {
    /// The game with the player's settings from `config` (None: defaults, not persisted).
    /// `terrain` holds the content's terrain texture layers (width, height, RGBA).
    /// The world and the UI are both drawn into `format`.
    pub fn new(
        core: &GpuCore,
        format: wgpu::TextureFormat,
        config: Option<std::path::PathBuf>,
        module: U,
        terrain: &[(u32, u32, Vec<u8>)],
    ) -> Self {
        Self::with_world_format(core, format, format, config, module, terrain)
    }

    /// `new` for a surface the world is drawn into through another view than the UI is
    /// (`frame_split`): the world in `world_format` (an sRGB view of a plain canvas, see
    /// `gpu::world_format`), egui in `ui_format`, the surface's own.
    pub fn with_world_format(
        core: &GpuCore,
        ui_format: wgpu::TextureFormat,
        world_format: wgpu::TextureFormat,
        config: Option<std::path::PathBuf>,
        module: U,
        terrain: &[(u32, u32, Vec<u8>)],
    ) -> Self {
        let format = world_format;
        let ctx = egui::Context::default();
        super::theme::install(&ctx, module.theme());
        if super::theme::touch_wanted() {
            super::theme::touch_targets(&ctx);
        }
        let mut screens = ScreenState::default();
        screens.game_mode = true;
        screens.screen = Screen::Menu;
        let saves = open_saves(&module, &mut screens);
        let settings = match &config {
            Some(path) => Settings::load(path),
            None => Settings::default(),
        };
        // The player's bundles, then the language they chose (offscreen runs keep English
        // unless `AURELIA_LANG` says otherwise).
        // (A page has no folder of the player's own bundles; its settings are a file of the browser's store.)
        if let Some(dir) = config.as_deref().and_then(std::path::Path::parent).filter(|_| !cfg!(target_arch = "wasm32")) {
            screens.language_notes = super::language::install_bundles(dir);
        }
        // Files that vanish with the page (no IndexedDB): said once, in the menu's status line.
        if super::storage::is_volatile() {
            screens.menu_status = Some(i18n::t!("shell.saves.not-persistent").to_owned());
        }
        i18n::set_scope(module.i18n_scope());
        super::language::apply(&settings.language);
        super::report::install_panic_recorder();
        screens.settings = settings.clone();
        super::fold::load(&ctx, &settings.open_panels);
        // A page keeps its settings in `config` too, but its graphics budget is the page's (`device`): unchanged.
        let auto_graphics = (config.is_some() && !cfg!(target_arch = "wasm32")) || std::env::var("AURELIA_AUTO_GRAPHICS").as_deref() == Ok("1");
        let mut renderer = Renderer::with_gpu(core, format, terrain);
        // The terrain a move reveals is built off the frame and shown when ready; far
        // regions are meshed coarser (AURELIA_TERRAIN_LOD=0 keeps full detail everywhere).
        renderer.set_terrain_sync(render::gpu::terrain_gpu::TerrainSync::Background);
        renderer.set_terrain_lod(std::env::var("AURELIA_TERRAIN_LOD").as_deref() != Ok("0"));
        let mut module = module;
        module.set_hall_of_fame(&saves.hall_of_fame().lines());
        let mut hud = VisualHudState::default();
        hud.unit_kinds = module.unit_kinds();
        Self {
            renderer,
            camera: CameraController::fit_map(1, 1),
            map_camera: None,
            ui_renderer: egui_wgpu::Renderer::new(&core.device, ui_format, Default::default()),
            ui_textures: Default::default(),
            ctx,
            hud,
            screens,
            module,
            empty: RenderWorld::default(),
            hover_cache: HoverCache::default(),
            dragging: false,
            safe_area_css: None,
            saves,
            journal: None,
            ended: false,
            last_frame: None,
            presentation_step: None,
            sounds: Vec::new(),
            glide: None,
            overlay_time: 0.0,
            last_frame_time: None,
            applied: None,
            language_fonts: &[],
            config,
            saved: settings,
            fixed_animation_time: None,
            device_type: core.adapter.get_info().device_type,
            auto_graphics,
            scene_quality: None,
            ui_delay: std::time::Duration::ZERO,
            stages: FrameStages::default(),
            observer: None,
            terrain: std::sync::Arc::new(terrain.to_vec()),
            format,
            observer_terrain_sync: render::gpu::terrain_gpu::TerrainSync::Background,
            observer_clock: super::observer::speed::Clock::default(),
            adapter: report_host::Adapter::of(&core.adapter.get_info()),
            report_tick: 0,
        }
    }

    /// Apply changed settings (UI scale, graphics preset) and persist them
    /// once no pointer button is held (a slider drag writes the file once).
    fn apply_settings(&mut self) {
        // Panels opened or folded in the last frame ride along with the other settings.
        super::fold::export(&self.ctx, &mut self.screens.settings.open_panels);
        let settings = self.screens.settings.clone();
        if self.applied.as_ref().is_none_or(|applied| applied.differs_apart_from_panels(&settings)) {
            let lod_env = std::env::var("AURELIA_TERRAIN_LOD").ok();
            let lod = match lod_env.as_deref() {
                Some("0") => false,
                Some(_) => true,
                None => settings.quality.terrain_lod(),
            };
            self.renderer.set_terrain_lod(lod);
            // Reduced motion stills the water too: nothing on the map moves by itself.
            let animated = settings.quality.animated() && !settings.animation.reduced_motion;
            let frozen = match (self.fixed_animation_time, animated) {
                (Some(time), _) => Some(time),
                (None, true) => None,
                (None, false) => Some(0.0),
            };
            self.renderer.set_animation_time_override(frozen);
            // Geysers, craters, fires and ruins smoke only while the map animates.
            self.renderer.set_ambient_effects(animated);
        }
        if self.applied.as_ref().is_none_or(|applied| applied.language != settings.language) {
            super::language::apply(&settings.language);
        }
        // The glyphs of the language (Chinese, Japanese) come from a fallback font; a browser
        // gets the file a frame or two later.
        let wanted = super::theme::language_fonts(&i18n::language());
        if wanted != self.language_fonts {
            if super::theme::install_fonts(&self.ctx, self.module.theme(), &i18n::language()) {
                self.language_fonts = wanted;
            } else {
                self.ctx.request_repaint();
            }
        }
        self.applied = Some(settings.clone());
        let held = self.ctx.input(|i| i.pointer.any_down());
        if settings != self.saved && !held {
            if let Some(path) = &self.config {
                if let Err(error) = settings.save(path) {
                    self.screens.menu_status = Some(error);
                }
            }
            self.saved = settings;
        }
    }

    /// A write the browser's storage refused after the save call returned (the store is full): the
    /// same words as a save that failed at once.
    fn show_storage_failure(&mut self) {
        if let Some(failure) = super::storage::take_failure() {
            let error = if failure.full { i18n::t!("shell.errors.storage-full").to_owned() } else { failure.message };
            self.screens.menu_status = Some(i18n::tf!("shell.errors.cannot-save", error = error));
        }
    }

    /// Autosave at the end of a round every `autosave_every` turns.
    fn autosave(&mut self) {
        let every = self.screens.settings.autosave_every;
        if every == 0 || self.module.view().is_none() || self.module.turn() % every != 0 {
            return;
        }
        let Some(bytes) = self.module.save_bytes() else {
            return;
        };
        let kept = self.screens.settings.autosaves_kept;
        match self.saves.autosaves().write(&bytes, self.module.turn(), kept) {
            Ok(slot) => {
                self.screens.continue_available = true;
                self.screens.save_available = true;
                self.begin_journal(&slot);
            }
            Err(error) => self.screens.menu_status = Some(error.to_string()),
        }
    }

    /// A new command journal beside the autosave just written (`saves::journal`).
    fn begin_journal(&mut self, save: &SaveSlot) {
        self.journal = None;
        let Some(match_journal) = self.module.command_journal() else {
            return;
        };
        match JournalFile::create(save.path(), &match_journal.begin_journal()) {
            Ok(file) => self.journal = Some(file),
            Err(error) => self.screens.menu_status = Some(error.to_string()),
        }
    }

    /// Append the commands played since the last frame to the journal; a journal that
    /// cannot be written stops (the game goes on).
    fn write_journal(&mut self) {
        let (Some(file), Some(match_journal)) = (&self.journal, self.module.command_journal()) else {
            return;
        };
        if let Err(error) = file.append(&match_journal.take_journal()) {
            self.journal = None;
            self.screens.menu_status = Some(error.to_string());
        }
    }

    pub fn view(&self) -> &RenderWorld {
        self.module.view().unwrap_or(&self.empty)
    }

    /// A new world runs (a new match or a loaded one): fresh models and camera.
    pub fn after_new_world(&mut self) {
        // The journal belongs to the match it started in.
        self.journal = None;
        self.release_observer();
        // Unit and world models, names and roles follow the module's ruleset.
        if let Some((units, world)) = self.module.art_books() {
            self.renderer.set_unit_models(units);
            self.renderer.set_world_models(world);
            self.hud.unit_kinds = self.module.unit_kinds();
        }
        // A loaded game that is over already is not written again.
        self.ended = self.module.outcome() != Outcome::Playing;
        self.camera = map_camera(self.view());
        self.map_camera = None;
        self.hover_cache = HoverCache::default();
        self.forget_old_match();
        self.center_on_selection();
    }

    /// A new match inherits nothing of the one before but the settings and the loaded assets
    /// (decision D330): what the shell kept of it between frames — a camera glide towards a
    /// place of the old map, a drag, the pointer's hover and badges, the selection, the marks
    /// and the route on the ground, the sounds not played yet, the frame in flight — goes. (The renderer's products and the minimap are keyed by the world's own layer
    /// revisions, which no two worlds share: `LayerRevisions::fresh`.)
    fn forget_old_match(&mut self) {
        self.glide = None;
        self.dragging = false;
        self.sounds.clear();
        self.last_frame = None;
        self.last_frame_time = None;
        let hud = &mut self.hud;
        hud.hovered_tile = None;
        hud.hovered_badge = None;
        hud.hover_over_badge = false;
        hud.selected_tile = None;
        hud.feed = None;
        hud.overlay = MapOverlay::default();
        hud.banners.clear();
        hud.badges.clear();
        self.renderer.set_unit_poses(&[]);
    }

    pub fn center_on_selection(&mut self) {
        let Some(tile) = self.module.selected_tile() else {
            return;
        };
        self.center_on(tile);
    }

    pub fn center_on(&mut self, tile: u32) {
        let width = self.view().width;
        center_camera(&mut self.camera, width, tile);
    }

    pub fn pick(&self, cursor: (f32, f32), size: (f32, f32)) -> Option<u32> {
        let tile = self.renderer.pick_tile(self.view(), &self.camera, cursor, size)?;
        Some(tile.0)
    }

    /// Map click: orders and selection go to the module (which may open a screen).
    pub fn click(&mut self, tile: Option<u32>, button: MapButton) {
        if matches!(button, MapButton::Primary) && self.observer_click() {
            return;
        }
        if self.screens.navigation_is_open() || self.module.view().is_none() {
            return;
        }
        self.module.click(tile, button);
        self.apply_module_actions();
    }

    pub fn frame(
        &mut self,
        core: &GpuCore,
        target: &wgpu::TextureView,
        size: (u32, u32),
        raw: egui::RawInput,
    ) -> egui::PlatformOutput {
        self.frame_split(core, target, target, size, raw)
    }

    /// `frame` with the world drawn into `world` and the UI into `target`: two views of one
    /// surface texture when the surface is a plain canvas (`with_world_format`).
    pub fn frame_split(
        &mut self,
        core: &GpuCore,
        world: &wgpu::TextureView,
        target: &wgpu::TextureView,
        size: (u32, u32),
        raw: egui::RawInput,
    ) -> egui::PlatformOutput {
        let started = web_time::Instant::now();
        self.remember_report();
        self.show_storage_failure();
        self.apply_settings();
        self.handle_keys(&raw);
        self.pan_with_keys(&raw);
        self.module.update();
        self.apply_module_actions();
        if self.observer_shown() {
            return self.observer_frame(core, target, size, raw);
        }
        self.tick_ai();
        self.refresh_models();
        let mut raw = raw;
        self.fit_ui_scale(size, &mut raw);
        self.apply_safe_area(&mut raw);
        let screen = raw.screen_rect.unwrap_or_else(|| {
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32) / self.ctx.pixels_per_point())
        });
        self.sync_screen_camera(screen);
        self.present();
        let map_visible = self.map_visible();
        let world_started = web_time::Instant::now();
        if map_visible {
            self.draw_world(core, world, size);
        }
        let ui_started = web_time::Instant::now();
        let output = self.run_ui(raw, size);
        self.ui_delay = repaint_delay(&self.ctx, &output);
        let paint_started = web_time::Instant::now();
        let platform = self.paint_ui(core, target, size, output, map_visible);
        let ms = |from: web_time::Instant, to: web_time::Instant| to.duration_since(from).as_secs_f64() * 1000.0;
        self.stages = FrameStages {
            module_ms: ms(started, world_started),
            world_ms: ms(world_started, ui_started),
            ui_ms: ms(ui_started, paint_started),
            paint_ms: paint_started.elapsed().as_secs_f64() * 1000.0,
        };
        platform
    }

    /// The interface scale: the player's setting times the frame's growth with the window
    /// (`HudFrame::ui_scale`). The frame's input was measured in the old scale's points:
    /// a change rescales its screen rect to the new ones.
    fn fit_ui_scale(&mut self, size: (u32, u32), raw: &mut egui::RawInput) {
        // Offscreen captures have no monitor: `AURELIA_VISUAL_NATIVE_SCALE=2.75` plays one (a phone's
        // density), so the layout at 1080x2400 pixels is the one a 393x873 dp screen gets.
        let (native, simulated) = match self.ctx.native_pixels_per_point() {
            Some(native) => (native, 1.0),
            None => (1.0, super::capture::simulated_native_scale()),
        };
        let window = egui::vec2(size.0 as f32, size.1 as f32) / (native * simulated);
        let zoom = self.screens.settings.ui_scale * self.module.hud_frame().ui_scale(window) * simulated;
        let before = self.ctx.zoom_factor();
        if (before - zoom).abs() < 1e-4 {
            return;
        }
        self.ctx.set_zoom_factor(zoom);
        if let Some(rect) = raw.screen_rect.as_mut() {
            *rect = egui::Rect::from_min_size(rect.min, rect.size() * before / zoom);
        }
    }

    /// The notch and the system bars the page reported (CSS pixels, one per native point) as
    /// egui's safe area, in the points of the scaled interface.
    fn apply_safe_area(&self, raw: &mut egui::RawInput) {
        let Some([left, top, right, bottom]) = self.safe_area_css else { return };
        let zoom = self.ctx.zoom_factor().max(0.01);
        raw.safe_area_insets = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
            left: left / zoom,
            top: top / zoom,
            right: right / zoom,
            bottom: bottom / zoom,
        }));
    }

    fn map_visible(&self) -> bool {
        screens::wants_world(self.screens.screen, &self.module) && self.module.view().is_some()
    }

    /// When the window draws again after the frame just drawn (`pacing`).
    pub fn next_frame(&self, now: web_time::Instant) -> FrameRequest {
        pacing::next_frame(&self.activity(), now)
    }

    /// What may still change on screen after the last frame.
    fn activity(&self) -> Activity {
        let in_game = self.module.view().is_some();
        let world_shown = self.map_visible();
        let overlay = &self.hud.overlay;
        let pulse = overlay.selection.is_some() || !overlay.route.is_empty();
        let settings = &self.screens.settings;
        Activity {
            turn_running: (in_game && self.module.turn_running()) || self.observer_wants_frames(),
            presentation: in_game && !self.module.presentation_idle(),
            glide: self.glide.is_some(),
            renderer_work: world_shown && self.renderer.has_pending_work(),
            ambient: pacing::ambient(world_shown, settings.animation.reduced_motion, settings.quality.animated(), pulse),
            ui_delay: self.ui_delay,
        }
    }

    /// One frame of the presentation: unit poses to the renderer, marks and the selection
    /// to the HUD, sounds to the audio player, a wonder to the camera.
    fn present(&mut self) {
        let seconds = self.frame_seconds();
        self.overlay_time += seconds;
        let settings = self.screens.presentation();
        if self.module.view().is_none() {
            self.renderer.set_unit_poses(&[]);
            self.hud.overlay = MapOverlay::default();
            return;
        }
        let frame = self.module.advance_presentation(seconds, settings);
        self.renderer.set_unit_poses(&frame.poses);
        // The 3D effects follow the frame's seconds, not the (pausable) water clock.
        let quality = self.screens.settings.quality;
        let effects = if settings.reduced_motion || !quality.animated() {
            render::gpu::effects::EffectsMode::Reduced
        } else {
            render::gpu::effects::EffectsMode::Full
        };
        self.renderer.effects().set_mode(effects);
        self.renderer.advance_effects(seconds);
        for event in &frame.fx {
            self.renderer.play_fx(event);
        }
        self.sounds.extend(frame.sounds.iter().copied());
        if let Some(tile) = frame.camera {
            self.glide_to(tile.0);
        }
        self.advance_glide(seconds);
        self.hud.overlay = self.overlay(&frame);
        if frame.busy || self.glide.is_some() {
            self.ctx.request_repaint();
        }
    }

    fn frame_seconds(&mut self) -> f32 {
        let now = web_time::Instant::now();
        let wall = self.last_frame.map_or(0.0, |last| now.duration_since(last).as_secs_f32());
        self.last_frame = Some(now);
        self.presentation_step.unwrap_or(wall).clamp(0.0, LONGEST_STEP)
    }

    /// Feet on the displayed ground at world XZ.
    fn feet(&self, x: f32, z: f32) -> [f32; 3] {
        [x, self.renderer.ground_height(x, z).unwrap_or(0.4), z]
    }

    fn tile_feet(&self, tile: u32) -> [f32; 3] {
        let width = self.view().width.max(1);
        let (x, z) = render::scene::layout::tile_to_world(tile % width, tile / width);
        self.feet(x, z)
    }

    /// Marks with the ground under them.
    fn screen_marks(&self, frame: &super::presentation::Frame) -> Vec<ScreenMark> {
        let mut marks = Vec::with_capacity(frame.marks.len());
        for live in &frame.marks {
            let at = self.feet(live.mark.at[0], live.mark.at[1]);
            marks.push(ScreenMark { mark: live.mark.clone(), progress: live.progress, ground: at[1] });
        }
        marks
    }

    fn overlay(&self, frame: &super::presentation::Frame) -> MapOverlay {
        let posed: Vec<(u64, [f32; 3])> = frame
            .poses
            .iter()
            .filter(|pose| !matches!(pose.gait, render::gpu::unit_poses::Gait::Fall { .. }))
            .map(|pose| (pose.id, self.feet(pose.position[0], pose.position[1])))
            .collect();
        let selected = self.module.selected_unit_id();
        let moving = selected.and_then(|id| posed.iter().find(|(p, _)| *p == id).map(|(_, at)| *at));
        let resting = selected.and(self.module.selected_tile());
        let route = self.hud.feed.as_ref().and_then(|feed| feed.route.as_ref());
        MapOverlay {
            marks: self.screen_marks(frame),
            selection: moving.or_else(|| resting.map(|tile| self.tile_feet(tile))),
            route: route.map_or(Vec::new(), |r| r.tiles.iter().map(|t| self.tile_feet(*t)).collect()),
            posed,
            time: self.overlay_time,
            reduced_motion: self.screens.presentation().reduced_motion,
            observer: self.view().observer,
        }
    }

    fn glide_to(&mut self, tile: u32) {
        if self.screens.screen != Screen::Map {
            return;
        }
        let width = self.view().width.max(1);
        let to = render::scene::layout::tile_to_world(tile % width, tile / width);
        self.glide = Some(Glide { from: (self.camera.x, self.camera.y), to, t: 0.0 });
    }

    fn advance_glide(&mut self, seconds: f32) {
        let Some(glide) = &mut self.glide else {
            return;
        };
        glide.t = (glide.t + seconds / GLIDE_SECONDS).min(1.0);
        let u = glide.t * glide.t * (3.0 - 2.0 * glide.t);
        self.camera.x = glide.from.0 + (glide.to.0 - glide.from.0) * u;
        self.camera.y = glide.from.1 + (glide.to.1 - glide.from.1) * u;
        if glide.t >= 1.0 {
            self.glide = None;
        }
    }

    /// The player moved the camera: a running glide gives way.
    pub fn cancel_glide(&mut self) {
        self.glide = None;
    }

    /// CPU time of the UI part of a frame — view models, egui layout and
    /// painting, tessellation — without the world or any GPU work (`--bench-ui`).
    pub fn ui_frame(&mut self, raw: egui::RawInput, size: (u32, u32)) -> (std::time::Duration, usize) {
        let start = web_time::Instant::now();
        let mut raw = raw;
        self.fit_ui_scale(size, &mut raw);
        self.refresh_models();
        let mut output = self.run_ui(raw, size);
        // Nothing is painted: the texture changes are dropped on purpose.
        output.textures_delta.clear();
        let jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let elapsed = start.elapsed();
        let vertices = jobs
            .iter()
            .map(|job| match &job.primitive {
                egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                egui::epaint::Primitive::Callback(_) => 0,
            })
            .sum();
        (elapsed, vertices)
    }

    /// Hands the renderer the render scale and samples of the settings for a window of
    /// `size` pixels (`Auto` follows the card and the size, so a resize may change it).
    /// `AURELIA_RENDER_SCALE` and `AURELIA_MSAA` in the environment win over the settings.
    fn apply_scene_quality(&mut self, size: (u32, u32)) {
        let phone = super::device::phone_graphics();
        let constrained = phone || (self.auto_graphics && super::settings::is_constrained(self.device_type));
        let target = if phone { super::device::PHONE_SCENE_PIXELS } else { super::settings::AUTO_SCENE_PIXELS };
        let quality = self.screens.settings.scene_quality_for(constrained, size, target);
        if self.scene_quality == Some(quality) {
            return;
        }
        if std::env::var_os("AURELIA_RENDER_SCALE").is_none() {
            self.renderer.set_render_scale(quality.scale);
        }
        if std::env::var_os("AURELIA_MSAA").is_none() {
            self.renderer.set_msaa_samples(quality.samples);
        }
        self.scene_quality = Some(quality);
    }

    fn draw_world(&mut self, core: &GpuCore, target: &wgpu::TextureView, size: (u32, u32)) {
        let flags = FrameFlags {
            grid: self.hud.show_grid,
            // The legacy flat city outline does not fit the visual surface; the selected
            // city's label carries the highlight instead.
            selected_city: None,
            yields: false,
        };
        self.apply_scene_quality(size);
        let world = self.module.view().unwrap_or(&self.empty);
        self.renderer.draw(core, world, &self.camera, target, size, flags);
    }

    /// HUD and screen data from the module.
    fn refresh_models(&mut self) {
        let input = super::module::FrameInput {
            hovered_tile: self.hud.hovered_tile,
            hovered_badge: self.hud.hovered_badge,
            shell: self.screens.flags(),
        };
        self.hud.feed = self.module.refresh(input);
        if self.module.view().is_none() {
            return;
        }
        self.hud.selected_tile = self.module.selected_tile();
        self.screens.has_session = true;
        let ended = self.module.outcome() != Outcome::Playing;
        if ended && !self.ended {
            self.record_game_end();
        }
        self.ended = ended;
    }

    /// The game just ended: one Hall of Fame line, then the module's results.
    fn record_game_end(&mut self) {
        let line = self.module.hall_of_fame_line();
        let hall_of_fame = self.saves.hall_of_fame();
        match hall_of_fame.append(&line) {
            Ok(()) => self.module.set_hall_of_fame(&hall_of_fame.lines()),
            Err(error) => self.screens.menu_status = Some(error),
        }
        self.module.game_ended();
        self.apply_module_actions();
    }

    fn handle_keys(&mut self, raw: &egui::RawInput) {
        // The observer has no text fields: a widget holding the focus (Tab gave it one) does not
        // keep the keys from the viewer.
        let observing = self.observer_shown();
        if (!observing && self.ctx.egui_wants_keyboard_input()) || self.module.creating_match() {
            return;
        }
        // Keys that an input method is composing are text, not shortcuts (`keymap::pressed`).
        for (key, modifiers) in super::keymap::pressed(&raw.events) {
            self.key_press(key, &modifiers);
        }
    }

    pub fn key(&mut self, key: egui::Key) {
        self.key_press(key, &egui::Modifiers::NONE);
    }

    /// A key, with Shift held or not.
    pub fn key_with(&mut self, key: egui::Key, shift: bool) {
        self.key_press(key, &egui::Modifiers { shift, ..egui::Modifiers::NONE });
    }

    /// A key press with its modifiers: the one door of the keyboard (the shortcuts of
    /// `keymap`, the remapping of the Settings screen, Esc).
    pub fn key_press(&mut self, key: egui::Key, modifiers: &egui::Modifiers) {
        if self.observer_shown() {
            return self.observer_key(key, modifiers.shift);
        }
        if self.screens.saves.is_open() && !self.screens.report.is_open() {
            // The saves dialogs are modal: every key is theirs.
            if key == egui::Key::Escape {
                self.screens.saves.cancel();
            } else if let Some(action) = self.screens.saves.key(key) {
                self.apply(action);
            }
            self.ctx.request_repaint();
            return;
        }
        if key == egui::Key::Escape {
            if !(self.screens.report.cancel() || self.screens.shortcuts.cancel() || self.module.cancel() || self.screens.close_navigation()) {
                self.escape();
            }
            return;
        }
        let Some(chord) = super::keymap::Chord::of_event(key, modifiers) else { return };
        if self.screens.shortcuts.listening() {
            self.screens.capture_key(&self.module, chord);
            return;
        }
        match self.screens.route_key(&mut self.module, chord) {
            super::shortcuts::Routed::Shell(action) => self.apply(action),
            super::shortcuts::Routed::Module => self.apply_module_actions(),
            super::shortcuts::Routed::Used | super::shortcuts::Routed::None => {}
        }
    }

    /// Esc: back from a screen, or the menu over the map.
    fn escape(&mut self) {
        let in_game = self.module.view().is_some() || self.observer.is_some();
        let next = match self.screens.screen {
            Screen::Map => Screen::Menu,
            Screen::Module(id) => match self.module.escape_to(id) {
                Some(screen) => screen,
                None if in_game => Screen::Map,
                None => Screen::Menu,
            },
            _ if in_game => Screen::Map,
            _ => Screen::Menu,
        };
        self.screens.open(next);
    }

    /// Arrow keys (and WASD without a selected unit) move the camera while held.
    fn pan_with_keys(&mut self, raw: &egui::RawInput) {
        let now = raw.time.unwrap_or(0.0);
        let dt = self.last_frame_time.map(|t| (now - t).clamp(0.0, 0.05)).unwrap_or(0.0) as f32;
        self.last_frame_time = Some(now);
        let typing = self.ctx.egui_wants_keyboard_input();
        if typing || self.screens.screen != Screen::Map || self.module.view().is_none() {
            return;
        }
        let unit_selected = self.module.unit_selected();
        let keymap = self.screens.keymap(&self.module);
        let (dx, dy) = self.ctx.input(|i| keymap.pan(|k| i.key_down(k) && !i.modifiers.command && !i.modifiers.alt, unit_selected));
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let step = KEY_PAN_SPEED * dt.max(1.0 / 60.0) * self.ctx.pixels_per_point();
        // `pan_by` follows a drag: the map moves with the pointer, so the camera goes the other way.
        self.cancel_glide();
        self.camera.pan_by(-dx * step, -dy * step);
        self.ctx.request_repaint();
    }

    fn run_ui(&mut self, raw: egui::RawInput, size: (u32, u32)) -> egui::FullOutput {
        let ctx = self.ctx.clone();
        let mut actions = hud::VisualHudActions::default();
        let output = ctx.run_ui(raw, |ui| {
            // Explicit offscreen QA route: inspect the actual runtime atlas crops.
            if let Ok(sheet) = std::env::var("AURELIA_GAME_ATLAS_REVIEW") {
                self.screens.draw_atlas_review(ui, &sheet);
                return;
            }
            actions = self.draw_ui(ui.ctx());
            self.update_hover(ui.ctx(), size);
            self.module.draw_tile_hover(ui.ctx(), None, self.hud.hovered_tile);
        });
        self.apply_actions(actions);
        output
    }

    fn draw_ui(&mut self, ctx: &egui::Context) -> hud::VisualHudActions {
        let surface = self.renderer.visual_surface();
        let parts = ShellUi { screens: &mut self.screens, hud: &mut self.hud, camera: &self.camera, surface };
        draw_shell_ui(ctx, parts, &mut self.module)
    }

    fn update_hover(&mut self, ctx: &egui::Context, size: (u32, u32)) {
        let allowed = hover_allowed(ctx, self.screens.screen, self.dragging, self.hud.hover_over_badge);
        let world = self.module.view().unwrap_or(&self.empty);
        let key = match ctx.pointer_hover_pos() {
            Some(pointer) if allowed => Some(HoverKey::new(pointer, ctx.pixels_per_point(), size, &self.camera, world)),
            _ => None,
        };
        let renderer = &self.renderer;
        let camera = &self.camera;
        self.hud.hovered_tile = self.hover_cache.resolve(key, |key| {
            let screen = (size.0 as f32, size.1 as f32);
            let tile = renderer.pick_tile(world, camera, key.physical_pointer(), screen)?;
            Some(tile.0)
        });
    }

    fn apply_actions(&mut self, actions: hud::VisualHudActions) {
        if actions.toggle_grid {
            self.hud.show_grid = !self.hud.show_grid;
        }
        if let Some((x, z)) = actions.camera_target {
            self.camera.x = x;
            self.camera.y = z;
        }
        if let Some(tile) = actions.select_city {
            self.module.select_city(tile);
        }
        if let Some(tile) = actions.strike_city {
            self.module.strike_city(tile);
        }
        if let Some(screen) = actions.open {
            self.screens.open(screen);
        }
        if actions.status_clicked {
            self.module.status_clicked();
        }
        if let Some(id) = actions.top_clicked {
            self.module.top_bar_clicked(id, self.screens.flags());
        }
        if actions.end_turn {
            self.begin_end_turn();
        }
        let flags = self.screens.flags();
        self.module.after_ui(&self.ctx, self.hud.hovered_tile, flags);
        let requests = std::mem::take(&mut self.screens.requests);
        for request in requests {
            self.apply(request);
        }
        self.apply_module_actions();
    }

    /// Carry out what the module queued (offscreen runs call it after driving the module).
    pub fn apply_module_actions(&mut self) {
        for action in self.module.take_actions() {
            self.apply(action);
        }
    }

    /// Carry out what a module or the menu asked for.
    pub fn apply(&mut self, action: ShellAction) {
        match action {
            ShellAction::Open(screen) => {
                self.screens.open(screen);
                self.ctx.request_repaint();
            }
            ShellAction::CenterOn(tile) => self.center_on(tile),
            ShellAction::CenterOnSelection => {
                self.cancel_glide();
                self.center_on_selection();
            }
            ShellAction::Save => self.save(),
            ShellAction::Load => self.load(),
            ShellAction::Continue => self.continue_latest(),
            ShellAction::LoadDialog => self.open_load_list(),
            ShellAction::LoadSave(path) => self.load_listed(path),
            ShellAction::DeleteSave(path) => self.delete_listed(&path),
            ShellAction::SaveDialog => self.open_save_dialog(),
            ShellAction::SaveAs(name) => self.save_named(&name),
            ShellAction::MatchStarted { summary } => {
                self.after_new_world();
                self.screens.match_ready(summary);
            }
            ShellAction::ToggleGrid => self.hud.show_grid = !self.hud.show_grid,
            ShellAction::ToggleYields => self.hud.show_yields = !self.hud.show_yields,
            ShellAction::Zoom(factor) => {
                self.cancel_glide();
                self.camera.zoom_by(factor);
            }
            ShellAction::EndTurn => self.begin_end_turn(),
            ShellAction::ObserverStarted { summary } => {
                self.observer_started();
                self.screens.match_ready(summary);
            }
            ShellAction::ReportBug => self.open_report(),
            ShellAction::ReportSave => self.save_report(),
        }
    }

    /// The AI turns of the module play on a worker thread, not in frame slices on this one
    /// (native hosts; the worker repaints the window when it has an answer). The sliced
    /// path stays for the browser build and for the offscreen runs, which never call this.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_ai_worker(&mut self) {
        let ctx = self.ctx.clone();
        self.module.set_ai_wake(Some(std::sync::Arc::new(move || ctx.request_repaint())));
    }

    fn begin_end_turn(&mut self) {
        if self.module.view().is_none() {
            return;
        }
        // The AI turns run in frame slices from here (`tick_ai`), or on the worker.
        self.module.begin_end_turn();
        self.ctx.request_repaint();
    }

    /// One frame's slice of the AI turns; the camera follows the selection after them.
    fn tick_ai(&mut self) {
        if self.module.view().is_none() {
            return;
        }
        let progress = self.module.tick_ai(AI_FRAME_SLICE);
        self.write_journal();
        match progress {
            AiProgress::Busy => self.ctx.request_repaint(),
            AiProgress::Finished => {
                self.center_on_selection();
                self.autosave();
            }
            AiProgress::Idle => {}
        }
    }

    /// Run the AI turns that are left at once (offscreen runs).
    pub fn finish_ai_turns(&mut self) {
        self.module.finish_ai_turns();
        self.center_on_selection();
    }

    fn save(&mut self) {
        if self.observer.is_some() {
            self.screens.menu_status = Some(self.module.words().observer.no_save.into());
            return;
        }
        if self.module.view().is_none() {
            return;
        }
        let Some(bytes) = self.module.save_bytes() else {
            self.screens.menu_status = Some(self.module.words().saves.wait_for_turns.into());
            return;
        };
        let quick_save = self.saves.quick_save();
        let words = &self.module.words().saves;
        let status = match quick_save.write(&bytes) {
            Ok(()) => i18n::tf!("shell.saves.saved", turn = self.module.turn()),
            Err(error) => error.describe(words),
        };
        self.refresh_save_flags();
        self.screens.menu_status = Some(status);
    }

    /// The menu learns whether there is a save to load or continue.
    fn refresh_save_flags(&mut self) {
        let any = self.saves.latest().is_some();
        self.screens.save_available = any;
        self.screens.continue_available = any;
    }

    /// The menu's "Load game": list every save.
    fn open_load_list(&mut self) {
        self.screens.saves.open_list(self.saves.entries());
        self.ctx.request_repaint();
    }

    /// A pick of the list: load it; the list closes with the load, and keeps the reason when it fails.
    fn load_listed(&mut self, path: std::path::PathBuf) {
        let slot = SaveSlot::at(path, self.module.module_id());
        if self.load_from(&slot) {
            self.screens.saves.close();
        } else if let Some(reason) = self.screens.menu_status.clone() {
            self.screens.saves.set_notice(reason, true);
        }
    }

    /// The list's Delete after its confirmation: the save goes, the list is read again.
    fn delete_listed(&mut self, path: &std::path::Path) {
        match self.saves.delete(path) {
            Ok(()) => {
                self.screens.saves.reload(self.saves.entries());
                self.screens.saves.set_notice(i18n::t!("shell.saves.deleted").to_owned(), false);
            }
            Err(error) => self.screens.saves.set_notice(error.describe(&self.module.words().saves), true),
        }
        self.refresh_save_flags();
        self.ctx.request_repaint();
    }

    /// The menu's "Save game": ask for a name, "<nation> turn N" to begin with.
    fn open_save_dialog(&mut self) {
        if self.observer.is_some() {
            self.screens.menu_status = Some(self.module.words().observer.no_save.into());
            return;
        }
        if self.module.view().is_none() {
            return;
        }
        if self.module.save_bytes().is_none() {
            self.screens.menu_status = Some(self.module.words().saves.wait_for_turns.into());
            return;
        }
        let name = super::saves_screen::default_name(self.module.describe_match().as_ref(), self.module.turn());
        self.screens.saves.open_name(name, self.saves.manual_names());
        self.ctx.request_repaint();
    }

    /// The naming dialog's Save: the match goes to `manual/<name>.aurelia`.
    fn save_named(&mut self, name: &str) {
        let Some(slot) = self.saves.manual_slot(name) else { return };
        let Some(bytes) = self.module.save_bytes() else {
            self.screens.saves.set_notice(self.module.words().saves.wait_for_turns.into(), true);
            return;
        };
        match slot.write(&bytes) {
            Ok(()) => {
                self.screens.saves.close();
                self.screens.menu_status = Some(i18n::tf!("shell.saves.saved", turn = self.module.turn()));
                self.refresh_save_flags();
            }
            Err(error) => self.screens.saves.set_notice(error.describe(&self.module.words().saves), true),
        }
    }

    /// Continue: the newest of the autosaves and the quick save.
    fn continue_latest(&mut self) {
        match self.saves.latest() {
            Some(slot) => {
                self.load_from(&slot);
            }
            None => self.screens.menu_status = Some(self.module.words().saves.nothing_saved.into()),
        }
    }

    fn load(&mut self) {
        self.load_from(&self.saves.quick_save());
    }

    /// Replace the match with the save in `slot`; false (and the reason in the menu's status)
    /// when it cannot be loaded.
    fn load_from(&mut self, slot: &SaveSlot) -> bool {
        let loaded = slot
            .read()
            .and_then(|bytes| self.module.load_bytes(&bytes).map_err(SaveError::Rejected));
        let words = &self.module.words().saves;
        match loaded {
            Ok(()) => {
                self.after_new_world();
                self.screens.menu_status = Some(i18n::tf!("shell.saves.loaded", turn = self.module.turn()));
                self.screens.match_ready(words.loaded_world.into());
                true
            }
            Err(error) => {
                self.screens.menu_status = Some(error.describe(words));
                false
            }
        }
    }

    /// A world-showing screen fits the camera around its panels; leaving it restores
    /// the map camera.
    fn sync_screen_camera(&mut self, screen: egui::Rect) {
        let shown = self.screens.screen;
        let module_world = matches!(shown, Screen::Module(_)) && screens::wants_world(shown, &self.module);
        if !module_world {
            if let Some(camera) = self.map_camera.take() {
                self.camera = camera;
            }
            return;
        }
        let saved = self.camera.clone();
        let fit = super::module::CameraFit {
            camera: &mut self.camera,
            surface: self.renderer.visual_surface(),
            screen,
        };
        if self.module.fit_camera(shown, fit) && self.map_camera.is_none() {
            self.map_camera = Some(saved);
        }
    }

    fn paint_ui(
        &mut self,
        core: &GpuCore,
        target: &wgpu::TextureView,
        size: (u32, u32),
        mut output: egui::FullOutput,
        map_visible: bool,
    ) -> egui::PlatformOutput {
        // The texture changes are drained, not borrowed: egui checks in debug builds that
        // none is dropped unapplied.
        for (id, deltas) in output.textures_delta.set.drain() {
            for delta in &deltas {
                self.ui_renderer.update_texture(&core.device, &core.queue, id, delta);
            }
            self.count_ui_texture(core, id);
        }
        let jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [size.0, size.1], pixels_per_point: output.pixels_per_point };
        let mut encoder = core.device.create_command_encoder(&Default::default());
        let callbacks = self.ui_renderer.update_buffers(&core.device, &core.queue, &mut encoder, &jobs, &screen);
        let load = if map_visible { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) };
        self.ui_pass(&mut encoder, core, target, load, &jobs, &screen);
        if let Some(timing) = &core.timing {
            timing.resolve(&mut encoder);
        }
        core.queue.submit(callbacks.into_iter().chain(std::iter::once(encoder.finish())));
        for id in output.textures_delta.free.drain() {
            self.ui_renderer.free_texture(&id);
            self.ui_textures.remove(&id);
        }
        output.platform_output
    }

    /// Keeps the ledger ticket of an egui texture at the size of the texture it has now
    /// (a whole-image update may have recreated it).
    fn count_ui_texture(&mut self, core: &GpuCore, id: egui::TextureId) {
        use render::gpu::memory::{texture_bytes_of, Category};
        let Some(texture) = self.ui_renderer.texture(&id).and_then(|t| t.texture.as_ref()) else {
            return;
        };
        let bytes = texture_bytes_of(texture);
        if self.ui_textures.get(&id).map(|a| a.bytes()) != Some(bytes) {
            self.ui_textures.insert(id, core.memory.allocate(Category::Ui, bytes));
        }
    }

    fn ui_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        core: &GpuCore,
        target: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
        jobs: &[egui::ClippedPrimitive],
        screen: &egui_wgpu::ScreenDescriptor,
    ) {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("game UI"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: core.timing.as_ref().map(|t| t.writes(render::gpu::timing::Stage::Ui)),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.ui_renderer.render(&mut pass.forget_lifetime(), jobs, screen);
    }
}

/// The camera of a new world: the whole map fitted, at the map's zoom.
pub(super) fn map_camera(world: &RenderWorld) -> CameraController {
    let mut camera = CameraController::fit_map(world.width, world.height);
    camera.zoom = MAP_ZOOM;
    camera
}

/// Centre `camera` on `tile` of a map `width` tiles wide.
pub(super) fn center_camera(camera: &mut CameraController, width: u32, tile: u32) {
    let width = width.max(1);
    let (x, z) = render::scene::layout::tile_to_world(tile % width, tile / width);
    camera.x = x;
    camera.y = z;
}

/// The shell's state one frame of UI draws with (the app's, or a snapshot stage's).
pub(super) struct ShellUi<'a> {
    pub screens: &'a mut ScreenState,
    pub hud: &'a mut VisualHudState,
    pub camera: &'a CameraController,
    pub surface: Option<&'a render::scene::terrain::surface::SurfaceMesh>,
}

/// The UI of a frame: the shown full screen, else the HUD over the map with the Empire
/// navigation. The window, the offscreen runs and the text snapshots draw through it.
pub(super) fn draw_shell_ui(ctx: &egui::Context, shell: ShellUi<'_>, module: &mut dyn ModuleUi) -> hud::VisualHudActions {
    let map = super::module::MapView { camera: shell.camera, surface: shell.surface, screen: ctx.content_rect() };
    module.prepare_ui(map, shell.screens.flags());
    let keymap = shell.screens.keymap(&*module);
    super::keymap::publish(ctx, &keymap);
    if screens::draw(ctx, shell.screens, module, None) {
        super::shortcuts::overlay(ctx, shell.screens, &*module);
        super::report::overlay(ctx, shell.screens);
        super::saves_screen::overlay(ctx, shell.screens, &*module);
        return hud::VisualHudActions::default();
    }
    let actions = hud::draw(ctx, None, shell.camera, shell.hud, shell.surface, module);
    if shell.hud.show_settings {
        shell.hud.show_settings = false;
        shell.screens.open(Screen::Settings);
    }
    screens::draw_navigation(ctx, shell.screens, module);
    super::shortcuts::overlay(ctx, shell.screens, &*module);
    super::report::overlay(ctx, shell.screens);
    super::saves_screen::overlay(ctx, shell.screens, &*module);
    actions
}

/// The running module's saves, after it adopted the shared layout's (`ModuleSaves`); the
/// menu learns whether there is a game to load or continue.
fn open_saves<U: ModuleUi>(module: &U, screens: &mut ScreenState) -> ModuleSaves {
    let saves = ModuleSaves::of(module.module_id());
    if module.adopts_shared_saves() {
        if let Err(error) = saves.adopt_shared_saves(&save_directory()) {
            screens.menu_status = Some(error.to_string());
        }
    }
    let any = saves.latest().is_some();
    screens.save_available = any;
    screens.continue_available = any;
    saves
}

/// When egui wants its next frame: zero is now, `Duration::MAX` is never.
fn repaint_delay(ctx: &egui::Context, output: &egui::FullOutput) -> std::time::Duration {
    match output.viewport_output.get(&ctx.viewport_id()) {
        Some(viewport) => viewport.repaint_delay,
        None => std::time::Duration::MAX,
    }
}
