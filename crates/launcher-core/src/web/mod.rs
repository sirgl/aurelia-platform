//! The browser launcher: one canvas, the shell of `client` driven by winit's web event loop, art
//! fetched by the page's script (`client::shell::assets::web`). Nothing here knows a rule set: a
//! web launcher crate (`lands/launcher/web`, `classic/launcher/web`) is one line,
//! `launcher_core::web_exports!(|| (MyUi::game(), MyUi::terrain_layers()));`, which writes the
//! `#[wasm_bindgen]` functions the page's script calls into that crate. (The exports are generated
//! there, not here, so that they are certain to survive linking into the `cdylib`.)

pub mod assets;
pub mod page;
pub mod report;
pub mod storage;

use wasm_bindgen::prelude::*;

/// Runs once when the module loads: panics and `log` go to the browser console.
pub fn init() {
    // A deep stack for panic reports (V8 keeps 10 frames by default); the release wasm has no function names,
    // frames read `wasm-function[N]` and are symbolized with the build's symbols file (classic/docs/web-traces.md).
    js_sys::Error::set_stack_trace_limit(&JsValue::from_f64(200.0));
    std::panic::set_hook(Box::new(|info| {
        console_error_panic_hook::hook(info);
        remember_panic(info);
    }));
    // wgpu and the game report through `log`; a page has no stderr, so they go to the console.
    let _ = log::set_logger(&ConsoleLog).map(|()| log::set_max_level(log::LevelFilter::Info));
    // After the panic hook above: the report's recorder runs that hook first, then shows the dialog.
    report::init();
}

/// `window.__aureliaLastPanic = {message, location, stack}` for the bug-report dialog. The hook runs before the
/// abort of `panic = "abort"`, so the record exists when the wasm traps with `unreachable`.
fn remember_panic(info: &std::panic::PanicHookInfo<'_>) {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    let location = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default();
    let stack = js_sys::Reflect::get(&js_sys::Error::new("panic"), &"stack".into()).unwrap_or(JsValue::UNDEFINED);
    let record = js_sys::Object::new();
    for (key, value) in [("message", JsValue::from(message)), ("location", location.into()), ("stack", stack)] {
        let _ = js_sys::Reflect::set(&record, &key.into(), &value);
    }
    let _ = js_sys::Reflect::set(&js_sys::global(), &"__aureliaLastPanic".into(), &record);
}

/// A named key (`Backspace`, `Enter`, `ArrowLeft`...) pressed and released.
pub fn push_key(name: &str) {
    if let Some(key) = egui::Key::from_name(name) {
        for pressed in [true, false] {
            page::push_event(egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
}

/// Typed text from the page's hidden input (the on-screen keyboard).
pub fn push_text(text: String) {
    if !text.is_empty() {
        page::push_event(egui::Event::Text(text));
    }
}

/// The input method composes `text` (not final yet); empty ends the composition.
pub fn push_preedit(text: String) {
    page::push_event(egui::Event::Ime(egui::ImeEvent::Preedit { text, active_range_chars: None }));
}

/// The input method finished a composition with `text`.
pub fn push_commit(text: String) {
    page::push_event(egui::Event::Ime(egui::ImeEvent::Commit(text)));
}

struct ConsoleLog;

impl log::Log for ConsoleLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // naga and wgpu_core chatter at info; warnings and errors are what a page needs.
        metadata.level() <= log::Level::Warn
            || (metadata.level() == log::Level::Info
                && !metadata.target().starts_with("wgpu")
                && !metadata.target().starts_with("naga"))
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        client::shell::report::push_log(format!("[{}] {}: {}", record.level(), record.target(), record.args()));
        let line = JsValue::from_str(&format!("[{}] {}", record.target(), record.args()));
        match record.level() {
            log::Level::Error => web_sys::console::error_1(&line),
            log::Level::Warn => web_sys::console::warn_1(&line),
            _ => web_sys::console::log_1(&line),
        }
    }

    fn flush(&self) {}
}

/// Writes the page API (`#[wasm_bindgen]` functions) of a web launcher into the calling crate; the
/// argument is the module registration `fn() -> (Ui, Layers)`. The calling crate depends on
/// `wasm-bindgen`, `js-sys` and `launcher-core` only.
#[macro_export]
macro_rules! web_exports {
    ($module:expr) => {
        use $crate::web as __core;
        use wasm_bindgen::prelude::*;

        /// The page runs in mobile mode (call before `run`): the touch layout (48 dp targets) and the
        /// phone graphics budget (`Auto` render scale, no multisampling).
        #[wasm_bindgen]
        pub fn set_mobile(on: bool) {
            __core::page::set_mobile(on);
        }
        /// The baked model vertices as 32-bit words: `unit_models.bin`, `world_models.bin` (the wasm does
        /// not carry them). Call once, before `run`; an error names the file that does not fit the build.
        #[wasm_bindgen]
        pub fn set_models(units: Vec<u32>, world: Vec<u32>) -> Result<(), JsValue> {
            __core::page::set_models(units, world)
        }
        /// The fog engravings (PNG files: serpent, kraken, compass); may come any time after `run`.
        #[wasm_bindgen]
        pub fn set_fog_ornaments(serpent: Vec<u8>, kraken: Vec<u8>, compass: Vec<u8>) {
            __core::page::set_fog_ornaments(serpent, kraken, compass);
        }
        /// The settings screen shows the interface-mode row: `mobile` is the mode the page runs in.
        #[wasm_bindgen]
        pub fn offer_mode_switch(mobile: bool) {
            __core::page::offer_mode_switch(mobile);
        }
        /// The notch and system bars in CSS pixels (`env(safe-area-inset-*)`); call again on rotation.
        #[wasm_bindgen]
        pub fn set_safe_area(left: f32, top: f32, right: f32, bottom: f32) {
            __core::page::set_safe_area(left, top, right, bottom);
        }
        /// Whether a text field of the game has the focus: the page then shows the on-screen keyboard.
        #[wasm_bindgen]
        pub fn wants_keyboard() -> bool {
            __core::page::wants_keyboard()
        }
        /// Typed text from the page's hidden input (the on-screen keyboard).
        #[wasm_bindgen]
        pub fn push_text(text: String) {
            __core::push_text(text);
        }
        /// A named key (`Backspace`, `Enter`, `ArrowLeft`...) pressed and released.
        #[wasm_bindgen]
        pub fn push_key(name: String) {
            __core::push_key(&name);
        }
        /// The input method composes `text` (not final yet); empty ends the composition.
        #[wasm_bindgen]
        pub fn push_preedit(text: String) {
            __core::push_preedit(text);
        }
        /// The input method finished a composition with `text`.
        #[wasm_bindgen]
        pub fn push_commit(text: String) {
            __core::push_commit(text);
        }
        /// Runs once when the module loads.
        #[wasm_bindgen(start)]
        pub fn init() {
            __core::init();
        }
        /// Opens the GPU, then runs the game on the canvas with the id `canvas_id`. Resolves when
        /// the event loop is installed (the loop itself runs on).
        #[wasm_bindgen]
        pub async fn run(canvas_id: String) -> Result<(), JsValue> {
            __core::page::run(canvas_id, $module).await
        }
        /// `assets/manifest.tsv`: `path<TAB>bytes` for every file the page serves.
        #[wasm_bindgen]
        pub fn set_manifest(text: &str) {
            __core::assets::set_manifest(text);
        }
        /// A fetched file (`path` is relative to the asset folder: `visual/generated/menu-v1.webp`).
        #[wasm_bindgen]
        pub fn register_asset(path: &str, bytes: Vec<u8>) {
            __core::assets::register_asset(path, bytes);
        }
        /// Files the game asked for since the last call; the script fetches and registers them.
        #[wasm_bindgen]
        pub fn take_wanted() -> js_sys::Array {
            __core::assets::take_wanted()
        }
        /// `[files fetched, bytes fetched]`.
        #[wasm_bindgen]
        pub fn asset_stats() -> js_sys::Array {
            __core::assets::asset_stats()
        }
        /// The pipeline's timed events since the last call.
        #[wasm_bindgen]
        pub fn take_trace() -> js_sys::Array {
            __core::assets::take_trace()
        }
        /// The pictures the screen wants now, most urgent first; `null` while unchanged.
        #[wasm_bindgen]
        pub fn take_picture_requests() -> JsValue {
            __core::assets::take_picture_requests()
        }
        /// A decoded picture: `rgba` is `width * height * 4` bytes, not premultiplied.
        #[wasm_bindgen]
        pub fn register_picture(key: &str, width: u32, height: u32, rgba: &[u8]) {
            __core::assets::register_picture(key, width, height, rgba);
        }
        /// The page could not make the picture (file missing, undecodable): it counts as missing.
        #[wasm_bindgen]
        pub fn fail_picture(key: &str) {
            __core::assets::fail_picture(key);
        }
    };
}
