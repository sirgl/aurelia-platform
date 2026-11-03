//! The desktop launcher: the winit window, the GPU surface, input, music and the frame loop around
//! the shell of `client` and ONE game module the launcher registers. Nothing here knows a rule set;
//! a launcher (`lands/launcher/desktop`, `classic/launcher/desktop`) calls [`main`] with its module,
//! its product name and its developer commands.
use std::sync::Arc;
use std::time::Instant;

use client::shell::module::{MapButton, ModuleUi, Screen};
use client::shell::pacing::FrameRequest;
use client::shell::saves;
use client::shell::settings::{self, Settings, WindowMode};
use render::gpu::context::GpuCore;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::audio::{MusicMode, MusicPlayer};
use crate::Layers;

/// Pointer travel (physical pixels) below which a press-release is a click, not a drag.
const CLICK_SLOP: f64 = 5.0;

/// egui asked for a frame from another thread (a world or artwork finished loading).
struct RepaintRequested;

struct Desktop<U: ModuleUi> {
    window: Arc<Window>,
    core: GpuCore,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    app: client::shell::app::App<U>,
    input: ui_input::State,
    cursor: (f64, f64),
    pressed: Option<(MouseButton, (f64, f64))>,
    music: MusicPlayer,
    music_mode: MusicMode,
    /// Window mode and size as last applied from the settings.
    window_applied: (WindowMode, (u32, u32)),
    /// The next frame of ambient animation or of a delayed UI repaint.
    wake_at: Option<Instant>,
}

impl<U: ModuleUi + 'static> Desktop<U> {
    /// The game in `window`, or why it cannot draw there (no GPU, no surface).
    fn new(
        window: Arc<Window>,
        window_applied: (WindowMode, (u32, u32)),
        proxy: EventLoopProxy<RepaintRequested>,
        (module, terrain): (U, Layers),
    ) -> Result<Self, String> {
        let core = client::shell::gpu::open()?;
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
        let mut app = client::shell::app::App::new(&core, format, Some(settings::config_path()), module, &terrain);
        // The AI players' turns play on a worker thread: the window stays at its frame rate.
        app.enable_ai_worker();
        // A repaint asked from a worker thread must wake the event loop.
        app.ctx.set_request_repaint_callback(move |info| {
            if info.delay.is_zero() {
                let _ = proxy.send_event(RepaintRequested);
            }
        });
        let input = ui_input::State::new(app.ctx.clone(), window.scale_factor() as f32);
        Ok(Self {
            window,
            core,
            surface,
            config,
            app,
            input,
            cursor: (0., 0.),
            pressed: None,
            music: MusicPlayer::new(),
            music_mode: MusicMode::Menu,
            window_applied,
            wake_at: None,
        })
    }

    /// After a frame: draw again now, at a time, or only on input (`desktop::pacing`).
    fn schedule_next_frame(&mut self) {
        self.wake_at = None;
        match self.app.next_frame(Instant::now()) {
            FrameRequest::Now => self.window.request_redraw(),
            FrameRequest::At(at) => self.wake_at = Some(at),
            FrameRequest::Idle => {}
        }
    }

    /// Before the event loop sleeps: draw when the time has come, else sleep until it.
    fn control_flow(&mut self) -> ControlFlow {
        match self.wake_at {
            Some(at) if at <= Instant::now() => {
                self.wake_at = None;
                self.window.request_redraw();
                ControlFlow::Wait
            }
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        }
    }

    /// Window mode and size from the settings screen, when they changed.
    fn sync_window(&mut self) {
        let s = &self.app.screens.settings;
        let wanted = (s.window, s.resolution);
        if wanted == self.window_applied {
            return;
        }
        match s.window {
            WindowMode::Fullscreen => self.window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None))),
            WindowMode::Windowed => {
                self.window.set_fullscreen(None);
                let (w, h) = s.resolution;
                let _ = self.window.request_inner_size(winit::dpi::LogicalSize::new(w as f64, h as f64));
            }
        }
        self.window_applied = wanted;
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.core.device, &self.config);
        }
    }

    fn draw(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                return self.resize(self.config.width, self.config.height);
            }
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let raw = self.input.take_egui_input(&self.window);
        let size = (self.config.width, self.config.height);
        let output = self.app.frame(&self.core, &view, size, raw);
        self.input.handle_platform_output(&self.window, output);
        self.sync_music();
        self.sync_window();
        self.core.queue.present(frame);
    }

    fn sync_music(&mut self) {
        let era = self.app.module.era();
        self.music_mode = self.app.screens.music(self.music_mode, &self.app.module);
        let screens = &mut self.app.screens;
        self.music.set_volume(screens.music_volume());
        self.music.set_muted(screens.music_muted());
        self.music.set_effects_volume(screens.effects_volume());
        self.music.set_effects_muted(screens.effects_muted());
        self.music.set_era(era);
        self.music.set_mode(self.music_mode);
        for sound in self.app.sounds.drain(..) {
            self.music.play(sound);
        }
        self.music.update();
        screens.set_music_error(self.music.unavailable_reason().map(str::to_owned));
    }

    fn pick(&self) -> Option<u32> {
        let cursor = (self.cursor.0 as f32, self.cursor.1 as f32);
        let size = (self.config.width as f32, self.config.height as f32);
        self.app.pick(cursor, size)
    }

    fn mouse(&mut self, state: ElementState, button: MouseButton, consumed: bool) {
        let on_map = self.app.screens.screen == Screen::Map && !self.app.screens.navigation_is_open();
        if state == ElementState::Pressed {
            if on_map && !consumed {
                self.pressed = Some((button, self.cursor));
                self.app.dragging = button == MouseButton::Left;
            }
            return;
        }
        let Some((pressed, at)) = self.pressed.take() else {
            return;
        };
        self.app.dragging = false;
        let moved = (self.cursor.0 - at.0).abs() + (self.cursor.1 - at.1).abs();
        if !on_map || consumed || pressed != button || moved >= CLICK_SLOP {
            return;
        }
        let target = match button {
            MouseButton::Left => MapButton::Primary,
            MouseButton::Right => MapButton::Secondary,
            _ => return,
        };
        self.app.note_pointer((self.cursor.0 as f32, self.cursor.1 as f32));
        let tile = self.pick();
        self.app.click(tile, target);
    }

    fn cursor_moved(&mut self, x: f64, y: f64, consumed: bool) {
        let dragging = matches!(self.pressed, Some((MouseButton::Left, _)));
        if dragging && !consumed && self.app.screens.screen == Screen::Map && !self.app.screens.navigation_is_open() {
            let (dx, dy) = ((x - self.cursor.0) as f32, (y - self.cursor.1) as f32);
            self.app.cancel_glide();
            self.app.pan_view(dx, dy);
        }
        self.cursor = (x, y);
    }
}

struct App<U: ModuleUi> {
    desktop: Option<Desktop<U>>,
    /// The product name in the window title.
    title: &'static str,
    /// The module and its terrain layers, made when the window opens.
    module: fn() -> (U, Layers),
    /// Why the game could not start (printed when the loop ends).
    failure: Option<String>,
    proxy: EventLoopProxy<RepaintRequested>,
}

/// Window attributes from the player's settings.
fn window_attributes(settings: &Settings, title: &str) -> winit::window::WindowAttributes {
    let (w, h) = settings.resolution;
    let attributes = Window::default_attributes()
        .with_title(title)
        .with_inner_size(winit::dpi::LogicalSize::new(w as f64, h as f64));
    match settings.window {
        WindowMode::Fullscreen => attributes.with_fullscreen(Some(winit::window::Fullscreen::Borderless(None))),
        WindowMode::Windowed => attributes,
    }
}

impl<U: ModuleUi + 'static> ApplicationHandler<RepaintRequested> for App<U> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.desktop.is_some() {
            return;
        }
        let settings = Settings::load(&settings::config_path());
        let applied = (settings.window, settings.resolution);
        let started = event_loop
            .create_window(window_attributes(&settings, self.title))
            .map_err(|e| format!("The game window cannot be opened ({e})"))
            .and_then(|window| Desktop::new(Arc::new(window), applied, self.proxy.clone(), (self.module)()));
        match started {
            Ok(desktop) => {
                desktop.window.request_redraw();
                self.desktop = Some(desktop);
            }
            Err(error) => {
                self.failure = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(d) = self.desktop.as_mut() else {
            return;
        };
        let response = d.input.on_window_event(&d.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => d.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                d.draw();
                if d.app.screens.quit_requested {
                    return event_loop.exit();
                }
                // the input glue answers `repaint` to RedrawRequested too: asking for the next
                // frame here (not below) is what lets the window rest.
                return d.schedule_next_frame();
            }
            WindowEvent::CursorLeft { .. } | WindowEvent::Focused(false) => {
                d.pressed = None;
                d.app.dragging = false;
                d.app.hud.hovered_tile = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                d.cursor_moved(position.x, position.y, response.consumed)
            }
            WindowEvent::MouseInput { state, button, .. } => d.mouse(state, button, response.consumed),
            WindowEvent::MouseWheel { delta, .. }
                if d.app.screens.screen == Screen::Map && !d.app.screens.navigation_is_open() && !response.consumed =>
            {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 80.,
                };
                d.app.cancel_glide();
                d.app.zoom_view(1.0 + lines * 0.1);
            }
            _ => {}
        }
        if response.repaint {
            d.window.request_redraw();
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, _: RepaintRequested) {
        if let Some(d) = &self.desktop {
            d.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(d) = self.desktop.as_mut() {
            event_loop.set_control_flow(d.control_flow());
        }
    }
}


/// What a launcher registers: its product, its module and its command-line commands.
pub struct Product<U: ModuleUi + 'static> {
    /// The window title, e.g. "Aurelia: Lands & Laws".
    pub title: &'static str,
    /// The module and its terrain layers, made when the window opens.
    pub module: fn() -> (U, Layers),
    /// Developer commands (`--screenshot`, `--bench*`, ...): runs the one `flag` names and returns
    /// true, or returns false when it names none. `args` is the whole command line.
    pub dev: fn(flag: &str, args: &[String]) -> bool,
    /// Replays a command journal of the module that wrote `save` (`--replay`), see [`crate::replay`].
    pub replay: fn(module: &str, save: &[u8], journal: &[u8]) -> Result<String, String>,
}

/// The whole `main` of a launcher: a developer command, `--replay`, or the game window.
pub fn main<U: ModuleUi + 'static>(product: Product<U>) {
    let args: Vec<String> = std::env::args().collect();
    let headless = args.get(1).is_some_and(|a| a.starts_with("--"));
    if args.get(1).is_some_and(|flag| (product.dev)(flag, &args)) {
        // A developer command ran.
    } else {
        match args.get(1).map(String::as_str) {
            Some("--replay") => replay(&args[2..], product.replay),
            _ => run(product.title, product.module),
        }
    }
    if headless {
        println!("memory at exit: {}", client::perf::MemoryLine);
    }
}

/// What a build without developer tools does with a developer flag: refuses it, so a typo in a script
/// fails loudly. Returns false for any other flag.
pub fn refuse_developer_flag(flag: &str) -> bool {
    let developer = flag == "--screenshot" || flag.starts_with("--gallery") || flag.starts_with("--bench");
    if developer {
        eprintln!("{flag}: this build has no developer tools (cargo build --features dev-tools; it is the default of a developer build)");
        std::process::exit(2);
    }
    false
}

/// `--replay <journal> [<save>]`: play a command journal from its save without a window
/// (the save defaults to the autosave beside the journal) and print what was replayed or
/// where it diverged; the exit code is 1 when it did not replay to its end.
fn replay(args: &[String], replay_module: fn(&str, &[u8], &[u8]) -> Result<String, String>) {
    let Some(journal_path) = args.first().map(std::path::PathBuf::from) else {
        eprintln!("usage: desktop --replay <journal.aurlog> [<save.aurelia>]");
        std::process::exit(2);
    };
    let save_path = args.get(1).map(std::path::PathBuf::from).unwrap_or_else(|| journal_path.with_extension("aurelia"));
    let replayed = std::fs::read(&journal_path)
        .map_err(|e| format!("Cannot read {}: {e}", journal_path.display()))
        .and_then(|journal| {
            let (module, save) = saves::read_tagged(&save_path).map_err(|e| format!("{}: {e}", save_path.display()))?;
            replay_module(&module, &save, &journal)
        });
    match replayed {
        Ok(report) => println!("{}: {report}", journal_path.display()),
        Err(error) => {
            eprintln!("{}: {error}", journal_path.display());
            std::process::exit(1);
        }
    }
}

/// The game window with `module` until it closes.
pub fn run<U: ModuleUi + 'static>(title: &'static str, module: fn() -> (U, Layers)) {
    crate::log_tail::install();
    let event_loop = match EventLoop::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("Aurelia cannot start: no window system ({error})");
            std::process::exit(1);
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App { desktop: None, title, module, failure: None, proxy: event_loop.create_proxy() };
    if let Err(error) = event_loop.run_app(&mut app) {
        eprintln!("Aurelia stopped: {error}");
    }
    if let Some(failure) = app.failure {
        eprintln!("Aurelia cannot start: {failure}");
        std::process::exit(1);
    }
}
