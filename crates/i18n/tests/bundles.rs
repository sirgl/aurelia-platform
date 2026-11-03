//! The shipped bundles (`assets/lang/<code>/`) and the code that uses them: every key the
//! code asks for exists in English, every bundle is free of errors, plural forms fit each
//! language, placeholders match the English text.

use std::path::{Path, PathBuf};

use i18n::diag::Severity;

/// The parts of the checkout that are present (this one and the ones beside it).
fn parts() -> Vec<PathBuf> {
    i18n::parts::parts_beside(Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn catalog() -> i18n::Catalog {
    i18n::builtin::catalog()
}

#[test]
fn every_key_used_in_code_exists_in_english() {
    let parts = parts();
    let c = catalog();
    let en = c.bundle("en").expect("en");
    let uses = {
        let dirs = i18n::parts::source_dirs(&parts);
        let dirs: Vec<&Path> = dirs.iter().map(PathBuf::as_path).collect();
        i18n::extract::scan_dirs(&dirs)
    };
    let report = i18n::tool::key_report(&en, &uses);
    assert!(
        report.used > 100,
        "the scanner found only {} keys: is it reading the sources?",
        report.used
    );
    assert!(
        report.missing.is_empty(),
        "keys used in code but missing from assets/lang/en:\n{}",
        report
            .missing
            .iter()
            .map(|(k, f, l)| format!("  {k}  ({f}:{l})"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn every_builtin_bundle_is_free_of_errors() {
    let c = catalog();
    let errors: Vec<String> = i18n::tool::lint(&c, false)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.to_string())
        .collect();
    assert!(errors.is_empty(), "bundle errors:\n{}", errors.join("\n"));
}

#[test]
fn every_builtin_language_has_a_name_and_a_status() {
    let c = catalog();
    for info in c.languages(false) {
        assert_ne!(
            info.name, info.tag,
            "{}: meta.name is missing in meta.ftl",
            info.tag
        );
        if info.tag != "en" {
            assert!(
                info.status.is_some(),
                "{}: the STATUS file is missing",
                info.tag
            );
        }
    }
}

#[test]
fn english_is_complete_for_the_languages_that_claim_coverage_and_never_has_unknown_keys() {
    let c = catalog();
    for (tag, cov, _) in i18n::tool::coverage_table(&c) {
        assert_eq!(
            cov.unused, 0,
            "{tag}: keys the English bundle does not have"
        );
    }
}

#[test]
fn the_pseudo_locale_covers_all_of_english() {
    let c = catalog();
    let en = c.bundle("en").unwrap();
    let xa = c.bundle("en-XA").unwrap();
    assert_eq!(xa.keys().len(), en.keys().len());
}

/// Terminology rule (2026-10-20): the Russian word for "civilization" goes (the words are "nation"
/// and "power"), and the English concept is "nation" (Lands & Laws: "faction").
#[test]
fn the_forbidden_words_are_not_in_the_english_and_russian_bundles() {
    let c = catalog();
    for (tag, banned) in [("en", "civilization"), ("ru", "\u{446}\u{438}\u{432}\u{438}\u{43b}\u{438}\u{437}\u{430}\u{446}")] {
        let bundle = c.bundle(tag).unwrap();
        let mut found = Vec::new();
        for (key, message) in &bundle.messages {
            if message.source.to_lowercase().contains(banned) {
                found.push(key.as_str());
            }
        }
        found.sort();
        assert!(
            found.is_empty(),
            "{tag}: `{banned}` in {} messages, e.g. {:?}",
            found.len(),
            &found[..found.len().min(5)]
        );
    }
}

#[test]
fn numbers_outside_messages_follow_the_active_language() {
    let n = |tag: &str| i18n::with_language(tag, || i18n::number(1234.5, 0, 2));
    assert_eq!(n("en"), "1,234.5");
    assert_eq!(n("de"), "1.234,5");
    assert_eq!(n("ru"), "1\u{a0}234,5");
    assert_eq!(n("fr"), "1\u{202f}234,5");
    assert_eq!(n("ja"), "1,234.5");
    assert_eq!(i18n::with_language("ru", i18n::decimal_separator), ",");
    assert_eq!(
        i18n::with_language("zh-Hans", || i18n::number_int(12345)),
        "12,345"
    );
}
