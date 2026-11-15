//! The line the main menu and the settings screen show for the version: it is built from the
//! build the launcher registered (`report::set_build`), and the game without a registered build
//! (a test, a tool) still has a line. One test, because the registration is once per process.
use client::shell::report::{set_build, version_line, Build};

#[test]
fn the_line_follows_the_registered_build() {
    i18n::with_language("en", || {
        // Nothing registered yet: the line names this crate's version and no commit.
        let unset = version_line();
        assert_eq!(unset, format!("Version {}", env!("CARGO_PKG_VERSION")));

        set_build("9.8.7", Some("0123456789abcdef"));
        assert_eq!(version_line(), "Version 9.8.7 (0123456)");
        assert_eq!(client::shell::report::build(), Build { version: "9.8.7", git: Some("0123456789abcdef") });
    });
    i18n::with_language("ru", || assert_eq!(version_line(), "Версия 9.8.7 (0123456)"));
}

#[test]
fn a_build_without_a_commit_shows_the_version_only() {
    assert_eq!(Build { version: "0.3.0", git: None }.label(), "0.3.0");
    assert_eq!(Build { version: "0.3.0", git: Some("abc") }.label(), "0.3.0 (abc)");
}
