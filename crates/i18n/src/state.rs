//! The active language of the process, and the lookups the code uses: `t!("key")` for a
//! text, `tf!("key", name = value)` for a text with arguments.
//!
//! One language is active at a time (the settings screen switches it). Tests and tools
//! can look at another one on the current thread only (`with_language`), so tests that run
//! in parallel do not disturb each other. Nothing here needs a window or the filesystem.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use crate::arg::Arg;
use crate::builtin;
use crate::catalog::{Catalog, Translator};
use crate::locale;

struct State {
    catalog: Arc<Catalog>,
    /// The language as chosen (`de`, `pt-BR`); the translator walks its fallback chain.
    tag: String,
    /// The game module whose overrides apply when the thread names none.
    scope: Option<&'static str>,
    translator: Arc<Translator>,
    generation: u64,
}

fn state() -> &'static RwLock<State> {
    static STATE: OnceLock<RwLock<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        let catalog = Arc::new(builtin::catalog());
        let translator = Arc::new(catalog.translator(locale::BASE));
        RwLock::new(State {
            catalog,
            tag: locale::BASE.to_owned(),
            scope: None,
            translator,
            generation: 0,
        })
    })
}

thread_local! {
    static OVERRIDE: RefCell<Vec<Arc<Translator>>> = const { RefCell::new(Vec::new()) };
}

fn missing() -> &'static Mutex<BTreeSet<String>> {
    static MISSING: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    MISSING.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Keys asked for that no bundle in the chain had, since the start (sorted). A test or
/// the debug overlay can assert it is empty.
pub fn missing_keys() -> Vec<String> {
    missing().lock().unwrap().iter().cloned().collect()
}

fn note_missing(key: &str) {
    missing().lock().unwrap().insert(key.to_owned());
}

/// The catalogue in use.
pub fn catalog() -> Arc<Catalog> {
    state().read().unwrap().catalog.clone()
}

/// Replace the catalogue (after external bundles were added) and re-resolve the language.
pub fn set_catalog(catalog: Catalog) {
    let mut s = state().write().unwrap();
    s.catalog = Arc::new(catalog);
    resolve(&mut s);
}

fn resolve(s: &mut State) {
    s.translator = Arc::new(s.catalog.translator(&s.tag));
    s.generation += 1;
}

/// Make `tag` the language (its fallback chain is used). Returns the tag now active.
pub fn set_language(tag: &str) -> String {
    let mut s = state().write().unwrap();
    let tag = locale::normalize(tag).unwrap_or_else(|| locale::BASE.to_owned());
    if s.tag != tag {
        s.tag = tag;
        resolve(&mut s);
    }
    s.tag.clone()
}

/// The active language tag.
pub fn language() -> String {
    OVERRIDE
        .with(|o| o.borrow().last().map(|t| t.tag.clone()))
        .unwrap_or_else(|| state().read().unwrap().tag.clone())
}

/// The overrides of the game module `scope` apply everywhere that no thread names its own
/// (`shell.turn@lal` replaces `shell.turn`).
pub fn set_scope(scope: Option<&'static str>) {
    let mut s = state().write().unwrap();
    if s.scope != scope {
        s.scope = scope;
        s.generation += 1;
    }
}

thread_local! {
    static SCOPE: RefCell<Vec<Option<&'static str>>> = const { RefCell::new(Vec::new()) };
}

/// The scope in force on this thread.
pub fn scope() -> Option<&'static str> {
    SCOPE
        .with(|s| s.borrow().last().copied())
        .unwrap_or_else(|| state().read().unwrap().scope)
}

/// What a cache of rendered text must key on: the language, the module scope and a counter
/// that changes whenever the catalogue, the global language or the global scope changes.
pub fn context() -> (String, Option<&'static str>, u64) {
    (language(), scope(), generation())
}

/// Changes when the language, the global scope or the catalogue changes.
pub fn generation() -> u64 {
    state().read().unwrap().generation
}

/// Guard of [`enter_scope`].
pub struct ScopeGuard;

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        SCOPE.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

/// The module `scope` is the one drawing on this thread until the guard drops: the shell
/// enters its module's scope around a frame, so tests of two modules can run side by side.
pub fn enter_scope(scope: Option<&'static str>) -> ScopeGuard {
    SCOPE.with(|s| s.borrow_mut().push(scope));
    ScopeGuard
}

/// Run `f` with `tag` as the language of the current thread (tests, tools, previews).
pub fn with_language<R>(tag: &str, f: impl FnOnce() -> R) -> R {
    let translator = Arc::new(catalog().translator(tag));
    OVERRIDE.with(|o| o.borrow_mut().push(translator));
    struct Pop;
    impl Drop for Pop {
        fn drop(&mut self) {
            OVERRIDE.with(|o| {
                o.borrow_mut().pop();
            });
        }
    }
    let _pop = Pop;
    f()
}

/// Run `f` with `tag` and the module `scope` on the current thread.
pub fn with_scope<R>(tag: &str, scope: Option<&'static str>, f: impl FnOnce() -> R) -> R {
    let _guard = enter_scope(scope);
    with_language(tag, f)
}

fn current() -> Arc<Translator> {
    OVERRIDE
        .with(|o| o.borrow().last().cloned())
        .unwrap_or_else(|| state().read().unwrap().translator.clone())
}

/// The text of `key` in the active language. A key that does not exist is shown as itself
/// (and noted in `missing_keys`): a visible bug, not a crash.
pub fn tr(key: &'static str) -> &'static str {
    match current().text_in(scope(), key) {
        Some(text) => text,
        None => {
            note_missing(key);
            key
        }
    }
}

/// The text of `key` with named arguments.
pub fn tr_args(key: &str, args: &[(&str, Arg)]) -> String {
    match current().format_in(scope(), key, args) {
        Some(text) => text,
        None => {
            note_missing(key);
            key.to_owned()
        }
    }
}

/// The text of a key built at run time, None when unknown (no note is made: a content name
/// falls back to the rules' own text).
pub fn tr_dynamic(key: &str, args: &[(&str, Arg)]) -> Option<String> {
    current().format_in(scope(), key, args)
}

/// A number in the active language's conventions: `number(1234.5, 0, 2)` is `1,234.5` in
/// English, `1.234,5` in German, `1 234,5` in Russian (narrow no-break space). Use it for a
/// quantity the player reads that is not inside a message; inside a message write
/// `{ NUMBER($n) }`.
pub fn number(value: f64, min_fraction: u8, max_fraction: u8) -> String {
    current()
        .numbers()
        .format(value, min_fraction, max_fraction)
}

/// A whole number in the active language's conventions (`number(n, 0, 0)`).
pub fn number_int(value: i64) -> String {
    current().numbers().format_int(value)
}

/// The active language's decimal separator (`.` or `,`).
pub fn decimal_separator() -> &'static str {
    current().numbers().decimal
}

/// Whether the active language chain has `key`.
pub fn has(key: &str) -> bool {
    current().find_in(scope(), key).is_some()
}

/// The stable key of a text that belongs to rule data: `<module>.<kind>.<id>.<field>`.
/// The id is lowercased; a leading `core:` and a table path (`unit/`) go; an `UPPER_SNAKE`
/// id loses its kind prefix (`UNIT_SCOUT` -> `scout`); `_` and `/` become `-` and `.`.
pub fn content_key(module: &str, kind: &str, id: &str, field: &str) -> String {
    format!("{module}.{kind}.{}.{field}", content_slug(id))
}

/// The id part of a content key (see [`content_key`]): the namespace before `:` goes too.
pub fn content_slug(id: &str) -> String {
    // A leading namespace (`core:`, `lal:`, `sandbox:`) is not part of the slug.
    let id = id.split_once(':').map_or(id, |(_, rest)| rest);
    let id = match id.split_once('/') {
        Some((_table, rest)) if !rest.is_empty() => rest,
        _ => id,
    };
    let id = if !id.contains('/') && id.contains('_') && id.chars().all(|c| !c.is_ascii_lowercase())
    {
        id.split_once('_').map(|(_, rest)| rest).unwrap_or(id)
    } else {
        id
    };
    id.to_ascii_lowercase()
        .replace('/', ".")
        .replace('_', "-")
        .replace(':', "-")
        .replace(' ', "-")
}

/// The name or description of a piece of rule data in the active language, or `fallback`
/// (the text the rules carry) when no bundle has it.
pub fn content(module: &str, kind: &str, id: &str, field: &str, fallback: &str) -> String {
    let key = content_key(module, kind, id, field);
    tr_dynamic(&key, &[]).unwrap_or_else(|| fallback.to_owned())
}

/// `tr("key")`: a text.
#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::tr($key)
    };
}

/// `tf!("key", name = value, ...)`: a text with named arguments, as a `String`.
#[macro_export]
macro_rules! tf {
    ($key:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::tr_args($key, &[$((stringify!($name), $crate::Arg::from($value))),*])
    };
}

/// `key!("key")`: a key the code stores to look up later (`i18n::tr_args(key, ...)`); the
/// scanner knows it is used.
#[macro_export]
macro_rules! key {
    ($key:literal) => {
        $key
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_keys_are_stable_slugs() {
        assert_eq!(
            content_slug("core:unit/anti-aircraft-gun"),
            "anti-aircraft-gun"
        );
        assert_eq!(
            content_slug("core:policy/aesthetics/opener"),
            "aesthetics.opener"
        );
        assert_eq!(content_slug("UNIT_SCOUT"), "scout");
        assert_eq!(
            content_slug("lal:UNIT_SCOUT"),
            "scout",
            "the kernel keeps the module prefix on its rows"
        );
        assert_eq!(content_slug("TECH_BRONZE_WORKING"), "bronze-working");
        assert_eq!(content_slug("core:nation/america"), "america");
        assert_eq!(content_slug("core:nation/america"), "america");
        assert_eq!(
            content_key("lal", "unit", "UNIT_SCOUT", "name"),
            "lal.unit.scout.name"
        );
    }

    #[test]
    fn a_missing_key_is_itself_and_noted() {
        assert_eq!(tr("test.state.no-such-key"), "test.state.no-such-key");
        assert!(missing_keys().iter().any(|k| k == "test.state.no-such-key"));
        assert_eq!(
            content("classic", "unit", "core:unit/zzz", "name", "Zzz"),
            "Zzz"
        );
    }
}
