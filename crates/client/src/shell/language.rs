//! The interface language: which bundles exist, which one the player chose (or `auto`:
//! the system's), and applying it to the catalogue. The player's own bundles are read from
//! `<config>/lang/<code>/*.ftl` (platform/docs/localization.md).

use std::path::Path;
use std::sync::OnceLock;

use i18n::{Catalog, Diagnostic, LanguageInfo, Severity};

/// The setting that follows the system language.
pub const AUTO: &str = "auto";

static OS_LOCALES: OnceLock<Vec<String>> = OnceLock::new();

/// The platform layer that cannot read environment variables (the browser's
/// `navigator.languages`, Android's locale list) tells the shell what the system asks for.
pub fn set_os_locales(tags: Vec<String>) {
    let _ = OS_LOCALES.set(
        tags.iter()
            .filter_map(|t| i18n::locale::normalize(t))
            .collect(),
    );
}

/// The languages the system asks for, most wanted first (empty when unknown).
pub fn os_locales() -> Vec<String> {
    if let Some(tags) = OS_LOCALES.get() {
        return tags.clone();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        #[allow(unused_mut)]
        let mut tags = i18n::locale::from_env(|name| std::env::var(name).ok());
        #[cfg(target_os = "macos")]
        if tags.is_empty() {
            tags = macos_languages();
        }
        #[cfg(windows)]
        if tags.is_empty() {
            tags = windows_languages();
        }
        tags
    }
    #[cfg(target_arch = "wasm32")]
    {
        Vec::new()
    }
}

/// `defaults read -g AppleLanguages` prints a plist-style list: ("de-DE", "en-US").
#[cfg(target_os = "macos")]
fn macos_languages() -> Vec<String> {
    let Ok(out) = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .filter_map(i18n::locale::normalize)
        .collect()
}

/// The user's locale name from the system ("de-DE"); kernel32 is linked by std already.
#[cfg(windows)]
fn windows_languages() -> Vec<String> {
    extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
    }
    let mut buffer = [0u16; 85];
    // SAFETY: the buffer is LOCALE_NAME_MAX_LENGTH (85) wide, as the call documents.
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if written <= 1 {
        return Vec::new();
    }
    String::from_utf16(&buffer[..written as usize - 1])
        .ok()
        .and_then(|name| i18n::locale::normalize(&name))
        .into_iter()
        .collect()
}

/// The tag a choice stands for: `auto` negotiates the system's languages with the bundles,
/// a tag is used if a bundle (or a family fallback of it) exists, else English.
/// `AURELIA_LANG` in the environment wins over the choice (screenshots, tests).
pub fn resolve(choice: &str, catalog: &Catalog) -> String {
    let choice = std::env::var("AURELIA_LANG")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| choice.to_owned());
    let available = catalog.tags();
    if choice == AUTO {
        return i18n::locale::negotiate(&os_locales(), &available);
    }
    match i18n::locale::normalize(&choice) {
        Some(tag) => i18n::locale::negotiate(&[tag], &available),
        None => i18n::locale::BASE.to_owned(),
    }
}

/// Make `choice` the active language. Returns the tag now in use.
pub fn apply(choice: &str) -> String {
    let tag = resolve(choice, &i18n::active_catalog());
    i18n::set_language(&tag)
}

/// Add the player's bundles from `<config dir>/lang` to the built-in ones. Returns what was
/// wrong with them (never fatal); errors are also printed once.
pub fn install_bundles(config_dir: &Path) -> Vec<Diagnostic> {
    let mut catalog = i18n::builtin::catalog();
    let mut notes = i18n::external::load_dir(&mut catalog, &config_dir.join("lang"));
    notes.extend(
        catalog
            .parse_all()
            .into_iter()
            .filter(|d| !matches!(d.kind, "missing")),
    );
    // The checks against English apply to the player's bundles only: the built-in ones are
    // checked by the tests.
    let en = catalog.bundle(i18n::locale::BASE);
    for info in catalog.languages(false) {
        if let (i18n::Origin::External(_), Some(en)) = (&info.origin, &en) {
            if info.tag != i18n::locale::BASE {
                if let Some(bundle) = catalog.bundle(&info.tag) {
                    notes.extend(
                        i18n::validate::validate(&bundle, en, &catalog.translator(&info.tag))
                            .into_iter()
                            .filter(|d| d.severity >= Severity::Warning),
                    );
                }
            }
        }
    }
    for note in notes.iter().filter(|n| n.severity == Severity::Error) {
        eprintln!("language bundle: {note}");
    }
    i18n::set_catalog(catalog);
    notes
}

/// The languages for the selector, kept until the catalogue changes.
#[derive(Default)]
pub struct LanguageList {
    generation: Option<u64>,
    with_pseudo: bool,
    items: Vec<LanguageInfo>,
}

impl LanguageList {
    pub fn get(&mut self, with_pseudo: bool) -> &[LanguageInfo] {
        let generation = i18n::generation();
        if self.generation != Some(generation) || self.with_pseudo != with_pseudo {
            self.items = i18n::active_catalog().languages(with_pseudo);
            self.generation = Some(generation);
            self.with_pseudo = with_pseudo;
        }
        &self.items
    }
}

/// The name of a language in the selector: its own name, marked when it comes from the
/// player's folder.
pub fn display_name(info: &LanguageInfo, custom_mark: &str) -> String {
    match info.origin {
        i18n::Origin::External(_) => format!("{} ({custom_mark})", info.name),
        i18n::Origin::Builtin => info.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A player's folder adds a language and overrides a message of a built-in one, and a
    /// broken file in it is reported, not fatal.
    #[test]
    fn the_players_bundles_are_installed_next_to_the_built_in_ones() {
        let dir = std::env::temp_dir().join(format!("aurelia-lang-test-{}", std::process::id()));
        let lang = dir.join("lang");
        std::fs::create_dir_all(lang.join("tlh")).unwrap();
        std::fs::create_dir_all(lang.join("ru")).unwrap();
        std::fs::write(
            lang.join("tlh/meta.ftl"),
            "meta.name = tlhIngan\nmeta.english = Klingon\n",
        )
        .unwrap();
        std::fs::write(
            lang.join("tlh/shell.ftl"),
            "shell.end-turn = Qapla\nbroken = { $x\n",
        )
        .unwrap();
        std::fs::write(lang.join("ru/mine.ftl"), "shell.back = \u{412}\u{435}\u{440}\u{43d}\u{443}\u{442}\u{44c}\u{441}\u{44f}\n").unwrap(); // "shell.back" = "Back" in Russian
        let notes = install_bundles(&dir);
        assert!(
            notes.iter().any(|n| n.severity == Severity::Error),
            "the broken entry is reported: {notes:?}"
        );
        let tags: Vec<String> = i18n::active_catalog()
            .languages(false)
            .into_iter()
            .map(|l| l.tag)
            .collect();
        assert!(tags.contains(&"tlh".to_owned()), "{tags:?}");
        let (klingon, russian, english) = (
            i18n::with_language("tlh", || super::super::words::current().end_turn),
            i18n::with_language("ru", || super::super::words::current().back),
            i18n::with_language("tlh", || super::super::words::current().back),
        );
        assert_eq!(
            (klingon, russian, english),
            ("Qapla", "\u{412}\u{435}\u{440}\u{43d}\u{443}\u{442}\u{44c}\u{441}\u{44f}", "Back"), // Russian: "Back"
            "a missing message is English"
        );
        // Put the built-in catalogue back for the other tests of this process.
        std::fs::remove_dir_all(&dir).unwrap();
        install_bundles(&dir);
        assert!(!i18n::active_catalog()
            .languages(false)
            .iter()
            .any(|l| l.tag == "tlh"));
    }

    #[test]
    fn a_choice_resolves_through_the_bundles() {
        let catalog = i18n::builtin::catalog();
        assert_eq!(resolve("ru", &catalog), "ru");
        assert_eq!(
            resolve("ru-RU", &catalog),
            "ru",
            "a regional tag uses its language's bundle"
        );
        assert_eq!(resolve("klingon", &catalog), "en");
        assert_eq!(resolve("xx", &catalog), "en");
    }
}
