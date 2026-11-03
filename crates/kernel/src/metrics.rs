//! Work counters for algorithmic regression CI (§49 platform/project.md).
//!
//! We gate work counters, not wall-clock +3% on shared CI:
//! `single-unit move must not scan all 50k units` is a hard bound,
//! wall-clock is informational only / a dedicated runner.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkCounters {
    /// How many entities were visited (units/cities).
    pub entities_visited: u64,
    /// How many tiles were visited.
    pub tiles_visited: u64,
    /// Approximate bytes allocated by a derived update.
    pub bytes_allocated: u64,
}

impl WorkCounters {
    pub fn add(&mut self, other: WorkCounters) {
        self.entities_visited += other.entities_visited;
        self.tiles_visited += other.tiles_visited;
        self.bytes_allocated += other.bytes_allocated;
    }
}

/// Bound for a small local action on a large map:
/// one move must not scan the whole world.
pub fn assert_local_move_bounds(c: &WorkCounters, tiles: usize) {
    assert!(
        c.tiles_visited <= 1024,
        "local move visited {} tiles on {}-tile map: must not scan world",
        c.tiles_visited,
        tiles
    );
    assert!(
        c.entities_visited <= 64,
        "local move visited {} entities: must not scan all units",
        c.entities_visited
    );
}
