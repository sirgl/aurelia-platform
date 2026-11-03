//! The winit event loop of the Android build: GPU, surface and shell state, the window
//! lifecycle (the surface is lost on every pause and made again on resume), touch and the
//! Back key. The frame itself is the shell's (`client::shell::app::App`).
use std::sync::Arc;
use std::time::Instant;

use client::shell::module::{ModuleUi, Screen};
use client::shell::pacing::FrameRequest;
use client::shell::settings;
use client::shell::touch::{Gesture, Phase, Touches};
use crate::audio::{MusicMode, MusicPlayer};
use render::gpu::context::GpuCore;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::platform::android::activity::AndroidApp;
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::window::{Window, WindowId};

use super::platform::Paths;
use crate::Layers;


/// Travel of a finger (density-independent pixels) below which a touch is still a tap.
const TAP_SLOP_DP: f32 = 10.0;

/// egui asked for a frame from another thread (a world or artwork finished loading).
struct RepaintRequested;

/// Everything that outlives a pause: the GPU, the shell with the match, the music.
struct Game<U: ModuleUi> {
    core: GpuCore,
    app: client::shell::app::App<U>,
    music: MusicPlayer,
    music_mode: MusicMode,
}

/// What exists only while the activity has a window: the window and its surface.
struct Display {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    input: ui_input::State,
    touches: Touches,
}

struct Host<U: ModuleUi> {
    android: AndroidApp,
    module: fn() -> (U, Layers),
    proxy: EventLoopProxy<RepaintRequested>,
    game: Option<Game<U>>,
    display: Option<Display>,
    /// The next frame of ambient animation or of a delayed UI repaint.
    wake_at: Option<Instant>,
    failure: Option<String>,
    /// `AURELIA_TOUCH_LOG=1`: one log line per finger event and gesture (adb logcat -s aurelia).
    touch_log: bool,
}

pub fn run<U: ModuleUi + 'static>(app: AndroidApp, paths: Paths, module: fn() -> (U, Layers)) {
    log::info!("Aurelia starts, assets in {}", paths.assets.display());
    start(app, module)
}

fn start<U: ModuleUi + 'static>(android: AndroidApp, module: fn() -> (U, Layers)) {
    let event_loop = match EventLoop::with_user_event()
        .with_android_app(android.clone())
        .build()
    {
        Ok(event_loop) => event_loop,
        Err(error) => return log::error!("no event loop ({error})"),
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut host = Host {
        android,
        module,
        proxy: event_loop.create_proxy(),
        game: None,
        display: None,
        wake_at: None,
        failure: None,
        touch_log: std::env::var("AURELIA_TOUCH_LOG").is_ok_and(|v| v == "1"),
    };
    if let Err(error) = event_loop.run_app(&mut host) {
        log::error!("Aurelia stopped: {error}");
    }
    if let Some(failure) = host.failure {
        log::error!("Aurelia cannot start: {failure}");
    }
}

impl<U: ModuleUi + 'static> Host<U> {
    /// The surface for `window` on the GPU of `core`.
    fn display(
        core: &GpuCore,
        window: Arc<Window>,
        ctx: &egui::Context,
    ) -> Result<(Display, wgpu::TextureFormat), String> {
        let surface = core
            .instance
            .create_surface(window.clone())
            .map_err(|e| format!("The window cannot be drawn to ({e})"))?;
        let caps = surface.get_capabilities(&core.adapter);
        let format = client::shell::gpu::surface_format(&caps)?;
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&core.device, &config);
        let scale = window.scale_factor() as f32;
        log::info!(
            "surface {}x{} px, scale factor {scale}, {format:?}",
            config.width,
            config.height
        );
        let input = ui_input::State::new(ctx.clone(), scale);
        let touches = new_touches(scale);
        Ok((
            Display {
                window,
                surface,
                config,
                input,
                touches,
            },
            format,
        ))
    }

    /// The first window: the GPU, the shell and the match are made once.
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let window = event_loop
            .create_window(Window::default_attributes().with_title("Aurelia"))
            .map_err(|e| format!("The game window cannot be opened ({e})"))?;
        let window = Arc::new(window);
        if let Some(game) = &self.game {
            // Back from a pause: the same game on a new surface.
            let (display, _) = Self::display(&game.core, window, &game.app.ctx)?;
            display.window.request_redraw();
            self.display = Some(display);
            return Ok(());
        }
        let core = client::shell::gpu::open()?;
        log::info!("GPU: {:?}", core.adapter.get_info());
        // The egui context belongs to the shell, which needs the surface format first.
        let probe = core
            .instance
            .create_surface(window.clone())
            .map_err(|e| format!("The window cannot be drawn to ({e})"))?;
        let format = client::shell::gpu::surface_format(&probe.get_capabilities(&core.adapter))?;
        drop(probe);
        let (module, terrain) = (self.module)();
        let app = client::shell::app::App::new(
            &core,
            format,
            Some(settings::config_path()),
            module,
            &terrain,
        );
        let proxy = self.proxy.clone();
        app.ctx.set_request_repaint_callback(move |info| {
            if info.delay.is_zero() {
                let _ = proxy.send_event(RepaintRequested);
            }
        });
        let (display, _) = Self::display(&core, window, &app.ctx)?;
        display.window.request_redraw();
        self.game = Some(Game {
            core,
            app,
            music: MusicPlayer::new(),
            music_mode: MusicMode::Menu,
        });
        self.display = Some(display);
        Ok(())
    }

    /// The system bars and the display cutout, in points, for egui's `safe_area_insets`.
    fn safe_area(&self, display: &Display) -> egui::SafeAreaInsets {
        let rect = self.android.content_rect();
        let size = display.window.inner_size();
        let ppp = display.window.scale_factor() as f32;
        let inset = |px: i32| (px.max(0) as f32) / ppp;
        egui::SafeAreaInsets(egui::epaint::MarginF32 {
            left: inset(rect.left),
            top: inset(rect.top),
            right: inset(size.width as i32 - rect.right),
            bottom: inset(size.height as i32 - rect.bottom),
        })
    }

    fn draw(&mut self) {
        let insets = self.display.as_ref().map(|d| self.safe_area(d));
        let (Some(game), Some(display)) = (self.game.as_mut(), self.display.as_mut()) else {
            return;
        };
        let frame = match display.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                display
                    .surface
                    .configure(&game.core.device, &display.config);
                display.window.request_redraw();
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let mut raw = display.input.take_egui_input(&display.window);
        if let Some(insets) = insets.filter(|i| i.0 != egui::epaint::MarginF32::default()) {
            raw.safe_area_insets = Some(insets);
        }
        let size = (display.config.width, display.config.height);
        let output = game.app.frame(&game.core, &view, size, raw);
        display
            .input
            .handle_platform_output(&display.window, output);
        sync_music(game);
        game.core.queue.present(frame);
    }

    fn schedule_next_frame(&mut self) {
        self.wake_at = None;
        let (Some(game), Some(display)) = (self.game.as_ref(), self.display.as_ref()) else {
            return;
        };
        match game.app.next_frame(Instant::now()) {
            FrameRequest::Now => display.window.request_redraw(),
            FrameRequest::At(at) => self.wake_at = Some(at),
            FrameRequest::Idle => {}
        }
    }

    /// Applies what the touch state machine decided to the shell.
    fn apply(&mut self, gestures: Vec<Gesture>) {
        let (Some(game), Some(display)) = (self.game.as_mut(), self.display.as_mut()) else {
            return;
        };
        let touch_log = self.touch_log;
        let app = &mut game.app;
        let on_map = app.screens.screen == Screen::Map && !app.screens.navigation_is_open();
        for gesture in gestures {
            if touch_log {
                log::info!("gesture {gesture:?} (on map {on_map})");
            }
            match gesture {
                Gesture::Pan { dx, dy } if on_map => {
                    app.cancel_glide();
                    app.pan_view(dx, dy);
                }
                Gesture::Zoom { factor, .. } if on_map => {
                    app.cancel_glide();
                    app.zoom_view(factor);
                }
                Gesture::Tap { at, button } if on_map => {
                    let size = (display.config.width as f32, display.config.height as f32);
                    app.note_pointer(at);
                    let tile = app.pick(at, size);
                    log::info!("tap {button:?} at {at:?} picks tile {tile:?}");
                    app.click(tile, button);
                }
                _ => {}
            }
        }
        app.dragging = display.touches.dragging();
        app.hud.hovered_tile = None;
        display.window.request_redraw();
    }
}

/// Whether the last frame had an egui panel (not the background) under the physical pixel `at`.
fn over_egui(ctx: &egui::Context, at: (f32, f32)) -> bool {
    let ppp = ctx.pixels_per_point();
    let point = egui::pos2(at.0 / ppp, at.1 / ppp);
    ctx.layer_id_at(point)
        .is_some_and(|layer| layer.order != egui::Order::Background)
}

/// The touch state machine for a screen of `scale` pixels per dp. `AURELIA_LONG_PRESS_MS` changes
/// the long-press time (a test driver such as `adb shell input` is slow between events).
fn new_touches(scale: f32) -> Touches {
    let touches = Touches::new(TAP_SLOP_DP * scale);
    match std::env::var("AURELIA_LONG_PRESS_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        Some(ms) => touches.with_long_press(std::time::Duration::from_millis(ms)),
        None => touches,
    }
}

fn sync_music<U: ModuleUi>(game: &mut Game<U>) {
    let era = game.app.module.era();
    game.music_mode = game.app.screens.music(game.music_mode, &game.app.module);
    let screens = &mut game.app.screens;
    game.music.set_volume(screens.music_volume());
    game.music.set_muted(screens.music_muted());
    game.music.set_effects_volume(screens.effects_volume());
    game.music.set_effects_muted(screens.effects_muted());
    game.music.set_era(era);
    game.music.set_mode(game.music_mode);
    for sound in game.app.sounds.drain(..) {
        game.music.play(sound);
    }
    game.music.update();
    screens.set_music_error(game.music.unavailable_reason().map(str::to_owned));
}

impl<U: ModuleUi + 'static> ApplicationHandler<RepaintRequested> for Host<U> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.display.is_some() {
            return;
        }
        if let Err(error) = self.open(event_loop) {
            self.failure = Some(error);
            event_loop.exit();
        }
    }

    fn suspended(&mut self, _: &ActiveEventLoop) {
        // The native window is gone after this call: the surface must go first.
        self.display = None;
        self.wake_at = None;
        if let Some(game) = self.game.as_mut() {
            game.music.set_muted(true);
            game.music.set_effects_muted(true);
            game.music.update();
        }
        log::info!("suspended: surface dropped");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(display) = self.display.as_mut() else {
            return;
        };
        let response = display.input.on_window_event(&display.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    display.config.width = size.width;
                    display.config.height = size.height;
                    if let Some(game) = &self.game {
                        display
                            .surface
                            .configure(&game.core.device, &display.config);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.draw();
                if self
                    .game
                    .as_ref()
                    .is_some_and(|g| g.app.screens.quit_requested)
                {
                    return event_loop.exit();
                }
                return self.schedule_next_frame();
            }
            WindowEvent::Touch(touch) => {
                let at = (touch.location.x as f32, touch.location.y as f32);
                let phase = match touch.phase {
                    TouchPhase::Started => Phase::Started,
                    TouchPhase::Moved => Phase::Moved,
                    TouchPhase::Ended => Phase::Ended,
                    TouchPhase::Cancelled => Phase::Cancelled,
                };
                // A finger that starts on a panel belongs to egui: it is never tracked here.
                // egui learns the finger's position only in the next frame, so its `consumed` is
                // stale for a first touch: the panels of the last frame are asked directly.
                let over_panel = phase == Phase::Started
                    && self
                        .game
                        .as_ref()
                        .is_some_and(|g| over_egui(&g.app.ctx, at));
                let free = !(phase == Phase::Started && (response.consumed || over_panel));
                if self.touch_log {
                    log::info!(
                        "touch {phase:?} id {} at {at:?}: egui consumed {}, over panel {over_panel}, layer {:?}",
                        touch.id,
                        response.consumed,
                        self.game.as_ref().and_then(|g| {
                            let ppp = g.app.ctx.pixels_per_point();
                            g.app.ctx.layer_id_at(egui::pos2(at.0 / ppp, at.1 / ppp))
                        })
                    );
                }
                let on_map = self.game.as_ref().is_some_and(|g| {
                    g.app.screens.screen == Screen::Map && !g.app.screens.navigation_is_open()
                });
                let gestures = match free && on_map {
                    true => display.touches.event(touch.id, phase, at, Instant::now()),
                    false => Vec::new(),
                };
                self.apply(gestures);
                return;
            }
            WindowEvent::KeyboardInput { event: key, .. }
                if key.state == ElementState::Pressed
                    && key.logical_key == Key::Named(NamedKey::BrowserBack) =>
            {
                // The Back button is Escape: close the open panel or screen, else the pause menu.
                if let Some(game) = self.game.as_mut() {
                    game.app.key(egui::Key::Escape);
                }
                display.window.request_redraw();
            }
            WindowEvent::Focused(false) => {
                display.touches = new_touches(display.window.scale_factor() as f32);
                if let Some(game) = self.game.as_mut() {
                    game.app.dragging = false;
                }
            }
            _ => {}
        }
        if response.repaint {
            display.window.request_redraw();
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, _: RepaintRequested) {
        if let Some(d) = &self.display {
            d.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // A finger held long enough makes the secondary click: the loop wakes for it.
        let now = Instant::now();
        let gestures = self
            .display
            .as_mut()
            .map(|d| d.touches.tick(now))
            .unwrap_or_default();
        if !gestures.is_empty() {
            self.apply(gestures);
        }
        let touch_deadline = self.display.as_ref().and_then(|d| d.touches.deadline());
        let wake = match (self.wake_at, touch_deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        match wake {
            Some(at) if at <= now => {
                self.wake_at = None;
                if let Some(d) = &self.display {
                    d.window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}
