//! Soft ground-contact artwork for the fixed visual sandbox.
//! Negative `fog` carries opacity to the alpha-blended feature shader.
//! These surface-draped shapes are intentionally not a shadow-map solution.
use crate::gpu::feature::{tree_layout, FeatureVertex};
use crate::scene::fog::fog_at;
use crate::scene::layout::tile_to_world;
use crate::scene::terrain::evaluator::sample_surface;
use crate::scene::terrain::fields::TerrainGeometry;
use crate::world::state::RenderWorld;

const SEGMENTS: usize = 18;
const SHADOW_COLOR: [f32; 3] = [0.10, 0.17, 0.23];
// Ground projection away from the fixed sun direction [-0.42, 0.80, 0.35].
const DIRECTION: [f32; 2] = [0.768_221_3, -0.640_184_4];

struct ShadowShape {
    center: [f32; 2],
    length: f32,
    width: f32,
    opacity: f32,
    /// Taper makes a conifer's shadow narrow toward its crown tip.
    taper: f32,
    phase: f32,
}

fn vertex(geom: &TerrainGeometry, p: [f32; 2], opacity: f32) -> Option<FeatureVertex> {
    let surface = sample_surface(geom, p[0], p[1])?;
    Some(FeatureVertex {
        pos: [p[0], surface.y + 0.012, p[1]],
        normal: [0.0, 1.0, 0.0],
        color: SHADOW_COLOR,
        fog: -opacity.max(0.001),
    })
}

fn ring_vertex(
    geom: &TerrainGeometry,
    shape: &ShadowShape,
    angle: f32,
    radius: f32,
    opacity: f32,
) -> Option<FeatureVertex> {
    let along = angle.cos();
    let irregular = 1.0 + 0.065 * (angle * 3.0 + shape.phase).sin();
    let cross = angle.sin() * (1.0 - shape.taper * along.max(0.0));
    let x = along * shape.length * radius * irregular;
    let z = cross * shape.width * radius * irregular;
    vertex(
        geom,
        [
            shape.center[0] + DIRECTION[0] * x - DIRECTION[1] * z,
            shape.center[1] + DIRECTION[1] * x + DIRECTION[0] * z,
        ],
        opacity,
    )
}

fn push_triangle(
    out: &mut Vec<FeatureVertex>,
    a: FeatureVertex,
    b: FeatureVertex,
    c: FeatureVertex,
) {
    // Skip any degenerate projected triangle, especially clipped map-edge props.
    let area = (b.pos[0] - a.pos[0]) * (c.pos[2] - a.pos[2])
        - (b.pos[2] - a.pos[2]) * (c.pos[0] - a.pos[0]);
    if area.abs() > 1e-8 {
        out.extend([a, b, c]);
    }
}

fn emit(out: &mut Vec<FeatureVertex>, geom: &TerrainGeometry, shape: ShadowShape) {
    let Some(center) = vertex(geom, shape.center, shape.opacity) else {
        return;
    };
    for i in 0..SEGMENTS {
        let a = i as f32 * std::f32::consts::TAU / SEGMENTS as f32;
        let b = (i + 1) as f32 * std::f32::consts::TAU / SEGMENTS as f32;
        let inner_a = ring_vertex(geom, &shape, a, 0.57, shape.opacity * 0.55);
        let inner_b = ring_vertex(geom, &shape, b, 0.57, shape.opacity * 0.55);
        let outer_a = ring_vertex(geom, &shape, a, 1.0, 0.001);
        let outer_b = ring_vertex(geom, &shape, b, 1.0, 0.001);
        if let (Some(ia), Some(ib), Some(oa), Some(ob)) = (inner_a, inner_b, outer_a, outer_b) {
            push_triangle(out, center, ib, ia);
            push_triangle(out, ia, ib, ob);
            push_triangle(out, ia, ob, oa);
        }
    }
}

pub fn build(world: &RenderWorld, geom: &TerrainGeometry) -> Vec<FeatureVertex> {
    let mut vertices = Vec::new();
    for cell in &world.terrain {
        let visibility = fog_at(world, cell.tile);
        for spot in tree_layout(world, geom, cell.tile) {
            let offset = 0.33 * spot.s;
            emit(
                &mut vertices,
                geom,
                ShadowShape {
                    center: [
                        spot.x + DIRECTION[0] * offset,
                        spot.z + DIRECTION[1] * offset,
                    ],
                    length: 0.64 * spot.s,
                    width: 0.25 * spot.s,
                    opacity: 0.25 * visibility,
                    taper: 0.46,
                    phase: spot.hue * std::f32::consts::TAU,
                },
            );
        }
    }
    for city in &world.cities {
        let (x, z) = tile_to_world(
            city.tile.0 % world.width.max(1),
            city.tile.0 / world.width.max(1),
        );
        emit(
            &mut vertices,
            geom,
            ShadowShape {
                center: [x + DIRECTION[0] * 0.25, z + DIRECTION[1] * 0.25],
                length: 0.94,
                width: 0.74,
                opacity: 0.23 * fog_at(world, city.tile),
                taper: 0.05,
                phase: city.tile.0 as f32,
            },
        );
    }
    for unit in &world.units {
        let (x, z) = tile_to_world(
            unit.tile.0 % world.width.max(1),
            unit.tile.0 / world.width.max(1),
        );
        emit(
            &mut vertices,
            geom,
            ShadowShape {
                center: [x + DIRECTION[0] * 0.10, z + DIRECTION[1] * 0.10],
                length: 0.36,
                width: 0.27,
                opacity: 0.24 * fog_at(world, unit.tile),
                taper: 0.08,
                phase: unit.tile.0 as f32,
            },
        );
    }
    vertices
}
