//! Text snapshots of screens (`.snap`): a frame drawn without a window or GPU becomes a
//! plain text — the widgets with their labels and states, the painted texts, shapes and
//! pictures with positions rounded to whole points — and a test compares it with the file
//! kept next to the test. A changed screen shows as a line diff, without the noise of
//! pixels (fonts, GPU, art that is or is not on disk).
//!
//! - `Stage` runs the shell's frame (full screens, HUD, navigation) over any module.
//! - `describe` turns one frame's egui output into the text.
//! - `assert_snapshot` compares with the file; `SNAPSHOT_UPDATE=1` rewrites the files.
//!
//! A module adds a snapshot in its own `tests/`:
//!
//! ```text
//! let mut stage = Stage::new(MyUi::game(), egui::vec2(1366.0, 768.0));
//! stage.open(Screen::Menu);
//! assert_snapshot(snapshot_file(env!("CARGO_MANIFEST_DIR"), "menu"), &stage.snapshot());
//! ```
mod diff;
mod record;
mod stage;

pub use record::describe;
pub use stage::Stage;

use std::path::{Path, PathBuf};

/// The environment variable that makes `assert_snapshot` write the files instead.
pub const UPDATE_VARIABLE: &str = "SNAPSHOT_UPDATE";

/// `tests/snapshots/<name>.snap` of the crate at `manifest_dir` (`env!("CARGO_MANIFEST_DIR")`).
pub fn snapshot_file(manifest_dir: &str, name: &str) -> PathBuf {
    Path::new(manifest_dir).join("tests").join("snapshots").join(format!("{name}.snap"))
}

/// Panics with a readable diff when `actual` differs from the snapshot at `path`, or when
/// there is no snapshot yet. With `SNAPSHOT_UPDATE=1` it writes `actual` there instead.
pub fn assert_snapshot(path: impl AsRef<Path>, actual: &str) {
    let update = std::env::var(UPDATE_VARIABLE).as_deref() == Ok("1");
    if let Err(failure) = compare(path.as_ref(), actual, update) {
        panic!("{failure}");
    }
}

/// `assert_snapshot` without the environment: Err carries the failure's message.
fn compare(path: &Path, actual: &str, update: bool) -> Result<(), String> {
    let expected = std::fs::read_to_string(path).ok().map(|text| text.replace("\r\n", "\n"));
    if expected.as_deref() == Some(actual) {
        return Ok(());
    }
    if update {
        write(path, actual);
        return Ok(());
    }
    let hint = format!("rerun with {UPDATE_VARIABLE}=1 to accept it");
    Err(match expected {
        None => format!("no snapshot {} ({hint}); the frame is:\n{actual}", path.display()),
        Some(expected) => {
            let changes = diff::lines(&expected, actual);
            format!("snapshot {} changed (- kept, + now; {hint}):\n{changes}", path.display())
        }
    })
}

fn write(path: &Path, text: &str) {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).unwrap_or_else(|error| panic!("{}: {error}", folder.display()));
    }
    std::fs::write(path, text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    eprintln!("snapshot written: {}", path.display());
}

#[cfg(test)]
mod tests;
