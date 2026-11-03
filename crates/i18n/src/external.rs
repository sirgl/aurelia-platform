//! Bundles from the player's folder: `<config>/lang/<code>/*.ftl`. A folder of a built-in
//! language overrides single messages; a folder of a new language adds it. Nothing here
//! panics: unreadable files and bad names are reported.

use std::path::Path;

use crate::catalog::{Catalog, Origin};
use crate::diag::{Diagnostic, Severity};
use crate::locale;

/// Read every language folder under `dir` into `catalog`. A missing `dir` is not a problem.
pub fn load_dir(catalog: &mut Catalog, dir: &Path) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let Ok(read) = std::fs::read_dir(dir) else {
        return diags;
    };
    let mut dirs: Vec<_> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for folder in dirs {
        let name = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(tag) =
            locale::normalize(&name).filter(|t| *t == name || t.eq_ignore_ascii_case(&name))
        else {
            diags.push(Diagnostic::new(
                Severity::Warning,
                folder.display().to_string(),
                0,
                "",
                "meta",
                format!("`{name}` is not a language tag (like de, pt-BR, zh-Hans); folder skipped"),
            ));
            continue;
        };
        let mut paths: Vec<_> = match std::fs::read_dir(&folder) {
            Ok(r) => r
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect(),
            Err(e) => {
                diags.push(Diagnostic::new(
                    Severity::Warning,
                    folder.display().to_string(),
                    0,
                    "",
                    "meta",
                    format!("cannot read the folder: {e}"),
                ));
                continue;
            }
        };
        paths.sort();
        let mut files = Vec::new();
        for path in paths {
            let file = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !(file.ends_with(".ftl") || file == "STATUS") {
                continue;
            }
            match std::fs::read(&path) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    // A name that tells the folder apart from a built-in file of the same name.
                    Ok(text) => files.push((format!("{name}/{file}"), text)),
                    Err(_) => diags.push(Diagnostic::new(
                        Severity::Error,
                        path.display().to_string(),
                        0,
                        "",
                        "syntax",
                        "the file is not UTF-8; skipped".to_owned(),
                    )),
                },
                Err(e) => diags.push(Diagnostic::new(
                    Severity::Warning,
                    path.display().to_string(),
                    0,
                    "",
                    "meta",
                    format!("cannot read the file: {e}"),
                )),
            }
        }
        if files.is_empty() {
            diags.push(Diagnostic::new(
                Severity::Info,
                folder.display().to_string(),
                0,
                "",
                "meta",
                "no .ftl files".to_owned(),
            ));
            continue;
        }
        // STATUS goes through `add` by name.
        let files = files
            .into_iter()
            .map(|(n, t)| {
                if n.ends_with("/STATUS") {
                    ("STATUS".to_owned(), t)
                } else {
                    (n, t)
                }
            })
            .collect();
        catalog.add(&tag, files, Origin::External(folder.display().to_string()));
    }
    diags
}
