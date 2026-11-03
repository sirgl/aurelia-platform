//! Rocky or sandy coast, decided from map data (decision V3 in ru/PLAN.en.md).
//!
//! The rules know no cliffs, so this is a visual property of a coastal edge,
//! derived on the render side from known tiles in one place, `is_rocky_coast`.
//! A coastal edge joins a known land cell and a known sea cell; all coastal
//! edges of a land cell share its flag. They are cliff edges when
//! 1. the land cell is a hill or a mountain; or
//! 2. a known coastal hill or mountain is at most `HEADLAND_REACH` cells away,
//!    so a rocky headland continues along its shore; or
//! 3. the tile hash of the land cell selects it: one coastal cell in
//!    `OUTCROP_SHARE` is a rocky outcrop.
//! Every other coastal edge is a beach. Only known cells are read, so hidden
//! terrain never changes the coast.
//!
//! In the field the rocky coast is a stage-4 cutout (`CoastGeology::cut`): the
//! rock-ledge profile weighted by the cliff share, over the beach of stage 3.
use super::{
    banks::{COAST_EXTENT, SEA_LEVEL},
    biomes::{smooth, Ground},
    cutouts::Cut,
    hydrology::{closest, noise, Point},
    index::SegmentIndex,
    transitions::{CoastProfile, CoastSection, CoastShape},
};
use crate::{
    scene::{
        fog::fog_at,
        layout::EDGE_NEIGHBORS,
        terrain::{corner_lattice, fields::hash_u32, lattice_to_world},
    },
    world::state::RenderWorld,
};
use kernel::ids::TileId;

/// One flat coastal cell in this many is a rocky outcrop.
const OUTCROP_SHARE: u32 = 3;
/// A rocky headland continues this many cells along its shore.
const HEADLAND_REACH: i32 = 2;
/// Along the coast, beach and cliff blend over twice this distance.
const TRANSITION: f32 = 0.4;
/// Farther than this, an edge kind does not influence the blend.
const REACH: f32 = 6.0;

/// Cliff and beach edges of the known coast, as hex-edge segments.
#[derive(Default)]
pub struct CoastGeology {
    cliffs: Edges,
    beaches: Edges,
}

#[derive(Default)]
struct Edges {
    segments: Vec<[Point; 2]>,
    index: SegmentIndex,
}

impl Edges {
    fn new(segments: Vec<[Point; 2]>) -> Self {
        let index = SegmentIndex::new(segments.iter().copied(), 1.0);
        Self { segments, index }
    }
    /// Distance to the nearest edge, or `REACH` when none is within `radius` (at most `REACH`).
    fn distance(&self, p: Point, radius: f32) -> f32 {
        let distance = |i: u32| {
            let [a, b] = self.segments[i as usize];
            closest(p, a, b).0
        };
        self.index.nearest(p, radius, distance).map_or(REACH, |n| n.distance)
    }
}

impl CoastGeology {
    pub fn new(world: &RenderWorld) -> Self {
        let mut cliffs = vec![];
        let mut beaches = vec![];
        for (land, segment) in coastal_edges(world) {
            if is_rocky_coast(world, land) {
                cliffs.push(segment);
            } else {
                beaches.push(segment);
            }
        }
        Self::from_edges(cliffs, beaches)
    }

    pub fn from_edges(cliffs: Vec<[Point; 2]>, beaches: Vec<[Point; 2]>) -> Self {
        Self { cliffs: Edges::new(cliffs), beaches: Edges::new(beaches) }
    }

    /// Cliff and beach edges for region fingerprints.
    pub fn fingerprint(&self, sink: &mut super::fingerprint::Sink) {
        use super::fingerprint::{bits, Tag};
        for (tag, edges) in [(Tag::Cliff, &self.cliffs), (Tag::Beach, &self.beaches)] {
            for &segment in &edges.segments {
                sink.segment(tag, segment, 0., &bits(&segment));
            }
        }
    }

    /// Share of cliff in the coast profile at `p`: one near cliff edges, zero
    /// near beach edges, a smooth blend where they meet. Continuous in XZ, so
    /// a narrow peninsula with two different shores has no seam on its ridge.
    pub fn rock(&self, p: Point) -> f32 {
        // No cliff within reach: beach. Every caller is within a few units of a
        // coastal edge, where this is what the blend below gives anyway; deciding
        // it locally keeps the share a function of the edges near `p` alone.
        let Some(cliff) = self.cliffs.index.nearest(p, REACH, |i| {
            let [a, b] = self.cliffs.segments[i as usize];
            closest(p, a, b).0
        }) else {
            return 0.;
        };
        let cliff = cliff.distance;
        // The blend saturates once one kind leads by `TRANSITION`: the beach
        // matters only within that of the nearest cliff.
        let beach = self.beaches.distance(p, (cliff + TRANSITION).min(REACH));
        let lead = beach - cliff;
        let t = (0.5 + lead / (2. * TRANSITION)).clamp(0., 1.);
        t * t * (3. - 2. * t)
    }
}

impl CoastGeology {
    /// The stage-4 cutout of a rocky coast at `shore` inland of the waterline:
    /// the rock-ledge profile with `weight`, the cliff share at the point.
    pub fn cut(p: Point, shore: f32, ground: &Ground, shape: CoastShape, weight: f32) -> Cut {
        Cut { weight, section: cliff_section(p, shore, ground, shape) }
    }
}

/// The rock ledge cuts the land-only plateau. Past the profile the plateau
/// eases back to the base height, which the sea no longer lowers at that
/// distance. A cliff stands on a raised level headland that only far inland
/// eases down to the ordinary ground; a slope right behind the brow reads as
/// a dome. Its stone weight covers the wall and spills over the rim as
/// broken flagstone patches.
fn cliff_section(p: Point, shore: f32, ground: &Ground, shape: CoastShape) -> CoastSection {
    let back = smooth((shore - 1.) / (COAST_EXTENT - 1.));
    let headland = 0.32 * (1. - smooth((shore - 0.9) / (COAST_EXTENT - 0.9)));
    // A hill's crown would rise as a mound right behind the brow: the cliff
    // top is the hill's level shoulder, its relief returns farther inland.
    let level_top = ground.hill * (1. - smooth((shore - 1.2) / (COAST_EXTENT - 1.2)));
    let plateau = ground.land_height * (1. - back) + ground.height * back + headland - level_top;
    let cliff = CoastProfile::RockLedge.sample_shaped(shore, SEA_LEVEL, plateau.max(0.10), shape);
    let across = shore / shape.width;
    let patch = smooth((noise(p[0] * 1.7, p[1] * 1.7, 3511) - 0.33) / 0.07);
    let tuft = smooth((noise(p[0] * 3.1, p[1] * 3.1, 3613) - 0.55) / 0.12);
    let shoulder = smooth((across - 0.62) / 0.3);
    let wall_stone = cliff.stone * (1. - shoulder * (1. - patch));
    // Behind the brow the stones thin out into the meadow in broken patches.
    let rim = 1. - smooth((across - 0.8) / 0.9);
    let spill = 0.95 * rim * smooth((patch - 0.15) / 0.6) * (1. - 0.6 * tuft);
    CoastSection { height: cliff.height, sand: 0., stone: wall_stone.max(spill) }
}

/// The single rule for rocky coast; see the module documentation.
pub fn is_rocky_coast(world: &RenderWorld, land: TileId) -> bool {
    rocky(world, land) || near_rocky_coast(world, land) || hash_u32(land.0 ^ 0xC11F_F5ED) % OUTCROP_SHARE == 0
}

fn rocky(world: &RenderWorld, tile: TileId) -> bool {
    let code = world.terrain[tile.0 as usize].terrain;
    world.terrain_kinds.is_hills(code) || world.terrain_kinds.is_mountains(code)
}

fn known(world: &RenderWorld, tile: TileId) -> bool {
    fog_at(world, tile) >= 0.4
}

fn is_sea(world: &RenderWorld, tile: TileId) -> bool {
    world.is_water(tile.0 as usize) && known(world, tile)
}

/// A known hill or mountain with a known sea neighbour within `HEADLAND_REACH`.
fn near_rocky_coast(world: &RenderWorld, land: TileId) -> bool {
    let (q, r) = ((land.0 % world.width) as i32, (land.0 / world.width) as i32);
    for dr in -HEADLAND_REACH..=HEADLAND_REACH {
        for dq in -HEADLAND_REACH..=HEADLAND_REACH {
            let inside = (0..world.width as i32).contains(&(q + dq)) && (0..world.height as i32).contains(&(r + dr));
            if !inside || (dq.abs() + dr.abs() + (dq + dr).abs()) / 2 > HEADLAND_REACH {
                continue;
            }
            let tile = TileId((r + dr) as u32 * world.width + (q + dq) as u32);
            if known(world, tile) && rocky(world, tile) && neighbours(world, tile).any(|(_, n)| is_sea(world, n)) {
                return true;
            }
        }
    }
    false
}

/// Edge neighbours inside the map with the edge direction leading to them.
fn neighbours(world: &RenderWorld, tile: TileId) -> impl Iterator<Item = (usize, TileId)> + '_ {
    let (q, r) = ((tile.0 % world.width) as i32, (tile.0 / world.width) as i32);
    EDGE_NEIGHBORS.iter().enumerate().filter_map(move |(d, &(dq, dr))| {
        let (nq, nr) = (q + dq, r + dr);
        let inside = nq >= 0 && nr >= 0 && nq < world.width as i32 && nr < world.height as i32;
        inside.then(|| (d, TileId(nr as u32 * world.width + nq as u32)))
    })
}

/// Every known land/sea edge as (land cell, world segment).
fn coastal_edges(world: &RenderWorld) -> Vec<(TileId, [Point; 2])> {
    let mut edges = vec![];
    for cell in &world.terrain {
        if world.terrain_kinds.is_water(cell.terrain) || !known(world, cell.tile) {
            continue;
        }
        let (q, r) = ((cell.tile.0 % world.width) as i32, (cell.tile.0 / world.width) as i32);
        for (d, sea) in neighbours(world, cell.tile) {
            if !is_sea(world, sea) {
                continue;
            }
            let a = lattice_to_world(corner_lattice(q, r, d));
            let b = lattice_to_world(corner_lattice(q, r, (d + 1) % 6));
            edges.push((cell.tile, [[a.0, a.1], [b.0, b.1]]));
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::state::TerrainCell;
    use kernel::rules::roles::{Ground, TerrainPalette};

    const HILLS: u16 = TerrainPalette::LEGACY.code(Ground::Hills);
    const SEA: u16 = TerrainPalette::LEGACY.code(Ground::Water);

    /// Columns 0..4 land (flat), 5.. sea; one hill on the coast at row 4.
    fn world() -> RenderWorld {
        let (width, height) = (8, 10);
        let count = (width * height) as usize;
        RenderWorld {
            width,
            height,
            visible: vec![1; count],
            explored: vec![1; count],
            terrain: (0..width * height)
                .map(|i| TerrainCell {
                    tile: TileId(i),
                    terrain: if i % width >= 5 { SEA } else if i == 4 * width + 4 { HILLS } else { 1 },
                    owner: None,
                    resource: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn kinds(w: &RenderWorld) -> Vec<(u32, bool)> {
        coastal_edges(w).iter().map(|e| (e.0 .0, is_rocky_coast(w, e.0))).collect()
    }

    #[test]
    fn hills_are_cliffs_and_their_headland_continues_along_the_shore() {
        let w = world();
        let hill = 4 * w.width + 4;
        let kinds = kinds(&w);
        assert!(kinds.iter().filter(|k| k.0 == hill).all(|k| k.1));
        // Coastal cells two rows away still belong to the headland.
        for row in [2, 3, 5, 6] {
            assert!(kinds.iter().any(|k| k.0 == row * w.width + 4 && k.1), "row {row}");
        }
    }

    #[test]
    fn coast_has_both_beaches_and_cliffs_and_hidden_cells_do_not_matter() {
        let mut w = world();
        let before = kinds(&w);
        assert!(before.iter().any(|k| k.1) && before.iter().any(|k| !k.1));
        // Hide the last row and turn it into hills: known edges keep their kind.
        let row = 9 * w.width;
        for q in 0..5 {
            w.visible[(row + q) as usize] = 0;
            w.explored[(row + q) as usize] = 0;
            w.terrain[(row + q) as usize].terrain = HILLS;
        }
        let after = kinds(&w);
        let known = |k: &&(u32, bool)| k.0 < row;
        assert_eq!(before.iter().filter(known).collect::<Vec<_>>(), after.iter().filter(known).collect::<Vec<_>>());
    }

    #[test]
    fn rock_share_is_one_on_cliffs_zero_on_beaches_and_continuous_between() {
        let geology = CoastGeology::from_edges(vec![[[0., 0.], [4., 0.]]], vec![[[4., 0.], [8., 0.]]]);
        assert_eq!(geology.rock([1., 0.2]), 1.);
        assert_eq!(geology.rock([7., 0.2]), 0.);
        assert!((geology.rock([4., 0.]) - 0.5).abs() < 1e-6);
        let mut previous = geology.rock([0., 0.3]);
        for i in 1..800 {
            let value = geology.rock([i as f32 * 0.01, 0.3]);
            assert!((value - previous).abs() < 0.03, "no step along the coast");
            previous = value;
        }
    }
}
