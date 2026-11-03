//! The shell's real saves, journal, Hall of Fame and settings code run on an installed `Store`: what a
//! page does, except that the durable side is a fake. Each test thread has its own store.
use std::path::{Path, PathBuf};

use kernel::world::module::ModuleId;

use super::fake::Fake;
use super::*;
use crate::shell::saves::{journal_path, latest_save, Autosaves, JournalFile, ModuleSaves, SaveError, SaveSlot};
use crate::shell::settings::Settings;
use crate::shell::storage;

const CLASSIC: ModuleId = ModuleId { name: "classic", version: 1 };
const OTHER: ModuleId = ModuleId { name: "other", version: 1 };

/// A path that is nowhere on the disk: if the code reached `std::fs`, the test would find nothing.
fn root() -> PathBuf {
    PathBuf::from("no-such-disk-root").join(format!("saves-{}", std::process::id()))
}

fn page(fake: &Fake) {
    install(Store::new(Box::new(fake.clone())));
}

/// A page load after the durable side holds `fake.disk()`: a new store, filled from it.
fn reload(fake: &Fake) -> Fake {
    let next = Fake::at(fake.0.borrow().now + 60_000);
    let mut store = Store::new(Box::new(next.clone()));
    store.load(fake.disk().into_iter().map(|(key, (bytes, modified))| (key, bytes, modified)));
    install(store);
    next
}

fn flush_all(fake: &Fake) {
    flush();
    let batches = fake.batches();
    for (id, _) in batches {
        batch_done(id, Ok(()));
    }
}

#[test]
fn a_save_survives_a_reload_and_stays_off_the_disk() {
    let fake = Fake::at(1_000);
    page(&fake);
    let saves = ModuleSaves::under(&root(), CLASSIC);
    assert!(saves.latest().is_none());
    saves.quick_save().write(b"AURELIA\0turn 12").unwrap();
    assert!(saves.quick_save().exists());
    assert_eq!(saves.quick_save().read().unwrap(), b"AURELIA\0turn 12", "read back from memory before anything is flushed");
    flush_all(&fake);
    assert!(!root().exists(), "nothing was written to the disk");
    uninstall();

    reload(&fake);
    let again = ModuleSaves::under(&root(), CLASSIC);
    assert!(again.quick_save().exists());
    assert_eq!(again.quick_save().read().unwrap(), b"AURELIA\0turn 12");
    assert_eq!(again.latest().unwrap(), again.quick_save(), "Continue finds it after the reload");
    uninstall();
}

#[test]
fn a_save_of_another_module_is_still_refused() {
    let fake = Fake::at(1);
    page(&fake);
    let path = root().join("classic").join("quicksave.aurelia");
    SaveSlot::at(path.clone(), OTHER).write(b"AURELIA\0x").unwrap();
    assert_eq!(SaveSlot::at(path, CLASSIC).read(), Err(SaveError::OtherModule { saved: "other".into(), running: "classic" }));
    assert!(matches!(SaveSlot::at(root().join("none"), CLASSIC).read(), Err(SaveError::Io(_))), "a missing save is the read error");
    uninstall();
}

#[test]
fn the_autosave_ring_rotates_with_its_journals_and_continue_picks_the_newer_of_quick_and_auto() {
    let fake = Fake::at(1_000);
    page(&fake);
    let saves = ModuleSaves::under(&root(), CLASSIC);
    let ring: Autosaves = saves.autosaves();
    for turn in 1..=7 {
        fake.tick(10);
        let slot = ring.write(format!("turn {turn}").as_bytes(), turn, 3).unwrap();
        let journal = JournalFile::create(slot.path(), b"head").unwrap();
        journal.append(b"+a").unwrap();
        journal.append(b"+b").unwrap();
        assert_eq!(storage::read(&journal_path(slot.path())).unwrap(), b"head+a+b");
    }
    let kept: Vec<u64> = ring.list().into_iter().map(|(seq, _)| seq).collect();
    assert_eq!(kept, [5, 6, 7]);
    let journals = storage::list(&root().join("classic").join("autosave")).unwrap().into_iter().filter(|p| p.extension().is_some_and(|e| e == "aurlog")).count();
    assert_eq!(journals, 3, "a journal leaves the ring with its save");
    assert!(ring.latest().unwrap().path().to_string_lossy().ends_with("-t0007.aurelia"));
    assert!(JournalFile::create(Path::new("a"), b"").is_ok());
    assert!(JournalFile::create(&root().join("x").join("a.aurelia"), b"").is_ok(), "folders are only names in the store");

    // The quick save written after the newest autosave wins; one written before it does not.
    let quick = saves.quick_save();
    fake.tick(10);
    quick.write(b"AURELIA\0quick").unwrap();
    assert_eq!(latest_save(&quick, &ring), Some(quick.clone()));
    fake.tick(10);
    let auto = ring.write(b"AURELIA\0auto", 8, 3).unwrap();
    assert_eq!(latest_save(&quick, &ring), Some(auto.clone()));
    // After a reload the order is the same: the times were stored with the files.
    flush_all(&fake);
    uninstall();
    reload(&fake);
    let (quick, ring) = (saves.quick_save(), saves.autosaves());
    assert_eq!(latest_save(&quick, &ring).map(|s| s.path().to_path_buf()), Some(auto.path().to_path_buf()));
    uninstall();
}

#[test]
fn a_journal_that_is_not_there_cannot_be_appended_to() {
    let fake = Fake::at(1);
    page(&fake);
    let journal = JournalFile::create(&root().join("a.aurelia"), b"h").unwrap();
    storage::remove_file(journal.path()).unwrap();
    assert!(matches!(journal.append(b"x"), Err(SaveError::Io(_))), "as on the disk, a vanished journal stops the journal");
    uninstall();
}

#[test]
fn the_hall_of_fame_keeps_its_lines_across_a_reload() {
    let fake = Fake::at(1);
    page(&fake);
    let hall = ModuleSaves::under(&root(), CLASSIC).hall_of_fame();
    assert!(hall.lines().is_empty());
    hall.append("Rome · Science victory · turn 300 · score 900").unwrap();
    hall.append("Rome · Eliminated · turn 80 · score 40").unwrap();
    flush_all(&fake);
    uninstall();
    reload(&fake);
    let hall = ModuleSaves::under(&root(), CLASSIC).hall_of_fame();
    assert_eq!(hall.lines(), ["Rome · Science victory · turn 300 · score 900", "Rome · Eliminated · turn 80 · score 40"]);
    hall.append("Rome · Eliminated · turn 5 · score 1").unwrap();
    assert_eq!(hall.lines().len(), 3);
    uninstall();
}

#[test]
fn settings_and_the_language_choice_survive_a_reload() {
    let fake = Fake::at(1);
    page(&fake);
    let path = crate::shell::settings::config_path();
    assert_eq!(Settings::load(&path), Settings::default(), "no file yet: the defaults");
    let chosen = Settings { language: "ru".to_owned(), ui_scale: 1.25, autosaves_kept: 3, ..Settings::default() };
    chosen.save(&path).unwrap();
    flush_all(&fake);
    uninstall();
    reload(&fake);
    let read = Settings::load(&path);
    assert_eq!((read.language.as_str(), read.ui_scale, read.autosaves_kept), ("ru", 1.25, 3));
    assert_eq!(read, chosen);
    uninstall();
}

#[test]
fn a_quota_failure_after_the_save_call_is_reported_and_the_save_stays_playable() {
    let fake = Fake::at(1);
    page(&fake);
    let slot = ModuleSaves::under(&root(), CLASSIC).quick_save();
    slot.write(b"AURELIA\0big").unwrap();
    flush();
    let (id, _) = fake.batches().pop().unwrap();
    batch_done(id, Err(PersistError { full: true, message: "QuotaExceededError".into() }));
    assert_eq!(take_failure().map(|f| f.full), Some(true));
    assert_eq!(take_failure(), None);
    assert_eq!(slot.read().unwrap(), b"AURELIA\0big", "Load still works until the page closes");
    uninstall();
}

#[test]
fn a_page_without_durable_storage_is_volatile_and_still_saves_in_memory() {
    let fake = Fake::at(1);
    fake.0.borrow_mut().durable_off = true;
    page(&fake);
    assert!(is_volatile());
    let slot = ModuleSaves::under(&root(), CLASSIC).quick_save();
    slot.write(b"AURELIA\0mem").unwrap();
    assert_eq!(slot.read().unwrap(), b"AURELIA\0mem");
    uninstall();
    assert!(!is_volatile(), "no store: the disk, never volatile");
}

#[test]
fn with_no_store_the_calls_are_the_disk() {
    let dir = std::env::temp_dir().join(format!("aurelia-storage-disk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("nested").join("a.txt");
    assert!(storage_read_fails(&file));
    create_dir_all(file.parent().unwrap()).unwrap();
    write(&file, b"one").unwrap();
    let mut appender = open_append(&file, false).unwrap();
    appender.write_all(b"+two").unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"one+two");
    assert_eq!(read_to_string(&file).unwrap(), "one+two");
    assert!(is_file(&file) && exists(&file) && modified(&file).is_some());
    assert_eq!(list(file.parent().unwrap()).unwrap(), [file.clone()]);
    rename(&file, &dir.join("b.txt")).unwrap();
    remove_file(&dir.join("b.txt")).unwrap();
    assert!(open_append(&dir.join("none"), false).is_err());
    assert!(take_failure().is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

fn storage_read_fails(path: &Path) -> bool {
    read(path).is_err()
}

#[test]
fn a_page_lists_names_and_deletes_saves_from_its_keys() {
    let fake = Fake::at(1_000);
    page(&fake);
    let saves = ModuleSaves::under(&root(), CLASSIC);
    assert!(saves.entries().is_empty());
    saves.manual_slot("Brazil turn 12").unwrap().write(b"AURELIA\0m").unwrap();
    fake.tick(5_000);
    saves.quick_save().write(b"AURELIA\0q").unwrap();
    fake.tick(5_000);
    let auto = saves.autosaves().write(b"AURELIA\0a", 14, 5).unwrap();
    JournalFile::create(auto.path(), b"head").unwrap();
    // A neighbour module's save and a stray file are not in this module's list.
    ModuleSaves::under(&root(), OTHER).manual_slot("Alien").unwrap().write(b"AURELIA\0x").unwrap();
    storage::write(&root().join("classic").join("manual").join("notes.txt"), b"ignored").unwrap();
    let kinds: Vec<_> = saves.entries().into_iter().map(|e| (e.kind, e.name)).collect();
    assert_eq!(
        kinds,
        vec![
            (crate::shell::saves::SaveKind::Auto, "autosave-000001-t0014".to_owned()),
            (crate::shell::saves::SaveKind::Quick, "quicksave".to_owned()),
            (crate::shell::saves::SaveKind::Manual, "Brazil turn 12".to_owned()),
        ]
    );
    assert_eq!(saves.latest().unwrap(), auto, "Continue is the newest of all three");
    flush_all(&fake);
    uninstall();

    // After a reload the page lists the same, and a delete reaches the durable side.
    let next = reload(&fake);
    let again = ModuleSaves::under(&root(), CLASSIC);
    assert_eq!(again.entries().len(), 3);
    again.delete(auto.path()).unwrap();
    again.delete(again.manual_slot("Brazil turn 12").unwrap().path()).unwrap();
    assert_eq!(again.entries().len(), 1);
    assert!(!storage::is_file(&journal_path(auto.path())), "the autosave's journal went with it");
    flush_all(&next);
    assert!(!next.disk().keys().any(|key| key.ends_with("Brazil turn 12.aurelia") || key.contains("autosave-")), "{:?}", next.disk().keys());
    assert!(!root().exists(), "nothing touched the disk");
    uninstall();
}
