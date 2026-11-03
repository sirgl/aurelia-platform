//! Compiled-in Blender meshes of static props (trees, rocks, animals, bridge).
//! Layout: position, flat normal, authored sRGB color, owner-color mask.
//! Unit and world object models live in crate::unit_models and crate::world_models
//! (manifest-driven, resolved by content key).

// 45-degree normals are printed with five digits (0.70711): clippy approx_constant
// fires irrecoverably on generated code; the geometry is validated by the bake.
#![allow(clippy::approx_constant)]

use crate::gpu::buffers::UnitVertex;

include!(concat!(env!("OUT_DIR"), "/baked_models.rs"));
// Terrain props (platform/tools/blender/export_terrain.py): sculpted sea stacks.
include!(concat!(env!("OUT_DIR"), "/terrain_models.rs"));

/// Fractured stone slabs authored and exported in Blender.
pub const ROCK_MESHES: [&[UnitVertex]; 3] = [VISUAL_ROCK_0, VISUAL_ROCK_1, VISUAL_ROCK_2];


/// Pine variants for trees (picked by tile hash).
pub const PINE_MESHES: [&[UnitVertex]; 3] = [PINE_0, PINE_1, PINE_2];

pub fn pine_for_tile(tile: u32) -> usize {
    (crate::scene::terrain::fields::hash_u32(tile ^ 0x71E5) % 3) as usize
}

#[cfg(test)]
mod geometry_tests {
    #[test]
    fn rock_assets_have_broad_fracture_planes_and_no_team_color() {
        use std::collections::BTreeSet;
        for mesh in super::ROCK_MESHES {
            let mut normals = BTreeSet::new();
            assert!((100..=250).contains(&(mesh.len() / 3)));
            for triangle in mesh.chunks_exact(3) {
                let n = triangle[0].normal;
                normals.insert(n.map(|v| (v * 1000.).round() as i32));
                for v in triangle {
                    assert!(v.pos.iter().all(|v| v.is_finite()));
                    assert_eq!(v.team, 0.);
                    assert!(v.pos[0].abs() <= 0.5 && v.pos[2].abs() <= 0.5);
                    assert!((0.0..=0.8).contains(&v.pos[1]));
                }
            }
            // Broad polygons share normals after triangulation; a triangulated
            // sphere would have a different normal for almost every triangle.
            assert!(
                normals.len() < mesh.len() / 6,
                "too many tiny facets, no broad fracture planes"
            );
            assert!(normals.len() > 20, "asset collapsed to a simple box");
        }
    }
}
