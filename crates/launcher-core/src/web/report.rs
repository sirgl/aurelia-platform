//! "Report a bug" in the browser: the report file is a download (`window.aureliaReport.download`
//! of the page's `report.js`: a Blob and an anchor), and a panic shows the page's own dialog
//! (`window.aureliaReport.crash`), because after a panic the program cannot run again and the
//! canvas is frozen. The dialog's text is made in `client::shell::report` before the program stops.
use client::shell::report::{self, Crash, Delivered};
use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};

/// Once, when the module loads: the browser's name, the download, the crash dialog.
pub fn init() {
    report::set_page_source(page_facts);
    report::set_file_sink(download);
    report::set_panic_observer(show_crash);
    report::set_trace_source(page_trace);
    report::install_panic_recorder();
}

/// A function of the page's `aureliaReport`, if the page has the script.
fn function(name: &str) -> Option<(JsValue, js_sys::Function)> {
    let api = Reflect::get(&js_sys::global(), &JsValue::from_str("aureliaReport")).ok().filter(|api| api.is_object())?;
    let function = Reflect::get(&api, &JsValue::from_str(name)).ok()?.dyn_into::<js_sys::Function>().ok()?;
    Some((api, function))
}

/// The report file goes to the player as a download.
fn download(name: &str, bytes: &[u8]) -> Result<Delivered, String> {
    let (api, function) = function("download").ok_or_else(|| "the page has no report.js".to_owned())?;
    function.call2(&api, &JsValue::from_str(name), &Uint8Array::from(bytes)).map_err(|e| format!("{e:?}"))?;
    Ok(Delivered { place: name.to_owned(), path: None })
}

/// The panic hook: the page shows the dialog.
fn show_crash(crash: &Crash) {
    let Some((api, function)) = function("crash") else { return };
    let object = Object::new();
    let set = |object: &Object, key: &str, value: &str| {
        let _ = Reflect::set(object, &JsValue::from_str(key), &JsValue::from_str(value));
    };
    for (key, value) in [("title", &crash.title), ("body", &crash.body), ("url", &crash.url), ("fileName", &crash.file_name), ("fileText", &crash.file_text)] {
        set(&object, key, value);
    }
    let words = Object::new();
    let w = &crash.words;
    for (key, value) in [("title", &w.title), ("intro", &w.intro), ("preview", &w.preview), ("openIssue", &w.open_issue), ("saveFile", &w.save_file), ("reload", &w.reload), ("saved", &w.saved)] {
        set(&words, key, value);
    }
    let _ = Reflect::set(&object, &JsValue::from_str("words"), &words);
    let _ = function.call1(&api, &object);
}

/// The trace of a panic in the page: `window.__aureliaLastPanic` ({message, location, stack}, set by
/// the page's panic hook before the abort) when it exists, else the stack of an error made right
/// here, in the hook (function names show when the wasm keeps its name section). Read defensively.
fn page_trace() -> Option<String> {
    let global = js_sys::global();
    let field = |object: &JsValue, key: &str| Reflect::get(object, &JsValue::from_str(key)).ok().and_then(|v| v.as_string()).filter(|v| !v.is_empty());
    let mut text = String::new();
    let mut stack = None;
    if let Some(panic) = Reflect::get(&global, &JsValue::from_str("__aureliaLastPanic")).ok().filter(|p| p.is_object()) {
        let message = field(&panic, "message");
        let location = field(&panic, "location");
        if message.is_some() || location.is_some() {
            text.push_str(&format!("page hook: {} at {}\n", message.unwrap_or_default(), location.unwrap_or_default())); // i18n-ok: a log line
        }
        stack = field(&panic, "stack");
    }
    let stack = stack.or_else(|| field(&js_sys::Error::new("panic"), "stack"));
    text.push_str(stack?.as_str());
    Some(text)
}

/// The browser and the build, asked when a report is made: `navigator.userAgent` and the page's
/// `window.__aureliaBuild` ({wasm, art}, set by index.html once the loader knows the files).
fn page_facts() -> report::PageFacts {
    let agent = web_sys::window().and_then(|window| window.navigator().user_agent().ok());
    let text = |value: JsValue| value.as_string().or_else(|| js_sys::JSON::stringify(&value).ok().and_then(|s| s.as_string()));
    let build = Reflect::get(&js_sys::global(), &JsValue::from_str("__aureliaBuild")).ok().filter(|b| b.is_object()).and_then(|build| {
        let field = |key: &str| Reflect::get(&build, &JsValue::from_str(key)).ok().filter(|v| !v.is_undefined() && !v.is_null()).and_then(text);
        let (wasm, art) = (field("wasm"), field("art"));
        (wasm.is_some() || art.is_some()).then(|| format!("wasm {}, art {}", wasm.unwrap_or_default(), art.unwrap_or_default())) // i18n-ok: a log line
    });
    report::PageFacts { agent, build }
}
