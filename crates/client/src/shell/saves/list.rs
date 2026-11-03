//! Every save the player can pick from, newest first: the manual saves they named
//! (`manual/<name>.aurelia`), the quick save and the autosave ring. A listing is names and file
//! times only; what a save says about its game is read apart and once (`header`).
//!
//! Listing goes through `storage`, so a page lists the keys of its store where a desktop lists
//! files.
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::autosave::turn_of;
use super::journal::journal_path;
use super::module_saves::ModuleSaves;
use super::slot::{SaveError, SaveSlot};
use crate::shell::storage;

const EXTENSION: &str = "aurelia";
/// The longest name of a manual save, in characters.
pub const NAME_LIMIT: usize = 60;

/// Where a save came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    /// Saved by the player under a name.
    Manual,
    /// The quick save (F5 / the quick-save key).
    Quick,
    /// One of the ring the game writes at the end of turns.
    Auto,
}

/// One save of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveEntry {
    pub path: PathBuf,
    pub kind: SaveKind,
    /// The file name without its extension (the name of a manual save).
    pub name: String,
    /// When the file was last written (None: the platform does not say).
    pub modified: Option<SystemTime>,
    /// The turn an autosave's file name carries.
    pub named_turn: Option<u32>,
    /// An autosave's sequence number (breaks ties between equal file times).
    sequence: u64,
}

impl SaveEntry {
    fn order_key(&self) -> (Option<SystemTime>, u64) {
        (self.modified, self.sequence)
    }
}

/// The file name a manual save of `name` gets (without its extension): characters a file name
/// cannot hold become `_`, leading and trailing blanks and dots go, and it is cut to
/// `NAME_LIMIT`. None when nothing is left to name a file by.
pub fn file_stem(name: &str) -> Option<String> {
    let cleaned: String = name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let trimmed = cleaned.trim_matches(|c: char| c.is_whitespace() || c == '.');
    let stem: String = trimmed.chars().take(NAME_LIMIT).collect();
    let stem = stem.trim_end_matches(|c: char| c.is_whitespace() || c == '.');
    (!stem.is_empty()).then(|| stem.to_owned())
}

impl ModuleSaves {
    /// The slot a manual save named `name` is written to (None: the name leaves nothing to name a file by).
    pub fn manual_slot(&self, name: &str) -> Option<SaveSlot> {
        let stem = file_stem(name)?;
        Some(SaveSlot::at(self.manual_dir().join(format!("{stem}.{EXTENSION}")), self.module_id()))
    }

    /// The manual saves, in no order.
    fn manual_entries(&self) -> Vec<SaveEntry> {
        let Ok(files) = storage::list(&self.manual_dir()) else {
            return Vec::new();
        };
        files
            .into_iter()
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some(EXTENSION) && storage::is_file(path))
            .filter_map(|path| {
                let name = path.file_stem()?.to_str()?.to_owned();
                Some(SaveEntry { modified: storage::modified(&path), path, kind: SaveKind::Manual, name, named_turn: None, sequence: 0 })
            })
            .collect()
    }

    /// The names of the manual saves (to tell the player a name is taken).
    pub fn manual_names(&self) -> Vec<String> {
        self.manual_entries().into_iter().map(|e| e.name).collect()
    }

    /// Every save of the module, newest first (by file time; an autosave ring breaks ties by
    /// its sequence number).
    pub fn entries(&self) -> Vec<SaveEntry> {
        let mut entries = self.manual_entries();
        let quick = self.quick_save();
        if quick.exists() {
            entries.push(SaveEntry {
                modified: storage::modified(quick.path()),
                path: quick.path().to_path_buf(),
                kind: SaveKind::Quick,
                name: stem_of(quick.path()),
                named_turn: None,
                sequence: 0,
            });
        }
        for (sequence, path) in self.autosaves().list() {
            entries.push(SaveEntry {
                modified: storage::modified(&path),
                kind: SaveKind::Auto,
                name: stem_of(&path),
                named_turn: turn_of(&path),
                sequence,
                path,
            });
        }
        entries.sort_by(|a, b| b.order_key().cmp(&a.order_key()).then_with(|| a.name.cmp(&b.name)));
        entries
    }

    /// The newest of the manual saves.
    pub(super) fn newest_manual(&self) -> Option<SaveSlot> {
        let newest = self.manual_entries().into_iter().max_by_key(SaveEntry::order_key)?;
        Some(SaveSlot::at(newest.path, self.module_id()))
    }

    /// Deletes the save at `path`, which must be one of `entries()` (an autosave leaves with
    /// its command journal).
    pub fn delete(&self, path: &Path) -> Result<(), SaveError> {
        let entry = self.entries().into_iter().find(|e| e.path == path).ok_or_else(|| {
            SaveError::Io(i18n::tf!("shell.errors.cannot-remove", path = format!("{path:?}"), error = i18n::t!("shell.saves.not-in-list")))
        })?;
        let failed = |e: std::io::Error| SaveError::Io(i18n::tf!("shell.errors.cannot-remove", path = format!("{path:?}"), error = e.to_string()));
        storage::remove_file(&entry.path).map_err(failed)?;
        let journal = journal_path(&entry.path);
        if entry.kind == SaveKind::Auto && storage::is_file(&journal) {
            storage::remove_file(&journal).map_err(failed)?;
        }
        Ok(())
    }
}

fn stem_of(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::world::module::ModuleId;

    const CLASSIC: ModuleId = ModuleId { name: "classic", version: 1 };

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurelia-list-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn set_time(path: &Path, seconds_ago: u64) {
        let when = SystemTime::now() - std::time::Duration::from_secs(seconds_ago);
        std::fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
    }

    #[test]
    fn a_file_name_is_made_of_what_the_player_typed() {
        assert_eq!(file_stem("Brazil turn 120").as_deref(), Some("Brazil turn 120"));
        assert_eq!(file_stem("  a/b\\c:d  ").as_deref(), Some("a_b_c_d"));
        assert_eq!(file_stem("..hidden.").as_deref(), Some("hidden"));
        assert_eq!(file_stem("Рим, ход 5").as_deref(), Some("Рим, ход 5"));
        assert_eq!(file_stem("   ..  "), None);
        assert_eq!(file_stem(""), None);
        assert_eq!(file_stem(&"x".repeat(200)).unwrap().chars().count(), NAME_LIMIT);
    }

    #[test]
    fn the_list_has_every_kind_newest_first() {
        let root = temp_dir("order");
        let saves = ModuleSaves::under(&root, CLASSIC);
        assert!(saves.entries().is_empty());
        let old = saves.manual_slot("Old one").unwrap();
        old.write(b"AURELIA\0old").unwrap();
        let quick = saves.quick_save();
        quick.write(b"AURELIA\0quick").unwrap();
        let first = saves.autosaves().write(b"AURELIA\0a1", 10, 5).unwrap();
        let second = saves.autosaves().write(b"AURELIA\0a2", 11, 5).unwrap();
        let named = saves.manual_slot("Brazil turn 12").unwrap();
        named.write(b"AURELIA\0named").unwrap();
        std::fs::write(root.join("classic/manual/notes.txt"), b"ignored").unwrap();
        std::fs::write(root.join("classic/manual/folder.aurelia.tmp"), b"ignored").unwrap();
        set_time(old.path(), 9000);
        set_time(quick.path(), 5000);
        set_time(first.path(), 3000);
        set_time(second.path(), 1000);
        set_time(named.path(), 10);
        let entries = saves.entries();
        let seen: Vec<(SaveKind, &str)> = entries.iter().map(|e| (e.kind, e.name.as_str())).collect();
        assert_eq!(
            seen,
            vec![
                (SaveKind::Manual, "Brazil turn 12"),
                (SaveKind::Auto, "autosave-000002-t0011"),
                (SaveKind::Auto, "autosave-000001-t0010"),
                (SaveKind::Quick, "quicksave"),
                (SaveKind::Manual, "Old one"),
            ]
        );
        assert_eq!(entries[1].named_turn, Some(11));
        assert_eq!(saves.latest().unwrap().path(), named.path(), "Continue takes the newest, a manual save too");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn equal_file_times_fall_back_to_the_autosave_sequence() {
        let root = temp_dir("ties");
        let saves = ModuleSaves::under(&root, CLASSIC);
        let paths: Vec<_> = (1..=3).map(|turn| saves.autosaves().write(b"AURELIA\0x", turn, 5).unwrap()).collect();
        let when = SystemTime::now() - std::time::Duration::from_secs(60);
        for slot in &paths {
            std::fs::File::options().write(true).open(slot.path()).unwrap().set_modified(when).unwrap();
        }
        let turns: Vec<Option<u32>> = saves.entries().iter().map(|e| e.named_turn).collect();
        assert_eq!(turns, vec![Some(3), Some(2), Some(1)]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_removes_the_save_and_an_autosaves_journal_only_from_the_list() {
        let root = temp_dir("delete");
        let saves = ModuleSaves::under(&root, CLASSIC);
        let auto = saves.autosaves().write(b"AURELIA\0a", 4, 5).unwrap();
        std::fs::write(journal_path(auto.path()), b"journal").unwrap();
        let manual = saves.manual_slot("keep me").unwrap();
        manual.write(b"AURELIA\0m").unwrap();
        saves.delete(auto.path()).unwrap();
        assert!(!auto.exists() && !journal_path(auto.path()).exists(), "the journal leaves with its autosave");
        assert!(manual.exists());
        let stranger = root.join("classic/hall_of_fame.txt");
        std::fs::write(&stranger, "a line").unwrap();
        assert!(saves.delete(&stranger).is_err(), "only a listed save can be deleted");
        assert!(stranger.exists());
        assert!(saves.delete(&root.join("classic/manual/gone.aurelia")).is_err());
        saves.delete(manual.path()).unwrap();
        assert!(saves.entries().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
