//! Region fingerprints: a hash of everything a region's part reads (platform/MESHING.md §2.5,
//! "the version of its dependencies").
//!
//! A part (`region_part`) is a pure function of the field near the region, the
//! region's seams and contour pieces. Each contributor feeds the items it holds to the
//! regions whose query box they touch: the box of the region's cells widened by
//! `REACH`, the farthest any field query of a region point looks (lattice cells reach
//! 1.92 past the region and ask with slack 1.36; the coast geology blends edges 6.4
//! away). Items go in in the contributor's own order, so an order that a tie could
//! depend on is part of the hash too. Equal fingerprints mean equal parts: a rebuild
//! reuses every part whose fingerprint did not change.
use super::{
    hydrology::Point,
    index::SegmentIndex,
    regions::{Layout, Levels, RegionId, SeamPlan},
};
use rustc_hash::FxHasher;
use std::hash::Hasher;

/// How far past its cells a region's field queries can look, world units.
pub const REACH: f32 = 9.0;

/// Item kinds, so two kinds with equal words never hash alike.
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum Tag {
    Cell = 1,
    Coast,
    River,
    Cliff,
    Beach,
    WallContour,
    WallStrip,
    WallFraction,
    Massif,
    Site,
    Seam,
    Piece,
    Row,
    Level,
    Relief,
    Pad,
}

/// Side of the bins regions are found in, world units.
const BIN: f32 = 8.0;

/// Collects the items of all contributors into one hash per region.
pub struct Sink {
    boxes: Vec<[Point; 2]>,
    index: SegmentIndex,
    hashers: Vec<FxHasher>,
    /// The item each region last took: a region found in several bins takes it once.
    taken: Vec<u32>,
    item: u32,
}

impl Sink {
    pub fn new(layout: &Layout, ids: &[RegionId]) -> Self {
        let boxes: Vec<[Point; 2]> = ids
            .iter()
            .map(|&id| {
                let [low, high] = layout.bounds(id);
                [[low[0] - REACH, low[1] - REACH], [high[0] + REACH, high[1] + REACH]]
            })
            .collect();
        let index = SegmentIndex::new(boxes.iter().copied(), BIN);
        let hashers = vec![FxHasher::default(); ids.len()];
        let taken = vec![u32::MAX; ids.len()];
        Self { boxes, index, hashers, taken, item: 0 }
    }

    /// An item whose data lies within `low`..`high`: fed to every region whose
    /// query box overlaps it.
    pub fn item(&mut self, tag: Tag, low: Point, high: Point, words: &[u32]) {
        self.item = self.item.wrapping_add(1);
        let bin = |v: f32| (v / BIN).floor() as i32;
        for z in bin(low[1])..=bin(high[1]) {
            for x in bin(low[0])..=bin(high[0]) {
                for &r in self.index.bin(x, z) {
                    let r = r as usize;
                    if self.taken[r] == self.item {
                        continue;
                    }
                    self.taken[r] = self.item;
                    let [bl, bh] = self.boxes[r];
                    if high[0] < bl[0] || low[0] > bh[0] || high[1] < bl[1] || low[1] > bh[1] {
                        continue;
                    }
                    write(&mut self.hashers[r], tag, words);
                }
            }
        }
    }

    /// A segment item (its box is the segment's) with a margin.
    pub fn segment(&mut self, tag: Tag, [a, b]: [Point; 2], margin: f32, words: &[u32]) {
        let low = [a[0].min(b[0]) - margin, a[1].min(b[1]) - margin];
        let high = [a[0].max(b[0]) + margin, a[1].max(b[1]) + margin];
        self.item(tag, low, high, words);
    }

    /// Data of one region only (its seams and contour pieces).
    fn region(&mut self, region: usize, tag: Tag, words: &[u32]) {
        write(&mut self.hashers[region], tag, words);
    }

    pub fn finish(self) -> Vec<u64> {
        self.hashers.into_iter().map(|h| h.finish()).collect()
    }
}

fn write(hasher: &mut FxHasher, tag: Tag, words: &[u32]) {
    hasher.write_u32(tag as u32);
    hasher.write_u32(words.len() as u32);
    for &w in words {
        hasher.write_u32(w);
    }
}

/// Bits of points, for item words.
pub fn bits(points: &[Point]) -> Vec<u32> {
    points.iter().flat_map(|p| [p[0].to_bits(), p[1].to_bits()]).collect()
}

/// The fingerprint of every region in `ids`: field contributors, then the region's
/// detail level, its own seams (cut by the levels on both sides), contour pieces and
/// the wall rows of their points.
pub fn fingerprints(field: &super::field::Landscape, layout: &Layout, plan: &SeamPlan, ids: &[RegionId], levels: &Levels) -> Vec<u64> {
    let mut sink = Sink::new(layout, ids);
    field.fingerprint(&mut sink);
    for (r, &id) in ids.iter().enumerate() {
        sink.region(r, Tag::Level, &[u32::from(levels.of(id))]);
        let (seams, pieces) = plan.constraints(layout, id);
        for (tag, segments) in [(Tag::Seam, seams), (Tag::Piece, pieces)] {
            for segment in segments {
                sink.region(r, tag, &bits(&segment));
                for p in segment {
                    let row = plan.row(p).map_or(u32::MAX, f32::to_bits);
                    sink.region(r, Tag::Row, &[row]);
                }
            }
        }
    }
    sink.finish()
}
