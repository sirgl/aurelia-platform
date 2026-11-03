//! Placement data derived from the baked meshes (no new art files): the footprint a model
//! touches the ground with, the stone plinth and wooden pile every seated model may stand
//! on, and the city wall ring split into parts that each take their own place on the ground
//! (build/world.rs emits them as models of their own).
//!
//! Everything here is a pure function of the loaded GLB meshes and the palette, so the
//! baked bytes stay deterministic.

use crate::glb::{cross, extent, sub, Mesh};
use std::collections::BTreeMap;

/// Vertices up to this far above the nominal ground plane (model y = 0) touch the ground.
const NOMINAL_BAND: f32 = 0.01;
/// Vertices this close to the lowest point touch the ground, even when it lies deeper.
const LOW_BAND: f32 = 0.035;
/// Footprint samples a model keeps: its centre and the farthest-apart points of its base.
pub const MAX_CONTACTS: usize = 11;
/// A new contact must be at least this far (hex units) from the chosen ones.
const MIN_SPREAD: f32 = 0.02;

/// The ground footprint of a model: sample points (x, z), their centre and enclosing radius.
pub struct Foot {
    pub points: Vec<[f32; 2]>,
    pub centre: [f32; 2],
    pub radius: f32,
}

/// Footprint of the LOD0 base: the centroid of the base vertices, then farthest-point samples
/// of the base outline. The same count for every model, so seating costs one probe each.
pub fn foot(mesh: &Mesh, max_points: usize) -> Foot {
    let (lo, _) = extent(&mesh.pos);
    let limit = (lo[1] + LOW_BAND).max(NOMINAL_BAND);
    let mut base: Vec<[f32; 2]> = mesh
        .pos
        .iter()
        .filter(|v| v[1] <= limit)
        .map(|v| [(v[0] * 1000.0).round() / 1000.0, (v[2] * 1000.0).round() / 1000.0])
        .collect();
    base.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    base.dedup();
    if base.is_empty() {
        return Foot { points: vec![[0.0, 0.0]], centre: [0.0, 0.0], radius: 0.0 };
    }
    let n = base.len() as f32;
    let centre = [base.iter().map(|p| p[0]).sum::<f32>() / n, base.iter().map(|p| p[1]).sum::<f32>() / n];
    let radius = base.iter().map(|p| (p[0] - centre[0]).hypot(p[1] - centre[1])).fold(0.0f32, f32::max);
    let mut points = vec![centre];
    let mut nearest: Vec<f32> = base.iter().map(|p| (p[0] - centre[0]).hypot(p[1] - centre[1])).collect();
    while points.len() < max_points {
        let (at, far) = nearest
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, d)| (i, *d))
            .unwrap_or((0, 0.0));
        if far < MIN_SPREAD {
            break;
        }
        let chosen = base[at];
        points.push(chosen);
        for (d, p) in nearest.iter_mut().zip(&base) {
            *d = d.min((p[0] - chosen[0]).hypot(p[1] - chosen[1]));
        }
    }
    Foot { points, centre, radius }
}

fn colour(palette: &BTreeMap<String, [f32; 3]>, name: &str) -> [f32; 3] {
    *palette.get(name).unwrap_or_else(|| panic!("support models: palette lacks {name}"))
}

/// Adds a triangle whose flat normal faces `outward` (winding fixed here).
fn face(mesh: &mut Mesh, a: [f32; 3], b: [f32; 3], c: [f32; 3], outward: [f32; 3], rgb: [f32; 3]) {
    let n = cross(sub(b, a), sub(c, a));
    let (b, c) = if n[0] * outward[0] + n[1] * outward[1] + n[2] * outward[2] < 0.0 { (c, b) } else { (b, c) };
    for p in [a, b, c] {
        mesh.pos.push(p);
        mesh.rgb.push(rgb);
        mesh.team.push(0.0);
    }
}

fn empty() -> Mesh {
    Mesh { pos: Vec::new(), rgb: Vec::new(), team: Vec::new() }
}

/// A vertical band of a prism: ring of `sides` points at radius `r` from y0 to y1.
fn wall(mesh: &mut Mesh, sides: usize, r: f32, y0: f32, y1: f32, rgb: [f32; 3]) {
    for k in 0..sides {
        let (a0, a1) = (std::f32::consts::TAU * k as f32 / sides as f32, std::f32::consts::TAU * (k + 1) as f32 / sides as f32);
        let mid = (a0 + a1) / 2.0;
        let out = [mid.cos(), 0.0, mid.sin()];
        let (p0, p1) = ([r * a0.cos(), r * a0.sin()], [r * a1.cos(), r * a1.sin()]);
        let (b0, b1) = ([p0[0], y0, p0[1]], [p1[0], y0, p1[1]]);
        let (t0, t1) = ([p0[0], y1, p0[1]], [p1[0], y1, p1[1]]);
        face(mesh, b0, b1, t1, out, rgb);
        face(mesh, b0, t1, t0, out, rgb);
    }
}

fn cap(mesh: &mut Mesh, sides: usize, r: f32, y: f32, rgb: [f32; 3]) {
    for k in 0..sides {
        let (a0, a1) = (std::f32::consts::TAU * k as f32 / sides as f32, std::f32::consts::TAU * (k + 1) as f32 / sides as f32);
        face(
            mesh,
            [0.0, y, 0.0],
            [r * a0.cos(), y, r * a0.sin()],
            [r * a1.cos(), y, r * a1.sin()],
            [0.0, 1.0, 0.0],
            rgb,
        );
    }
}

/// Low stone plinth of unit radius and unit height: a body and a slightly wider lip on top.
/// An instance scales x/z by the footprint radius and y by the height to fill; the underside
/// is never seen, the top is flush with the model above.
pub fn plinth(palette: &BTreeMap<String, [f32; 3]>) -> [Mesh; 3] {
    let (body, lip) = (colour(palette, "rock"), colour(palette, "stone"));
    [10usize, 8, 6].map(|sides| {
        let mut m = empty();
        wall(&mut m, sides, 1.0, 0.0, 0.8, body);
        wall(&mut m, sides, 1.06, 0.8, 1.0, lip);
        cap(&mut m, sides, 1.06, 1.0, lip);
        m
    })
}

/// Wooden pile of unit radius and unit height, six-sided; scaled to reach the sea floor.
pub fn pile(palette: &BTreeMap<String, [f32; 3]>) -> [Mesh; 3] {
    let wood = colour(palette, "wood_dark");
    [6usize, 5, 4].map(|sides| {
        let mut m = empty();
        wall(&mut m, sides, 1.0, 0.0, 1.0, wood);
        cap(&mut m, sides, 1.0, 1.0, wood);
        m
    })
}

/// How far the wall footing reaches below the wall base (hex units).
pub const FOOTING_DEPTH: f32 = 0.32;
/// The wall base sits this far below the nominal ground in the authored mesh.
const BASE_EPS: f32 = 0.006;

/// Appends a footing under the downward faces at the mesh base: a vertical skirt below every
/// boundary edge of the base triangles, `depth` deep, so a wall part standing on uneven ground
/// has stone down to the ground on its low side and buries the footing on its high side.
fn add_footing(mesh: &mut Mesh, depth: f32, rgb: [f32; 3]) {
    let (lo, _) = extent(&mesh.pos);
    let key = |p: [f32; 3]| ((p[0] * 1000.0).round() as i32, (p[2] * 1000.0).round() as i32);
    type Edge = ((i32, i32), (i32, i32));
    let mut edges: BTreeMap<Edge, (usize, [f32; 3], [f32; 3], [f32; 3])> = BTreeMap::new();
    for t in 0..mesh.triangles() {
        let [a, b, c] = [mesh.pos[3 * t], mesh.pos[3 * t + 1], mesh.pos[3 * t + 2]];
        let n = cross(sub(b, a), sub(c, a));
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
        let down = n[1] / len < -0.9 && [a, b, c].iter().all(|p| p[1] <= lo[1] + BASE_EPS);
        if !down {
            continue;
        }
        for (p, q, third) in [(a, b, c), (b, c, a), (c, a, b)] {
            let (kp, kq) = (key(p), key(q));
            let ordered: Edge = if kp <= kq { (kp, kq) } else { (kq, kp) };
            let entry = edges.entry(ordered).or_insert((0, p, q, third));
            entry.0 += 1;
        }
    }
    for (count, p, q, inside) in edges.into_values() {
        if count != 1 {
            continue;
        }
        let mid = [(p[0] + q[0]) / 2.0, (p[2] + q[2]) / 2.0];
        let out = [mid[0] - inside[0], 0.0, mid[1] - inside[2]];
        let (p_low, q_low) = ([p[0], p[1] - depth, p[2]], [q[0], q[1] - depth, q[2]]);
        face(mesh, p, q, q_low, out, rgb);
        face(mesh, p, q_low, p_low, out, rgb);
    }
}

/// The ring of the city walls cut into parts: one per hex corner (its tower and the two
/// half-runs beside it, nearest-corner rule) and the gatehouse, which sits across a seam and is
/// kept whole. Every part keeps the ring's own coordinates (its anchor is the tile centre) and
/// gets a footing; parts are seated one by one, so the ring follows uneven ground.
pub fn wall_parts(lods: &[Mesh; 3], palette: &BTreeMap<String, [f32; 3]>) -> Vec<[Mesh; 3]> {
    const CORNERS: usize = 6;
    // Corners at 30 + 60k degrees (the hex is pointy-top, model x/z are game x/z).
    let corner = |k: usize| (30.0 + 60.0 * k as f32).to_radians();
    let mid_of = |k: usize| (60.0 * k as f32).to_radians();
    let reach = {
        let (_, hi) = extent(&lods[0].pos);
        hi[2] - 0.04
    };
    let edge_mid = |k: usize| [reach * 0.866 * mid_of(k).cos(), reach * 0.866 * mid_of(k).sin()];
    // The gatehouse: the edge middle with the most tall vertices around it.
    let tall_near = |k: usize| {
        let m = edge_mid(k);
        lods[0].pos.iter().filter(|v| v[1] > 0.14 && (v[0] - m[0]).hypot(v[2] - m[1]) < 0.09).count()
    };
    let gate = (0..CORNERS).map(|k| (tall_near(k), k)).max().filter(|(n, _)| *n > 0).map(|(_, k)| k);
    let footing = colour(palette, "rock");
    let mut parts: Vec<[Mesh; 3]> = Vec::new();
    for part in 0..=CORNERS {
        if part == CORNERS && gate.is_none() {
            break;
        }
        let meshes: Vec<Mesh> = lods
            .iter()
            .map(|mesh| {
                let mut out = empty();
                for t in 0..mesh.triangles() {
                    let tri = [mesh.pos[3 * t], mesh.pos[3 * t + 1], mesh.pos[3 * t + 2]];
                    let c = [(tri[0][0] + tri[1][0] + tri[2][0]) / 3.0, (tri[0][2] + tri[1][2] + tri[2][2]) / 3.0];
                    let in_gate = gate.is_some_and(|k| {
                        let m = edge_mid(k);
                        (c[0] - m[0]).hypot(c[1] - m[1]) < 0.1
                    });
                    let owner = if in_gate {
                        CORNERS
                    } else {
                        let angle = c[1].atan2(c[0]);
                        (0..CORNERS)
                            .min_by(|a, b| {
                                let d = |k: usize| (angle - corner(k)).sin().atan2((angle - corner(k)).cos()).abs();
                                d(*a).total_cmp(&d(*b))
                            })
                            .unwrap_or(0)
                    };
                    if owner == part {
                        out.pos.extend_from_slice(&tri);
                        out.rgb.extend_from_slice(&mesh.rgb[3 * t..3 * t + 3]);
                        out.team.extend_from_slice(&mesh.team[3 * t..3 * t + 3]);
                    }
                }
                out
            })
            .collect();
        let mut meshes = meshes;
        for mesh in &mut meshes {
            if mesh.pos.is_empty() {
                continue;
            }
            add_footing(mesh, FOOTING_DEPTH, footing);
        }
        // A reduced LOD without any triangle of this part (the far gatehouse has none) falls
        // back to the next finer LOD.
        for lod in 1..3 {
            if meshes[lod].pos.is_empty() {
                meshes[lod] = Mesh {
                    pos: meshes[lod - 1].pos.clone(),
                    rgb: meshes[lod - 1].rgb.clone(),
                    team: meshes[lod - 1].team.clone(),
                };
            }
        }
        parts.push(meshes.try_into().ok().expect("three LODs"));
    }
    parts
}
