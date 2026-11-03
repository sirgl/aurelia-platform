use std::{cmp::Reverse, collections::BinaryHeap};

/// Reusable working memory; gameplay code does not know about it (§19).
/// `open` is a min-heap by (f, g, tile); `cost` is the g-score (MAX = unseen);
/// `parent` is the raw TileId of the parent (MAX = none).
/// The enemy set v6 is removed: ZOC reads the occupancy cache in O(1), setup is zero.
#[derive(Debug, Clone, Default)]
pub struct PathScratch {
    pub(crate) open: BinaryHeap<Reverse<(u32, u32, u32)>>,
    pub(crate) cost: Vec<u32>,
    pub(crate) parent: Vec<u32>,
    /// Metric of the previous query (§45: nodes expanded). Not gameplay.
    pub expanded_last: u64,
}

impl PathScratch {
    pub(crate) fn ensure(&mut self, n: usize) {
        if self.cost.len() < n {
            self.cost.resize(n, u32::MAX);
            self.parent.resize(n, u32::MAX);
        }
        self.open.clear();
        self.expanded_last = 0;
    }
}
