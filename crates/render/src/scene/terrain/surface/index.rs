//! Uniform-grid index of 2D segments for nearest-segment queries.
//! A query visits rings of bins around the point and stops as soon as no
//! unvisited bin can hold a closer segment, so it returns exactly what a full
//! scan would, including the lowest index among equally distant segments.
use super::hydrology::Point;

/// Bins are stored densely over the bounding box of all segments: `starts`
/// holds the offset of each bin's run in `items`. `empty_rings` holds, per
/// bin, how many rings around it hold no segment at all (the Chebyshev
/// distance in bins to the nearest occupied bin), so a query far from every
/// segment skips the empty rings instead of visiting them.
#[derive(Debug, Clone, Default)]
pub struct SegmentIndex {
    cell: f32,
    low: (i32, i32),
    size: (i32, i32),
    starts: Vec<u32>,
    items: Vec<u32>,
    empty_rings: Vec<u16>,
}

/// Nearest segment found by a query: its index and its ranking distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nearest {
    pub index: u32,
    pub distance: f32,
}

impl SegmentIndex {
    pub fn new(segments: impl IntoIterator<Item = [Point; 2]>, cell: f32) -> Self {
        let key = |p: Point| ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32);
        let boxes: Vec<_> = segments
            .into_iter()
            .map(|[a, b]| (key([a[0].min(b[0]), a[1].min(b[1])]), key([a[0].max(b[0]), a[1].max(b[1])])))
            .collect();
        if boxes.is_empty() {
            return Self { cell, ..Self::default() };
        }
        let low = boxes.iter().fold((i32::MAX, i32::MAX), |l, b| (l.0.min(b.0 .0), l.1.min(b.0 .1)));
        let high = boxes.iter().fold((i32::MIN, i32::MIN), |h, b| (h.0.max(b.1 .0), h.1.max(b.1 .1)));
        let size = (high.0 - low.0 + 1, high.1 - low.1 + 1);
        let bin_of = |x: i32, z: i32| ((z - low.1) * size.0 + x - low.0) as usize;
        // Counting sort into one array: per-bin counts, offsets, then items.
        let mut starts = vec![0u32; (size.0 * size.1) as usize + 1];
        for (from, to) in &boxes {
            for z in from.1..=to.1 {
                for x in from.0..=to.0 {
                    starts[bin_of(x, z) + 1] += 1;
                }
            }
        }
        for i in 1..starts.len() {
            starts[i] += starts[i - 1];
        }
        let mut fill = starts.clone();
        let mut items = vec![0u32; *starts.last().unwrap() as usize];
        for (i, (from, to)) in boxes.iter().enumerate() {
            for z in from.1..=to.1 {
                for x in from.0..=to.0 {
                    let bin = bin_of(x, z);
                    items[fill[bin] as usize] = i as u32;
                    fill[bin] += 1;
                }
            }
        }
        let empty_rings = empty_rings(&starts, size);
        Self { cell, low, size, starts, items, empty_rings }
    }

    /// Segments whose bounding box touches the bin at integer grid coordinates.
    pub fn bin(&self, x: i32, z: i32) -> &[u32] {
        let (x, z) = (x - self.low.0, z - self.low.1);
        if x < 0 || z < 0 || x >= self.size.0 || z >= self.size.1 {
            return &[];
        }
        let i = (z * self.size.0 + x) as usize;
        &self.items[self.starts[i] as usize..self.starts[i + 1] as usize]
    }

    /// Every segment whose bounding box touches the bin of `p`.
    pub fn candidates(&self, p: Point) -> &[u32] {
        if self.items.is_empty() {
            return &[];
        }
        self.bin((p[0] / self.cell).floor() as i32, (p[1] / self.cell).floor() as i32)
    }

    /// Every segment whose bounding box touches a bin of the box spanned by
    /// `a` and `b`, each once, in ascending order.
    pub fn candidates_between(&self, a: Point, b: Point) -> Vec<u32> {
        if self.items.is_empty() {
            return vec![];
        }
        let key = |v: f32| (v / self.cell).floor() as i32;
        let mut out = vec![];
        for z in key(a[1].min(b[1]))..=key(a[1].max(b[1])) {
            for x in key(a[0].min(b[0]))..=key(a[0].max(b[0])) {
                out.extend_from_slice(self.bin(x, z));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The segment with the smallest `distance(index)` among those within
    /// `radius` of `p`; `distance` must not be smaller than the Euclidean one.
    pub fn nearest(&self, p: Point, radius: f32, mut distance: impl FnMut(u32) -> f32) -> Option<Nearest> {
        if self.items.is_empty() {
            return None;
        }
        let center = ((p[0] / self.cell).floor() as i32, (p[1] / self.cell).floor() as i32);
        // Rings closer than the nearest occupied bin hold nothing; a segment
        // in ring k is at least (k - 1) cells away.
        let first = self.first_occupied_ring(center);
        if (first as f32 - 1.) * self.cell > radius {
            return None;
        }
        let mut best: Option<Nearest> = None;
        for ring in first..=self.rings_to_cover(center) {
            for dz in -ring..=ring {
                // Inner rows of a ring hold only its two side bins.
                let step = if dz.abs() == ring { 1 } else { 2 * ring as usize };
                for dx in (-ring..=ring).step_by(step) {
                    for &index in self.bin(center.0 + dx, center.1 + dz) {
                        let candidate = Nearest { index, distance: distance(index) };
                        if is_better(candidate, best) {
                            best = Some(candidate);
                        }
                    }
                }
            }
            let unvisited = ring as f32 * self.cell;
            if best.is_some_and(|b| b.distance <= unvisited) || unvisited > radius {
                break;
            }
        }
        best.filter(|b| b.distance <= radius)
    }

    /// Whether some segment has `distance(index) <= radius`; stops at the first.
    pub fn any_within(&self, p: Point, radius: f32, mut distance: impl FnMut(u32) -> f32) -> bool {
        if self.items.is_empty() {
            return false;
        }
        let center = ((p[0] / self.cell).floor() as i32, (p[1] / self.cell).floor() as i32);
        let first = self.first_occupied_ring(center);
        let last = ((radius / self.cell).floor() as i32 + 1).min(self.rings_to_cover(center));
        for ring in first..=last {
            for dz in -ring..=ring {
                let step = if dz.abs() == ring { 1 } else { 2 * ring as usize };
                for dx in (-ring..=ring).step_by(step.max(1)) {
                    if self.bin(center.0 + dx, center.1 + dz).iter().any(|&i| distance(i) <= radius) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// A lower bound of the ring of the nearest occupied bin around `center`.
    fn first_occupied_ring(&self, center: (i32, i32)) -> i32 {
        let clamped = (center.0.clamp(self.low.0, self.low.0 + self.size.0 - 1), center.1.clamp(self.low.1, self.low.1 + self.size.1 - 1));
        let outside = (center.0 - clamped.0).abs().max((center.1 - clamped.1).abs());
        let i = ((clamped.1 - self.low.1) * self.size.0 + clamped.0 - self.low.0) as usize;
        outside.max(self.empty_rings[i] as i32 - outside)
    }

    fn rings_to_cover(&self, center: (i32, i32)) -> i32 {
        let high = (self.low.0 + self.size.0 - 1, self.low.1 + self.size.1 - 1);
        let dx = (center.0 - self.low.0).abs().max((high.0 - center.0).abs());
        let dz = (center.1 - self.low.1).abs().max((high.1 - center.1).abs());
        dx.max(dz)
    }
}

/// Chebyshev distance in bins from every bin to the nearest occupied one, by
/// two sweeps of the 8-neighbour chamfer transform (exact for Chebyshev).
fn empty_rings(starts: &[u32], size: (i32, i32)) -> Vec<u16> {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let far = u16::MAX - 1;
    let mut d: Vec<u16> = (0..w * h).map(|i| if starts[i + 1] > starts[i] { 0 } else { far }).collect();
    for z in 0..h {
        for x in 0..w {
            let mut v = d[z * w + x];
            if x > 0 { v = v.min(d[z * w + x - 1] + 1); }
            if z > 0 {
                v = v.min(d[(z - 1) * w + x] + 1);
                if x > 0 { v = v.min(d[(z - 1) * w + x - 1] + 1); }
                if x + 1 < w { v = v.min(d[(z - 1) * w + x + 1] + 1); }
            }
            d[z * w + x] = v;
        }
    }
    for z in (0..h).rev() {
        for x in (0..w).rev() {
            let mut v = d[z * w + x];
            if x + 1 < w { v = v.min(d[z * w + x + 1] + 1); }
            if z + 1 < h {
                v = v.min(d[(z + 1) * w + x] + 1);
                if x > 0 { v = v.min(d[(z + 1) * w + x - 1] + 1); }
                if x + 1 < w { v = v.min(d[(z + 1) * w + x + 1] + 1); }
            }
            d[z * w + x] = v;
        }
    }
    d
}

fn is_better(candidate: Nearest, best: Option<Nearest>) -> bool {
    let Some(best) = best else { return true };
    candidate.distance < best.distance || (candidate.distance == best.distance && candidate.index < best.index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::terrain::surface::hydrology::closest;

    #[test]
    fn indexed_nearest_matches_a_full_scan_including_ties() {
        let mut segments = vec![];
        for i in 0..60 {
            let t = i as f32 * 0.37;
            segments.push([[t.sin() * 6., t.cos() * 4.], [t.sin() * 6. + 0.4, t.cos() * 4. + 0.3]]);
        }
        // An exact tie: two segments sharing an endpoint.
        segments.push([[20., 20.], [21., 20.]]);
        segments.push([[21., 20.], [22., 20.]]);
        let index = SegmentIndex::new(segments.iter().copied(), 1.0);
        for i in 0..400 {
            let p = [(i as f32 * 0.731).sin() * 9., (i as f32 * 0.417).cos() * 7.];
            let scan = (0..segments.len() as u32)
                .map(|s| Nearest { index: s, distance: closest(p, segments[s as usize][0], segments[s as usize][1]).0 })
                .reduce(|a, b| if is_better(b, Some(a)) { b } else { a });
            let found = index.nearest(p, f32::INFINITY, |s| closest(p, segments[s as usize][0], segments[s as usize][1]).0);
            assert_eq!(found, scan);
        }
        let tie = index.nearest([21., 21.], 5., |s| closest([21., 21.], segments[s as usize][0], segments[s as usize][1]).0);
        assert_eq!(tie.unwrap().index, 60);
        let far = [40., 40.];
        assert!(index.nearest(far, 2., |s| closest(far, segments[s as usize][0], segments[s as usize][1]).0).is_none());
    }
}
