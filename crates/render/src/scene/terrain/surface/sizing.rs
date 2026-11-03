//! Sizing field and point placement (platform/MESHING.md §2.4).
//!
//! The field answers "how long may an edge be here". Points come from a
//! hierarchical lattice: a cell of level k is a leaf when its size is the
//! finest level size not smaller than the requested length anywhere in it.
//! Neighbouring leaves differ by at most one level (2:1 balance), every leaf
//! gives one point jittered by a hash of the cell, so the triangulation needs
//! no refinement and stays deterministic.
use super::hydrology::Point;
use crate::scene::terrain::fields::hash_u32;
use rustc_hash::FxHashSet;

/// Cell sizes of the lattice levels, coarse to fine. The two coarsest serve
/// only cells nobody has seen: known ground asks for 0.45 at most.
pub const LEVELS: [f32; 6] = [1.92, 0.96, 0.48, 0.24, 0.12, 0.06];
/// Global bounds of the sizing field.
pub const MIN_EDGE: f32 = 0.06;
pub const MAX_EDGE: f32 = 1.92;
/// Lattice points closer than this share of their cell size to a contour
/// are dropped, so a contour never gets a sliver beside it.
pub const CONTOUR_CLEARANCE: f32 = 0.3;
const JITTER: f32 = 0.22;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Cell {
    level: u8,
    x: i32,
    z: i32,
}

/// A lattice point and the size of the cell that produced it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatticePoint {
    pub position: Point,
    pub size: f32,
}

impl Cell {
    fn size(self) -> f32 {
        LEVELS[self.level as usize]
    }
    fn center(self) -> Point {
        [(self.x as f32 + 0.5) * self.size(), (self.z as f32 + 0.5) * self.size()]
    }
    fn children(self) -> [Cell; 4] {
        let (x, z, level) = (self.x * 2, self.z * 2, self.level + 1);
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dz)| Cell { level, x: x + dx, z: z + dz })
    }
    fn ancestor(self, level: u8) -> Cell {
        let shift = self.level - level;
        Cell { level, x: self.x >> shift, z: self.z >> shift }
    }
    fn neighbours(self) -> impl Iterator<Item = Cell> {
        (-1..=1)
            .flat_map(move |dz| (-1..=1).map(move |dx| (dx, dz)))
            .filter(|&d| d != (0, 0))
            .map(move |(dx, dz)| Cell { level: self.level, x: self.x + dx, z: self.z + dz })
    }
    fn jittered(self) -> Point {
        let seed = hash_u32(hash_u32(self.x as u32 ^ (self.level as u32) << 28).wrapping_add(self.z as u32));
        let jx = (seed & 255) as f32 / 255. - 0.5;
        let jz = ((seed >> 8) & 255) as f32 / 255. - 0.5;
        let c = self.center();
        [c[0] + jx * 2. * JITTER * self.size(), c[1] + jz * 2. * JITTER * self.size()]
    }
}

/// Index of the finest level whose size is not smaller than `edge`.
pub fn level_for(edge: f32) -> u8 {
    let edge = edge.clamp(MIN_EDGE, MAX_EDGE);
    let mut level = 0;
    while (level as usize + 1) < LEVELS.len() && LEVELS[level as usize + 1] >= edge - 1e-6 {
        level += 1;
    }
    level
}

/// Jittered lattice points over `[low, high]`. `edge(p, slack)` must return
/// the smallest requested edge length within `slack` of `p`.
pub fn lattice(low: Point, high: Point, edge: impl Fn(Point, f32) -> f32) -> Vec<LatticePoint> {
    let size = LEVELS[0];
    let mut leaves = FxHashSet::default();
    for z in (low[1] / size).floor() as i32..=(high[1] / size).ceil() as i32 {
        for x in (low[0] / size).floor() as i32..=(high[0] / size).ceil() as i32 {
            refine(Cell { level: 0, x, z }, &edge, &mut leaves);
        }
    }
    balance(&mut leaves);
    // One order whatever the set's: the triangulation sees the same input.
    let mut leaves: Vec<Cell> = leaves.into_iter().collect();
    leaves.sort_unstable();
    leaves
        .into_iter()
        .map(|cell| LatticePoint { position: cell.jittered(), size: cell.size() })
        .collect()
}

fn refine(cell: Cell, edge: &impl Fn(Point, f32) -> f32, leaves: &mut FxHashSet<Cell>) {
    let half_diagonal = cell.size() * std::f32::consts::FRAC_1_SQRT_2;
    if level_for(edge(cell.center(), half_diagonal)) <= cell.level {
        leaves.insert(cell);
        return;
    }
    for child in cell.children() {
        refine(child, edge, leaves);
    }
}

/// Split coarse leaves until every neighbour of a leaf is at most one level coarser.
fn balance(leaves: &mut FxHashSet<Cell>) {
    let mut pending: Vec<Cell> = leaves.iter().copied().collect();
    pending.sort_unstable();
    while let Some(cell) = pending.pop() {
        if !leaves.contains(&cell) {
            continue;
        }
        for neighbour in cell.neighbours() {
            let Some(coarse) = coarse_leaf(leaves, neighbour) else { continue };
            leaves.remove(&coarse);
            for child in coarse.children() {
                leaves.insert(child);
                pending.push(child);
            }
            pending.push(cell);
        }
    }
}

/// The leaf covering `cell` when it is at least two levels coarser.
fn coarse_leaf(leaves: &FxHashSet<Cell>, cell: Cell) -> Option<Cell> {
    (0..cell.level.saturating_sub(1))
        .map(|level| cell.ancestor(level))
        .find(|ancestor| leaves.contains(ancestor))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(p: Point, slack: f32) -> f32 {
        // A fine band along x = 3, coarse elsewhere.
        if (p[0] - 3.).abs() < 0.2 + slack { 0.06 } else { 0.45 }
    }

    #[test]
    fn level_is_the_finest_size_not_smaller_than_the_request() {
        assert_eq!(level_for(1.92), 0);
        assert_eq!(level_for(1.5), 0);
        assert_eq!(level_for(0.45), 2);
        assert_eq!(level_for(0.24), 3);
        assert_eq!(level_for(0.15), 3);
        assert_eq!(level_for(0.1), 4);
        assert_eq!(level_for(0.06), 5);
        assert_eq!(level_for(0.01), 5);
    }

    #[test]
    fn lattice_is_balanced_deterministic_and_dense_only_in_the_band() {
        let points = lattice([0., 0.], [6., 4.], band);
        assert_eq!(points, lattice([0., 0.], [6., 4.], band));
        let near: Vec<_> = points.iter().filter(|p| (p.position[0] - 3.).abs() < 0.15).collect();
        assert!(near.iter().all(|p| p.size == 0.06), "the band gets the finest level");
        let far = points.iter().filter(|p| (p.position[0] - 3.).abs() > 1.5);
        assert!(far.clone().all(|p| p.size == 0.48), "plains stay coarse");
        // 2:1: within one coarse cell of the band no point is more than one level apart.
        for a in &points {
            for b in &points {
                let d = (a.position[0] - b.position[0]).hypot(a.position[1] - b.position[1]);
                if d < 0.5 * (a.size + b.size) {
                    assert!(a.size / b.size <= 2.01 && b.size / a.size <= 2.01, "{a:?} {b:?}");
                }
            }
        }
    }
}
