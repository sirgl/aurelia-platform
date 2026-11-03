//! Checking a bundle against the base language: missing and unused keys, placeholders,
//! plural forms and references. The checks never panic and never stop at the first problem.

use std::collections::BTreeSet;

use crate::bundle::Bundle;
use crate::catalog::Translator;
use crate::diag::{Diagnostic, Severity};
use crate::plural::{Category, PluralRule};
use crate::syntax::{Pattern, VariantKey};

/// How much of the base language a bundle covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Coverage {
    pub total: usize,
    pub translated: usize,
    /// Keys of the bundle that the base language does not have.
    pub unused: usize,
}

impl Coverage {
    pub fn percent(&self) -> f32 {
        if self.total == 0 {
            100.0
        } else {
            self.translated as f32 * 100.0 / self.total as f32
        }
    }
}

fn base_key(key: &str) -> &str {
    key.split('@').next().unwrap_or(key)
}

pub fn coverage(bundle: &Bundle, reference: &Bundle) -> Coverage {
    // A module's override (`key@scope`) is optional: the plain text stands in for it.
    let keys: Vec<&str> = reference
        .keys()
        .into_iter()
        .filter(|k| !k.contains('@'))
        .collect();
    let translated = keys
        .iter()
        .filter(|k| bundle.messages.contains_key(**k))
        .count();
    let unused = bundle
        .keys()
        .into_iter()
        .filter(|k| {
            !reference.messages.contains_key(*k) && !reference.messages.contains_key(base_key(k))
        })
        .count();
    Coverage {
        total: keys.len(),
        translated,
        unused,
    }
}

/// The problems of `bundle` measured against `reference` (the base language). `chain` is the
/// translator that resolves `{ references }` (the bundle's own fallback chain).
pub fn validate(bundle: &Bundle, reference: &Bundle, chain: &Translator) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let is_reference = bundle.tag == reference.tag;
    if !is_reference && crate::plural::PluralRule::known(&bundle.tag).is_none() {
        out.push(Diagnostic::new(
            Severity::Info,
            "",
            0,
            "",
            "plural",
            format!(
                "no plural rule known for `{}`; the English one (one/other) is used",
                bundle.tag
            ),
        ));
    }
    if !is_reference && bundle.meta.name.is_none() {
        out.push(Diagnostic::new(
            Severity::Warning,
            "",
            0,
            "meta.name",
            "meta",
            "the bundle has no meta.name (its own name for the language selector)".into(),
        ));
    }
    let mut keys: Vec<&String> = bundle
        .messages
        .keys()
        .filter(|k| !k.starts_with("meta."))
        .collect();
    keys.sort();
    for key in keys {
        let message = &bundle.messages[key];
        let file = &*message.file;
        let diag = |severity, kind, text: String| {
            Diagnostic::new(severity, file, message.line, key.as_str(), kind, text)
        };
        if message.pattern.nodes.is_empty() {
            out.push(diag(Severity::Warning, "empty", "the text is empty".into()));
        }
        // References resolve through the bundle's chain.
        for r in message.pattern.references() {
            if !chain.has(&r) {
                out.push(diag(
                    Severity::Error,
                    "reference",
                    format!("`{{ {r} }}` names no message"),
                ));
            }
        }
        if is_reference {
            check_plurals(&message.pattern, bundle, true, &mut |sev, kind, text| {
                out.push(diag(sev, kind, text))
            });
            continue;
        }
        let original = reference
            .messages
            .get(key)
            .or_else(|| reference.messages.get(base_key(key)));
        let Some(original) = original else {
            out.push(diag(
                Severity::Warning,
                "unused",
                "the base language has no such message".into(),
            ));
            continue;
        };
        let (theirs, ours) = (message.pattern.variables(), original.pattern.variables());
        for v in theirs.iter().filter(|v| !ours.contains(v)) {
            out.push(diag(
                Severity::Error,
                "placeholder",
                format!(
                    "argument ${v} is not in the original (original: {})",
                    list(&ours)
                ),
            ));
        }
        for v in ours.iter().filter(|v| !theirs.contains(v)) {
            out.push(diag(
                Severity::Warning,
                "placeholder",
                format!("argument ${v} of the original is not used"),
            ));
        }
        check_plurals(&message.pattern, bundle, false, &mut |sev, kind, text| {
            out.push(diag(sev, kind, text))
        });
        // The original counts something but this language shows no forms.
        let rule = bundle.lang.rule;
        let original_selects: BTreeSet<&str> =
            original.pattern.selects().iter().map(|(v, _)| *v).collect();
        let our_selects: BTreeSet<&str> =
            message.pattern.selects().iter().map(|(v, _)| *v).collect();
        for var in original_selects.difference(&our_selects) {
            if theirs.iter().any(|t| t == var) && rule.categories().len() > 1 {
                out.push(diag(
                    Severity::Warning,
                    "plural",
                    format!(
                        "${var} is counted in the original; {} has {} plural forms",
                        bundle.tag,
                        rule.categories().len()
                    ),
                ));
            }
        }
    }
    if !is_reference {
        let mut missing: Vec<&str> = reference
            .keys()
            .into_iter()
            .filter(|k| !k.contains('@') && !bundle.messages.contains_key(*k))
            .collect();
        missing.sort();
        for key in missing {
            let m = &reference.messages[key];
            out.push(Diagnostic::new(
                Severity::Info,
                &*m.file,
                m.line,
                key,
                "missing",
                "not translated; the fallback language is shown".into(),
            ));
        }
    }
    out
}

fn list(vars: &[String]) -> String {
    if vars.is_empty() {
        "none".into()
    } else {
        vars.iter()
            .map(|v| format!("${v}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Plural variants must fit the language: all its categories present, none foreign.
fn check_plurals(
    pattern: &Pattern,
    bundle: &Bundle,
    is_reference: bool,
    report: &mut dyn FnMut(Severity, &'static str, String),
) {
    let rule: PluralRule = bundle.lang.rule;
    for (var, variants) in pattern.selects() {
        let names: Vec<&str> = variants
            .iter()
            .filter_map(|v| {
                if let VariantKey::Name(n) = &v.key {
                    Some(n.as_str())
                } else {
                    None
                }
            })
            .collect();
        // A select on the text of an argument has no plural names.
        let plural_names: Vec<Category> = names
            .iter()
            .filter_map(|n| Category::from_name(n))
            .collect();
        if plural_names.is_empty() {
            continue;
        }
        for name in &names {
            if Category::from_name(name).is_none() && !is_reference {
                report(
                    Severity::Warning,
                    "plural",
                    format!("${var}: variant [{name}] mixes plural categories and other keys"),
                );
            }
        }
        let default_is_other = variants
            .iter()
            .any(|v| matches!(&v.key, VariantKey::Name(n) if n == "other"));
        let has_default_other = default_is_other;
        for category in rule.required() {
            let present = plural_names.contains(category)
                || (*category == Category::Other && has_default_other);
            if !present && !(*category == Category::Other) {
                report(
                    Severity::Error,
                    "plural",
                    format!(
                        "${var}: `{}` has no [{}] variant ({} needs: {})",
                        bundle.tag,
                        category.name(),
                        bundle.tag,
                        rule.categories()
                            .iter()
                            .map(|c| c.name())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                );
            }
        }
        if !has_default_other {
            report(
                Severity::Warning,
                "plural",
                format!("${var}: no [other] variant"),
            );
        }
        for category in &plural_names {
            if !rule.categories().contains(category) {
                report(
                    Severity::Warning,
                    "plural",
                    format!(
                        "${var}: [{}] is never used in `{}`",
                        category.name(),
                        bundle.tag
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, Origin};

    fn catalog(files: &[(&str, &str)]) -> Catalog {
        let mut c = Catalog::new();
        for (tag, text) in files {
            c.add(tag, vec![("a.ftl".into(), (*text).into())], Origin::Builtin);
        }
        c
    }

    fn report(c: &Catalog, tag: &str) -> Vec<Diagnostic> {
        let en = c.bundle("en").unwrap();
        let bundle = c.bundle(tag).unwrap();
        validate(&bundle, &en, &c.translator(tag))
    }

    fn kinds(d: &[Diagnostic]) -> Vec<(Severity, &'static str, String)> {
        d.iter()
            .map(|d| (d.severity, d.kind, d.key.clone()))
            .collect()
    }

    const EN: &str = "meta.name = English\nturn = Turn { $n }\nturns = { $n ->\n [one] { $n } turn\n *[other] { $n } turns\n}\nonly-en = x\n";

    #[test]
    fn missing_unused_and_placeholders() {
        let c = catalog(&[("en", EN), ("de", "meta.name = Deutsch\nturn = Zug { $m }\nextra = y\nturns = { $n ->\n [one] { $n } Zug\n *[other] { $n } Züge\n}\n")]);
        let d = report(&c, "de");
        let k = kinds(&d);
        assert!(
            k.contains(&(Severity::Error, "placeholder", "turn".into())),
            "{d:?}"
        );
        assert!(k.contains(&(Severity::Warning, "placeholder", "turn".into())));
        assert!(k.contains(&(Severity::Warning, "unused", "extra".into())));
        assert!(k.contains(&(Severity::Info, "missing", "only-en".into())));
        assert!(!k
            .iter()
            .any(|(_, kind, key)| *kind == "plural" && key == "turns"));
    }

    #[test]
    fn russian_needs_its_plural_forms() {
        let ru =
            "meta.name = \u{420}\u{443}\u{441}\u{441}\u{43a}\u{438}\u{439}\nturns = { $n ->\n [one] { $n } \u{445}\u{43e}\u{434}\n *[other] { $n } \u{445}\u{43e}\u{434}\u{43e}\u{432}\n}\n";
        let c = catalog(&[("en", EN), ("ru", ru)]);
        let d = report(&c, "ru");
        assert!(
            d.iter().any(|d| d.kind == "plural"
                && d.severity == Severity::Error
                && d.message.contains("[few]")),
            "{d:?}"
        );
        let good = "meta.name = \u{420}\u{443}\u{441}\u{441}\u{43a}\u{438}\u{439}\nturns = { $n ->\n [one] { $n } \u{445}\u{43e}\u{434}\n [few] { $n } \u{445}\u{43e}\u{434}\u{430}\n [many] { $n } \u{445}\u{43e}\u{434}\u{43e}\u{432}\n *[other] { $n } \u{445}\u{43e}\u{434}\u{430}\n}\n";
        let c = catalog(&[("en", EN), ("ru", good)]);
        assert!(!report(&c, "ru").iter().any(|d| d.kind == "plural"));
    }

    #[test]
    fn chinese_has_no_forms_and_extra_ones_are_flagged() {
        let zh = "meta.name = 中文\nturns = { $n } 回合\n";
        let c = catalog(&[("en", EN), ("zh-Hans", zh)]);
        assert!(
            !report(&c, "zh-Hans").iter().any(|d| d.kind == "plural"),
            "{:?}",
            report(&c, "zh-Hans")
        );
        let zh2 =
            "meta.name = 中文\nturns = { $n ->\n [one] { $n } 回合\n *[other] { $n } 回合\n}\n";
        let c = catalog(&[("en", EN), ("zh-Hans", zh2)]);
        assert!(report(&c, "zh-Hans")
            .iter()
            .any(|d| d.kind == "plural" && d.severity == Severity::Warning));
    }

    #[test]
    fn broken_references_are_errors() {
        let c = catalog(&[("en", "a = the { nope }\n")]);
        assert!(report(&c, "en").iter().any(|d| d.kind == "reference"));
    }

    #[test]
    fn coverage_counts() {
        let c = catalog(&[("en", EN), ("de", "turn = Zug { $n }\nzzz = 1\n")]);
        let cov = coverage(&c.bundle("de").unwrap(), &c.bundle("en").unwrap());
        assert_eq!(
            cov,
            Coverage {
                total: 3,
                translated: 1,
                unused: 1
            }
        );
    }
}
