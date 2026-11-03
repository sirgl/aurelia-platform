//! A fast, deterministic hasher for small keys (ids, tiles, small tuples) and the
//! `HashMap`/`HashSet` aliases that use it.
//!
//! `std`'s default hasher is SipHash with a random state per map: robust against
//! hostile keys, and slow for a `u32`. The simulation never hashes hostile input, and it
//! must not depend on a random state, so its maps use this one: one multiply per word
//! (the algorithm of rustc's `FxHasher`), the same hash on every run and platform word
//! size (`usize` is folded as 64 bits).
//!
//! Determinism rule: a map's ITERATION order still depends on its insertion history and
//! capacity. Nothing in the simulation may let a result depend on it: iterate a map only
//! to compute an order-insensitive value (a sum, a check), or sort first.
//!
//! Build with `FxHashMap::default()` / `with_capacity_and_hasher(n, Default::default())`;
//! `new()` exists only for the default hasher.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

/// The hasher: `hash = (hash.rotate_left(5) ^ word) * SEED` per word of input.
#[derive(Debug, Clone, Copy, Default)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut rest = bytes;
        while rest.len() >= 8 {
            self.add(u64::from_le_bytes([rest[0], rest[1], rest[2], rest[3], rest[4], rest[5], rest[6], rest[7]]));
            rest = &rest[8..];
        }
        if rest.len() >= 4 {
            self.add(u64::from(u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]])));
            rest = &rest[4..];
        }
        for byte in rest {
            self.add(u64::from(*byte));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// `BuildHasher` of [`FxHasher`] (no state: every map hashes alike).
pub type FxBuildHasher = BuildHasherDefault<FxHasher>;
/// A `HashMap` with the deterministic fast hasher.
pub type FxHashMap<K, V> = HashMap<K, V, FxBuildHasher>;
/// A `HashSet` with the deterministic fast hasher.
pub type FxHashSet<T> = HashSet<T, FxBuildHasher>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{BuildHasher, Hash};

    fn hash_of<T: Hash>(value: &T) -> u64 {
        FxBuildHasher::default().hash_one(value)
    }

    #[test]
    fn the_hash_is_a_fixed_function_of_the_key() {
        // Pinned values: the hasher must never depend on a random state or on the platform.
        assert_eq!(hash_of(&0u32), 0);
        assert_eq!(hash_of(&1u32), SEED);
        assert_eq!(hash_of(&(1u8, 2u16)), (SEED.rotate_left(5) ^ 2).wrapping_mul(SEED));
        assert_eq!(hash_of(&12345u32), hash_of(&12345u32));
        assert_ne!(hash_of(&1u32), hash_of(&2u32));
    }

    #[test]
    fn byte_slices_hash_by_words_then_tail() {
        let a = hash_of(&"a longer text than eight bytes".to_string());
        let b = hash_of(&"a longer text than eight bytes".to_string());
        assert_eq!(a, b);
        assert_ne!(a, hash_of(&"a longer text than eight byteZ".to_string()));
    }

    #[test]
    fn maps_and_sets_work_with_default_construction() {
        let mut map: FxHashMap<u32, &str> = FxHashMap::default();
        map.insert(7, "seven");
        map.insert(9, "nine");
        assert_eq!(map.get(&7), Some(&"seven"));
        assert_eq!(map.get(&8), None);
        let mut set: FxHashSet<(u8, u16)> = FxHashSet::default();
        assert!(set.insert((1, 2)));
        assert!(!set.insert((1, 2)));
    }
}
