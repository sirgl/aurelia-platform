//! Invariants of the regional build (platform/MESHING.md §3, 1–2 and §2.5).
use super::*;
use crate::scene::style::{compile, default_style};
use crate::world::state::TerrainCell;
use kernel::ids::TileId;
use std::collections::{HashMap, HashSet};

type Key = (u32, u32);

fn key(p: [f32; 3]) -> Key {
    (p[0].to_bits(), p[2].to_bits())
}

/// The island and a coast of hills with a river reaching the cliffs.
fn worlds() -> Vec<RenderWorld> {
    let mut cliffs = RenderWorld {
        visual_surface: true,
        width: 20,
        height: 14,
        visible: vec![1; 280],
        explored: vec![1; 280],
        terrain: (0..280)
            .map(|i| {
                let (q, r) = (i % 20, i / 20);
                let terrain = if q >= 12 || (r == 7 && q >= 10) { if r == 7 && q <= 13 { 2 } else { 4 } } else if q >= 10 { 2 } else { 1 };
                TerrainCell { tile: TileId(i), terrain, owner: None, resource: None }
            })
            .collect(),
        ..Default::default()
    };
    let a = 5 * 20 + 9;
    cliffs.river_edges = vec![(a, a + 1), (a - 20 + 1, a + 1), (a - 20 + 1, a - 20 + 2)];
    vec![fixtures::island(), cliffs]
}

/// Ground copies by point for the triangles of one region.
fn region_ground(mesh: &SurfaceMesh, region: &SurfaceRegion) -> HashMap<Key, [u32; 3]> {
    let mut out = HashMap::new();
    for t in &mesh.land_triangles[region.land.start as usize..region.land.end as usize] {
        for &i in t {
            let p = mesh.vertices[i as usize].position;
            out.insert(key(p), p.map(f32::to_bits));
        }
    }
    out
}

fn region_edges(mesh: &SurfaceMesh, region: &SurfaceRegion) -> HashSet<(Key, Key)> {
    let mut out = HashSet::new();
    for t in &mesh.land_triangles[region.land.start as usize..region.land.end as usize] {
        for i in 0..3 {
            let (a, b) = (key(mesh.vertices[t[i] as usize].position), key(mesh.vertices[t[(i + 1) % 3] as usize].position));
            out.insert((a.min(b), a.max(b)));
        }
    }
    out
}

/// MESHING §3.1: every seam vertex and every seam edge is present in both
/// regions, bit for bit, including its height.
#[test]
fn seams_are_identical_on_both_sides() {
    let style = compile(default_style()).unwrap();
    for world in worlds() {
        for (size, mixed) in [(16, false), (8, false), (4, false), (8, true), (4, true)] {
            // Mixed: a checkerboard of detail levels, so seams join regions of
            // different levels and take the coarser one.
            let layout = regions::Layout::new(&world, size);
            let mut levels = Levels::default();
            if mixed {
                for id in layout.known(&world) {
                    levels.set(id, ((id.x + 2 * id.y) % 3) as u8);
                }
            }
            let mesh = SurfaceBuilder::default().build(&world, &style, size, &levels, None);
            let field = field::Landscape::new(&world, &style);
            let ids = layout.known(&world);
            let plan = regions::SeamPlan::new(&field, &layout, &ids, &field.constraints(), &levels);
            let by_id: HashMap<RegionId, &SurfaceRegion> = mesh.regions.iter().map(|r| (r.id, r)).collect();
            let grounds: HashMap<RegionId, _> = mesh.regions.iter().map(|r| (r.id, region_ground(&mesh, r))).collect();
            let edges: HashMap<RegionId, _> = mesh.regions.iter().map(|r| (r.id, region_edges(&mesh, r))).collect();
            let mut checked = 0;
            for (_, points) in plan.edge_points() {
                let (a, b) = (points[0], *points.last().unwrap());
                let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
                let normal = [-(b[1] - a[1]), b[0] - a[0]];
                let side = |s: f32| layout.of_point([mid[0] + normal[0] * s * 0.01, mid[1] + normal[1] * s * 0.01]);
                let (Some(left), Some(right)) = (side(1.), side(-1.)) else { continue };
                if left == right || !by_id.contains_key(&left) || !by_id.contains_key(&right) {
                    continue;
                }
                for pair in points.windows(2) {
                    let ka = (pair[0][0].to_bits(), pair[0][1].to_bits());
                    let kb = (pair[1][0].to_bits(), pair[1][1].to_bits());
                    for id in [left, right] {
                        if !edges[&id].contains(&(ka.min(kb), ka.max(kb))) {
                            let on: Vec<_> = grounds[&id].keys().map(|k| [f32::from_bits(k.0), f32::from_bits(k.1)]).filter(|p| hydrology::closest(*p, pair[0], pair[1]).0 < 1e-4).collect();
                            for c in field.constraints() {
                                let x = on.iter().find(|p| !(p[0] == pair[0][0] && p[1] == pair[0][1]) && !(p[0] == pair[1][0] && p[1] == pair[1][1])).unwrap();
                                if hydrology::closest(*x, c[0], c[1]).0 < 1e-3 {
                                    eprintln!("constraint through it: {c:?}");
                                }
                            }
                            panic!("seam edge {pair:?} missing in {id:?} (size {size}); region points on it: {on:?}; has ends: {} {}", grounds[&id].contains_key(&ka), grounds[&id].contains_key(&kb));
                        }
                    }
                    for k in [ka, kb] {
                        assert_eq!(grounds[&left].get(&k), grounds[&right].get(&k), "seam vertex differs (size {size})");
                    }
                    checked += 1;
                }
            }
            assert!(checked > 20 * (16 / size) as usize, "fixture must have internal seams: {checked}");
        }
    }
}

/// Whether a point borders a region that was not built: the whole-map build
/// meshes unknown cells there and may find a contact the regions cannot see.
fn borders_unbuilt(mesh: &SurfaceMesh, p: [f32; 3]) -> bool {
    (0..12).any(|k| {
        let a = k as f32 * std::f32::consts::TAU / 12.;
        let (x, z) = (p[0] + 0.05 * a.cos(), p[2] + 0.05 * a.sin());
        match mesh.region_of_point(x, z) {
            Some(id) => !mesh.regions.iter().any(|r| r.id == id),
            None => true,
        }
    })
}

/// MESHING §3.2: the whole map as one region and the same map in regions
/// give the same height at every point both meshes share (≤ 1e-5), away from
/// regions that were not built.
#[test]
fn whole_map_equals_regions_at_shared_points() {
    let style = compile(default_style()).unwrap();
    for world in worlds() {
        let whole = build_in_regions(&world, &style, 1 << 16);
        assert_eq!(whole.regions.len(), 1);
        let heights: HashMap<Key, f32> = whole
            .vertices
            .iter()
            .filter(|v| v.water_kind == SurfaceWaterKind::None)
            .map(|v| (key(v.position), v.position[1]))
            .collect();
        for size in [16, 8, 4] {
            let parts = build_in_regions(&world, &style, size);
            let (mut shared, mut total, mut worst) = (0, 0, 0f32);
            // Under fog the edge of known water against unknown cells is not a
            // contour, so its wet faces depend on the triangulation in either build.
            let observed = |v: &&SurfaceVertex| v.water_kind == SurfaceWaterKind::None && v.fog >= 0.55 && !borders_unbuilt(&parts, v.position);
            for v in parts.vertices.iter().filter(observed) {
                total += 1;
                if let Some(&y) = heights.get(&key(v.position)) {
                    shared += 1;
                    worst = worst.max((y - v.position[1]).abs());
                }
            }
            assert!(worst <= 1e-5, "height differs by {worst} at size {size}");
            // Seams move lattice points nearby; smaller regions have more seam.
            let expected = if size >= 8 { 0.85 } else { 0.75 };
            assert!(shared as f32 > expected * total as f32, "most points are shared: {shared}/{total} at size {size}");
        }
    }
}

/// River water is the channel between the bank contours, closed by caps at
/// a source: no river face reaches past a bank, so no dry vertex is pulled
/// down under the water by whichever triangle happened to cover it.
#[test]
fn river_faces_stay_inside_the_banks() {
    let style = compile(default_style()).unwrap();
    for world in worlds() {
        let mesh = build_in_regions(&world, &style, 4);
        let field = field::Landscape::new(&world, &style);
        let mut checked = 0;
        for &t in &mesh.water_triangles {
            if mesh.vertices[t[0] as usize].water_kind != SurfaceWaterKind::River {
                continue;
            }
            let p = t.map(|i| mesh.vertices[i as usize].position);
            let c = [(p[0][0] + p[1][0] + p[2][0]) / 3., (p[0][2] + p[1][2] + p[2][2]) / 3.];
            for v in p {
                // A hair inside the corner, so a vertex on the bank counts as inside.
                let q = [v[0] + (c[0] - v[0]) * 0.02, v[2] + (c[1] - v[2]) * 0.02];
                assert_ne!(field.water_at(q[0], q[1]).kind, SurfaceWaterKind::None, "river face reaches past the bank at {q:?}");
            }
            checked += 1;
        }
        assert!(checked > 50);
    }
}

/// Only regions with a known cell are built; a point in an unknown region has no surface.
#[test]
fn only_known_regions_are_built() {
    let style = compile(default_style()).unwrap();
    let world = fixtures::island();
    let mesh = build_in_regions(&world, &style, 8);
    let layout = regions::Layout::new(&world, 8);
    assert!(mesh.regions.len() < (layout.columns() * layout.rows()) as usize, "the island fixture has unknown corners");
    for region in &mesh.regions {
        assert!(layout.cells(region.id).any(|(q, r)| world.explored[(r * world.width + q) as usize] == 1));
    }
    let unbuilt = (0..layout.rows())
        .flat_map(|y| (0..layout.columns()).map(move |x| RegionId { x, y }))
        .find(|id| !mesh.regions.iter().any(|r| r.id == *id))
        .unwrap();
    let (q, r) = layout.cells(unbuilt).next().unwrap();
    let (x, z) = crate::scene::layout::tile_to_world(q, r);
    assert!(mesh.sample_ground(x, z).is_none());
}

/// The halo bounds what a region depends on: changing a cell farther than
/// `HALO_RINGS` from every cell of a region leaves the region bit for bit,
/// while a change inside it does change it.
#[test]
fn a_change_outside_the_halo_leaves_a_region_unchanged() {
    let style = compile(default_style()).unwrap();
    let world = fixtures::island();
    let size = 8;
    let layout = regions::Layout::new(&world, size);
    let target = RegionId { x: 1, y: 1 };
    let halo: HashSet<TileId> = regions::halo(&layout, target).into_iter().collect();
    let river: HashSet<u32> = world.river_edges.iter().flat_map(|&(a, b)| [a, b]).collect();
    let far = world
        .terrain
        .iter()
        .find(|c| !halo.contains(&c.tile) && world.explored[c.tile.0 as usize] == 1 && c.terrain == 1 && !river.contains(&c.tile.0))
        .expect("a known plain cell outside the halo")
        .tile;
    let near = layout.cells(target).map(|(q, r)| TileId(r * world.width + q)).find(|t| world.terrain[t.0 as usize].terrain == 1 && world.explored[t.0 as usize] == 1).unwrap();
    let region_vertices = |w: &RenderWorld| {
        let mesh = build_in_regions(w, &style, size);
        let region = mesh.regions.iter().find(|r| r.id == target).unwrap().clone();
        let mut vertices: Vec<_> = mesh.land_triangles[region.land.start as usize..region.land.end as usize]
            .iter()
            .flatten()
            .map(|&i| {
                let v = mesh.vertices[i as usize];
                (v.position.map(f32::to_bits), v.albedo.map(f32::to_bits))
            })
            .collect();
        vertices.sort();
        vertices.dedup();
        vertices
    };
    let before = region_vertices(&world);
    let mut far_hill = world.clone();
    far_hill.terrain[far.0 as usize].terrain = 2;
    assert_eq!(before, region_vertices(&far_hill), "cell {far:?} outside the halo changed the region");
    let mut near_hill = world.clone();
    near_hill.terrain[near.0 as usize].terrain = 2;
    assert_ne!(before, region_vertices(&near_hill));
}

/// Picking walks the region indexes; it must find exactly what a scan of
/// every triangle finds, near and far, at grazing and steep angles.
#[test]
fn indexed_ray_hit_matches_a_scan_of_every_triangle() {
    let style = compile(default_style()).unwrap();
    for world in worlds() {
        let mesh = build_in_regions(&world, &style, 8);
        assert!(mesh.regions.len() > 1, "the fixture spans several regions");
        let mut hits = 0;
        for i in 0..600 {
            let t = i as f32;
            let target = [(t * 0.618).fract() * 34., 0.2, (t * 0.381).fract() * 21.];
            let tilt = 0.25 + (t * 0.137).fract() * 1.2;
            let turn = (t * 0.733).fract() * std::f32::consts::TAU;
            let dir = [turn.cos() * tilt.cos(), -tilt.sin(), turn.sin() * tilt.cos()];
            let origin = std::array::from_fn(|k| target[k] - dir[k] * 30.);
            let indexed = mesh.ray_hit(origin, dir);
            assert_eq!(indexed, mesh.ray_hit_scan(origin, dir), "ray {i}");
            hits += usize::from(indexed.is_some());
        }
        assert!(hits > 300, "most rays hit the surface: {hits}");
    }
}

fn known_tile(world: &RenderWorld, x: f32, z: f32) -> Option<bool> {
    let tile = crate::scene::picking::world_to_tile(x, z, (world.width, world.height))?;
    Some(crate::scene::fog::fog_at(world, tile) >= 0.4)
}

/// Ground nobody has seen is parchment: past the fog band it is meshed with
/// the coarsest lattice level, many times coarser than known plains.
#[test]
fn unknown_ground_gets_the_coarsest_lattice() {
    let style = compile(default_style()).unwrap();
    let world = fixtures::island();
    let mesh = build_in_regions(&world, &style, REGION);
    let known: Vec<[f32; 2]> = (0..world.terrain.len())
        .filter(|&i| crate::scene::fog::fog_at(&world, TileId(i as u32)) >= 0.4)
        .map(|i| {
            let (x, z) = crate::scene::layout::tile_to_world(i as u32 % world.width, i as u32 / world.width);
            [x, z]
        })
        .collect();
    let far = |p: [f32; 3]| known.iter().all(|c| (c[0] - p[0]).hypot(c[1] - p[2]) > 3.5);
    let area = |t: &[u32; 3]| {
        let [a, b, c] = t.map(|i| mesh.vertices[i as usize].position);
        ((b[0] - a[0]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[0] - a[0])).abs() * 0.5
    };
    let (mut unknown, mut count) = (0., 0);
    for t in &mesh.land_triangles {
        if t.iter().all(|&i| far(mesh.vertices[i as usize].position)) {
            unknown += area(t);
            count += 1;
        }
    }
    assert!(count > 50, "the fixture has unknown ground inside built regions: {count}");
    // A known plain at 0.48 cells averages about 0.12 per triangle.
    assert!(unknown / count as f32 > 0.9, "mean unknown triangle area {}", unknown / count as f32);
}

/// Known water ends on the hex edges it shares with unknown cells: no water
/// face reaches into the fog.
#[test]
fn known_water_ends_on_the_fog_edge() {
    let style = compile(default_style()).unwrap();
    let world = fixtures::island();
    let mesh = build_in_regions(&world, &style, REGION);
    let mut fog_edge = 0;
    for t in &mesh.water_triangles {
        let p = t.map(|i| mesh.vertices[i as usize].position);
        let center = [(p[0][0] + p[1][0] + p[2][0]) / 3., (p[0][2] + p[1][2] + p[2][2]) / 3.];
        for v in p {
            let inner = [v[0] + (center[0] - v[0]) * 0.01, v[2] + (center[1] - v[2]) * 0.01];
            assert_eq!(known_tile(&world, inner[0], inner[1]), Some(true), "water face {p:?} reaches an unknown cell");
            let outer = [v[0] - (center[0] - v[0]) * 0.05, v[2] - (center[1] - v[2]) * 0.05];
            fog_edge += usize::from(known_tile(&world, outer[0], outer[1]) == Some(false));
        }
    }
    assert!(fog_edge > 10, "the fixture's water meets the fog: {fog_edge}");
}
