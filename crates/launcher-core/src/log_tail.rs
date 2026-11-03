//! The game's log for a bug report: warnings and errors (the GPU, the art, the saves) are kept in
//! `client::shell::report` so the last lines go into the issue. On a desktop they are also written
//! to stderr, where a launcher without a logger printed nothing before.
#![cfg(not(any(target_arch = "wasm32", target_os = "android")))]

struct TailLog;

impl log::Log for TailLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // wgpu and naga chatter at info; warnings and errors are what a report needs.
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let line = format!("[{}] {}: {}", record.level(), record.target(), record.args());
            eprintln!("{line}");
            client::shell::report::push_log(line);
        }
    }

    fn flush(&self) {}
}

/// Installs the log (once; another logger already set stays).
pub fn install() {
    if log::set_logger(&TailLog).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
}
