//! Lists the built-in bundles: every `assets/lang/<code>/*` file of every part of the checkout
//! becomes an `include_str!`, so adding a language is dropping a folder in `assets/lang/` and
//! rebuilding. The parts are the folders beside `platform/` that mark themselves with a file `PART`
//! (docs/split-layout.md): the platform owns the shell keys, each module part its own; a part that
//! is absent simply contributes nothing. The same file name in two parts is an error.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn lang_dirs(manifest: &Path) -> Vec<PathBuf> {
    // manifest = <part>/crates/i18n; the parts sit beside <part>.
    let own = manifest.join("../..").canonicalize().unwrap();
    let root = own.parent().unwrap().to_path_buf();
    let mut parts: Vec<PathBuf> = std::fs::read_dir(&root)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| p.join("PART").is_file())
                .collect()
        })
        .unwrap_or_default();
    parts.sort();
    parts
        .into_iter()
        .map(|p| p.join("assets/lang"))
        .filter(|p| p.is_dir())
        .collect()
}

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut languages: BTreeMap<String, BTreeMap<String, PathBuf>> = BTreeMap::new();
    for lang in lang_dirs(&manifest) {
        println!("cargo:rerun-if-changed={}", lang.display());
        for dir in std::fs::read_dir(&lang).unwrap().flatten().map(|e| e.path()) {
            if !dir.is_dir() {
                continue;
            }
            let tag = dir.file_name().unwrap().to_string_lossy().into_owned();
            println!("cargo:rerun-if-changed={}", dir.display());
            let files = languages.entry(tag.clone()).or_default();
            for file in std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()) {
                let name = file.file_name().unwrap().to_string_lossy().into_owned();
                if !file.is_file() || !(name.ends_with(".ftl") || name == "STATUS") {
                    continue;
                }
                let path = file.canonicalize().unwrap();
                println!("cargo:rerun-if-changed={}", path.display());
                if let Some(first) = files.insert(name.clone(), path.clone()) {
                    panic!(
                        "assets/lang/{tag}/{name} exists in two parts: {} and {}",
                        first.display(),
                        path.display()
                    );
                }
            }
        }
    }
    let mut out = String::from("pub(crate) static BUILTIN: &[(&str, &[(&str, &str)])] = &[\n");
    for (tag, files) in &languages {
        let _ = writeln!(out, "    ({tag:?}, &[");
        for (name, path) in files {
            let _ = writeln!(
                out,
                "        ({name:?}, include_str!({:?})),",
                path.display().to_string()
            );
        }
        out.push_str("    ]),\n");
    }
    out.push_str("];\n");
    let target = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("builtin_files.rs");
    std::fs::write(target, out).unwrap();
}
