//! Faceted masonry and boat silhouettes for the coastal art scene.
use super::height;
use crate::{gpu::feature::FeatureVertex, scene::terrain::fields::TerrainGeometry};

type Point = [f32; 3];
struct Mesh {
    vertices: Vec<FeatureVertex>,
}
impl Mesh {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
        }
    }
    fn tri(&mut self, mut p: [Point; 3], color: Point, outward: Point) {
        let a = sub(p[1], p[0]);
        let b = sub(p[2], p[0]);
        let mut n = cross(a, b);
        let length = dot(n, n).sqrt();
        if length < 1e-8 {
            return;
        }
        if dot(n, outward) < 0. {
            p.swap(1, 2);
            n = n.map(|v| -v);
        }
        n = n.map(|v| v / length);
        for pos in p {
            self.vertices.push(FeatureVertex {
                pos,
                normal: n,
                color,
                fog: 1.,
            });
        }
    }
    fn quad(&mut self, p: [Point; 4], color: Point, outward: Point) {
        self.tri([p[0], p[1], p[2]], color, outward);
        self.tri([p[0], p[2], p[3]], color, outward);
    }
    fn tapered_block(
        &mut self,
        center: [f32; 2],
        bottom: f32,
        top: f32,
        foot: [f32; 2],
        cap: [f32; 2],
        color: Point,
    ) {
        let ring = |size: [f32; 2], y: f32| {
            [
                [center[0] - size[0], y, center[1] - size[1]],
                [center[0] + size[0], y, center[1] - size[1]],
                [center[0] + size[0], y, center[1] + size[1]],
                [center[0] - size[0], y, center[1] + size[1]],
            ]
        };
        let a = ring(foot, bottom);
        let b = ring(cap, top);
        for i in 0..4 {
            let j = (i + 1) % 4;
            self.quad(
                [a[i], a[j], b[j], b[i]],
                color,
                [
                    a[i][0] + a[j][0] - 2. * center[0],
                    0.,
                    a[i][2] + a[j][2] - 2. * center[1],
                ],
            );
        }
        self.quad(b, color, [0., 1., 0.]);
        self.quad(a, color, [0., -1., 0.]);
    }
    fn block(&mut self, c: Point, s: Point, color: Point) {
        self.tapered_block(
            [c[0], c[2]],
            c[1],
            c[1] + s[1],
            [s[0], s[2]],
            [s[0], s[2]],
            color,
        );
    }
    fn beam(&mut self, a: Point, b: Point, radius: f32, color: Point, sides: usize) {
        let axis = sub(b, a);
        let length = dot(axis, axis).sqrt();
        if length < 1e-6 {
            return;
        }
        let axis = axis.map(|v| v / length);
        let basis = if axis[1].abs() < 0.9 {
            [0., 1., 0.]
        } else {
            [1., 0., 0.]
        };
        let mut u = cross(axis, basis);
        let norm = dot(u, u).sqrt();
        u = u.map(|v| v / norm);
        let v = cross(axis, u);
        let point = |p: Point, i: usize| {
            let theta = i as f32 * std::f32::consts::TAU / sides as f32;
            let d = add(mul(u, theta.cos() * radius), mul(v, theta.sin() * radius));
            add(p, d)
        };
        for i in 0..sides {
            let j = (i + 1) % sides;
            let p = point(a, i);
            self.quad([p, point(a, j), point(b, j), point(b, i)], color, sub(p, a));
        }
    }
}
fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: Point, s: f32) -> Point {
    a.map(|v| v * s)
}
fn dot(a: Point, b: Point) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(super) fn bridge(g: &TerrainGeometry, bx: f32, bz: f32, water_y: f32) -> Vec<FeatureVertex> {
    let base = (height(g, bx, bz) + 0.018 - 0.105).max(water_y + 0.015);
    crate::models::VISUAL_BRIDGE
        .iter()
        .map(|v| FeatureVertex {
            pos: [bx + v.pos[0], base + v.pos[1], bz + v.pos[2]],
            normal: v.normal,
            color: v.color,
            fog: 1.,
        })
        .collect()
}

fn sail_point(u: f32, v: f32) -> Point {
    let width = 0.52 + 0.105 * (std::f32::consts::PI * v).sin();
    [
        u * width,
        0.72 + v * 1.12,
        -0.09 + 0.27 * (std::f32::consts::PI * v).sin() * (1. - u * u) + 0.025 * (1. - v),
    ]
}
fn ship_hull(mesh: &mut Mesh) {
    let outline: [[f32; 2]; 11] = [
        [-0.20, -0.88],
        [0.20, -0.88],
        [0.35, -0.58],
        [0.44, -0.10],
        [0.40, 0.34],
        [0.25, 0.73],
        [0.0, 1.10],
        [-0.25, 0.73],
        [-0.40, 0.34],
        [-0.44, -0.10],
        [-0.35, -0.58],
    ];
    let ring = |level: usize| {
        outline.map(|p| {
            let width = [0.40, 0.75, 1.0, 1.02][level];
            let y = [-0.09, 0.095, 0.32, 0.43][level];
            let rise = (p[1].abs() - 0.38).max(0.) * 0.15;
            [p[0] * width, y + rise, p[1] * (0.92 + level as f32 * 0.025)]
        })
    };
    for level in 0..3 {
        let a = ring(level);
        let b = ring(level + 1);
        for i in 0..outline.len() {
            let j = (i + 1) % outline.len();
            let tint = 1. - (i % 3) as f32 * 0.045;
            let color = if level == 1 {
                [0.46 * tint, 0.29 * tint, 0.15 * tint]
            } else {
                [0.33 * tint, 0.20 * tint, 0.105 * tint]
            };
            mesh.quad(
                [a[i], a[j], b[j], b[i]],
                color,
                [outline[i][0], 0., outline[i][1]],
            );
        }
    }
    let rim = ring(3);
    for i in 0..outline.len() {
        let j = (i + 1) % outline.len();
        let a = rim[i];
        let b = rim[j];
        let inside = |p: Point| [p[0] * 0.85, p[1], p[2] * 0.96];
        mesh.quad(
            [a, b, inside(b), inside(a)],
            [0.69, 0.44, 0.20],
            [0., 1., 0.],
        );
        mesh.beam(
            add(a, [0., 0.025, 0.]),
            add(b, [0., 0.025, 0.]),
            0.022,
            [0.57, 0.34, 0.15],
            4,
        );
        let da = [a[0] * 0.84, 0.355, a[2] * 0.93];
        let db = [b[0] * 0.84, 0.355, b[2] * 0.93];
        mesh.tri([[0., 0.355, 0.], da, db], [0.52, 0.35, 0.19], [0., 1., 0.]);
        mesh.quad(
            [inside(a), inside(b), db, da],
            [0.43, 0.27, 0.13],
            [-a[0], 0., -a[2]],
        );
        if i % 2 == 0 {
            mesh.beam(
                add(a, [0., 0.0, 0.]),
                add(a, [0., 0.12, 0.]),
                0.012,
                [0.59, 0.37, 0.17],
                4,
            );
        }
    }
    // Stern platform, small hatch and plank inlays are visible around the mast.
    mesh.block([0., 0.36, -0.57], [0.25, 0.13, 0.23], [0.42, 0.27, 0.15]);
    mesh.block([0., 0.49, -0.57], [0.27, 0.025, 0.245], [0.63, 0.43, 0.23]);
    mesh.block([0., 0.365, 0.39], [0.14, 0.027, 0.17], [0.30, 0.21, 0.13]);
    for i in 0..5 {
        mesh.block(
            [-0.105 + i as f32 * 0.052, 0.393, 0.39],
            [0.009, 0.007, 0.14],
            [0.60, 0.41, 0.22],
        );
    }
    mesh.beam(
        [0., 0.07, 1.04],
        [0., 0.71, 1.15],
        0.04,
        [0.53, 0.30, 0.12],
        5,
    );
}
pub(super) fn ship(sx: f32, sz: f32) -> Vec<FeatureVertex> {
    let mut mesh = Mesh::new();
    ship_hull(&mut mesh);
    mesh.beam(
        [0., 0.36, -0.10],
        [0., 2.08, -0.10],
        0.027,
        [0.38, 0.24, 0.12],
        6,
    );
    mesh.beam(
        [-0.67, 1.865, -0.09],
        [0.67, 1.865, -0.09],
        0.018,
        [0.46, 0.29, 0.13],
        5,
    );
    for y in 0..8 {
        for x in 0..8 {
            let u = -1. + x as f32 * 0.25;
            let v = y as f32 * 0.125;
            let blue =
                ((x == 3 || x == 4) && (2..=5).contains(&y)) || (y == 4 && (1..=6).contains(&x));
            let color = if blue {
                [0.15, 0.35, 0.62]
            } else if (x + y) % 3 == 0 {
                [0.88, 0.83, 0.69]
            } else {
                [0.96, 0.91, 0.77]
            };
            mesh.quad(
                [
                    sail_point(u, v),
                    sail_point(u + 0.25, v),
                    sail_point(u + 0.25, v + 0.125),
                    sail_point(u, v + 0.125),
                ],
                color,
                [0., 0., 1.],
            );
        }
    }
    for end in [
        [0., 0.65, 1.13],
        [0., 0.55, -0.89],
        [-0.37, 0.51, -0.18],
        [0.37, 0.51, -0.18],
    ] {
        mesh.beam([0., 1.98, -0.10], end, 0.008, [0.36, 0.29, 0.17], 4);
    }
    mesh.quad(
        [
            [0., 2.03, -0.1],
            [0.32, 2.0, -0.08],
            [0.25, 1.90, -0.08],
            [0., 1.91, -0.1],
        ],
        [0.15, 0.36, 0.66],
        [0., 0., 1.],
    );
    for v in &mut mesh.vertices {
        v.pos[0] += sx;
        v.pos[1] += 0.105;
        v.pos[2] += sz;
    }
    mesh.vertices
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vessel_has_curved_sail_real_depth_and_finite_unit_normals() {
        let out = ship(0., 0.);
        assert!(out.len() < 2500 * 3);
        for v in &out {
            assert!(v.pos.iter().chain(v.normal.iter()).all(|v| v.is_finite()));
            assert!((dot(v.normal, v.normal) - 1.).abs() < 0.0001);
        }
        assert!(
            sail_point(0., 0.5)[2] - sail_point(1., 0.5)[2] > 0.2,
            "sail must billow, not be a flat card"
        );
        assert!(
            out.iter().any(|v| v.pos[1] < 0.10),
            "keel extends below waterline"
        );
        assert!(
            out.iter().any(|v| v.pos[2] > 1.1),
            "raised narrow bow stem exists"
        );
        assert!(
            out.iter().filter(|v| v.normal[1].abs() < 0.5).count() > 100,
            "side lighting must use actual face normals"
        );
    }
    #[test]
    fn bridge_has_empty_water_arch_under_masonry() {
        use crate::{
            scene::{
                heightfield::build_heightfield,
                style::{compile, default_style},
                terrain::fields::build_terrain_geometry,
            },
            world::state::{RenderWorld, TerrainCell},
        };
        use kernel::ids::TileId;
        let w = RenderWorld {
            width: 8,
            height: 8,
            visible: vec![1; 64],
            explored: vec![1; 64],
            terrain: (0..64)
                .map(|i| TerrainCell {
                    tile: TileId(i),
                    terrain: 1,
                    owner: None,
                    resource: None,
                })
                .collect(),
            ..Default::default()
        };
        let style = compile(default_style()).unwrap();
        let hf = build_heightfield(&w, &style);
        let g = build_terrain_geometry(&w, &style, &hf);
        let (x, z) = (10., 6.);
        let water_y = height(&g, x, z);
        let base = water_y + 0.015;
        let out = bridge(&g, x, z, water_y);
        let covers = |p: [f32; 2]| {
            out.chunks_exact(3).any(|t| {
                let a = [t[0].pos[0], t[0].pos[1]];
                let b = [t[1].pos[0], t[1].pos[1]];
                let c = [t[2].pos[0], t[2].pos[1]];
                let denom = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                if denom.abs() < 1e-7 {
                    return false;
                }
                let u = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / denom;
                let v = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / denom;
                u >= 0. && v >= 0. && u + v <= 1.
            })
        };
        assert!(
            !covers([x, base + 0.035]),
            "river opening must not be a painted/slab arch"
        );
        assert!(
            covers([x, base + 0.10]),
            "masonry ring remains above opening"
        );
        let raised_water = base + 0.6;
        let raised = bridge(&g, x, z, raised_water);
        for v in raised.iter().filter(|v| (v.pos[0] - x).abs() < 0.07) {
            assert!(
                v.pos[1] > raised_water + 0.065,
                "arch clearance follows elevated river ribbon"
            );
        }
        for v in out {
            // Adjacent centres are ±sqrt(3)/2 from this shared edge.
            assert!((v.pos[0] - x).abs() < 0.44);
            for centre in [-0.8660254, 0.8660254] {
                let dx = v.pos[0] - x - centre;
                let dz = v.pos[2] - z;
                assert!(dx * dx + dz * dz > 0.4 * 0.4);
            }
            assert!((dot(v.normal, v.normal) - 1.).abs() < 0.0001);
        }
    }
}
