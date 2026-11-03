//! "Report a bug": the dialog behind the menu button, the text it puts into a GitHub issue and
//! the report file the player attaches to it (docs/bug-reports.md of the game).
//!
//! Nothing is sent. The dialog shows the issue text, the button opens GitHub's "new issue"
//! page with that text filled in (`url`), and the player presses Submit there. The report file
//! (`bundle`) is a `.zip` with the same facts and a replayable game; the player drags it into the
//! issue. A crash in the browser shows the same two buttons (`crash`).
pub mod bundle;
mod crash;
mod dialog;
pub mod info;
pub mod url;

pub use crash::{install_panic_recorder, last_panic, last_trace, log_tail, push_log, remember, set_panic_observer, set_trace_source, wants_snapshots, Crash, DialogWords};
pub use dialog::{dialog_words, overlay, ReportUi};
pub use info::{GameInfo, ReportInfo};

use std::sync::OnceLock;

/// The build the launcher registers: its version (`CARGO_PKG_VERSION` of the launcher crate) and
/// the commit when the build knows it (`option_env!("AURELIA_GIT_HASH")`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Build {
    pub version: &'static str,
    pub git: Option<&'static str>,
}

static BUILD: OnceLock<Build> = OnceLock::new();
static PAGE: OnceLock<fn() -> PageFacts> = OnceLock::new();

/// A launcher says which build this is (call once, before the first frame).
pub fn set_build(version: &'static str, git: Option<&'static str>) {
    let _ = BUILD.set(Build { version, git: git.filter(|hash| !hash.is_empty()) });
}

/// What the browser page knows about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageFacts {
    /// `navigator.userAgent`.
    pub agent: Option<String>,
    /// The files of the build the page loaded (the wasm and the art), so the right symbols file
    /// can read a panic trace.
    pub build: Option<String>,
}

/// The browser launcher says how to ask the page about itself (called at report time).
pub fn set_page_source(source: fn() -> PageFacts) {
    let _ = PAGE.set(source);
}

/// The registered build, else this crate's own version.
pub fn build() -> Build {
    BUILD.get().copied().unwrap_or(Build { version: env!("CARGO_PKG_VERSION"), git: None })
}

pub(crate) fn page() -> PageFacts {
    PAGE.get().map(|source| source()).unwrap_or_default()
}

/// Where a finished report file went: a path the player can copy, or a download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    /// What the dialog says: the full path, or the name of the downloaded file.
    pub place: String,
    /// The path to copy (None in a browser, which has no folders to name).
    pub path: Option<String>,
}

/// Hands a report file to the player where the platform can: a launcher without a file system
/// (the browser) registers one.
pub type FileSink = fn(name: &str, bytes: &[u8]) -> Result<Delivered, String>;

static SINK: OnceLock<FileSink> = OnceLock::new();

pub fn set_file_sink(sink: FileSink) {
    let _ = SINK.set(sink);
}

/// Writes the file where the player can find it: `reports/` in the saves folder, or the
/// registered sink.
pub fn deliver(name: &str, bytes: &[u8]) -> Result<Delivered, String> {
    if let Some(sink) = SINK.get() {
        return sink(name, bytes).map_err(|e| i18n::tf!("shell.report.cannot-write", error = e));
    }
    let dir = super::saves::save_directory().join("reports");
    std::fs::create_dir_all(&dir).map_err(|e| i18n::tf!("shell.report.cannot-write", error = e.to_string()))?;
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| i18n::tf!("shell.report.cannot-write", error = e.to_string()))?;
    let full = std::path::absolute(&path).unwrap_or(path).display().to_string();
    Ok(Delivered { place: full.clone(), path: Some(full) })
}

/// Seconds since 1970 (the wall clock; a browser's too).
pub fn unix_seconds() -> u64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The player's home directory as the system names it, only to write it as `~` in the report.
pub fn home_dir() -> Option<String> {
    std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).ok().filter(|home| !home.is_empty())
}

/// The text of one report, ready to show and to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub title: String,
    /// Exactly what the issue is filled with.
    pub body: String,
    pub url: String,
    pub file_name: String,
    /// The facts for the top of the report file.
    pub header: String,
}

/// The issue and the file name for `info` at `unix_seconds`, with `home` written as `~`.
pub fn prepare_for(info: &ReportInfo, unix_seconds: u64, home: Option<&str>) -> Prepared {
    let file_name = bundle::file_name(unix_seconds);
    let title = url::scrub(&info::title(info), home);
    let body = info.body(url::ISSUE_REPO, &file_name, home);
    Prepared { url: url::issue_url(&title, &body), title, body, header: info.file_header(home), file_name }
}

/// The same for the machine this runs on.
pub fn prepare(info: &ReportInfo) -> Prepared {
    prepare_for(info, unix_seconds(), home_dir().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ReportInfo {
        ReportInfo {
            version: "0.2.0".into(),
            git: Some("abc1234".into()),
            os: "macos".into(),
            arch: "aarch64".into(),
            backend: "Metal".into(),
            adapter: "Apple M4 (IntegratedGpu)".into(),
            agent: None,
            page_build: None,
            language: "en".into(),
            window: "1280x720 px, scale 2.00".into(),
            game: Some(GameInfo { module: "classic".into(), seed: Some(20260928), round: 41, commands: 1234 }),
            panic: Some("index out of bounds at /Users/jane/.cargo/src/lib.rs:10:5".into()),
            trace: Some("   0: core::panicking::panic_bounds_check\n             at /Users/jane/.rustup/lib.rs:5\n   1: client::main".into()),
            log: (1..=30).map(|n| format!("[wgpu] line {n} in /Users/jane/work")).collect(),
        }
    }

    #[test]
    fn the_header_states_the_build_the_system_the_graphics_and_the_match() {
        let facts = sample().facts();
        for want in [
            "- Game: Aurelia 0.2.0 (abc1234)",
            "- System: macos aarch64",
            "- Graphics: Metal, Apple M4 (IntegratedGpu)",
            "- Language: en",
            "- Window: 1280x720 px, scale 2.00",
            "- Match: classic rules, seed 20260928, round 41, last command 1234",
            "- Last panic: index out of bounds",
        ] {
            assert!(facts.contains(want), "{want} missing in\n{facts}");
        }
        let mut loaded = sample();
        loaded.game.as_mut().unwrap().seed = None;
        loaded.panic = None;
        loaded.agent = Some("Mozilla/5.0 (X11)".into());
        loaded.page_build = Some("wasm web_bg.abc123.wasm, art v7".into());
        let facts = loaded.facts();
        assert!(facts.contains("seed unknown (loaded from a save)") && facts.contains("- Browser: Mozilla/5.0 (X11)") && facts.contains("- Web build: wasm web_bg.abc123.wasm, art v7") && !facts.contains("panic"), "{facts}");
        loaded.game = None;
        assert!(loaded.facts().contains("- Match: none running"));
    }

    #[test]
    fn the_prepared_issue_hides_the_home_directory_and_fits_the_link() {
        let prepared = prepare_for(&sample(), 1_793_000_000, Some("/Users/jane"));
        assert!(!prepared.body.contains("jane") && !prepared.url.contains("jane"), "{}", prepared.body);
        assert!(prepared.body.contains("~/.cargo/src/lib.rs:10:5") && prepared.body.contains("## What happened"));
        assert!(prepared.body.contains("aurelia-report-20261026-073320.zip") && prepared.header.contains("- Game: Aurelia 0.2.0"));
        assert!(prepared.body.chars().count() <= url::BODY_CAP && prepared.url.len() <= url::LINK_BUDGET);
        assert!(prepared.url.starts_with("https://github.com/sirgl/aurelia-classic/issues/new?template=bug_report.md&title=Crash%3A%20index%20out%20of%20bounds"));
        // The newest log lines stay; the header never goes.
        assert!(prepared.body.contains("line 30") && prepared.body.contains("- Match:"));
        assert!(prepared.header.contains("Log (last lines)") && !prepared.header.contains("jane"));
        assert!(prepared.body.contains("### Trace") && prepared.body.contains("at ~/.rustup/lib.rs:5") && prepared.header.contains("Panic trace\n   0: core::panicking"));
        assert!(prepared.body.find("### Trace").unwrap() < prepared.body.find("### Log").unwrap());
    }
}
