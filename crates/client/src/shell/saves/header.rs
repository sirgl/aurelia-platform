//! What a list of saves shows about a save without loading it: the turn, the player's nation and
//! era. The shell cannot read the module's bytes, so the module describes them
//! (`ModuleUi::describe_save`); this file reads the tagged file, hands the module the bytes after
//! the tag and sorts the failures into the reasons the list gives.
use std::path::Path;

use kernel::world::module::ModuleId;

use super::slot::{read_tagged, SaveError};
use crate::shell::words::SaveWords;

/// The facts a module can read from a save without building the game. Every part is optional: a
/// module that keeps no nation shows none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveSummary {
    /// The round the game was saved on (the turn the player sees).
    pub turn: Option<u32>,
    /// The player's nation, in the active language.
    pub nation: Option<String>,
    /// The player's era, in the active language.
    pub era: Option<String>,
}

/// Why a module cannot read a save, in terms the list can put into words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveProblem {
    /// Written by a newer build: its format number is above what this build reads.
    Newer(u32),
    /// Written by a build whose format this one no longer reads.
    Older(u32),
    /// The bytes are not a save of this module (cut short, damaged); the module's own words.
    Damaged(String),
}

/// A save the list cannot describe, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    /// The tagged file itself: missing, another module's, not a save.
    File(SaveError),
    /// The module refused the bytes.
    Module(SaveProblem),
}

impl Unreadable {
    /// The reason in the active language.
    pub fn text(&self, words: &SaveWords) -> String {
        match self {
            Self::File(error) => error.describe(words),
            Self::Module(SaveProblem::Newer(format)) => i18n::tf!("shell.saves.problem-newer", format = *format),
            Self::Module(SaveProblem::Older(format)) => i18n::tf!("shell.saves.problem-older", format = *format),
            Self::Module(SaveProblem::Damaged(detail)) => i18n::tf!("shell.saves.problem-damaged", detail = detail),
        }
    }
}

/// The header of the save at `path`: read the file once, check it names `module`, then let
/// `describe` read what it needs from the module's bytes.
pub fn read_header(
    path: &Path,
    module: ModuleId,
    describe: impl FnOnce(&[u8]) -> Result<SaveSummary, SaveProblem>,
) -> Result<SaveSummary, Unreadable> {
    let (saved, payload) = read_tagged(path).map_err(Unreadable::File)?;
    if saved != module.name {
        return Err(Unreadable::File(SaveError::OtherModule { saved, running: module.name }));
    }
    describe(&payload).map_err(Unreadable::Module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::saves::SaveSlot;

    const CLASSIC: ModuleId = ModuleId { name: "classic", version: 1 };
    const OTHER: ModuleId = ModuleId { name: "other", version: 1 };

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aurelia-header-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// A module that reads the first byte as the turn and refuses a payload that starts with 0xFF.
    fn describe(payload: &[u8]) -> Result<SaveSummary, SaveProblem> {
        match payload.first() {
            Some(0xFF) => Err(SaveProblem::Newer(24)),
            Some(&turn) => Ok(SaveSummary { turn: Some(turn.into()), ..Default::default() }),
            None => Err(SaveProblem::Damaged("empty".into())),
        }
    }

    #[test]
    fn the_header_comes_from_the_modules_bytes_after_the_tag() {
        let dir = temp_dir("read");
        let path = dir.join("a.aurelia");
        SaveSlot::at(path.clone(), CLASSIC).write(&[42, 1, 2, 3]).unwrap();
        assert_eq!(read_header(&path, CLASSIC, describe).unwrap().turn, Some(42));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_refused_file_is_unreadable_with_its_reason_and_never_a_panic() {
        let dir = temp_dir("refused");
        let path = dir.join("a.aurelia");
        let slot = SaveSlot::at(path.clone(), CLASSIC);
        slot.write(&[0xFF]).unwrap();
        assert_eq!(read_header(&path, CLASSIC, describe), Err(Unreadable::Module(SaveProblem::Newer(24))));
        slot.write(&[]).unwrap();
        assert_eq!(read_header(&path, CLASSIC, describe), Err(Unreadable::Module(SaveProblem::Damaged("empty".into()))));
        assert!(matches!(read_header(&path, OTHER, describe), Err(Unreadable::File(SaveError::OtherModule { .. }))));
        std::fs::write(&path, b"not a save at all").unwrap();
        assert_eq!(read_header(&path, CLASSIC, describe), Err(Unreadable::File(SaveError::UnknownModule)));
        assert!(matches!(read_header(&dir.join("gone.aurelia"), CLASSIC, describe), Err(Unreadable::File(SaveError::Io(_)))));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_reasons_are_put_into_words() {
        let words = i18n::with_language("en", crate::shell::words::current).saves;
        i18n::with_language("en", || {
            assert_eq!(Unreadable::Module(SaveProblem::Newer(24)).text(&words), "Saved by a newer version of the game (format 24)");
            assert_eq!(Unreadable::Module(SaveProblem::Older(3)).text(&words), "Saved by an older version this game no longer reads (format 3)");
            assert_eq!(Unreadable::Module(SaveProblem::Damaged("save file is truncated".into())).text(&words), "The file is damaged: save file is truncated");
            assert_eq!(Unreadable::File(SaveError::UnknownModule).text(&words), "This file does not name the rules it was saved with");
        });
    }
}
