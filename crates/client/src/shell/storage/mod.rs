//! Where the shell keeps files that belong to the player: saves, journals, the Hall of Fame and the
//! settings. Every such file is read and written through this module and nowhere else, so a platform
//! without a disk can supply its own:
//!
//! * Desktop and Android: the functions are `std::fs` (same paths, same bytes; nothing is cached).
//! * A browser page: the launcher `install`s a `Store` before the app starts. Files are in memory, reads
//!   come from there, writes change the memory at once and the store hands them to IndexedDB in the
//!   background (`store`).
//!
//! The functions mirror the `std::fs` ones they replace, with the same `io::Error`s, so the callers
//! keep their messages.
use std::cell::RefCell;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[cfg(test)]
pub(crate) mod fake;
pub mod store;

pub use store::{Millis, Op, Persist, PersistError, Store};

thread_local! {
    /// The store of a page (one thread in a browser). Tests install one on their own thread to run the
    /// real saves code against it.
    static STORE: RefCell<Option<Store>> = const { RefCell::new(None) };
}

/// Use `store` for every file call of this thread from now on.
pub fn install(store: Store) {
    STORE.with(|s| *s.borrow_mut() = Some(store));
}

/// Take the store away (the calls fall back to the disk); tests use it to read the store's state.
pub fn uninstall() -> Option<Store> {
    STORE.with(|s| s.borrow_mut().take())
}

/// Run `f` on the installed store (None: there is none, the disk is used). A backend must not call back
/// into this module from `Persist` methods: it reports with `batch_done` later, from its own task.
pub fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> Option<R> {
    STORE.with(|s| s.borrow_mut().as_mut().map(f))
}

pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    with_store(|s| s.read(path)).unwrap_or_else(|| std::fs::read(path))
}

pub fn read_to_string(path: &Path) -> io::Result<String> {
    match with_store(|s| s.read(path)) {
        Some(bytes) => String::from_utf8(bytes?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        None => std::fs::read_to_string(path),
    }
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match with_store(|s| s.write(path, bytes)) {
        Some(()) => Ok(()),
        None => std::fs::write(path, bytes),
    }
}

pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    with_store(|s| s.rename(from, to)).unwrap_or_else(|| std::fs::rename(from, to))
}

pub fn remove_file(path: &Path) -> io::Result<()> {
    with_store(|s| s.remove_file(path)).unwrap_or_else(|| std::fs::remove_file(path))
}

/// Folders do not exist in the store (a file's name carries its folders).
pub fn create_dir_all(dir: &Path) -> io::Result<()> {
    with_store(|_| ()).map_or_else(|| std::fs::create_dir_all(dir), Ok)
}

/// Removes an empty folder; in the store there is nothing to remove.
pub fn remove_dir(dir: &Path) -> io::Result<()> {
    with_store(|_| ()).map_or_else(|| std::fs::remove_dir(dir), Ok)
}

pub fn is_file(path: &Path) -> bool {
    with_store(|s| s.is_file(path)).unwrap_or_else(|| path.is_file())
}

/// Whether something is at `path` (a file or, on the disk, a folder).
pub fn exists(path: &Path) -> bool {
    with_store(|s| s.is_file(path)).unwrap_or_else(|| path.exists())
}

/// When the file was last written (None: no such file, or the platform does not say).
pub fn modified(path: &Path) -> Option<SystemTime> {
    with_store(|s| s.modified(path)).unwrap_or_else(|| std::fs::metadata(path).and_then(|m| m.modified()).ok())
}

/// The entries of `dir` (on the disk also its folders); an error when the folder cannot be read.
pub fn list(dir: &Path) -> io::Result<Vec<PathBuf>> {
    match with_store(|s| s.list(dir)) {
        Some(files) => Ok(files),
        None => Ok(std::fs::read_dir(dir)?.filter_map(Result::ok).map(|e| e.path()).collect()),
    }
}

/// A file opened for adding at its end (`std::fs::OpenOptions::append`).
pub struct Appender {
    target: Target,
}

enum Target {
    Disk(std::fs::File),
    Store(PathBuf),
}

/// Open `path` for appending; `create` makes a missing file, else a missing file is an error.
pub fn open_append(path: &Path, create: bool) -> io::Result<Appender> {
    if let Some(opened) = with_store(|s| s.open_append(path, create)) {
        return opened.map(|()| Appender { target: Target::Store(path.to_path_buf()) });
    }
    let file = std::fs::OpenOptions::new().create(create).append(true).open(path)?;
    Ok(Appender { target: Target::Disk(file) })
}

impl Appender {
    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match &mut self.target {
            Target::Disk(file) => file.write_all(bytes),
            Target::Store(path) => with_store(|s| s.append(path, bytes, true)).unwrap_or_else(|| Err(io::Error::other("the store is gone"))),
        }
    }
}

/// Hand the changes to the durable side now (a timer, `pagehide`). No store: nothing to do.
pub fn flush() {
    with_store(Store::flush);
}

/// The durable side finished batch `id` (`Persist::submit`).
pub fn batch_done(id: u64, result: Result<(), PersistError>) {
    with_store(|s| s.batch_done(id, result));
}

/// The durable side was emptied behind the page's back: write every file again.
pub fn resync() {
    with_store(Store::resync);
}

/// The newest failure to persist, once: the shell shows it in the menu status.
pub fn take_failure() -> Option<PersistError> {
    with_store(Store::take_failure).flatten()
}

/// Whether files are kept only until the page closes (no durable storage): the shell says so once.
pub fn is_volatile() -> bool {
    with_store(|s| !s.durable()).unwrap_or(false)
}

#[cfg(test)]
mod tests;
