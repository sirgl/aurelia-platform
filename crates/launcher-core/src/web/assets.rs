//! What the page's script hands the game: the manifest of the art files, the files it
//! fetched, and the list of files the game asked for and has not got yet. The `#[wasm_bindgen]` wrappers
//! are generated into the launcher crate by [`web_exports!`](crate::web_exports).
use client::shell::assets::web;
use wasm_bindgen::prelude::*;

/// `assets/manifest.tsv`: `path<TAB>bytes` for every file the page serves.
pub fn set_manifest(text: &str) {
    web::set_manifest(text);
}

/// A fetched file (`path` is relative to the asset folder: `visual/generated/menu-v1.webp`).
pub fn register_asset(path: &str, bytes: Vec<u8>) {
    web::register(path, bytes);
}

/// Files the game asked for since the last call; the script fetches and registers them.
pub fn take_wanted() -> js_sys::Array {
    web::take_wanted()
        .into_iter()
        .map(|path| JsValue::from_str(&path))
        .collect()
}

/// `[files fetched, bytes fetched]`.
pub fn asset_stats() -> js_sys::Array {
    let (files, bytes) = web::stats();
    js_sys::Array::of2(
        &JsValue::from_f64(files as f64),
        &JsValue::from_f64(bytes as f64),
    )
}

/// The pipeline's timed events since the last call, as `[ms on performance.now(), kind, name, detail]` rows.
pub fn take_trace() -> js_sys::Array {
    let now_page = web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now());
    let now = web_time::Instant::now();
    web::take_trace()
        .into_iter()
        .map(|event| {
            let ago = now.saturating_duration_since(event.at).as_secs_f64() * 1000.0;
            JsValue::from(js_sys::Array::of4(
                &JsValue::from_f64(now_page - ago),
                &JsValue::from_str(event.kind),
                &JsValue::from_str(&event.name),
                &JsValue::from_str(&event.detail),
            ))
        })
        .collect()
}

/// The pictures the screen wants now, most urgent first, as rows `[key, path, x, y, w, h, maxSide, priority]`
/// (`x` is -1 for a whole file); `null` while the list is the one returned last. The page's art service
/// replaces its queue with it: what is not in it any more is dropped if it has not started.
pub fn take_picture_requests() -> JsValue {
    let Some(list) = web::take_picture_requests() else {
        return JsValue::NULL;
    };
    list.into_iter()
        .map(|r| {
            let [x, y, w, h] = r.rect.map_or([-1.0; 4], |rect| rect.map(f64::from));
            let row = js_sys::Array::new();
            row.push(&JsValue::from_str(&r.key));
            row.push(&JsValue::from_str(&r.path));
            for number in [x, y, w, h, f64::from(r.max_side), f64::from(r.priority)] {
                row.push(&JsValue::from_f64(number));
            }
            JsValue::from(row)
        })
        .collect::<js_sys::Array>()
        .into()
}

/// A decoded picture: `rgba` is `width * height * 4` bytes, not premultiplied.
pub fn register_picture(key: &str, width: u32, height: u32, rgba: &[u8]) {
    let size = [width as usize, height as usize];
    let image = (rgba.len() == size[0] * size[1] * 4 && width > 0 && height > 0)
        .then(|| egui::ColorImage::from_rgba_unmultiplied(size, rgba));
    web::register_picture(key, image);
}

/// The page could not make the picture (file missing, undecodable): it counts as missing.
pub fn fail_picture(key: &str) {
    web::register_picture(key, None);
}
