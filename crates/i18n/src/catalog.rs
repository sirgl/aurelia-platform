//! The catalogue: every known language's sources, and the translator that formats a
//! message through a fallback chain.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use crate::arg::Arg;
use crate::bundle::{Bundle, Lang, Message};
use crate::diag::Diagnostic;
use crate::locale;
use crate::numfmt::DateOrder;
use crate::pseudo;
use crate::syntax::{Node, Pattern, VariantKey};

/// Where a bundle's files came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Compiled into the game (`assets/lang/<code>/`).
    Builtin,
    /// A folder of the player's (`<config>/lang/<code>/`).
    External(String),
}

/// The files of one language, not yet parsed.
#[derive(Debug, Clone)]
struct Source {
    files: Vec<(String, String)>,
    origins: Vec<Origin>,
    status: Option<String>,
}

/// What the language selector shows about a language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageInfo {
    pub tag: String,
    /// The language's own name ("Français"); the tag when the bundle has none.
    pub name: String,
    pub english_name: String,
    pub origin: Origin,
    /// The first line of the bundle's STATUS file ("machine-drafted ...").
    pub status: Option<String>,
}

/// All languages known to the game.
#[derive(Debug, Default)]
pub struct Catalog {
    sources: BTreeMap<String, Source>,
    parsed: Mutex<HashMap<String, Arc<Bundle>>>,
}

impl Catalog {
    pub fn new() -> Catalog {
        Catalog::default()
    }

    /// Add the files of a language. Files of an existing language are appended: their
    /// messages override the earlier ones, file by file.
    pub fn add(&mut self, tag: &str, files: Vec<(String, String)>, origin: Origin) {
        let tag = locale::normalize(tag).unwrap_or_else(|| tag.to_owned());
        let source = self.sources.entry(tag.clone()).or_insert_with(|| Source {
            files: Vec::new(),
            origins: Vec::new(),
            status: None,
        });
        for (name, text) in files {
            if name == "STATUS" {
                source.status = Some(text.lines().next().unwrap_or("").trim().to_owned())
                    .filter(|s| !s.is_empty());
                continue;
            }
            source.origins.push(origin.clone());
            source.files.push((name, text));
        }
        self.parsed.lock().unwrap().remove(&tag);
    }

    /// The tags of the known languages, sorted (the pseudo-locale included when `en` is known).
    pub fn tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self.sources.keys().cloned().collect();
        if self.sources.contains_key(locale::BASE) && !tags.iter().any(|t| t == locale::PSEUDO) {
            tags.push(locale::PSEUDO.to_owned());
        }
        tags.sort();
        tags
    }

    pub fn has(&self, tag: &str) -> bool {
        tag == locale::PSEUDO && self.sources.contains_key(locale::BASE)
            || self.sources.contains_key(tag)
    }

    /// The parsed bundle of `tag` (None: unknown), with what was wrong in its files.
    pub fn bundle(&self, tag: &str) -> Option<Arc<Bundle>> {
        self.bundle_with_diagnostics(tag).map(|(b, _)| b)
    }

    pub fn bundle_with_diagnostics(&self, tag: &str) -> Option<(Arc<Bundle>, Vec<Diagnostic>)> {
        if let Some(b) = self.parsed.lock().unwrap().get(tag) {
            return Some((b.clone(), Vec::new()));
        }
        let (bundle, diags) = if tag == locale::PSEUDO && !self.sources.contains_key(tag) {
            let en = self.bundle(locale::BASE)?;
            (pseudo::from(&en), Vec::new())
        } else {
            let source = self.sources.get(tag)?;
            Bundle::from_files(tag, &source.files)
        };
        let bundle = Arc::new(bundle);
        self.parsed
            .lock()
            .unwrap()
            .insert(tag.to_owned(), bundle.clone());
        Some((bundle, diags))
    }

    /// Parse every language and collect the syntax problems of all of them.
    pub fn parse_all(&self) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        for tag in self.sources.keys() {
            self.parsed.lock().unwrap().remove(tag);
            if let Some((_, mut d)) = self.bundle_with_diagnostics(tag) {
                out.append(&mut d);
            }
        }
        out
    }

    /// The languages for a selector, `en` first, the pseudo-locale last (only when asked).
    pub fn languages(&self, with_pseudo: bool) -> Vec<LanguageInfo> {
        let mut out = Vec::new();
        for (tag, source) in &self.sources {
            // Only the files that mention `meta.` are read for the selector.
            let meta: Vec<crate::syntax::Entry> = source
                .files
                .iter()
                .filter(|(_, t)| t.contains("meta."))
                .flat_map(|(n, t)| crate::syntax::parse_file(n, t).0)
                .collect();
            let text = |key: &str| {
                meta.iter()
                    .rev()
                    .find(|e| e.key == key)
                    .and_then(|e| e.pattern.plain().map(str::to_owned))
            };
            let origin = source.origins.last().cloned().unwrap_or(Origin::Builtin);
            out.push(LanguageInfo {
                tag: tag.clone(),
                name: text("meta.name").unwrap_or_else(|| tag.clone()),
                english_name: text("meta.english").unwrap_or_else(|| tag.clone()),
                origin,
                status: source.status.clone(),
            });
        }
        out.sort_by(|a, b| {
            (a.tag != locale::BASE, a.english_name.to_lowercase())
                .cmp(&(b.tag != locale::BASE, b.english_name.to_lowercase()))
        });
        if with_pseudo && self.sources.contains_key(locale::BASE) {
            out.push(LanguageInfo {
                tag: locale::PSEUDO.into(),
                name: "Pseudo-English".into(),
                english_name: "Pseudo-locale".into(),
                origin: Origin::Builtin,
                status: Some("test locale: accented, expanded, bracketed English".into()),
            });
        }
        out
    }

    pub fn status(&self, tag: &str) -> Option<String> {
        self.sources.get(tag).and_then(|s| s.status.clone())
    }

    /// A translator for `tag` through its fallback chain (unknown bundles are skipped; the
    /// chain always ends in `en` when it exists).
    pub fn translator(&self, tag: &str) -> Translator {
        let mut chain = Vec::new();
        for candidate in locale::chain(tag) {
            if let Some(bundle) = self.bundle(&candidate) {
                chain.push(bundle);
            }
        }
        Translator::new(tag, chain)
    }
}

/// Formats messages through a chain of bundles, the requested language first.
///
/// A game module may override single messages: `key@scope` wins over `key` when the
/// lookup names that scope (Lands & Laws says "Round" where the shell says "Turn").
#[derive(Debug)]
pub struct Translator {
    pub tag: String,
    chain: Vec<Arc<Bundle>>,
    /// scope ("" for none) -> key -> the text kept for the life of the process.
    interned: Mutex<HashMap<String, HashMap<String, &'static str>>>,
}

const MAX_DEPTH: u8 = 8;

impl Translator {
    pub fn new(tag: &str, chain: Vec<Arc<Bundle>>) -> Translator {
        Translator {
            tag: tag.to_owned(),
            chain,
            interned: Mutex::new(HashMap::new()),
        }
    }

    /// How the language of this translator writes numbers (English conventions when the
    /// chain is empty).
    pub fn numbers(&self) -> crate::numfmt::NumberFormat {
        self.chain
            .first()
            .map(|b| b.lang.numbers)
            .unwrap_or_else(|| crate::numfmt::NumberFormat::for_language(crate::locale::BASE))
    }

    pub fn chain_tags(&self) -> Vec<&str> {
        self.chain.iter().map(|b| b.tag.as_str()).collect()
    }

    /// The message for `key` and the language it was found in. In each language of the
    /// chain the module's override (`key@scope`) comes before the plain key.
    pub fn find_in(&self, scope: Option<&str>, key: &str) -> Option<(&Message, &Bundle)> {
        for bundle in &self.chain {
            if let Some(scope) = scope {
                if let Some(m) = bundle.messages.get(&format!("{key}@{scope}")) {
                    return Some((m, bundle));
                }
            }
            if let Some(m) = bundle.messages.get(key) {
                return Some((m, bundle));
            }
        }
        None
    }

    pub fn find(&self, key: &str) -> Option<(&Message, &Bundle)> {
        self.find_in(None, key)
    }

    pub fn has(&self, key: &str) -> bool {
        self.find(key).is_some()
    }

    /// The text of `key` with `args`; None when no bundle in the chain has the key.
    pub fn format(&self, key: &str, args: &[(&str, Arg)]) -> Option<String> {
        self.format_in(None, key, args)
    }

    pub fn format_in(
        &self,
        scope: Option<&str>,
        key: &str,
        args: &[(&str, Arg)],
    ) -> Option<String> {
        let (message, bundle) = self.find_in(scope, key)?;
        let mut out = String::new();
        self.render(
            scope,
            &message.pattern,
            bundle.lang,
            bundle,
            args,
            0,
            &mut out,
        );
        Some(out)
    }

    /// The text of `key` without arguments, as a `&'static str` kept for the life of the
    /// process (one copy per key, scope and translator). None when the key is unknown.
    pub fn text_in(&self, scope: Option<&str>, key: &str) -> Option<&'static str> {
        let scope_key = scope.unwrap_or("");
        if let Some(text) = self
            .interned
            .lock()
            .unwrap()
            .get(scope_key)
            .and_then(|m| m.get(key))
        {
            return Some(text);
        }
        let text: &'static str = Box::leak(self.format_in(scope, key, &[])?.into_boxed_str());
        self.interned
            .lock()
            .unwrap()
            .entry(scope_key.to_owned())
            .or_default()
            .insert(key.to_owned(), text);
        Some(text)
    }

    fn render(
        &self,
        scope: Option<&str>,
        pattern: &Pattern,
        lang: Lang,
        home: &Bundle,
        args: &[(&str, Arg)],
        depth: u8,
        out: &mut String,
    ) {
        let arg = |name: &str| args.iter().find(|(n, _)| *n == name).map(|(_, v)| v);
        for node in &pattern.nodes {
            match node {
                Node::Text(t) => out.push_str(t),
                Node::Var(name) => match arg(name) {
                    Some(Arg::Int(i)) => out.push_str(&i.to_string()),
                    Some(Arg::Float(f)) => out.push_str(&lang.numbers.format(*f, 0, 6)),
                    Some(Arg::Text(t)) => out.push_str(t),
                    Some(Arg::Date(y, m, d)) => {
                        out.push_str(&DateOrder::format(&home.tag, *y, *m, *d))
                    }
                    None => {
                        out.push_str("{$");
                        out.push_str(name);
                        out.push('}');
                    }
                },
                Node::Number {
                    var,
                    min_fraction,
                    max_fraction,
                } => match arg(var) {
                    Some(Arg::Int(i)) => {
                        out.push_str(&lang.numbers.format(*i as f64, *min_fraction, *max_fraction))
                    }
                    Some(Arg::Float(f)) => {
                        out.push_str(&lang.numbers.format(*f, *min_fraction, *max_fraction))
                    }
                    Some(Arg::Text(t)) => out.push_str(t),
                    Some(Arg::Date(..)) => {}
                    None => {
                        out.push_str("{$");
                        out.push_str(var);
                        out.push('}');
                    }
                },
                Node::Date(var) => match arg(var) {
                    Some(Arg::Date(y, m, d)) => {
                        out.push_str(&DateOrder::format(&home.tag, *y, *m, *d))
                    }
                    Some(Arg::Text(t)) => out.push_str(t),
                    _ => {
                        out.push_str("{$");
                        out.push_str(var);
                        out.push('}');
                    }
                },
                Node::Ref(key) => {
                    if depth >= MAX_DEPTH {
                        out.push_str("{…}");
                    } else if let Some((message, bundle)) = self.find_in(scope, key) {
                        self.render(
                            scope,
                            &message.pattern,
                            bundle.lang,
                            bundle,
                            args,
                            depth + 1,
                            out,
                        );
                    } else {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                Node::Select {
                    var,
                    variants,
                    default,
                } => {
                    let chosen = match arg(var) {
                        Some(Arg::Int(i)) => select_number(variants, lang, *i as f64, Some(*i)),
                        Some(Arg::Float(f)) => select_number(
                            variants,
                            lang,
                            *f,
                            (f.fract() == 0.0 && f.abs() < 1e15).then_some(*f as i64),
                        ),
                        Some(Arg::Text(t)) => variants
                            .iter()
                            .position(|v| matches!(&v.key, VariantKey::Name(n) if n == t)),
                        _ => None,
                    };
                    let variant = &variants[chosen.unwrap_or(*default)];
                    self.render(scope, &variant.pattern, lang, home, args, depth, out);
                }
            }
        }
    }
}

fn select_number(
    variants: &[crate::syntax::Variant],
    lang: Lang,
    value: f64,
    whole: Option<i64>,
) -> Option<usize> {
    if let Some(n) = whole {
        if let Some(i) = variants.iter().position(|v| v.key == VariantKey::Number(n)) {
            return Some(i);
        }
    }
    let category = match whole {
        Some(n) => lang.rule.category(n),
        None => lang.rule.category_fraction(value),
    };
    variants
        .iter()
        .position(|v| matches!(&v.key, VariantKey::Name(name) if name == category.name()))
}
