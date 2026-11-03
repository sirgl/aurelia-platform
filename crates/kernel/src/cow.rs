//! A vector in shared chunks: cloning it copies a few pointers, writing to it copies only the
//! chunk written (copy on write).
//!
//! For large per-tile layers that a copy of the game (`GameState::clone`, a what-if trial)
//! reads but hardly writes: the clone shares every chunk and pays for the chunks the trial
//! touches. A fresh `filled` vector shares ONE chunk for all its full chunks, so an untouched
//! layer costs almost nothing. Reads are `get`/`iter` like a slice's; no dependencies.

use std::sync::Arc;

/// Elements per chunk.
const CHUNK: usize = 256;

/// A vector of `Copy` elements in shared chunks (a fixed-size array each: a read is two loads).
#[derive(Debug, Clone)]
pub struct Chunked<T: Copy> {
    chunks: Vec<Arc<[T; CHUNK]>>,
    len: usize,
}

impl<T: Copy> Default for Chunked<T> {
    fn default() -> Self {
        Self { chunks: Vec::new(), len: 0 }
    }
}

impl<T: Copy> Chunked<T> {
    /// `len` copies of `value`; all chunks are one shared allocation.
    pub fn filled(len: usize, value: T) -> Self {
        let blank = Arc::new([value; CHUNK]);
        Self { chunks: vec![blank; len.div_ceil(CHUNK)], len }
    }

    /// The values of `values`; a chunk that holds only `blank` (the unused end of the last chunk
    /// is `blank` too) is the one shared blank chunk, so an unexplored part of a map costs nothing,
    /// in memory or in a later clone.
    pub fn from_vec_sharing_blank(values: Vec<T>, blank: T) -> Self
    where
        T: PartialEq,
    {
        let len = values.len();
        let shared = Arc::new([blank; CHUNK]);
        let chunks = values
            .chunks(CHUNK)
            .map(|c| {
                if c.iter().all(|v| *v == blank) {
                    Arc::clone(&shared)
                } else {
                    let mut chunk = [blank; CHUNK];
                    chunk[..c.len()].copy_from_slice(c);
                    Arc::new(chunk)
                }
            })
            .collect();
        Self { chunks, len }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.len {
            return None;
        }
        Some(&self.chunks[index / CHUNK][index % CHUNK])
    }

    /// Writable element; the chunk is copied first when a clone still shares it.
    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }
        Some(&mut Arc::make_mut(&mut self.chunks[index / CHUNK])[index % CHUNK])
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> + '_ {
        self.chunks.iter().flat_map(|chunk| chunk.iter()).take(self.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_like_a_vec() {
        let values: Vec<u32> = (0..1000).collect();
        let c = Chunked::from_vec_sharing_blank(values.clone(), u32::MAX);
        assert_eq!(c.len(), 1000);
        assert_eq!(c.get(0), Some(&0));
        assert_eq!(c.get(256), Some(&256));
        assert_eq!(c.get(999), Some(&999));
        assert_eq!(c.get(1000), None);
        assert_eq!(c.iter().copied().collect::<Vec<_>>(), values);
    }

    #[test]
    fn from_vec_shares_blank_chunks() {
        let mut values = vec![0u32; 1000];
        values[300] = 5;
        let c = Chunked::from_vec_sharing_blank(values.clone(), 0);
        assert_eq!(c.iter().copied().collect::<Vec<_>>(), values);
        assert!(Arc::ptr_eq(&c.chunks[0], &c.chunks[2]), "blank full chunks are one allocation");
        assert!(!Arc::ptr_eq(&c.chunks[0], &c.chunks[1]), "the chunk with a value is its own");
    }

    #[test]
    fn filled_shares_its_full_chunks() {
        let c = Chunked::filled(600, 7u8);
        assert_eq!(c.len(), 600);
        assert!(Arc::ptr_eq(&c.chunks[0], &c.chunks[2]), "chunks are one allocation");
        assert_eq!(c.iter().filter(|v| **v == 7).count(), 600);
        assert_eq!(Chunked::<u8>::filled(0, 1).len(), 0);
        assert_eq!(Chunked::filled(256, 1u8).iter().count(), 256);
    }

    #[test]
    fn a_write_copies_only_its_chunk_and_never_shows_in_a_clone() {
        let mut a = Chunked::filled(600, 0u32);
        let b = a.clone();
        *a.get_mut(300).unwrap() = 9;
        assert_eq!(a.get(300), Some(&9));
        assert_eq!(b.get(300), Some(&0), "the clone keeps its value");
        assert!(a.get_mut(600).is_none());
        // the chunk of 300 is now its own; the others are still shared with `b`
        assert!(!Arc::ptr_eq(&a.chunks[1], &b.chunks[1]));
        assert!(Arc::ptr_eq(&a.chunks[0], &b.chunks[0]));
    }
}
