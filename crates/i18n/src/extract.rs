//! Finding the keys the code uses: `t!("k")`, `tf!("k", ...)`, `key!("k")`, `tr_args("k"`
//! and `// i18n-keys: a.b, c.d.*` for keys built at run time. Used by the checker and by the
//! test that every key used in code exists in the base language.

use std::collections::BTreeMap;

const CALLS: &[&str] = &["t!(", "tf!(", "key!(", "tr_args(", "tr_dynamic(", "tr("];

/// A use of a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    pub key: String,
    pub line: u32,
}

/// The literal keys used in `source`, and the `i18n-keys:` declarations (a trailing `*` is a prefix).
pub fn keys_in(source: &str) -> Vec<Use> {
    let mut out = Vec::new();
    for (i, line) in source.lines().enumerate() {
        let number = i as u32 + 1;
        // Documentation shows example calls; they use no key.
        if line.trim_start().starts_with("///") || line.trim_start().starts_with("//!") {
            continue;
        }
        let code = match line.find("//") {
            Some(at) if !line[..at].contains('"') => {
                if let Some(rest) = line[at..].split("i18n-keys:").nth(1) {
                    for item in rest.split(',') {
                        let item = item.trim();
                        if !item.is_empty() {
                            out.push(Use {
                                key: item.to_owned(),
                                line: number,
                            });
                        }
                    }
                }
                &line[..at]
            }
            _ => line,
        };
        for call in CALLS {
            let mut from = 0;
            while let Some(at) = code[from..].find(call) {
                let start = from + at;
                from = start + call.len();
                // `t!(` must not be the end of a longer name (`format!(`, `assert!(`).
                if call.ends_with("!(") || *call == "tr(" {
                    let prev = code[..start].chars().next_back();
                    if prev.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                        continue;
                    }
                }
                let rest = code[from..].trim_start();
                if let Some(rest) = rest.strip_prefix('"') {
                    if let Some(end) = rest.find('"') {
                        let key = &rest[..end];
                        if crate::syntax::valid_key(key) && key.contains('.') {
                            out.push(Use {
                                key: key.to_owned(),
                                line: number,
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

/// Every key used under `dirs` (all `.rs` files), with the files that use it.
pub fn scan_dirs(dirs: &[&std::path::Path]) -> BTreeMap<String, Vec<(String, u32)>> {
    let mut out: BTreeMap<String, Vec<(String, u32)>> = BTreeMap::new();
    let mut stack: Vec<std::path::PathBuf> = dirs.iter().map(|d| d.to_path_buf()).collect();
    while let Some(path) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in read.flatten() {
            let p = entry.path();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if p.is_dir() {
                // The i18n crate's own tests use made-up keys.
                if name != "target"
                    && !name.starts_with('.')
                    && !(name == "i18n" && p.ends_with("crates/i18n"))
                {
                    stack.push(p);
                }
            } else if name.ends_with(".rs") {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    for u in keys_in(&text) {
                        out.entry(u.key)
                            .or_default()
                            .push((p.display().to_string(), u.line));
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_calls_and_declarations() {
        let src = r#"
let a = t!("shell.menu.quit");
let b = tf!("shell.saved", turn = 3);
let c = format!("not.a.key");
const K: &str = key!("shell.hud.turns");
// i18n-keys: lal.law.*, lal.event.a
let d = i18n::tr_args("shell.x.y", &[]);
"#;
        let keys: Vec<String> = keys_in(src).into_iter().map(|u| u.key).collect();
        assert_eq!(
            keys,
            [
                "shell.menu.quit",
                "shell.saved",
                "shell.hud.turns",
                "lal.law.*",
                "lal.event.a",
                "shell.x.y"
            ]
        );
    }
}
