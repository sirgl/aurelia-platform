//! CPU side of feature meshes: mountains + trees (non-indexed, flat normals).
//!
//! The CPU vertex format is 40B: pos + normal + color + fog; on the GPU 32B
//! (`GpuFeatureVertex`: normal and color in half float). Colours are baked
//! (rock->snow by height, needles/bark on trees): there is no separate material/
//! texture, the shader is only lambert + fog. One aggregated buffer
//! for all features (batching contract: tens of draw calls, not hundreds).

use crate::scene::layout::tile_to_world;
use crate::scene::terrain::mountains::{mountain_seeds, mountain_sample, MountainSample};
#[cfg(test)]
use crate::scene::terrain::mountains::mountain_profile;
use crate::scene::terrain::fields::TerrainGeometry;
use crate::world::state::RenderWorld;
use kernel::ids::TileId;

/// Feature vertex: position + flat normal + colour + fog.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FeatureVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
    pub fog: f32,
}

/// A feature vertex as the GPU stores it: 32 B instead of 40 (features are the largest
/// GPU category of a developed map, and their meshes wait on the CPU for upload).
/// Position and fog keep full precision (the shader compares fog with the exact fog
/// levels); normal and colour are half floats, which keep colours above 1.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuFeatureVertex {
    pub pos: [f32; 3],
    pub fog: f32,
    /// Normal xyz, 0.
    normal: [u16; 4],
    /// Colour rgb, 1.
    color: [u16; 4],
}

impl From<&FeatureVertex> for GpuFeatureVertex {
    fn from(v: &FeatureVertex) -> Self {
        let half = |x: f32| half::f16::from_f32(x).to_bits();
        let [nx, ny, nz] = v.normal.map(half);
        let [r, g, b] = v.color.map(half);
        Self { pos: v.pos, fog: v.fog, normal: [nx, ny, nz, 0], color: [r, g, b, half(1.0)] }
    }
}

/// The GPU form of a mesh.
pub fn to_gpu(mesh: &[FeatureVertex]) -> Vec<GpuFeatureVertex> {
    mesh.iter().map(GpuFeatureVertex::from).collect()
}

fn flat_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3], outward: [f32; 3]) -> [f32; 3] {
    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let mut n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    if n[0] * outward[0] + n[1] * outward[1] + n[2] * outward[2] < 0.0 {
        n = [-n[0], -n[1], -n[2]];
    }
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
    [n[0] / len, n[1] / len, n[2] / len]
}

fn mountain_face(out: &mut Vec<FeatureVertex>, tri: [MountainSample; 3]) {
    if tri.iter().all(|v| v.lift < 0.075) {
        return;
    }
    let [a, b, c] = tri.map(|v| v.pos);
    let normal = flat_normal(a, b, c, [0.0, 1.0, 0.0]);
    let lift = tri.iter().map(|v| v.lift).sum::<f32>() / 3.0;
    let snow = tri.iter().map(|v| v.snow).sum::<f32>();
    // Shared stone albedo lets actual ridge planes and light define the facets.
    let color = if snow >= 1.5 && normal[1] > 0.43 {
        [0.92, 0.94, 0.95]
    } else if lift < 0.18 {
        [0.49, 0.51, 0.44]
    } else {
        // Shared world-space strata give the massif warm stone and cool cuts;
        // lighting still provides the actual facet contrast.
        let center = [(a[0] + b[0] + c[0]) / 3., (a[2] + b[2] + c[2]) / 3.];
        let mineral = ((center[0] * 0.37 + center[1] * 0.23).sin() * 0.5 + 0.5) * 0.07;
        [0.48 + mineral, 0.51 + mineral * 0.65, 0.54 + mineral * 0.28]
    };
    for v in tri {
        out.push(FeatureVertex {
            pos: [v.pos[0], v.pos[1] + 0.012, v.pos[2]],
            normal,
            color,
            fog: v.fog,
        });
    }
}

fn emit_massifs(out: &mut Vec<FeatureVertex>, world: &RenderWorld, geom: &TerrainGeometry) {
    use crate::scene::terrain::fields::patch_control_points;
    if geom.surface.is_some() {
        return;
    }
    let seeds = mountain_seeds(world);
    if seeds.is_empty() {
        return;
    }
    // Legacy terrain still uses these hex patch triangles.
    for cell in &world.terrain {
        if crate::scene::fog::fog_at(world, cell.tile) < 0.4 {
            continue;
        }
        let q = cell.tile.0 % world.width.max(1);
        let r = cell.tile.0 / world.width.max(1);
        let (cx, cz) = tile_to_world(q, r);
        let nearby: Vec<_> = seeds
            .iter()
            .copied()
            .filter(|s| {
                let range = s.radius * 1.1 + 1.1;
                (s.center[0] - cx).abs() < range && (s.center[1] - cz).abs() < range
            })
            .collect();
        if nearby.is_empty() {
            continue;
        }
        let (o, inner, corners, edges) = patch_control_points(geom, cell.tile, q, r);
        for i in 0..6 {
            let j = (i + 1) % 6;
            let triangles = [
                [o, inner[i], inner[j]],
                [inner[i], corners[i], edges[i]],
                [inner[i], edges[i], inner[j]],
                [inner[j], edges[i], corners[j]],
            ];
            for triangle in triangles {
                mountain_face(
                    out,
                    triangle.map(|p| mountain_sample(world, &nearby, p)),
                );
            }
        }
    }
}

/// Pine meshes are stretched vertically to form narrow, readable conifers.
const PINE_BASE_SCALE: f32 = 0.88;

/// A tree spot: position + ground + scale + tint (a pure function).
#[derive(Debug, Clone, Copy)]
pub struct TreeSpot {
    pub x: f32,
    pub z: f32,
    pub y: f32,
    pub s: f32,
    pub hue: f32,
    pub species: Species,
}

/// Which trees a wooded tile grows: temperate pines, dark taiga on tundra,
/// broad tropical crowns in the jungle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Species {
    Pine,
    Taiga,
    Jungle,
}

impl Species {
    fn of(world: &RenderWorld, tile: TileId, terrain: u16) -> Self {
        use crate::world::state::FeatureKind;
        match world.feature.get(tile.0 as usize) {
            Some(FeatureKind::Jungle) => Self::Jungle,
            _ if terrain == 7 => Self::Taiga,
            _ => Self::Pine,
        }
    }
    /// Crown width over the authored pine, crown height, and the needle palette.
    fn shape(self) -> (f32, f32, [[f32; 3]; 4]) {
        match self {
            Self::Pine => (0.64, 1.0, [[0.19, 0.36, 0.27], [0.31, 0.44, 0.26], [0.39, 0.48, 0.26], [0.17, 0.38, 0.34]]),
            Self::Taiga => (0.52, 1.12, [[0.13, 0.27, 0.24], [0.18, 0.31, 0.25], [0.22, 0.33, 0.26], [0.14, 0.29, 0.29]]),
            Self::Jungle => (1.05, 0.78, [[0.11, 0.42, 0.16], [0.20, 0.52, 0.14], [0.09, 0.35, 0.20], [0.27, 0.55, 0.17]]),
        }
    }
}

/// Tree layout of a tile (deterministic: tile hashes, no RNG).
/// Empty: no forest in the projection (sim layer Map.feature -> RenderWorld.forest),
/// not grass/plains/hills, a city, an unknown tile, off the map, a cliff.
/// On hills two asymmetric groups open a shoulder and a slope between them.
pub fn tree_layout(world: &RenderWorld, geom: &TerrainGeometry, tile: TileId) -> Vec<TreeSpot> {
    use crate::scene::terrain::evaluator::{sample_surface, slope_zone, SlopeZone};
    use crate::scene::terrain::fields::hash_u32;
    let terrain = world
        .terrain
        .get(tile.0 as usize)
        .map(|c| c.terrain)
        .unwrap_or(99);
    if !matches!(terrain, 0..=2 | 7) || crate::scene::fog::fog_at(world, tile) < 0.4 {
        return Vec::new();
    }
    // The sim's forest flag is the only source of "there is forest here" (the 45% hash
    // of v35 is removed: the list of tiles is owned by Map.feature).
    if world.forest.get(tile.0 as usize).copied().unwrap_or(0) == 0 {
        return Vec::new();
    }
    if world.cities.iter().any(|m| m.tile == tile) {
        return Vec::new();
    }
    let h01 =
        |slot: u32| (hash_u32(tile.0 ^ slot.wrapping_mul(0x85EB_CA6B)) % 1000) as f32 / 1000.0;
    let w = geom.width.max(1);
    let (wx, wz) = tile_to_world(tile.0 % w, tile.0 / w);
    let hill = terrain == 2;
    let species = Species::of(world, tile, terrain);
    // Jungle canopies close up: more groups.
    let dense = if species == Species::Jungle { 2 } else { 0 };
    let groups = 5 + dense + (h01(0xBE_EF) * if hill { 3.0 } else { 4.0 }) as usize;
    let groups = if hill { groups.min(7) } else { groups };
    let n = groups * 2;
    let rotation = h01(0x7100) * std::f32::consts::TAU;
    let (sin, cos) = rotation.sin_cos();
    let occupied = world.visible.get(tile.0 as usize).copied().unwrap_or(0) > 0
        && world.units.iter().any(|unit| unit.tile == tile);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let group = i / 2;
        let (dx, dz) = if hill {
            // Inner grove reaches the crown; outer grove follows one shoulder.
            // A rotated, uneven gap exposes the hill without a bare summit or ring.
            let offsets = [
                [0.10, -0.18],
                [0.34, -0.19],
                [0.25, 0.06],
                [-0.51, 0.27],
                [-0.70, 0.12],
                [-0.55, 0.51],
                [-0.32, 0.49],
            ];
            let [a, b] = offsets[group];
            let a = a + (h01(0x7500 + group as u32) - 0.5) * 0.07;
            let b = b + (h01(0x7600 + group as u32) - 0.5) * 0.07;
            (a * cos - b * sin, a * sin + b * cos)
        } else {
            let ang = group as f32 * 2.399_963 + rotation;
            let dist = 0.78 * ((group as f32 + 0.5) / groups as f32).sqrt();
            (ang.cos() * dist, ang.sin() * dist)
        };
        // Small, rotated pairs retain broad clearings instead of filling a regular grid.
        let pair_angle = h01(0x7700 + group as u32) * std::f32::consts::TAU;
        let radius = 0.065 + h01(0x7800 + group as u32) * 0.015;
        let side = if i % 2 == 0 { -1. } else { 1. };
        let dx = dx + pair_angle.cos() * radius * side;
        let dz = dz + pair_angle.sin() * radius * side;
        let dist = dx.hypot(dz);
        if occupied && dist < 0.65 {
            continue;
        }
        let (x, z) = (wx + dx, wz + dz);
        let Some(s) = sample_surface(geom, x, z) else {
            continue;
        };
        if slope_zone(&s) == SlopeZone::Cliff {
            continue;
        }
        if geom
            .surface
            .as_ref()
            .is_some_and(|surface| surface.sample_water(x, z).is_some())
        {
            continue;
        }
        let scale = if hill {
            (0.60 + 0.24 * h01(0x7300 + i as u32)) * (1. - s.slope * 0.6)
        } else {
            0.70 + 0.48 * h01(0x7300 + i as u32)
        };
        out.push(TreeSpot {
            x,
            z,
            y: s.y,
            s: scale,
            hue: h01(0x7400 + i as u32),
            species,
        });
    }
    out
}

/// A conifer from the baked pin (PINE_MESHES): spot position + rotation by hue
/// (so the trees do not all face one way) + uniform scale; normals are
/// preserved by rotation, lengths do not change. The mesh's team mask is ignored.
fn emit_pine(out: &mut Vec<FeatureVertex>, spot: &TreeSpot, variant: usize, fog: f32) {
    use crate::models::PINE_MESHES;
    let mesh = PINE_MESHES[variant.min(PINE_MESHES.len() - 1)];
    let (width, tall, palette) = spot.species.shape();
    let k = PINE_BASE_SCALE * spot.s;
    let rot = spot.hue * std::f32::consts::TAU;
    let (cs, sn) = (rot.cos(), rot.sin());
    for v in mesh {
        let (lx, ly, lz) = (v.pos[0] * k * width, v.pos[1] * k * tall, v.pos[2] * k * width);
        out.push(FeatureVertex {
            pos: [
                spot.x + lx * cs - lz * sn,
                spot.y + ly - k * (0.18 / 1.35),
                spot.z + lx * sn + lz * cs,
            ],
            normal: {
                let n = [v.normal[0] / width, v.normal[1] / tall, v.normal[2] / width];
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
                [
                    (n[0] * cs - n[2] * sn) / len,
                    n[1] / len,
                    (n[0] * sn + n[2] * cs) / len,
                ]
            },
            color: if v.color[1] > v.color[0] * 1.1 {
                let c = palette[(spot.hue * 3.99) as usize];
                let ao = 0.80 + 0.20 * v.pos[1].clamp(0.0, 1.0);
                [c[0] * ao, c[1] * ao, c[2] * ao]
            } else {
                [0.28, 0.23, 0.17]
            },
            fog,
        });
    }
}

/// Batched faceted mountain ranges and dense pine groves.
/// Every mountain biome tile has a silhouette, including isolated peaks.
/// Placement is deterministic and grounded on the terrain evaluator.
pub fn build_mountain_mesh(world: &RenderWorld, geom: &TerrainGeometry) -> Vec<FeatureVertex> {
    let mut out = build_rock_mesh(world, geom);
    out.extend(build_tree_mesh(world, geom));
    out
}

/// Legacy rock overlay. Continuous surfaces already include their mountain geometry.
pub fn build_rock_mesh(world: &RenderWorld, geom: &TerrainGeometry) -> Vec<FeatureVertex> {
    let mut out = Vec::new();
    emit_massifs(&mut out, world, geom);
    out
}

/// Tree-only mesh lets the renderer cache mountain geometry independently.
pub fn build_tree_mesh(world: &RenderWorld, geom: &TerrainGeometry) -> Vec<FeatureVertex> {
    let tiles: Vec<TileId> = world.terrain.iter().map(|cell| cell.tile).collect();
    build_tree_mesh_for(world, geom, &tiles)
}

/// Trees of the given tiles only. A tile's trees depend on that tile alone, so the
/// renderer rebuilds just the feature chunks whose tiles changed.
pub fn build_tree_mesh_for(
    world: &RenderWorld,
    geom: &TerrainGeometry,
    tiles: &[TileId],
) -> Vec<FeatureVertex> {
    use crate::scene::fog::fog_at;
    let mut out = Vec::new();
    // Forest comes from the simulation (RenderWorld.forest); trees are baked pines
    // (PINE_MESHES) with rotation and scale, without discs under them.
    for &tile in tiles {
        for (i, spot) in tree_layout(world, geom, tile).into_iter().enumerate() {
            let fog = fog_at(world, tile);
            emit_pine(
                &mut out,
                &spot,
                crate::models::pine_for_tile(tile.0 ^ i as u32),
                fog,
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::heightfield::build_heightfield;
    use crate::scene::style::{compile, default_style};
    use crate::scene::terrain::fields::build_terrain_geometry;
    use crate::world::state::TerrainCell;

    #[test]
    fn mountain_ridge_has_broad_planes_asymmetry_and_bounded_support() {
        for variation in [0., 0.5, 1.] {
            assert_eq!(mountain_profile(0., 0., variation), 1., "retain authored summit height");
            let flank = |x| mountain_profile(x, 0.32, variation);
            assert!((flank(0.10) + flank(0.20) - 2. * flank(0.15)).abs() < 1e-6,
                "a broad flank is planar, not a rounded radial cone");
            assert!((mountain_profile(0.4, 0., variation)
                - mountain_profile(0., 0.4, variation)).abs() > 0.15,
                "ridge must be elongated rather than rotationally symmetric");
            assert!((mountain_profile(0., 0.3, variation)
                - mountain_profile(0., -0.3, variation)).abs() > 0.03,
                "opposite shoulders have different slopes");
            for z in -24..=24 {
                for x in -24..=24 {
                    let u = x as f32 * 0.05;
                    let v = z as f32 * 0.05;
                    let height = mountain_profile(u, v, variation);
                    assert!((0.0..=1.0).contains(&height));
                    if u.hypot(v) >= 1. {
                        assert_eq!(height, 0., "support must stay inside existing seed spatial bins");
                    }
                }
            }
        }
    }

    #[test]
    fn continuous_mountains_are_actual_ground_and_need_no_overlay() {
        let mut world = test_world(7, 7, 1);
        world.visual_surface = true;
        world.terrain[24].terrain = 6;
        let geom = geom_of(&world);
        let surface = geom.surface.as_ref().unwrap();
        assert!(surface.mountain.iter().any(|m| m.lift > 1.));
        let mut checked = 0;
        for face in &surface.land_triangles {
            if !face.iter().any(|&i| surface.mountain[i as usize].lift > 0.075) { continue; }
            let p = face.map(|i| surface.vertices[i as usize].position);
            let center: [f32; 3] = std::array::from_fn(|i| (p[0][i] + p[1][i] + p[2][i]) / 3.);
            let ground = surface.sample_ground(center[0], center[2]).unwrap();
            assert!((ground.y - center[1]).abs() < 1e-4, "queries use the actual mountain face");
            assert!(ground.normal.iter().all(|x| x.is_finite()));
            checked += 1;
        }
        assert!(checked > 20);
        assert!(build_rock_mesh(&world, &geom).is_empty(), "no duplicate continuous rock overlay");
        assert!(build_mountain_mesh(&world, &geom).is_empty(), "no forest in this fixture");
    }

    #[test]
    fn mountain_surface_leaves_actual_river_faces_and_contacts_open() {
        use std::collections::BTreeMap;
        let mut world = test_world(7, 7, 1);
        world.visual_surface = true;
        world.terrain[24].terrain = 6;
        world.river_edges = vec![(24, 25)];
        let geom = geom_of(&world);
        let surface = geom.surface.as_ref().unwrap();
        let key = |p: [[f32; 3]; 3]| { let mut k = p.map(|v| [v[0].to_bits(), v[2].to_bits()]); k.sort(); k };
        let ground: BTreeMap<_, _> = surface.land_triangles.iter().map(|f| {
            (key(f.map(|i| surface.vertices[i as usize].position)), *f)
        }).collect();
        let seeds = mountain_seeds(&world);
        let mut overlap_without_carving = false;
        for water in &surface.water_triangles {
            let p = water.map(|i| surface.vertices[i as usize].position);
            overlap_without_carving |= p.iter().any(|p| mountain_sample(&world, &seeds, *p).lift > 0.2);
            let face = ground[&key(p)];
            for &i in &face {
                let vertex = &surface.vertices[i as usize];
                let water_height = p.iter().find(|v| v[0] == vertex.position[0] && v[2] == vertex.position[2]).unwrap()[1];
                assert_eq!(surface.mountain[i as usize].lift, 0., "wet bed and shared contact never lifted");
                assert!(vertex.position[1] <= water_height + 1e-5);
            }
            let center = [p.iter().map(|v| v[0]).sum::<f32>() / 3., p.iter().map(|v| v[2]).sum::<f32>() / 3.];
            assert!(surface.sample_ground(center[0], center[1]).unwrap().y
                <= surface.sample_water(center[0], center[1]).unwrap().y + 1e-5);
        }
        assert!(overlap_without_carving, "fixture intersects the uncarved envelope");
        assert!(surface.mountain.iter().any(|m| m.lift > 0.3), "neighbouring summit retained");
        let again = geom_of(&world);
        assert_eq!(surface.vertices, again.surface.as_ref().unwrap().vertices);
    }

    #[test]
    fn continuous_surface_does_not_leak_hidden_mountain_seeds_or_neighbour_rank() {
        let mut world = test_world(7, 7, 1);
        world.visual_surface = true;
        world.terrain[24].terrain = 6;
        world.visible[25] = 0;
        world.explored[25] = 0;
        let before = geom_of(&world).surface.unwrap();
        assert!(before.mountain.iter().any(|m| m.lift > 0.3));
        world.terrain[25].terrain = 6;
        let after = geom_of(&world).surface.unwrap();
        assert_eq!(before.vertices, after.vertices, "hidden neighbour cannot change known geometry or materials");
        assert_eq!(before.mountain, after.mountain);
        world.terrain[24].terrain = 1;
        let hidden_only = geom_of(&world).surface.unwrap();
        assert!(hidden_only.mountain.iter().all(|m| m.lift == 0.), "hidden peaks cannot grow visible foothills");
        world.visible.fill(0);
        world.explored.fill(0);
        assert!(geom_of(&world).surface.unwrap().mountain.iter().all(|m| m.lift == 0.));
    }

    #[test]
    fn visible_formation_gets_a_clearing_without_removing_forest_state() {
        let mut world = test_world(8, 8, 1);
        world.forest = vec![1; 64];
        let tile = TileId(27);
        let style = compile(default_style()).unwrap();
        let hf = build_heightfield(&world, &style);
        let geom = build_terrain_geometry(&world, &style, &hf);
        let original = tree_layout(&world, &geom, tile);
        world.units.push(crate::world::state::UnitMarker {
            id: 0, embarked: false, fortified: false, tile,
            owner: 0,
            kind: 4,
            hp: 100,
        });
        let occupied = tree_layout(&world, &geom, tile);
        assert!(!occupied.is_empty() && occupied.len() < original.len());
        assert_eq!(world.forest[tile.0 as usize], 1);
        world.visible[tile.0 as usize] = 0;
        assert_eq!(
            tree_layout(&world, &geom, tile).len(),
            original.len(),
            "hidden unit must not alter forest silhouette"
        );
    }

    fn test_world(w: u32, h: u32, terrain: u16) -> RenderWorld {
        let mut world = RenderWorld {
            width: w,
            height: h,
            visible: vec![1; (w * h) as usize],
            explored: vec![1; (w * h) as usize],
            ..Default::default()
        };
        for r in 0..h {
            for q in 0..w {
                world.terrain.push(TerrainCell {
                    tile: TileId(r * w + q),
                    terrain,
                    owner: None,
                    resource: None,
                });
            }
        }
        world
    }

    fn geom_of(world: &RenderWorld) -> TerrainGeometry {
        let style = compile(default_style()).unwrap();
        let hf = build_heightfield(world, &style);
        build_terrain_geometry(world, &style, &hf)
    }

    #[test]
    fn mountain_silhouettes_are_finite_and_grounded() {
        let world = test_world(5, 5, 6);
        let geom = geom_of(&world);
        let mesh = build_mountain_mesh(&world, &geom);
        assert!(!mesh.is_empty());
        for v in &mesh {
            assert!(v.pos.iter().all(|c| c.is_finite()));
            let n = (v.normal[0].powi(2) + v.normal[1].powi(2) + v.normal[2].powi(2)).sqrt();
            assert!((n - 1.0).abs() < 1e-4);
            assert!(v.color.iter().all(|c| (0.0..=1.0).contains(c)));
        }
        // The rock skin follows the exact ground triangulation: intermediate
        // points must not expose green ground wedges through the slopes.
        for tri in mesh.chunks_exact(3) {
            let center = [
                (tri[0].pos[0] + tri[1].pos[0] + tri[2].pos[0]) / 3.0,
                (tri[0].pos[1] + tri[1].pos[1] + tri[2].pos[1]) / 3.0,
                (tri[0].pos[2] + tri[1].pos[2] + tri[2].pos[2]) / 3.0,
            ];
            if let Some(ground) =
                crate::scene::terrain::evaluator::sample_surface(&geom, center[0], center[2])
            {
                assert!(
                    center[1] >= ground.y + 0.005,
                    "rock triangle cuts through terrain"
                );
            }
        }
        let hills = test_world(3, 1, 2);
        assert!(build_mountain_mesh(&hills, &geom_of(&hills)).is_empty());
        let isolated = test_world(1, 1, 6);
        assert!(!build_mountain_mesh(&isolated, &geom_of(&isolated)).is_empty());
    }

    #[test]
    fn wooded_hill_has_grounded_groves_open_shoulder_and_crown_trees() {
        use crate::scene::terrain::evaluator::{sample_surface, slope_zone, SlopeZone};
        let mut world = test_world(7, 7, 1);
        world.visual_surface = true;
        let tile = TileId(24);
        world.terrain[24].terrain = 2;
        let geom = geom_of(&world);
        assert!(tree_layout(&world, &geom, tile).is_empty());
        world.forest = vec![0; 49];
        world.forest[24] = 1;
        let original = tree_layout(&world, &geom, tile);
        assert!(
            (7..=14).contains(&original.len()),
            "a hill still reads as forest: {}",
            original.len()
        );
        let (cx, cz) = tile_to_world(3, 3);
        let angles: Vec<_> = original
            .iter()
            .map(|s| (s.z - cz).atan2(s.x - cx))
            .collect();
        let mut angles = angles;
        angles.sort_by(f32::total_cmp);
        let max_gap = angles
            .windows(2)
            .map(|a| a[1] - a[0])
            .chain(std::iter::once(
                angles[0] + std::f32::consts::TAU - angles[angles.len() - 1],
            ))
            .fold(0.0, f32::max);
        assert!(
            max_gap > 1.7,
            "a broad shoulder remains visible between groves"
        );
        assert!(
            original.iter().any(|s| (s.x - cx).hypot(s.z - cz) < 0.3),
            "no bald summit ring"
        );
        assert!(
            original.iter().any(|s| (s.x - cx).hypot(s.z - cz) > 0.65),
            "outer grove reaches slope"
        );
        for spot in &original {
            let ground = sample_surface(&geom, spot.x, spot.z).unwrap();
            assert!((spot.y - ground.y).abs() < 1e-5);
            assert_ne!(slope_zone(&ground), SlopeZone::Cliff);
            assert!(spot.s <= 0.84, "hill trees do not hide the entire landform");
        }
        let signature = |spots: &[TreeSpot]| {
            spots
                .iter()
                .map(|s| [s.x, s.y, s.z, s.s, s.hue])
                .collect::<Vec<_>>()
        };
        assert_eq!(
            signature(&original),
            signature(&tree_layout(&world, &geom, tile))
        );
        world.units.push(crate::world::state::UnitMarker {
            id: 0, embarked: false, fortified: false, tile,
            owner: 0,
            kind: 4,
            hp: 100,
        });
        let occupied = tree_layout(&world, &geom, tile);
        assert!(!occupied.is_empty() && occupied.len() < original.len());
        assert!(occupied.iter().all(|s| (s.x - cx).hypot(s.z - cz) >= 0.65));
        assert_eq!(world.forest[24], 1);
        world.visible[24] = 0;
        assert_eq!(
            signature(&original),
            signature(&tree_layout(&world, &geom, tile)),
            "hidden units cannot change a remembered forest"
        );
        world.explored[24] = 0;
        assert!(
            tree_layout(&world, &geom, tile).is_empty(),
            "unknown hills leak no forest"
        );
    }

    #[test]
    fn smaller_forest_keeps_roots_dry_and_tree_height_reduced() {
        let spot = TreeSpot { x: 0., y: 0., z: 0., s: 1., hue: 0.35, species: Species::Pine };
        for (variant, source) in crate::models::PINE_MESHES.iter().enumerate() {
            let mut tree = vec![];
            emit_pine(&mut tree, &spot, variant, 1.);
            let top = tree.iter().map(|v| v.pos[1]).fold(f32::NEG_INFINITY, f32::max);
            let bottom = tree.iter().map(|v| v.pos[1]).fold(f32::INFINITY, f32::min);
            let previous_top = source.iter().map(|v| v.pos[1] * 1.35 - 0.18)
                .fold(f32::NEG_INFINITY, f32::max);
            assert!((0.60..=0.70).contains(&(top / previous_top)));
            assert!(bottom < 0., "small trees must retain embedded roots");
        }
        let mut world = test_world(7, 7, 1);
        world.visual_surface = true;
        world.forest = vec![1; 49];
        world.river_edges = vec![(24, 25)];
        let geom = geom_of(&world);
        let surface = geom.surface.as_ref().unwrap();
        assert!(!surface.water_triangles.is_empty());
        let mut count = 0;
        for tile in [TileId(24), TileId(25)] {
            for spot in tree_layout(&world, &geom, tile) {
                count += 1;
                assert!(surface.sample_water(spot.x, spot.z).is_none());
                assert!((surface.sample_ground(spot.x, spot.z).unwrap().y - spot.y).abs() < 1e-5);
            }
        }
        assert!(count >= 10, "riverbank retains a visible forest");
    }

    #[test]
    fn trees_grow_on_forest_cells_and_drape() {
        // Forest is a sim layer (RenderWorld.forest), not a 45% hash: without the flag even
        // grass is empty; with the flag, 10-16 smaller pins per tile. A flag on
        // desert/water/mountains gives no trees: the terrain gate is kept.
        let mut world = test_world(6, 6, 1);
        let geom = geom_of(&world);
        for cell in &world.terrain {
            assert!(
                tree_layout(&world, &geom, cell.tile).is_empty(),
                "no forest flag — no trees"
            );
        }
        assert!(build_mountain_mesh(&world, &geom).is_empty());
        world.forest = vec![1u8; (6 * 6) as usize];
        let mut spots = 0usize;
        for cell in &world.terrain {
            let layout = tree_layout(&world, &geom, cell.tile);
            assert!(layout.len() <= 16, "dense grove remains bounded");
            for s in &layout {
                assert!([s.x, s.y, s.z, s.s, s.hue].iter().all(|c| c.is_finite()));
                // Anchor on the surface (roots slightly in the ground, the trunk with -0.06).
                let gy = crate::scene::terrain::evaluator::sample_surface(&geom, s.x, s.z)
                    .expect("spot on mesh")
                    .y;
                assert!((s.y - gy).abs() < 1e-3);
            }
            spots += layout.len();
        }
        assert!(spots > 0, "forest grows trees");
        let mesh = build_mountain_mesh(&world, &geom);
        assert!((378..=466).contains(&spots), "forest density stays near twice the prior 222-tree fixture");
        // Mesh output contains only pines, with no opaque ground disks.
        let mut expect = 0usize;
        for cell in &world.terrain {
            for (i, _) in tree_layout(&world, &geom, cell.tile)
                .into_iter()
                .enumerate()
            {
                expect += crate::models::PINE_MESHES
                    [crate::models::pine_for_tile(cell.tile.0 ^ i as u32)]
                .len();
            }
        }
        assert_eq!(mesh.len(), expect, "pines only, no opaque footprint disks");
        for v in &mesh {
            assert!(v.pos.iter().all(|c| c.is_finite()));
            assert!(v.color.iter().all(|c| (0.0..=1.0).contains(c)));
            let n = (v.normal[0].powi(2) + v.normal[1].powi(2) + v.normal[2].powi(2)).sqrt();
            assert!((n - 1.0).abs() < 1e-4, "unit normal");
        }
        // Byte-wise determinism.
        let again = build_mountain_mesh(&world, &geom);
        assert_eq!(mesh.len(), again.len());
        for (x, y) in mesh.iter().zip(again.iter()) {
            assert_eq!((x.pos, x.normal, x.color), (y.pos, y.normal, y.color));
        }
        for terrain in [3u16, 4, 5, 6] {
            let mut w = test_world(4, 4, terrain);
            w.forest = vec![1u8; 16];
            let g = geom_of(&w);
            for cell in &w.terrain {
                assert!(
                    tree_layout(&w, &g, cell.tile).is_empty(),
                    "no trees on terrain {terrain}"
                );
            }
        }
    }

    #[test]
    fn remembered_massif_foothills_never_default_to_visible() {
        let mut world = test_world(8, 8, 1);
        world.terrain[27].terrain = 6;
        world.visible = vec![0; 64];
        world.explored = vec![1; 64];
        let mesh = build_mountain_mesh(&world, &geom_of(&world));
        assert!(!mesh.is_empty());
        assert!(
            mesh.iter().all(|v| v.fog <= 0.45),
            "foothills must use cell visibility"
        );
        world.explored.fill(0);
        assert!(build_mountain_mesh(&world, &geom_of(&world)).is_empty());
    }

    #[test]
    fn mountains_deterministic_and_empty_without_hills() {
        let world = test_world(4, 4, 6);
        let geom = geom_of(&world);
        let a = build_mountain_mesh(&world, &geom);
        let b = build_mountain_mesh(&world, &geom);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!((x.pos, x.normal, x.color), (y.pos, y.normal, y.color));
        }
        // Without hills/grass it is empty, not a panic (desert: no peaks, no trees).
        let flat = test_world(4, 4, 3);
        let geom_flat = geom_of(&flat);
        assert!(build_mountain_mesh(&flat, &geom_flat).is_empty());
    }
}
