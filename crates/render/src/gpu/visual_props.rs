//! Geological coast props and sea features in feature chunks.
//! Resources and improvements are semantic instanced models (gpu/world_objects).
//! Bare inland terrain deliberately has no random decorative rock scatter.
use crate::gpu::feature::FeatureVertex;
use crate::scene::terrain::evaluator::sample_surface;
use crate::scene::terrain::fields::TerrainGeometry;
use crate::world::state::RenderWorld;

/// Rigid, uniformly scaled geology retains the broad authored fracture planes.
/// A low terrain anchor embeds the feet, rather than warping each vertex over relief.
pub(super) fn rock_mesh(
    geom: &TerrainGeometry,
    center: [f32; 2],
    angle: f32,
    scale: f32,
    variant: usize,
    fog: f32,
) -> Vec<FeatureVertex> {
    let (sin, cos) = angle.sin_cos();
    let horizontal = |x: f32, z: f32| {
        [
            center[0] + scale * (x * cos - z * sin),
            center[1] + scale * (x * sin + z * cos),
        ]
    };
    let base = [[0., 0.], [-0.35, 0.], [0.35, 0.], [0., -0.35], [0., 0.35]]
        .into_iter()
        .map(|p| {
            let p = horizontal(p[0], p[1]);
            sample_surface(geom, p[0], p[1]).map_or(0., |s| s.y)
        })
        .fold(f32::INFINITY, f32::min)
        - 0.035;
    crate::models::ROCK_MESHES[variant % 3]
        .iter()
        .map(|v| {
            let p = horizontal(v.pos[0], v.pos[2]);
            FeatureVertex {
                pos: [p[0], base + v.pos[1] * scale, p[1]],
                normal: [
                    v.normal[0] * cos - v.normal[2] * sin,
                    v.normal[1],
                    v.normal[0] * sin + v.normal[2] * cos,
                ],
                color: v.color,
                fog,
            }
        })
        .collect()
}

/// Only forests change their geometry to make room for a unit.
pub fn occupancy_matters(world: &RenderWorld, tile: kernel::ids::TileId) -> bool {
    world.forest.get(tile.0 as usize).copied().unwrap_or(0) != 0
}

pub fn build(world: &RenderWorld, geom: &TerrainGeometry) -> Vec<FeatureVertex> {
    let tiles: Vec<kernel::ids::TileId> = world.terrain.iter().map(|cell| cell.tile).collect();
    build_for(world, geom, &tiles)
}

/// Coast and sea geometry for this chunk. Resource models are built separately.
pub fn build_for(
    world: &RenderWorld,
    geom: &TerrainGeometry,
    tiles: &[kernel::ids::TileId],
) -> Vec<FeatureVertex> {
    let mut vertices = Vec::new();
    if let Some(surface) = &geom.surface {
        vertices.extend(super::coast_props::build(world, surface, tiles));
    }
    vertices.extend(super::sea_features::build(world, tiles));
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::heightfield::build_heightfield;
    use crate::scene::style::{compile, default_style};
    use crate::scene::terrain::fields::build_terrain_geometry;
    use crate::world::state::TerrainCell;
    use kernel::ids::TileId;

    fn open_world() -> RenderWorld {
        let n = 81;
        RenderWorld {
            width: 9,
            height: 9,
            visible: vec![1; n],
            explored: vec![1; n],
            forest: vec![0; n],
            river: vec![0; n],
            road: vec![0; n],
            improvements: vec![None; n],
            natural_wonders: vec![None; n],
            terrain: (0..n as u32)
                .map(|i| TerrainCell {
                    tile: TileId(i),
                    terrain: 1,
                    owner: None,
                    resource: None,
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn bare_inland_tiles_do_not_spawn_decorative_rocks() {
        let mut world = open_world();
        let style = compile(default_style()).unwrap();
        for terrain in [0, 1, 2] {
            for cell in &mut world.terrain {
                cell.terrain = terrain;
            }
            let geom = build_terrain_geometry(&world, &style, &build_heightfield(&world, &style));
            assert!(
                build(&world, &geom).is_empty(),
                "bare plains, grassland and hills have no meaningless props"
            );
        }
    }
}
