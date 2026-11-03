//! Sea stacks and boulders of the cliffs, in the feature batch.
//!
//! Placement comes from the cliff data (`surface/stacks.rs`); this adapter
//! only instances the authored fractured-rock meshes and grounds them with
//! the final mesh query, so a rock never floats over or hides under the
//! surface it stands on.
use super::feature::FeatureVertex;
use crate::{
    models::{ROCK_MESHES, SEA_STACK_MESHES},
    scene::{
        fog::fog_at_position,
        terrain::surface::{CoastProp, CoastPropKind, SurfaceMesh},
    },
    world::state::RenderWorld,
};
use glam::Vec3;

const SEA_LEVEL: f32 = 0.08;
/// The authored rocks are normalized to this footprint radius (platform/tools/blender/visual_rocks.py).
const MESH_RADIUS: f32 = 0.485;
/// How deep the foot is set below the lowest ground under the footprint.
const EMBED: f32 = 0.03;

/// Coast rocks standing on the given tiles (a feature chunk's tiles: each rock is built
/// by the one chunk that holds its tile, not by every chunk).
pub fn build(world: &RenderWorld, surface: &SurfaceMesh, tiles: &[kernel::ids::TileId]) -> Vec<FeatureVertex> {
    let mut out = vec![];
    let map = (world.width, world.height);
    // Which tiles are asked for: one lookup per prop (a scan of `tiles` per
    // prop was quadratic on a full rebuild).
    let mut asked = vec![false; world.terrain.len()];
    for t in tiles {
        if let Some(slot) = asked.get_mut(t.0 as usize) {
            *slot = true;
        }
    }
    for prop in &surface.coast_props {
        let tile = crate::scene::picking::world_to_tile(prop.position[0], prop.position[1], map);
        if !tile.is_some_and(|t| asked.get(t.0 as usize) == Some(&true)) {
            continue;
        }
        let fog = fog_at_position(world, prop.position[0], prop.position[1]);
        if fog < 0.4 {
            continue;
        }
        let Some(base) = ground_under(surface, prop) else {
            continue;
        };
        let start = out.len();
        append_rock(&mut out, prop, base, fog);
        // Surf is live water: the feature shader fades it out of the visible area.
        let rock = out[start..].to_vec();
        append_surf(&mut out, prop, &rock);
    }
    out
}

/// Broken surf around the waterline of a rock, like the wash at the foot of
/// the cliffs: per angular sector the ring starts where the rock's faces cross
/// the sea level, so it hugs the rock instead of drawing a circle. Unlit and
/// translucent: the feature shader reads a negative fog as the alpha of a
/// flat color.
fn append_surf(out: &mut Vec<FeatureVertex>, prop: &CoastProp, rock: &[FeatureVertex]) {
    const SECTORS: usize = 18;
    let center = Vec3::new(prop.position[0], SEA_LEVEL, prop.position[1]);
    let sector_of = |p: Vec3| {
        let angle = (p.z - center.z).atan2(p.x - center.x).rem_euclid(std::f32::consts::TAU);
        ((angle / std::f32::consts::TAU * SECTORS as f32) as usize).min(SECTORS - 1)
    };
    let mut reach = [0f32; SECTORS];
    for face in rock.chunks_exact(3) {
        for i in 0..3 {
            let (a, b) = (Vec3::from(face[i].pos), Vec3::from(face[(i + 1) % 3].pos));
            if (a.y - SEA_LEVEL) * (b.y - SEA_LEVEL) > 0. || (a.y - b.y).abs() < 1e-6 {
                continue;
            }
            let p = a.lerp(b, (SEA_LEVEL - a.y) / (b.y - a.y));
            let sector = sector_of(p);
            reach[sector] = reach[sector].max((p - center).length());
        }
    }
    let seed = prop.variant.wrapping_mul(0x85eb_ca6b);
    let color = [0.80, 0.93, 0.91];
    let vertex = |angle: f32, radius: f32, alpha: f32| FeatureVertex {
        pos: [center.x + radius * angle.cos(), SEA_LEVEL + 0.004, center.z + radius * angle.sin()],
        normal: [0., 1., 0.],
        color,
        fog: -alpha,
    };
    for i in 0..SECTORS {
        let next = (i + 1) % SECTORS;
        if reach[i] == 0. || reach[next] == 0. || crate::scene::terrain::fields::hash_u32(seed ^ i as u32) % 3 == 0 {
            continue;
        }
        let a = (i as f32 + 0.5) / SECTORS as f32 * std::f32::consts::TAU;
        let b = (next as f32 + 0.5) / SECTORS as f32 * std::f32::consts::TAU;
        let width = 0.025 + 0.02 * (crate::scene::terrain::fields::hash_u32(seed ^ 0x51 ^ i as u32) & 255) as f32 / 255.;
        let [ai, bi] = [vertex(a, reach[i], 0.8), vertex(b, reach[next], 0.8)];
        let [ao, bo] = [vertex(a, reach[i] + width, 0.0), vertex(b, reach[next] + width, 0.0)];
        out.extend([ai, ao, bo, ai, bo, bi]);
    }
}

/// Lowest final-mesh ground under the footprint, or `None` where the rock
/// would stand outside the mesh or a stack would stand on dry land.
fn ground_under(surface: &SurfaceMesh, prop: &CoastProp) -> Option<f32> {
    let mut low = f32::INFINITY;
    for [x, z] in [[0., 0.], [1., 0.], [-1., 0.], [0., 1.], [0., -1.]] {
        let p = [prop.position[0] + x * prop.radius, prop.position[1] + z * prop.radius];
        let ground = surface.sample_ground(p[0], p[1])?.y;
        if prop.kind == CoastPropKind::Stack && surface.sample_water(p[0], p[1]).is_none() {
            return None;
        }
        low = low.min(ground);
    }
    Some(low - EMBED)
}

/// One rock: the authored mesh turned, scaled to the footprint and stretched
/// from the ground to its top above the sea. Faces keep flat normals, taken
/// again after the stretch.
fn append_rock(out: &mut Vec<FeatureVertex>, prop: &CoastProp, base: f32, fog: f32) {
    // Stacks are the sculpted sea-stack models, boulders the fractured rocks.
    let mesh = match prop.kind {
        CoastPropKind::Stack => SEA_STACK_MESHES[prop.variant as usize % SEA_STACK_MESHES.len()],
        CoastPropKind::Boulder => ROCK_MESHES[prop.variant as usize % ROCK_MESHES.len()],
    };
    let mesh_height = mesh.iter().map(|v| v.pos[1]).fold(0f32, f32::max).max(1e-3);
    let horizontal = prop.radius / MESH_RADIUS;
    let vertical = (SEA_LEVEL + prop.height - base).max(0.02) / mesh_height;
    let (sin, cos) = prop.angle.sin_cos();
    let place = |p: [f32; 3]| {
        let (x, z) = (p[0] * horizontal, p[2] * horizontal);
        Vec3::new(prop.position[0] + x * cos - z * sin, base + p[1] * vertical, prop.position[1] + x * sin + z * cos)
    };
    for face in mesh.chunks_exact(3) {
        let points = [place(face[0].pos), place(face[1].pos), place(face[2].pos)];
        let mut normal = (points[1] - points[0]).cross(points[2] - points[0]).normalize_or_zero();
        // Keep the authored outward side.
        let authored = Vec3::from(face[0].normal);
        let turned = Vec3::new(authored.x * cos - authored.z * sin, authored.y, authored.x * sin + authored.z * cos);
        if normal.dot(turned) < 0. {
            normal = -normal;
        }
        for (point, vertex) in points.iter().zip(face) {
            out.push(FeatureVertex {
                pos: point.to_array(),
                normal: normal.to_array(),
                color: match prop.kind {
                    CoastPropKind::Stack => stack_color(vertex.color, point.y),
                    CoastPropKind::Boulder => rock_color(vertex.color, point.y),
                },
                fog,
            });
        }
    }
}

/// A sea stack keeps its authored turf cap and pale ledges; its rock takes
/// the cool slate of the cliff walls, dark and wet in the swash.
fn stack_color(authored: [f32; 3], y: f32) -> [f32; 3] {
    let wet = 1. - ((y - SEA_LEVEL) / 0.05).clamp(0., 1.);
    if authored[1] > authored[0] * 1.15 {
        return authored;
    }
    let slate = [0.36, 0.39, 0.45];
    std::array::from_fn(|k| (authored[k] * 0.55 + slate[k] * 0.45) * (1. - 0.4 * wet))
}

/// The authored stone, cooled toward the cliff walls and dark and wet where
/// the swash reaches it.
fn rock_color(authored: [f32; 3], y: f32) -> [f32; 3] {
    let wall = [0.52, 0.53, 0.56];
    let wet = 1. - ((y - SEA_LEVEL) / 0.04).clamp(0., 1.);
    std::array::from_fn(|k| (authored[k] * 0.4 + wall[k] * 0.6) * (1. - 0.35 * wet))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        scene::{
            style::{compile, default_style},
            terrain::surface,
        },
        world::state::TerrainCell,
    };
    use kernel::ids::TileId;

    /// Sea in the east, a coast of hills in front of it: a rocky coast.
    fn world() -> RenderWorld {
        let (width, height) = (14, 12);
        RenderWorld {
            visual_surface: true,
            width,
            height,
            visible: vec![1; (width * height) as usize],
            explored: vec![1; (width * height) as usize],
            terrain: (0..width * height)
                .map(|i| {
                    let (q, r) = (i % width, i / width);
                    let terrain = if r == 6 && (8..=9).contains(&q) { 2 } else if q >= 8 { 4 } else if q == 7 { 2 } else { 1 };
                    TerrainCell { tile: TileId(i), terrain, owner: None, resource: None }
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn coast_rocks_are_grounded_on_the_final_mesh_and_rise_out_of_the_sea() {
        let w = world();
        let mesh = surface::build(&w, &compile(default_style()).unwrap());
        assert!(mesh.coast_props.iter().any(|p| p.kind == CoastPropKind::Stack), "the hill peninsula has a stack");
        let rocks = build(&w, &mesh, &every_tile(&w));
        assert!(!rocks.is_empty());
        assert_eq!(rocks.len() % 3, 0);
        for prop in &mesh.coast_props {
            let Some(base) = ground_under(&mesh, prop) else { continue };
            let ground = mesh.sample_ground(prop.position[0], prop.position[1]).unwrap().y;
            assert!(base < ground, "the foot is set into the ground, not above it");
            assert!(ground - base < 0.6, "grounded near the surface under it");
        }
        let top = rocks.iter().map(|v| v.pos[1]).fold(f32::MIN, f32::max);
        assert!(top > SEA_LEVEL + 0.2, "stacks rise well out of the sea: {top}");
        assert!(rocks.iter().all(|v| v.pos.iter().chain(&v.normal).all(|c| c.is_finite())));
        let again = build(&w, &mesh, &every_tile(&w));
        assert_eq!(bytemuck::cast_slice::<FeatureVertex, u8>(&rocks), bytemuck::cast_slice::<FeatureVertex, u8>(&again));
    }

    #[test]
    fn island_rock_budget() {
        let w = crate::scene::terrain::surface::fixtures::island();
        let mesh = surface::build(&w, &compile(default_style()).unwrap());
        let rocks = build(&w, &mesh, &every_tile(&w));
        assert!(mesh.coast_props.iter().any(|p| p.kind == CoastPropKind::Stack));
        assert!(rocks.len() / 3 < 20_000, "coast rocks stay a small share of the feature batch");
    }

    fn every_tile(w: &RenderWorld) -> Vec<TileId> {
        (0..w.width * w.height).map(TileId).collect()
    }

    #[test]
    fn each_rock_belongs_to_one_chunk() {
        let w = world();
        let mesh = surface::build(&w, &compile(default_style()).unwrap());
        let all = build(&w, &mesh, &every_tile(&w)).len();
        let halves: usize = [0..w.width * w.height / 2, w.width * w.height / 2..w.width * w.height]
            .into_iter()
            .map(|range| build(&w, &mesh, &range.map(TileId).collect::<Vec<_>>()).len())
            .sum();
        assert!(all > 0);
        assert_eq!(halves, all, "split chunks build every rock exactly once");
    }

    #[test]
    fn hidden_coast_has_no_rocks() {
        let mut w = world();
        w.visible.fill(0);
        w.explored.fill(0);
        let mesh = surface::build(&w, &compile(default_style()).unwrap());
        assert!(build(&w, &mesh, &every_tile(&w)).is_empty());
    }
}
