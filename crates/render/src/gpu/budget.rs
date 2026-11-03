//! A frame's allowance for rebuilds that may finish on later frames.
//!
//! A large terrain update (an end of turn that reveals land around many cities, a zoom
//! that changes the detail of many regions) marks many feature and border chunks at
//! once. They are rebuilt nearest to the view first until the allowance is spent; the
//! rest keep their previous meshes for a few frames. One budget covers all such work
//! of a frame, the terrain publish included: after a costly publish the chunks get
//! little. Every update still makes progress: an update that finds the allowance spent
//! rebuilds one chunk. With no deadline (immediate terrain: screenshots, tests)
//! everything is rebuilt at once.
use std::time::Duration;
use web_time::Instant;

/// Chunks built side by side on worker threads between two budget checks.
pub const BATCH: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct Budget {
    /// When to stop starting new rebuilds; None: never.
    until: Option<Instant>,
    /// World XZ the rebuilds start nearest to.
    pub focus: [f32; 2],
}

impl Budget {
    pub fn unlimited(focus: [f32; 2]) -> Self {
        Self { until: None, focus }
    }

    /// `allowance` from now.
    pub fn from_now(allowance: Duration, focus: [f32; 2]) -> Self {
        Self { until: Some(Instant::now() + allowance), focus }
    }

    pub fn spent(&self) -> bool {
        self.until.is_some_and(|until| Instant::now() >= until)
    }

    /// Splits `items` into batches to build side by side while the allowance lasts:
    /// `BATCH` items while it is not spent, else a single item when nothing was built
    /// yet (`started` false), else none.
    pub fn batches<'a>(&self, items: &'a [usize], started: bool) -> Batches<'a> {
        Batches { budget: *self, items, started }
    }

    /// Indices of the marked items, nearest to the focus first.
    pub fn order(&self, marked: &[bool], center: impl Fn(usize) -> [f32; 2]) -> Vec<usize> {
        let distance = |i: usize| {
            let c = center(i);
            (c[0] - self.focus[0]).hypot(c[1] - self.focus[1])
        };
        let mut order: Vec<(f32, usize)> = (0..marked.len()).filter(|&i| marked[i]).map(|i| (distance(i), i)).collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        order.into_iter().map(|(_, i)| i).collect()
    }
}

/// Batches of items while a budget lasts (`Budget::batches`).
pub struct Batches<'a> {
    budget: Budget,
    items: &'a [usize],
    started: bool,
}

impl<'a> Iterator for Batches<'a> {
    type Item = &'a [usize];

    fn next(&mut self) -> Option<&'a [usize]> {
        if self.items.is_empty() {
            return None;
        }
        let size = match (self.budget.spent(), self.started) {
            (false, _) => BATCH,
            (true, false) => 1,
            (true, true) => return None,
        };
        let (batch, rest) = self.items.split_at(size.min(self.items.len()));
        self.items = rest;
        self.started = true;
        Some(batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spent_budget_still_builds_one_item() {
        let items = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3];
        let spent = Budget::from_now(Duration::ZERO, [0.; 2]);
        assert_eq!(spent.batches(&items, false).collect::<Vec<_>>(), vec![&[3][..]]);
        assert_eq!(spent.batches(&items, true).count(), 0);
        let unlimited = Budget::unlimited([0.; 2]);
        let all: Vec<usize> = unlimited.batches(&items, true).flatten().copied().collect();
        assert_eq!(all, items);
        assert_eq!(unlimited.batches(&items, false).next().map(<[usize]>::len), Some(BATCH));
    }
}
