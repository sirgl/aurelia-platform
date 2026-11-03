//! The catalogue end to end: fallback, plurals per language, scopes, genders, external
//! bundles (including broken ones) and the pseudo-locale.

use i18n::{Arg, Catalog, Origin, Severity};

fn catalog() -> Catalog {
    let mut c = Catalog::new();
    let files = |text: &str| vec![("main.ftl".to_owned(), text.to_owned())];
    c.add(
        "en",
        files(
            "meta.name = English\n\
             turn = Turn\n\
             turn@lal = Round\n\
             hello = Hello, { $name }!\n\
             only-en = Only English\n\
             turns = { $n ->\n    [0] No turns\n    [one] { $n } turn\n   *[other] { NUMBER($n) } turns\n}\n\
             owner = { $who ->\n    [fem] her city\n   *[other] their city\n}\n\
             term.city = city\n\
             founded = You founded a { term.city }.\n",
        ),
        Origin::Builtin,
    );
    c.add(
        "ru",
        files(
            "meta.name = \u{420}\u{443}\u{441}\u{441}\u{43a}\u{438}\u{439}\n\
             turn = \u{425}\u{43e}\u{434}\n\
             hello = \u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}, { $name }!\n\
             turns = { $n ->\n    [one] { $n } \u{445}\u{43e}\u{434}\n    [few] { $n } \u{445}\u{43e}\u{434}\u{430}\n    [many] { NUMBER($n) } \u{445}\u{43e}\u{434}\u{43e}\u{432}\n   *[other] { $n } \u{445}\u{43e}\u{434}\u{430}\n}\n",
        ),
        Origin::Builtin,
    );
    c.add("de", files("meta.name = Deutsch\nturn = Zug\nturns = { $n ->\n    [one] { $n } Zug\n   *[other] { NUMBER($n) } Züge\n}\n"), Origin::Builtin);
    c.add(
        "zh-Hans",
        files("meta.name = 简体中文\nturn = 回合\nturns = { $n } 回合\n"),
        Origin::Builtin,
    );
    c
}

fn text(c: &Catalog, tag: &str, key: &str, args: &[(&str, Arg)]) -> String {
    c.translator(tag)
        .format(key, args)
        .unwrap_or_else(|| format!("<missing {key}>"))
}

#[test]
fn a_missing_message_falls_back_through_the_family_to_english() {
    let c = catalog();
    assert_eq!(
        text(&c, "de-AT", "turn", &[]),
        "Zug",
        "de-AT uses the de bundle"
    );
    assert_eq!(
        text(&c, "de-AT", "only-en", &[]),
        "Only English",
        "a message de lacks is English"
    );
    assert_eq!(
        text(&c, "fr", "turn", &[]),
        "Turn",
        "no bundle at all is English"
    );
    assert_eq!(c.translator("zh-CN").chain_tags(), ["zh-Hans", "en"]);
    assert_eq!(c.translator("zh-TW").chain_tags(), ["zh-Hans", "en"]);
}

#[test]
fn plurals_follow_each_language() {
    let c = catalog();
    let n = |tag: &str, n: i64| text(&c, tag, "turns", &[("n", n.into())]);
    assert_eq!(
        [n("en", 0), n("en", 1), n("en", 2)],
        ["No turns", "1 turn", "2 turns"]
    );
    assert_eq!(
        [
            n("ru", 1),
            n("ru", 3),
            n("ru", 5),
            n("ru", 21),
            n("ru", 112)
        ],
        ["1 \u{445}\u{43e}\u{434}", "3 \u{445}\u{43e}\u{434}\u{430}", "5 \u{445}\u{43e}\u{434}\u{43e}\u{432}", "21 \u{445}\u{43e}\u{434}", "112 \u{445}\u{43e}\u{434}\u{43e}\u{432}"]
    );
    assert_eq!(
        [n("de", 1), n("de", 1234)],
        ["1 Zug", "1.234 Züge"],
        "German groups thousands with a dot"
    );
    assert_eq!(n("zh-Hans", 7), "7 回合");
    // Numbers are grouped by the language of the text that is shown.
    assert_eq!(n("ru", 12345), "12\u{a0}345 \u{445}\u{43e}\u{434}\u{43e}\u{432}");
}

#[test]
fn a_game_module_overrides_single_messages() {
    let c = catalog();
    let en = c.translator("en");
    assert_eq!(en.format("turn", &[]).unwrap(), "Turn");
    assert_eq!(en.format_in(Some("lal"), "turn", &[]).unwrap(), "Round");
    assert_eq!(
        en.format_in(Some("classic"), "turn", &[]).unwrap(),
        "Turn",
        "another module's scope changes nothing"
    );
    // A language without the override shows its own base text, not the English override.
    let ru = c.translator("ru");
    assert_eq!(ru.format_in(Some("lal"), "turn", &[]).unwrap(), "\u{425}\u{43e}\u{434}");
}

#[test]
fn arguments_selects_on_text_and_references() {
    let c = catalog();
    assert_eq!(
        text(&c, "ru", "hello", &[("name", "\u{41c}\u{438}\u{440}".into())]),
        "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}, \u{41c}\u{438}\u{440}!"
    );
    assert_eq!(
        text(&c, "en", "owner", &[("who", "fem".into())]),
        "her city"
    );
    assert_eq!(
        text(&c, "en", "owner", &[("who", "x".into())]),
        "their city"
    );
    assert_eq!(text(&c, "en", "founded", &[]), "You founded a city.");
    assert_eq!(
        text(&c, "en", "hello", &[]),
        "Hello, {$name}!",
        "a missing argument stays visible"
    );
}

#[test]
fn the_pseudo_locale_wraps_english() {
    let c = catalog();
    let t = text(&c, "en-XA", "hello", &[("name", "Bob".into())]);
    assert!(t.starts_with("[Ĥéļļö, Bob") && t.ends_with(']'), "{t}");
    assert!(c.tags().contains(&"en-XA".to_owned()));
}

#[test]
fn languages_list_names_and_put_english_first() {
    let c = catalog();
    let names: Vec<(String, String)> = c
        .languages(false)
        .into_iter()
        .map(|l| (l.tag, l.name))
        .collect();
    assert_eq!(names[0], ("en".to_owned(), "English".to_owned()));
    assert!(names.contains(&("zh-Hans".to_owned(), "简体中文".to_owned())));
    assert_eq!(c.languages(true).last().unwrap().tag, "en-XA");
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("i18n-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn external_bundles_add_languages_and_override_messages_without_panicking() {
    let dir = temp_dir("external");
    let write = |rel: &str, text: &str| {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write("fr/meta.ftl", "meta.name = Français\n");
    write(
        "fr/main.ftl",
        "turn = Tour\nbroken = { $x\nhello = Bonjour, { $name } !\nstray line\n",
    );
    write("fr/STATUS", "community draft\nsecond line\n");
    write("de/override.ftl", "turn = Zug (Mod)\n");
    write("not a tag/x.ftl", "turn = nope\n");
    write("it/readme.txt", "no bundle here");
    std::fs::write(dir.join("loose-file"), "x").unwrap();

    let mut c = catalog();
    let diags = i18n::external::load_dir(&mut c, &dir);
    assert!(
        diags
            .iter()
            .any(|d| d.message.contains("not a language tag")),
        "{diags:?}"
    );
    // The broken entries are reported when the bundle is read, the good ones work.
    let (fr, parse) = c.bundle_with_diagnostics("fr").unwrap();
    assert!(
        parse
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
            >= 2,
        "{parse:?}"
    );
    assert_eq!(fr.meta.name.as_deref(), Some("Français"));
    assert_eq!(text(&c, "fr", "turn", &[]), "Tour");
    assert_eq!(
        text(&c, "fr", "hello", &[("name", "Zoé".into())]),
        "Bonjour, Zoé !"
    );
    assert_eq!(text(&c, "fr", "only-en", &[]), "Only English");
    assert_eq!(c.status("fr").as_deref(), Some("community draft"));
    // A folder of a built-in language overrides single messages and leaves the rest.
    assert_eq!(text(&c, "de", "turn", &[]), "Zug (Mod)");
    assert_eq!(text(&c, "de", "turns", &[("n", 2.into())]), "2 Züge");
    let info = c
        .languages(false)
        .into_iter()
        .find(|l| l.tag == "fr")
        .unwrap();
    assert!(matches!(info.origin, Origin::External(_)));
    // A folder that does not exist is no problem.
    assert!(i18n::external::load_dir(&mut c, &dir.join("missing")).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_non_utf8_file_is_reported_not_fatal() {
    let dir = temp_dir("binary");
    std::fs::create_dir_all(dir.join("es")).unwrap();
    std::fs::write(dir.join("es/bad.ftl"), [0xff, 0xfe, 0x00, 0x61]).unwrap();
    std::fs::write(dir.join("es/ok.ftl"), "turn = Turno\n").unwrap();
    let mut c = catalog();
    let diags = i18n::external::load_dir(&mut c, &dir);
    assert!(
        diags.iter().any(|d| d.message.contains("UTF-8")),
        "{diags:?}"
    );
    assert_eq!(text(&c, "es", "turn", &[]), "Turno");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_validator_reports_a_whole_bundle() {
    let c = catalog();
    let en = c.bundle("en").unwrap();
    let ru = c.bundle("ru").unwrap();
    let report = i18n::validate::validate(&ru, &en, &c.translator("ru"));
    let errors: Vec<_> = report
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(report
        .iter()
        .any(|d| d.kind == "missing" && d.key == "only-en"));
}
