//! The first view of a world: the camera zoomed to what the player knows, not to a constant.
//! On turn 1 the known land is a few tiles around the settlers, and a fixed zoom left it a fifth of
//! the screen; a loaded game knows far more and wants the same rule. The fit is the app's, called
//! when a world starts: the snapshot stage keeps its fixed zoom (`app::map_camera`) so that its
//! pictures do not move with the rule.
use render::scene::layout::tile_to_world;
use render::world::state::RenderWorld;

/// How much of the window's smaller side the known land and its ring fill. The ring and the oblique
/// view make the box wider than the land that shows, so the land itself ends near 60-65 % (measured
/// on a turn-1 capture: a box share of 0.6 left it at 49 % and zoomed out from the old constant).
pub const FILL: f32 = 0.8;
/// The zoom stays inside this: the camera's own range is wider (0.15 to 8).
pub const ZOOM_RANGE: (f32, f32) = (1.5, 4.0);
/// The ground is seen at 45 degrees: a distance north-south covers this much of the same distance
/// east-west on the screen.
const FORESHORTENING: f32 = std::f32::consts::FRAC_1_SQRT_2;
/// The ring of tiles added around the known ones, in world units (one hex across and a row).
const RING: (f32, f32) = (1.75, 1.5);

/// The rectangle of world units that the explored tiles cover, with a ring around it:
/// (min x, min z, max x, max z). None when nothing is explored.
pub fn known_area(world: &RenderWorld) -> Option<(f32, f32, f32, f32)> {
    let width = world.width.max(1);
    let mut area: Option<(f32, f32, f32, f32)> = None;
    for (tile, explored) in world.explored.iter().enumerate() {
        if *explored == 0 {
            continue;
        }
        let (x, z) = tile_to_world(tile as u32 % width, tile as u32 / width);
        area = Some(area.map_or((x, z, x, z), |(x0, z0, x1, z1)| (x0.min(x), z0.min(z), x1.max(x), z1.max(z))));
    }
    area.map(|(x0, z0, x1, z1)| (x0 - RING.0, z0 - RING.1, x1 + RING.0, z1 + RING.1))
}

/// The zoom that makes the known land fill `FILL` of the smaller side of a window `window` points
/// across, clamped to `ZOOM_RANGE`; the centre of the land is the second value. None when nothing
/// is explored (the caller keeps its zoom).
pub fn fit(world: &RenderWorld, window: egui::Vec2) -> Option<(f32, (f32, f32))> {
    let (x0, z0, x1, z1) = known_area(world)?;
    let unit = render::camera::controller::CameraController { x: 0.0, y: 0.0, zoom: 1.0 }.pixels_per_unit(window.y.max(1.0));
    // Pixels the land takes at zoom 1, by the larger of its two sides; the zoom scales them.
    let at_one = ((x1 - x0) * unit).max((z1 - z0) * FORESHORTENING * unit).max(1.0);
    let zoom = (FILL * window.x.min(window.y) / at_one).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
    Some((zoom, ((x0 + x1) / 2.0, (z0 + z1) / 2.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world `side` tiles across with `explored` tiles known (given as (column, row)).
    fn world(side: u32, explored: &[(u32, u32)]) -> RenderWorld {
        let mut world = RenderWorld { width: side, height: side, explored: vec![0; (side * side) as usize], ..RenderWorld::default() };
        for (q, r) in explored {
            world.explored[(r * side + q) as usize] = 1;
        }
        world
    }

    /// The seven tiles of the settlers' sight at the start: a tile and its six neighbours.
    fn seven() -> RenderWorld {
        world(40, &[(20, 20), (21, 20), (19, 20), (19, 21), (20, 21), (20, 19), (21, 19)])
    }

    /// How much of the smaller side the fitted land takes, from the same projection the zoom is chosen with.
    fn filled(world: &RenderWorld, window: egui::Vec2) -> f32 {
        let (zoom, _) = fit(world, window).expect("land is known");
        let (x0, z0, x1, z1) = known_area(world).unwrap();
        let unit = render::camera::controller::CameraController { x: 0.0, y: 0.0, zoom }.pixels_per_unit(window.y);
        ((x1 - x0) * unit).max((z1 - z0) * FORESHORTENING * unit) / window.x.min(window.y)
    }

    #[test]
    fn seven_known_tiles_fill_the_window_in_the_zoom_range_on_a_desktop_and_a_phone() {
        for window in [egui::vec2(1600., 900.), egui::vec2(844., 390.), egui::vec2(390., 844.)] {
            let (zoom, _) = fit(&seven(), window).expect("land is known");
            assert!((ZOOM_RANGE.0..=ZOOM_RANGE.1).contains(&zoom), "{window:?}: zoom {zoom}");
            let share = filled(&seven(), window);
            // Inside the window, and (while the clamp lets it) near the wanted share.
            assert!(share <= 1.0, "{window:?}: the land overflows the window ({share})");
            assert!(share > 0.3, "{window:?}: the land is lost in the window ({share})");
        }
    }

    #[test]
    fn a_wide_known_area_zooms_out_to_the_floor_and_nothing_known_keeps_the_zoom() {
        let wide: Vec<(u32, u32)> = (0..40).flat_map(|r| (0..40).map(move |q| (q, r))).collect();
        assert_eq!(fit(&world(40, &wide), egui::vec2(1600., 900.)).unwrap().0, ZOOM_RANGE.0);
        assert!(fit(&world(40, &[]), egui::vec2(1600., 900.)).is_none());
    }

    #[test]
    fn the_view_centres_on_the_known_land() {
        let (_, (x, z)) = fit(&seven(), egui::vec2(1600., 900.)).unwrap();
        let (cx, cz) = tile_to_world(20, 20);
        assert!((x - cx).abs() < 2.0 && (z - cz).abs() < 2.0, "centre ({x}, {z}) against the middle tile ({cx}, {cz})");
    }
}
