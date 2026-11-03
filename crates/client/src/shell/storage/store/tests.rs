use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::*;
use crate::shell::storage::fake::Fake;

fn put(key: &str, bytes: &[u8], modified: Millis) -> Op {
    Op::Put { key: key.into(), bytes: bytes.to_vec(), modified }
}

fn delete(key: &str) -> Op {
    Op::Delete { key: key.into() }
}

fn full() -> PersistError {
    PersistError { full: true, message: "QuotaExceededError".into() }
}

fn store(fake: &Fake) -> Store {
    Store::new(Box::new(fake.clone()))
}

#[test]
fn reads_come_from_memory_and_a_write_is_visible_at_once() {
    let fake = Fake::at(1000);
    let mut store = store(&fake);
    let path = Path::new("saves/classic/quicksave.aurelia");
    assert_eq!(store.read(path).unwrap_err().kind(), ErrorKind::NotFound);
    assert!(!store.is_file(path));
    store.write(path, b"one");
    assert_eq!(store.read(path).unwrap(), b"one");
    store.write(path, b"two");
    assert_eq!(store.read(path).unwrap(), b"two", "the newest write wins");
    assert!(store.is_file(path));
    assert!(fake.batches().is_empty(), "nothing goes to the backend before a flush");
}

#[test]
fn a_burst_of_changes_asks_for_one_flush_and_makes_one_batch_with_the_last_values() {
    let fake = Fake::at(10);
    let mut store = store(&fake);
    store.write(Path::new("a"), b"first");
    fake.tick(5);
    store.write(Path::new("b"), b"temporary");
    store.write(Path::new("a"), b"second");
    store.remove_file(Path::new("b")).unwrap();
    store.write(Path::new("c"), b"kept");
    assert_eq!(fake.0.borrow().scheduled, 1, "one timer for the whole burst");
    store.flush();
    assert_eq!(fake.batches(), vec![(1, vec![put("a", b"second", 15), delete("b"), put("c", b"kept", 15)])]);
    assert!(store.unsaved(), "the batch is not confirmed yet");
    store.batch_done(1, Ok(()));
    assert!(!store.unsaved());
    store.flush();
    assert_eq!(fake.batches().len(), 1, "a flush with no change submits nothing");
    store.write(Path::new("a"), b"third");
    assert_eq!(fake.0.borrow().scheduled, 2, "the next change after a flush asks again");
}

#[test]
fn batches_keep_the_order_they_were_made_in_even_while_one_is_in_flight() {
    let fake = Fake::at(1);
    let mut store = store(&fake);
    store.write(Path::new("save"), b"turn 1");
    store.flush();
    store.write(Path::new("save"), b"turn 2");
    store.write(Path::new("journal"), b"j");
    store.flush();
    let batches = fake.batches();
    assert_eq!(batches.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(batches[0].1, vec![put("save", b"turn 1", 1)]);
    assert_eq!(batches[1].1, vec![put("journal", b"j", 1), put("save", b"turn 2", 1)]);
    // The backend applies them in submit order: the durable copy ends on the newest value.
    assert_eq!(fake.disk()["save"].0, b"turn 2");
    store.batch_done(2, Ok(()));
    store.batch_done(1, Ok(()));
    assert!(!store.unsaved());
}

#[test]
fn a_full_storage_is_reported_once_and_the_data_is_written_again_at_the_next_flush() {
    let fake = Fake::at(1);
    let mut store = store(&fake);
    store.write(Path::new("saves/m/quicksave.aurelia"), b"big save");
    store.flush();
    store.batch_done(1, Err(full()));
    assert_eq!(store.take_failure(), Some(full()));
    assert_eq!(store.take_failure(), None, "reported once");
    assert_eq!(store.read(Path::new("saves/m/quicksave.aurelia")).unwrap(), b"big save", "the game keeps the save in memory");
    assert!(store.unsaved(), "the failed key is queued again");
    store.flush();
    assert_eq!(fake.batches()[1].1, vec![put("saves/m/quicksave.aurelia", b"big save", 1)]);
    store.batch_done(2, Ok(()));
    assert!(!store.unsaved());
    assert_eq!(store.take_failure(), None, "a success after a failure raises nothing new");
}

#[test]
fn a_failed_batch_does_not_bring_back_what_was_written_again_since() {
    let fake = Fake::at(1);
    let mut store = store(&fake);
    store.write(Path::new("save"), b"old");
    store.flush();
    store.write(Path::new("save"), b"new");
    store.flush();
    store.batch_done(1, Err(PersistError { full: false, message: "aborted".into() }));
    store.batch_done(2, Ok(()));
    store.flush();
    let last = fake.batches().pop().unwrap().1;
    assert_eq!(last, vec![put("save", b"new", 1)], "the retry carries the current bytes, never the old ones");
    assert_eq!(store.take_failure().map(|f| f.full), Some(false));
}

#[test]
fn a_removed_file_is_a_delete_in_the_batch() {
    let fake = Fake::at(1);
    let mut store = store(&fake);
    store.load([("old".to_owned(), b"x".to_vec(), 7)]);
    assert!(!store.unsaved(), "loaded files are already on disk");
    store.remove_file(Path::new("old")).unwrap();
    assert_eq!(store.remove_file(Path::new("old")).unwrap_err().kind(), ErrorKind::NotFound);
    store.flush();
    assert_eq!(fake.batches()[0].1, vec![delete("old")]);
}

#[test]
fn files_loaded_at_start_keep_their_times() {
    let fake = Fake::at(1_000_000);
    let mut store = store(&fake);
    store.load([("saves/m/quicksave.aurelia".to_owned(), b"q".to_vec(), 5_000)]);
    let modified = store.modified(Path::new("saves/m/quicksave.aurelia")).unwrap();
    assert_eq!(modified, SystemTime::UNIX_EPOCH + Duration::from_millis(5_000));
    assert_eq!(store.modified(Path::new("nothing")), None);
    assert_eq!(store.total_bytes(), 1);
}

#[test]
fn folders_are_names_and_listing_shows_only_the_files_directly_inside() {
    let fake = Fake::at(1);
    let mut store = store(&fake);
    for name in ["saves/m/quicksave.aurelia", "saves/m/autosave/a-1.aurelia", "saves/m/autosave/a-2.aurelia", "saves/m/autosave/deeper/x", "saves/other/q"] {
        store.write(Path::new(name), b"-");
    }
    let names = |dir: &str| store.list(Path::new(dir)).into_iter().map(|p| p.to_string_lossy().into_owned()).collect::<Vec<_>>();
    assert_eq!(names("saves/m/autosave"), ["saves/m/autosave/a-1.aurelia", "saves/m/autosave/a-2.aurelia"]);
    assert_eq!(names("saves/m"), ["saves/m/quicksave.aurelia"]);
    assert_eq!(names("saves/m/autosave/"), names("saves/m/autosave"), "a trailing slash is the same folder");
    assert_eq!(names("./saves/m/autosave"), ["./saves/m/autosave/a-1.aurelia", "./saves/m/autosave/a-2.aurelia"], "paths keep the caller's spelling");
    assert!(names("saves/none").is_empty());
    assert!(store.is_file(Path::new("./saves//other/q")), "./ and doubled slashes name the same file");
    store.rename(Path::new("saves/other/q"), Path::new("saves/other/moved")).unwrap();
    assert!(!store.is_file(Path::new("saves/other/q")) && store.is_file(Path::new("saves/other/moved")));
    assert_eq!(store.rename(Path::new("saves/other/q"), Path::new("x")).unwrap_err().kind(), ErrorKind::NotFound);
    store.flush();
    let batch = &fake.batches()[0].1;
    assert!(batch.contains(&delete("saves/other/q")) && batch.contains(&put("saves/other/moved", b"-", 1)));
}

#[test]
fn appending_extends_the_file_and_a_missing_one_is_made_only_when_asked() {
    let fake = Fake::at(1);
    let mut store = store(&fake);
    let path = Path::new("saves/m/autosave/a-1.aurlog");
    assert_eq!(store.append(path, b"x", false).unwrap_err().kind(), ErrorKind::NotFound);
    assert_eq!(store.open_append(path, false).unwrap_err().kind(), ErrorKind::NotFound);
    store.open_append(path, true).unwrap();
    assert_eq!(store.read(path).unwrap(), b"");
    store.append(path, b"head", false).unwrap();
    store.append(path, b"+one", true).unwrap();
    assert_eq!(store.read(path).unwrap(), b"head+one");
    store.flush();
    assert_eq!(fake.batches()[0].1, vec![put("saves/m/autosave/a-1.aurlog", b"head+one", 1)], "appends become one write of the whole file");
}

#[test]
fn without_durable_storage_everything_works_in_memory_and_nothing_is_queued() {
    let fake = Fake::at(1);
    fake.0.borrow_mut().durable_off = true;
    let mut store = store(&fake);
    assert!(!store.durable());
    store.write(Path::new("save"), b"s");
    assert_eq!(store.read(Path::new("save")).unwrap(), b"s");
    store.flush();
    assert!(!store.unsaved() && fake.batches().is_empty() && fake.0.borrow().scheduled == 0);
}

#[test]
fn a_database_deleted_behind_the_page_is_written_again_in_full() {
    let fake = Fake::at(1);
    let mut store = store(&fake);
    store.load([("a".to_owned(), b"A".to_vec(), 3), ("b".to_owned(), b"B".to_vec(), 4)]);
    store.resync();
    store.resync();
    assert_eq!(fake.0.borrow().scheduled, 1);
    store.flush();
    assert_eq!(fake.batches()[0].1, vec![put("a", b"A", 3), put("b", b"B", 4)], "every file, with the time it had");
}
