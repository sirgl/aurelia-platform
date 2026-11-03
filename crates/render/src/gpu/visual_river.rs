//! Continuous water ribbons on actual shared hex edges, plus sandbox props.
use crate::{
    gpu::feature::FeatureVertex,
    scene::{
        fog::fog_at,
        layout::{tile_to_world, EDGE_NEIGHBORS},
        picking::world_to_tile,
        terrain::{
            corner_lattice, evaluator::sample_surface, fields::TerrainGeometry, lattice_to_world,
            LatticeKey,
        },
    },
    world::state::RenderWorld,
};
use kernel::ids::TileId;
use std::collections::{BTreeMap, BTreeSet};

fn height(g: &TerrainGeometry, x: f32, z: f32) -> f32 {
    sample_surface(g, x, z).map_or(0.3, |p| p.y)
}
fn tri(out: &mut Vec<FeatureVertex>, p: [[f32; 3]; 3], c: [f32; 3]) {
    for pos in p {
        out.push(FeatureVertex {
            pos,
            normal: [0., 1., 0.],
            color: c,
            fog: 1.,
        });
    }
}
fn quad(out: &mut Vec<FeatureVertex>, p: [[f32; 3]; 4], c: [f32; 3]) {
    tri(out, [p[0], p[1], p[2]], c);
    tri(out, [p[0], p[2], p[3]], c);
}
fn sections(pts: &[[f32; 2]], width: f32) -> Vec<[[f32; 2]; 2]> {
    let normal = |a: [f32; 2], b: [f32; 2]| {
        let dx = b[0] - a[0];
        let dz = b[1] - a[1];
        let len = dx.hypot(dz).max(0.0001);
        [-dz / len, dx / len]
    };
    (0..pts.len())
        .map(|i| {
            let p = pts[i];
            let n0 = normal(pts[i.saturating_sub(1)], pts[(i + 1).min(pts.len() - 1)]);
            let offset = if i == 0 || i == pts.len() - 1 {
                [n0[0] * width, n0[1] * width]
            } else {
                let a = normal(pts[i - 1], p);
                let b = normal(p, pts[i + 1]);
                let sum = [a[0] + b[0], a[1] + b[1]];
                let len = sum[0].hypot(sum[1]).max(0.0001);
                let m = [sum[0] / len, sum[1] / len];
                let reach = width / (m[0] * b[0] + m[1] * b[1]).max(0.65);
                [m[0] * reach, m[1] * reach]
            };
            [
                [p[0] - offset[0], p[1] - offset[1]],
                [p[0] + offset[0], p[1] + offset[1]],
            ]
        })
        .collect()
}
fn strip(
    out: &mut Vec<FeatureVertex>,
    g: &TerrainGeometry,
    pts: &[[f32; 2]],
    width: f32,
    lift: f32,
    c: [f32; 3],
) {
    if pts.len() < 2 {
        return;
    }
    // Both triangles at every join use the SAME two cross-section vertices.
    let cross: Vec<_> = sections(pts, width)
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let y = height(g, p[0][0], p[0][1])
                .max(height(g, p[1][0], p[1][1]))
                .max(height(g, pts[i][0], pts[i][1]))
                + lift;
            [[p[0][0], y, p[0][1]], [p[1][0], y, p[1][1]]]
        })
        .collect();
    for p in cross.windows(2) {
        quad(out, [p[0][0], p[1][0], p[1][1], p[0][1]], c);
    }
}

fn edge_nodes(w: &RenderWorld, a: u32, b: u32) -> Option<[LatticeKey; 2]> {
    if w.width == 0 || a as usize >= w.terrain.len() || b as usize >= w.terrain.len() {
        return None;
    }
    let (aq, ar) = ((a % w.width) as i32, (a / w.width) as i32);
    let (bq, br) = ((b % w.width) as i32, (b / w.width) as i32);
    let dir = EDGE_NEIGHBORS
        .iter()
        .position(|&(dq, dr)| aq + dq == bq && ar + dr == br)?;
    Some([
        corner_lattice(aq, ar, dir),
        corner_lattice(aq, ar, (dir + 1) % 6),
    ])
}
fn river_paths(w: &RenderWorld) -> Vec<Vec<[f32; 2]>> {
    let mut graph: BTreeMap<LatticeKey, Vec<LatticeKey>> = BTreeMap::new();
    let mut remaining = BTreeSet::new();
    for &(a, b) in &w.river_edges {
        // Unknown neighbours cannot contribute live terrain or river information.
        if fog_at(w, TileId(a)) < 0.4 || fog_at(w, TileId(b)) < 0.4 {
            continue;
        }
        let Some([p, q]) = edge_nodes(w, a, b) else {
            continue;
        };
        let edge = (p.min(q), p.max(q));
        if remaining.insert(edge) {
            graph.entry(p).or_default().push(q);
            graph.entry(q).or_default().push(p);
        }
    }
    let mut paths = Vec::new();
    while let Some(&(a, b)) = remaining.first() {
        let start = graph
            .keys()
            .copied()
            .find(|p| {
                graph[p]
                    .iter()
                    .filter(|q| remaining.contains(&((*p).min(**q), (*p).max(**q))))
                    .count()
                    == 1
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
            if graph[&p].len() != 2 {
                break;
            }
        }
        if path.len() == 1 {
            remaining.remove(&(a, b));
            continue;
        }
        // Logical paths stay on shared boundaries; only their presentation is smoothed.
        paths.push(
            path.into_iter()
                .map(|key| {
                    let (x, z) = lattice_to_world(key);
                    [x, z]
                })
                .collect(),
        );
    }
    paths
}
/// Broad bends stay in the edge corridor, with endpoints fixed for mouths and junctions.
fn organic_path(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut out = pts.to_vec();
    for i in 1..pts.len() - 1 {
        let dx = pts[i + 1][0] - pts[i - 1][0];
        let dz = pts[i + 1][1] - pts[i - 1][1];
        let len = dx.hypot(dz).max(0.001);
        let [x, z] = pts[i];
        let bend = 0.09 * ((x * 0.71 + z * 0.43).sin() + 0.35 * (z * 1.27 - x * 0.3).sin());
        out[i] = [x - dz / len * bend, z + dx / len * bend];
    }
    for _ in 0..3 {
        let mut rounded = vec![out[0]];
        for edge in out.windows(2) {
            for t in [0.25, 0.75] {
                rounded.push([
                    edge[0][0] * (1. - t) + edge[1][0] * t,
                    edge[0][1] * (1. - t) + edge[1][1] * t,
                ]);
            }
        }
        rounded.push(*out.last().unwrap());
        out = rounded;
    }
    out
}

fn river_surface(out: &mut Vec<FeatureVertex>, g: &TerrainGeometry, pts: &[[f32; 2]]) {
    let edges = sections(pts, 0.235);
    let mut rows = Vec::with_capacity(pts.len());
    for (p, sides) in pts.iter().zip(edges) {
        let width = 0.93
            + 0.10 * (p[0] * 0.87 + p[1] * 0.51).sin()
            + 0.04 * (p[0] * 1.73 - p[1] * 0.31).sin();
        let dx = (sides[1][0] - p[0]) * width;
        let dz = (sides[1][1] - p[1]) * width;
        let y = height(g, p[0] - dx, p[1] - dz)
            .max(height(g, p[0] + dx, p[1] + dz))
            .max(height(g, p[0], p[1]))
            + 0.125;
        rows.push([-1.0, -0.90, -0.55, 0.55, 0.90, 1.0].map(|t| [p[0] + dx * t, y, p[1] + dz * t]));
    }
    // A meander crosses the old carved edge mesh. Keep its water profile above
    // those small ridges without reproducing every hex-corner depression.
    let mut water_y: Vec<_> = rows.iter().map(|row| row[0][1]).collect();
    for i in 1..water_y.len() {
        let distance = (pts[i][0] - pts[i - 1][0]).hypot(pts[i][1] - pts[i - 1][1]);
        water_y[i] = water_y[i].max(water_y[i - 1] - distance * 0.12);
    }
    for i in (0..water_y.len() - 1).rev() {
        let distance = (pts[i + 1][0] - pts[i][0]).hypot(pts[i + 1][1] - pts[i][1]);
        water_y[i] = water_y[i].max(water_y[i + 1] - distance * 0.12);
    }
    for (row, y) in rows.iter_mut().zip(water_y) {
        for p in row {
            p[1] = y;
        }
    }
    let colors = [
        [0.46, 0.59, 0.36],
        [0.23, 0.60, 0.65],
        [0.11, 0.51, 0.69],
        [0.11, 0.51, 0.69],
        [0.23, 0.60, 0.65],
        [0.46, 0.59, 0.36],
    ];
    for rows in rows.windows(2) {
        for j in 0..5 {
            for (row, lane) in [(0, j), (1, j), (1, j + 1), (0, j), (1, j + 1), (0, j + 1)] {
                out.push(FeatureVertex {
                    pos: rows[row][lane],
                    normal: [0., 1., 0.],
                    color: colors[lane],
                    fog: 1.,
                });
            }
        }
    }
}

/// Point and tangent on the actual visible centreline, used by bridge placement.
fn nearest_anchor(paths: &[Vec<[f32; 2]>], p: [f32; 2]) -> Option<([f32; 2], [f32; 2])> {
    let mut closest = None;
    let mut best = f32::INFINITY;
    for path in paths {
        for edge in path.windows(2) {
            let d = [edge[1][0] - edge[0][0], edge[1][1] - edge[0][1]];
            let length = d[0].hypot(d[1]);
            if length < 1e-6 {
                continue;
            }
            let t = (((p[0] - edge[0][0]) * d[0] + (p[1] - edge[0][1]) * d[1]) / (length * length))
                .clamp(0., 1.);
            let q = [edge[0][0] + t * d[0], edge[0][1] + t * d[1]];
            let distance = (q[0] - p[0]).hypot(q[1] - p[1]);
            if distance < best {
                best = distance;
                closest = Some((q, [d[0] / length, d[1] / length]));
            }
        }
    }
    closest
}
/// Highest rendered ribbon at a point, including interpolation across slopes.
fn ribbon_height(vertices: &[FeatureVertex], x: f32, z: f32) -> f32 {
    let mut height = f32::NEG_INFINITY;
    for t in vertices.chunks_exact(3) {
        let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
        let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
        if d.abs() < 1e-8 {
            continue;
        }
        let u = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
        let v = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
        if u >= -1e-4 && v >= -1e-4 && u + v <= 1.0001 {
            height = height.max(u * a[1] + v * b[1] + (1. - u - v) * c[1]);
        }
    }
    height
}

fn apply_visibility(w: &RenderWorld, vertices: Vec<FeatureVertex>) -> Vec<FeatureVertex> {
    let mut out = Vec::with_capacity(vertices.len());
    for triangle in vertices.chunks_exact(3) {
        let mut fog = 1.0f32;
        for v in triangle {
            let Some(t) = world_to_tile(v.pos[0], v.pos[2], (w.width, w.height)) else {
                fog = 0.0;
                break;
            };
            fog = fog.min(fog_at(w, t));
        }
        if fog < 0.4 {
            continue;
        }
        for v in triangle {
            let mut v = *v;
            v.fog = fog;
            out.push(v);
        }
    }
    out
}
fn smooth(pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out = vec![];
    for i in 0..pts.len() - 1 {
        let a = pts[i.saturating_sub(1)];
        let b = pts[i];
        let c = pts[i + 1];
        let d = pts[(i + 2).min(pts.len() - 1)];
        for j in 0..10 {
            let t = j as f32 / 10.;
            let mut p = [0.; 2];
            for k in 0..2 {
                p[k] = 0.5
                    * ((2. * b[k])
                        + (-a[k] + c[k]) * t
                        + (2. * a[k] - 5. * b[k] + 4. * c[k] - d[k]) * t * t
                        + (-a[k] + 3. * b[k] - 3. * c[k] + d[k]) * t * t * t);
            }
            out.push(p);
        }
    }
    out.push(*pts.last().unwrap());
    out
}
#[path = "visual_river_models.rs"]
mod models;

pub fn handles_world(w: &RenderWorld) -> bool {
    w.width == 32 && w.height == 28
}

pub fn build(w: &RenderWorld, g: &TerrainGeometry) -> Vec<FeatureVertex> {
    let mut out = vec![];
    if !handles_world(w) {
        return out;
    }
    if let Some(surface) = &g.surface {
        return surface_props(w, surface);
    }
    let paths: Vec<_> = river_paths(w).iter().map(|p| organic_path(p)).collect();
    for path in &paths {
        river_surface(&mut out, g, path);
    }
    let road = vec![
        [28.7, 20.0],
        [30.3, 19.5],
        [31.1769, 19.5],
        [32.3, 19.5],
        [34.8, 19.1],
        [36.4, 18.],
    ];
    let road = smooth(&road);
    strip(&mut out, g, &road, 0.13, 0.068, [0.74, 0.68, 0.44]);
    let bridge_edge = (13 * w.width + 11, 13 * w.width + 12);
    let bridge_exists = w
        .river_edges
        .iter()
        .any(|&(a, b)| (a, b) == bridge_edge || (b, a) == bridge_edge);
    let bridge_start = out.len();
    let nodes = edge_nodes(w, bridge_edge.0, bridge_edge.1).unwrap();
    let (ax, az) = lattice_to_world(nodes[0]);
    let (ex, ez) = lattice_to_world(nodes[1]);
    let midpoint = [(ax + ex) * 0.5, (az + ez) * 0.5];
    let ([bx, bz], tangent) = nearest_anchor(&paths, midpoint).unwrap_or((midpoint, [0., 1.]));
    let water_y = [-0.16, 0., 0.16]
        .into_iter()
        .map(|d| ribbon_height(&out, bx + tangent[0] * d, bz + tangent[1] * d))
        .fold(height(g, bx, bz), f32::max);
    let bridge_angle = (-tangent[0]).atan2(tangent[1]);
    let (sin, cos) = bridge_angle.sin_cos();
    out.extend(models::bridge(g, bx, bz, water_y).into_iter().map(|mut v| {
        let x = v.pos[0] - bx;
        let z = v.pos[2] - bz;
        v.pos[0] = bx + x * cos - z * sin;
        v.pos[2] = bz + x * sin + z * cos;
        let [nx, ny, nz] = v.normal;
        v.normal = [nx * cos - nz * sin, ny, nx * sin + nz * cos];
        v
    }));
    if !bridge_exists {
        out.truncate(bridge_start);
    }
    let ship_start = out.len();
    let (sx, sz) = tile_to_world(17, 14);
    out.extend(models::ship(sx, sz));
    if w.visible
        .get((14 * w.width + 17) as usize)
        .copied()
        .unwrap_or(0)
        == 0
    {
        out.truncate(ship_start);
    }
    apply_visibility(w, out)
}

struct BridgeSite {
    center: [f32; 2],
    across: [f32; 2],
    banks: [f32; 2],
    heights: [f32; 2],
    water_y: f32,
}
use crate::scene::terrain::surface::SurfaceMesh;

fn bridge_site(surface: &SurfaceMesh, midpoint: [f32; 2]) -> Option<BridgeSite> {
    let anchor = surface.nearest_river_anchor(midpoint)?;
    let center = [anchor.position[0], anchor.position[2]];
    if (center[0] - midpoint[0]).hypot(center[1] - midpoint[1]) > 0.8 {
        return None;
    }
    let mut across = [anchor.tangent[1], -anchor.tangent[0]];
    if across[0] < 0.0 {
        across = across.map(|v| -v);
    }
    let mut site = BridgeSite {
        center,
        across,
        banks: [0.0; 2],
        heights: [0.0; 2],
        water_y: anchor.position[1],
    };
    for (i, sign) in [-1.0, 1.0].into_iter().enumerate() {
        let mut bank = None;
        for step in 0..18 {
            let distance = (anchor.half_width * 1.7 + 0.04).max(0.34) + step as f32 * 0.035;
            if distance > 0.66 {
                break;
            }
            let x = center[0] + across[0] * distance * sign;
            let z = center[1] + across[1] * distance * sign;
            let Some(ground) = surface.sample_ground(x, z) else {
                continue;
            };
            let dry_width = [-0.145, 0.145].into_iter().all(|offset| {
                surface
                    .sample_water(x - across[1] * offset, z + across[0] * offset)
                    .is_none()
            });
            if surface.sample_water(x, z).is_none()
                && dry_width
                && ground.y >= anchor.position[1] - 0.08
            {
                bank = Some((distance, ground.y));
                break;
            }
        }
        let (distance, y) = bank?;
        site.banks[i] = distance;
        site.heights[i] = y;
    }
    Some(site)
}
const BRIDGE_DECK_END: f32 = 0.105;
const ROAD_LIFT: f32 = 0.018;
fn bridge_mesh(surface: &SurfaceMesh, site: &BridgeSite) -> Vec<FeatureVertex> {
    let span = site.banks[0] + site.banks[1];
    // A shallow crown clears water even where the dry bank slopes below its level.
    let crown = (site.water_y - (site.heights[0] + site.heights[1]) * 0.5 + 0.055).clamp(0., 0.14);
    let mut out: Vec<_> = crate::models::VISUAL_BRIDGE
        .iter()
        .map(|v| {
            let t = ((v.pos[0] + 0.43) / 0.86).clamp(0., 1.);
            let distance = -site.banks[0] + t * span;
            // Sample both real bank cross sections, including their cross slope.
            let banks = std::array::from_fn::<_, 2, _>(|i| {
                let along = if i == 0 {
                    -site.banks[0]
                } else {
                    site.banks[1]
                };
                surface
                    .sample_ground(
                        site.center[0] + site.across[0] * along - site.across[1] * v.pos[2],
                        site.center[1] + site.across[1] * along + site.across[0] * v.pos[2],
                    )
                    .map_or(site.heights[i], |p| p.y)
            });
            FeatureVertex {
                pos: [
                    site.center[0] + site.across[0] * distance - site.across[1] * v.pos[2],
                    banks[0] * (1. - t) + banks[1] * t + ROAD_LIFT + v.pos[1] - BRIDGE_DECK_END
                        + crown * (std::f32::consts::PI * t).sin(),
                    site.center[1] + site.across[1] * distance + site.across[0] * v.pos[2],
                ],
                normal: v.normal,
                color: v.color,
                fog: 1.,
            }
        })
        .collect();
    // Bank tilt and cross slope deform the model; recompute actual face normals.
    for face in out.chunks_exact_mut(3) {
        let a = glam::Vec3::from_array(face[0].pos);
        let b = glam::Vec3::from_array(face[1].pos);
        let c = glam::Vec3::from_array(face[2].pos);
        let n = (b - a).cross(c - a).normalize_or_zero().to_array();
        for v in face {
            v.normal = n;
        }
    }
    out
}

pub fn shader_roads() -> bool {
    std::env::var("AURELIA_VISUAL_ROADS").as_deref() != Ok("geometry")
}
/// Shared visible road footprint for the material mask and the geometry A/B path.
pub fn road_quads(w: &RenderWorld, surface: &SurfaceMesh) -> Vec<[[f32; 2]; 4]> {
    if !handles_world(w) {
        return vec![];
    }
    let site = scene_bridge_site(w, surface);
    let mut out = vec![];
    for path in road_paths(site.as_ref()) {
        let points = smooth(&path);
        for pair in sections(&points, 0.125).windows(2) {
            let quad = [pair[0][0], pair[1][0], pair[1][1], pair[0][1]];
            let valid = quad.iter().all(|p| {
                surface.sample_water(p[0], p[1]).is_none()
                    && surface.sample_ground(p[0], p[1]).is_some()
                    && world_to_tile(p[0], p[1], (w.width, w.height))
                        .is_some_and(|tile| fog_at(w, tile) >= 0.4)
            });
            if valid {
                out.push(quad);
            }
        }
    }
    out
}
fn surface_road(out: &mut Vec<FeatureVertex>, surface: &SurfaceMesh, quads: &[[[f32; 2]; 4]]) {
    for points in quads {
        for j in [0, 1, 2, 0, 2, 3] {
            let p = points[j];
            let sample = surface
                .sample_ground(p[0], p[1])
                .expect("validated road footprint");
            out.push(FeatureVertex {
                pos: [p[0], sample.y + ROAD_LIFT, p[1]],
                normal: sample.normal,
                color: [0.74, 0.68, 0.44],
                fog: 1.0,
            });
        }
    }
}

fn surface_ship(w: &RenderWorld, surface: &SurfaceMesh) -> Vec<FeatureVertex> {
    let (x, z) = tile_to_world(17, 14);
    let mut locations = vec![];
    for dz in -4i32..=4 {
        for dx in -4i32..=4 {
            locations.push((dx * dx + dz * dz, dx, dz));
        }
    }
    locations.sort_unstable();
    for (_, dx, dz) in locations {
        let (x, z) = (x + dx as f32 * 0.4, z + dz as f32 * 0.4);
        let Some(tile) = world_to_tile(x, z, (w.width, w.height)) else {
            continue;
        };
        if w.visible.get(tile.0 as usize).copied() != Some(1) {
            continue;
        }
        let Some(water) = surface.sample_water(x, z) else {
            continue;
        };
        let floats = [
            (0.0, 0.0),
            (-0.46, 0.0),
            (0.46, 0.0),
            (0.0, -0.9),
            (0.0, 1.15),
        ]
        .iter()
        .all(|&(dx, dz)| {
            let Some(wet) = surface.sample_water(x + dx, z + dz) else {
                return false;
            };
            let Some(ground) = surface.sample_ground(x + dx, z + dz) else {
                return false;
            };
            wet.y - ground.y > 0.09
        });
        if !floats {
            continue;
        }
        return models::ship(x, z)
            .into_iter()
            .map(|mut v| {
                v.pos[1] += water.y - 0.105;
                v
            })
            .collect();
    }
    vec![]
}
fn scene_bridge_site(w: &RenderWorld, surface: &SurfaceMesh) -> Option<BridgeSite> {
    let edge = (13 * w.width + 11, 13 * w.width + 12);
    if !w
        .river_edges
        .iter()
        .any(|&(a, b)| (a, b) == edge || (b, a) == edge)
    {
        return None;
    }
    let nodes = edge_nodes(w, edge.0, edge.1)?;
    let (ax, az) = lattice_to_world(nodes[0]);
    let (bx, bz) = lattice_to_world(nodes[1]);
    bridge_site(surface, [(ax + bx) * 0.5, (az + bz) * 0.5])
}
fn road_paths(site: Option<&BridgeSite>) -> Vec<Vec<[f32; 2]>> {
    let Some(site) = site else {
        return vec![vec![[28.7, 20.0], [30.3, 19.5], [34.8, 19.1], [36.4, 18.0]]];
    };
    let endpoint = |side: usize, extra: f32| {
        let sign = if side == 0 { -1.0 } else { 1.0 };
        [
            site.center[0] + site.across[0] * sign * (site.banks[side] + extra),
            site.center[1] + site.across[1] * sign * (site.banks[side] + extra),
        ]
    };
    vec![
        vec![
            [28.7, 20.0],
            [30.3, 19.5],
            endpoint(0, 0.35),
            endpoint(0, 0.0),
        ],
        vec![
            endpoint(1, 0.0),
            endpoint(1, 0.35),
            [34.8, 19.1],
            [36.4, 18.0],
        ],
    ]
}
fn surface_props(w: &RenderWorld, surface: &SurfaceMesh) -> Vec<FeatureVertex> {
    let mut out = vec![];
    if !shader_roads() {
        surface_road(&mut out, surface, &road_quads(w, surface));
    }
    if let Some(site) = scene_bridge_site(w, surface) {
        out.extend(bridge_mesh(surface, &site));
    }
    out.extend(surface_ship(w, surface));
    apply_visibility(w, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        scene::{
            heightfield::build_heightfield,
            style::{compile, default_style},
            terrain::fields::build_terrain_geometry,
        },
        world::state::TerrainCell,
    };
    fn world() -> RenderWorld {
        let n = 32 * 28;
        RenderWorld {
            width: 32,
            height: 28,
            visible: vec![1; n],
            explored: vec![1; n],
            terrain: (0..n)
                .map(|i| TerrainCell {
                    tile: TileId(i as u32),
                    terrain: 1,
                    owner: None,
                    resource: None,
                })
                .collect(),
            ..Default::default()
        }
    }
    fn geometry(w: &RenderWorld) -> TerrainGeometry {
        let style = compile(default_style()).unwrap();
        let hf = build_heightfield(w, &style);
        build_terrain_geometry(w, &style, &hf)
    }
    #[test]
    fn independent_surface_props_follow_ground_water_and_actual_river_banks() {
        let mut w = world();
        w.visual_surface = true;
        for cell in &mut w.terrain {
            if cell.tile.0 % w.width >= 16 {
                cell.terrain = 4;
            }
        }
        w.river_edges = vec![(13 * 32 + 11, 13 * 32 + 12)];
        let g = geometry(&w);
        let surface = g.surface.as_ref().unwrap();
        let nodes = edge_nodes(&w, 13 * 32 + 11, 13 * 32 + 12).unwrap();
        let (ax, az) = lattice_to_world(nodes[0]);
        let (bx, bz) = lattice_to_world(nodes[1]);
        let site = bridge_site(surface, [(ax + bx) * 0.5, (az + bz) * 0.5])
            .expect("dry banks beside river");
        let bridge = bridge_mesh(surface, &site);
        assert!(
            site.banks[0] + site.banks[1] <= 1.32,
            "bridge must not span two hexes"
        );
        for (sign, d) in [(-1.0, site.banks[0]), (1.0, site.banks[1])] {
            let landing: Vec<_> = bridge
                .iter()
                .filter(|v| {
                    let along = (v.pos[0] - site.center[0]) * site.across[0]
                        + (v.pos[2] - site.center[1]) * site.across[1];
                    (along - sign * d).abs() < 0.0001
                })
                .collect();
            assert!(!landing.is_empty());
            let mut top_meets_road = false;
            let mut underside_meets_ground = false;
            for v in landing {
                let ground = surface.sample_ground(v.pos[0], v.pos[2]).unwrap().y;
                let offset = v.pos[1] - ground;
                assert!(
                    (-0.0001..=ROAD_LIFT + 0.0001).contains(&offset),
                    "landing step or floating foot: {offset}"
                );
                top_meets_road |= (offset - ROAD_LIFT).abs() < 0.0001;
                underside_meets_ground |= offset.abs() < 0.0001;
            }
            assert!(top_meets_road && underside_meets_ground);
        }
        let water_y = surface
            .sample_water(site.center[0], site.center[1])
            .unwrap()
            .y;
        for v in &bridge {
            let along = (v.pos[0] - site.center[0]) * site.across[0]
                + (v.pos[2] - site.center[1]) * site.across[1];
            let t = (along + site.banks[0]) / (site.banks[0] + site.banks[1]);
            let grade = site.heights[0] * (1. - t) + site.heights[1] * t;
            assert!(
                v.pos[1] < grade + 0.22,
                "bridge became a tower above its banks"
            );
            if along.abs() < 0.035 {
                assert!(
                    v.pos[1] > water_y + 0.02,
                    "arch must leave real water clearance"
                );
            }
        }
        let props = build(&w, &g);
        let mut road_geometry = vec![];
        surface_road(&mut road_geometry, surface, &road_quads(&w, surface));
        if shader_roads() {
            assert!(
                !props.iter().any(|v| v.color == [0.74, 0.68, 0.44]),
                "mask mode has no road geometry"
            );
        }
        let roads: Vec<_> = road_geometry
            .iter()
            .filter(|v| v.color == [0.74, 0.68, 0.44])
            .collect();
        assert!(!roads.is_empty());
        for v in roads {
            let ground = surface.sample_ground(v.pos[0], v.pos[2]).unwrap();
            assert!((v.pos[1] - ground.y - 0.018).abs() < 0.0001);
            assert!(surface.sample_water(v.pos[0], v.pos[2]).is_none());
        }
        assert!(
            !props.iter().any(|v| v.color == [0.11, 0.51, 0.69]),
            "old ribbon must not overlap independent water"
        );
        let ship = surface_ship(&w, surface);
        assert!(!ship.is_empty(), "visible deep sea must retain the ship");
        w.visible.fill(0);
        assert!(
            surface_ship(&w, surface).is_empty(),
            "ship is not remembered scenery"
        );
    }

    #[test]
    fn low_bank_bridge_stays_short_and_keeps_the_water_open() {
        let mut w = world();
        w.visual_surface = true;
        w.river_edges = vec![(13 * 32 + 11, 13 * 32 + 12)];
        let mut g = geometry(&w);
        let surface = g.surface.as_mut().unwrap();
        let nodes = edge_nodes(&w, 13 * 32 + 11, 13 * 32 + 12).unwrap();
        let (ax, az) = lattice_to_world(nodes[0]);
        let (bx, bz) = lattice_to_world(nodes[1]);
        let midpoint = [(ax + bx) * 0.5, (az + bz) * 0.5];
        let level = surface.nearest_river_anchor(midpoint).unwrap().position[1];
        // Regression: the authored scene has shallow banks below the local water level.
        for triangle in &surface.land_triangles {
            for &index in triangle {
                surface.vertices[index as usize].position[1] = level - 0.035;
            }
        }
        let site = bridge_site(surface, midpoint).expect("short crossing on shallow dry banks");
        assert!(site.banks[0] + site.banks[1] <= 1.32);
        let bridge = bridge_mesh(surface, &site);
        let center: Vec<_> = bridge
            .iter()
            .filter(|v| {
                let along = (v.pos[0] - site.center[0]) * site.across[0]
                    + (v.pos[2] - site.center[1]) * site.across[1];
                along.abs() < 0.025
            })
            .collect();
        assert!(!center.is_empty());
        assert!(center.iter().all(|v| v.pos[1] > level + 0.02));
        assert!(bridge.iter().all(|v| v.pos[1] < level + 0.16));
        assert!(bridge
            .iter()
            .all(|v| v.normal.iter().all(|x| x.is_finite())));
    }

    #[test]
    fn custom_river_replaces_only_its_supported_scene() {
        let mut w = world();
        assert!(handles_world(&w));
        w.width = 31;
        assert!(!handles_world(&w));
        w.width = 32;
        w.height = 29;
        assert!(!handles_world(&w));
    }
    #[test]
    fn river_is_one_chain_of_shared_boundaries() {
        let mut w = world();
        let a = 3 * 32 + 3;
        w.river_edges = vec![(a, a + 1), (a, a + 32)];
        let paths = river_paths(&w);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].len(), 3);
        for &(a, b) in &w.river_edges {
            let [p, q] = edge_nodes(&w, a, b).unwrap();
            let (px, pz) = lattice_to_world(p);
            let (qx, qz) = lattice_to_world(q);
            let midpoint = [(px + qx) * 0.5, (pz + qz) * 0.5];
            let ca = tile_to_world(a % 32, a / 32);
            let cb = tile_to_world(b % 32, b / 32);
            let da = (midpoint[0] - ca.0).hypot(midpoint[1] - ca.1);
            let db = (midpoint[0] - cb.0).hypot(midpoint[1] - cb.1);
            assert!((da - db).abs() < 0.00001);
            assert!((da - 3f32.sqrt() * 0.5).abs() < 0.00001);
        }
    }
    #[test]
    fn ribbon_reuses_exact_cross_section_at_turns() {
        let w = world();
        let g = geometry(&w);
        let pts = [[10., 10.], [10., 11.], [10.866025, 11.5]];
        let mut vertices = Vec::new();
        strip(&mut vertices, &g, &pts, 0.23, 0.1, [0., 0.5, 0.7]);
        assert_eq!(vertices.len(), 12);
        assert_eq!(vertices[1].pos, vertices[6].pos);
        assert_eq!(vertices[2].pos, vertices[11].pos);
        for (p, sides) in pts.iter().zip(sections(&pts, 0.23)) {
            assert!(((sides[0][0] + sides[1][0]) * 0.5 - p[0]).abs() < 0.00001);
            assert!(((sides[0][1] + sides[1][1]) * 0.5 - p[1]).abs() < 0.00001);
        }
    }
    #[test]
    fn rounded_turns_stay_inside_edge_corridor_and_preserve_endpoints() {
        let path = [[10., 10.], [10., 11.], [10.866025, 11.5], [11.73205, 11.0]];
        let rounded = organic_path(&path);
        assert_eq!(rounded.first(), path.first());
        assert_eq!(rounded.last(), path.last());
        let distance = |p: [f32; 2], a: [f32; 2], b: [f32; 2]| {
            let d = [b[0] - a[0], b[1] - a[1]];
            let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1]))
                .clamp(0., 1.);
            (p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1])
        };
        for p in rounded {
            let deviation = path
                .windows(2)
                .map(|e| distance(p, e[0], e[1]))
                .fold(f32::INFINITY, f32::min);
            assert!(
                deviation < 0.28,
                "rounded river escaped edge corridor: {deviation}"
            );
        }
    }
    #[test]
    fn organic_bends_are_smooth_and_bridge_anchor_follows_them() {
        let skeleton = [[10., 10.], [10., 11.], [10.866025, 11.5], [11.73205, 11.0]];
        let path = organic_path(&skeleton);
        for p in path.windows(3) {
            let a = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
            let b = [p[2][0] - p[1][0], p[2][1] - p[1][1]];
            let cosine = (a[0] * b[0] + a[1] * b[1]) / (a[0].hypot(a[1]) * b[0].hypot(b[1]));
            assert!(
                cosine > 0.96,
                "visible centerline must have broad bends: {cosine}"
            );
        }
        let anchor = nearest_anchor(&[path.clone()], [10., 11.]).unwrap();
        assert!(
            (anchor.0[0] - 10.).hypot(anchor.0[1] - 11.) > 0.08,
            "bridge anchor must follow rounded river, not obsolete hex corner"
        );
        assert!((anchor.1[0].hypot(anchor.1[1]) - 1.).abs() < 1e-5);
        let w = world();
        let g = geometry(&w);
        let mut vertices = vec![];
        river_surface(&mut vertices, &g, &path);
        assert!(ribbon_height(&vertices, anchor.0[0], anchor.0[1]).is_finite());
        for pair in path.windows(2) {
            let a = ribbon_height(&vertices, pair[0][0], pair[0][1]);
            let b = ribbon_height(&vertices, pair[1][0], pair[1][1]);
            let distance = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
            assert!(
                (b - a).abs() <= distance * 0.121 + 0.0001,
                "water profile must not repeat stepped hex depressions"
            );
        }
        for v in vertices {
            assert!(v.pos.iter().all(|x| x.is_finite()));
        }
    }
    #[test]
    fn branching_paths_share_an_unshifted_junction() {
        let mut w = world();
        let a = 3 * 32 + 3;
        w.river_edges = vec![(a, a + 1), (a, a + 32), (a + 1, a + 32)];
        let paths = river_paths(&w);
        assert_eq!(paths.len(), 3);
        let smoothed: Vec<_> = paths.iter().map(|p| organic_path(p)).collect();
        for (a, b) in paths.iter().zip(smoothed) {
            assert_eq!(a.first(), b.first());
            assert_eq!(a.last(), b.last());
        }
    }
    #[test]
    fn unknown_river_and_props_generate_no_geometry() {
        let mut w = world();
        w.river_edges = vec![(3 * 32 + 3, 3 * 32 + 4)];
        w.visible.fill(0);
        w.explored.fill(0);
        assert!(river_paths(&w).is_empty());
        assert!(build(&w, &geometry(&w)).is_empty());
    }
    #[test]
    fn remembered_river_is_dimmed_and_hidden_neighbour_is_not_used() {
        let mut w = world();
        let a = 3 * 32 + 3;
        w.river_edges = vec![(a, a + 1)];
        w.visible.fill(0);
        let (x, z) = tile_to_world(3, 3);
        let mut out = Vec::new();
        tri(
            &mut out,
            [[x, 0., z], [x + 0.1, 0., z], [x, 0., z + 0.1]],
            [1.; 3],
        );
        let out = apply_visibility(&w, out);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|v| v.fog == 0.45));
        w.explored[(a + 1) as usize] = 0;
        assert!(river_paths(&w).is_empty());
    }
}
