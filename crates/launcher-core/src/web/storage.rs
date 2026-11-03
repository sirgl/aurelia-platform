//! The browser's durable store for the shell's files (saves, journals, Hall of Fame, settings): IndexedDB
//! through `window.aureliaStorage` (classic/launcher/web/static/storage.js, a script beside report.js).
//! The shell reads and writes files in memory (`client::shell::storage`); this is the part that fills
//! that memory before the app starts (`install`) and persists the changes in the background (`Idb`).
//!
//! Without IndexedDB (a blocked or private window, a page without the script) the shell gets a store that
//! lives in memory only and says so once in the menu.
use client::shell::storage::{self, Millis, Op, Persist, PersistError, Store};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{spawn_local, JsFuture};

/// A burst of changes is written together this long after the first one (a journal grows every frame
/// of an AI round; its file is written whole).
const FLUSH_DELAY_MS: i32 = 400;

/// Fill the shell's store from IndexedDB and install it. Never fails: the worst case is a store that
/// does not persist. Call before the app is made.
pub async fn install() {
    let store = match open().await {
        Ok(store) => store,
        Err(error) => {
            log::warn!("storage: IndexedDB is not usable ({error}); saves will not persist");
            Store::new(Box::new(Volatile))
        }
    };
    storage::install(store);
}

async fn open() -> Result<Store, String> {
    let shim = js_sys::Reflect::get(&js_sys::global(), &"aureliaStorage".into())
        .ok()
        .filter(|s| s.is_object())
        .ok_or("the page has no aureliaStorage")?;
    let started = call(&shim, "load", &[]).map_err(|e| describe(&e).message)?;
    let stored = JsFuture::from(js_sys::Promise::resolve(&started)).await.map_err(|e| describe(&e).message)?;
    let mut files = Vec::new();
    for entry in js_sys::Array::from(&stored).iter() {
        let entry = js_sys::Array::from(&entry);
        let (Some(key), Ok(bytes)) = (entry.get(0).as_string(), entry.get(1).dyn_into::<js_sys::Uint8Array>()) else { continue };
        files.push((key, bytes.to_vec(), entry.get(2).as_f64().unwrap_or(0.0) as Millis));
    }
    let timer = Closure::<dyn FnMut()>::new(storage::flush);
    let mut store = Store::new(Box::new(Idb { shim: shim.clone(), timer }));
    store.load(files);
    log::info!("storage: {} bytes in IndexedDB", store.total_bytes());
    // The page is hidden or closing: start the last write now.
    let hook = Closure::<dyn FnMut()>::new(storage::flush);
    let _ = call(&shim, "setFlushHook", &[hook.as_ref().clone()]);
    hook.forget();
    Ok(store)
}

fn call(target: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let function: js_sys::Function = js_sys::Reflect::get(target, &name.into())?.dyn_into().map_err(|_| JsValue::from_str(&format!("aureliaStorage.{name} is missing")))?;
    function.apply(target, &args.iter().collect::<js_sys::Array>())
}

/// An error thrown by the browser, in words for the log and the flag the shell needs.
fn describe(error: &JsValue) -> PersistError {
    let field = |key: &str| js_sys::Reflect::get(error, &key.into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
    let (name, message) = (field("name"), field("message"));
    let full = name == "QuotaExceededError" || message.to_lowercase().contains("quota");
    let message = match (name.is_empty(), message.is_empty()) {
        (true, true) => error.as_string().unwrap_or_else(|| format!("{error:?}")),
        (false, false) => format!("{name}: {message}"),
        _ => format!("{name}{message}"),
    };
    PersistError { full, message }
}

/// IndexedDB behind the shim: one `commit` per batch.
struct Idb {
    shim: JsValue,
    /// Calls `storage::flush`; the timer of `schedule_flush`.
    timer: Closure<dyn FnMut()>,
}

impl Persist for Idb {
    fn now_ms(&self) -> Millis {
        js_sys::Date::now() as Millis
    }

    fn schedule_flush(&mut self) {
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(self.timer.as_ref().unchecked_ref(), FLUSH_DELAY_MS);
        }
    }

    fn submit(&mut self, id: u64, batch: Vec<Op>) {
        let (keys, bytes, times) = (js_sys::Array::new(), js_sys::Array::new(), js_sys::Array::new());
        for op in &batch {
            match op {
                Op::Put { key, bytes: data, modified } => {
                    keys.push(&JsValue::from_str(key));
                    bytes.push(&js_sys::Uint8Array::from(data.as_slice()));
                    times.push(&JsValue::from_f64(*modified as f64));
                }
                Op::Delete { key } => {
                    keys.push(&JsValue::from_str(key));
                    bytes.push(&JsValue::NULL);
                    times.push(&JsValue::from_f64(0.0));
                }
            }
        }
        let started = call(&self.shim, "commit", &[keys.into(), bytes.into(), times.into()]);
        // The answer comes from a task, never from inside this call (the store is borrowed here).
        spawn_local(async move {
            let outcome = match started {
                Ok(promise) => JsFuture::from(js_sys::Promise::resolve(&promise)).await,
                Err(error) => Err(error),
            };
            finished(id, outcome);
        });
    }
}

fn finished(id: u64, outcome: Result<JsValue, JsValue>) {
    match outcome {
        Ok(reopened) => {
            storage::batch_done(id, Ok(()));
            if reopened.as_bool() == Some(true) {
                log::warn!("storage: the IndexedDB database was lost and made anew; writing every file again");
                storage::resync();
            }
        }
        Err(error) => {
            let error = describe(&error);
            log::warn!("storage: writing to IndexedDB failed ({}){}", error.message, if error.full { ": the browser's storage is full" } else { "" });
            storage::batch_done(id, Err(error));
        }
    }
    // The menu shows a failure on the next frame.
    super::page::wake();
}

/// No durable storage: files live until the page closes.
struct Volatile;

impl Persist for Volatile {
    fn now_ms(&self) -> Millis {
        js_sys::Date::now() as Millis
    }

    fn schedule_flush(&mut self) {}

    fn submit(&mut self, _id: u64, _batch: Vec<Op>) {}

    fn durable(&self) -> bool {
        false
    }
}
