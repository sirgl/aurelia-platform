//! One rules module's saves: `$AURELIA_SAVE_DIR/<module name>/` holds its quick save, its
//! autosaves (`autosave/`), the saves the player named (`manual/`) and its Hall of Fame, so a module never sees, overwrites or
//! loads another module's games. The directory is the module id's name, not its version:
//! a newer version of a module finds the saves of the one before, and its loader decides
//! whether they still load.
//!
//! Before, every module shared `$AURELIA_SAVE_DIR` itself and the files named no module. The
//! module that wrote them adopts them once at start (`adopt_shared_saves`): they move
//! under its name and get its tag.
use std::path::{Path, PathBuf};

use kernel::world::module::ModuleId;

use super::autosave::{latest_save, sequence_of, Autosaves};
use super::hall_of_fame::HallOfFame;
use super::slot::{save_directory, SaveError, SaveSlot};
use crate::shell::storage;

const QUICK_SAVE: &str = "quicksave.aurelia";
const AUTOSAVES: &str = "autosave";
const MANUAL_SAVES: &str = "manual";
const HALL_OF_FAME: &str = "hall_of_fame.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSaves {
    dir: PathBuf,
    module: ModuleId,
}

impl ModuleSaves {
    /// The saves of `module` in the player's save directory.
    pub fn of(module: ModuleId) -> Self {
        Self::under(&save_directory(), module)
    }

    pub fn under(root: &Path, module: ModuleId) -> Self {
        Self { dir: root.join(module.name), module }
    }

    pub fn quick_save(&self) -> SaveSlot {
        SaveSlot::at(self.dir.join(QUICK_SAVE), self.module)
    }

    pub fn autosaves(&self) -> Autosaves {
        Autosaves::in_dir(self.dir.join(AUTOSAVES), self.module)
    }

    /// The folder of the saves the player named.
    pub(super) fn manual_dir(&self) -> PathBuf {
        self.dir.join(MANUAL_SAVES)
    }

    pub(super) fn module_id(&self) -> ModuleId {
        self.module
    }

    pub fn hall_of_fame(&self) -> HallOfFame {
        HallOfFame::at(self.dir.join(HALL_OF_FAME))
    }

    /// The save Continue loads: the newest of the quick save, the latest autosave and the
    /// saves the player named (`latest_save`; a named save wins only when it is newer).
    pub fn latest(&self) -> Option<SaveSlot> {
        let newest = latest_save(&self.quick_save(), &self.autosaves());
        let manual = self.newest_manual();
        let modified = |s: &SaveSlot| storage::modified(s.path());
        match (newest, manual) {
            (Some(n), Some(m)) if modified(&m) > modified(&n) => Some(m),
            (n, m) => n.or(m),
        }
    }

    /// Moves the files of the shared layout under `root` into this module's saves, tagged:
    /// the quick save, the autosaves (names kept) and the Hall of Fame. A file this module
    /// already has is never overwritten; the shared one then stays where it is. Returns
    /// how many files moved.
    pub fn adopt_shared_saves(&self, root: &Path) -> Result<usize, SaveError> {
        let mut moved = usize::from(adopt_save(&root.join(QUICK_SAVE), &self.quick_save())?);
        let shared_autosaves = root.join(AUTOSAVES);
        for path in files_in(&shared_autosaves).into_iter().filter(|p| sequence_of(p).is_some()) {
            let Some(name) = path.file_name() else { continue };
            let slot = SaveSlot::at(self.dir.join(AUTOSAVES).join(name), self.module);
            moved += usize::from(adopt_save(&path, &slot)?);
        }
        // Only an empty directory goes; anything else in it stays the player's.
        let _ = storage::remove_dir(&shared_autosaves);
        moved += usize::from(adopt_file(&root.join(HALL_OF_FAME), &self.dir.join(HALL_OF_FAME))?);
        Ok(moved)
    }
}

/// An untagged save at `shared` becomes `slot` (written tagged, then the shared one removed).
fn adopt_save(shared: &Path, slot: &SaveSlot) -> Result<bool, SaveError> {
    if !storage::is_file(shared) || slot.exists() {
        return Ok(false);
    }
    let bytes = storage::read(shared).map_err(|e| SaveError::Io(i18n::tf!("shell.errors.cannot-read", path = format!("{shared:?}"), error = e.to_string())))?;
    slot.write(&bytes)?;
    storage::remove_file(shared).map_err(|e| SaveError::Io(i18n::tf!("shell.errors.cannot-remove", path = format!("{shared:?}"), error = e.to_string())))?;
    Ok(true)
}

/// A file moved as it is.
fn adopt_file(shared: &Path, own: &Path) -> Result<bool, SaveError> {
    if !storage::is_file(shared) || storage::exists(own) {
        return Ok(false);
    }
    let failed = |e: std::io::Error| SaveError::Io(format!("Cannot move {shared:?}: {e}"));
    if let Some(dir) = own.parent() {
        storage::create_dir_all(dir).map_err(failed)?;
    }
    storage::rename(shared, own).map_err(failed)?;
    Ok(true)
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = storage::list(dir) else {
        return Vec::new();
    };
    entries.into_iter().filter(|p| storage::is_file(p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: ModuleId = ModuleId { name: "first", version: 1 };
    const SECOND: ModuleId = ModuleId { name: "second", version: 1 };

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurelia-module-saves-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn modules_keep_separate_saves() {
        let root = temp_dir("separate");
        let (first, second) = (ModuleSaves::under(&root, FIRST), ModuleSaves::under(&root, SECOND));
        first.quick_save().write(b"first game").unwrap();
        first.autosaves().write(b"first auto", 4, 3).unwrap();
        first.hall_of_fame().append("first won").unwrap();
        assert!(second.latest().is_none(), "another module's saves are not Continue");
        assert!(!second.quick_save().exists());
        assert!(second.hall_of_fame().lines().is_empty());
        second.quick_save().write(b"second game").unwrap();
        assert_eq!(first.quick_save().read().unwrap(), b"first game", "a save never overwrites another module's");
        assert_eq!(second.quick_save().read().unwrap(), b"second game");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_layout_moves_under_the_adopting_module() {
        let root = temp_dir("adopt");
        std::fs::create_dir_all(root.join(AUTOSAVES)).unwrap();
        std::fs::write(root.join(QUICK_SAVE), b"AURELIA\0quick").unwrap();
        std::fs::write(root.join(AUTOSAVES).join("autosave-000007-t0030.aurelia"), b"AURELIA\0auto").unwrap();
        std::fs::write(root.join(HALL_OF_FAME), "an old victory\n").unwrap();
        let saves = ModuleSaves::under(&root, FIRST);
        assert_eq!(saves.adopt_shared_saves(&root).unwrap(), 3);
        assert_eq!(saves.quick_save().read().unwrap(), b"AURELIA\0quick");
        let auto = saves.autosaves().latest().unwrap();
        assert_eq!(auto.read().unwrap(), b"AURELIA\0auto");
        assert!(auto.path().ends_with("first/autosave/autosave-000007-t0030.aurelia"));
        assert_eq!(saves.hall_of_fame().lines(), vec!["an old victory"]);
        assert!(!root.join(QUICK_SAVE).exists() && !root.join(AUTOSAVES).exists() && !root.join(HALL_OF_FAME).exists());
        assert_eq!(saves.adopt_shared_saves(&root).unwrap(), 0, "adoption runs once");
        let other = ModuleSaves::under(&root, SECOND).quick_save();
        let moved = SaveSlot::at(saves.quick_save().path().to_path_buf(), SECOND);
        assert!(!other.exists());
        assert_eq!(moved.read(), Err(SaveError::OtherModule { saved: "first".into(), running: "second" }));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn adoption_never_overwrites_the_module_saves() {
        let root = temp_dir("keep");
        let saves = ModuleSaves::under(&root, FIRST);
        saves.quick_save().write(b"newer").unwrap();
        std::fs::write(root.join(QUICK_SAVE), b"AURELIA\0older").unwrap();
        assert_eq!(saves.adopt_shared_saves(&root).unwrap(), 0);
        assert_eq!(saves.quick_save().read().unwrap(), b"newer");
        assert!(root.join(QUICK_SAVE).exists(), "the shared file stays");
        std::fs::remove_dir_all(root).unwrap();
    }
}
