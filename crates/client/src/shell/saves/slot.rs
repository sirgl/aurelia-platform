//! One save file: the name of the rules module that wrote it (the tag), then the bytes
//! the module saved. The tag lets a load refuse another module's game with a typed error
//! before the module reads a byte; what follows the tag is the module's own.
use std::path::{Path, PathBuf};

use kernel::world::module::ModuleId;

use crate::shell::storage;

/// The start of a tagged save (the module's bytes start with the kernel's `AURELIA\0`).
const TAG: &[u8; 8] = b"AURMODS\0";

/// Why a save cannot be written or loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The file cannot be read or written.
    Io(String),
    /// The save names another rules module than the one playing.
    OtherModule { saved: String, running: &'static str },
    /// The file names no rules module (not a save, or a damaged tag).
    UnknownModule,
    /// The module could not load its own bytes.
    Rejected(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) | Self::Rejected(error) => f.write_str(error),
            Self::OtherModule { saved, running } => {
                write!(f, "This save was made with the {saved} rules; this game runs the {running} rules") // i18n-ok: log text of Display; the player reads `describe`
            }
            Self::UnknownModule => f.write_str("This file does not name the rules it was saved with"), // i18n-ok: log text of Display
        }
    }
}

impl std::error::Error for SaveError {}

impl SaveError {
    /// The error in the module's words (the file and module errors carry their own text).
    pub fn describe(&self, words: &crate::shell::words::SaveWords) -> String {
        match self {
            Self::Io(error) | Self::Rejected(error) => error.clone(),
            Self::OtherModule { saved, running } => i18n::tf!("shell.saves.other-module", saved = saved, running = running),
            Self::UnknownModule => words.unknown_module.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSlot {
    path: PathBuf,
    module: ModuleId,
}

/// `$AURELIA_SAVE_DIR`, default `saves/` under the working directory: every module's saves
/// live under it (`ModuleSaves`).
pub fn save_directory() -> PathBuf {
    let dir = std::env::var("AURELIA_SAVE_DIR").unwrap_or_else(|_| "saves".into());
    PathBuf::from(dir)
}

impl SaveSlot {
    /// The save of `module` at `path`.
    pub fn at(path: PathBuf, module: ModuleId) -> Self {
        Self { path, module }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        storage::is_file(&self.path)
    }

    /// Write the tag and `bytes` through a temporary file so a crash never leaves half
    /// a save behind.
    pub fn write(&self, bytes: &[u8]) -> Result<(), SaveError> {
        let mut tagged = tag(self.module);
        tagged.extend_from_slice(bytes);
        write_atomic(&self.path, &tagged)
    }

    /// The bytes the module saved, when the file is a save of this slot's module.
    pub fn read(&self) -> Result<Vec<u8>, SaveError> {
        let (saved, payload) = read_tagged(&self.path)?;
        if saved != self.module.name {
            return Err(SaveError::OtherModule { saved, running: self.module.name });
        }
        Ok(payload)
    }
}

/// The name of the module that wrote the save at `path` and the bytes it saved (a
/// replay picks the module by it).
pub fn read_tagged(path: &Path) -> Result<(String, Vec<u8>), SaveError> {
    let bytes = storage::read(path).map_err(|e| SaveError::Io(i18n::tf!("shell.errors.cannot-read-save", error = e.to_string())))?;
    let (module, payload) = untag(&bytes).ok_or(SaveError::UnknownModule)?;
    Ok((module.to_owned(), payload.to_vec()))
}

/// `payload` as a save file of `module` is written: the tag, then the module's bytes (a report
/// file carries the match as such a file, so `--replay` reads it).
pub fn tagged(module: ModuleId, payload: &[u8]) -> Vec<u8> {
    let mut out = tag(module);
    out.extend_from_slice(payload);
    out
}

/// The tag of `module`: the marker, the name's length (u32 LE) and the name.
fn tag(module: ModuleId) -> Vec<u8> {
    let mut out = TAG.to_vec();
    out.extend_from_slice(&(module.name.len() as u32).to_le_bytes());
    out.extend_from_slice(module.name.as_bytes());
    out
}

/// The module name a file carries and the bytes after it (None: no tag).
fn untag(bytes: &[u8]) -> Option<(&str, &[u8])> {
    let rest = bytes.strip_prefix(TAG.as_slice())?;
    let (length, rest) = rest.split_first_chunk::<4>()?;
    let length = u32::from_le_bytes(*length) as usize;
    if length == 0 || length > rest.len() {
        return None;
    }
    let (name, payload) = rest.split_at(length);
    Some((std::str::from_utf8(name).ok()?, payload))
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    let failed = |e: std::io::Error| SaveError::Io(i18n::tf!("shell.errors.cannot-save", error = e.to_string()));
    if let Some(dir) = path.parent() {
        storage::create_dir_all(dir).map_err(|e| SaveError::Io(i18n::tf!("shell.errors.cannot-create", path = format!("{dir:?}"), error = e.to_string())))?;
    }
    let temporary = path.with_extension("tmp");
    storage::write(&temporary, bytes).map_err(failed)?;
    storage::rename(&temporary, path).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASSIC: ModuleId = ModuleId { name: "classic", version: 1 };
    const OTHER: ModuleId = ModuleId { name: "other", version: 3 };

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurelia-save-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn written_save_reads_back_as_the_module_bytes() {
        let dir = temp_dir("slot");
        let slot = SaveSlot::at(dir.join("nested").join("quicksave.aurelia"), CLASSIC);
        assert!(!slot.exists());
        slot.write(b"AURELIA\0payload").unwrap();
        assert!(slot.exists());
        assert_eq!(slot.read().unwrap(), b"AURELIA\0payload");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn another_module_save_is_a_typed_error() {
        let dir = temp_dir("other");
        let path = dir.join("quicksave.aurelia");
        SaveSlot::at(path.clone(), OTHER).write(b"AURELIA\0payload").unwrap();
        let error = SaveSlot::at(path, CLASSIC).read().unwrap_err();
        assert_eq!(error, SaveError::OtherModule { saved: "other".into(), running: "classic" });
        assert_eq!(error.to_string(), "This save was made with the other rules; this game runs the classic rules");
        let english = i18n::with_language("en", crate::shell::words::current).saves;
        assert_eq!(i18n::with_language("en", || error.describe(&english)), error.to_string(), "the English words say what Display says");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn file_without_a_module_name_is_a_typed_error() {
        let dir = temp_dir("unknown");
        let path = dir.join("quicksave.aurelia");
        let slot = SaveSlot::at(path.clone(), CLASSIC);
        for bytes in [&b"AURELIA\0untagged"[..], b"AURMODS\0", b"AURMODS\0\x09\0\0\0classic", b"AURMODS\0\0\0\0\0"] {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(slot.read(), Err(SaveError::UnknownModule), "{bytes:?}");
        }
        std::fs::remove_dir_all(dir).unwrap();
        assert!(matches!(slot.read(), Err(SaveError::Io(_))));
    }
}
