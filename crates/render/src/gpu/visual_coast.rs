//! Low-poly coastal geology. One batched mesh, anchored to real land/water edges.
use crate::{
    gpu::feature::FeatureVertex,
    scene::{
        fog::fog_at,
        layout::{tile_to_world, EDGE_NEIGHBORS},
        picking::world_to_tile,
        terrain::{
            corner_lattice,
            evaluator::sample_surface,
            fields::{hash_u32, TerrainGeometry},
            lattice_to_world,
        },
    },
    world::state::RenderWorld,
};

#[derive(Clone, Copy)]
struct Shore {
    a: [f32; 2],
    b: [f32; 2],
    sea: [f32; 2],
    seed: u32,
    fog: f32,
}
fn surface(g: &TerrainGeometry, p: [f32; 2]) -> f32 {
    sample_surface(g, p[0], p[1]).map_or(0.05, |s| s.y)
}
fn random(seed: u32) -> f32 {
    (hash_u32(seed) & 65535) as f32 / 65535.
}

fn shores(w: &RenderWorld) -> Vec<Shore> {
    let mut out = Vec::new();
    if w.width == 0 {
        return out;
    }
    for cell in &w.terrain {
        if w.terrain_kinds.is_water(cell.terrain) || fog_at(w, cell.tile) < 0.4 {
            continue;
        }
        let q = (cell.tile.0 % w.width) as i32;
        let r = (cell.tile.0 / w.width) as i32;
        let (x, z) = tile_to_world(q as u32, r as u32);
        for (i, &(dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
            let (nq, nr) = (q + dq, r + dr);
            if nq < 0 || nr < 0 || nq >= w.width as i32 || nr >= w.height as i32 {
                continue;
            }
            let other = (nr as u32 * w.width + nq as u32) as usize;
            let Some(water) = w.terrain.get(other) else {
                continue;
            };
            if !w.terrain_kinds.is_water(water.terrain) || fog_at(w, water.tile) < 0.4 {
                continue;
            }
            // Leave settlement approaches and cultivated shore open and legible.
            if w.cities.iter().any(|c| {
                let (cx, cz) = tile_to_world(c.tile.0 % w.width, c.tile.0 / w.width);
                (x - cx).hypot(z - cz) < 2.7
            }) {
                continue;
            }
            // Open river mouths retain a beach instead of a stone dam.
            if w.river_edges.iter().any(|&(a, b)| {
                (a == cell.tile.0 && b == water.tile.0) || (b == cell.tile.0 && a == water.tile.0)
            }) {
                continue;
            }
            let a = lattice_to_world(corner_lattice(q, r, i));
            let b = lattice_to_world(corner_lattice(q, r, (i + 1) % 6));
            let (sx, sz) = tile_to_world(nq as u32, nr as u32);
            let length = (sx - x).hypot(sz - z);
            out.push(Shore {
                a: [a.0, a.1],
                b: [b.0, b.1],
                sea: [(sx - x) / length, (sz - z) / length],
                seed: hash_u32(cell.tile.0 ^ ((i as u32 + 1) * 773)),
                fog: fog_at(w, cell.tile).min(fog_at(w, water.tile)),
            });
        }
    }
    out
}

struct Mesh<'a> {
    world: &'a RenderWorld,
    vertices: Vec<FeatureVertex>,
}
impl Mesh<'_> {
    fn tri(&mut self, mut p: [[f32; 3]; 3], color: [f32; 3], mut fog: f32, outward: [f32; 3]) {
        // Features must not reveal an unknown third tile around a shared corner.
        for v in p {
            let Some(tile) = world_to_tile(v[0], v[2], (self.world.width, self.world.height))
            else {
                return;
            };
            fog = fog.min(fog_at(self.world, tile));
            if fog < 0.4 {
                return;
            }
        }
        let a = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
        let b = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
        let mut n = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length < 1e-7 {
            return;
        }
        if n[0] * outward[0] + n[1] * outward[1] + n[2] * outward[2] < 0. {
            p.swap(1, 2);
            n = n.map(|v| -v);
        }
        n = n.map(|v| v / length);
        for pos in p {
            self.vertices.push(FeatureVertex {
                pos,
                normal: n,
                color,
                fog,
            });
        }
    }
}

fn cliff(mesh: &mut Mesh, g: &TerrainGeometry, s: Shore) {
    let center = [(s.a[0] + s.b[0]) * 0.5, (s.a[1] + s.b[1]) * 0.5];
    let edge = [s.b[0] - s.a[0], s.b[1] - s.a[1]];
    let length = edge[0].hypot(edge[1]);
    let tangent = [edge[0] / length, edge[1] / length];
    let land = [center[0] - s.sea[0] * 0.23, center[1] - s.sea[1] * 0.23];
    let toe = [center[0] + s.sea[0] * 0.25, center[1] + s.sea[1] * 0.25];
    // A geological ledge descends from the ground into water. It does not
    // erect a new pointed mountain on top of every shoreline cell.
    let base = surface(g, toe) - 0.085;
    let height = (surface(g, land) + 0.14 - base).clamp(0.32, 0.56);
    let height_scale = height / 0.433;
    let mirror = if s.seed & 1 == 0 { 1. } else { -1. };
    let width_scale = length * (0.74 + random(s.seed ^ 42) * 0.21) / 1.16;
    let tint = 0.96 + random(s.seed ^ 917) * 0.06;
    for face in crate::models::VISUAL_COASTAL_CLIFF.chunks_exact(3) {
        let points = std::array::from_fn(|i| {
            let p = face[i].pos;
            let along = p[0] * width_scale * mirror;
            let across = p[2] * 1.08 + 0.025;
            [
                center[0] + tangent[0] * along + s.sea[0] * across,
                base + p[1] * height_scale,
                center[1] + tangent[1] * along + s.sea[1] * across,
            ]
        });
        let normal = face[0].normal;
        let outward = [
            tangent[0] * normal[0] * mirror + s.sea[0] * normal[2],
            normal[1],
            tangent[1] * normal[0] * mirror + s.sea[1] * normal[2],
        ];
        mesh.tri(points, face[0].color.map(|v| v * tint), s.fog, outward);
    }
}

fn boulder(
    mesh: &mut Mesh,
    g: &TerrainGeometry,
    s: Shore,
    t: f32,
    offset: f32,
    scale: f32,
    seed: u32,
) {
    let center = [
        s.a[0] + (s.b[0] - s.a[0]) * t + s.sea[0] * offset,
        s.a[1] + (s.b[1] - s.a[1]) * t + s.sea[1] * offset,
    ];
    let angle = random(seed) * std::f32::consts::TAU;
    let vertices = crate::gpu::visual_props::rock_mesh(
        g,
        center,
        angle,
        scale * 1.75,
        (seed % 3) as usize,
        s.fog,
    );
    for face in vertices.chunks_exact(3) {
        mesh.tri(
            [face[0].pos, face[1].pos, face[2].pos],
            face[0].color,
            s.fog,
            face[0].normal,
        );
    }
}

pub fn build(w: &RenderWorld, g: &TerrainGeometry) -> Vec<FeatureVertex> {
    let mut mesh = Mesh {
        world: w,
        vertices: Vec::new(),
    };
    for s in shores(w) {
        let x = (s.a[0] + s.b[0]) * 0.5;
        let z = (s.a[1] + s.b[1]) * 0.5;
        // Broad patches of geology leave intervening coves as the existing sand beach.
        let rockiness = (x * 0.64 + 0.3).sin() + (z * 0.73 - 0.4).cos();
        if rockiness > 0.18 {
            cliff(&mut mesh, g, s);
            if s.seed % 3 == 0 {
                boulder(&mut mesh, g, s, 0.34, 0.27, 0.15, s.seed ^ 872);
            }
        } else if s.seed % 4 == 0 {
            boulder(
                &mut mesh,
                g,
                s,
                0.58,
                0.08,
                0.16 + random(s.seed) * 0.07,
                s.seed,
            );
        }
    }
    mesh.vertices
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
    use kernel::ids::TileId;
    fn fixture() -> (RenderWorld, TerrainGeometry) {
        let mut w = RenderWorld {
            width: 12,
            height: 12,
            visible: vec![1; 144],
            explored: vec![1; 144],
            ..Default::default()
        };
        for r in 0..12 {
            for q in 0..12 {
                w.terrain.push(TerrainCell {
                    tile: TileId(r * 12 + q),
                    terrain: if q >= 6 { 4 } else { 1 },
                    owner: None,
                    resource: None,
                });
            }
        }
        let style = compile(default_style()).unwrap();
        let hf = build_heightfield(&w, &style);
        let g = build_terrain_geometry(&w, &style, &hf);
        (w, g)
    }
    #[test]
    fn shore_vertices_are_finite_grounded_and_close_to_real_water_edges() {
        let (w, g) = fixture();
        let edges = shores(&w);
        let vertices = build(&w, &g);
        assert!(!vertices.is_empty());
        assert!(
            vertices.len() <= edges.len() * (170 + 120) * 3,
            "at most one 170-triangle cliff and one 120-triangle rock cluster per edge"
        );
        for v in &vertices {
            assert!(v.pos.iter().chain(v.normal.iter()).all(|v| v.is_finite()));
            let norm = v.normal.iter().map(|v| v * v).sum::<f32>().sqrt();
            assert!((norm - 1.).abs() < 0.0001);
            let near = edges.iter().any(|s| {
                let d = [s.b[0] - s.a[0], s.b[1] - s.a[1]];
                let t = (((v.pos[0] - s.a[0]) * d[0] + (v.pos[2] - s.a[1]) * d[1])
                    / (d[0] * d[0] + d[1] * d[1]))
                    .clamp(0., 1.);
                (v.pos[0] - s.a[0] - t * d[0]).hypot(v.pos[2] - s.a[1] - t * d[1]) < 0.7
            });
            assert!(near, "cliff escaped shoreline corridor");
        }
        assert!(
            vertices
                .iter()
                .any(|v| v.pos[1] < surface(&g, [v.pos[0], v.pos[2]])),
            "rock feet intersect terrain rather than float"
        );
        let again = build(&w, &g);
        assert_eq!(
            bytemuck::cast_slice::<FeatureVertex, u8>(&vertices),
            bytemuck::cast_slice::<FeatureVertex, u8>(&again)
        );
    }
    #[test]
    fn unknown_shore_is_empty_and_remembered_shore_keeps_cartography_factor() {
        let (mut w, g) = fixture();
        w.visible.fill(0);
        w.explored.fill(0);
        assert!(build(&w, &g).is_empty());
        // Unknown sanitized land beside known sea must not reveal a fake coastline.
        for c in &w.terrain {
            if c.terrain == 4 {
                w.visible[c.tile.0 as usize] = 1;
                w.explored[c.tile.0 as usize] = 1;
            }
        }
        assert!(build(&w, &g).is_empty());
        w.visible.fill(0);
        w.explored.fill(1);
        let out = build(&w, &g);
        assert!(!out.is_empty());
        assert!(out.iter().all(|v| v.fog == 0.45));
    }
    #[test]
    fn river_mouth_has_no_cliff_and_ledge_stays_below_half_tile_height() {
        let (mut w, g) = fixture();
        let before = shores(&w).len();
        w.river_edges.push((65, 66)); // Adjacent land and sea at the river mouth.
        assert_eq!(shores(&w).len(), before - 1);
        for shore in shores(&w) {
            let mut mesh = Mesh {
                world: &w,
                vertices: Vec::new(),
            };
            cliff(&mut mesh, &g, shore);
            if mesh.vertices.is_empty() {
                continue;
            }
            let low = mesh
                .vertices
                .iter()
                .map(|v| v.pos[1])
                .fold(f32::INFINITY, f32::min);
            let high = mesh
                .vertices
                .iter()
                .map(|v| v.pos[1])
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(high - low <= 0.57, "shore cliff grew into a mountain");
            assert!(
                mesh.vertices
                    .iter()
                    .any(|v| v.pos[1] < surface(&g, [v.pos[0], v.pos[2]])),
                "each ledge must intersect the shore terrain"
            );
        }
    }
    #[test]
    fn inland_tiles_do_not_produce_coastal_rocks() {
        let (mut w, g) = fixture();
        for c in &mut w.terrain {
            c.terrain = 1;
        }
        assert!(build(&w, &g).is_empty());
    }
}
