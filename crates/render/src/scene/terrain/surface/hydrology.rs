use super::super::{corner_lattice, lattice_to_world, LatticeKey};
use crate::{
    scene::{
        fog::fog_at,
        layout::{tile_to_world, EDGE_NEIGHBORS},
    },
    world::state::RenderWorld,
};
use kernel::ids::TileId;
use super::{index::SegmentIndex, reaches::{self, Key, Reach}};
use std::collections::{BTreeMap, BTreeSet};
pub type Point = [f32; 2];
#[derive(Debug, Clone)]
pub struct Coast {
    pub a: Point,
    pub b: Point,
    pub sea: Point,
}
#[derive(Debug, Clone)]
pub struct River {
    pub a: Point,
    pub b: Point,
    /// Half width of the channel at `a` and at `b` (`reaches`: it follows the flow).
    pub width: f32,
    pub width_b: f32,
    pub ya: f32,
    pub yb: f32,
    pub start: f32,
    pub end: f32,
    /// Weights of the river mouth and of the spring at `a` and `b` (`reaches::Attrs`).
    pub mouth: [f32; 2],
    pub spring: [f32; 2],
}
impl River {
    /// Half width of the channel at parameter `t` of the segment.
    pub fn width_at(&self, t: f32) -> f32 {
        self.width + (self.width_b - self.width) * t
    }
    pub fn mouth_at(&self, t: f32) -> f32 {
        self.mouth[0] + (self.mouth[1] - self.mouth[0]) * t
    }
    pub fn spring_at(&self, t: f32) -> f32 {
        self.spring[0] + (self.spring[1] - self.spring[0]) * t
    }
    /// Fall of the water level per world unit of the segment.
    pub fn fall(&self) -> f32 {
        (self.ya - self.yb).max(0.) / (self.b[0] - self.a[0]).hypot(self.b[1] - self.a[1]).max(1e-6)
    }
    /// How much of the widest river's size the channel has at `t`, 0 to 1.
    pub fn size_at(&self, t: f32) -> f32 {
        (self.width_at(t) / super::reaches::WIDTH_MAX).clamp(0., 1.)
    }
    /// Scale of the bank profile at `t`: noise along the bank, smaller banks for a brook.
    pub fn bank_scale(&self, t: f32) -> f32 {
        let anchor = [self.a[0] * (1. - t) + self.b[0] * t, self.a[1] * (1. - t) + self.b[1] * t];
        super::transitions::RiverBankProfile::width_scale(anchor) * (0.55 + 0.45 * self.size_at(t))
    }
    /// Depth of the bed below the water at the axis: a brook is shallow, the mouth shoals
    /// to the depth of the sea floor at the coast.
    pub fn depth_at(&self, t: f32) -> f32 {
        0.16 * (self.width_at(t) / 0.205).clamp(0.3, 1.3) * (1. - 0.85 * self.mouth_at(t)) * (1. - 0.6 * self.spring_at(t))
    }
    pub fn flow_at(&self, p: Point, t: f32) -> [f32; 2] {
        let dx = self.b[0] - self.a[0];
        let dz = self.b[1] - self.a[1];
        let length = dx.hypot(dz).max(1e-6);
        let across = ((p[1] - self.a[1]) * dx - (p[0] - self.a[0]) * dz)
            / (length * self.width_at(t).max(1e-6));
        [self.start * (1. - t) + self.end * t, across.clamp(-1., 1.)]
    }
}
/// The nearest river segment of a point: its index, the distance to its axis
/// and the parameter of the closest point.
#[derive(Debug, Clone, Copy)]
pub struct RiverHit {
    pub index: usize,
    pub distance: f32,
    pub t: f32,
}
#[derive(Default)]
pub struct Hydrology {
    pub coasts: Vec<Coast>,
    pub rivers: Vec<River>,
    coast_index: SegmentIndex,
    river_index: SegmentIndex,
    /// How many river segment ends meet at each end point: two or more
    /// where another path continues, one at a source or mouth.
    river_ends: std::collections::HashMap<[u32; 2], u32>,
    /// How far the channel of each segment runs on past its end (`mouth_extension`).
    mouth_runs: Vec<f32>,
}
/// Queries farther than this from every coast fall back to the cell's
/// wetness: the coast profile (`COAST_EXTENT`) and the sea floor (saturated
/// at 2.66 offshore) end within it.
const COAST_QUERY: f32 = super::banks::COAST_EXTENT;
/// No river shapes ground or material beyond this distance from its axis.
const RIVER_QUERY: f32 = 2.5;
pub fn closest(p: Point, a: Point, b: Point) -> (f32, Point, f32) {
    let dx = b[0] - a[0];
    let dz = b[1] - a[1];
    let t =
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / (dx * dx + dz * dz).max(1e-12)).clamp(0., 1.);
    let q = [a[0] + t * dx, a[1] + t * dz];
    ((p[0] - q[0]).hypot(p[1] - q[1]), q, t)
}
/// Continuous, deterministic variation in world coordinates; no tile-shaped patches.
pub(super) fn noise(x: f32, z: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let hash = |a: i32, b: i32| {
        let mut v =
            (a as u32).wrapping_mul(0x9e3779b9) ^ (b as u32).wrapping_mul(0x85ebca6b) ^ seed;
        v = (v ^ (v >> 16)).wrapping_mul(0x7feb352d);
        v = (v ^ (v >> 15)).wrapping_mul(0x846ca68b);
        (v ^ (v >> 16)) as f32 / u32::MAX as f32
    };
    let fade = |v: f32| v * v * v * (v * (v * 6. - 15.) + 10.);
    let (u, v) = (fade(x - x.floor()), fade(z - z.floor()));
    let a = hash(ix, iz) * (1. - u) + hash(ix + 1, iz) * u;
    let b = hash(ix, iz + 1) * (1. - u) + hash(ix + 1, iz + 1) * u;
    a * (1. - v) + b * v
}

/// Keep the logical edge corridor, but break its repeating rounded-hex rhythm.
/// Open endpoints remain fixed where the known coast meets the fog boundary.
/// The logical coast edge nearest to `p`; on a tie, the first one.
fn nearest_source<'a>(source: &'a [Coast], index: &SegmentIndex, p: Point) -> &'a Coast {
    let nearest = index
        .nearest(p, f32::INFINITY, |i| closest(p, source[i as usize].a, source[i as usize].b).0)
        .expect("a coast chain has source edges");
    &source[nearest.index as usize]
}

fn natural_coast(path: Vec<Point>, source: &[Coast], index: &SegmentIndex) -> Vec<Point> {
    // One corner cut preserves long, angular geological panels. Repeated
    // subdivision rounded every headland into a tube before height sampling.
    let closed = path.first() == path.last();
    let mut points = Vec::with_capacity(path.len() * 2);
    if !closed { points.push(path[0]); }
    for edge in path.windows(2) {
        for t in [0.18, 0.82] {
            points.push([edge[0][0] * (1. - t) + edge[1][0] * t,
                         edge[0][1] * (1. - t) + edge[1][1] * t]);
        }
    }
    if closed { points.push(points[0]); } else { points.push(*path.last().unwrap()); }
    let count = points.len() - usize::from(closed);
    let mut result = Vec::with_capacity(points.len());
    for i in 0..count {
        let p = points[i];
        let old = nearest_source(source, index, p);
        let a = points[if i > 0 {
            i - 1
        } else if closed {
            count - 1
        } else {
            0
        }];
        let b = points[if i + 1 < count {
            i + 1
        } else if closed {
            0
        } else {
            i
        }];
        let len = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-6);
        let mut normal = [-(b[1] - a[1]) / len, (b[0] - a[0]) / len];
        if normal[0] * old.sea[0] + normal[1] * old.sea[1] < 0. {
            normal = normal.map(|v| -v);
        }
        let broad = noise(p[0] * 0.43, p[1] * 0.43, 73) * 2. - 1.;
        let ledge = noise(p[0] * 1.13 + broad * 0.7, p[1] * 1.13, 219) * 2. - 1.;
        let grain = noise(p[0] * 3.1, p[1] * 3.1, 419) * 2. - 1.;
        let taper = if closed {
            1.
        } else {
            let first = points[0];
            let last = points[count - 1];
            ((p[0] - first[0])
                .hypot(p[1] - first[1])
                .min((p[0] - last[0]).hypot(p[1] - last[1]))
                / 0.8)
                .min(1.)
        };
        let shift = (broad * 0.33 + ledge * 0.12 + grain * 0.035) * taper;
        let mut q = [p[0] + normal[0] * shift, p[1] + normal[1] * shift];
        let (d, foot, _) = closest(q, old.a, old.b);
        if d > 0.42 {
            q = [
                foot[0] + (q[0] - foot[0]) * 0.42 / d,
                foot[1] + (q[1] - foot[1]) * 0.42 / d,
            ];
        }
        result.push(q);
    }
    if closed {
        result.push(result[0]);
    }
    result
}
fn nodes(w: &RenderWorld, a: u32, b: u32) -> Option<[LatticeKey; 2]> {
    if a as usize >= w.terrain.len() || b as usize >= w.terrain.len() {
        return None;
    }
    let (q, r) = ((a % w.width) as i32, (a / w.width) as i32);
    let d = EDGE_NEIGHBORS
        .iter()
        .position(|&(dq, dr)| q + dq == (b % w.width) as i32 && r + dr == (b / w.width) as i32)?;
    Some([corner_lattice(q, r, d), corner_lattice(q, r, (d + 1) % 6)])
}
fn chains(edges: &[[LatticeKey; 2]]) -> Vec<Vec<Point>> {
    let mut graph: BTreeMap<LatticeKey, Vec<LatticeKey>> = BTreeMap::new();
    let mut remaining = BTreeSet::new();
    for &[a, b] in edges {
        if remaining.insert((a.min(b), a.max(b))) {
            graph.entry(a).or_default().push(b);
            graph.entry(b).or_default().push(a);
        }
    }
    let mut result = vec![];
    while let Some(&(a, _)) = remaining.first() {
        let start = graph
            .keys()
            .copied()
            .find(|p| {
                graph[p].len() != 2
                    && graph[p]
                        .iter()
                        .any(|q| remaining.contains(&(p.min(q).to_owned(), p.max(q).to_owned())))
            })
            .unwrap_or(a);
        let mut p = start;
        let mut path = vec![p];
        while let Some(q) = graph[&p]
            .iter()
            .copied()
            .find(|q| remaining.contains(&(p.min(*q), p.max(*q))))
        {
            remaining.remove(&(p.min(q), p.max(q)));
            path.push(q);
            p = q;
            if graph[&p].len() != 2 || p == start {
                break;
            }
        }
        result.push(
            path.into_iter()
                .map(|p| {
                    let (x, z) = lattice_to_world(p);
                    [x, z]
                })
                .collect(),
        );
    }
    result
}
fn round(mut points: Vec<Point>) -> Vec<Point> {
    let closed = points.len() > 2 && points.first() == points.last();
    if closed {
        points.pop();
    }
    for _ in 0..3 {
        let mut out = vec![];
        if !closed {
            out.push(points[0]);
        }
        let count = if closed {
            points.len()
        } else {
            points.len() - 1
        };
        for i in 0..count {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            for t in [0.25, 0.75] {
                out.push([a[0] * (1. - t) + b[0] * t, a[1] * (1. - t) + b[1] * t]);
            }
        }
        if !closed {
            out.push(*points.last().unwrap());
        }
        points = out;
    }
    if closed {
        points.push(points[0]);
    }
    points
}
/// Steepest fall of a river's water level per world unit of its length (a rapid).
const FALL_MAX: f32 = 0.10;

/// Carve downstream through local terrain instead of lifting upstream water
/// above a low bank. Sea-level lowlands may have a flat final reach. A brook sits
/// shallower in the ground than a river, and keeps the dry shoulders of its narrower banks.
fn river_levels(path: &[Point], widths: &[f32], height: &impl Fn(f32, f32) -> f32, mouth: bool) -> Vec<f32> {
    let mut levels = Vec::with_capacity(path.len());
    for (i, &p) in path.iter().enumerate() {
        let a = path[i.saturating_sub(1)];
        let b = path[(i + 1).min(path.len() - 1)];
        let length = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-6);
        let normal = [-(b[1] - a[1]) / length, (b[0] - a[0]) / length];
        let size = (widths[i] / super::reaches::WIDTH_MAX).clamp(0., 1.);
        let mut limit = height(p[0], p[1]) - (0.03 + 0.08 * size);
        // Check both edges and the dry shoulder beyond the 0.28 bank blend.
        // This is a bounded local query and reads the same known-only field.
        for distance in [widths[i] + 0.08, widths[i] + 0.30] {
            for sign in [-1., 1.] {
                let x = p[0] + normal[0] * distance * sign;
                let z = p[1] + normal[1] * distance * sign;
                limit = limit.min(height(x, z) - 0.035);
            }
        }
        if i > 0 {
            let distance = (p[0] - path[i - 1][0]).hypot(p[1] - path[i - 1][1]);
            limit = limit.min(levels[i - 1] - distance * 0.002);
        }
        levels.push(limit.max(0.08));
    }
    if mouth {
        *levels.last_mut().unwrap() = 0.08;
    }
    levels
}

/// Orient a known component toward its discovered outlet. Distances are computed
/// on the graph once per rebuild; negative distance is a shared downstream phase.
fn directed_rivers(
    paths: Vec<Vec<Point>>,
    coasts: &[Coast],
    height: &impl Fn(f32, f32) -> f32,
    open: &dyn Fn(Point) -> bool,
) -> Vec<River> {
    // A closed reach has no graph endpoints. Split it at a stable outlet and
    // the opposite point, so its two arms can both carry water toward the outlet.
    let paths: Vec<_> = paths
        .into_iter()
        .flat_map(|path| {
            if path.first() != path.last() || path.len() < 4 {
                return vec![path];
            }
            let count = path.len() - 1;
            let outlet = (0..count)
                .min_by(|&a, &b| {
                    let mouth = |p| coasts.iter().any(|c| closest(p, c.a, c.b).0 < 0.6);
                    (!mouth(path[a]))
                        .cmp(&!mouth(path[b]))
                        .then_with(|| {
                            height(path[a][0], path[a][1])
                                .total_cmp(&height(path[b][0], path[b][1]))
                        })
                        .then(path[a][0].total_cmp(&path[b][0]))
                        .then(path[a][1].total_cmp(&path[b][1]))
                })
                .unwrap();
            let ring: Vec<_> = (0..=count).map(|i| path[(outlet + i) % count]).collect();
            let mid = count / 2;
            vec![ring[..=mid].to_vec(), ring[mid..].to_vec()]
        })
        .collect();
    let key = |p: Point| p.map(f32::to_bits);
    let mut graph = BTreeMap::<Key, Vec<(Key, f32)>>::new();
    for path in &paths {
        let length: f32 = path
            .windows(2)
            .map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]))
            .sum();
        let a = key(path[0]);
        let b = key(*path.last().unwrap());
        graph.entry(a).or_default().push((b, length));
        graph.entry(b).or_default().push((a, length));
    }
    let is_mouth = |node: Key| {
        let p = node.map(f32::from_bits);
        coasts.iter().any(|c| closest(p, c.a, c.b).0 < 0.6)
    };
    let mut remaining: BTreeSet<_> = graph.keys().copied().collect();
    let mut roots = BTreeSet::new();
    while let Some(&seed) = remaining.first() {
        let mut component = vec![];
        let mut pending = vec![seed];
        remaining.remove(&seed);
        while let Some(p) = pending.pop() {
            component.push(p);
            for &(q, _) in &graph[&p] {
                if remaining.remove(&q) {
                    pending.push(q);
                }
            }
        }
        let root = component
            .into_iter()
            .min_by(|a, b| {
                (!is_mouth(*a))
                    .cmp(&!is_mouth(*b))
                    .then_with(|| {
                        let pa = a.map(f32::from_bits);
                        let pb = b.map(f32::from_bits);
                        height(pa[0], pa[1]).total_cmp(&height(pb[0], pb[1]))
                    })
                    .then(a.cmp(b))
            })
            .unwrap();
        roots.insert(root);
    }
    let mut distance = BTreeMap::<Key, f32>::new();
    let mut queue = BTreeSet::<(u32, Key)>::new();
    for &root in &roots {
        distance.insert(root, 0.);
        queue.insert((0, root));
    }
    while let Some((bits, node)) = queue.pop_first() {
        let d = f32::from_bits(bits);
        if d > distance[&node] {
            continue;
        }
        for &(next, length) in &graph[&node] {
            let candidate = d + length;
            if candidate < *distance.get(&next).unwrap_or(&f32::INFINITY) {
                distance.insert(next, candidate);
                queue.insert((candidate.to_bits(), next));
            }
        }
    }
    // A non-tree reach has a watershed inside it. Split there instead of
    // assigning a constant phase to an equal-distance junction-to-junction edge.
    let mut reaches = vec![];
    for path in paths {
        let a = key(path[0]);
        let b = key(*path.last().unwrap());
        let length: f32 = path.windows(2)
            .map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1])).sum();
        let cut = (distance[&b] + length - distance[&a]) * 0.5;
        if cut <= 0.0001 || cut >= length - 0.0001 {
            reaches.push(path);
            continue;
        }
        let mut along = 0.;
        for i in 1..path.len() {
            let segment = (path[i][0] - path[i - 1][0]).hypot(path[i][1] - path[i - 1][1]);
            if along + segment >= cut {
                let t = (cut - along) / segment.max(1e-6);
                let p = std::array::from_fn(|k| path[i - 1][k] * (1. - t) + path[i][k] * t);
                let mut first = path[..i].to_vec();
                if first.last() != Some(&p) { first.push(p); }
                let mut second = vec![p];
                second.extend(path[i..].iter().copied().filter(|q| *q != p));
                distance.insert(key(p), distance[&a] + cut);
                reaches.extend([first, second]);
                break;
            }
            along += segment;
        }
    }
    let mut oriented: Vec<Reach> = reaches
        .into_iter()
        .map(|mut path| {
            let a = key(path[0]);
            let b = key(*path.last().unwrap());
            if distance[&a]
                .total_cmp(&distance[&b])
                .then(a.cmp(&b))
                .is_lt()
            {
                path.reverse();
            }
            Reach { a: key(path[0]), b: key(*path.last().unwrap()), path }
        })
        .collect();
    // Upstream first: a junction is reached after every reach that flows into it.
    oriented.sort_by(|x, y| {
        distance[&y.a]
            .total_cmp(&distance[&x.a])
            .then(y.a.cmp(&x.a))
            .then(x.b.cmp(&y.b))
    });
    let mouths: BTreeSet<Key> = roots.iter().copied().filter(|&n| is_mouth(n)).collect();
    let context = reaches::Context { coasts, height, open, mouths: &mouths };
    let attrs = reaches::shape(&mut oriented, &context);
    let mut paths: Vec<_> = oriented
        .into_iter()
        .zip(attrs)
        .map(|(reach, attrs)| {
            let levels = river_levels(&reach.path, &attrs.width, height, mouths.contains(&reach.b));
            (reach.a, reach.b, reach.path, levels, attrs)
        })
        .collect();
    let mut node_level = BTreeMap::<Key, f32>::new();
    for (a, b, _, levels, _) in &paths {
        for (node, level) in [(*a, levels[0]), (*b, *levels.last().unwrap())] {
            let value = node_level.entry(node).or_insert(level);
            *value = value.min(level);
        }
    }
    // Incoming reaches lower a shared junction before its outgoing reach is cut.
    for (a, b, path, levels, _) in &mut paths {
        levels[0] = levels[0].min(node_level[a]);
        for i in 1..levels.len() {
            let length = (path[i][0] - path[i - 1][0]).hypot(path[i][1] - path[i - 1][1]);
            levels[i] = levels[i].min((levels[i - 1] - length * 0.002).max(0.08));
        }
        let last = *levels.last().unwrap();
        node_level
            .entry(*b)
            .and_modify(|level| *level = level.min(last));
    }
    // The fall of the water is bounded along the whole river: a tributary whose ground lies
    // high comes down to the level of the trunk it joins over a stretch, not in a step.
    // Downstream first, so every reach finds its outlet's level already lowered.
    for (a, b, path, levels, _) in paths.iter_mut().rev() {
        let last = levels.len() - 1;
        levels[last] = levels[last].min(node_level[b]);
        for i in (0..last).rev() {
            let length = (path[i + 1][0] - path[i][0]).hypot(path[i + 1][1] - path[i][1]);
            levels[i] = levels[i].min(levels[i + 1] + FALL_MAX * length);
        }
        node_level.entry(*a).and_modify(|level| *level = level.min(levels[0]));
    }
    let mut rivers = vec![];
    for (a, b, path, mut levels, attrs) in paths {
        levels[0] = node_level[&a];
        *levels.last_mut().unwrap() = node_level[&b];
        let length = reaches::length(&path);
        let mut along = 0.;
        for (i, edge) in path.windows(2).enumerate() {
            let next = along + (edge[1][0] - edge[0][0]).hypot(edge[1][1] - edge[0][1]);
            let phase = |s: f32| -distance[&a] * (1. - s / length) - distance[&b] * (s / length);
            rivers.push(River {
                a: edge[0],
                b: edge[1],
                width: attrs.width[i],
                width_b: attrs.width[i + 1],
                mouth: [attrs.mouth[i], attrs.mouth[i + 1]],
                spring: [attrs.spring[i], attrs.spring[i + 1]],
                ya: levels[i],
                yb: levels[i + 1],
                start: phase(along),
                end: phase(next),
            });
            along = next;
        }
    }
    rivers
}

/// Cuts the coast segments at the points where a river ends on them, so that the mouth
/// is a vertex of the coast line and the mesh joins river and sea exactly there.
fn split_at_mouths(mut coasts: Vec<Coast>, rivers: &[River]) -> Vec<Coast> {
    for end in rivers.iter().flat_map(|r| [r.a, r.b]) {
        let hit = coasts.iter().enumerate().find_map(|(i, c)| {
            let (d, q, t) = closest(end, c.a, c.b);
            let ends = (end[0] - c.a[0]).hypot(end[1] - c.a[1]).min((end[0] - c.b[0]).hypot(end[1] - c.b[1]));
            (d < 1e-4 && t > 0. && t < 1. && ends > 1e-4).then_some((i, q))
        });
        if let Some((i, _)) = hit {
            let c = coasts[i].clone();
            coasts[i] = Coast { a: c.a, b: end, sea: c.sea };
            coasts.insert(i + 1, Coast { a: end, b: c.b, sea: c.sea });
        }
    }
    coasts
}

impl Hydrology {
    pub fn new(w: &RenderWorld, height: impl Fn(f32, f32) -> f32) -> Self {
        let mut out = Self::default();
        let mut edges = vec![];
        let mut source = vec![];
        for cell in &w.terrain {
            if cell.terrain == 4 || fog_at(w, cell.tile) < 0.4 {
                continue;
            }
            let (q, r) = (
                (cell.tile.0 % w.width) as i32,
                (cell.tile.0 / w.width) as i32,
            );
            let (x, z) = tile_to_world(q as u32, r as u32);
            for (d, &(dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
                let (nq, nr) = (q + dq, r + dr);
                if nq < 0 || nr < 0 || nq >= w.width as i32 || nr >= w.height as i32 {
                    continue;
                }
                let id = TileId(nr as u32 * w.width + nq as u32);
                if w.terrain[id.0 as usize].terrain != 4 || fog_at(w, id) < 0.4 {
                    continue;
                }
                let keys = [corner_lattice(q, r, d), corner_lattice(q, r, (d + 1) % 6)];
                edges.push(keys);
                let (a, b) = (lattice_to_world(keys[0]), lattice_to_world(keys[1]));
                let (wx, wz) = tile_to_world(nq as u32, nr as u32);
                source.push(Coast {
                    a: [a.0, a.1],
                    b: [b.0, b.1],
                    sea: [(wx - x) / 1.7320508, (wz - z) / 1.7320508],
                });
            }
        }
        let index = SegmentIndex::new(source.iter().map(|c| [c.a, c.b]), 1.0);
        for path in chains(&edges) {
            for edge in natural_coast(path, &source, &index).windows(2) {
                let p = [
                    (edge[0][0] + edge[1][0]) * 0.5,
                    (edge[0][1] + edge[1][1]) * 0.5,
                ];
                let old = nearest_source(&source, &index, p);
                let dx = edge[1][0] - edge[0][0];
                let dz = edge[1][1] - edge[0][1];
                let len = dx.hypot(dz).max(1e-6);
                let mut sea = [-dz / len, dx / len];
                if sea[0] * old.sea[0] + sea[1] * old.sea[1] < 0. {
                    sea = sea.map(|v| -v);
                }
                out.coasts.push(Coast {
                    a: edge[0],
                    b: edge[1],
                    sea,
                });
            }
        }
        let river_edges: Vec<_> = w
            .river_edges
            .iter()
            .filter(|&&(a, b)| fog_at(w, TileId(a)) >= 0.4 && fog_at(w, TileId(b)) >= 0.4)
            .filter_map(|&(a, b)| nodes(w, a, b))
            .collect();
        let paths = chains(&river_edges)
            .into_iter()
            .map(round)
            .filter(|p| p.len() >= 2)
            .collect();
        // A node beside a tile the observer does not know: the river goes on out of sight.
        let size = (w.width, w.height);
        let open = |p: Point| {
            (0..6).any(|k| {
                let angle = k as f32 * std::f32::consts::FRAC_PI_3;
                let (x, z) = (p[0] + 0.55 * angle.cos(), p[1] + 0.55 * angle.sin());
                crate::scene::picking::world_to_tile(x, z, size).map_or(true, |id| fog_at(w, id) < 0.4)
            })
        };
        let rivers = directed_rivers(paths, &out.coasts, &height, &open);
        let coasts = split_at_mouths(out.coasts, &rivers);
        Self::from_parts(coasts, rivers)
    }
    pub fn from_parts(coasts: Vec<Coast>, rivers: Vec<River>) -> Self {
        let coast_index = SegmentIndex::new(coasts.iter().map(|c| [c.a, c.b]), 1.0);
        let river_index = SegmentIndex::new(rivers.iter().map(|r| [r.a, r.b]), 1.0);
        let mut river_ends = std::collections::HashMap::new();
        for r in &rivers {
            for p in [r.a, r.b] {
                *river_ends.entry(p.map(f32::to_bits)).or_insert(0) += 1;
            }
        }
        let mut hydrology = Self { coasts, rivers, coast_index, river_index, river_ends, mouth_runs: vec![] };
        hydrology.mouth_runs = (0..hydrology.rivers.len()).map(|i| hydrology.mouth_run(i)).collect();
        hydrology
    }
    /// Coast and river segments for region fingerprints. A river segment carries its
    /// neighbours two places either way: its bank frames and channel read them.
    pub fn fingerprint(&self, sink: &mut super::fingerprint::Sink) {
        use super::fingerprint::{bits, Tag};
        for c in &self.coasts {
            sink.segment(Tag::Coast, [c.a, c.b], 0., &bits(&[c.a, c.b, c.sea]));
        }
        for (i, r) in self.rivers.iter().enumerate() {
            let mut words = Vec::with_capacity(5 * 10 + 2);
            for j in i.saturating_sub(2)..(i + 3).min(self.rivers.len()) {
                let n = &self.rivers[j];
                words.push((j as i64 - i as i64) as u32);
                words.extend(bits(&[n.a, n.b]));
                words.extend([n.width, n.ya, n.yb, n.start, n.end].map(f32::to_bits));
            }
            for end in [r.a, r.b] {
                words.push(self.river_ends.get(&end.map(f32::to_bits)).copied().unwrap_or(0));
            }
            sink.segment(Tag::River, [r.a, r.b], 0., &words);
        }
    }
    /// Signed distance to the nearest coast (negative at sea) and its closest point.
    pub fn coast(&self, p: Point) -> Option<(f32, Point)> {
        let nearest = self.coast_index.nearest(p, COAST_QUERY, |i| {
            let c = &self.coasts[i as usize];
            closest(p, c.a, c.b).0
        })?;
        let c = &self.coasts[nearest.index as usize];
        let (d, q, _) = closest(p, c.a, c.b);
        let side = (p[0] - q[0]) * c.sea[0] + (p[1] - q[1]) * c.sea[1];
        Some((if side > 0. { -d } else { d }, q))
    }
    pub fn river(&self, p: Point) -> Option<(&River, f32, f32)> {
        self.river_at(self.nearest_river(p))
    }
    /// The nearest river segment within the query radius: (index, distance, parameter).
    pub fn nearest_river(&self, p: Point) -> Option<RiverHit> {
        let nearest = self.river_index.nearest(p, RIVER_QUERY, |i| {
            let r = &self.rivers[i as usize];
            closest(p, r.a, r.b).0
        })?;
        let r = &self.rivers[nearest.index as usize];
        let (d, _, t) = closest(p, r.a, r.b);
        Some(RiverHit { index: nearest.index as usize, distance: d, t })
    }
    /// Every river segment whose bank profile can reach `p`, with distance and parameter.
    /// Banks of neighbouring rivers (a tributary and its trunk) overlap near a junction, and
    /// the ground there must follow all of them, not jump where the nearest one changes.
    pub fn rivers_around(&self, p: Point, reach: f32) -> impl Iterator<Item = (&River, f32, f32)> + '_ {
        let low = [p[0] - reach, p[1] - reach];
        let high = [p[0] + reach, p[1] + reach];
        self.river_index.candidates_between(low, high).into_iter().filter_map(move |i| {
            let r = &self.rivers[i as usize];
            let (d, _, t) = closest(p, r.a, r.b);
            (d <= reach).then_some((r, d, t))
        })
    }
    pub fn river_at(&self, hit: Option<RiverHit>) -> Option<(&River, f32, f32)> {
        hit.map(|h| (&self.rivers[h.index], h.distance, h.t))
    }
    /// Joint normals and half widths at both ends of river segment `i`, shared
    /// with contiguous segments so their banks join exactly.
    fn bank_frame(&self, i: usize) -> ([Point; 2], [f32; 2]) {
        let r = &self.rivers[i];
        let normal = |a: Point, b: Point| {
            let dx = b[0] - a[0];
            let dz = b[1] - a[1];
            let l = dx.hypot(dz).max(1e-6);
            [-dz / l, dx / l]
        };
        let joined_before = i > 0 && self.rivers[i - 1].b == r.a;
        let joined_after = i + 1 < self.rivers.len() && self.rivers[i + 1].a == r.b;
        let na = if joined_before { normal(self.rivers[i - 1].a, r.b) } else { normal(r.a, r.b) };
        let nb = if joined_after { normal(r.a, self.rivers[i + 1].b) } else { normal(r.a, r.b) };
        ([na, nb], [r.width, r.width_b])
    }
    /// The nearest river, if `p` is in its channel: inside the quad between
    /// the bank contours of a segment, the very lines the mesh is constrained
    /// to, so water faces never straddle a bank. At a junction, where a path
    /// ends on another path, no bank contour closes the wedge between them,
    /// and the distance to the axis decides. A source or a mouth ends flat.
    pub fn channel(&self, p: Point) -> Option<(&River, f32, f32)> {
        self.channel_at(p, self.nearest_river(p))
    }
    /// `channel` with the nearest river segment of `p` already found.
    pub fn channel_at(&self, p: Point, hit: Option<RiverHit>) -> Option<(&River, f32, f32)> {
        let RiverHit { index: i, distance: d, t } = hit?;
        let r = &self.rivers[i];
        let joined_before = i > 0 && self.rivers[i - 1].b == r.a;
        let joined_after = i + 1 < self.rivers.len() && self.rivers[i + 1].a == r.b;
        let junction = |end: Point| self.river_ends.get(&end.map(f32::to_bits)).is_some_and(|&n| n >= 2);
        let open_end = (t == 0. && !joined_before && junction(r.a)) || (t == 1. && !joined_after && junction(r.b));
        if open_end && d < r.width_at(t) {
            return Some((r, d, t));
        }
        // The segments around `p`, not only the neighbours of the nearest one: where a river
        // bends tighter than its width, the nearest segment can be on the other side of a bend.
        let around = self.river_index.candidates_between([p[0] - 0.4, p[1] - 0.4], [p[0] + 0.4, p[1] + 0.4]);
        let inside = around.into_iter().map(|j| j as usize).chain(i.saturating_sub(1)..(i + 2).min(self.rivers.len())).any(|j| {
            let r = &self.rivers[j];
            let ([na, nb], [wa, wb]) = self.bank_frame(j);
            let (ext, dir) = (self.mouth_extension(j), unit_of(r.a, r.b));
            let b = [r.b[0] + dir[0] * ext, r.b[1] + dir[1] * ext];
            let quad = [
                [r.a[0] - na[0] * wa, r.a[1] - na[1] * wa],
                [b[0] - nb[0] * wb, b[1] - nb[1] * wb],
                [b[0] + nb[0] * wb, b[1] + nb[1] * wb],
                [r.a[0] + na[0] * wa, r.a[1] + na[1] * wa],
            ];
            inside_polygon(&quad, p)
        });
        inside.then_some((r, d, t))
    }
    /// River axes and bank contours.
    pub fn river_constraints(&self) -> Vec<[Point; 2]> {
        let mut result = vec![];
        for (i, r) in self.rivers.iter().enumerate() {
            let ([na, nb], [wa, wb]) = self.bank_frame(i);
            let scale_a = r.bank_scale(0.);
            let scale_b = r.bank_scale(1.);
            let ext = self.mouth_extension(i);
            let dir = unit_of(r.a, r.b);
            for sign in [-1., 1.] {
                for shoulder in super::transitions::RiverBankProfile::CONTOURS {
                    let sa = shoulder * scale_a;
                    let sb = shoulder * scale_b;
                    result.push([
                        [r.a[0] + na[0] * (wa + sa) * sign, r.a[1] + na[1] * (wa + sa) * sign],
                        [
                            r.b[0] + nb[0] * (wb + sb) * sign + dir[0] * ext,
                            r.b[1] + nb[1] * (wb + sb) * sign + dir[1] * ext,
                        ],
                    ]);
                }
            }
            result.push([r.a, [r.b[0] + dir[0] * ext, r.b[1] + dir[1] * ext]]);
            // A source closes the channel with a cap across the bed, so its water ends on a
            // contour, not inside a triangle. A mouth has none: the channel runs on to the
            // coast, where the sea's water begins.
            let joined_before = i > 0 && self.rivers[i - 1].b == r.a;
            let joined_after = i + 1 < self.rivers.len() && self.rivers[i + 1].a == r.b;
            let alone = |p: Point| self.river_ends.get(&p.map(f32::to_bits)).is_some_and(|&n| n < 2);
            for (end, n, w, open) in [(r.a, na, wa, !joined_before), (r.b, nb, wb, !joined_after && ext == 0.)] {
                if open && alone(end) {
                    result.push([[end[0] - n[0] * w, end[1] - n[1] * w], [end[0] + n[0] * w, end[1] + n[1] * w]]);
                }
            }
        }
        result
    }
    /// How far the channel of segment `i` runs on past its end.
    fn mouth_extension(&self, i: usize) -> f32 {
        self.mouth_runs.get(i).copied().unwrap_or(0.)
    }
    /// A river that ends at the sea flows out over the coast line, which cuts it off: the channel
    /// reaches as far past the end as both its corners stay on the sea's edge, up to `MOUTH_RUN`,
    /// so that beside a mouth, where the coast may curve back, it never floods the land.
    fn mouth_run(&self, i: usize) -> f32 {
        let r = &self.rivers[i];
        let joined_after = i + 1 < self.rivers.len() && self.rivers[i + 1].a == r.b;
        if r.mouth[1] <= 0.99 || joined_after {
            return 0.;
        }
        let ([_, n], [_, w]) = self.bank_frame(i);
        let dir = unit_of(r.a, r.b);
        let dry = |p: Point| self.coast(p).is_some_and(|(shore, _)| shore > 0.08);
        [MOUTH_RUN, 0.3, 0.15]
            .into_iter()
            .find(|&run| {
                let end = [r.b[0] + dir[0] * run, r.b[1] + dir[1] * run];
                ![-1., 1.].iter().any(|&side| dry([end[0] + n[0] * w * side, end[1] + n[1] * w * side]))
            })
            .unwrap_or(0.)
    }
}

/// How far a mouth's channel runs past the point where the river meets the coast.
const MOUTH_RUN: f32 = 0.45;

fn unit_of(a: Point, b: Point) -> [f32; 2] {
    let l = (b[0] - a[0]).hypot(b[1] - a[1]).max(1e-9);
    [(b[0] - a[0]) / l, (b[1] - a[1]) / l]
}

/// Even-odd point in polygon test.
fn inside_polygon(polygon: &[Point], p: Point) -> bool {
    let mut inside = false;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equal_distance_cycle_reach_splits_into_two_moving_arms() {
        let root = [0., 0.];
        let a = [-1., 1.];
        let b = [1., 1.];
        let paths = vec![vec![a, root], vec![b, root], vec![a, [0., 1.], b]];
        let coast = [Coast { a: [-0.1, 0.], b: [0.1, 0.], sea: [0., -1.] }];
        let streams = directed_rivers(paths.clone(), &coast, &|_, _| 0.5, &|_| false);
        assert_eq!(streams.len(), 4);
        for stream in &streams {
            assert!(stream.end > stream.start, "every arm must have a downstream phase gradient");
            assert!(stream.ya >= stream.yb);
        }
        let mut reversed = paths;
        reversed.reverse();
        for path in &mut reversed { path.reverse(); }
        let again = directed_rivers(reversed, &coast, &|_, _| 0.5, &|_| false);
        let signature = |r: &River| (r.a.map(f32::to_bits), r.b.map(f32::to_bits),
            [r.start, r.end, r.ya, r.yb].map(f32::to_bits));
        assert_eq!(streams.iter().map(signature).collect::<Vec<_>>(),
                   again.iter().map(signature).collect::<Vec<_>>());
    }

    #[test]
    fn river_cross_section_is_signed_and_longitudinal_phase_is_shared() {
        let river = River { a: [0., 0.], b: [2., 0.], width: 0.2, width_b: 0.2,
            ya: 0.3, yb: 0.2, start: -2., end: 0., mouth: [0.; 2], spring: [0.; 2] };
        assert_eq!(river.flow_at([1., -0.2], 0.5), [-1., -1.]);
        assert_eq!(river.flow_at([1., 0.], 0.5), [-1., 0.]);
        assert_eq!(river.flow_at([1., 0.2], 0.5), [-1., 1.]);
    }
    #[test]
    fn closed_known_reach_has_flowing_arms_instead_of_a_constant_uv() {
        let path = vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.], [0., 0.]];
        let river = directed_rivers(vec![path], &[], &|x, z| 0.4 + x * 0.01 + z * 0.02, &|_| false);
        assert_eq!(river.len(), 4);
        assert!(river.iter().all(|r| r.end > r.start));
        assert!(river.iter().filter(|r| r.b == [0., 0.]).count() == 2);
    }
    #[test]
    fn known_mouth_overrides_endpoint_height_and_input_order() {
        let path = vec![[0., 0.], [1., 0.], [2., 0.]];
        let coast = vec![Coast {
            a: [2., -1.],
            b: [2., 1.],
            sea: [1., 0.],
        }];
        let height = |x: f32, _: f32| 0.4 + x * 0.1;
        let a = directed_rivers(vec![path.clone()], &coast, &height, &|_| false);
        let b = directed_rivers(vec![path.into_iter().rev().collect()], &coast, &height, &|_| false);
        assert_eq!(
            a[0].a,
            [0., 0.],
            "known outlet wins even when base terrain rises toward it"
        );
        assert_eq!(a.last().unwrap().b, [2., 0.]);
        assert_eq!(a.last().unwrap().yb, 0.08);
        for (a, b) in a.iter().zip(b) {
            assert_eq!(
                (a.a, a.b, a.start, a.end, a.ya, a.yb),
                (b.a, b.b, b.start, b.end, b.ya, b.yb)
            );
            assert!(a.end > a.start);
        }
    }
    #[test]
    fn tributaries_share_phase_and_water_level_at_the_junction() {
        let paths = vec![
            vec![[0., 2.], [1., 1.]],
            vec![[2., 2.], [1., 1.]],
            vec![[1., 1.], [1., 0.]],
        ];
        let coast = vec![Coast {
            a: [0., 0.],
            b: [2., 0.],
            sea: [0., -1.],
        }];
        let height = |_: f32, _: f32| 0.4;
        let a = directed_rivers(paths.clone(), &coast, &height, &|_| false);
        let b = directed_rivers(
            paths
                .into_iter()
                .rev()
                .map(|p| p.into_iter().rev().collect())
                .collect(),
            &coast,
            &height,
            &|_| false,
        );
        let mut junctions = vec![];
        for r in &a {
            if r.a == [1., 1.] {
                junctions.push((r.start, r.ya));
            }
            if r.b == [1., 1.] {
                junctions.push((r.end, r.yb));
            }
        }
        assert_eq!(junctions.len(), 3);
        for pair in junctions.windows(2) {
            assert!((pair[0].0 - pair[1].0).abs() < 1e-6);
            assert!((pair[0].1 - pair[1].1).abs() < 1e-6);
        }
        for (a, b) in a.iter().zip(b) {
            assert_eq!(
                (a.a, a.b, a.start, a.end, a.ya, a.yb),
                (b.a, b.b, b.start, b.end, b.ya, b.yb)
            );
            assert!(a.end > a.start && a.ya >= a.yb);
        }
    }
    #[test]
    fn river_carves_downstream_without_flooding_a_low_side_bank() {
        let path: Vec<_> = (0..17).map(|i| [i as f32 * 0.25, 0.]).collect();
        let height = |x: f32, z: f32| {
            // A downstream ridge used to raise the entire upstream profile.
            // The right bank is lower than the centre throughout the valley.
            let ridge = if x > 2. { 0.8 } else { 0.35 };
            if z > 0.15 {
                ridge - 0.12
            } else {
                ridge
            }
        };
        let widths = vec![0.205; path.len()];
        let levels = river_levels(&path, &widths, &height, true);
        assert_eq!(*levels.last().unwrap(), 0.08);
        assert!(levels.windows(2).all(|l| l[1] <= l[0]));
        for (p, level) in path.iter().zip(&levels) {
            assert!(*level <= height(p[0], 0.205 + 0.30) - 0.0349);
        }
        assert!(
            levels[0] < 0.2,
            "upstream low bank must constrain the water"
        );
        assert!(
            levels[12] < 0.2,
            "downstream ridge is carved down, not flooded"
        );
    }
    #[test]
    fn sea_level_lowland_profile_stays_flat_without_an_upstream_dam() {
        let path = vec![[0., 0.], [1., 0.], [2., 0.]];
        assert_eq!(river_levels(&path, &[0.205; 3], &|_, _| 0.10, true), vec![0.08; 3]);
    }
    #[test]
    fn irregular_coast_stays_in_corridor_and_preserves_open_endpoints() {
        let path: Vec<_> = (0..25).map(|i| [0., i as f32]).collect();
        let source: Vec<_> = path
            .windows(2)
            .map(|p| Coast {
                a: p[0],
                b: p[1],
                sea: [1., 0.],
            })
            .collect();
        let index = SegmentIndex::new(source.iter().map(|c| [c.a, c.b]), 1.0);
        let coast = natural_coast(path.clone(), &source, &index);
        assert!(coast.len() <= path.len() * 2,
            "geological panels must not turn into dense rounded shoreline strips");
        assert_eq!(coast.first(), path.first());
        assert_eq!(coast.last(), path.last());
        assert!(coast.iter().all(|p| p[0].abs() <= 0.420001));
        assert!(coast.windows(2).all(|p| p[1][1] > p[0][1]));
        let min = coast.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min);
        let max = coast.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max - min > 0.3,
            "coast must contain distinct bays and headlands"
        );
    }
}
