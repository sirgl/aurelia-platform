//! What a report remembers between frames: the last panic, the last lines of the log, and a
//! snapshot of the facts for a crash. After a panic in the browser the program cannot run
//! again (the wasm instance is dead), so the crash dialog is the page's own; it gets everything
//! ready from here, in the hook that runs before the program stops.
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, Once, OnceLock, PoisonError};

use super::info::ReportInfo;

/// The log keeps this many lines; a report shows the last `LOG_SHOWN`.
const LOG_KEPT: usize = 60;
pub const LOG_SHOWN: usize = 20;

/// The words of the crash dialog, resolved while the program still runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogWords {
    pub title: String,
    pub intro: String,
    pub preview: String,
    pub open_issue: String,
    pub save_file: String,
    pub reload: String,
    pub saved: String,
}

/// A crash, ready for the page to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crash {
    pub words: DialogWords,
    pub title: String,
    pub body: String,
    pub url: String,
    pub file_name: String,
    /// The text of the report file for a page that cannot make a zip: the facts and the log.
    pub file_text: String,
}

struct Memory {
    panic: Option<String>,
    trace: Option<String>,
    log: VecDeque<String>,
    snapshot: Option<(ReportInfo, DialogWords)>,
}

static MEMORY: Mutex<Memory> = Mutex::new(Memory { panic: None, trace: None, log: VecDeque::new(), snapshot: None });
static OBSERVER: OnceLock<fn(&Crash)> = OnceLock::new();
static TRACE_SOURCE: OnceLock<fn() -> Option<String>> = OnceLock::new();

fn memory() -> MutexGuard<'static, Memory> {
    MEMORY.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A line of the game's log (a launcher's logger, the shell's own warnings).
pub fn push_log(line: impl Into<String>) {
    let mut memory = memory();
    if memory.log.len() == LOG_KEPT {
        memory.log.pop_front();
    }
    memory.log.push_back(line.into());
}

/// The last lines of the log, oldest first.
pub fn log_tail() -> Vec<String> {
    let memory = memory();
    memory.log.iter().skip(memory.log.len().saturating_sub(LOG_SHOWN)).cloned().collect()
}

/// Where a trace comes from when the platform has its own way (the browser: the page's stack).
/// Without one, a native build captures the Rust backtrace.
pub fn set_trace_source(source: fn() -> Option<String>) {
    let _ = TRACE_SOURCE.set(source);
}

/// The trace of the panic being handled.
fn capture_trace() -> Option<String> {
    if let Some(source) = TRACE_SOURCE.get() {
        return source();
    }
    #[cfg(not(target_arch = "wasm32"))]
    return Some(std::backtrace::Backtrace::force_capture().to_string());
    #[cfg(target_arch = "wasm32")]
    None
}

/// The trace of the last panic of this run.
pub fn last_trace() -> Option<String> {
    memory().trace.clone()
}

/// The last panic of this run: message, location, thread.
pub fn last_panic() -> Option<String> {
    memory().panic.clone()
}

/// The facts as the last frames knew them, for a crash (`set_panic_observer`).
pub fn remember(info: ReportInfo, words: DialogWords) {
    memory().snapshot = Some((info, words));
}

/// Whether anything wants the snapshot (a launcher that shows a crash dialog registered).
pub fn wants_snapshots() -> bool {
    OBSERVER.get().is_some()
}

/// The launcher shows a crash: it is called from the panic hook, once per panic, with everything
/// ready. The browser launcher registers the page's dialog here.
pub fn set_panic_observer(observer: fn(&Crash)) {
    let _ = OBSERVER.set(observer);
}

/// Remembers every panic (message, location, thread) for the report, and tells the observer.
/// The previous hook still runs first (the console, the default message).
pub fn install_panic_recorder() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            previous(info);
            let text = describe(info);
            push_log(format!("panic: {text}")); // i18n-ok: a log line
            let trace = capture_trace();
            let (snapshot, panic) = {
                let mut memory = memory();
                memory.panic = Some(text.clone());
                memory.trace = trace.clone();
                (memory.snapshot.clone(), text)
            };
            if let Some(observer) = OBSERVER.get() {
                observer(&crash_of(snapshot, panic, trace));
            }
        }));
    });
}

/// `message at file:line:col in thread 'name'`
fn describe(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    let message = payload.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| payload.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".to_owned()); // i18n-ok: a log line
    let place = info.location().map(|l| format!(" at {}:{}:{}", l.file(), l.line(), l.column())).unwrap_or_default(); // i18n-ok: a log line
    let thread = std::thread::current().name().map(|n| format!(" in thread '{n}'")).unwrap_or_default(); // i18n-ok: a log line
    format!("{message}{place}{thread}")
}

/// The crash for `panic`, from the last snapshot (or what a bare report knows).
fn crash_of(snapshot: Option<(ReportInfo, DialogWords)>, panic: String, trace: Option<String>) -> Crash {
    let (mut info, words) = snapshot.unwrap_or_else(|| (bare_info(), DialogWords::default()));
    info.panic = Some(panic);
    info.trace = trace;
    info.log = log_tail();
    let prepared = super::prepare_for(&info, super::unix_seconds(), super::home_dir().as_deref());
    let file_name = prepared.file_name.replace(".zip", ".txt");
    Crash { words, file_text: prepared.header, title: prepared.title, body: prepared.body, url: prepared.url, file_name }
}

/// The facts that need no frame: the build and the system.
fn bare_info() -> ReportInfo {
    let build = super::build();
    ReportInfo {
        version: build.version.to_owned(),
        git: build.git.map(str::to_owned),
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        backend: "unknown".to_owned(), // i18n-ok: part of the public issue
        adapter: "unknown".to_owned(), // i18n-ok: part of the public issue
        agent: super::page().agent,
        page_build: super::page().build,
        language: i18n::language(),
        window: "unknown".to_owned(), // i18n-ok: part of the public issue
        ..ReportInfo::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_keeps_the_newest_lines_and_a_report_shows_the_last_twenty() {
        for n in 0..80 {
            push_log(format!("crash-test line {n}"));
        }
        let tail = log_tail();
        assert_eq!(tail.len(), LOG_SHOWN);
        assert_eq!(tail.last().unwrap(), "crash-test line 79");
        assert_eq!(tail[0], "crash-test line 60");
    }

    #[test]
    fn a_crash_has_the_panic_the_log_and_a_text_file_name() {
        let crash = crash_of(None, "boom at src/lib.rs:1:1".into(), Some("0: a\n1: b".into()));
        assert!(crash.title.starts_with("Crash: boom"), "{}", crash.title);
        assert!(crash.body.contains("- Last panic: boom at src/lib.rs:1:1") && crash.url.contains("issues/new?template="));
        assert!(crash.file_name.starts_with("aurelia-report-") && crash.file_name.ends_with(".txt"));
        assert!(crash.file_text.contains("Aurelia bug report") && crash.file_text.contains("Panic trace\n0: a\n1: b"));
        assert!(crash.body.contains("### Trace\n```\n0: a\n1: b\n```"), "{}", crash.body);
    }
}
