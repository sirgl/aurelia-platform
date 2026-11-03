//! The checker behind `i18n-check`: lint the bundles, compare the keys the code uses with
//! the base language, report coverage and write translation templates. The functions work
//! on a catalogue and strings so tests can drive them; the binary only reads the files.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use crate::bundle::Bundle;
use crate::catalog::Catalog;
use crate::diag::{Diagnostic, Severity};
use crate::locale::BASE;
use crate::validate::{coverage, validate, Coverage};

/// Read every language folder of `dir` (the repository's `assets/lang`, or a player's
/// `lang` folder) into a catalogue, as plain files.
pub fn read_dir(dir: &Path) -> (Catalog, Vec<Diagnostic>) {
    let mut catalog = Catalog::new();
    let diags = crate::external::load_dir(&mut catalog, dir);
    (catalog, diags)
}

/// All problems of all bundles, against the base language. `missing` keys are counted, not
/// listed, unless `list_missing` (they fill a template, not a report).
pub fn lint(catalog: &Catalog, list_missing: bool) -> Vec<Diagnostic> {
    let mut out = catalog.parse_all();
    let Some(en) = catalog.bundle(BASE) else {
        out.push(Diagnostic::new(
            Severity::Error,
            "",
            0,
            "",
            "meta",
            "there is no base language bundle (en)".into(),
        ));
        return out;
    };
    for tag in catalog.tags() {
        let Some(bundle) = catalog.bundle(&tag) else {
            continue;
        };
        if tag == crate::locale::PSEUDO {
            continue;
        }
        let translator = catalog.translator(&tag);
        let mut found = validate(&bundle, &en, &translator);
        if !list_missing {
            found.retain(|d| d.kind != "missing");
        }
        for d in &mut found {
            if d.file.is_empty() {
                d.file = format!("{tag}");
            } else {
                d.file = format!("{tag}: {}", d.file);
            }
        }
        out.extend(found);
    }
    out
}

/// Coverage of every language against the base language.
pub fn coverage_table(catalog: &Catalog) -> Vec<(String, Coverage, Option<String>)> {
    let Some(en) = catalog.bundle(BASE) else {
        return Vec::new();
    };
    catalog
        .tags()
        .into_iter()
        .filter(|t| t != crate::locale::PSEUDO)
        .filter_map(|tag| {
            catalog
                .bundle(&tag)
                .map(|b| (tag.clone(), coverage(&b, &en), catalog.status(&tag)))
        })
        .collect()
}

/// Translated messages per file of the base language: (file, total, translated per language
/// in the order of the returned tags).
pub fn coverage_matrix(catalog: &Catalog) -> (Vec<String>, Vec<(String, usize, Vec<usize>)>) {
    let Some(en) = catalog.bundle(BASE) else {
        return (Vec::new(), Vec::new());
    };
    let tags: Vec<String> = catalog
        .tags()
        .into_iter()
        .filter(|t| t != BASE && t != crate::locale::PSEUDO)
        .collect();
    let bundles: Vec<Option<std::sync::Arc<Bundle>>> =
        tags.iter().map(|t| catalog.bundle(t)).collect();
    let mut files: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for key in en.keys().into_iter().filter(|k| !k.contains('@')) {
        let file = en.messages[key]
            .file
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim_end_matches(".ftl")
            .to_owned();
        files.entry(file).or_default().push(key);
    }
    let rows = files
        .into_iter()
        .map(|(file, keys)| {
            let done = bundles
                .iter()
                .map(|b| {
                    b.as_ref().map_or(0, |b| {
                        keys.iter().filter(|k| b.messages.contains_key(**k)).count()
                    })
                })
                .collect();
            (file, keys.len(), done)
        })
        .collect();
    (tags, rows)
}

/// The glossary of all languages as a Markdown table (one row per `glossary.*` term of the
/// base language, one column per language).
pub fn glossary_table(catalog: &Catalog) -> String {
    glossary_table_without(catalog, &[])
}

/// The glossary without the columns of the languages in `skip` (a published document that
/// must not carry the letters of one of them).
pub fn glossary_table_without(catalog: &Catalog, skip: &[&str]) -> String {
    let Some(en) = catalog.bundle(BASE) else {
        return String::new();
    };
    let tags: Vec<String> = catalog
        .tags()
        .into_iter()
        .filter(|t| t != crate::locale::PSEUDO && !skip.contains(&t.as_str()))
        .collect();
    let bundles: Vec<Option<std::sync::Arc<Bundle>>> =
        tags.iter().map(|t| catalog.bundle(t)).collect();
    let mut out = String::from("| term |");
    for tag in &tags {
        let _ = write!(out, " {tag} |");
    }
    out.push_str("\n|---|");
    for _ in &tags {
        out.push_str("---|");
    }
    out.push('\n');
    let mut keys: Vec<&str> = en
        .keys()
        .into_iter()
        .filter(|k| k.starts_with("glossary."))
        .collect();
    keys.sort_by_key(|k| (&*en.messages[*k].file, en.messages[*k].line));
    for key in keys {
        let _ = write!(out, "| {} |", &key["glossary.".len()..]);
        for bundle in &bundles {
            let text = bundle
                .as_ref()
                .and_then(|b| b.messages.get(key))
                .and_then(|m| m.pattern.plain().map(str::to_owned))
                .unwrap_or_else(|| "·".into());
            let _ = write!(out, " {text} |");
        }
        out.push('\n');
    }
    out
}

/// Keys the code asks for that the base language lacks, and base keys nobody asks for.
#[derive(Debug, Default)]
pub struct KeyReport {
    pub missing: Vec<(String, String, u32)>,
    pub unused: Vec<String>,
    pub used: usize,
}

/// Is a file of keys driven by data (content names), not by `t!` calls in code?
pub fn is_content_file(file: &str) -> bool {
    file.rsplit('/')
        .next()
        .is_some_and(|name| name.starts_with("content") || name.starts_with("glossary"))
}

pub fn key_report(en: &Bundle, uses: &BTreeMap<String, Vec<(String, u32)>>) -> KeyReport {
    let mut report = KeyReport::default();
    let exact: BTreeSet<&str> = uses
        .keys()
        .filter(|k| !k.ends_with('*'))
        .map(String::as_str)
        .collect();
    let prefixes: Vec<&str> = uses.keys().filter_map(|k| k.strip_suffix('*')).collect();
    report.used = exact.len();
    let base = |k: &str| k.split('@').next().unwrap_or(k).to_owned();
    for (key, sites) in uses {
        if key.ends_with('*') {
            continue;
        }
        if !en.messages.contains_key(key) && !en.messages.contains_key(&base(key)) {
            let (file, line) = sites.first().cloned().unwrap_or_default();
            report.missing.push((key.clone(), file, line));
        }
    }
    // Messages that mention others count as using them.
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    for message in en.messages.values() {
        referenced.extend(message.pattern.references());
    }
    for key in en.keys() {
        let message = &en.messages[key];
        if is_content_file(&message.file) {
            continue;
        }
        let k = base(key);
        let used = exact.contains(k.as_str())
            || exact.contains(key)
            || prefixes.iter().any(|p| k.starts_with(p))
            || referenced.contains(&k);
        if !used {
            report.unused.push(key.to_owned());
        }
    }
    report
}

/// A translation template for `tag`: the base language's files in order, every key with its
/// existing translation, or the English text marked `# TODO` when it has none. The files
/// are returned as (name, text).
pub fn template(catalog: &Catalog, tag: &str) -> Vec<(String, String)> {
    let Some(en) = catalog.bundle(BASE) else {
        return Vec::new();
    };
    let target = catalog.bundle(tag);
    // The base language's files in their order, each with its keys in the file's order.
    let mut files: Vec<(String, Vec<&str>)> = Vec::new();
    let mut keys: Vec<(&str, &crate::bundle::Message)> = en
        .messages
        .iter()
        .map(|(k, m)| (k.as_str(), m))
        .filter(|(k, _)| !k.starts_with("meta."))
        .collect();
    keys.sort_by(|a, b| (&*a.1.file, a.1.line).cmp(&(&*b.1.file, b.1.line)));
    for (key, message) in &keys {
        let name = message
            .file
            .rsplit('/')
            .next()
            .unwrap_or(&message.file)
            .to_owned();
        match files.iter_mut().find(|(n, _)| *n == name) {
            Some((_, list)) => list.push(key),
            None => files.push((name, vec![key])),
        }
    }
    let mut out = Vec::new();
    for (name, list) in files {
        let mut text = format!("# Translation of {name} into {tag}. Keys are in the order of the English file.\n# Lines marked TODO still hold the English text: translate them and delete the marker.\n\n");
        for key in list {
            let original = &en.messages[key];
            let existing = target.as_ref().and_then(|b| b.messages.get(key));
            match existing {
                Some(m) => {
                    let _ = writeln!(text, "{key} = {}", m.source);
                }
                None => {
                    let _ = writeln!(text, "# TODO");
                    let _ = writeln!(text, "{key} = {}", original.source);
                }
            }
        }
        out.push((name, text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Origin;

    fn catalog() -> Catalog {
        let mut c = Catalog::new();
        c.add("en", vec![("shell.ftl".into(), "meta.name = English\na.one = One\na.two = Two { $n }\na.dead = Never used\nc.x = ref { a.one }\n".into())], Origin::Builtin);
        c.add(
            "de",
            vec![(
                "shell.ftl".into(),
                "meta.name = Deutsch\na.one = Eins\n".into(),
            )],
            Origin::Builtin,
        );
        c
    }

    #[test]
    fn coverage_counts_translated_keys() {
        let table = coverage_table(&catalog());
        let de = table.iter().find(|(t, ..)| t == "de").unwrap();
        assert_eq!((de.1.total, de.1.translated), (4, 1));
        assert!((de.1.percent() - 25.0).abs() < 0.01);
    }

    #[test]
    fn the_glossary_table_has_a_column_per_language() {
        let mut c2 = Catalog::new();
        c2.add(
            "en",
            vec![(
                "glossary.ftl".into(),
                "glossary.city = city\nglossary.war = war\n".into(),
            )],
            Origin::Builtin,
        );
        c2.add(
            "fr",
            vec![("glossary.ftl".into(), "glossary.city = ville\n".into())],
            Origin::Builtin,
        );
        let table = glossary_table(&c2);
        assert!(table.contains("| city | city | ville |"), "{table}");
        assert!(table.contains("| war | war | · |"), "{table}");
    }

    #[test]
    fn the_matrix_counts_per_file() {
        let (tags, rows) = coverage_matrix(&catalog());
        assert_eq!(tags, ["de"]);
        assert_eq!(rows, [("shell".to_owned(), 4, vec![1])]);
    }

    #[test]
    fn key_report_finds_missing_and_unused() {
        let c = catalog();
        let en = c.bundle("en").unwrap();
        let mut uses = BTreeMap::new();
        uses.insert("a.two".to_owned(), vec![("x.rs".to_owned(), 3)]);
        uses.insert("a.gone".to_owned(), vec![("y.rs".to_owned(), 9)]);
        uses.insert("c.*".to_owned(), vec![("z.rs".to_owned(), 1)]);
        let report = key_report(&en, &uses);
        assert_eq!(
            report.missing,
            [("a.gone".to_owned(), "y.rs".to_owned(), 9)]
        );
        // a.one is used through the reference in c.x, c.x through the prefix.
        assert_eq!(report.unused, ["a.dead"]);
    }

    #[test]
    fn the_template_keeps_translations_and_marks_the_rest() {
        let files = template(&catalog(), "de");
        assert_eq!(files.len(), 1);
        let text = &files[0].1;
        assert!(text.contains("a.one = Eins\n"), "{text}");
        assert!(text.contains("# TODO\na.two = Two { $n }\n"), "{text}");
        // The template is itself a valid bundle whose coverage is complete.
        let mut c = Catalog::new();
        c.add("en", catalog().bundle("en").map(|_| vec![("shell.ftl".to_owned(), "a.one = One\na.two = Two { $n }\na.dead = Never used\nc.x = ref { a.one }\n".to_owned())]).unwrap(), Origin::Builtin);
        c.add("fr", files, Origin::Builtin);
        let cov = coverage(&c.bundle("fr").unwrap(), &c.bundle("en").unwrap());
        assert_eq!(cov.translated, cov.total);
    }

    #[test]
    fn lint_lists_problems_with_the_language_in_front() {
        let diags = lint(&catalog(), false);
        assert!(diags.iter().all(|d| d.kind != "missing"));
        let diags = lint(&catalog(), true);
        assert!(
            diags
                .iter()
                .any(|d| d.kind == "missing" && d.file.starts_with("de: ")),
            "{diags:?}"
        );
    }
}
