//! The bundles compiled into the game: `assets/lang/<code>/*.ftl`, listed by `build.rs`.

use crate::catalog::{Catalog, Origin};

include!(concat!(env!("OUT_DIR"), "/builtin_files.rs"));

/// A catalogue of the built-in languages.
pub fn catalog() -> Catalog {
    let mut catalog = Catalog::new();
    for (tag, files) in BUILTIN {
        catalog.add(
            tag,
            files
                .iter()
                .map(|(n, t)| (n.to_string(), t.to_string()))
                .collect(),
            Origin::Builtin,
        );
    }
    catalog
}
