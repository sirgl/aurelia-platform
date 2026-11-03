//! The in-memory file store behind the browser build: every file the shell keeps (saves, journals,
//! the Hall of Fame, the settings) lives in a map here, reads are served from it, writes change it at
//! once and are handed to a durable backend (IndexedDB in a page) in batches in the background.
//!
//! The store knows nothing of a browser: the backend is the `Persist` trait, so the cache rules, the
//! batching and the failure path are tested natively with a fake backend (`tests`).
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Milliseconds since the Unix epoch (the browser's `Date.now()`).
pub type Millis = u64;

/// One change of the durable store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Put { key: String, bytes: Vec<u8>, modified: Millis },
    Delete { key: String },
}

impl Op {
    pub fn key(&self) -> &str {
        match self {
            Self::Put { key, .. } | Self::Delete { key } => key,
        }
    }
}

/// Why a batch did not reach the durable store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistError {
    /// The browser's storage is full (`QuotaExceededError`): the player must free space.
    pub full: bool,
    /// The backend's own words, for the log.
    pub message: String,
}

/// The durable side of the store. `submit` must return at once and report the outcome later through
/// `Store::batch_done` (a page: when the IndexedDB transaction ends). Batches that overlap are
/// applied in the order they were submitted.
pub trait Persist {
    /// The clock the files' modification times come from.
    fn now_ms(&self) -> Millis;
    /// Ask for `Store::flush` to run soon (a timer). Called when the first change after a flush arrives.
    fn schedule_flush(&mut self);
    /// Start writing `batch` (all or nothing); `id` comes back in `batch_done`.
    fn submit(&mut self, id: u64, batch: Vec<Op>);
    /// False when nothing is kept after the page closes (no IndexedDB: private mode).
    fn durable(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct File {
    bytes: Vec<u8>,
    modified: Millis,
}

/// The file cache plus the queue of changes not yet persisted.
pub struct Store {
    files: BTreeMap<String, File>,
    /// Keys changed since the last `flush`; the batch takes their value from `files` (gone = delete).
    dirty: BTreeSet<String>,
    /// Submitted batches the backend has not reported on: their keys, to write again if one fails.
    in_flight: BTreeMap<u64, Vec<String>>,
    next_batch: u64,
    scheduled: bool,
    failure: Option<PersistError>,
    backend: Box<dyn Persist>,
}

impl Store {
    pub fn new(backend: Box<dyn Persist>) -> Self {
        Self { files: BTreeMap::new(), dirty: BTreeSet::new(), in_flight: BTreeMap::new(), next_batch: 1, scheduled: false, failure: None, backend }
    }

    /// The files the durable store held at start (nothing is queued for them).
    pub fn load(&mut self, entries: impl IntoIterator<Item = (String, Vec<u8>, Millis)>) {
        for (key, bytes, modified) in entries {
            self.files.insert(key, File { bytes, modified });
        }
    }

    /// Whether files survive the page (false: they are kept in memory only, with a notice).
    pub fn durable(&self) -> bool {
        self.backend.durable()
    }

    /// Bytes held, for the log.
    pub fn total_bytes(&self) -> usize {
        self.files.values().map(|f| f.bytes.len()).sum()
    }

    /// Whether changes wait for a flush or for the backend's answer.
    pub fn unsaved(&self) -> bool {
        !self.dirty.is_empty() || !self.in_flight.is_empty()
    }

    pub fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.files.get(&key_of(path)).map(|f| f.bytes.clone()).ok_or_else(not_found)
    }

    pub fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(&key_of(path))
    }

    pub fn modified(&self, path: &Path) -> Option<SystemTime> {
        self.files.get(&key_of(path)).map(|f| SystemTime::UNIX_EPOCH + Duration::from_millis(f.modified))
    }

    /// The files directly in `dir`, as paths in the directory's own spelling.
    pub fn list(&self, dir: &Path) -> Vec<std::path::PathBuf> {
        let prefix = match key_of(dir) {
            key if key.is_empty() => String::new(),
            key => format!("{key}/"),
        };
        self.files
            .keys()
            .filter_map(|key| key.strip_prefix(prefix.as_str()))
            .filter(|rest| !rest.is_empty() && !rest.contains('/'))
            .map(|name| dir.join(name))
            .collect()
    }

    pub fn write(&mut self, path: &Path, bytes: &[u8]) {
        let key = key_of(path);
        let modified = self.backend.now_ms();
        self.files.insert(key.clone(), File { bytes: bytes.to_vec(), modified });
        self.changed(key);
    }

    /// Add `bytes` to the file; a missing file is created when `create`, else an error as `open` gives.
    pub fn append(&mut self, path: &Path, bytes: &[u8], create: bool) -> io::Result<()> {
        let key = key_of(path);
        let modified = self.backend.now_ms();
        match self.files.get_mut(&key) {
            Some(file) => {
                file.bytes.extend_from_slice(bytes);
                file.modified = modified;
            }
            None if create => {
                self.files.insert(key.clone(), File { bytes: bytes.to_vec(), modified });
            }
            None => return Err(not_found()),
        }
        self.changed(key);
        Ok(())
    }

    /// Open the file for appending without writing (the same errors `append` has).
    pub fn open_append(&mut self, path: &Path, create: bool) -> io::Result<()> {
        if self.is_file(path) {
            return Ok(());
        }
        if !create {
            return Err(not_found());
        }
        self.write(path, &[]);
        Ok(())
    }

    pub fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        let (from, to) = (key_of(from), key_of(to));
        let file = self.files.remove(&from).ok_or_else(not_found)?;
        self.files.insert(to.clone(), file);
        self.changed(from);
        self.changed(to);
        Ok(())
    }

    pub fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        let key = key_of(path);
        self.files.remove(&key).ok_or_else(not_found)?;
        self.changed(key);
        Ok(())
    }

    fn changed(&mut self, key: String) {
        if !self.backend.durable() {
            return;
        }
        self.dirty.insert(key);
        if !self.scheduled {
            self.scheduled = true;
            self.backend.schedule_flush();
        }
    }

    /// Hand the changes since the last flush to the backend as one batch (one key, one entry: the
    /// last value). Safe to call any time (a timer, `pagehide`); nothing changed: nothing happens.
    pub fn flush(&mut self) {
        self.scheduled = false;
        if self.dirty.is_empty() {
            return;
        }
        let keys: Vec<String> = std::mem::take(&mut self.dirty).into_iter().collect();
        let batch: Vec<Op> = keys
            .iter()
            .map(|key| match self.files.get(key) {
                Some(file) => Op::Put { key: key.clone(), bytes: file.bytes.clone(), modified: file.modified },
                None => Op::Delete { key: key.clone() },
            })
            .collect();
        let id = self.next_batch;
        self.next_batch += 1;
        self.in_flight.insert(id, keys);
        self.backend.submit(id, batch);
    }

    /// The backend finished batch `id`. A failed batch's keys are queued again (the next change or
    /// flush retries them) and the failure waits for `take_failure`.
    pub fn batch_done(&mut self, id: u64, result: Result<(), PersistError>) {
        let Some(keys) = self.in_flight.remove(&id) else { return };
        if let Err(error) = result {
            self.dirty.extend(keys);
            self.failure = Some(error);
        }
    }

    /// The durable store lost its data behind our back (the database was deleted and made anew): everything
    /// the page holds is written again at the next flush.
    pub fn resync(&mut self) {
        if !self.backend.durable() || self.files.is_empty() {
            return;
        }
        self.dirty.extend(self.files.keys().cloned());
        if !self.scheduled {
            self.scheduled = true;
            self.backend.schedule_flush();
        }
    }

    /// The newest persisting failure, once.
    pub fn take_failure(&mut self) -> Option<PersistError> {
        self.failure.take()
    }
}

/// `saves/classic/quicksave.aurelia`: the names joined with `/` (a leading `/` or `./` does not count).
fn key_of(path: &Path) -> String {
    use std::path::Component::{Normal, ParentDir};
    let parts: Vec<_> = path.components().filter(|c| matches!(c, Normal(_) | ParentDir)).map(|c| c.as_os_str().to_string_lossy()).collect();
    parts.join("/")
}

fn not_found() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "no such file")
}

#[cfg(test)]
mod tests;
