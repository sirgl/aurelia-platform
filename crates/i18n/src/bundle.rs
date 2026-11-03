//! A bundle: the messages of one language, read from its `.ftl` files.

use std::collections::HashMap;

use crate::diag::{Diagnostic, Severity};
use crate::numfmt::NumberFormat;
use crate::plural::PluralRule;
use crate::syntax::{self, Pattern};

/// What the formatter needs to know about a language.
#[derive(Debug, Clone, Copy)]
pub struct Lang {
    pub rule: PluralRule,
    pub numbers: NumberFormat,
}

impl Lang {
    pub fn of(tag: &str) -> Lang {
        Lang {
            rule: PluralRule::for_language(tag),
            numbers: NumberFormat::for_language(tag),
        }
    }
}

/// One message of a bundle.
#[derive(Debug, Clone)]
pub struct Message {
    pub pattern: Pattern,
    /// The value as written, for templates.
    pub source: String,
    /// The file and line it came from.
    pub file: std::sync::Arc<str>,
    pub line: u32,
}

/// What the language says about itself (`meta.ftl`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    /// The language's own name: "Deutsch".
    pub name: Option<String>,
    /// Its name in English: "German".
    pub english: Option<String>,
}

/// The messages of one language.
#[derive(Debug)]
pub struct Bundle {
    pub tag: String,
    pub lang: Lang,
    pub messages: HashMap<String, Message>,
    pub meta: Meta,
    /// The files read, in order.
    pub files: Vec<String>,
}

impl Bundle {
    /// Read a bundle from its files `(name, text)`. A later file overrides an earlier one
    /// (a user's bundle for a built-in language overrides single messages).
    pub fn from_files(tag: &str, files: &[(String, String)]) -> (Bundle, Vec<Diagnostic>) {
        let mut messages: HashMap<String, Message> = HashMap::new();
        let mut diags = Vec::new();
        let mut names = Vec::new();
        for (name, text) in files {
            names.push(name.clone());
            let file: std::sync::Arc<str> = name.as_str().into();
            let (entries, mut d) = syntax::parse_file(name, text);
            diags.append(&mut d);
            for entry in entries {
                if let Some(previous) = messages.get(&entry.key) {
                    if *previous.file == *name {
                        diags.push(Diagnostic::new(
                            Severity::Warning,
                            name.clone(),
                            entry.line,
                            entry.key.clone(),
                            "duplicate",
                            format!("defined twice (first at line {})", previous.line),
                        ));
                    }
                }
                messages.insert(
                    entry.key,
                    Message {
                        pattern: entry.pattern,
                        source: entry.source,
                        file: file.clone(),
                        line: entry.line,
                    },
                );
            }
        }
        let text_of = |key: &str| {
            messages
                .get(key)
                .and_then(|m| m.pattern.plain())
                .map(str::to_owned)
                .filter(|s| !s.is_empty())
        };
        let meta = Meta {
            name: text_of("meta.name"),
            english: text_of("meta.english"),
        };
        (
            Bundle {
                tag: tag.to_owned(),
                lang: Lang::of(tag),
                messages,
                meta,
                files: names,
            },
            diags,
        )
    }

    pub fn get(&self, key: &str) -> Option<&Message> {
        self.messages.get(key)
    }

    /// The keys of the translatable messages (not `meta.*`), sorted.
    pub fn keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = self
            .messages
            .keys()
            .map(String::as_str)
            .filter(|k| !k.starts_with("meta."))
            .collect();
        keys.sort_unstable();
        keys
    }
}
