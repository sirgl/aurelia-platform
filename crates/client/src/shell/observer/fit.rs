//! The camera of a panel: it frames what is alive for the power, its own cities and the
//! land it sees now, and not the faint trace of everything it ever knew. What the power
//! only remembers stays on the panel as background. Pure: a world and a pixel size in, a
//! camera out; `settle` keeps the camera from jumping between rounds.
use egui::{vec2, Pos2, Rect};
use render::camera::CameraController;
use render::scene::layout::{map_center, tile_to_world};
use render::world::state::RenderWorld;

use super::frame::Size;

/// The share of the panel the framed land fills.
const FILL: f32 = 0.92;
/// Hexes of margin around the framed land.
const MARGIN_HEXES: f32 = 2.5;
/// The least land a panel frames, so one city at the start is not shown gigantic.
const MIN_WIDTH_HEXES: f32 = 9.0;
const MIN_DEPTH_HEXES: f32 = 6.0;
/// Seen land counts only this near the cities: a scout far away does not stretch the frame.
const SEEN_REACH_HEXES: f32 = 6.0;
/// The view is tilted: depth shows about this much of its length.
const DEPTH: f32 = 0.7;
/// The zooms a panel keeps to.
const ZOOMS: (f32, f32) = (0.4, 4.0);
/// How much one step of zooming out keeps, and how many steps are tried.
const SHRINK: f32 = 0.93;
const SHRINK_STEPS: usize = 24;
/// A view this much emptier than the frame it should have is framed again.
const TOO_EMPTY: f32 = 0.6;

/// Width of a hex step in world units (tile 0 to tile 1).
fn spacing() -> f32 {
    let (a, b) = (tile_to_world(0, 0), tile_to_world(1, 0));
    (b.0 - a.0).abs().max(0.1)
}

/// What is alive for a power, in world units on the ground.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Focus {
    pub cities: Vec<Pos2>,
    pub seen: Vec<Pos2>,
}

/// The ground a panel should show: the focus, else what the power knows, else the map.
pub fn ground(world: &RenderWorld) -> Rect {
    focus_ground(&focus_of(world)).or_else(|| known_ground(world)).unwrap_or_else(|| whole_ground(world))
}

/// The own cities of the observer and the tiles it sees now.
pub fn focus_of(world: &RenderWorld) -> Focus {
    let cities = world.cities.iter().filter(|c| c.owner == world.observer).map(|c| ground_point(world, c.tile.0)).collect();
    let seen = world.visible.iter().enumerate().filter(|(_, v)| **v > 0).map(|(tile, _)| ground_point(world, tile as u32)).collect();
    Focus { cities, seen }
}

/// Where a tile lies on the ground, in world units.
pub fn ground_point(world: &RenderWorld, tile: u32) -> Pos2 {
    let width = world.width.max(1);
    let (x, z) = tile_to_world(tile % width, tile / width);
    Pos2::new(x, z)
}

/// The box round the cities and the land seen near them; with no city, round all seen.
pub fn focus_ground(focus: &Focus) -> Option<Rect> {
    let Some(own) = bounds(&focus.cities) else {
        return bounds(&focus.seen);
    };
    let near = own.expand(SEEN_REACH_HEXES * spacing());
    let mut ground = own;
    focus.seen.iter().filter(|p| near.contains(**p)).for_each(|p| ground.extend_with(*p));
    Some(ground)
}

fn bounds(points: &[Pos2]) -> Option<Rect> {
    (!points.is_empty()).then(|| Rect::from_points(points))
}

/// The box round the tiles the power has explored.
fn known_ground(world: &RenderWorld) -> Option<Rect> {
    let points: Vec<Pos2> = (0..world.terrain.len() as u32).filter(|tile| world.is_known(*tile as usize)).map(|tile| ground_point(world, tile)).collect();
    bounds(&points)
}

/// The whole map, centred as the map is.
fn whole_ground(world: &RenderWorld) -> Rect {
    let (w, h) = (world.width.max(1), world.height.max(1));
    let (far_x, far_z) = tile_to_world(w - 1, h - 1);
    let (cx, cz) = map_center(w, h);
    let half = vec2(cx.max(far_x - cx), cz.max(far_z - cz));
    Rect::from_center_size(Pos2::new(cx, cz), 2.0 * half)
}

/// The camera that frames `ground` in a panel of `size` pixels.
pub fn frame(ground: Rect, size: Size) -> CameraController {
    let margin = 2.0 * MARGIN_HEXES * spacing();
    let span = vec2((ground.width() + margin).max(MIN_WIDTH_HEXES * spacing()), (ground.height() + margin).max(MIN_DEPTH_HEXES * spacing()));
    let mut camera = CameraController { x: ground.center().x, y: ground.center().y, zoom: zoom_for(size, span) };
    shrink_to_show(&mut camera, size, ground);
    camera
}

/// The tilted view is wider near than far and the estimate of the depth is rough: zoom
/// out until the four corners of the ground, a hex wider (a border tile is drawn whole),
/// are all on the panel (or the least zoom).
fn shrink_to_show(camera: &mut CameraController, size: Size, ground: Rect) {
    let ground = ground.expand(spacing());
    for _ in 0..SHRINK_STEPS {
        if shows(camera, size, ground) || camera.zoom <= ZOOMS.0 {
            return;
        }
        camera.zoom = (camera.zoom * SHRINK).max(ZOOMS.0);
    }
}

/// Are all four corners of `ground` on the panel?
pub fn shows(camera: &CameraController, size: Size, ground: Rect) -> bool {
    let (w, h) = (size.width.max(1) as f32, size.height.max(1) as f32);
    [ground.left_top(), ground.right_top(), ground.left_bottom(), ground.right_bottom()]
        .into_iter()
        .all(|p| camera.project_to_screen([p.x, 0.4, p.y], w / h, w, h).is_some())
}

/// The zoom at which a ground box of `span` (world units) fills `size` pixels.
fn zoom_for(size: Size, span: egui::Vec2) -> f32 {
    let unit = CameraController { zoom: 1.0, ..CameraController::default() }.pixels_per_unit(size.height.max(1) as f32);
    let wanted = (FILL * size.width.max(1) as f32 / span.x.max(0.1)).min(FILL * size.height.max(1) as f32 / (DEPTH * span.y.max(0.1)));
    (wanted / unit).clamp(ZOOMS.0, ZOOMS.1)
}

/// The camera after a round: the one before when it still shows the ground well, and
/// otherwise half way to `target`, so a growing empire is followed without jumps. A
/// camera that does not show the ground moves even when no move is `due`.
pub fn settle(previous: Option<&CameraController>, target: &CameraController, ground: Rect, size: Size, due: bool) -> CameraController {
    let Some(old) = previous else { return target.clone() };
    let shown = shows(old, size, ground.expand(spacing()));
    if shown && (!due || old.zoom >= TOO_EMPTY * target.zoom) {
        return old.clone();
    }
    let mut next = CameraController {
        x: (old.x + target.x) / 2.0,
        y: (old.y + target.y) / 2.0,
        zoom: (old.zoom * target.zoom).sqrt(),
    };
    shrink_to_show(&mut next, size, ground);
    next
}

#[cfg(test)]
mod tests;
