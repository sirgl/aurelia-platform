//! The command journal of the running match (platform/docs/saves.md §3): every autosave starts
//! a new one beside it, `autosave-000042-t0107.aurlog` next to
//! `autosave-000042-t0107.aurelia`, and the match's commands and round hashes are
//! appended to it as they are played. `desktop --replay <journal>` plays it back from its
//! save; a bug report is the pair. The ring of autosaves removes a save with its journal.
use std::path::{Path, PathBuf};

use kernel::replay::codec::CommandCodec;

use super::slot::SaveError;
use crate::shell::storage;
use crate::game::GameClient;

const EXTENSION: &str = "aurlog";

/// A match whose commands go into a journal (`ModuleUi::command_journal`).
pub trait CommandJournal {
    /// Start a journal from the world as it is now (just saved): its header.
    fn begin_journal(&mut self) -> Vec<u8>;
    /// The journal's records since the last call.
    fn take_journal(&mut self) -> Vec<u8>;
    /// How many commands the match has played since it started or was loaded here (a bug report
    /// names the last one).
    fn commands_played(&self) -> u64 {
        0
    }
}

impl<M: CommandCodec> CommandJournal for GameClient<M> {
    fn begin_journal(&mut self) -> Vec<u8> {
        self.game.begin_journal(&self.rules)
    }

    fn take_journal(&mut self) -> Vec<u8> {
        self.game.take_journal()
    }

    fn commands_played(&self) -> u64 {
        self.commands
    }
}

/// The journal beside the save at `save`.
pub fn journal_path(save: &Path) -> PathBuf {
    save.with_extension(EXTENSION)
}

/// The journal file being written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalFile {
    path: PathBuf,
}

impl JournalFile {
    /// A new journal beside the save at `save`, starting with `header`.
    pub fn create(save: &Path, header: &[u8]) -> Result<Self, SaveError> {
        let path = journal_path(save);
        storage::write(&path, header).map_err(|e| SaveError::Io(i18n::tf!("shell.errors.cannot-write-journal", error = e.to_string())))?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append `records` (whole records: a crash leaves the file readable to its last one).
    pub fn append(&self, records: &[u8]) -> Result<(), SaveError> {
        if records.is_empty() {
            return Ok(());
        }
        let failed = |e: std::io::Error| SaveError::Io(i18n::tf!("shell.errors.cannot-write-journal", error = e.to_string()));
        let mut file = storage::open_append(&self.path, false).map_err(failed)?;
        file.write_all(records).map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_journal_starts_with_its_header_and_grows_by_appends() {
        let dir = std::env::temp_dir().join(format!("aurelia-journal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save = dir.join("autosave-000003-t0012.aurelia");
        let journal = JournalFile::create(&save, b"head").unwrap();
        assert_eq!(journal.path(), dir.join("autosave-000003-t0012.aurlog"));
        journal.append(b"").unwrap();
        journal.append(b"+one").unwrap();
        journal.append(b"+two").unwrap();
        assert_eq!(std::fs::read(journal.path()).unwrap(), b"head+one+two");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
