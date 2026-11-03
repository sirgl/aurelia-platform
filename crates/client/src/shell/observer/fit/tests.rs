//! The framing of a panel: what it shows, what it ignores and how it follows a growth.
use render::world::state::{CityMarker, TerrainCell};

use super::*;

const SIZE: Size = Size { width: 800, height: 600 };

/// A world of `width` x `height` tiles; `cities` are own cities (tile columns and rows),
/// `seen` the tiles in sight and `known` the explored ones.
fn world(width: u32, height: u32, cities: &[(u32, u32)], seen: impl Fn(u32, u32) -> bool, known: impl Fn(u32, u32) -> bool) -> RenderWorld {
    let n = (width * height) as usize;
    let at = |t: usize| (t as u32 % width, t as u32 / width);
    let terrain = (0..n as u32).map(|t| TerrainCell { tile: kernel::ids::TileId(t), terrain: 1, owner: None, resource: None }).collect();
    let city = |&(q, r): &(u32, u32)| CityMarker {
        tile: kernel::ids::TileId(r * width + q),
        owner: 0,
        name: String::new(),
        population: Some(3),
        buildings: Vec::new(),
        era: 0,
        showpieces: Vec::new(),
        unworked: Vec::new(),
    };
    RenderWorld {
        width,
        height,
        terrain,
        explored: (0..n).map(|t| u8::from(known(at(t).0, at(t).1))).collect(),
        visible: (0..n).map(|t| u8::from(seen(at(t).0, at(t).1))).collect(),
        cities: cities.iter().map(city).collect(),
        observer: 0,
        ..Default::default()
    }
}

fn near(a: Pos2, q: u32, r: u32, hexes: f32) -> bool {
    let (x, z) = tile_to_world(q, r);
    (a.x - x).abs() <= hexes * spacing() && (a.y - z).abs() <= hexes * spacing()
}

#[test]
fn the_land_in_memory_does_not_stretch_the_frame() {
    let cities = [(20, 20), (24, 22)];
    let w = world(80, 50, &cities, |q, r| (17..28).contains(&q) && (17..26).contains(&r), |_, _| true);
    let ground = ground(&w);
    assert!(ground.width() < 0.4 * whole_ground(&w).width(), "{ground:?} is the land of the cities, not of the memory");
    let camera = frame(ground, SIZE);
    let everything = frame(whole_ground(&w), SIZE);
    assert!(camera.zoom > everything.zoom * 3.0, "{} against {}", camera.zoom, everything.zoom);
    assert!(near(ground.center(), 22, 21, 3.0));
}

#[test]
fn one_city_at_the_start_is_not_shown_gigantic() {
    let w = world(80, 50, &[(10, 10)], |q, r| q.abs_diff(10) <= 1 && r.abs_diff(10) <= 1, |_, _| false);
    let camera = frame(ground(&w), SIZE);
    let point = frame(Rect::from_center_size(ground(&w).center(), vec2(0.0, 0.0)), SIZE);
    assert_eq!(camera.zoom, point.zoom, "a small frame is no tighter than the least land");
    assert!(camera.zoom < ZOOMS.1, "{}", camera.zoom);
}

#[test]
fn an_empire_over_half_the_map_fits_the_panel() {
    let cities: Vec<(u32, u32)> = (0..8).flat_map(|i| [(4 + 5 * i, 6 + 2 * i), (6 + 5 * i, 30 - 2 * i)]).collect();
    let w = world(60, 40, &cities, |q, r| cities.iter().any(|&(cq, cr)| cq.abs_diff(q) <= 2 && cr.abs_diff(r) <= 2), |_, _| true);
    for size in [SIZE, Size { width: 300, height: 300 }, Size { width: 1200, height: 500 }] {
        let ground = ground(&w);
        assert!(shows(&frame(ground, size), size, ground), "the empire leaves the panel {size:?}");
    }
}

#[test]
fn a_scout_far_from_the_cities_does_not_widen_the_frame() {
    let near_city = |q: u32, r: u32| q.abs_diff(10) <= 3 && r.abs_diff(10) <= 3;
    let w = world(80, 50, &[(10, 10)], |q, r| near_city(q, r) || (q, r) == (70, 40), |_, _| true);
    assert!(ground(&w).width() < 12.0 * spacing(), "{:?}", ground(&w));
}

#[test]
fn without_a_city_the_seen_land_is_framed() {
    let w = world(80, 50, &[], |q, r| (30..38).contains(&q) && (20..26).contains(&r), |_, _| true);
    let ground = ground(&w);
    assert!(near(ground.center(), 34, 23, 3.0), "{ground:?}");
}

#[test]
fn a_power_that_sees_and_knows_nothing_shows_the_whole_map() {
    let w = world(64, 40, &[], |_, _| false, |_, _| false);
    let camera = frame(ground(&w), SIZE);
    let (cx, cz) = map_center(64, 40);
    assert!((camera.x - cx).abs() < 0.01 && (camera.y - cz).abs() < 0.01);
    assert!(camera.zoom >= ZOOMS.0 && camera.zoom <= ZOOMS.1);
}

#[test]
fn the_camera_stays_while_it_shows_the_ground_and_moves_half_way_when_it_does_not() {
    let small = Rect::from_center_size(Pos2::new(20.0, 20.0), vec2(10.0, 8.0));
    let first = frame(small, SIZE);
    assert_eq!(settle(None, &first, small, SIZE, true).zoom, first.zoom, "the first frame is taken as it is");
    let slightly = Rect::from_center_size(Pos2::new(20.5, 20.0), vec2(10.5, 8.0));
    let kept = settle(Some(&first), &frame(slightly, SIZE), slightly, SIZE, true);
    assert_eq!((kept.x, kept.y, kept.zoom), (first.x, first.y, first.zoom), "a small change leaves the camera alone");
    let grown = Rect::from_center_size(Pos2::new(40.0, 22.0), vec2(40.0, 22.0));
    let target = frame(grown, SIZE);
    let moved = settle(Some(&first), &target, grown, SIZE, false);
    assert!(moved.zoom < first.zoom, "it zooms out: {} from {}", moved.zoom, first.zoom);
    assert!(moved.x > first.x && moved.x <= target.x, "the centre goes half way");
    assert!(shows(&moved, SIZE, grown), "whatever the step, the ground stays on the panel");
}

#[test]
fn a_view_much_emptier_than_its_frame_is_framed_again_only_when_due() {
    let big = Rect::from_center_size(Pos2::new(40.0, 20.0), vec2(80.0, 40.0));
    let loose = frame(big, SIZE);
    let tight = Rect::from_center_size(Pos2::new(40.0, 20.0), vec2(20.0, 10.0));
    let target = frame(tight, SIZE);
    assert!(settle(Some(&loose), &target, tight, SIZE, false).zoom == loose.zoom, "not due: the camera stays");
    assert!(settle(Some(&loose), &target, tight, SIZE, true).zoom > loose.zoom, "due: it closes in");
}
