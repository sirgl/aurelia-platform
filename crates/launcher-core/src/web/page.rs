//! The game on a canvas. The frame is the shell's (`client::shell::app::App`); this file
//! wires winit's web events to it, as `platform/crates/launcher-core/src/desktop.rs` does for a window.
use std::cell::{Cell, RefCell};
use std::sync::Arc;

use client::shell::app::TouchHost;
use client::shell::module::{MapButton, ModuleUi, Screen};
use client::shell::pacing::FrameRequest;
use client::shell::touch::Phase;
use render::gpu::context::GpuCore;
use wasm_bindgen::prelude::*;
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::web::{EventLoopExtWebSys, WindowAttributesExtWebSys};
use winit::window::{Window, WindowId};

/// Pointer travel (physical pixels) below which a press-release is a click, not a drag.
const CLICK_SLOP: f64 = 5.0;

/// egui asked for a frame (a picture finished decoding, a file arrived).
struct RepaintRequested;

use crate::Layers;

thread_local! {
    /// What the page's script pushes between frames (keys and text of the on-screen keyboard).
    static PENDING: RefCell<Vec<egui::Event>> = const { RefCell::new(Vec::new()) };
    /// The notch and system bars, CSS pixels: left, top, right, bottom.
    static SAFE_AREA: Cell<Option<[f32; 4]>> = const { Cell::new(None) };
    /// A text field of the game holds the focus: the page shows the on-screen keyboard.
    static WANTS_KEYBOARD: Cell<bool> = const { Cell::new(false) };
    /// To wake the loop when the script pushed something.
    static WAKE: RefCell<Option<EventLoopProxy<RepaintRequested>>> = const { RefCell::new(None) };
}

pub(super) fn wake() {
    WAKE.with(|w| {
        if let Some(proxy) = w.borrow().as_ref() {
            let _ = proxy.send_event(RepaintRequested);
        }
    });
}

/// The page is in mobile mode: touch targets, the phone's graphics budget (before `run`).
pub fn set_mobile(on: bool) {
    client::shell::device::set_touch(on);
    client::shell::device::set_phone_graphics(on);
}

/// The baked model vertices (`unit_models.bin`, `world_models.bin` as 32-bit words): the wasm does not
/// carry them, the page fetches them. Call once, before `run`.
pub fn set_models(units: Vec<u32>, world: Vec<u32>) -> Result<(), JsValue> {
    render::unit_models::provide(units).map_err(|e| JsValue::from_str(&e))?;
    render::world_models::provide(world).map_err(|e| JsValue::from_str(&e))
}

/// The three fog engravings (PNG files: serpent, kraken, compass), 6.7 MB the wasm does not carry; the
/// page fetches them after the menu is up. The map shows no engravings before they arrive.
pub fn set_fog_ornaments(serpent: Vec<u8>, kraken: Vec<u8>, compass: Vec<u8>) {
    client::shell::hud::fog_ornaments::provide(serpent, kraken, compass);
}

/// The settings screen offers the other mode: the page's `aureliaMode.switchTo` does it.
pub fn offer_mode_switch(mobile: bool) {
    client::shell::device::set_mode_switch(Some(client::shell::device::ModeSwitch { mobile, switch: switch_mode }));
}

fn switch_mode() {
    let Some(window) = web_sys::window() else { return };
    let Ok(mode) = js_sys::Reflect::get(&window, &JsValue::from_str("aureliaMode")) else { return };
    let Ok(switch) = js_sys::Reflect::get(&mode, &JsValue::from_str("switchTo")) else { return };
    let Ok(switch) = switch.dyn_into::<js_sys::Function>() else { return };
    let target = if client::shell::device::mode_switch().is_some_and(|m| m.mobile) { "desktop" } else { "mobile" };
    let _ = switch.call1(&mode, &JsValue::from_str(target));
}

pub fn set_safe_area(left: f32, top: f32, right: f32, bottom: f32) {
    let insets = [left, top, right, bottom].map(|v| if v.is_finite() { v.max(0.) } else { 0. });
    SAFE_AREA.with(|s| s.set((insets != [0.; 4]).then_some(insets)));
    wake();
}

pub fn push_event(event: egui::Event) {
    PENDING.with(|p| p.borrow_mut().push(event));
    wake();
}

pub fn wants_keyboard() -> bool {
    WANTS_KEYBOARD.with(Cell::get)
}

struct Page<U: ModuleUi> {
    window: Arc<Window>,
    core: GpuCore,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    app: client::shell::app::App<U>,
    input: ui_input::State,
    cursor: (f64, f64),
    pressed: Option<(MouseButton, (f64, f64))>,
    /// Fingers on the map (touch screens); mice and trackpads use `pressed`.
    touch: TouchHost,
    wake_at: Option<Instant>,
    frames: u64,
}

impl<U: ModuleUi + 'static> Page<U> {
    fn new(
        window: Arc<Window>,
        core: GpuCore,
        proxy: EventLoopProxy<RepaintRequested>,
        (module, terrain): (U, Layers),
    ) -> Result<Self, String> {
        let surface = core
            .instance
            .create_surface(window.clone())
            .map_err(|e| format!("The canvas cannot be drawn to ({e})"))?;
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
            // A WebGPU canvas is plain (`Bgra8Unorm`): the world is drawn through its sRGB view, so the GPU
            // encodes the linear colour the world shaders write (`gpu::world_format`).
            view_formats: client::shell::gpu::view_formats(format),
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&core.device, &config);
        // The browser's languages decide `auto` (there is no environment to read in a page).
        let languages = web_sys::window().map(|w| w.navigator().languages()).map(|list| list.iter().filter_map(|v| v.as_string()).collect::<Vec<_>>()).unwrap_or_default();
        client::shell::language::set_os_locales(languages);
        // The settings are a file of the browser's store (`web::storage`, loaded before the run).
        let app = client::shell::app::App::with_world_format(&core, format, client::shell::gpu::world_format(format), Some(client::shell::settings::config_path()), module, &terrain);
        app.ctx.set_request_repaint_callback(move |info| {
            if info.delay.is_zero() {
                let _ = proxy.send_event(RepaintRequested);
            }
        });
        let scale = Some(window.scale_factor() as f32);
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
            touch: TouchHost::new(scale.unwrap_or(1.0)),
            wake_at: None,
            frames: 0,
        })
    }

    fn schedule_next_frame(&mut self) {
        self.wake_at = None;
        match self.app.next_frame(Instant::now()) {
            FrameRequest::Now => self.window.request_redraw(),
            FrameRequest::At(at) => self.wake_at = Some(at),
            FrameRequest::Idle => {}
        }
    }

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

    fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.core.device, &self.config);
        }
    }

    fn draw(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                return self.resize(self.config.width, self.config.height);
            }
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());
        let world_format = client::shell::gpu::world_format(self.config.format);
        let world = (world_format != self.config.format)
            .then(|| frame.texture.create_view(&wgpu::TextureViewDescriptor { format: Some(world_format), ..Default::default() }));
        let mut raw = self.input.take_egui_input(&self.window);
        raw.events.extend(PENDING.with(|p| std::mem::take(&mut *p.borrow_mut())));
        self.app.safe_area_css = SAFE_AREA.with(Cell::get);
        let size = (self.config.width, self.config.height);
        let output = self.app.frame_split(&self.core, world.as_ref().unwrap_or(&view), &view, size, raw);
        WANTS_KEYBOARD.with(|w| w.set(self.app.ctx.egui_wants_keyboard_input()));
        self.input.handle_platform_output(&self.window, output);
        // No audio in a page yet: the sounds of the frame are dropped.
        self.app.sounds.clear();
        self.core.queue.present(frame);
        self.frames += 1;
        publish_status(self.frames, &self.app);
    }

    fn pick(&self) -> Option<u32> {
        let cursor = (self.cursor.0 as f32, self.cursor.1 as f32);
        let size = (self.config.width as f32, self.config.height as f32);
        self.app.pick(cursor, size)
    }

    fn mouse(&mut self, state: ElementState, button: MouseButton, consumed: bool) {
        let on_map =
            self.app.screens.screen == Screen::Map && !self.app.screens.navigation_is_open();
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
        self.app
            .note_pointer((self.cursor.0 as f32, self.cursor.1 as f32));
        let tile = self.pick();
        self.app.click(tile, target);
    }

    fn cursor_moved(&mut self, x: f64, y: f64, consumed: bool) {
        let dragging = matches!(self.pressed, Some((MouseButton::Left, _)));
        if dragging
            && !consumed
            && self.app.screens.screen == Screen::Map
            && !self.app.screens.navigation_is_open()
        {
            let (dx, dy) = ((x - self.cursor.0) as f32, (y - self.cursor.1) as f32);
            self.app.cancel_glide();
            self.app.pan_view(dx, dy);
        }
        self.cursor = (x, y);
    }
}

/// `window.__aurelia`: what the page's script and the tests read of the game.
fn publish_status<U: ModuleUi>(frames: u64, app: &client::shell::app::App<U>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let status = js_sys::Object::new();
    let set = |key: &str, value: JsValue| {
        let _ = js_sys::Reflect::set(&status, &JsValue::from_str(key), &value);
    };
    set("frames", JsValue::from_f64(frames as f64));
    set(
        "screen",
        JsValue::from_str(&format!("{:?}", app.screens.screen)),
    );
    let stages = app.stages;
    set("moduleMs", JsValue::from_f64(stages.module_ms));
    set("worldMs", JsValue::from_f64(stages.world_ms));
    set("uiMs", JsValue::from_f64(stages.ui_ms));
    set("paintMs", JsValue::from_f64(stages.paint_ms));
    let _ = js_sys::Reflect::set(&window, &JsValue::from_str("__aurelia"), &status);
}

struct Host<U: ModuleUi> {
    canvas: web_sys::HtmlCanvasElement,
    /// The GPU, until the window exists and takes it.
    core: Option<GpuCore>,
    module: fn() -> (U, Layers),
    proxy: EventLoopProxy<RepaintRequested>,
    page: Option<Page<U>>,
}

impl<U: ModuleUi + 'static> ApplicationHandler<RepaintRequested> for Host<U> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.page.is_some() {
            return;
        }
        let Some(core) = self.core.take() else { return };
        let attributes = Window::default_attributes()
            .with_canvas(Some(self.canvas.clone()))
            .with_title("Aurelia");
        let started = event_loop
            .create_window(attributes)
            .map_err(|e| format!("The canvas cannot be used ({e})"))
            .and_then(|window| {
                Page::new(Arc::new(window), core, self.proxy.clone(), (self.module)())
            });
        match started {
            Ok(page) => {
                page.window.request_redraw();
                self.page = Some(page);
            }
            Err(error) => report_failure(&error),
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(p) = self.page.as_mut() else { return };
        let response = p.input.on_window_event(&p.window, &event);
        match event {
            WindowEvent::Resized(size) => p.resize(size.width, size.height),
            WindowEvent::RedrawRequested => {
                p.draw();
                return p.schedule_next_frame();
            }
            WindowEvent::Touch(touch) => {
                let at = (touch.location.x as f32, touch.location.y as f32);
                let phase = match touch.phase {
                    TouchPhase::Started => Phase::Started,
                    TouchPhase::Moved => Phase::Moved,
                    TouchPhase::Ended => Phase::Ended,
                    TouchPhase::Cancelled => Phase::Cancelled,
                };
                let size = (p.config.width, p.config.height);
                p.touch.event(&mut p.app, touch.id, phase, at, response.consumed, size);
                p.window.request_redraw();
                return;
            }
            WindowEvent::Focused(false) => {
                p.touch.reset(p.window.scale_factor() as f32);
                p.app.dragging = false;
                p.app.hud.hovered_tile = None;
                p.pressed = None;
            }
            WindowEvent::CursorLeft { .. } => {
                p.pressed = None;
                p.app.dragging = false;
                p.app.hud.hovered_tile = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                p.cursor_moved(position.x, position.y, response.consumed)
            }
            WindowEvent::MouseInput { state, button, .. } => {
                p.mouse(state, button, response.consumed)
            }
            WindowEvent::MouseWheel { delta, .. }
                if p.app.screens.screen == Screen::Map
                    && !p.app.screens.navigation_is_open()
                    && !response.consumed =>
            {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(d) => d.y as f32 / 80.,
                };
                p.app.cancel_glide();
                p.app.zoom_view(1.0 + lines * 0.1);
            }
            _ => {}
        }
        if response.repaint {
            p.window.request_redraw();
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, _: RepaintRequested) {
        if let Some(p) = &self.page {
            p.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(p) = self.page.as_mut() {
            // A finger held long enough makes the secondary click: the loop wakes for it.
            let size = (p.config.width, p.config.height);
            p.touch.tick(&mut p.app, size);
            let flow = match (p.control_flow(), p.touch.deadline()) {
                (ControlFlow::WaitUntil(a), Some(b)) => ControlFlow::WaitUntil(a.min(b)),
                (ControlFlow::Wait, Some(b)) => ControlFlow::WaitUntil(b),
                (flow, _) => flow,
            };
            event_loop.set_control_flow(flow);
        }
    }
}

/// Shows why the game cannot run in the page's `#failure` element and in the console.
fn report_failure(message: &str) {
    web_sys::console::error_1(&JsValue::from_str(message));
    if let Some(element) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("failure"))
    {
        element.set_text_content(Some(message));
    }
}

/// Opens the GPU, then runs the game on the canvas with the id `canvas_id`. Resolves when
/// the event loop is installed (the loop itself runs on).
pub async fn run<U: ModuleUi + 'static>(canvas_id: String, module: fn() -> (U, Layers)) -> Result<(), JsValue> {
    // Saves and settings are read before the app exists: it looks for them when it starts.
    super::storage::install().await;
    let canvas = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(&canvas_id))
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .ok_or_else(|| JsValue::from_str("the page has no canvas with that id"))?;
    // WebGPU where the browser has an adapter, else WebGL2 (`?backend=gl` forces it).
    let force_gl = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .is_some_and(|q| q.contains("backend=gl"));
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    if force_gl {
        descriptor.backends = wgpu::Backends::GL;
    }
    let instance = wgpu::util::new_instance_with_webgpu_detection(descriptor).await;
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .ok();
    let core = match client::shell::gpu::open_on(instance, surface.as_ref()).await {
        Ok(core) => core,
        Err(error) => {
            report_failure(&error);
            return Err(JsValue::from_str(&error));
        }
    };
    let event_loop = EventLoop::<RepaintRequested>::with_user_event()
        .build()
        .map_err(|e| JsValue::from_str(&format!("no event loop ({e})")))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    WAKE.with(|w| *w.borrow_mut() = Some(proxy.clone()));
    let host = Host {
        canvas,
        core: Some(core),
        module,
        proxy,
        page: None,
    };
    // Not `run_app`: that returns an error by unwinding on the web; `spawn_app` returns.
    event_loop.spawn_app(host);
    Ok(())
}
