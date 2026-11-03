//! The asset folder of a browser page: there is no file system, so the files the game
//! reads are fetched by the page's script and handed over (`register`). What the game
//! asks for before it has arrived reads as missing and is put on the wanted list the
//! script drains (`take_wanted`); the art screens retry missing pictures, so a picture
//! shows once its file has come.
//!
//! The manifest (`assets/manifest.tsv`, `path<TAB>bytes` per file, made by the page build)
//! tells which files exist at all: a file outside it is missing for good and is never fetched.
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Files {
    /// Every file the page can serve, with its size.
    manifest: HashMap<String, u64>,
    loaded: HashMap<String, Arc<Vec<u8>>>,
    /// Asked for, not yet handed to the script.
    wanted: BTreeSet<String>,
    /// Handed to the script (fetching or fetched).
    requested: BTreeSet<String>,
}

static FILES: Mutex<Option<Files>> = Mutex::new(None);

/// One timed event of the picture pipeline (lands/docs/web.md, "Picture loading").
pub struct TraceEvent {
    pub at: web_time::Instant,
    pub kind: &'static str,
    pub name: String,
    pub detail: String,
}

static TRACE: Mutex<Vec<TraceEvent>> = Mutex::new(Vec::new());

/// Records an event; the page reads them with `take_trace` (`?trace=1` prints the table).
pub fn trace(kind: &'static str, name: &str, detail: impl Into<String>) {
    let mut events = TRACE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if events.len() < 20_000 {
        events.push(TraceEvent { at: web_time::Instant::now(), kind, name: name.to_owned(), detail: detail.into() });
    }
}

/// Like [`trace`], but only the first time for this kind and name.
pub fn trace_once(kind: &'static str, name: &str) {
    static SEEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    let fresh = SEEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(format!("{kind}|{name}"));
    if fresh {
        trace(kind, name, "");
    }
}

/// The events recorded since the last call.
pub fn take_trace() -> Vec<TraceEvent> {
    std::mem::take(&mut *TRACE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

fn with<R>(f: impl FnOnce(&mut Files) -> R) -> R {
    let mut guard = FILES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(Files::default))
}

/// The key of a path under the asset folder: `visual/generated/menu-v1.webp`.
pub fn key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("assets/").unwrap_or(&text).to_owned()
}

/// Reads the manifest text (`path<TAB>bytes` lines).
pub fn set_manifest(text: &str) {
    with(|files| {
        files.manifest = text
            .lines()
            .filter_map(|line| {
                let (path, bytes) = line.split_once('\t')?;
                Some((path.to_owned(), bytes.trim().parse().ok()?))
            })
            .collect();
    });
}

/// A fetched file.
pub fn register(path: &str, bytes: Vec<u8>) {
    with(|files| {
        trace("arrived", path, bytes.len().to_string());
        files.loaded.insert(path.to_owned(), Arc::new(bytes));
    });
}

/// Files asked for since the last call, to fetch.
pub fn take_wanted() -> Vec<String> {
    with(|files| {
        let wanted = std::mem::take(&mut files.wanted);
        for path in &wanted {
            trace("handed", path, "");
        }
        files.requested.extend(wanted.iter().cloned());
        wanted.into_iter().collect()
    })
}

/// A fetched file's size in bytes, with the manifest's, for the page's statistics.
pub fn stats() -> (usize, u64) {
    with(|files| {
        (
            files.loaded.len(),
            files.loaded.values().map(|b| b.len() as u64).sum(),
        )
    })
}

/// Whether the page serves the file.
pub fn exists(path: &Path) -> bool {
    let key = key(path);
    with(|files| files.manifest.contains_key(&key) || files.loaded.contains_key(&key))
}

/// The file's bytes when they have arrived; otherwise the file is put on the wanted list.
pub fn read(path: &Path) -> Option<Arc<Vec<u8>>> {
    let key = key(path);
    with(|files| {
        if let Some(bytes) = files.loaded.get(&key) {
            return Some(bytes.clone());
        }
        if files.manifest.contains_key(&key) && !files.requested.contains(&key) {
            if files.wanted.insert(key.clone()) {
                trace("wanted", &key, "");
            }
        }
        None
    })
}

pub use super::pictures::PictureRequest;
use super::pictures::Pictures;

static PICTURES: Mutex<Option<Pictures<egui::ColorImage>>> = Mutex::new(None);
static REPAINT: Mutex<Option<egui::Context>> = Mutex::new(None);

fn pictures<R>(f: impl FnOnce(&mut Pictures<egui::ColorImage>) -> R) -> R {
    let mut guard = PICTURES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(Pictures::default))
}

/// A picture arrived (or `None`: the page could not make it); the loader that wants it takes it.
pub fn register_picture(key: &str, image: Option<egui::ColorImage>) {
    trace("delivered", key, image.as_ref().map_or_else(|| "none".to_owned(), |i| format!("{}x{}", i.size[0], i.size[1])));
    pictures(|p| p.deliver(key, image));
    let repaint = REPAINT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    if let Some(ctx) = repaint {
        ctx.request_repaint();
    }
}

/// The loaders ask the page to repaint through this context when a picture arrives.
pub fn set_repaint(ctx: &egui::Context) {
    *REPAINT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ctx.clone());
}

/// A new egui pass begins: wants of earlier passes are no longer wanted.
pub fn begin_picture_pass(pass: u64) {
    pictures(|p| p.begin_pass(pass));
}

/// The pass paints `request` and does not have it yet.
pub fn want_picture(request: &PictureRequest) {
    pictures(|p| p.want(request));
}

/// Whether the page has delivered picture `key` and no loader has taken it yet.
pub fn has_picture(key: &str) -> bool {
    pictures(|p| p.has(key))
}

/// The delivered picture `key`, once.
pub fn take_picture(key: &str) -> Option<Option<egui::ColorImage>> {
    pictures(|p| p.take(key))
}

/// What the current pass wants, most urgent first; `None` while that is what the last call returned.
pub fn take_picture_requests() -> Option<Vec<PictureRequest>> {
    pictures(|p| p.requests())
}
