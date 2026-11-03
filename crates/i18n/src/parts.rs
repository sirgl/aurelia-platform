//! Where the parts of a checkout sit (docs/split-layout.md): the repository is `platform/`,
//! `classic/`, `lands/` and `integration/` side by side, and each part that exists marks itself
//! with a file `PART` in its root. Nothing here names a part: tools and tests that must see the
//! bundles and the sources of every part that is present ask for the marked siblings.

use std::path::{Path, PathBuf};

/// The file that marks a folder as a part of the checkout.
pub const MARKER: &str = "PART";

/// The part folder that contains `from` (or `from` itself), if any.
pub fn part_of(from: &Path) -> Option<PathBuf> {
    let mut dir = from.to_path_buf();
    loop {
        if dir.join(MARKER).is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// All part folders beside the part that contains `from`, that part included, by name. When
/// `from` is not inside a part but holds parts (the root of the checkout), those are returned.
pub fn parts_beside(from: &Path) -> Vec<PathBuf> {
    let root = match part_of(from) {
        Some(own) => match own.parent() {
            Some(root) => root.to_path_buf(),
            None => return vec![own],
        },
        None => from.to_path_buf(),
    };
    let root = root.as_path();
    let mut parts: Vec<PathBuf> = std::fs::read_dir(root)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| p.join(MARKER).is_file())
                .collect()
        })
        .unwrap_or_default();
    parts.sort();
    parts
}

/// The `assets/lang` folders of the parts that have one.
pub fn lang_dirs(parts: &[PathBuf]) -> Vec<PathBuf> {
    parts
        .iter()
        .map(|p| p.join("assets/lang"))
        .filter(|p| p.is_dir())
        .collect()
}

/// The folders with Rust sources of the parts: `crates/` and `app/`.
pub fn source_dirs(parts: &[PathBuf]) -> Vec<PathBuf> {
    parts
        .iter()
        .flat_map(|p| [p.join("crates"), p.join("app")])
        .filter(|p| p.is_dir())
        .collect()
}
