//! A set of tiles as a bit vector: one bit per `TileId`, 64 tiles to a word.
//!
//! The one type for "a flag per tile" (pillaged, river, explored, seen) and "a set of
//! tiles" (a flood fill's visited tiles). Against `Vec<bool>`/`Vec<u8>` it is 8x/64x
//! smaller to clone, compare and scan; against `HashSet<TileId>` it has no hashing and
//! no allocation per element. Iteration is in ASCENDING tile order, so a result never
//! depends on insertion order (determinism).
//!
//! A set grows on insert; `with_tile_count` allocates for a map up front. Two sets are
//! equal when they hold the same tiles, whatever their allocated size. No dependencies.

use crate::ids::TileId;

const BITS: usize = 64;

#[derive(Clone, Default)]
pub struct TileSet {
    words: Vec<u64>,
}

impl TileSet {
    /// An empty set that allocates on the first insert.
    #[inline]
    pub const fn new() -> Self {
        Self { words: Vec::new() }
    }

    /// An empty set with room for tiles `0..tiles`.
    pub fn with_tile_count(tiles: usize) -> Self {
        Self { words: vec![0; tiles.div_ceil(BITS)] }
    }

    /// A set holding every tile of `0..tiles`.
    pub fn full(tiles: usize) -> Self {
        let mut set = Self { words: vec![u64::MAX; tiles.div_ceil(BITS)] };
        let tail = tiles % BITS;
        if tail != 0 {
            if let Some(last) = set.words.last_mut() {
                *last = (1u64 << tail) - 1;
            }
        }
        set
    }

    /// Tiles the set has room for without growing.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.words.len() * BITS
    }

    #[inline]
    fn split(tile: TileId) -> (usize, u64) {
        let i = tile.0 as usize;
        (i / BITS, 1u64 << (i % BITS))
    }

    #[inline]
    pub fn contains(&self, tile: TileId) -> bool {
        let (w, bit) = Self::split(tile);
        self.words.get(w).is_some_and(|word| word & bit != 0)
    }

    /// Adds the tile; true when it was not in the set.
    #[inline]
    pub fn insert(&mut self, tile: TileId) -> bool {
        let (w, bit) = Self::split(tile);
        if w >= self.words.len() {
            self.words.resize(w + 1, 0);
        }
        let word = &mut self.words[w];
        let fresh = *word & bit == 0;
        *word |= bit;
        fresh
    }

    /// Removes the tile; true when it was in the set.
    #[inline]
    pub fn remove(&mut self, tile: TileId) -> bool {
        let (w, bit) = Self::split(tile);
        match self.words.get_mut(w) {
            Some(word) => {
                let had = *word & bit != 0;
                *word &= !bit;
                had
            }
            None => false,
        }
    }

    /// Puts the tile in or takes it out.
    #[inline]
    pub fn set(&mut self, tile: TileId, on: bool) {
        if on {
            self.insert(tile);
        } else {
            self.remove(tile);
        }
    }

    /// Number of tiles in the set.
    pub fn len(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|w| *w == 0)
    }

    /// Empties the set; the allocation stays.
    pub fn clear(&mut self) {
        self.words.iter_mut().for_each(|w| *w = 0);
    }

    /// Adds every tile of `other`.
    pub fn union_with(&mut self, other: &TileSet) {
        if self.words.len() < other.words.len() {
            self.words.resize(other.words.len(), 0);
        }
        for (a, b) in self.words.iter_mut().zip(&other.words) {
            *a |= *b;
        }
    }

    /// Keeps the tiles that are also in `other`.
    pub fn intersect_with(&mut self, other: &TileSet) {
        for (i, a) in self.words.iter_mut().enumerate() {
            *a &= other.words.get(i).copied().unwrap_or(0);
        }
    }

    /// Removes the tiles that are in `other`.
    pub fn difference_with(&mut self, other: &TileSet) {
        for (a, b) in self.words.iter_mut().zip(&other.words) {
            *a &= !*b;
        }
    }

    /// Every tile of `self` is in `other`.
    pub fn is_subset(&self, other: &TileSet) -> bool {
        self.words
            .iter()
            .enumerate()
            .all(|(i, a)| *a & !other.words.get(i).copied().unwrap_or(0) == 0)
    }

    /// The two sets have a tile in common.
    pub fn intersects(&self, other: &TileSet) -> bool {
        self.words.iter().zip(&other.words).any(|(a, b)| *a & *b != 0)
    }

    /// The tiles, ascending.
    pub fn iter(&self) -> Iter<'_> {
        Iter { words: &self.words, index: 0, word: self.words.first().copied().unwrap_or(0) }
    }

    /// The words of the set (tile `t` is bit `t % 64` of word `t / 64`), for encoders.
    pub fn words(&self) -> &[u64] {
        &self.words
    }
}

/// Ascending tiles of a [`TileSet`].
pub struct Iter<'a> {
    words: &'a [u64],
    index: usize,
    word: u64,
}

impl Iterator for Iter<'_> {
    type Item = TileId;

    #[inline]
    fn next(&mut self) -> Option<TileId> {
        while self.word == 0 {
            self.index += 1;
            self.word = *self.words.get(self.index)?;
        }
        let bit = self.word.trailing_zeros() as usize;
        self.word &= self.word - 1;
        Some(TileId((self.index * BITS + bit) as u32))
    }
}

impl<'a> IntoIterator for &'a TileSet {
    type Item = TileId;
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

impl FromIterator<TileId> for TileSet {
    fn from_iter<I: IntoIterator<Item = TileId>>(iter: I) -> Self {
        let mut set = TileSet::new();
        set.extend(iter);
        set
    }
}

impl Extend<TileId> for TileSet {
    fn extend<I: IntoIterator<Item = TileId>>(&mut self, iter: I) {
        for tile in iter {
            self.insert(tile);
        }
    }
}

impl PartialEq for TileSet {
    fn eq(&self, other: &Self) -> bool {
        let (short, long) = if self.words.len() <= other.words.len() {
            (&self.words, &other.words)
        } else {
            (&other.words, &self.words)
        };
        short == &long[..short.len()] && long[short.len()..].iter().all(|w| *w == 0)
    }
}

impl Eq for TileSet {}

impl std::fmt::Debug for TileSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.iter().map(|t| t.0)).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(i: u32) -> TileId {
        TileId(i)
    }

    #[test]
    fn insert_remove_contains_len() {
        let mut s = TileSet::with_tile_count(200);
        assert!(s.is_empty());
        assert!(s.insert(t(0)));
        assert!(s.insert(t(63)));
        assert!(s.insert(t(64)));
        assert!(s.insert(t(199)));
        assert!(!s.insert(t(64)), "second insert is not fresh");
        assert_eq!(s.len(), 4);
        assert!(s.contains(t(63)) && s.contains(t(64)) && !s.contains(t(65)));
        assert!(s.remove(t(63)));
        assert!(!s.remove(t(63)));
        assert!(!s.remove(t(5000)), "outside the capacity: not there");
        assert!(!s.contains(t(5000)));
        assert_eq!(s.len(), 3);
        s.clear();
        assert!(s.is_empty() && s.len() == 0);
        assert_eq!(s.capacity(), 256, "clear keeps the allocation");
    }

    #[test]
    fn growing_set_and_set_by_flag() {
        let mut s = TileSet::new();
        assert!(!s.contains(t(70)));
        s.set(t(70), true);
        s.set(t(3), true);
        s.set(t(70), false);
        assert_eq!(s.iter().collect::<Vec<_>>(), [t(3)]);
    }

    #[test]
    fn iteration_is_ascending_whatever_the_insert_order() {
        let order = [190, 5, 64, 0, 127, 128, 63, 65, 1];
        let s: TileSet = order.iter().map(|i| t(*i)).collect();
        let mut sorted = order.to_vec();
        sorted.sort_unstable();
        assert_eq!(s.iter().map(|x| x.0).collect::<Vec<_>>(), sorted);
        assert_eq!((&s).into_iter().count(), 9);
    }

    #[test]
    fn word_wise_algebra_matches_the_naive_one() {
        let a: TileSet = (0..300).filter(|i| i % 3 == 0).map(t).collect();
        let b: TileSet = (0..300).filter(|i| i % 5 == 0).map(t).collect();
        let mut u = a.clone();
        u.union_with(&b);
        let mut n = a.clone();
        n.intersect_with(&b);
        let mut d = a.clone();
        d.difference_with(&b);
        for i in 0..300 {
            assert_eq!(u.contains(t(i)), i % 3 == 0 || i % 5 == 0, "union {i}");
            assert_eq!(n.contains(t(i)), i % 15 == 0, "intersection {i}");
            assert_eq!(d.contains(t(i)), i % 3 == 0 && i % 5 != 0, "difference {i}");
        }
        assert!(n.is_subset(&a) && n.is_subset(&b) && !a.is_subset(&b));
        assert!(a.intersects(&b));
        assert!(!d.intersects(&b));
    }

    #[test]
    fn a_short_set_algebra_with_a_longer_one() {
        let small: TileSet = [t(1), t(2)].into_iter().collect();
        let big: TileSet = [t(2), t(500)].into_iter().collect();
        let mut n = small.clone();
        n.intersect_with(&big);
        assert_eq!(n.iter().collect::<Vec<_>>(), [t(2)]);
        let mut u = small.clone();
        u.union_with(&big);
        assert_eq!(u.len(), 3);
        assert!(!big.is_subset(&small));
    }

    #[test]
    fn equality_ignores_allocated_size() {
        let a: TileSet = [t(1), t(9)].into_iter().collect();
        let mut b = TileSet::with_tile_count(1000);
        b.insert(t(9));
        b.insert(t(1));
        assert_eq!(a, b);
        b.insert(t(999));
        assert_ne!(a, b);
        b.remove(t(999));
        assert_eq!(a, b);
    }

    #[test]
    fn full_covers_exactly_the_tile_count() {
        for n in [0usize, 1, 63, 64, 65, 128, 130] {
            let s = TileSet::full(n);
            assert_eq!(s.len(), n, "n={n}");
            assert!(!s.contains(t(n as u32)), "n={n}");
        }
    }
}
