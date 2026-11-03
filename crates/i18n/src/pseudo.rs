//! The pseudo-locale `en-XA`: English with accented letters, expanded by about a third and
//! put in brackets. A screen in this language shows at a glance which text is not
//! localized (plain letters), which overflows or is clipped (the closing bracket is
//! missing) and how much room a longer language needs.

use std::collections::HashMap;

use crate::bundle::{Bundle, Lang, Message, Meta};
use crate::syntax::{Node, Pattern};

/// Accented look-alikes from Latin-1 and Latin Extended-A, which ordinary UI fonts have (the
/// letters with no such form, b f m q v x, stay as they are: a box for a letter would read as
/// a missing font, not as an accent).
fn accent(c: char) -> char {
    match c {
        'a' => 'á',
        'c' => 'ç',
        'd' => 'ð',
        'e' => 'é',
        'g' => 'ĝ',
        'h' => 'ĥ',
        'i' => 'í',
        'j' => 'ĵ',
        'k' => 'ķ',
        'l' => 'ļ',
        'n' => 'ñ',
        'o' => 'ö',
        'p' => 'þ',
        'r' => 'ŕ',
        's' => 'š',
        't' => 'ţ',
        'u' => 'û',
        'w' => 'ŵ',
        'y' => 'ý',
        'z' => 'ž',
        'A' => 'Å',
        'C' => 'Ç',
        'D' => 'Ð',
        'E' => 'É',
        'G' => 'Ĝ',
        'H' => 'Ĥ',
        'I' => 'Î',
        'J' => 'Ĵ',
        'K' => 'Ķ',
        'L' => 'Ļ',
        'N' => 'Ñ',
        'O' => 'Ö',
        'P' => 'Þ',
        'R' => 'Ŕ',
        'S' => 'Š',
        'T' => 'Ţ',
        'U' => 'Û',
        'W' => 'Ŵ',
        'Y' => 'Ý',
        'Z' => 'Ž',
        other => other,
    }
}

/// The accented, expanded text of `text`.
pub fn text(text: &str) -> String {
    text.chars().map(accent).collect()
}

fn transform(pattern: &Pattern) -> Pattern {
    Pattern {
        nodes: pattern
            .nodes
            .iter()
            .map(|node| match node {
                Node::Text(t) => Node::Text(text(t)),
                Node::Select {
                    var,
                    variants,
                    default,
                } => Node::Select {
                    var: var.clone(),
                    variants: variants
                        .iter()
                        .map(|v| crate::syntax::Variant {
                            key: v.key.clone(),
                            pattern: transform(&v.pattern),
                        })
                        .collect(),
                    default: *default,
                },
                other => other.clone(),
            })
            .collect(),
    }
}

/// Characters of fixed text in a pattern (the longest variant of a select).
fn length(pattern: &Pattern) -> usize {
    pattern
        .nodes
        .iter()
        .map(|n| match n {
            Node::Text(t) => t.chars().count(),
            Node::Select { variants, .. } => variants
                .iter()
                .map(|v| length(&v.pattern))
                .max()
                .unwrap_or(0),
            _ => 3,
        })
        .sum()
}

fn has_words(pattern: &Pattern) -> bool {
    pattern.nodes.iter().any(|n| match n {
        Node::Text(t) => t.chars().any(char::is_alphabetic),
        Node::Select { variants, .. } => variants.iter().any(|v| has_words(&v.pattern)),
        _ => false,
    })
}

/// The pseudo-bundle of English.
pub fn from(en: &Bundle) -> Bundle {
    let mut messages = HashMap::new();
    for (key, message) in &en.messages {
        if key.starts_with("meta.") {
            continue;
        }
        // Figures and signs ("×0.25") have no words to expand: they stay as they are.
        if !has_words(&message.pattern) {
            messages.insert(key.clone(), message.clone());
            continue;
        }
        let mut pattern = transform(&message.pattern);
        // A word or two of letters (a button's "Max") gets accents only: brackets and padding
        // would be wider than the label and say nothing about the layout.
        if length(&message.pattern) <= 4 {
            messages.insert(
                key.clone(),
                Message {
                    pattern,
                    source: String::new(),
                    file: message.file.clone(),
                    line: message.line,
                },
            );
            continue;
        }
        let pad = "~".repeat((length(&message.pattern) as f32 * 0.35).ceil() as usize);
        pattern.nodes.insert(0, Node::Text("[".into()));
        pattern.nodes.push(Node::Text(format!("{pad}]")));
        messages.insert(
            key.clone(),
            Message {
                pattern,
                source: String::new(),
                file: message.file.clone(),
                line: message.line,
            },
        );
    }
    Bundle {
        tag: crate::locale::PSEUDO.to_owned(),
        lang: Lang::of("en"),
        messages,
        meta: Meta {
            name: Some("Pseudo-English".into()),
            english: Some("Pseudo-locale".into()),
        },
        files: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_accented_and_keeps_placeholders() {
        let (b, _) = Bundle::from_files("en", &[("a.ftl".into(), "k = Saved on turn { $turn }\nn = { $n ->\n [one] { $n } turn\n *[other] { $n } turns\n}".into())]);
        let p = from(&b);
        let t = crate::catalog::Translator::new("en-XA", vec![std::sync::Arc::new(p)]);
        let saved = t.format("k", &[("turn", 7.into())]).unwrap();
        assert!(
            saved.starts_with("[Šávéð öñ ţûŕñ 7") && saved.ends_with("~~~~]"),
            "{saved}"
        );
        assert_eq!(t.format("n", &[("n", 1.into())]).unwrap(), "[1 ţûŕñ~~~~]");
    }
}
