//! Where the asset folder is, for everything loaded at run time: `AURELIA_ASSETS` when it is
//! set, else `assets` beside the executable (a distribution; in a macOS app bundle,
//! `Contents/Resources/assets`), else the source tree's (development builds run from the
//! workspace). A distribution has ONE merged folder; the source tree has one `assets` folder per
//! part of the checkout (platform, classic, lands: docs/split-layout.md), and a file is looked up
//! in the platform's first and then in the folders of the parts beside it.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The list of pictures a page's art service fetches and decodes (pure; tested natively).
pub mod pictures;

/// In a browser the files come over the network (`web`).
#[cfg(target_arch = "wasm32")]
pub mod web;

/// The asset folder (resolved once).
pub fn root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(resolve)
}

/// A file or folder under the asset folder; in a source tree the first of its parts' asset
/// folders that has it (the platform's when none has).
pub fn path(relative: impl AsRef<Path>) -> PathBuf {
    let first = root().join(relative.as_ref());
    #[cfg(not(target_arch = "wasm32"))]
    if !first.exists() {
        for dir in extra_roots() {
            let candidate = dir.join(relative.as_ref());
            if candidate.exists() {
                return candidate;
            }
        }
    }
    first
}

/// The asset folders of the other parts of a source tree (empty in a distribution, where
/// `AURELIA_ASSETS` or the folder beside the executable is the only one).
#[cfg(not(target_arch = "wasm32"))]
fn extra_roots() -> &'static [PathBuf] {
    static EXTRA: OnceLock<Vec<PathBuf>> = OnceLock::new();
    EXTRA.get_or_init(|| {
        let tree = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        if root() != tree {
            return Vec::new();
        }
        i18n::parts::parts_beside(Path::new(env!("CARGO_MANIFEST_DIR")))
            .into_iter()
            .map(|part| part.join("assets"))
            .filter(|dir| dir.is_dir() && dir.canonicalize().ok() != tree.canonicalize().ok())
            .collect()
    })
}

/// The file that holds an image: `path` itself, else the same name with `.webp` (the
/// distributed art pack stores its pictures as WebP, the source tree as PNG). The path
/// itself is returned when neither exists, so errors name the file the caller asked for.
pub fn image_file(path: &Path) -> PathBuf {
    if is_file(path) {
        return path.to_owned();
    }
    let webp = path.with_extension("webp");
    if is_file(&webp) {
        webp
    } else {
        path.to_owned()
    }
}

/// Whether the image exists, as PNG or as WebP.
pub fn image_exists(path: &Path) -> bool {
    is_file(&image_file(path))
}

/// Decodes an image of the asset folder, PNG or WebP (see [`image_file`]).
pub fn open_image(path: &Path) -> Option<image::DynamicImage> {
    #[cfg(not(target_arch = "wasm32"))]
    return image::open(image_file(path)).ok();
    #[cfg(target_arch = "wasm32")]
    image::load_from_memory(&web::read(&image_file(path))?).ok()
}

/// Whether the asset file exists (in a browser: whether the page serves it).
pub fn is_file(path: &Path) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    return path.is_file();
    #[cfg(target_arch = "wasm32")]
    web::exists(path)
}

/// The bytes of an asset file; None when it is missing (in a browser: or has not arrived).
pub fn read(path: &Path) -> Option<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    return std::fs::read(path).ok();
    #[cfg(target_arch = "wasm32")]
    web::read(path).map(|bytes| bytes.to_vec())
}

/// The text of an asset file; None when it is missing.
pub fn read_to_string(path: &Path) -> Option<String> {
    String::from_utf8(read(path)?).ok()
}

fn resolve() -> PathBuf {
    #[cfg(target_arch = "wasm32")]
    return PathBuf::from("assets");
    #[cfg(not(target_arch = "wasm32"))]
    resolve_native()
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_native() -> PathBuf {
    if let Some(dir) = std::env::var_os("AURELIA_ASSETS") {
        return PathBuf::from(dir);
    }
    let exe = std::env::current_exe().ok();
    let beside = exe.as_deref().and_then(Path::parent).into_iter().flat_map(|dir| [dir.join("assets"), dir.join("../Resources/assets")]);
    beside.into_iter().find(|dir| dir.is_dir()).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"))
}
