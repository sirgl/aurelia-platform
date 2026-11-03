//! The message syntax: a subset of Mozilla's Fluent (FTL) with dotted keys.
//!
//! ```text
//! # A comment.
//! shell.end-turn = End turn
//! shell.saved = Saved on turn { $turn }
//! shell.hud.turns = { $n ->
//!     [one] { $n } turn
//!    *[other] { $n } turns
//! }
//! shell.score = Score: { NUMBER($points) }
//! shell.turn@lal = Round
//! content.term = the { glossary.city }
//! ```
//!
//! * a key is dotted lowercase words (`[A-Za-z][A-Za-z0-9_.-]*`); `key@scope` overrides `key`
//!   for the game module `scope` (Lands & Laws says "Round" where the shell says "Turn");
//! * `{ $name }` inserts an argument; `{ NUMBER($n) }` inserts a number in the language's
//!   digits grouping; `{ DATE($d) }` a date; `{ other.key }` another message;
//! * `{ $n -> [one] .. *[other] .. }` picks a variant by the plural category of a number, by
//!   an exact number (`[0]`) or by the text of an argument (`[feminine]`); `*` marks the
//!   default;
//! * a value continues on the indented lines below it; `{"{"}` writes a brace.
//!
//! Anything this parser cannot read becomes a [`Diagnostic`] and the entry is skipped: a bad
//! bundle never stops the game.

use crate::diag::{Diagnostic, Severity};

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Text(String),
    Var(String),
    Number {
        var: String,
        min_fraction: u8,
        max_fraction: u8,
    },
    Date(String),
    Ref(String),
    Select {
        var: String,
        variants: Vec<Variant>,
        default: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub key: VariantKey,
    pub pattern: Pattern,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VariantKey {
    /// A plural category (`one`) or the text of an argument (`feminine`).
    Name(String),
    /// An exact whole number (`[0]`).
    Number(i64),
}

/// One `key = value` entry of a bundle file.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub pattern: Pattern,
    /// The value as written (lines joined with `\n`), for templates.
    pub source: String,
    pub line: u32,
}

impl Pattern {
    pub fn text(text: impl Into<String>) -> Pattern {
        Pattern {
            nodes: vec![Node::Text(text.into())],
        }
    }

    /// The text of a pattern without placeables, None when it has any.
    pub fn plain(&self) -> Option<&str> {
        match self.nodes.as_slice() {
            [] => Some(""),
            [Node::Text(t)] => Some(t),
            _ => None,
        }
    }

    /// Every argument the pattern reads (also inside variants), sorted and unique.
    pub fn variables(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_variables(&mut out);
        out.sort();
        out.dedup();
        out
    }

    fn collect_variables(&self, out: &mut Vec<String>) {
        for node in &self.nodes {
            match node {
                Node::Text(_) | Node::Ref(_) => {}
                Node::Var(v) | Node::Date(v) => out.push(v.clone()),
                Node::Number { var, .. } => out.push(var.clone()),
                Node::Select { var, variants, .. } => {
                    out.push(var.clone());
                    for v in variants {
                        v.pattern.collect_variables(out);
                    }
                }
            }
        }
    }

    /// Every message the pattern refers to.
    pub fn references(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect_refs(&mut out);
        out.sort();
        out.dedup();
        out
    }

    fn collect_refs(&self, out: &mut Vec<String>) {
        for node in &self.nodes {
            match node {
                Node::Ref(r) => out.push(r.clone()),
                Node::Select { variants, .. } => {
                    variants.iter().for_each(|v| v.pattern.collect_refs(out))
                }
                _ => {}
            }
        }
    }

    /// The selects of the pattern: the selector variable and the variant keys.
    pub fn selects(&self) -> Vec<(&str, &[Variant])> {
        let mut out = Vec::new();
        self.collect_selects(&mut out);
        out
    }

    fn collect_selects<'a>(&'a self, out: &mut Vec<(&'a str, &'a [Variant])>) {
        for node in &self.nodes {
            if let Node::Select { var, variants, .. } = node {
                out.push((var, variants));
                for v in variants {
                    v.pattern.collect_selects(out);
                }
            }
        }
    }
}

/// Is `key` a valid message key?
pub fn valid_key(key: &str) -> bool {
    let (base, scope) = match key.split_once('@') {
        Some((b, s)) => (b, Some(s)),
        None => (key, None),
    };
    let mut chars = base.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    first_ok
        && base
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        && !base.ends_with('.')
        && !base.contains("..")
        && scope.is_none_or(|s| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
        })
}

/// Parse a bundle file. Returns the entries and what could not be read.
pub fn parse_file(file: &str, text: &str) -> (Vec<Entry>, Vec<Diagnostic>) {
    let mut entries = Vec::new();
    let mut diags = Vec::new();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let number = i as u32 + 1;
        if line.trim().is_empty() || line.starts_with('#') {
            i += 1;
            continue;
        }
        let Some((key, rest)) = split_entry(line) else {
            diags.push(Diagnostic::new(
                Severity::Error,
                file,
                number,
                "",
                "syntax",
                format!("not an entry: {:.60}", line),
            ));
            i += 1;
            continue;
        };
        // The value continues on the indented (or blank-then-indented) lines below.
        let mut value = rest.trim_start().to_owned();
        let mut j = i + 1;
        while j < lines.len() {
            let next = lines[j];
            let indented = next.starts_with(' ') || next.starts_with('\t');
            // Inside an open `{` a line at column 0 (the closing brace) still belongs to
            // the entry, unless it starts a new entry or a comment.
            let inside = open_braces(&value) > 0
                && !next.starts_with('#')
                && !split_entry(next)
                    .is_some_and(|(k, _)| valid_key(k) && !next.starts_with(['[', '*', '}']));
            if indented || inside {
                value.push('\n');
                value.push_str(next);
                j += 1;
            } else if next.trim().is_empty() {
                // blank: belongs to the entry only if an indented line follows
                let mut k = j;
                while k < lines.len() && lines[k].trim().is_empty() {
                    k += 1;
                }
                if k < lines.len() && (lines[k].starts_with(' ') || lines[k].starts_with('\t')) {
                    for _ in j..k {
                        value.push('\n');
                    }
                    j = k;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        if !valid_key(key) {
            diags.push(Diagnostic::new(
                Severity::Error,
                file,
                number,
                key,
                "syntax",
                "invalid key".to_owned(),
            ));
        } else {
            match parse_pattern(&value) {
                Ok(pattern) => entries.push(Entry {
                    key: key.to_owned(),
                    pattern,
                    source: value.trim_end().to_owned(),
                    line: number,
                }),
                Err(message) => diags.push(Diagnostic::new(
                    Severity::Error,
                    file,
                    number,
                    key,
                    "syntax",
                    message,
                )),
            }
        }
        i = j;
    }
    (entries, diags)
}

/// Braces still open at the end of `text` (string literals `"{"` do not count).
fn open_braces(text: &str) -> i32 {
    let (mut depth, mut quoted, mut escaped) = (0, false, false);
    for c in text.chars() {
        if quoted {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' if depth > 0 => quoted = true,
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn split_entry(line: &str) -> Option<(&str, &str)> {
    let eq = line.find('=')?;
    let key = line[..eq].trim_end();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    Some((key, &line[eq + 1..]))
}

/// Parse the value of one entry.
pub fn parse_pattern(source: &str) -> Result<Pattern, String> {
    let mut cursor = Cursor {
        chars: source.chars().collect(),
        at: 0,
    };
    let pattern = cursor.pattern(false)?;
    if cursor.at < cursor.chars.len() {
        return Err(format!("unexpected `{}`", cursor.chars[cursor.at]));
    }
    Ok(pattern)
}

struct Cursor {
    chars: Vec<char>,
    at: usize,
}

impl Cursor {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn starts_with(&self, text: &str) -> bool {
        text.chars()
            .enumerate()
            .all(|(i, c)| self.chars.get(self.at + i) == Some(&c))
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    /// After a newline (and its indentation): does a variant marker or the closing brace start
    /// the line? Returns the offset of that marker.
    fn marker_after_break(&self) -> bool {
        let mut k = self.at;
        while self.chars.get(k).is_some_and(|c| c.is_whitespace()) {
            k += 1;
        }
        match self.chars.get(k) {
            Some('}') | Some('[') => true,
            Some('*') => self.chars.get(k + 1) == Some(&'['),
            _ => false,
        }
    }

    /// A pattern. In a variant it ends before the next variant marker at a line start or a
    /// bare `}`; at the top it ends at the end of the text.
    fn pattern(&mut self, in_variant: bool) -> Result<Pattern, String> {
        let mut nodes: Vec<Node> = Vec::new();
        let mut text = String::new();
        loop {
            let Some(c) = self.peek() else { break };
            match c {
                '{' => {
                    if !text.is_empty() {
                        nodes.push(Node::Text(std::mem::take(&mut text)));
                    }
                    self.at += 1;
                    nodes.push(self.placeable()?);
                }
                '}' => {
                    if in_variant {
                        break;
                    }
                    return Err("unmatched `}` (write {\"}\"} for a brace)".into());
                }
                '\n' => {
                    if in_variant && self.marker_after_break() {
                        break;
                    }
                    // A line break inside the value; the indentation of the next line is dropped.
                    let mut breaks = 0;
                    while self.peek().is_some_and(char::is_whitespace) {
                        if self.peek() == Some('\n') {
                            breaks += 1;
                        }
                        self.at += 1;
                    }
                    for _ in 0..breaks {
                        text.push('\n');
                    }
                }
                _ => {
                    text.push(c);
                    self.at += 1;
                }
            }
        }
        if !text.is_empty() {
            nodes.push(Node::Text(text));
        }
        // `a {"{"} b` is three texts: join neighbours.
        let mut joined: Vec<Node> = Vec::with_capacity(nodes.len());
        for node in nodes {
            match (joined.last_mut(), node) {
                (Some(Node::Text(prev)), Node::Text(next)) => prev.push_str(&next),
                (_, node) => joined.push(node),
            }
        }
        let mut nodes = joined;
        // Trim the ends.
        if let Some(Node::Text(first)) = nodes.first_mut() {
            *first = first.trim_start().to_owned();
        }
        if let Some(Node::Text(last)) = nodes.last_mut() {
            *last = last.trim_end().to_owned();
        }
        nodes.retain(|n| !matches!(n, Node::Text(t) if t.is_empty()));
        Ok(Pattern { nodes })
    }

    fn ident(&mut self) -> Option<String> {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '@'))
        {
            self.at += 1;
        }
        (self.at > start).then(|| self.chars[start..self.at].iter().collect())
    }

    /// After `{`.
    fn placeable(&mut self) -> Result<Node, String> {
        self.skip_ws();
        let expr = self.expression()?;
        self.skip_ws();
        if self.starts_with("->") {
            self.at += 2;
            let Expr::Selector(var) = expr else {
                return Err("only $name or NUMBER($name) can select".into());
            };
            let (variants, default) = self.variants()?;
            return Ok(Node::Select {
                var,
                variants,
                default,
            });
        }
        if self.peek() != Some('}') {
            return Err(match self.peek() {
                Some(c) => format!("expected `}}`, found `{c}`"),
                None => "a `{` is never closed".into(),
            });
        }
        self.at += 1;
        Ok(match expr {
            Expr::Node(n) => n,
            Expr::Selector(var) => Node::Var(var),
        })
    }

    fn expression(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some('$') => {
                self.at += 1;
                let name = self.ident().ok_or("expected a name after `$`")?;
                Ok(Expr::Selector(name))
            }
            Some('"') => {
                self.at += 1;
                let mut s = String::new();
                loop {
                    match self.peek() {
                        None => return Err("a string literal is never closed".into()),
                        Some('"') => {
                            self.at += 1;
                            break;
                        }
                        Some('\\') => {
                            self.at += 1;
                            match self.peek() {
                                Some('"') => s.push('"'),
                                Some('\\') => s.push('\\'),
                                Some('u') => {
                                    let hex: String =
                                        self.chars[self.at + 1..].iter().take(4).collect();
                                    let c = u32::from_str_radix(&hex, 16)
                                        .ok()
                                        .and_then(char::from_u32)
                                        .ok_or("bad \\u escape")?;
                                    s.push(c);
                                    self.at += 4;
                                }
                                _ => return Err("unknown escape in a string literal".into()),
                            }
                            self.at += 1;
                        }
                        Some(c) => {
                            s.push(c);
                            self.at += 1;
                        }
                    }
                }
                Ok(Expr::Node(Node::Text(s)))
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let name = self.ident().ok_or("expected a name")?;
                if self.peek() == Some('(') {
                    self.at += 1;
                    self.skip_ws();
                    if self.peek() != Some('$') {
                        return Err(format!("{name}() takes an argument like $n"));
                    }
                    self.at += 1;
                    let var = self.ident().ok_or("expected a name after `$`")?;
                    let (mut min_fraction, mut max_fraction) = (0u8, 3u8);
                    self.skip_ws();
                    while self.peek() == Some(',') {
                        self.at += 1;
                        self.skip_ws();
                        let option = self.ident().ok_or("expected an option name")?;
                        self.skip_ws();
                        if self.peek() != Some(':') {
                            return Err("expected `:` after the option name".into());
                        }
                        self.at += 1;
                        self.skip_ws();
                        let start = self.at;
                        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                            self.at += 1;
                        }
                        let value: u8 = self.chars[start..self.at]
                            .iter()
                            .collect::<String>()
                            .parse()
                            .map_err(|_| "expected a number")?;
                        match option.as_str() {
                            "minimumFractionDigits" => min_fraction = value.min(9),
                            "maximumFractionDigits" => max_fraction = value.min(9),
                            other => return Err(format!("unknown option {other}")),
                        }
                        self.skip_ws();
                    }
                    if self.peek() != Some(')') {
                        return Err(format!("expected `)` after {name}"));
                    }
                    self.at += 1;
                    max_fraction = max_fraction.max(min_fraction);
                    match name.as_str() {
                        "NUMBER" => {
                            // A selector too: `{ NUMBER($n) -> ... }`.
                            self.skip_ws();
                            if self.starts_with("->") {
                                Ok(Expr::Selector(var))
                            } else {
                                Ok(Expr::Node(Node::Number {
                                    var,
                                    min_fraction,
                                    max_fraction,
                                }))
                            }
                        }
                        "DATE" => Ok(Expr::Node(Node::Date(var))),
                        other => Err(format!("unknown function {other}")),
                    }
                } else {
                    Ok(Expr::Node(Node::Ref(name)))
                }
            }
            Some(c) => Err(format!("unexpected `{c}` in a placeable")),
            None => Err("a `{` is never closed".into()),
        }
    }

    fn variants(&mut self) -> Result<(Vec<Variant>, usize), String> {
        let mut variants = Vec::new();
        let mut default = None;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('}') => {
                    self.at += 1;
                    break;
                }
                None => return Err("a select is never closed".into()),
                _ => {}
            }
            let star = if self.peek() == Some('*') {
                self.at += 1;
                true
            } else {
                false
            };
            if self.peek() != Some('[') {
                return Err("expected `[variant]`".into());
            }
            self.at += 1;
            let start = self.at;
            while self.peek().is_some_and(|c| c != ']' && c != '\n') {
                self.at += 1;
            }
            if self.peek() != Some(']') {
                return Err("a variant key is never closed".into());
            }
            let key: String = self.chars[start..self.at]
                .iter()
                .collect::<String>()
                .trim()
                .to_owned();
            self.at += 1;
            let key = match key.parse::<i64>() {
                Ok(n) => VariantKey::Number(n),
                Err(_)
                    if !key.is_empty()
                        && key
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) =>
                {
                    VariantKey::Name(key)
                }
                Err(_) => return Err(format!("bad variant key [{key}]")),
            };
            let pattern = self.pattern(true)?;
            if star {
                if default.is_some() {
                    return Err("two default variants".into());
                }
                default = Some(variants.len());
            }
            variants.push(Variant { key, pattern });
        }
        let default = default.ok_or("a select needs a default variant `*[other]`")?;
        Ok((variants, default))
    }
}

enum Expr {
    /// `$name` (or NUMBER($name) before `->`).
    Selector(String),
    Node(Node),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str) -> Pattern {
        parse_pattern(text).unwrap()
    }

    #[test]
    fn plain_text_and_variables() {
        assert_eq!(one("End turn").plain(), Some("End turn"));
        let p = one("Saved on turn { $turn } of {$total}");
        assert_eq!(p.variables(), ["total", "turn"]);
        assert_eq!(p.plain(), None);
    }

    #[test]
    fn selects_read_variants() {
        let p = one("{ $n ->\n    [one] { $n } turn\n   *[other] { $n } turns\n}");
        let [Node::Select {
            var,
            variants,
            default,
        }] = p.nodes.as_slice()
        else {
            panic!("{p:?}")
        };
        assert_eq!(var, "n");
        assert_eq!(variants.len(), 2);
        assert_eq!(*default, 1);
        assert_eq!(variants[0].key, VariantKey::Name("one".into()));
        assert_eq!(
            variants[1].pattern.nodes,
            [Node::Var("n".into()), Node::Text(" turns".into())]
        );
    }

    #[test]
    fn exact_numbers_and_number_function() {
        let p = one(
            "{ NUMBER($n) ->\n [0] none\n *[other] { NUMBER($n, minimumFractionDigits: 1) }\n}",
        );
        assert!(matches!(p.nodes[0], Node::Select { .. }), "{p:?}");
        let q = one("Score { NUMBER($points) }");
        assert_eq!(
            q.nodes[1],
            Node::Number {
                var: "points".into(),
                min_fraction: 0,
                max_fraction: 3
            }
        );
    }

    #[test]
    fn braces_and_references() {
        assert_eq!(one("a {\"{\"} b").plain(), Some("a { b"));
        let p = one("the { glossary.city } here");
        assert_eq!(p.references(), ["glossary.city"]);
        assert!(parse_pattern("oops }").is_err());
        assert!(parse_pattern("oops { $x").is_err());
        assert!(parse_pattern("{ $n -> [one] x }").is_err(), "no default");
    }

    #[test]
    fn multi_line_values_keep_line_breaks() {
        let (entries, diags) = parse_file(
            "t.ftl",
            "a = first\n    second\n\nb = x\n# note\nc.d = {\"\"}y",
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(entries[0].pattern.plain(), Some("first\nsecond"));
        assert_eq!(entries[1].key, "b");
        assert_eq!(entries[2].key, "c.d");
    }

    #[test]
    fn bad_entries_are_reported_not_fatal() {
        let (entries, diags) = parse_file(
            "t.ftl",
            "good = ok\nbad key = x\n= nothing\nalso-good = fine\nbroken = { $x\n",
        );
        let keys: Vec<_> = entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["good", "also-good"]);
        assert_eq!(diags.len(), 3, "{diags:?}");
    }

    #[test]
    fn scoped_keys() {
        assert!(valid_key("shell.turn@lal"));
        assert!(valid_key("classic.unit.anti-aircraft-gun.name"));
        assert!(!valid_key("1abc"));
        assert!(!valid_key("a..b"));
        assert!(!valid_key("a@"));
    }

    #[test]
    fn variants_keep_text_with_brackets_inside() {
        let p = one("{ $n ->\n  [one] a [b] c\n *[other] d\n}");
        let [Node::Select { variants, .. }] = p.nodes.as_slice() else {
            panic!()
        };
        assert_eq!(variants[0].pattern.plain(), Some("a [b] c"));
    }
}
