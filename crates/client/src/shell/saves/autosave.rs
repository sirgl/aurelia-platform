//! Autosaves: a ring of the newest N games in the module's `autosave/` directory
//! (`ModuleSaves`). Files are named by a sequence number and the
//! turn, `autosave-000042-t0107.aurelia`; the sequence orders them, so the
//! latest is found without trusting file times. Continue loads the newest of
//! the latest autosave and the quick save. An autosave's command journal lies beside it
//! (`journal`) and leaves the ring with it.
use std::path::{Path, PathBuf};

use kernel::world::module::ModuleId;

use super::journal::journal_path;
use super::slot::{SaveError, SaveSlot};
use crate::shell::storage;

const PREFIX: &str = "autosave-";
const EXTENSION: &str = "aurelia";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autosaves {
    dir: PathBuf,
    module: ModuleId,
}

impl Autosaves {
    pub fn in_dir(dir: PathBuf, module: ModuleId) -> Self {
        Self { dir, module }
    }

    /// Autosaves by sequence number, oldest first.
    pub fn list(&self) -> Vec<(u64, PathBuf)> {
        let Ok(entries) = storage::list(&self.dir) else {
            return Vec::new();
        };
        let mut saves: Vec<(u64, PathBuf)> = entries
            .into_iter()
            .filter_map(|path| sequence_of(&path).map(|seq| (seq, path)))
            .collect();
        saves.sort();
        saves
    }

    pub fn latest(&self) -> Option<SaveSlot> {
        self.list().pop().map(|(_, path)| SaveSlot::at(path, self.module))
    }

    /// Writes the next autosave, then removes all but the newest `keep`.
    pub fn write(&self, bytes: &[u8], turn: u32, keep: u32) -> Result<SaveSlot, SaveError> {
        let next = self.list().last().map(|(seq, _)| seq + 1).unwrap_or(1);
        let slot = SaveSlot::at(self.dir.join(format!("{PREFIX}{next:06}-t{turn:04}.{EXTENSION}")), self.module);
        slot.write(bytes)?;
        self.rotate(keep.max(1) as usize)?;
        Ok(slot)
    }

    fn rotate(&self, keep: usize) -> Result<(), SaveError> {
        let saves = self.list();
        let excess = saves.len().saturating_sub(keep);
        for (_, path) in saves.into_iter().take(excess) {
            storage::remove_file(&path).map_err(|e| SaveError::Io(i18n::tf!("shell.errors.cannot-rotate", error = e.to_string())))?;
            let journal = journal_path(&path);
            if storage::is_file(&journal) {
                storage::remove_file(&journal).map_err(|e| SaveError::Io(i18n::tf!("shell.errors.cannot-rotate", error = e.to_string())))?;
            }
        }
        Ok(())
    }
}

/// `autosave-000042-t0107.aurelia` -> 42.
pub(super) fn sequence_of(path: &Path) -> Option<u64> {
    if path.extension()?.to_str()? != EXTENSION {
        return None;
    }
    let name = path.file_stem()?.to_str()?;
    let rest = name.strip_prefix(PREFIX)?;
    let (seq, _) = rest.split_once("-t")?;
    seq.parse().ok()
}

/// `autosave-000042-t0107.aurelia` -> 107.
pub(super) fn turn_of(path: &Path) -> Option<u32> {
    let name = path.file_stem()?.to_str()?;
    name.strip_prefix(PREFIX)?.split_once("-t")?.1.parse().ok()
}

/// The save Continue loads: the newer of the quick save and the latest
/// autosave by modification time (the autosave when times are equal).
pub fn latest_save(quick: &SaveSlot, autosaves: &Autosaves) -> Option<SaveSlot> {
    let auto = autosaves.latest();
    let quick = quick.exists().then(|| quick.clone());
    let modified = |s: &SaveSlot| storage::modified(s.path());
    match (auto, quick) {
        (Some(a), Some(q)) => {
            if modified(&q) > modified(&a) {
                Some(q)
            } else {
                Some(a)
            }
        }
        (a, q) => a.or(q),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULE: ModuleId = ModuleId { name: "classic", version: 1 };

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurelia-autosave-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn rotation_keeps_the_newest_and_latest_is_by_sequence() {
        let dir = temp_dir("ring");
        let ring = Autosaves::in_dir(dir.clone(), MODULE);
        assert!(ring.latest().is_none());
        for turn in 1..=7 {
            let slot = ring.write(format!("turn {turn}").as_bytes(), turn, 3).unwrap();
            std::fs::write(journal_path(slot.path()), b"journal").unwrap();
        }
        let kept: Vec<u64> = ring.list().into_iter().map(|(seq, _)| seq).collect();
        assert_eq!(kept, vec![5, 6, 7]);
        let journals = std::fs::read_dir(&dir).unwrap().filter(|e| e.as_ref().unwrap().path().extension().unwrap() == "aurlog");
        assert_eq!(journals.count(), 3, "a journal leaves the ring with its save");
        let latest = ring.latest().unwrap();
        assert_eq!(latest.read().unwrap(), b"turn 7");
        assert!(latest.path().file_name().unwrap().to_str().unwrap().ends_with("-t0007.aurelia"));
        std::fs::write(dir.join("notes.txt"), b"ignored").unwrap();
        std::fs::write(dir.join("autosave-x-t1.aurelia"), b"ignored").unwrap();
        assert_eq!(ring.list().len(), 3, "foreign files are not autosaves");
        ring.write(b"turn 8", 8, 2).unwrap();
        let kept: Vec<u64> = ring.list().into_iter().map(|(seq, _)| seq).collect();
        assert_eq!(kept, vec![7, 8], "a smaller ring drops the extra saves");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn continue_picks_the_newer_of_quick_save_and_autosave() {
        let dir = temp_dir("continue");
        let ring = Autosaves::in_dir(dir.join("autosave"), MODULE);
        let quick = SaveSlot::at(dir.join("quicksave.aurelia"), MODULE);
        assert_eq!(latest_save(&quick, &ring), None);
        quick.write(b"quick").unwrap();
        assert_eq!(latest_save(&quick, &ring), Some(quick.clone()));
        std::thread::sleep(std::time::Duration::from_millis(20));
        let auto = ring.write(b"auto", 3, 5).unwrap();
        assert_eq!(latest_save(&quick, &ring), Some(auto));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
