//! Cliff walls as constrained contours (platform/MESHING.md §2.3, §3).
//!
//! Along a rocky coast, stations sit every `STATION_SPACING` on the coast
//! line. Each station casts a ray inland and places one vertex per row of the
//! coast profile: the foot on the water line, ledges, and the brow. Rows are
//! joined along the coast and every station joins its rows from foot to brow,
//! so a wall is a strip of constrained columns.
//!
//! Fractures move the vertices of a station inland, never seaward: a column
//! of rock is set back by a shift shared by the stations it covers, and a
//! V-notch cuts deeper at one station, more at the brow than at the foot.
//! Heights are not moved: a wall vertex takes the field's profile height of
//! its row, so a cleft keeps a level brow and a set-back column stays a wall.
//!
//! Where the inland rays of a station would cross those of a neighbour, or
//! reach the opposite shore of a narrow peninsula, the rows of the station
//! are compressed. A narrow peninsula thus keeps a wall on both sides and a
//! level top between the brows: a mesa.
use super::{
    hydrology::{Coast, Point},
    index::SegmentIndex,
    stacks::{self, CoastProp, Foot},
    transitions::CoastShape,
};
use crate::scene::terrain::fields::hash_u32;
use rustc_hash::FxHashMap as HashMap;
use std::collections::BTreeMap;

/// Rows of a wall as fractions of the local profile width: foot, ledges, brow.
/// The profile contours 0.08, 0.24, 0.42 and 0.66 are among them, so a wall
/// meets the contours of a neighbouring beach at the same points.
pub const ROWS: [f32; 8] = [0., 0.08, 0.16, 0.24, 0.33, 0.42, 0.54, 0.66];
const BROW: usize = ROWS.len() - 1;
/// Share of a notch's depth at each row: the cleft is widest at the brow.
const NOTCH_SHARE: [f32; 8] = [0., 0., 0.5, 0.8, 0.95, 1., 1., 1.];
/// Distance between stations along the coast; this is the wall edge length.
pub const STATION_SPACING: f32 = 0.075;
/// Column widths and set-backs, and the depth of V-notches, world units.
const COLUMN_WIDTH: [f32; 2] = [0.1, 0.34];
const COLUMN_SHIFT: f32 = 0.14;
const NOTCH_DEPTH: [f32; 2] = [0.1, 0.2];
const NOTCH_CHANCE: f32 = 0.3;
/// Rows of neighbouring stations keep at least this share of their gap.
const CROSSING_MARGIN: f32 = 0.6;
/// Largest change of the brow depth limit per unit along the coast.
const LIMIT_SLOPE: f32 = 0.7;

/// Averaged seaward normals at shared coast vertices, so the contours of two
/// neighbouring coast segments start and end at the same points.
pub struct CoastNormals(BTreeMap<[u32; 2], Point>);

impl CoastNormals {
    pub fn new(coasts: &[Coast]) -> Self {
        let mut normals = BTreeMap::<[u32; 2], Point>::new();
        for c in coasts {
            for p in [c.a, c.b] {
                let n = normals.entry(p.map(f32::to_bits)).or_default();
                n[0] += c.sea[0];
                n[1] += c.sea[1];
            }
        }
        Self(normals)
    }
    /// Unit seaward normal at a coast vertex.
    pub fn at(&self, p: Point) -> Point {
        let n = self.0[&p.map(f32::to_bits)];
        let length = n[0].hypot(n[1]).max(1e-6);
        [n[0] / length, n[1] / length]
    }
}

#[derive(Debug, Clone, Copy)]
struct Station {
    coast: Point,
    inland: Point,
    depth: [f32; ROWS.len()],
}

impl Station {
    fn row(&self, k: usize) -> Point {
        [self.coast[0] + self.inland[0] * self.depth[k], self.coast[1] + self.inland[1] * self.depth[k]]
    }
}

/// The constrained walls of all rocky coast runs.
#[derive(Default)]
pub struct CliffWalls {
    /// Constraint segments: rows along the coast and columns foot to brow.
    pub contours: Vec<[Point; 2]>,
    /// Coast segments drawn by a wall; their plain coast contours are replaced.
    pub replaced: Vec<bool>,
    fractions: HashMap<[u32; 2], f32>,
    strips: Vec<[Point; 4]>,
    strip_index: SegmentIndex,
    /// Sea stacks and boulders at the foot of the walls.
    pub props: Vec<CoastProp>,
}

impl CliffWalls {
    /// `rocky(i)` says whether coast segment `i` is a cliff; `room(p, inland,
    /// depth)` how far inland from coast point `p` the shore is still the nearest.
    pub fn new(coasts: &[Coast], rocky: impl Fn(usize) -> bool + Sync, room: impl Fn(Point, Point, f32) -> f32 + Sync) -> Self {
        let normals = CoastNormals::new(coasts);
        let mut walls = Self { replaced: vec![false; coasts.len()], ..Self::default() };
        // Runs are independent: their stations are fitted on worker threads.
        let indices: Vec<usize> = (0..coasts.len()).collect();
        let is_rocky = super::parallel_map(&indices, |&i| rocky(i));
        let runs = rocky_runs(coasts, &|i| is_rocky[i]);
        let fitted = super::parallel_map(&runs, |run| {
            let mut stations = run_stations(coasts, run, &normals);
            fit_depths(&mut stations, &room);
            stations
        });
        for (run, stations) in runs.into_iter().zip(fitted) {
            walls.add_run(&stations);
            let feet: Vec<Foot> = stations.iter().map(|s| Foot { position: s.row(0), seaward: [-s.inland[0], -s.inland[1]] }).collect();
            walls.props.extend(stacks::place(&feet));
            for i in run {
                walls.replaced[i] = true;
            }
        }
        let boxes = walls.strips.iter().map(|q| bounding_box(q));
        walls.strip_index = SegmentIndex::new(boxes, 0.5);
        walls
    }

    /// Contours, strips and row fractions for region fingerprints.
    pub fn fingerprint(&self, sink: &mut super::fingerprint::Sink) {
        use super::fingerprint::{bits, Tag};
        for &segment in &self.contours {
            sink.segment(Tag::WallContour, segment, 0., &bits(&segment));
        }
        for strip in &self.strips {
            let [low, high] = bounding_box(strip);
            sink.item(Tag::WallStrip, low, high, &bits(strip));
        }
        // The map has no order of its own: by position.
        let mut fractions: Vec<([u32; 2], f32)> = self.fractions.iter().map(|(k, v)| (*k, *v)).collect();
        fractions.sort_unstable_by_key(|(k, _)| *k);
        for (key, row) in fractions {
            let p = key.map(f32::from_bits);
            sink.item(Tag::WallFraction, p, p, &[key[0], key[1], row.to_bits()]);
        }
    }

    /// The profile row of a wall vertex at exactly this position.
    pub fn fraction_at(&self, p: Point) -> Option<f32> {
        self.fractions.get(&p.map(f32::to_bits)).copied()
    }

    /// Inside a wall strip between the foot and the brow.
    pub fn contains(&self, p: Point) -> bool {
        self.strip_index.candidates(p).iter().any(|&i| inside(&self.strips[i as usize], p))
    }

    fn add_run(&mut self, stations: &[Station]) {
        for s in stations {
            for k in 0..ROWS.len() {
                self.fractions.insert(s.row(k).map(f32::to_bits), ROWS[k]);
            }
            for k in 0..BROW {
                self.push(s.row(k), s.row(k + 1));
            }
        }
        for pair in stations.windows(2) {
            for k in 0..ROWS.len() {
                self.push(pair[0].row(k), pair[1].row(k));
            }
            self.strips.push([pair[0].row(0), pair[1].row(0), pair[1].row(BROW), pair[0].row(BROW)]);
        }
    }

    fn push(&mut self, a: Point, b: Point) {
        if (a[0] - b[0]).hypot(a[1] - b[1]) > 1e-4 {
            self.contours.push([a, b]);
        }
    }
}

/// Maximal runs of consecutive rocky coast segments that join end to start.
fn rocky_runs(coasts: &[Coast], rocky: &impl Fn(usize) -> bool) -> Vec<Vec<usize>> {
    let mut runs: Vec<Vec<usize>> = vec![];
    for i in 0..coasts.len() {
        if !rocky(i) {
            continue;
        }
        let joins = i > 0 && rocky(i - 1) && coasts[i - 1].b == coasts[i].a;
        match runs.last_mut() {
            Some(run) if joins => run.push(i),
            _ => runs.push(vec![i]),
        }
    }
    runs
}

/// Stations along a run, the run's end included; fractures inside segments.
fn run_stations(coasts: &[Coast], run: &[usize], normals: &CoastNormals) -> Vec<Station> {
    let mut stations = vec![];
    for &i in run {
        let c = &coasts[i];
        let length = (c.b[0] - c.a[0]).hypot(c.b[1] - c.a[1]);
        let count = (length / STATION_SPACING).ceil().max(1.) as usize;
        let fractures = Fractures::new(c, length, count);
        let (sea_a, sea_b) = (normals.at(c.a), normals.at(c.b));
        for j in 0..count {
            let t = j as f32 / count as f32;
            stations.push(station(lerp(c.a, c.b, t), lerp(sea_a, sea_b, t), fractures.offsets(j)));
        }
    }
    let last = &coasts[*run.last().unwrap()];
    stations.push(station(last.b, normals.at(last.b), [0.; ROWS.len()]));
    stations
}

fn station(coast: Point, sea: Point, offsets: [f32; ROWS.len()]) -> Station {
    let length = sea[0].hypot(sea[1]).max(1e-6);
    let sea = [sea[0] / length, sea[1] / length];
    let depth = std::array::from_fn(|k| CoastShape::contour_offset(coast, sea, ROWS[k]) + offsets[k]);
    Station { coast, inland: [-sea[0], -sea[1]], depth }
}

/// Column set-backs and V-notches of one coast segment, from a hash of its
/// end points: deterministic and independent of the rest of the coast.
struct Fractures {
    /// Inland offset of every row at each station of the segment.
    offsets: Vec<[f32; ROWS.len()]>,
}

impl Fractures {
    fn new(coast: &Coast, length: f32, count: usize) -> Self {
        let mut seed = hash_u32(coast.a[0].to_bits() ^ coast.a[1].to_bits().rotate_left(11) ^ coast.b[0].to_bits().rotate_left(22));
        let mut random = move || {
            seed = hash_u32(seed.wrapping_add(0x9e37_79b9));
            seed as f32 / u32::MAX as f32
        };
        let spacing = length / count as f32;
        let mut offsets = vec![[0.; ROWS.len()]; count];
        let mut notches = vec![];
        let mut start = 0.;
        while start < length {
            let width = COLUMN_WIDTH[0] + (COLUMN_WIDTH[1] - COLUMN_WIDTH[0]) * random();
            let column = Self::column(COLUMN_SHIFT * random());
            // The first station is shared with the previous segment and stays in place.
            for (j, offset) in offsets.iter_mut().enumerate().skip(1) {
                if (start..start + width).contains(&(j as f32 * spacing)) {
                    *offset = column;
                }
            }
            if random() < NOTCH_CHANCE {
                notches.push((start + width, NOTCH_DEPTH[0] + (NOTCH_DEPTH[1] - NOTCH_DEPTH[0]) * random()));
            }
            start += width;
        }
        for (at, depth) in notches {
            Self::notch(&mut offsets, (at / spacing).round() as usize, depth);
        }
        Self::lean(&mut offsets, &mut random);
        Self { offsets }
    }
    /// Broad facets: the lower and the upper half of the wall each lean back
    /// by an amount that rises from zero at the segment ends to a peak inside
    /// it, so every segment shows a few planar faces at different slopes
    /// instead of one vertical sheet. Offsets stay non-decreasing from foot
    /// to brow, so rows never cross.
    fn lean(offsets: &mut [[f32; ROWS.len()]], random: &mut impl FnMut() -> f32) {
        const LOWER: [f32; 2] = [0.03, 0.15];
        const UPPER: [f32; 2] = [0.03, 0.20];
        let count = offsets.len() as f32;
        for (amount, half) in [(LOWER, 0), (UPPER, 1)] {
            let peak_at = 0.3 + 0.4 * random();
            let peak = amount[0] + (amount[1] - amount[0]) * random();
            for (j, offset) in offsets.iter_mut().enumerate() {
                let t = j as f32 / count;
                let tent = if t < peak_at { t / peak_at } else { (1. - t) / (1. - peak_at) };
                for (k, value) in offset.iter_mut().enumerate() {
                    let share = if half == 0 { (ROWS[k] / 0.33).min(1.) } else { ((ROWS[k] - 0.33) / 0.33).max(0.) };
                    *value += peak * tent * share;
                }
            }
        }
    }
    /// A V-notch at one station between two columns, deepest at the brow.
    fn notch(offsets: &mut [[f32; ROWS.len()]], apex: usize, depth: f32) {
        if apex == 0 || apex >= offsets.len() {
            return;
        }
        for k in 0..ROWS.len() {
            offsets[apex][k] += depth * NOTCH_SHARE[k];
        }
    }
    fn column(shift: f32) -> [f32; ROWS.len()] {
        std::array::from_fn(|k| if k == 0 { 0. } else { shift * (0.8 + 0.2 * NOTCH_SHARE[k]) })
    }
    fn offsets(&self, j: usize) -> [f32; ROWS.len()] {
        self.offsets[j]
    }
}

/// Compress the rows of each station so that they neither cross the rows of
/// its neighbours nor pass the middle of a narrow peninsula. Run ends stay
/// in place: they are shared with the contours of the neighbouring beach.
fn fit_depths(stations: &mut [Station], room: &impl Fn(Point, Point, f32) -> f32) {
    let count = stations.len();
    let mut limits: Vec<f32> = (0..count)
        .map(|j| {
            let s = &stations[j];
            let mut limit = room(s.coast, s.inland, s.depth[BROW]);
            for other in [j.wrapping_sub(1), j + 1] {
                if other < count {
                    limit = limit.min(CROSSING_MARGIN * crossing_depth(s, &stations[other]));
                }
            }
            limit
        })
        .collect();
    smooth_limits(stations, &mut limits);
    for j in 1..count.saturating_sub(1) {
        let scale = (limits[j] / stations[j].depth[BROW]).min(1.);
        for depth in &mut stations[j].depth {
            *depth *= scale;
        }
    }
}

/// Neighbouring brows differ in depth by at most `LIMIT_SLOPE` per unit along
/// the coast, so a compressed headland narrows gradually, not station by station.
fn smooth_limits(stations: &[Station], limits: &mut [f32]) {
    let gap = |j: usize| {
        let (a, b) = (stations[j].coast, stations[j + 1].coast);
        (b[0] - a[0]).hypot(b[1] - a[1])
    };
    for j in 1..limits.len() {
        limits[j] = limits[j].min(limits[j - 1] + LIMIT_SLOPE * gap(j - 1));
    }
    for j in (0..limits.len().saturating_sub(1)).rev() {
        limits[j] = limits[j].min(limits[j + 1] + LIMIT_SLOPE * gap(j));
    }
}

/// Depth at which the inland rays of two stations meet; infinite if they part.
fn crossing_depth(a: &Station, b: &Station) -> f32 {
    let chord = [b.coast[0] - a.coast[0], b.coast[1] - a.coast[1]];
    let length = chord[0].hypot(chord[1]).max(1e-6);
    let closing = -((b.inland[0] - a.inland[0]) * chord[0] + (b.inland[1] - a.inland[1]) * chord[1]) / length;
    if closing <= 1e-6 {
        return f32::INFINITY;
    }
    length / closing
}

fn lerp(a: Point, b: Point, t: f32) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn bounding_box(q: &[Point; 4]) -> [Point; 2] {
    let low = [q.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min), q.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min)];
    let high = [q.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max), q.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max)];
    [low, high]
}

/// Even-odd point in polygon test.
fn inside(polygon: &[Point; 4], p: Point) -> bool {
    let mut inside = false;
    for i in 0..4 {
        let (a, b) = (polygon[i], polygon[(i + 1) % 4]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}
