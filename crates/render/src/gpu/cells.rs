//! Culling cells for instanced models: square blocks of `CELL`×`CELL` tiles with a
//! bounding sphere and an XZ box each.
//!
//! Instances of one model are stored cell by cell, so the instances of a set of cells
//! are a few contiguous ranges: the camera frustum (scene draws) and the shadow box
//! (caster draws) each pick cells, and consecutive picked runs merge into one draw.
use std::ops::Range;

use crate::scene::layout::tile_to_world;
use crate::world::state::RenderWorld;

/// Side of a culling cell in tiles.
pub const CELL: u32 = 8;
/// Height margin of a culling cell's sphere: the tallest world models and terrain.
const CELL_HEIGHT: f32 = 2.5;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct CellGrid {
    cols: u32,
    width: u32,
    spheres: Vec<([f32; 3], f32)>,
    /// XZ box of each cell's tiles, widened by one hex radius.
    boxes: Vec<[[f32; 2]; 2]>,
}

impl CellGrid {
    pub fn of(world: &RenderWorld) -> Self {
        let width = world.width.max(1);
        let height = world.height.max(1);
        let cols = width.div_ceil(CELL);
        let rows = height.div_ceil(CELL);
        let mut spheres = Vec::with_capacity((cols * rows) as usize);
        let mut boxes = Vec::with_capacity((cols * rows) as usize);
        for cy in 0..rows {
            for cx in 0..cols {
                let q = [cx * CELL, ((cx + 1) * CELL).min(width) - 1];
                let r = [cy * CELL, ((cy + 1) * CELL).min(height) - 1];
                let corners = [(q[0], r[0]), (q[1], r[0]), (q[0], r[1]), (q[1], r[1])].map(|(q, r)| tile_to_world(q, r));
                let mut lo = [f32::INFINITY; 2];
                let mut hi = [f32::NEG_INFINITY; 2];
                for (x, z) in corners {
                    lo = [lo[0].min(x), lo[1].min(z)];
                    hi = [hi[0].max(x), hi[1].max(z)];
                }
                let center = [(lo[0] + hi[0]) * 0.5, CELL_HEIGHT * 0.5, (lo[1] + hi[1]) * 0.5];
                let half = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2)).sqrt() * 0.5;
                spheres.push((center, half + 1.0 + CELL_HEIGHT));
                boxes.push([[lo[0] - 1.0, lo[1] - 1.0], [hi[0] + 1.0, hi[1] + 1.0]]);
            }
        }
        Self { cols, width, spheres, boxes }
    }

    pub fn cell(&self, tile: u32) -> u32 {
        let (q, r) = (tile % self.width.max(1), tile / self.width.max(1));
        (r / CELL) * self.cols + q / CELL
    }

    pub fn len(&self) -> usize {
        self.spheres.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spheres.is_empty()
    }

    pub fn sphere(&self, cell: u32) -> ([f32; 3], f32) {
        self.spheres[cell as usize]
    }

    /// Cells whose sphere is in the camera frustum.
    pub fn in_frustum(&self, planes: &[glam::Vec4; 6]) -> Vec<bool> {
        let seen = |(center, radius): &([f32; 3], f32)| crate::camera::CameraController::sphere_visible(planes, *center, *radius);
        self.spheres.iter().map(seen).collect()
    }

    /// Cells whose XZ box overlaps the XZ box `low`..`high` (shadow casters).
    pub fn in_box(&self, low: [f32; 2], high: [f32; 2]) -> Vec<bool> {
        let overlaps = |[lo, hi]: &[[f32; 2]; 2]| hi[0] >= low[0] && lo[0] <= high[0] && hi[1] >= low[1] && lo[1] <= high[1];
        self.boxes.iter().map(overlaps).collect()
    }
}

/// Instances of one model in one cell: `count` instances from `first`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub cell: u32,
    pub first: u32,
    pub count: u32,
}

/// Instance ranges of the runs whose cell is picked; runs that touch merge.
pub fn picked_ranges(runs: &[Run], picked: &[bool]) -> Vec<Range<u32>> {
    let mut ranges: Vec<Range<u32>> = Vec::new();
    for run in runs {
        // A cell outside the grid (a world of another size) is kept: never lose an instance.
        if !picked.get(run.cell as usize).copied().unwrap_or(true) {
            continue;
        }
        match ranges.last_mut() {
            Some(last) if last.end == run.first => last.end += run.count,
            _ => ranges.push(run.first..run.first + run.count),
        }
    }
    ranges
}

/// Appends the runs of one group of instances already sorted by cell, numbered from
/// `first`. Runs never extend a run of an earlier group.
pub fn push_runs(runs: &mut Vec<Run>, cells: impl Iterator<Item = u32>, first: u32) {
    let start = runs.len();
    let mut index = first;
    for cell in cells {
        let open = runs.len() > start;
        match runs.last_mut() {
            Some(run) if open && run.cell == cell && run.first + run.count == index => run.count += 1,
            _ => runs.push(Run { cell, first: index, count: 1 }),
        }
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picked_runs_merge_only_when_contiguous() {
        let runs = [
            Run { cell: 0, first: 0, count: 2 },
            Run { cell: 3, first: 2, count: 1 },
            Run { cell: 5, first: 3, count: 4 },
        ];
        let picked = [true, false, false, true, false, false];
        assert_eq!(picked_ranges(&runs, &picked), vec![0..3]);
        let picked = [true, false, false, false, false, true];
        assert_eq!(picked_ranges(&runs, &picked), vec![0..2, 3..7]);
    }

    #[test]
    fn every_tile_has_a_cell_whose_box_contains_it() {
        let world = RenderWorld { width: 21, height: 13, ..Default::default() };
        let grid = CellGrid::of(&world);
        for tile in 0..21 * 13 {
            let (x, z) = tile_to_world(tile % 21, tile / 21);
            let cell = grid.cell(tile);
            let near = grid.in_box([x - 0.01, z - 0.01], [x + 0.01, z + 0.01]);
            assert!(near[cell as usize], "tile {tile} outside its cell box");
        }
    }
}
