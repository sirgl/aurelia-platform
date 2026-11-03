//! Screen rays intersect presentation triangles, then map the hit XZ to a logical hex.
//! `pick_tile` retains the legacy iterative-height approximation for old callers.

use crate::camera::CameraController;
use crate::scene::{layout::tile_to_world, terrain::surface::SurfaceMesh};
use kernel::ids::TileId;

/// A ray from a screen point (pixels, origin top-left, as in winit).
/// Returns (origin, dir) in world coordinates.
pub fn screen_ray(
    camera: &CameraController,
    aspect: f32,
    screen: (f32, f32),
    size: (f32, f32),
) -> ([f32; 3], [f32; 3]) {
    use glam::{Mat4, Vec4};
    let vp = Mat4::from_cols_array_2d(&camera.view_proj(aspect));
    let inv = vp.inverse();
    let nx = (screen.0 / size.0) * 2.0 - 1.0;
    let ny = 1.0 - (screen.1 / size.1) * 2.0;
    let to_world = |x: f32, y: f32, z: f32| {
        let p = inv * Vec4::new(x, y, z, 1.0);
        [p.x / p.w, p.y / p.w, p.z / p.w]
    };
    let near = to_world(nx, ny, 0.0);
    let far = to_world(nx, ny, 1.0);
    let dir = [far[0] - near[0], far[1] - near[1], far[2] - near[2]];
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2])
        .sqrt()
        .max(1e-6);
    (near, [dir[0] / len, dir[1] / len, dir[2] / len])
}

/// Tile centre on screen (for roundtrip tests and future UI markers).
pub fn tile_to_screen(
    camera: &CameraController,
    aspect: f32,
    world: (f32, f32, f32),
    size: (f32, f32),
) -> (f32, f32) {
    use glam::{Mat4, Vec4};
    let vp = Mat4::from_cols_array_2d(&camera.view_proj(aspect));
    let p = vp * Vec4::new(world.0, world.1, world.2, 1.0);
    let (nx, ny) = (p.x / p.w, p.y / p.w);
    ((nx + 1.0) * 0.5 * size.0, (1.0 - ny) * 0.5 * size.1)
}

/// Exact nearest hit on the rendered land or water surface, mapped to a logical hex.
/// Visibility filtering belongs to the caller; this function does not query game state.
pub fn pick_surface(
    camera: &CameraController,
    aspect: f32,
    screen: (f32, f32),
    size: (f32, f32),
    map: (u32, u32),
    surface: &SurfaceMesh,
) -> Option<TileId> {
    pick_surface_with_features(camera, aspect, screen, size, map, surface, &[])
}

/// Picks the nearest rendered terrain or feature triangle. `triangles` contains
/// consecutive world-space triangle vertices, already filtered by the renderer.
pub fn pick_surface_with_features(
    camera: &CameraController,
    aspect: f32,
    screen: (f32, f32),
    size: (f32, f32),
    map: (u32, u32),
    surface: &SurfaceMesh,
    triangles: &[[f32; 3]],
) -> Option<TileId> {
    if map.0 == 0
        || map.1 == 0
        || size.0 <= 0.0
        || size.1 <= 0.0
        || ![aspect, screen.0, screen.1, size.0, size.1]
            .iter()
            .all(|v| v.is_finite())
        || aspect <= 0.0
    {
        return None;
    }
    let (origin, dir) = screen_ray(camera, aspect, screen, size);
    let hit = ray_surface_hit(origin, dir, surface, triangles)?;
    world_to_tile(hit[0], hit[2], map)
}

fn ray_surface_hit(
    origin: [f32; 3],
    dir: [f32; 3],
    surface: &SurfaceMesh,
    features: &[[f32; 3]],
) -> Option<[f32; 3]> {
    // The surface answers through its region indexes (terrain/surface.rs).
    let mut nearest = surface.ray_hit(origin, dir).unwrap_or(f32::INFINITY);
    for triangle in features.chunks_exact(3) {
        if let Some(distance) =
            ray_triangle_distance(origin, dir, [triangle[0], triangle[1], triangle[2]])
        {
            nearest = nearest.min(distance);
        }
    }
    nearest
        .is_finite()
        .then(|| std::array::from_fn(|i| origin[i] + dir[i] * nearest))
}

pub(crate) fn ray_triangle_distance(origin: [f32; 3], dir: [f32; 3], triangle: [[f32; 3]; 3]) -> Option<f32> {
    use glam::Vec3;
    let [a, b, c] = triangle.map(Vec3::from_array);
    let ab = b - a;
    let ac = c - a;
    let cross = Vec3::from_array(dir).cross(ac);
    let determinant = ab.dot(cross);
    // Two-sided: surface winding does not change which visible point is selected.
    if !determinant.is_finite() || determinant.abs() < 1e-8 {
        return None;
    }
    let offset = Vec3::from_array(origin) - a;
    let u = offset.dot(cross) / determinant;
    let q = offset.cross(ab);
    let v = Vec3::from_array(dir).dot(q) / determinant;
    // Small barycentric tolerance keeps a shared edge pickable on both triangles.
    if u < -1e-5 || v < -1e-5 || u + v > 1.00001 {
        return None;
    }
    let distance = ac.dot(q) / determinant;
    (distance.is_finite() && distance >= 0.0).then_some(distance)
}

/// Screen -> tile. height: the surface height of the tile (for iterations).
/// Returns None off the map.
pub fn pick_tile(
    camera: &CameraController,
    aspect: f32,
    screen: (f32, f32),
    size: (f32, f32),
    map: (u32, u32),
    height: impl Fn(TileId) -> f32,
) -> Option<TileId> {
    let (origin, dir) = screen_ray(camera, aspect, screen, size);
    if dir[1].abs() < 1e-6 {
        return None;
    }
    // Iterations: the mean-height plane -> tile -> its height -> again.
    let mut y = 0.35;
    let mut tile = None;
    for _ in 0..3 {
        let t = (y - origin[1]) / dir[1];
        if t < 0.0 {
            return None;
        }
        let x = origin[0] + dir[0] * t;
        let z = origin[2] + dir[2] * t;
        tile = world_to_tile(x, z, map);
        match tile {
            Some(id) => {
                let h = height(id);
                if (h - y).abs() < 0.05 {
                    break;
                }
                y = h;
            }
            None => break,
        }
    }
    tile
}

/// World XZ -> tile (nearest hex: coarse rounding + a check of neighbours).
pub fn world_to_tile(x: f32, z: f32, map: (u32, u32)) -> Option<TileId> {
    // The nearest centre is the cube-rounded axial coordinate; a point on a
    // shared edge or corner is equally near a neighbour of it. Of the centres
    // within the circumradius (1.0) the nearest wins, ties to the lowest row,
    // then column: the hex containing the point, one answer on its border.
    const SQRT3: f32 = 1.7320508;
    let (w, h) = (map.0 as i32, map.1 as i32);
    let (qf, rf) = (x / SQRT3 - z / 3.0, z / 1.5);
    let (sf, mut q, mut r) = (-qf - rf, qf.round(), rf.round());
    let s = sf.round();
    let (dq, dr, ds) = ((q - qf).abs(), (r - rf).abs(), (s - sf).abs());
    if dq > dr && dq > ds {
        q = -r - s;
    } else if dr > ds {
        r = -q - s;
    }
    let (q0, r0) = (q as i32, r as i32);
    let mut best: Option<(f32, i32, i32)> = None;
    for (dq, dr) in [(0, 0), (1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)] {
        let (q, r) = (q0 + dq, r0 + dr);
        if q < 0 || r < 0 || q >= w || r >= h {
            continue;
        }
        let (cx, cz) = tile_to_world(q as u32, r as u32);
        let d = (x - cx).powi(2) + (z - cz).powi(2);
        if d > 1.0 {
            continue;
        }
        let better = match best {
            None => true,
            Some((bd, br, bq)) => d < bd || (d == bd && (r, q) < (br, bq)),
        };
        if better {
            best = Some((d, r, q));
        }
    }
    best.map(|(_, r, q)| TileId(r as u32 * map.0 + q as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> CameraController {
        let mut c = CameraController::fit_map(16, 16);
        c.zoom = 1.0;
        c
    }

    fn append_triangle(surface: &mut SurfaceMesh, positions: [[f32; 3]; 3], water: bool) {
        use crate::scene::terrain::surface::{SurfaceVertex, SurfaceWaterKind};
        let first = surface.vertices.len() as u32;
        surface
            .vertices
            .extend(positions.map(|position| SurfaceVertex {
                position,
                albedo: [1.0; 3],
                coastal_rock: 0.,
                fallout: 0.,
                terrain: 0,
                water: if water { 1.0 } else { 0.0 },
                water_depth: 0.0,
                shore_distance: 0.0,
                fog: 1.0,
                water_kind: if water {
                    SurfaceWaterKind::Sea
                } else {
                    SurfaceWaterKind::None
                },
                flow_uv: [0.0; 2],
                tone: [0.; 3],
            }));
        let triangles = if water {
            &mut surface.water_triangles
        } else {
            &mut surface.land_triangles
        };
        triangles.push([first, first + 1, first + 2]);
    }
    fn horizontal_triangle(center: [f32; 3]) -> [[f32; 3]; 3] {
        let [x, y, z] = center;
        [
            [x - 0.7, y, z - 0.5],
            [x + 0.7, y, z - 0.5],
            [x, y, z + 1.0],
        ]
    }

    #[test]
    fn surface_pick_projected_centers_at_different_elevations() {
        let camera = cam();
        let size = (800.0, 600.0);
        for (q, r, y) in [(2, 2, 0.15), (8, 8, 4.2), (7, 3, 1.8)] {
            let (x, z) = tile_to_world(q, r);
            let mut surface = SurfaceMesh::default();
            append_triangle(&mut surface, horizontal_triangle([x, y, z]), false);
            let screen = tile_to_screen(&camera, size.0 / size.1, (x, y, z), size);
            assert_eq!(
                pick_surface(&camera, size.0 / size.1, screen, size, (16, 16), &surface),
                Some(TileId(r * 16 + q))
            );
        }
    }

    #[test]
    fn surface_pick_hits_actual_slope_without_tile_height_approximation() {
        let camera = cam();
        let (x, z) = tile_to_world(8, 8);
        let mut surface = SurfaceMesh::default();
        append_triangle(
            &mut surface,
            [
                [x - 0.7, 1.0, z - 0.5],
                [x + 0.7, 4.0, z - 0.5],
                [x, 7.0, z + 1.0],
            ],
            false,
        );
        let screen = tile_to_screen(&camera, 4.0 / 3.0, (x, 4.0, z), (800.0, 600.0));
        let (origin, dir) = screen_ray(&camera, 4.0 / 3.0, screen, (800.0, 600.0));
        let hit = ray_surface_hit(origin, dir, &surface, &[]).unwrap();
        assert!(
            (hit[1] - 4.0).abs() < 0.001,
            "hit actual triangle elevation: {hit:?}"
        );
        assert_eq!(
            pick_surface(
                &camera,
                4.0 / 3.0,
                screen,
                (800.0, 600.0),
                (16, 16),
                &surface
            ),
            Some(TileId(8 * 16 + 8))
        );
    }

    #[test]
    fn surface_pick_chooses_water_before_submerged_ground() {
        let camera = cam();
        let (x, z) = tile_to_world(8, 8);
        let water = [x, 3.0, z];
        let screen = tile_to_screen(&camera, 4.0 / 3.0, (x, 3.0, z), (800.0, 600.0));
        let (origin, dir) = screen_ray(&camera, 4.0 / 3.0, screen, (800.0, 600.0));
        let ground = std::array::from_fn(|i| water[i] + dir[i] * 6.0);
        let mut surface = SurfaceMesh::default();
        append_triangle(&mut surface, horizontal_triangle(ground), false);
        assert_ne!(
            world_to_tile(ground[0], ground[2], (16, 16)),
            Some(TileId(8 * 16 + 8))
        );
        append_triangle(&mut surface, horizontal_triangle(water), true);
        let hit = ray_surface_hit(origin, dir, &surface, &[]).unwrap();
        assert!((hit[1] - 3.0).abs() < 0.001);
        assert_eq!(
            pick_surface(
                &camera,
                4.0 / 3.0,
                screen,
                (800.0, 600.0),
                (16, 16),
                &surface
            ),
            Some(TileId(8 * 16 + 8))
        );
    }

    #[test]
    fn raised_mountain_triangle_occludes_background_hex_under_tilted_camera() {
        let camera = cam();
        let size = (800.0, 600.0);
        let (x, z) = tile_to_world(8, 8);
        let screen = tile_to_screen(&camera, 4.0 / 3.0, (x, 4.5, z), size);
        let (origin, dir) = screen_ray(&camera, 4.0 / 3.0, screen, size);
        let mountain = [x, 4.5, z];
        let background = std::array::from_fn(|i| mountain[i] + dir[i] * 8.0);
        let mut surface = SurfaceMesh::default();
        append_triangle(&mut surface, horizontal_triangle(background), false);
        let background_tile = world_to_tile(background[0], background[2], (16, 16));
        assert!(background_tile.is_some());
        assert_ne!(background_tile, Some(TileId(8 * 16 + 8)));
        assert_eq!(
            pick_surface(&camera, 4.0 / 3.0, screen, size, (16, 16), &surface),
            background_tile
        );
        let slope = [
            [x - 0.6, 3.0, z + 0.3],
            [x + 0.6, 3.0, z + 0.3],
            [x, 7.5, z - 0.6],
        ];
        assert_eq!(
            pick_surface_with_features(
                &camera,
                4.0 / 3.0,
                screen,
                size,
                (16, 16),
                &surface,
                &slope
            ),
            Some(TileId(8 * 16 + 8))
        );
        let hit = ray_surface_hit(origin, dir, &surface, &slope).unwrap();
        assert!((hit[1] - 4.5).abs() < 0.001);
    }

    #[test]
    fn surface_pick_miss_does_not_fall_back_to_a_flat_map_plane() {
        let camera = cam();
        let size = (800.0, 600.0);
        let (x, z) = tile_to_world(8, 8);
        let mut surface = SurfaceMesh::default();
        append_triangle(&mut surface, horizontal_triangle([x, 2.0, z]), false);
        let (miss_x, miss_z) = tile_to_world(2, 2);
        let screen = tile_to_screen(&camera, 4.0 / 3.0, (miss_x, 2.0, miss_z), size);
        assert_eq!(
            pick_surface(&camera, 4.0 / 3.0, screen, size, (16, 16), &surface),
            None
        );
        let mut outside = SurfaceMesh::default();
        append_triangle(
            &mut outside,
            horizontal_triangle([-20.0, 2.0, -20.0]),
            false,
        );
        let screen = tile_to_screen(&camera, 4.0 / 3.0, (-20.0, 2.0, -20.0), size);
        assert_eq!(
            pick_surface(&camera, 4.0 / 3.0, screen, size, (16, 16), &outside),
            None
        );
    }

    #[test]
    fn surface_pick_rejects_misses_behind_origin_and_degenerate_triangles() {
        let triangle = horizontal_triangle([0.0, 0.0, 0.0]);
        assert_eq!(
            ray_triangle_distance([5.0, 2.0, 0.0], [0.0, -1.0, 0.0], triangle),
            None
        );
        assert_eq!(
            ray_triangle_distance([0.0, 2.0, 0.0], [0.0, 1.0, 0.0], triangle),
            None
        );
        assert_eq!(
            ray_triangle_distance([0.0, 2.0, 0.0], [1.0, 0.0, 0.0], triangle),
            None
        );
        assert_eq!(
            ray_triangle_distance([0.0, 2.0, 0.0], [0.0, -1.0, 0.0], [[0.0; 3]; 3]),
            None
        );
        assert_eq!(
            ray_surface_hit(
                [0.0, 2.0, 0.0],
                [0.0, -1.0, 0.0],
                &SurfaceMesh::default(),
                &[]
            ),
            None
        );
    }

    fn flat_height(_: TileId) -> f32 {
        0.35
    }

    #[test]
    fn roundtrip_center_tiles() {
        // Tile -> screen -> tile: the centre of each sampled tile comes back.
        let camera = cam();
        let size = (800.0, 600.0);
        let aspect = size.0 / size.1;
        for (q, r) in [(2, 2), (8, 8), (0, 0), (15, 15), (7, 3)] {
            let (wx, wz) = tile_to_world(q, r);
            let s = tile_to_screen(&camera, aspect, (wx, 0.35, wz), size);
            let back = pick_tile(&camera, aspect, s, size, (16, 16), flat_height);
            assert_eq!(back, Some(TileId(r * 16 + q)), "tile ({},{})", q, r);
        }
    }

    #[test]
    fn screen_center_hits_map() {
        // The camera looks at the map centre: the screen centre must land in a tile.
        let camera = cam();
        let back = pick_tile(
            &camera,
            800.0 / 600.0,
            (400.0, 300.0),
            (800.0, 600.0),
            (16, 16),
            flat_height,
        );
        assert!(back.is_some(), "screen center must hit the map");
    }

    #[test]
    fn world_to_tile_matches_the_candidate_scan_on_edges_and_corners() {
        // Every centre of the map: the nearest within the circumradius, ties
        // to the lowest row, then column. (The former 15-candidate scan
        // skipped a column whenever round(q) fell two below ceil(q + 1).)
        fn scan(x: f32, z: f32, map: (u32, u32)) -> Option<TileId> {
            let mut best: Option<(f32, u32, u32)> = None;
            for r in 0..map.1 {
                for q in 0..map.0 {
                    let (cx, cz) = tile_to_world(q, r);
                    let d = (x - cx).powi(2) + (z - cz).powi(2);
                    if d <= 1.0 && best.is_none_or(|(bd, _, _)| d < bd) {
                        best = Some((d, r, q));
                    }
                }
            }
            best.map(|(_, r, q)| TileId(r * map.0 + q))
        }
        let map = (23, 17);
        let mut points = vec![];
        for i in 0..5_000 {
            let t = i as f32;
            points.push([(t * 0.618).fract() * 44. - 2., (t * 0.381).fract() * 29. - 2.]);
        }
        for r in 0..map.1 as i32 {
            for q in 0..map.0 as i32 {
                for d in 0..6 {
                    let a = crate::scene::terrain::lattice_to_world(crate::scene::terrain::corner_lattice(q, r, d));
                    let b = crate::scene::terrain::lattice_to_world(crate::scene::terrain::corner_lattice(q, r, (d + 1) % 6));
                    points.push([a.0, a.1]);
                    points.push([(a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5]);
                }
            }
        }
        for p in points {
            assert_eq!(world_to_tile(p[0], p[1], map), scan(p[0], p[1], map), "{p:?}");
        }
    }

    #[test]
    fn world_to_tile_nearest() {
        let (wx, wz) = tile_to_world(5, 5);
        assert_eq!(world_to_tile(wx, wz, (16, 16)), Some(TileId(5 * 16 + 5)));
        assert_eq!(world_to_tile(-100.0, -100.0, (16, 16)), None);
    }
}
