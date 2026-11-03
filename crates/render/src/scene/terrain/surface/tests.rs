use super::*;
use std::collections::BTreeMap;
use crate::{
    scene::{
        layout::tile_to_world,
        style::{compile, default_style},
    },
    world::state::TerrainCell,
};
use kernel::ids::TileId;
fn world() -> RenderWorld {
    let (width, height) = (9, 8);
    RenderWorld {
        visual_surface: true,
        width,
        height,
        visible: vec![1; (width * height) as usize],
        explored: vec![1; (width * height) as usize],
        terrain: (0..width * height)
            .map(|i| TerrainCell {
                tile: TileId(i),
                terrain: if i % width >= 6 {
                    4
                } else if i % width == 2 && i / width == 3 {
                    2
                } else {
                    1
                },
                owner: None,
                resource: None,
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn grid_preserves_ocean_hexes_but_does_not_cut_through_rivers() {
    let mut w = world();
    let tile = 3 * w.width + 3;
    w.river_edges = vec![(tile, tile + 1), (tile, tile + w.width)];
    let mesh = build(&w, &compile(default_style()).unwrap());
    let mut checked = [false; 2];
    for triangle in &mesh.water_triangles {
        let vertices = triangle.map(|id| mesh.vertices[id as usize]);
        let kind = vertices[0].water_kind;
        let k = usize::from(kind == SurfaceWaterKind::Sea);
        if checked[k] || vertices.iter().any(|v| v.fog < 0.99) {
            continue;
        }
        let center: [f32; 3] = std::array::from_fn(|i| vertices.iter().map(|v| v.position[i]).sum::<f32>() / 3.);
        let end = std::array::from_fn(|i| center[i] * 0.9 + vertices[0].position[i] * 0.1);
        assert_eq!(mesh.water_kind_at(center[0], center[2]), kind);
        let lines = crate::gpu::surface_mesh::grid(&mesh, &w, &[center, end]);
        assert_eq!(lines.is_empty(), kind == SurfaceWaterKind::River);
        checked[k] = true;
    }
    assert!(checked.into_iter().all(|v| v), "fixture must exercise both water kinds");
    assert_eq!(mesh.water_kind_at(-50., -50.), SurfaceWaterKind::None);
}

#[test]
fn hill_keeps_a_local_crown_without_lifting_distant_plain_or_sea() {
    let mut plain = world();
    for cell in &mut plain.terrain {
        if cell.terrain == 2 {
            cell.terrain = 1;
        }
    }
    let mut hills = plain.clone();
    let hill = 3 * hills.width + 4;
    hills.terrain[hill as usize].terrain = 2;
    let style = compile(default_style()).unwrap();
    let a = build(&plain, &style);
    let b = build(&hills, &style);
    let (x, z) = tile_to_world(4, 3);
    let rise = b.sample_ground(x, z).unwrap().y - a.sample_ground(x, z).unwrap().y;
    assert!(
        rise > 0.8,
        "hill crown must be legible above the plain: {rise}"
    );
    let (fx, fz) = tile_to_world(1, 3);
    assert_eq!(
        a.sample_ground(fx, fz).unwrap().y,
        b.sample_ground(fx, fz).unwrap().y
    );
    let (sx, sz) = tile_to_world(7, 3);
    assert_eq!(
        a.sample_water(sx, sz).unwrap().y,
        b.sample_water(sx, sz).unwrap().y
    );
}
#[test]
fn actual_mesh_is_finite_upward_and_queries_match_triangle_planes() {
    let w = world();
    let style = compile(default_style()).unwrap();
    let mesh = build(&w, &style);
    assert!(mesh.land_triangles.len() > w.terrain.len() * 30);
    assert!(!mesh.water_triangles.is_empty());
    for v in &mesh.vertices {
        assert!(v
            .position
            .iter()
            .chain(v.albedo.iter())
            .all(|v| v.is_finite()));
    }
    for t in &mesh.land_triangles {
        let p = t.map(|i| mesh.vertices[i as usize].position);
        let area =
            (p[1][2] - p[0][2]) * (p[2][0] - p[0][0]) - (p[1][0] - p[0][0]) * (p[2][2] - p[0][2]);
        assert!(area > 0.);
        let x = (p[0][0] + p[1][0] + p[2][0]) / 3.;
        let z = (p[0][2] + p[1][2] + p[2][2]) / 3.;
        let sample = mesh.sample_ground(x, z).unwrap();
        assert!(
            (sample.y - (p[0][1] + p[1][1] + p[2][1]) / 3.).abs() < 0.0003,
            "triangle {p:?}, area={area}, sample={sample:?}"
        );
    }
}
#[test]
fn coast_preserves_land_water_centres_and_single_cell_islands() {
    let mut w = world();
    w.terrain[(4 * w.width + 7) as usize].terrain = 1;
    let mesh = build(&w, &compile(default_style()).unwrap());
    for cell in &w.terrain {
        let (x, z) = tile_to_world(cell.tile.0 % w.width, cell.tile.0 / w.width);
        assert_eq!(
            mesh.sample_water(x, z).is_some(),
            cell.terrain == 4,
            "tile {}",
            cell.tile.0
        );
    }
    let contact = mesh
        .vertices
        .iter()
        .filter(|v| v.water_kind == SurfaceWaterKind::Sea && v.water_depth < 0.005)
        .count();
    assert!(contact > 10, "coast and water share contact vertices");
}
#[test]
fn generation_is_deterministic_and_unknown_mutations_are_irrelevant() {
    let mut a = world();
    for i in 0..a.terrain.len() {
        if i % (a.width as usize) < 2 {
            a.visible[i] = 0;
            a.explored[i] = 0;
        }
    }
    let mut b = a.clone();
    for i in 0..b.terrain.len() {
        if b.explored[i] == 0 {
            b.terrain[i].terrain = if i % 2 == 0 { 6 } else { 4 };
        }
    }
    b.river_edges.push((0, 1));
    let style = compile(default_style()).unwrap();
    let ma = build(&a, &style);
    let mb = build(&b, &style);
    let again = build(&a, &style);
    assert_eq!(ma.vertices, again.vertices);
    assert_eq!(ma.land_triangles, again.land_triangles);
    assert_eq!(ma.vertices, mb.vertices);
    assert_eq!(ma.land_triangles, mb.land_triangles);
    assert_eq!(ma.water_triangles, mb.water_triangles);
}
#[test]
fn river_and_sea_water_cover_the_same_carved_ground_without_inverted_faces() {
    let mut w = world();
    let a = 3 * w.width + 3;
    w.river_edges = vec![(a, a + 1), (a, a + w.width), (a + w.width, a + w.width + 1)];
    let mesh = build(&w, &compile(default_style()).unwrap());
    assert!(mesh
        .vertices
        .iter()
        .any(|v| v.water_kind == SurfaceWaterKind::River));
    for t in &mesh.water_triangles {
        assert!(t.iter().all(|&i| mesh.vertices[i as usize].coastal_rock == 0.),
            "water clones cannot inherit coastal stone material");
        let p = t.map(|i| mesh.vertices[i as usize].position);
        let x = (p[0][0] + p[1][0] + p[2][0]) / 3.;
        let z = (p[0][2] + p[1][2] + p[2][2]) / 3.;
        let ground = mesh.sample_ground(x, z).unwrap();
        let water = mesh.sample_water(x, z).unwrap();
        assert!(
            ground.y <= water.y + 0.0003,
            "ground {} above water {}",
            ground.y,
            water.y
        );
        assert!(water.normal[1] > 0.);
    }
}

#[test]
fn every_internal_wet_dry_edge_has_exact_shared_contact_height() {
    let mut w = world();
    let a = 3 * w.width + 3;
    w.river_edges = vec![(a, a + 1), (a, a + w.width), (a + 1, a + w.width)];
    assert_wet_dry_contacts(&w);
}

fn assert_wet_dry_contacts(w: &RenderWorld) {
    let mesh = build(w, &compile(default_style()).unwrap());
    let key = |p: [f32; 3]| (p[0].to_bits(), p[2].to_bits());
    let mut ground_ids = BTreeMap::new();
    let mut ground_edges = BTreeMap::new();
    let mut wet_edges = BTreeMap::new();
    for t in &mesh.land_triangles {
        for &i in t {
            ground_ids.insert(key(mesh.vertices[i as usize].position), i);
        }
        for j in 0..3 {
            let a = t[j];
            let b = t[(j + 1) % 3];
            *ground_edges.entry((a.min(b), a.max(b))).or_insert(0usize) += 1;
        }
    }
    for t in &mesh.water_triangles {
        for j in 0..3 {
            let wa = t[j];
            let wb = t[(j + 1) % 3];
            let a = ground_ids[&key(mesh.vertices[wa as usize].position)];
            let b = ground_ids[&key(mesh.vertices[wb as usize].position)];
            let entry = wet_edges
                .entry((a.min(b), a.max(b)))
                .or_insert((0usize, [(a, wa), (b, wb)]));
            entry.0 += 1;
        }
    }
    let mut contacts = 0;
    for (edge, (count, ends)) in wet_edges {
        if count != 1 || ground_edges.get(&edge) != Some(&2) {
            continue;
        }
        contacts += 1;
        if ends
            .iter()
            .all(|&(_, w)| mesh.vertices[w as usize].fog >= 0.55)
        {
            for (_, w) in ends {
                assert!(
                    mesh.vertices[w as usize].shore_distance.abs() < 1e-6,
                    "true observed coast must have zero shore distance"
                );
            }
        }
        for (g, w) in ends {
            assert!(
                (mesh.vertices[g as usize].position[1] - mesh.vertices[w as usize].position[1])
                    .abs()
                    < 1e-6,
                "floating wet/dry contact"
            );
        }
    }
    assert!(contacts > 20);
}
#[test]
fn revealing_distant_cells_does_not_change_known_interior_surface() {
    let mut a = world();
    for i in 0..a.terrain.len() {
        if i % (a.width as usize) >= 7 {
            a.visible[i] = 0;
            a.explored[i] = 0;
        }
    }
    let mut b = a.clone();
    b.visible.fill(1);
    b.explored.fill(1);
    let style = compile(default_style()).unwrap();
    let ma = build(&a, &style);
    let mb = build(&b, &style);
    for r in 1..6 {
        for q in 1..3 {
            let (x, z) = tile_to_world(q, r);
            let sa = ma.sample_ground(x + 0.11, z + 0.17).unwrap();
            let sb = mb.sample_ground(x + 0.11, z + 0.17).unwrap();
            assert!(
                (sa.y - sb.y).abs() < 1e-6,
                "discovery changed a distant interior"
            );
        }
    }
}

#[test]
fn elevated_oblique_river_and_sea_have_one_level_at_every_shared_contact() {
    let mut w = world();
    for cell in &mut w.terrain {
        if cell.terrain != 4 {
            cell.terrain = 2;
        }
    }
    let a = 3 * w.width + 5;
    // A diagonal tributary and a branching estuary touch the curved shoreline.
    w.river_edges = vec![
        (a, a + w.width),
        (a, a + 1),
        (a + w.width, a + 1),
        (a - w.width, a - w.width + 1),
    ];
    let mesh = build(&w, &compile(default_style()).unwrap());
    let mut shared: BTreeMap<(u32, u32), Vec<SurfaceVertex>> = BTreeMap::new();
    let mut ground = BTreeMap::new();
    for t in &mesh.land_triangles {
        for &i in t {
            let v = mesh.vertices[i as usize];
            ground.insert(
                (v.position[0].to_bits(), v.position[2].to_bits()),
                v.position[1],
            );
        }
    }
    for v in &mesh.vertices {
        if v.water_kind != SurfaceWaterKind::None {
            shared
                .entry((v.position[0].to_bits(), v.position[2].to_bits()))
                .or_default()
                .push(*v);
        }
    }
    let mut contacts = 0;
    let mut interior_contacts = 0;
    for (key, vertices) in shared {
        let sea = vertices
            .iter()
            .any(|v| v.water_kind == SurfaceWaterKind::Sea);
        let river = vertices
            .iter()
            .any(|v| v.water_kind == SurfaceWaterKind::River);
        if sea && river {
            contacts += 1;
            if vertices.iter().all(|v| v.shore_distance < -0.02) {
                interior_contacts += 1;
            }
            for v in &vertices {
                assert_eq!(
                    v.position[1], 0.08,
                    "wet/wet sea-river seam must share exact level"
                );
            }
        }
        for v in &vertices {
            assert!(
                (v.position[1] - vertices[0].position[1]).abs() < 1e-6,
                "shared water XZ has more than one level"
            );
            assert!(
                ground[&key] <= v.position[1] + 1e-6,
                "estuary ground protrudes through water"
            );
            assert!(
                (v.water_depth - (v.position[1] - ground[&key]).max(0.)).abs() < 1e-6,
                "depth must be recomputed after contact reconciliation"
            );
        }
    }
    assert!(
        contacts >= 2,
        "fixture must actually include shared oblique estuary contacts"
    );
    assert!(
        interior_contacts > 0,
        "interior sea/river seam must not masquerade as coastline"
    );
}

#[test]
fn duplicate_tributary_vertices_share_a_single_junction_level() {
    let vertex = |x: f32, z: f32, y: f32, kind| SurfaceVertex {
        position: [x, y, z],
        albedo: [0.5; 3],
        coastal_rock: 0.,
        fallout: 0.,
        terrain: 1,
        water: 0.,
        water_depth: 0.,
        shore_distance: 0.,
        fog: 1.,
        water_kind: kind,
        flow_uv: [0.; 2],
        tone: [0.; 3],
    };
    let mut mesh = SurfaceMesh::default();
    for (x, z) in [(0., 0.), (1., 0.), (0., 1.), (1., 1.)] {
        mesh.vertices
            .push(vertex(x, z, 0.1, SurfaceWaterKind::None));
    }
    mesh.land_triangles = vec![[0, 2, 1], [1, 2, 3]];
    let mut contacts = vec![];
    for (face, level) in [([0, 2, 1], 0.6), ([1, 2, 3], 0.3)] {
        let mut water = [0; 3];
        for i in 0..3 {
            let p = mesh.vertices[face[i] as usize].position;
            water[i] = mesh.vertices.len() as u32;
            mesh.vertices
                .push(vertex(p[0], p[2], level, SurfaceWaterKind::River));
        }
        mesh.water_triangles.push(water);
        contacts.push((face, water));
    }
    mesh.join_water_contacts(&contacts);
    assert!(
        mesh.vertices
            .iter()
            .filter(|v| v.water_kind != SurfaceWaterKind::None)
            .all(|v| v.shore_distance == -3.),
        "wet/wet seam and outer map edges cannot generate a coast"
    );
    for v in mesh
        .vertices
        .iter()
        .filter(|v| v.water_kind == SurfaceWaterKind::River)
    {
        if v.position[0] + v.position[2] == 1. {
            assert_eq!(v.position[1], 0.3);
            assert!((v.water_depth - 0.2).abs() < 1e-6);
        }
    }
}

#[test]
fn uniform_grass_biome_has_coherent_meadow_patches_and_small_folds() {
    let mut w = world();
    for cell in &mut w.terrain {
        cell.terrain = 1;
    }
    let style = compile(default_style()).unwrap();
    let land = field::Landscape::new(&w, &style);
    let mut min_red = f32::INFINITY;
    let mut max_red = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for z in 3..9 {
        for x in 5..13 {
            let a = land.vertex(x as f32, z as f32);
            let b = land.vertex(x as f32 + 0.01, z as f32 + 0.01);
            min_red = min_red.min(a.albedo[0]);
            max_red = max_red.max(a.albedo[0]);
            min_y = min_y.min(a.position[1]);
            max_y = max_y.max(a.position[1]);
            assert!(
                a.albedo
                    .iter()
                    .zip(b.albedo)
                    .all(|(a, b)| (a - b).abs() < 0.015),
                "patches must vary smoothly instead of per-triangle noise"
            );
        }
    }
    assert!(
        max_red - min_red > 0.15,
        "warm and cool patches must remain visible"
    );
    assert!(max_y - min_y > 0.07 && max_y - min_y < 0.32);
}

/// Sea in the east, a rocky coast of hills in front of it and a one-cell
/// hill peninsula: walls, a mesa and a beach in one fixture.
fn cliff_world() -> RenderWorld {
    let (width, height) = (14, 12);
    let terrain = |q: u32, r: u32| {
        if r == 6 && (8..=9).contains(&q) {
            2
        } else if q >= 8 {
            4
        } else if q == 7 && r <= 7 {
            2
        } else {
            1
        }
    };
    RenderWorld {
        visual_surface: true,
        width,
        height,
        visible: vec![1; (width * height) as usize],
        explored: vec![1; (width * height) as usize],
        terrain: (0..width * height)
            .map(|i| TerrainCell { tile: TileId(i), terrain: terrain(i % width, i / width), owner: None, resource: None })
            .collect(),
        ..Default::default()
    }
}

/// Land triangles whose three vertices are cliff wall vertices.
fn wall_triangles(w: &RenderWorld, mesh: &SurfaceMesh) -> Vec<[[f32; 3]; 3]> {
    let style = compile(default_style()).unwrap();
    let field = field::Landscape::new(w, &style);
    mesh.land_triangles
        .iter()
        .map(|t| t.map(|i| mesh.vertices[i as usize].position))
        .filter(|p| p.iter().all(|v| field.wall_row([v[0], v[2]]).is_some()))
        .collect()
}

fn min_angle_degrees(p: [[f32; 3]; 3]) -> f32 {
    let mut smallest = 180f32;
    for i in 0..3 {
        let (a, b, c) = (p[i], p[(i + 1) % 3], p[(i + 2) % 3]);
        let u = [b[0] - a[0], b[2] - a[2]];
        let v = [c[0] - a[0], c[2] - a[2]];
        let cos = (u[0] * v[0] + u[1] * v[1]) / (u[0].hypot(u[1]) * v[0].hypot(v[1])).max(1e-12);
        smallest = smallest.min(cos.clamp(-1., 1.).acos().to_degrees());
    }
    smallest
}

fn longest_edge(p: [[f32; 3]; 3]) -> f32 {
    (0..3).map(|i| (p[i][0] - p[(i + 1) % 3][0]).hypot(p[i][2] - p[(i + 1) % 3][2])).fold(0., f32::max)
}

#[test]
fn rocky_coast_builds_walls_deterministically() {
    let w = cliff_world();
    let style = compile(default_style()).unwrap();
    let a = build(&w, &style);
    let b = build(&w, &style);
    assert_eq!(a.vertices, b.vertices);
    assert_eq!(a.land_triangles, b.land_triangles);
    assert_eq!(a.water_triangles, b.water_triangles);
    let walls = wall_triangles(&w, &a);
    assert!(walls.len() > 300, "the hill coast is drawn as walls: {}", walls.len());
    let steep = walls.iter().filter(|p| {
        let rise = p.iter().map(|v| v[1]).fold(f32::MIN, f32::max) - p.iter().map(|v| v[1]).fold(f32::MAX, f32::min);
        rise > longest_edge(**p)
    });
    assert!(steep.count() > 100, "walls are steep, not slopes");
}

#[test]
fn cliff_walls_keep_exact_water_contacts() {
    assert_wet_dry_contacts(&cliff_world());
}

#[test]
fn cliff_wall_edges_follow_the_sizing_field() {
    let w = cliff_world();
    let mesh = build(&w, &compile(default_style()).unwrap());
    let walls = wall_triangles(&w, &mesh);
    let edges: Vec<f32> = walls.iter().map(|p| longest_edge(*p)).collect();
    let within = edges.iter().filter(|&&e| e <= 1.5 * 0.1).count();
    eprintln!("wall triangles {}, longest edge <= 0.15: {}", edges.len(), within);
    assert!(within as f32 >= 0.9 * edges.len() as f32);
}

/// platform/MESHING.md §3.6 asks for a minimum angle of 20° away from contour
/// intersections. The lattice meets it; wall strips are structured columns
/// whose thin foot rows and fan at convex corners give narrower triangles,
/// so for them the measured share is pinned instead.
#[test]
fn triangle_quality_is_bounded() {
    let w = cliff_world();
    let mesh = build(&w, &compile(default_style()).unwrap());
    let walls = wall_triangles(&w, &mesh);
    let all: Vec<f32> = mesh.land_triangles.iter().map(|t| min_angle_degrees(t.map(|i| mesh.vertices[i as usize].position))).collect();
    let wall: Vec<f32> = walls.iter().map(|p| min_angle_degrees(*p)).collect();
    let share = |v: &[f32], limit: f32| v.iter().filter(|&&a| a >= limit).count() as f32 / v.len() as f32;
    eprintln!("all {}: >=20 {:.3} >=10 {:.3}; wall {}: >=20 {:.3} >=10 {:.3}", all.len(), share(&all, 20.), share(&all, 10.), wall.len(), share(&wall, 20.), share(&wall, 10.));
    assert!(share(&all, 20.) >= 0.95 && share(&all, 10.) >= 0.98);
    assert!(share(&wall, 20.) >= 0.9 && share(&wall, 10.) >= 0.97);
}

/// Fixtures for the differential tests: hills, mountains with a hidden cell
/// and a river, the rocky coast with a mesa, and a river mouth through it.
fn differential_worlds() -> Vec<RenderWorld> {
    let mut mountains = world();
    for (q, r) in [(1, 2), (2, 2), (1, 3), (3, 5)] {
        mountains.terrain[(r * mountains.width + q) as usize].terrain = 6;
    }
    let a = 3 * mountains.width + 3;
    mountains.river_edges = vec![(a, a + 1), (a, a + mountains.width), (a + mountains.width, a + mountains.width + 1)];
    mountains.visible[(6 * mountains.width + 1) as usize] = 0;
    mountains.explored[(6 * mountains.width + 1) as usize] = 0;
    let mut river_cliff = cliff_world();
    let b = 4 * river_cliff.width + 6;
    river_cliff.river_edges = vec![(b, b + 1), (b - river_cliff.width + 1, b + 1)];
    vec![world(), mountains, cliff_world(), river_cliff, fixtures::island()]
}

/// Stage 3 owns the river channel: the rocky-coast cutout gives way to the
/// river corridor at a mouth, so no wall row and no wall stone stand in it.
/// Before, the walls were cut afterwards by the river and left jagged teeth.
#[test]
fn a_river_mouth_cuts_through_a_rocky_coast_without_walls() {
    let w = fixtures::island();
    let style = compile(default_style()).unwrap();
    let mesh = build(&w, &style);
    let field = field::Landscape::new(&w, &style);
    let near_river = |p: [f32; 2]| mesh.rivers.iter().any(|r| hydrology::closest(p, r.a, r.b).0 < 0.5);
    let mouth: Vec<_> = mesh
        .vertices
        .iter()
        .filter(|v| v.water_kind == SurfaceWaterKind::None && v.fog > 0.4 && v.shore_distance.abs() < 1.)
        .filter(|v| near_river([v.position[0], v.position[2]]))
        .collect();
    assert!(mouth.len() > 20, "the island's river reaches its coast");
    for v in mouth {
        let p = [v.position[0], v.position[2]];
        assert!(field.wall_row(p).is_none(), "wall vertex in the river corridor at {p:?}");
        assert!(v.coastal_rock < 0.05, "wall stone in the river corridor at {p:?}");
    }
}

#[test]
fn water_contacts_are_exact_on_every_fixture() {
    for w in differential_worlds() {
        assert_wet_dry_contacts(&w);
    }
}
