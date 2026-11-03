//! What the desktop gets from its environment and Android does not: log output, folders for
//! settings, saves, the art cache and the assets, and the files bundled in the APK.
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use winit::platform::android::activity::AndroidApp;

const TAG: &str = "aurelia";

/// Log lines go to logcat (`adb logcat -s aurelia`); panics too, before the process dies.
pub fn init_logging() {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag(TAG),
    );
    std::panic::set_hook(Box::new(|info| {
        log::error!("panic: {info}");
    }));
}

/// The folders this run uses.
#[derive(Debug, Clone)]
pub struct Paths {
    pub assets: PathBuf,
}

/// Points the shell's environment lookups at the app's own folders (the shell reads
/// `AURELIA_ASSETS`, `AURELIA_SAVE_DIR`, `AURELIA_CONFIG_DIR` and `AURELIA_ART_CACHE`; `HOME` is unset on
/// Android and the working directory is `/`), applies `launch.env` and unpacks the bundled
/// assets on the first run of a build.
pub fn prepare(app: &AndroidApp) -> Paths {
    let internal = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/aurelia"));
    let external = app.external_data_path();
    let cache = internal
        .parent()
        .map(|p| p.join("cache"))
        .unwrap_or_else(|| internal.join("cache"));
    // `launch.env` (KEY=VALUE lines) is a developer's switch: `adb push` it to the external
    // folder to set AURELIA_RULES, AURELIA_VISUAL_* and so on without rebuilding.
    for dir in external.iter().chain(std::iter::once(&internal)) {
        apply_launch_env(&dir.join("launch.env"));
    }
    // The unpacked bundle lives in `no_backup`: Android's Auto Backup (25 MB limit, on by default)
    // must not try to upload 75 MB of art with the saves. Art pushed by hand to the external
    // folder (`adb push`) wins over the unpacked bundle.
    let unpacked = internal
        .parent()
        .map(|p| p.join("no_backup"))
        .unwrap_or_else(|| internal.clone())
        .join("assets");
    let pushed = external
        .as_ref()
        .map(|e| e.join("assets"))
        .filter(|p| p.is_dir());
    let assets = pushed.unwrap_or_else(|| unpacked.clone());
    set_default("AURELIA_TOUCH", Path::new("1"));
    set_default("HOME", &internal);
    set_default("AURELIA_ASSETS", &assets);
    set_default("AURELIA_SAVE_DIR", &internal.join("saves"));
    set_default("AURELIA_CONFIG_DIR", &internal.join("config"));
    set_default("AURELIA_ART_CACHE", &cache.join("art"));
    unpack_bundle(app, &unpacked);
    log::info!(
        "paths: assets {}, saves {}, config {}",
        std::env::var("AURELIA_ASSETS").unwrap_or_default(),
        std::env::var("AURELIA_SAVE_DIR").unwrap_or_default(),
        std::env::var("AURELIA_CONFIG_DIR").unwrap_or_default()
    );
    Paths { assets }
}

fn set_default(key: &str, value: &Path) {
    if std::env::var_os(key).is_none() {
        // Single-threaded at this point: the event loop and the worker threads start later.
        std::env::set_var(key, value);
    }
}

fn apply_launch_env(path: &Path) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        if let Some((key, value)) = line.split_once('=') {
            log::info!("launch.env: {}={}", key.trim(), value.trim());
            std::env::set_var(key.trim(), value.trim());
        }
    }
}

/// The APK's `assets/FILES.txt` names every bundled file (the NDK lists no sub-folders);
/// `BUILD-ID` there says which build packed them. Files are copied to `target` when the
/// build id there differs, so an update refreshes them and a restart costs nothing.
fn unpack_bundle(app: &AndroidApp, target: &Path) {
    let manager = app.asset_manager();
    let read = |name: &str| -> Option<Vec<u8>> {
        let name = std::ffi::CString::new(name).ok()?;
        let mut asset = manager.open(&name)?;
        let mut bytes = Vec::new();
        asset.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    };
    let Some(list) = read("FILES.txt") else {
        log::info!("no bundled assets in this APK");
        return;
    };
    let build = read("BUILD-ID")
        .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
        .unwrap_or_default();
    let marker = target.join(".build-id");
    if fs::read_to_string(&marker).ok().as_deref().map(str::trim) == Some(build.as_str()) {
        return;
    }
    let started = std::time::Instant::now();
    let (mut files, mut bytes_total) = (0usize, 0usize);
    for name in String::from_utf8_lossy(&list)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        let Some(bytes) = read(name) else {
            log::warn!("bundled file {name} cannot be read");
            continue;
        };
        let path = target.join(name);
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        match fs::File::create(&path).and_then(|mut f| f.write_all(&bytes)) {
            Ok(()) => {
                files += 1;
                bytes_total += bytes.len();
            }
            Err(error) => log::warn!("cannot write {}: {error}", path.display()),
        }
    }
    let _ = fs::write(&marker, &build);
    log::info!(
        "unpacked {files} bundled files ({} MB) in {:.1} s",
        bytes_total / 1_000_000,
        started.elapsed().as_secs_f32()
    );
}
