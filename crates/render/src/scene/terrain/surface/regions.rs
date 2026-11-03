//! Regions, halo and canonical seams (platform/MESHING.md §2.5, decision D1).
//!
//! A region is a block of `REGION`×`REGION` cells in map (q, r) coordinates.
//! Its boundary runs along hex edges, which are exact keys of the integer
//! lattice (`lattice.rs`). Each boundary edge is a seam, cut into points by
//! one function of the edge's canonical lattice keys alone (`SeamPlan`), so
//! both regions insert bit-identical seam vertices and a T-junction cannot
//! occur. Contours that cross a seam are split exactly at a seam point, once,
//! before any region is triangulated.
//!
//! Contributor data (coasts, rivers, walls, massifs) is derived once per
//! world observation (`Landscape`). A region reads only the field within its
//! cells, and the field at a point depends on cells at most `HALO_RINGS` away
//! (river levels and wall runs excepted: they follow a whole river or cliff
//! run), so editing a cell outside a region's halo leaves the region as it was.
use super::{
    field::Landscape,
    hydrology::{closest, Point},
    index::SegmentIndex,
    sizing, SurfaceWaterKind,
};
use crate::scene::{
    fog::fog_at,
    layout::{tile_to_world, EDGE_NEIGHBORS},
    picking::world_to_tile,
    terrain::{corner_lattice, lattice_to_world, LatticeKey},
};
use crate::world::state::RenderWorld;
use kernel::ids::TileId;
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
use std::collections::{BTreeMap, HashMap};

/// Cells per region side (decision D1: 16x16 until measured otherwise).
pub const REGION: u32 = 16;
/// Rings of cells around a region that its field depends on: coast distance
/// queries reach 4.0, massifs 2.9, the cell mix 2.35 (3 rings ≈ 4.5 world).
pub const HALO_RINGS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegionId {
    pub x: u32,
    pub y: u32,
}

/// Coarsest detail level: level L asks for edges 2^L times longer.
pub const MAX_LEVEL: u8 = 2;

/// Detail level of every region (platform/MESHING.md §2.5, LOD): a region of level L meshes
/// with the sizing field scaled by 2^L, and a seam takes the coarser level of its two
/// sides, so both cut it alike. Regions not listed are at level 0.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Levels(pub std::collections::BTreeMap<RegionId, u8>);

impl Levels {
    pub fn of(&self, id: RegionId) -> u8 {
        self.0.get(&id).copied().unwrap_or(0).min(MAX_LEVEL)
    }

    /// Sets a region's level; level 0 is not stored, so equal levels compare equal.
    pub fn set(&mut self, id: RegionId, level: u8) {
        if level == 0 {
            self.0.remove(&id);
        } else {
            self.0.insert(id, level.min(MAX_LEVEL));
        }
    }

    /// How much longer than requested the edges of a level are.
    pub fn scale(level: u8) -> f32 {
        (1u32 << level.min(MAX_LEVEL)) as f32
    }
}

type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>>;
type Edge = (LatticeKey, LatticeKey);

/// How the map is cut into regions of `size` cells.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub size: u32,
    pub width: u32,
    pub height: u32,
}

impl Layout {
    pub fn new(world: &RenderWorld, size: u32) -> Self {
        Self { size: size.max(1), width: world.width, height: world.height }
    }
    pub fn columns(&self) -> u32 {
        self.width.div_ceil(self.size)
    }
    pub fn rows(&self) -> u32 {
        self.height.div_ceil(self.size)
    }
    pub fn of_cell(&self, q: u32, r: u32) -> RegionId {
        RegionId { x: q / self.size, y: r / self.size }
    }
    /// The region of the cell under a world point, if the point is on the map.
    pub fn of_point(&self, p: Point) -> Option<RegionId> {
        let tile = world_to_tile(p[0], p[1], (self.width, self.height))?;
        Some(self.of_cell(tile.0 % self.width, tile.0 / self.width))
    }
    pub fn index(&self, id: RegionId) -> usize {
        (id.y * self.columns() + id.x) as usize
    }
    pub fn cells(&self, id: RegionId) -> impl Iterator<Item = (u32, u32)> {
        let (size, width, height) = (self.size, self.width, self.height);
        let qs = id.x * size..((id.x + 1) * size).min(width);
        let rs = id.y * size..((id.y + 1) * size).min(height);
        rs.flat_map(move |r| qs.clone().map(move |q| (q, r)))
    }
    /// How many cells of a region are known.
    pub fn known_cells(&self, world: &RenderWorld, id: RegionId) -> usize {
        self.cells(id).filter(|&(q, r)| fog_at(world, TileId(r * self.width + q)) >= 0.4).count()
    }
    /// Regions with at least one known cell, row-major. Unknown regions are not built.
    pub fn known(&self, world: &RenderWorld) -> Vec<RegionId> {
        let mut ids = vec![];
        for y in 0..self.rows() {
            for x in 0..self.columns() {
                let id = RegionId { x, y };
                let known = self.cells(id).any(|(q, r)| fog_at(world, TileId(r * self.width + q)) >= 0.4);
                if known {
                    ids.push(id);
                }
            }
        }
        ids
    }
    /// Hex edges on the boundary of a region, as canonical lattice key pairs.
    pub fn boundary(&self, id: RegionId) -> Vec<Edge> {
        let mut edges = vec![];
        for (q, r) in self.cells(id) {
            for (d, &(dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
                let (nq, nr) = (q as i32 + dq, r as i32 + dr);
                let inside = nq >= 0 && nr >= 0 && (nq as u32) < self.width && (nr as u32) < self.height;
                if inside && self.of_cell(nq as u32, nr as u32) == id {
                    continue;
                }
                let a = corner_lattice(q as i32, r as i32, d);
                let b = corner_lattice(q as i32, r as i32, (d + 1) % 6);
                edges.push((a.min(b), a.max(b)));
            }
        }
        edges
    }
    /// World bounds of a region's cells.
    pub fn bounds(&self, id: RegionId) -> [Point; 2] {
        let mut low = [f32::INFINITY; 2];
        let mut high = [f32::NEG_INFINITY; 2];
        for (q, r) in self.cells(id) {
            let (x, z) = tile_to_world(q, r);
            low = [low[0].min(x - 1.), low[1].min(z - 1.)];
            high = [high[0].max(x + 1.), high[1].max(z + 1.)];
        }
        [low, high]
    }
}

/// The point at parameter `t` of a seam edge: one function of the canonical
/// keys, so both regions compute the same bits.
fn seam_point((a, b): Edge, t: f32) -> Point {
    let (ax, az) = lattice_to_world(a);
    let (bx, bz) = lattice_to_world(b);
    // The ends are the lattice corners themselves, shared by every edge there.
    if t == 0. {
        return [ax, az];
    }
    if t == 1. {
        return [bx, bz];
    }
    [ax + (bx - ax) * t, az + (bz - az) * t]
}

/// Seams of every region boundary and contour pieces split at them. Built
/// once for all regions of a build, read by each region.
pub struct SeamPlan {
    /// Ordered points along each boundary edge, ends included.
    edges: BTreeMap<Edge, Vec<Point>>,
    /// Contour pieces by region, in the global contour order.
    pieces: HashMap<RegionId, Vec<[Point; 2]>>,
    /// Cliff-wall row of a contour point created by a seam split.
    rows: HashMap<[u32; 2], f32>,
}

/// Seam points closer than this share of the edge length `h` to a contour crossing are dropped.
const CROSSING_GAP: f32 = 0.3;

impl SeamPlan {
    pub fn new(field: &Landscape, layout: &Layout, regions: &[RegionId], contours: &[[Point; 2]], levels: &Levels) -> Self {
        // Each boundary edge with the coarsest level of the regions it bounds.
        let mut sided: Vec<(Edge, u8)> = regions.iter().flat_map(|&id| layout.boundary(id).into_iter().map(move |e| (e, levels.of(id)))).collect();
        sided.sort();
        let mut level_of: BTreeMap<Edge, u8> = BTreeMap::new();
        for (edge, level) in sided {
            let slot = level_of.entry(edge).or_insert(level);
            *slot = (*slot).max(level);
        }
        let boundary: Vec<Edge> = level_of.keys().copied().collect();
        let index = SegmentIndex::new(boundary.iter().map(|&e| [seam_point(e, 0.), seam_point(e, 1.)]), 1.0);
        let mut crossings: Vec<Vec<f32>> = vec![vec![]; boundary.len()];
        let mut pieces: HashMap<RegionId, Vec<[Point; 2]>> = HashMap::new();
        let mut rows = HashMap::new();
        for &[a, b] in contours {
            if (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-4 {
                continue;
            }
            // Where the contour meets seams: (parameter along it, exact seam point),
            // and the parameter intervals in which it runs along a seam.
            let mut events: Vec<(f32, Point)> = vec![];
            let mut along: Vec<[f32; 2]> = vec![];
            for &i in &index.candidates_between(a, b) {
                let edge = boundary[i as usize];
                let meeting = meet([a, b], [seam_point(edge, 0.), seam_point(edge, 1.)]);
                for &(s, t) in &meeting.points {
                    crossings[i as usize].push(t);
                    events.push((s, seam_point(edge, t)));
                }
                along.extend(meeting.along);
            }
            events.sort_by(|x, y| x.0.total_cmp(&y.0));
            let row = |s: f32| match (field.wall_row(a), field.wall_row(b)) {
                (Some(ra), Some(rb)) => Some(ra + (rb - ra) * s),
                _ => None,
            };
            let mut start = (0., a);
            for (s, p) in events.into_iter().chain([(1., b)]) {
                if let Some(r) = row(s).filter(|_| p != b) {
                    rows.insert(p.map(f32::to_bits), r);
                }
                // An endpoint on a seam is replaced by the exact seam point.
                if s == 0. {
                    start = (0., p);
                    continue;
                }
                // A part running along a seam is drawn by the seam itself.
                let on_seam = along.iter().any(|&[s0, s1]| (start.0 + s) * 0.5 > s0 && (start.0 + s) * 0.5 < s1);
                if (p[0] - start.1[0]).hypot(p[1] - start.1[1]) > 1e-5 && !on_seam {
                    let mid = [(p[0] + start.1[0]) * 0.5, (p[1] + start.1[1]) * 0.5];
                    if let Some(id) = layout.of_point(mid) {
                        pieces.entry(id).or_default().push([start.1, p]);
                    }
                }
                start = (s, p);
            }
        }
        let jobs: Vec<(Edge, Vec<f32>)> = boundary.iter().copied().zip(crossings).collect();
        let points = super::parallel_map(&jobs, |(edge, cuts)| seam_points(field, *edge, cuts.clone(), Levels::scale(level_of[edge])));
        let edges = boundary.into_iter().zip(points).collect();
        Self { edges, pieces, rows }
    }

    /// The seam polyline segments of a region's boundary.
    fn seams(&self, layout: &Layout, id: RegionId) -> Vec<[Point; 2]> {
        let mut out = vec![];
        let mut boundary = layout.boundary(id);
        boundary.sort();
        boundary.dedup();
        for edge in boundary {
            for pair in self.edges[&edge].windows(2) {
                out.push([pair[0], pair[1]]);
            }
        }
        out
    }

    /// A region's constraints: its seam segments and its contour pieces.
    pub fn constraints(&self, layout: &Layout, id: RegionId) -> (Vec<[Point; 2]>, Vec<[Point; 2]>) {
        let pieces = self.pieces.get(&id).cloned().unwrap_or_default();
        (self.seams(layout, id), pieces)
    }

    /// Points of a boundary edge, for the seam tests.
    #[cfg(test)]
    pub fn edge_points(&self) -> impl Iterator<Item = (&Edge, &Vec<Point>)> {
        self.edges.iter()
    }

    pub fn row(&self, p: Point) -> Option<f32> {
        self.rows.get(&p.map(f32::to_bits)).copied()
    }
}

/// Uniform points by the smallest requested edge length along the edge (times the
/// scale of the edge's level), plus the contour crossings; uniform points too close to
/// a crossing go.
fn seam_points(field: &Landscape, edge: Edge, mut cuts: Vec<f32>, scale: f32) -> Vec<Point> {
    let (a, b) = (seam_point(edge, 0.), seam_point(edge, 1.));
    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
    // The smallest request anywhere on the edge: within half its length of the middle.
    let h = (field.edge_length(seam_point(edge, 0.5), length * 0.5) * scale).min(sizing::MAX_EDGE);
    let n = (length / h).ceil().max(1.) as u32;
    let gap = CROSSING_GAP * h / length;
    cuts.sort_by(f32::total_cmp);
    cuts.dedup();
    let mut ts: Vec<f32> = (0..=n)
        .map(|k| k as f32 / n as f32)
        .filter(|&t| t == 0. || t == 1. || cuts.iter().all(|c| (c - t).abs() >= gap))
        .collect();
    ts.extend(cuts.iter().copied().filter(|&c| c > 0. && c < 1.));
    ts.sort_by(f32::total_cmp);
    ts.dedup();
    ts.into_iter().map(|t| seam_point(edge, t)).collect()
}

/// How a contour segment meets a seam edge.
#[derive(Default)]
struct Meeting {
    /// (parameter along the contour, parameter along the edge) of each meeting point.
    points: Vec<(f32, f32)>,
    /// Parameter intervals of the contour that lie on the edge.
    along: Vec<[f32; 2]>,
}

/// A contour closer than this to a seam edge is on it.
const ON_SEAM: f64 = 1e-4;

/// Where segment `c` meets seam edge `e`, in f64: a proper crossing, an
/// endpoint lying on the edge, or a stretch running along it (rivers follow
/// hex edges, and so do seams). A meeting within a hair of an edge end snaps
/// to that lattice corner, a seam point of every region around it.
fn meet(c: [Point; 2], e: [Point; 2]) -> Meeting {
    let p = |q: Point| [q[0] as f64, q[1] as f64];
    let (a, b, p0, p1) = (p(c[0]), p(c[1]), p(e[0]), p(e[1]));
    let edge = [p1[0] - p0[0], p1[1] - p0[1]];
    let length = edge[0].hypot(edge[1]);
    // Signed distance from the edge line and parameter along the edge.
    let place = |q: [f64; 2]| {
        let w = [q[0] - p0[0], q[1] - p0[1]];
        ((edge[0] * w[1] - edge[1] * w[0]) / length, (edge[0] * w[0] + edge[1] * w[1]) / (length * length))
    };
    let snap = |t: f64| if t < 1e-5 { 0. } else if t > 1. - 1e-5 { 1. } else { t as f32 };
    let on_edge = |t: f64| (-1e-7..=1. + 1e-7).contains(&t);
    let ((da, ta), (db, tb)) = (place(a), place(b));
    let mut meeting = Meeting::default();
    if da.abs() < ON_SEAM && db.abs() < ON_SEAM {
        // Along the edge line: the overlap with the edge, in contour parameters.
        let s_of = |t: f64| ((t - ta) / (tb - ta)).clamp(0., 1.);
        let (s0, s1) = (s_of(0.).min(s_of(1.)), s_of(0.).max(s_of(1.)));
        if s1 - s0 > 1e-9 {
            for s in [s0, s1] {
                meeting.points.push((s as f32, snap(ta + (tb - ta) * s)));
            }
            meeting.along.push([s0 as f32, s1 as f32]);
        }
        return meeting;
    }
    for (d, t, s) in [(da, ta, 0.), (db, tb, 1.)] {
        if d.abs() < ON_SEAM && on_edge(t) {
            meeting.points.push((s, snap(t)));
        }
    }
    if da.abs() >= ON_SEAM && db.abs() >= ON_SEAM && (da < 0.) != (db < 0.) {
        let s = da / (da - db);
        let t = ta + (tb - ta) * s;
        if on_edge(t) {
            meeting.points.push((s as f32, snap(t)));
        }
    }
    meeting
}

/// One region's triangulation: its seams and contour pieces as constraints,
/// lattice points of the sizing field inside its cells.
pub struct RegionTriangles {
    pub positions: Vec<Point>,
    /// Ground faces, counter-clockwise from above, with the water under their centroid.
    pub faces: Vec<([u32; 3], SurfaceWaterKind)>,
}

/// `scale`: how much longer than the sizing field asks the region's edges may be (its
/// detail level).
pub fn triangulate(field: &Landscape, layout: &Layout, plan: &SeamPlan, id: RegionId, scale: f32) -> RegionTriangles {
    let (seams, pieces) = plan.constraints(layout, id);
    let constraints: Vec<[Point; 2]> = seams.into_iter().chain(pieces).collect();
    let clearance = SegmentIndex::new(constraints.iter().copied(), 0.5);
    let [low, high] = layout.bounds(id);
    let points: Vec<_> = sizing::lattice(low, high, |p, slack| field.edge_length(p, slack) * scale)
        .into_iter()
        .filter(|point| {
            let p = point.position;
            if layout.of_point(p) != Some(id) {
                return false;
            }
            let reach = sizing::CONTOUR_CLEARANCE * point.size;
            let near = clearance.nearest(p, reach, |i| {
                let [a, b] = constraints[i as usize];
                closest(p, a, b).0
            });
            near.is_none() && !field.inside_wall(p[0], p[1])
        })
        .map(|point| Point2::new(point.position[0] as f64, point.position[1] as f64))
        .collect();
    let mut cdt = Cdt::bulk_load(points).expect("finite presentation points");
    for &[a, b] in &constraints {
        let ah = cdt.insert(Point2::new(a[0] as f64, a[1] as f64)).expect("finite constraint");
        let bh = cdt.insert(Point2::new(b[0] as f64, b[1] as f64)).expect("finite constraint");
        if ah != bh {
            cdt.add_constraint_and_split(ah, bh, |p| p);
        }
    }
    let positions: Vec<Point> = cdt.vertices().map(|v| [v.position().x as f32, v.position().y as f32]).collect();
    let mut faces = vec![];
    for face in cdt.inner_faces() {
        let mut triangle = face.vertices().map(|v| v.fix().index() as u32);
        let p = triangle.map(|i| positions[i as usize]);
        let center = [(p[0][0] + p[1][0] + p[2][0]) / 3., (p[0][1] + p[1][1] + p[2][1]) / 3.];
        let area = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
        // Seam points of one straight edge are collinear up to rounding: the
        // hull spans them with slivers, which are not part of any region.
        let longest = (0..3).map(|i| (p[i][0] - p[(i + 1) % 3][0]).hypot(p[i][1] - p[(i + 1) % 3][1])).fold(0., f32::max);
        if area.abs() < 1e-5 * longest * longest || layout.of_point(center) != Some(id) {
            continue;
        }
        // XZ orientation is opposite the 3D upward normal.
        if area > 0. {
            triangle.swap(1, 2);
        }
        faces.push((triangle, field.water_at(center[0], center[1]).kind));
    }
    RegionTriangles { positions, faces }
}

/// Every cell within `HALO_RINGS` of a region: the cells whose change can
/// change the region's field.
#[cfg(test)]
pub fn halo(layout: &Layout, id: RegionId) -> Vec<TileId> {
    let rings = HALO_RINGS as i32;
    let mut cells = vec![];
    for r in 0..layout.height as i32 {
        for q in 0..layout.width as i32 {
            let near = layout.cells(id).any(|(cq, cr)| {
                let (dq, dr) = (q - cq as i32, r - cr as i32);
                (dq.abs() + dr.abs() + (dq + dr).abs()) / 2 <= rings
            });
            if near {
                cells.push(TileId(r as u32 * layout.width + q as u32));
            }
        }
    }
    cells
}
