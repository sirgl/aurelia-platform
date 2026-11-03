//! The parts of the checkout that ship models (docs/models-ownership.md).
//!
//! The platform's `assets/models/manifest.json` holds the shared and generic models; every other
//! part beside `platform/` that marks itself with a `PART` file (the same discovery as
//! `i18n/build.rs`) and has an `assets/models/manifest.json` adds the models and the content
//! keys of its module. A part that is absent contributes nothing, so the platform alone, the
//! platform with one module and the whole checkout all build, each with the models it has.

use std::path::{Path, PathBuf};

pub const PLATFORM: &str = "platform";

pub struct Part {
    /// The part's name: its folder name (`platform`, `classic`, `lands`).
    pub name: String,
    /// `<part>/assets/models`: the manifest and the model files its entries name.
    pub models: PathBuf,
    /// The text of `<part>/assets/models/manifest.json`.
    pub text: String,
}

impl Part {
    pub fn manifest_path(&self) -> PathBuf {
        self.models.join("manifest.json")
    }

    pub fn is_platform(&self) -> bool {
        self.name == PLATFORM
    }
}

/// The platform first, then the modules in name order. `platform` is the platform folder.
pub fn discover(platform: &Path) -> Vec<Part> {
    let platform = platform
        .canonicalize()
        .unwrap_or_else(|e| panic!("models: cannot resolve {}: {e}", platform.display()));
    let root = platform.parent().expect("the platform folder has a parent");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| p.join("PART").is_file() && p != &platform)
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    std::iter::once(platform)
        .chain(dirs)
        .filter_map(|dir| {
            let models = dir.join("assets/models");
            let manifest = models.join("manifest.json");
            let text = std::fs::read_to_string(&manifest).ok()?;
            let name = dir.file_name()?.to_string_lossy().into_owned();
            println!("cargo:rerun-if-changed={}", manifest.display());
            println!("cargo:rerun-if-changed={}", models.display());
            Some(Part { name, models, text })
        })
        .collect()
}

/// The part sets whose merge is checked, as indices into `parts` (the platform is 0): the platform
/// alone, the platform with each module, and, when there are several modules, everything.
pub fn sets(parts: &[Part]) -> Vec<Vec<usize>> {
    let mut sets = vec![vec![0]];
    for i in 1..parts.len() {
        sets.push(vec![0, i]);
    }
    if parts.len() > 2 {
        sets.push((0..parts.len()).collect());
    }
    sets
}
