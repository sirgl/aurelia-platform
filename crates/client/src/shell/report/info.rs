//! What a bug report says about the game and the machine, as the text of the issue and as the
//! header of the report file. The player reads exactly this text in the dialog before anything
//! leaves the program. It names the build, the system, the graphics and the match; never the
//! player's name, the folders of their account, the network or the environment.

use super::url::{fit_body, scrub};

/// The facts about the running match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInfo {
    /// The rules module (`classic`).
    pub module: String,
    /// The world seed; None for a match loaded from a save (the save does not keep it).
    pub seed: Option<u64>,
    pub round: u32,
    /// Commands played since this match started or was loaded in this session.
    pub commands: u64,
}

/// Everything the report states.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReportInfo {
    pub version: String,
    /// The commit the build was made from, when the build says (`AURELIA_GIT_HASH`).
    pub git: Option<String>,
    pub os: String,
    pub arch: String,
    /// The graphics backend (`Metal`, `Vulkan`, `BrowserWebGpu`, `Gl`).
    pub backend: String,
    /// The adapter's name, kind and driver as wgpu reports them.
    pub adapter: String,
    /// The browser's `navigator.userAgent` on the web, nothing elsewhere.
    pub agent: Option<String>,
    /// The web build's files (`wasm`, `art`) the page loaded.
    pub page_build: Option<String>,
    pub language: String,
    /// The window in physical pixels and the display scale: `1280x720 px, scale 2`.
    pub window: String,
    /// None: no match runs.
    pub game: Option<GameInfo>,
    /// The last panic: message and location.
    pub panic: Option<String>,
    /// The panic's trace, one frame per line group (a Rust backtrace, or the page's stack).
    pub trace: Option<String>,
    /// The last lines of the game's log, oldest first.
    pub log: Vec<String>,
}

/// The title of the issue.
pub fn title(info: &ReportInfo) -> String {
    let panic = info.panic.as_deref().map(|p| p.lines().next().unwrap_or("")).filter(|p| !p.is_empty());
    match (panic, &info.game) {
        (Some(panic), _) => format!("Crash: {}", short(panic, 60)), // i18n-ok: the public issue is in English
        (None, Some(game)) => format!("Bug report: Aurelia {} (round {})", info.version, game.round), // i18n-ok: the public issue is in English
        (None, None) => format!("Bug report: Aurelia {}", info.version), // i18n-ok: the public issue is in English
    }
}

/// `text` cut to `limit` characters with an ellipsis.
fn short(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let cut: String = text.chars().take(limit.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

impl ReportInfo {
    /// The facts, one per line: the info section of the issue and the header of the report file.
    pub fn facts(&self) -> String {
        let mut lines = Vec::new();
        let git = self.git.as_deref().map(|hash| format!(" ({hash})")).unwrap_or_default();
        lines.push(format!("- Game: Aurelia {}{git}", self.version)); // i18n-ok: the public issue is in English
        lines.push(format!("- System: {} {}", self.os, self.arch)); // i18n-ok: the public issue is in English
        lines.push(format!("- Graphics: {}, {}", self.backend, self.adapter)); // i18n-ok: the public issue is in English
        if let Some(agent) = &self.agent {
            lines.push(format!("- Browser: {agent}")); // i18n-ok: the public issue is in English
        }
        if let Some(build) = &self.page_build {
            lines.push(format!("- Web build: {build}")); // i18n-ok: the public issue is in English
        }
        lines.push(format!("- Language: {}", self.language)); // i18n-ok: the public issue is in English
        lines.push(format!("- Window: {}", self.window)); // i18n-ok: the public issue is in English
        lines.push(match &self.game {
            Some(game) => {
                let seed = game.seed.map_or_else(|| "unknown (loaded from a save)".to_owned(), |seed| seed.to_string()); // i18n-ok: the public issue is in English
                format!("- Match: {} rules, seed {seed}, round {}, last command {}", game.module, game.round, game.commands) // i18n-ok: the public issue is in English
            }
            None => "- Match: none running".to_owned(), // i18n-ok: the public issue is in English
        });
        if let Some(panic) = &self.panic {
            lines.push(format!("- Last panic: {panic}")); // i18n-ok: the public issue is in English
        }
        lines.join("\n")
    }

    /// The body of the issue: the sections of the template, the facts and the log, with the home
    /// directory written as `~` and the length within what the link allows. `file` is the name
    /// of the report file to attach.
    pub fn body(&self, repo: &str, file: &str, home: Option<&str>) -> String {
        let title = scrub(&title(self), home);
        let template = "## What happened\n\n\n## What you expected\n\n\n## Steps to reproduce\n1. \n\n## Report file\nDrag `{file}` into this issue.\n\n### Info (filled in by the game)\n"; // i18n-ok: the public issue is in English
        let head = format!("{}{}\n", template.replace("{file}", file), scrub(&self.facts(), home));
        let trace: Vec<String> = self.trace_lines(home);
        let log: Vec<String> = self.log.iter().map(|line| scrub(line, home)).collect();
        fit_body(repo, &title, &head, &trace, &log)
    }

    /// The panic trace without blank lines, the home directory written as `~`.
    fn trace_lines(&self, home: Option<&str>) -> Vec<String> {
        let text = self.trace.as_deref().map(|trace| scrub(trace, home)).unwrap_or_default();
        text.lines().filter(|line| !line.trim().is_empty()).map(str::to_owned).collect()
    }

    /// The header of the report file: the facts, then how to replay the game.
    pub fn file_header(&self, home: Option<&str>) -> String {
        let mut text = format!("Aurelia bug report\n\n{}\n", scrub(&self.facts(), home)); // i18n-ok: the file is for developers, in English
        if let Some(trace) = &self.trace {
            text.push_str("\nPanic trace\n"); // i18n-ok: the file is for developers, in English
            text.push_str(scrub(trace, home).trim_end());
            text.push('\n');
        }
        if !self.log.is_empty() {
            text.push_str("\nLog (last lines)\n"); // i18n-ok: the file is for developers, in English
            for line in &self.log {
                text.push_str(&scrub(line, home));
                text.push('\n');
            }
        }
        text
    }
}
