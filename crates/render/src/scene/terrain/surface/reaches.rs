//! The shape of the river reaches: what the logical graph of hex borders becomes as water.
//!
//! The map stores a river as borders between hexes. A reach is a path of that graph from a
//! source or junction to the next junction or the mouth, already directed downstream
//! (`hydrology::directed_rivers`). Its look is derived here from the graph and the ground
//! alone, never from a key or a seed:
//!
//! * the **width** follows the flow the reach carries: the length of river upstream of each
//!   point. A source is a thin brook, every junction widens the river, a branch is thinner
//!   than the trunk it joins, the mouth is the widest. Along a reach it only grows;
//! * the **line** is a smooth curve instead of the hex polyline: the corners are cut once
//!   (`round`), a position-hashed lateral drift makes meanders on a plain and keeps the bed
//!   straight on a slope, a tributary bends into the trunk's direction (a Y, not a T);
//! * a **mouth** is carried to the coast line, where the water of the river is the sea's;
//!   the bed shoals and the channel widens toward it. A **source** that nothing continues
//!   (not on the edge of the known world) ends as a spring: a marsh, not a cut.
//!
//! Everything is a function of the positions in the observation, so the same map gives the
//! same rivers (no clock, no random numbers).
use super::{
    biomes::smooth,
    hydrology::{closest, noise, Coast, Point},
};
use std::collections::{BTreeMap, BTreeSet};

pub type Key = [u32; 2];

/// Half width of a brook at its source, and of the widest river, world units.
pub const WIDTH_MIN: f32 = 0.045;
pub const WIDTH_MAX: f32 = 0.27;
/// River length upstream (world units) at which the width has risen by 63 % of its range.
const FLOW_SCALE: f32 = 14.;
/// Path length either side of a point over which its bend is measured.
const BEND_SPAN: f32 = 0.15;
/// A bank offset takes at most this much of the radius of a bend.
const BEND_FILL: f32 = 0.8;
/// How fast, per unit upstream, the width a bend allows relaxes.
const BEND_EASE: f32 = 0.25;
/// How much of its width a river loses at its very source.
const SPRING_THIN: f32 = 0.55;
/// Along-river distance over which the mouth's flare and shoaling, and the spring, fade.
pub const MOUTH_REACH: f32 = 0.9;
pub const SPRING_REACH: f32 = 0.7;
/// The widest the channel flares at the very mouth, as a share of the width there.
const FLARE: f32 = 0.35;
/// Lateral drift of the line, world units: on level ground and on a slope.
const MEANDER_PLAIN: f32 = 0.34;
const MEANDER_SLOPE: f32 = 0.05;
/// Distance from the ends of a reach over which the drift grows from zero.
const MEANDER_TAPER: f32 = 1.4;

/// A reach, oriented downstream, with the graph keys of its ends.
#[derive(Debug, Clone)]
pub struct Reach {
    pub a: Key,
    pub b: Key,
    pub path: Vec<Point>,
}

/// Per point of a reach.
#[derive(Debug, Clone, Default)]
pub struct Attrs {
    /// Half width of the channel.
    pub width: Vec<f32>,
    /// How much the point belongs to the river mouth: 1 at the coast, 0 beyond `MOUTH_REACH`.
    pub mouth: Vec<f32>,
    /// How much the point belongs to a spring: 1 at the source, 0 beyond `SPRING_REACH`.
    pub spring: Vec<f32>,
}

/// What the shaping reads of the world.
pub struct Context<'a> {
    pub coasts: &'a [Coast],
    pub height: &'a dyn Fn(f32, f32) -> f32,
    /// A node on the edge of the known world: the river goes on there, so it is neither a
    /// source nor a mouth.
    pub open: &'a dyn Fn(Point) -> bool,
    /// The outlets that lie on a coast.
    pub mouths: &'a BTreeSet<Key>,
}

/// Half width of a channel that carries `flow`.
pub fn width_of_flow(flow: f32) -> f32 {
    WIDTH_MIN + (WIDTH_MAX - WIDTH_MIN) * (1. - (-flow.max(0.) / FLOW_SCALE).exp())
}

pub fn length(path: &[Point]) -> f32 {
    path.windows(2).map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1])).sum()
}

fn arclengths(path: &[Point]) -> Vec<f32> {
    let mut s = vec![0.];
    for p in path.windows(2) {
        s.push(s.last().unwrap() + (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]));
    }
    s
}

/// The widest half-width each point of `path` can have: a bank offset by more than the radius of
/// the bend folds over itself. The radius is measured over `BEND_SPAN` either side of a point; the
/// cap relaxes upstream by `BEND_EASE` per unit, so a tight bend narrows its approach gently.
fn bend_caps(path: &[Point], s: &[f32]) -> Vec<f32> {
    let n = path.len();
    let mut caps = vec![f32::INFINITY; n];
    for i in 1..n.saturating_sub(1) {
        let before = (0..i).rev().find(|&j| s[i] - s[j] >= BEND_SPAN).unwrap_or(0);
        let after = (i + 1..n).find(|&k| s[k] - s[i] >= BEND_SPAN).unwrap_or(n - 1);
        let (a, b, c) = (path[before], path[i], path[after]);
        let side = |p: Point, q: Point| (p[0] - q[0]).hypot(p[1] - q[1]);
        let area2 = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
        if area2 > 1e-9 {
            caps[i] = BEND_FILL * side(a, b) * side(b, c) * side(a, c) / (2. * area2);
        }
    }
    // The last point has no bend of its own: it takes the one before.
    if n >= 2 {
        caps[n - 1] = caps[n - 2];
    }
    for i in (0..n.saturating_sub(1)).rev() {
        caps[i] = caps[i].min(caps[i + 1] + BEND_EASE * (s[i + 1] - s[i]));
    }
    caps
}

fn unit(v: [f32; 2]) -> [f32; 2] {
    let l = v[0].hypot(v[1]).max(1e-9);
    [v[0] / l, v[1] / l]
}

/// The direction of the path near its end (`start`: the upstream end, pointing downstream;
/// else the downstream end, pointing downstream too), measured over about `span` of its length.
fn direction_at_end(path: &[Point], start: bool, span: f32) -> [f32; 2] {
    let n = path.len();
    let at = |k: usize| if start { path[k] } else { path[n - 1 - k] };
    let end = at(0);
    let mut other = at(1);
    for k in 1..n {
        other = at(k);
        if (other[0] - end[0]).hypot(other[1] - end[1]) >= span {
            break;
        }
    }
    if start {
        unit([other[0] - end[0], other[1] - end[1]])
    } else {
        unit([end[0] - other[0], end[1] - other[1]])
    }
}

/// Slope of the base ground at `p`: height change per world unit.
fn slope(height: &dyn Fn(f32, f32) -> f32, p: Point) -> f32 {
    const E: f32 = 0.25;
    let dx = height(p[0] + E, p[1]) - height(p[0] - E, p[1]);
    let dz = height(p[0], p[1] + E) - height(p[0], p[1] - E);
    dx.hypot(dz) / (2. * E)
}

/// The nearest coast point at which a ray from `from` along `direction` leaves the land
/// within `reach`: the ray's first crossing of a coast segment whose sea lies ahead.
fn coast_crossing(coasts: &[Coast], from: Point, direction: [f32; 2], reach: f32) -> Option<(f32, Point)> {
    let mut best: Option<(f32, Point)> = None;
    for c in coasts {
        let e = [c.b[0] - c.a[0], c.b[1] - c.a[1]];
        let denominator = direction[0] * e[1] - direction[1] * e[0];
        if denominator.abs() < 1e-9 {
            continue;
        }
        let w = [c.a[0] - from[0], c.a[1] - from[1]];
        let t = (w[0] * e[1] - w[1] * e[0]) / denominator;
        let u = (w[0] * direction[1] - w[1] * direction[0]) / denominator;
        if !(0. ..=reach).contains(&t) || !(0. ..=1.).contains(&u) {
            continue;
        }
        if direction[0] * c.sea[0] + direction[1] * c.sea[1] <= 0.05 {
            continue;
        }
        if best.map_or(true, |(bt, _)| t < bt) {
            best = Some((t, [from[0] + direction[0] * t, from[1] + direction[1] * t]));
        }
    }
    best
}

/// Where the water of a river that ends at `node` meets the coast: the crossing of its last
/// direction with the coast, else the nearest coast point.
fn mouth_point(coasts: &[Coast], node: Point, direction: [f32; 2]) -> Point {
    let from = [node[0] - direction[0] * 0.7, node[1] - direction[1] * 0.7];
    if let Some((_, p)) = coast_crossing(coasts, from, direction, 2.0) {
        return p;
    }
    coasts
        .iter()
        .map(|c| closest(node, c.a, c.b))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(node, |(_, q, _)| q)
}

/// Moves the last points of `path` with its end: the shift of the end fades along the path.
fn drag_end(path: &mut [Point], start: bool, to: Point, span: f32) {
    let n = path.len();
    let end = if start { 0 } else { n - 1 };
    let shift = [to[0] - path[end][0], to[1] - path[end][1]];
    let mut along = 0.;
    for k in 0..n {
        let i = if start { k } else { n - 1 - k };
        if k > 0 {
            let j = if start { i - 1 } else { i + 1 };
            along += (path[i][0] - path[j][0]).hypot(path[i][1] - path[j][1]);
        }
        let weight = 1. - smooth(along / span);
        if weight <= 0. {
            break;
        }
        path[i] = [path[i][0] + shift[0] * weight, path[i][1] + shift[1] * weight];
    }
}

/// Lateral drift of the interior of a reach.
fn meander(path: &mut [Point], context: &Context) {
    if path.len() < 3 {
        return;
    }
    let s = arclengths(path);
    let total = *s.last().unwrap();
    let old: Vec<Point> = path.to_vec();
    for i in 1..path.len() - 1 {
        let p = old[i];
        let taper = smooth(s[i].min(total - s[i]) / MEANDER_TAPER);
        if taper <= 0. {
            continue;
        }
        let steep = smooth(slope(context.height, p) / 0.35);
        let coast = context.coasts.iter().map(|c| closest(p, c.a, c.b).0).fold(f32::INFINITY, f32::min);
        let room = smooth((coast - 0.3) / 0.7);
        let amplitude = (MEANDER_PLAIN * (1. - steep) + MEANDER_SLOPE * steep) * room;
        let t = unit([old[i + 1][0] - old[i - 1][0], old[i + 1][1] - old[i - 1][1]]);
        let wander = 0.8 * (noise(p[0] * 0.62, p[1] * 0.62, 3301) * 2. - 1.) + 0.35 * (noise(p[0] * 1.9, p[1] * 1.9, 3307) * 2. - 1.);
        let offset = amplitude * taper * wander;
        path[i] = [p[0] - t[1] * offset, p[1] + t[0] * offset];
    }
}

/// Bends the last stretch of a tributary so that it enters the trunk along the trunk's
/// direction: a cubic whose end tangent is the downstream direction.
fn bend_into(path: &mut Vec<Point>, downstream: [f32; 2]) {
    let n = path.len();
    if n < 4 {
        return;
    }
    let end = path[n - 1];
    let arrival = direction_at_end(path, false, 0.5);
    let cos = arrival[0] * downstream[0] + arrival[1] * downstream[1];
    if cos > 0.9 {
        return;
    }
    let s = arclengths(path);
    let reach = 1.3f32.min(s[n - 1] * 0.45);
    let first = (0..n).rev().find(|&i| s[n - 1] - s[i] >= reach).unwrap_or(0);
    let from = path[first];
    let span = (end[0] - from[0]).hypot(end[1] - from[1]);
    let tangent_from = direction_at_end(&path[..=first.max(1)], false, 0.5);
    let c1 = [from[0] + tangent_from[0] * span / 3., from[1] + tangent_from[1] * span / 3.];
    let c2 = [end[0] - downstream[0] * span / 3., end[1] - downstream[1] * span / 3.];
    let samples = (n - 1 - first).max(3);
    path.truncate(first + 1);
    for k in 1..=samples {
        let t = k as f32 / samples as f32;
        let u = 1. - t;
        let b = |i: usize| u * u * u * from[i] + 3. * u * u * t * c1[i] + 3. * u * t * t * c2[i] + t * t * t * end[i];
        path.push([b(0), b(1)]);
    }
}

/// Shapes every reach (their order is the caller's, upstream first) and gives its points a
/// width, mouth and spring weights.
pub fn shape(reaches: &mut [Reach], context: &Context) -> Vec<Attrs> {
    // The flow each node receives: reaches come upstream first.
    let mut incoming: BTreeMap<Key, usize> = BTreeMap::new();
    let mut outgoing: BTreeMap<Key, usize> = BTreeMap::new();
    for r in reaches.iter() {
        *outgoing.entry(r.a).or_default() += 1;
        *incoming.entry(r.b).or_default() += 1;
    }
    let node_point = |key: Key| key.map(f32::from_bits);
    let is_mouth = |key: Key| context.mouths.contains(&key) && !(context.open)(node_point(key)) && !outgoing.contains_key(&key);
    let is_source = |key: Key| !incoming.contains_key(&key) && outgoing.get(&key) == Some(&1) && !(context.open)(node_point(key));

    // Where each mouth lands on the coast, by the direction its main reach arrives in.
    let mut landing: BTreeMap<Key, Point> = BTreeMap::new();
    for r in reaches.iter() {
        if is_mouth(r.b) && r.path.len() >= 2 {
            let direction = direction_at_end(&r.path, false, 0.6);
            landing.entry(r.b).or_insert_with(|| mouth_point(context.coasts, node_point(r.b), direction));
        }
    }
    for r in reaches.iter_mut() {
        if let Some(&p) = landing.get(&r.b) {
            drag_end(&mut r.path, false, p, 0.9);
        }
        meander(&mut r.path, context);
    }
    // A tributary enters its trunk along the trunk's direction.
    let mut downstream: BTreeMap<Key, [f32; 2]> = BTreeMap::new();
    for r in reaches.iter() {
        if r.path.len() >= 2 && !downstream.contains_key(&r.a) {
            downstream.insert(r.a, direction_at_end(&r.path, true, 0.6));
        }
    }
    for r in reaches.iter_mut() {
        if incoming.get(&r.b).is_some_and(|&n| n >= 1) && outgoing.contains_key(&r.b) {
            if let Some(&direction) = downstream.get(&r.b) {
                bend_into(&mut r.path, direction);
            }
        }
    }

    // Flow and width.
    let mut node_flow: BTreeMap<Key, f32> = BTreeMap::new();
    let mut attrs = Vec::with_capacity(reaches.len());
    for r in reaches.iter() {
        let s = arclengths(&r.path);
        let inflow = node_flow.get(&r.a).copied().unwrap_or(0.);
        let widths: Vec<f32> = s.iter().map(|&along| width_of_flow(inflow + along)).collect();
        *node_flow.entry(r.b).or_default() += inflow + *s.last().unwrap();
        let total = *s.last().unwrap();
        let mouth_here = is_mouth(r.b);
        let spring_here = is_source(r.a);
        let mouth: Vec<f32> = s.iter().map(|&along| if mouth_here { 1. - smooth((total - along) / MOUTH_REACH) } else { 0. }).collect();
        let spring: Vec<f32> = s.iter().map(|&along| if spring_here { 1. - smooth(along / SPRING_REACH) } else { 0. }).collect();
        // The channel opens into the water.
        // and the brook thins out into its spring.
        let widths: Vec<f32> = widths.iter().zip(mouth.iter().zip(&spring)).map(|(w, (m, sp))| w * (1. + FLARE * m * m) * (1. - SPRING_THIN * sp)).collect();
        // No bend is tighter than the river is wide.
        // The reach to a mouth curls into the coast; elsewhere the meander is gentle enough.
        let widths = if mouth_here {
            let caps = bend_caps(&r.path, &s);
            widths.iter().zip(&caps).map(|(w, cap)| w.min(cap.max(0.5 * WIDTH_MIN))).collect()
        } else {
            widths
        };
        attrs.push(Attrs { width: widths, mouth, spring });
    }
    // Flow is accumulated in the order given, upstream first; a junction's width is the same
    // from every reach that meets there (it is the flow they carry together downstream).
    for (r, a) in reaches.iter().zip(attrs.iter_mut()) {
        let total = node_flow.get(&r.b).copied().unwrap_or(0.);
        let last = a.width.len() - 1;
        if !is_mouth(r.b) {
            a.width[last] = width_of_flow(total);
        }
    }
    attrs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn width_grows_with_flow_from_a_brook_to_a_river() {
        assert!((width_of_flow(0.) - WIDTH_MIN).abs() < 1e-6);
        let mut last = 0.;
        for i in 0..200 {
            let w = width_of_flow(i as f32 * 0.5);
            assert!(w >= last && w <= WIDTH_MAX);
            last = w;
        }
        assert!(last > 0.9 * WIDTH_MAX);
    }
}
