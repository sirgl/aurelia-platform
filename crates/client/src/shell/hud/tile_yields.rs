//! Tile-yield presentation: the six yields the observer knows of each tile
//! (`RenderWorld::yields`, the core's `plot_yields`). Hand-built showcase worlds
//! without that layer fall back to the bare terrain's food and production.
//! No meshes are built here: anchors sample the renderer's cached display mesh.
use super::{Icon, LIGHT, VisualHudState, city_banner::Fit, icon, text};
use crate::shell::module::ModuleUi;
use render::gpu::buffers::terrain_icon_food_prod;
use render::{
    camera::CameraController,
    scene::{layout::tile_to_world, terrain::surface::SurfaceMesh},
    world::state::{RenderWorld, TerrainCell},
};
use egui::{Align2, Color32, Pos2, Rect, pos2, vec2};

pub fn draw(
    ctx: &egui::Context,
    world: &RenderWorld,
    camera: &CameraController,
    state: &VisualHudState,
    surface: Option<&SurfaceMesh>,
    reserved: Option<Rect>,
    module: &dyn ModuleUi,
    fit: Fit,
) {
    let Some(surface) = surface else { return };
    let screen = ctx.content_rect();
    if world.width == 0 || world.height == 0 || screen.height() < 240.0 {
        return;
    }
    let matrix = camera.view_proj(screen.aspect_ratio());
    let Some(scale) = tile_screen_width(&matrix, screen, [camera.x, 0.0, camera.y]) else {
        return;
    };
    if scale < 45.0 {
        return;
    }
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            "tile_yields".into(),
        ))
        .with_clip_rect(screen);
    let mut occupied = reserved_rects(world, camera, screen, state, reserved, fit);
    // A picture the module put on a tile (a deposit, a land work) keeps the middle of the
    // hex; the yields stand under it.
    let marked: std::collections::BTreeMap<u32, bool> = module
        .map_paint()
        .map(|paint| paint.icons.iter().map(|mark| (mark.tile, mark.label.is_some())).collect())
        .unwrap_or_default();
    occupied.extend(crate::shell::layout::current(ctx).obstacles().map(|r| r.expand(5.0)));
    let occupied_tiles: std::collections::BTreeSet<u32> = world
        .cities
        .iter()
        .map(|city| city.tile.0)
        .chain(world.units.iter().map(|unit| unit.tile.0))
        .collect();
    // The camera has a fixed tilt; this deliberately generous ground rectangle
    // bounds candidates before projection. At eligible zooms its area is bounded
    // independently of total map size. RenderWorld terrain is row-major.
    let reach = camera.distance() * 2.0;
    let row_min = (((camera.y - reach) / 1.5).floor() as i32).max(0) as u32;
    let row_max = (((camera.y + reach) / 1.5).ceil() as u32).min(world.height - 1);
    let east = reach * screen.aspect_ratio().max(1.0);
    for row in row_min..=row_max {
        let q_min =
            (((camera.x - east) / 1.732_050_8 - row as f32 * 0.5).floor() as i32).max(0) as u32;
        let q_max = (((camera.x + east) / 1.732_050_8 - row as f32 * 0.5).ceil() as i32)
            .min(world.width as i32 - 1);
        if q_max < 0 {
            continue;
        }
        for col in q_min..=q_max as u32 {
            let tile = row * world.width + col;
            if occupied_tiles.contains(&tile) {
                continue;
            }
            let Some((amounts, seen)) = shown_amounts(world, tile, module.tile_yields(tile)) else {
                continue;
            };
            // A remembered tile's numbers come from the module; its anchor is the known one.
            let anchor = if seen { surface_anchor(surface, world, tile) } else { known_anchor(surface, world, tile) };
            let Some(anchor) = anchor else {
                continue;
            };
            let Some(at) = project(&matrix, screen, anchor) else {
                continue;
            };
            let available = tile_screen_width(&matrix, screen, anchor).unwrap_or(0.0) - 9.0;
            let Some(layout) = YieldLayout::new(amounts, available) else {
                continue;
            };
            let at = match marked.get(&tile) {
                Some(&labelled) => at + vec2(0.0, MARK_CLEARANCE + if labelled { MARK_LABEL } else { 0.0 } + layout.height * 0.5),
                None => at,
            };
            let rect = Rect::from_center_size(at, vec2(layout.width, layout.height));
            if !screen.shrink(12.0).contains_rect(rect)
                || occupied.iter().any(|r| r.intersects(rect))
            {
                continue;
            }
            occupied.push(rect.expand(4.0));
            let fade = if seen { 1.0 } else { REMEMBERED };
            for (index, (kind, tint)) in YIELD_ICONS.into_iter().enumerate() {
                draw_quantity(
                    &painter,
                    rect.min + layout.origins[index],
                    layout.quantities[index],
                    kind,
                    tint.gamma_multiply(fade),
                    fade,
                );
            }
        }
    }
}

/// The rows a module's tile picture takes below the hex's centre: its disc (12 px) and its
/// label (15 px), which the yields stand under.
const MARK_CLEARANCE: f32 = 14.0;
const MARK_LABEL: f32 = 15.0;

/// How the labels of the frame are sized (`ModuleUi::labels_follow_zoom`): the hex's width in
/// pixels at the camera's centre sets the scale. The classic module's labels keep one size.
pub(super) fn label_fit(camera: &CameraController, screen: Rect, follow: bool) -> Fit {
    if !follow {
        return Fit::CLASSIC;
    }
    let matrix = camera.view_proj(screen.aspect_ratio());
    tile_screen_width(&matrix, screen, [camera.x, 0.0, camera.y]).map_or(Fit { k: 1.0, above: true, banners: true }, Fit::zoomed)
}

/// Icon and tint of each yield, in `TileYields` order (food, production, gold,
/// science, culture, faith).
pub(super) const YIELD_ICONS: [(Icon, Color32); 6] = [
    (Icon::Apple, Color32::from_rgb(157, 215, 49)),
    (Icon::Hammer, Color32::from_rgb(239, 166, 43)),
    (Icon::Coin, Color32::from_rgb(240, 200, 80)),
    (Icon::Flask, Color32::from_rgb(100, 198, 239)),
    (Icon::Note, Color32::from_rgb(186, 136, 216)),
    (Icon::Faith, Color32::from_rgb(236, 236, 214)),
];

/// How much of its colour a yield keeps on a tile out of sight.
const REMEMBERED: f32 = 0.6;

/// The tile's yields: the module's own numbers when it gives them (`from_module`), else
/// as the observer knows them; the bare terrain's food and production in a world without
/// the yield layer.
fn tile_amounts(from_module: Option<[i32; 6]>, world: &RenderWorld, tile: u32, cell: &TerrainCell) -> [i32; 6] {
    if let Some(amounts) = from_module {
        return amounts;
    }
    match world.yields.get(tile as usize) {
        Some(y) => y.map(i32::from),
        None => {
            let (food, production) = terrain_icon_food_prod(cell.terrain);
            [food, production, 0, 0, 0, 0]
        }
    }
}

// Up to four yields are counted by silhouette. Larger quantities switch to one
// symbol and a numeral, avoiding unbounded rows on heavily improved game tiles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum YieldQuantity {
    Empty,
    Repeated(u8),
    Numbered(i32),
}
impl YieldQuantity {
    fn new(amount: i32) -> Self {
        match amount {
            ..=0 => Self::Empty,
            1..=4 => Self::Repeated(amount as u8),
            _ => Self::Numbered(amount),
        }
    }
    fn width(self) -> f32 {
        match self {
            Self::Empty => 0.0,
            Self::Repeated(1 | 2) => 12.0,
            Self::Repeated(_) => 24.0,
            Self::Numbered(count) => 15.0 + (count.ilog10() + 1) as f32 * 7.0,
        }
    }
    fn height(self) -> f32 {
        match self {
            Self::Empty => 0.0,
            Self::Repeated(2..=4) => 24.0,
            _ => 12.0,
        }
    }
    fn icon_positions(self) -> &'static [[f32; 2]] {
        match self {
            Self::Empty => &[],
            Self::Repeated(2) => &[[6.0, 6.0], [6.0, 18.0]],
            Self::Repeated(3) => &[[12.0, 6.0], [6.0, 18.0], [18.0, 18.0]],
            Self::Repeated(4) => &[[6.0, 6.0], [18.0, 6.0], [6.0, 18.0], [18.0, 18.0]],
            _ => &[[6.0, 6.0]],
        }
    }
}

/// Groups of yield symbols side by side, 5 px apart, in `YIELD_ICONS` order.
#[derive(Debug)]
struct YieldLayout {
    quantities: [YieldQuantity; 6],
    origins: [egui::Vec2; 6],
    width: f32,
    height: f32,
}
impl YieldLayout {
    fn new(amounts: [i32; 6], available: f32) -> Option<Self> {
        const GAP: f32 = 5.0;
        let quantities = amounts.map(YieldQuantity::new);
        let widths = quantities.map(YieldQuantity::width);
        let groups = widths.iter().filter(|w| **w > 0.0).count();
        if groups == 0 {
            return None;
        }
        let width = widths.iter().sum::<f32>() + GAP * (groups - 1) as f32 + 4.0;
        if width > available {
            return None;
        }
        let heights = quantities.map(YieldQuantity::height);
        let height = heights.iter().copied().fold(0.0, f32::max) + 4.0;
        let mut origins = [vec2(0.0, 0.0); 6];
        let mut x = 2.0;
        for i in 0..6 {
            origins[i] = vec2(x, (height - heights[i]) * 0.5);
            if widths[i] > 0.0 {
                x += widths[i] + GAP;
            }
        }
        Some(Self { quantities, origins, width, height })
    }
}

fn draw_quantity(
    painter: &egui::Painter,
    origin: Pos2,
    quantity: YieldQuantity,
    kind: Icon,
    tint: Color32,
    fade: f32,
) {
    for &[x, y] in quantity.icon_positions() {
        icon(painter, origin + vec2(x, y), 5.0, kind, tint);
    }
    if let YieldQuantity::Numbered(amount) = quantity {
        let at = origin + vec2(15.0, 6.0);
        text(
            painter,
            at + vec2(1.0, 1.0),
            Align2::LEFT_CENTER,
            amount,
            11.0,
            Color32::from_black_alpha(235).gamma_multiply(fade),
        );
        text(painter, at, Align2::LEFT_CENTER, amount, 11.0, LIGHT.gamma_multiply(fade));
    }
}

/// The yields to draw on a tile and whether the tile is in sight now. A tile in sight shows
/// its yields; one only explored shows the module's remembered numbers (`given`), dimmer,
/// when the module gives them (a module that gives none, the classic one, keeps its icons on tiles in
/// sight alone). None: nothing to draw.
fn shown_amounts(world: &RenderWorld, tile: u32, given: Option<[i32; 6]>) -> Option<([i32; 6], bool)> {
    let (cell, seen) = known_cell(world, tile, given.is_some())?;
    let amounts = tile_amounts(given, world, tile, cell);
    amounts.iter().any(|a| *a > 0).then_some((amounts, seen))
}

/// The tile's cell and whether it is in sight now: a tile in sight, or (with `remembered`)
/// one explored. None: the tile is off the map, unexplored, or out of sight with no
/// remembered numbers to show.
fn known_cell(world: &RenderWorld, tile: u32, remembered: bool) -> Option<(&TerrainCell, bool)> {
    if let Some(cell) = visible_cell(world, tile) {
        return Some((cell, true));
    }
    let explored = world.explored.get(tile as usize).copied().unwrap_or(0) != 0;
    if !(remembered && explored) || tile >= world.width.saturating_mul(world.height) {
        return None;
    }
    world.terrain.get(tile as usize).filter(|cell| cell.tile.0 == tile).map(|cell| (cell, false))
}

fn visible_cell(world: &RenderWorld, tile: u32) -> Option<&TerrainCell> {
    if tile >= world.width.saturating_mul(world.height)
        || world.visible.get(tile as usize).copied().unwrap_or(0) == 0
    {
        return None;
    }
    world
        .terrain
        .get(tile as usize)
        .filter(|cell| cell.tile.0 == tile)
}

/// Where a known tile's centre is on `screen` (None: under fog or off screen). Module
/// overlays (route lines, district marks) draw with it over the map.
pub fn tile_on_screen(world: &RenderWorld, camera: &CameraController, surface: &SurfaceMesh, screen: Rect, tile: u32) -> Option<Pos2> {
    let matrix = camera.view_proj(screen.aspect_ratio());
    project(&matrix, screen, surface_anchor(surface, world, tile)?)
}

pub(super) fn surface_anchor(surface: &SurfaceMesh, world: &RenderWorld, tile: u32) -> Option<[f32; 3]> {
    anchor_where_fog_is_at_least(surface, world, tile, 0.96)
}

/// The anchor of a tile the player has explored, in sight or remembered: the fog of a
/// remembered tile's centre is its own level (`render::scene::fog::fog_at`, 0.45), an
/// unexplored tile's is lower (0.18). For marks that stand on remembered land too.
pub(super) fn known_anchor(surface: &SurfaceMesh, world: &RenderWorld, tile: u32) -> Option<[f32; 3]> {
    anchor_where_fog_is_at_least(surface, world, tile, 0.44)
}

fn anchor_where_fog_is_at_least(surface: &SurfaceMesh, world: &RenderWorld, tile: u32, least: f32) -> Option<[f32; 3]> {
    let (x, z) = tile_to_world(tile % world.width, tile / world.width);
    // Hide near the fading knowledge boundary rather than stamp symbols on fog.
    if render::scene::fog::fog_at_position(world, x, z) < least {
        return None;
    }
    let sample = surface.sample_display(x, z)?;
    Some([x, sample.y + 0.025, z])
}

pub(super) fn project(matrix: &[[f32; 4]; 4], screen: Rect, position: [f32; 3]) -> Option<Pos2> {
    let p = [position[0], position[1], position[2], 1.0];
    let clip: [f32; 4] =
        std::array::from_fn(|row| (0..4).map(|col| matrix[col][row] * p[col]).sum());
    if clip[3] <= 0.0 || clip[2] < 0.0 || clip[2] > clip[3] {
        return None;
    }
    Some(pos2(
        screen.left() + (clip[0] / clip[3] + 1.0) * screen.width() * 0.5,
        screen.top() + (1.0 - clip[1] / clip[3]) * screen.height() * 0.5,
    ))
}

pub(super) fn tile_screen_width(matrix: &[[f32; 4]; 4], screen: Rect, p: [f32; 3]) -> Option<f32> {
    let left = project(matrix, screen, [p[0] - 0.866, p[1], p[2]])?;
    let right = project(matrix, screen, [p[0] + 0.866, p[1], p[2]])?;
    Some(right.distance(left))
}

fn reserved_rects(
    world: &RenderWorld,
    camera: &CameraController,
    screen: Rect,
    state: &VisualHudState,
    reserved: Option<Rect>,
    fit: Fit,
) -> Vec<Rect> {
    // The panels and controls come from the HUD layout (`draw`).
    let mut rects: Vec<Rect> = reserved.into_iter().collect();
    if state.show_settings {
        rects.push(Rect::from_min_size(
            pos2(screen.right() - 390.0, 48.0),
            vec2(390.0, 270.0),
        ));
    }
    if state.show_labels && fit.banners {
        for city in &world.cities {
            let (x, z) = tile_to_world(city.tile.0 % world.width, city.tile.0 / world.width);
            if let Some((sx, sy)) = camera.project_to_screen(
                [x, 1.65, z],
                screen.aspect_ratio(),
                screen.width(),
                screen.height(),
            ) {
                let rect = Rect::from_min_size(pos2(sx - 105.0, sy - 27.0), vec2(210.0, 50.0));
                rects.push(if fit.above {
                    // Over the model: the banner's size at the scale, lifted by its height.
                    let own = state.feed.as_ref().and_then(|feed| feed.banners.iter().find(|b| b.tile == city.tile.0));
                    let banner = super::city_banner::rect_at(pos2(sx, sy), own, fit);
                    Rect::from_min_max(banner.min - vec2(0.0, 7.0 * fit.k), banner.max + vec2(0.0, 3.0 * fit.k))
                } else {
                    rect
                });
            }
        }
    }
    for badge in super::visible_unit_badges(world, &[]) {
        let (x, z) = tile_to_world(badge.tile % world.width, badge.tile / world.width);
        let city = state.show_labels && world.cities.iter().any(|c| c.tile.0 == badge.tile);
        if let Some((sx, sy)) = camera.project_to_screen(
            [x, if city { 1.65 } else { 1.95 }, z],
            screen.aspect_ratio(),
            screen.width(),
            screen.height(),
        ) {
            let size = (if city {
                vec2(330.0, 82.0)
            } else {
                vec2(65.0, 83.0)
            }) * fit.k;
            rects.push(Rect::from_center_size(pos2(sx, sy), size));
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::ids::TileId;

    #[test]
    fn quantity_counts_individual_symbols_until_five() {
        assert_eq!(YieldQuantity::new(0), YieldQuantity::Empty);
        assert_eq!(YieldQuantity::new(1), YieldQuantity::Repeated(1));
        assert_eq!(YieldQuantity::new(4), YieldQuantity::Repeated(4));
        assert_eq!(YieldQuantity::new(5), YieldQuantity::Numbered(5));
        assert!(YieldLayout::new([0; 6], 100.0).is_none());
    }

    #[test]
    fn each_yield_has_its_own_icon_and_faith_shows_the_dove() {
        let kinds = YIELD_ICONS.map(|(kind, _)| kind);
        for (n, kind) in kinds.iter().enumerate() {
            assert!(!kinds[n + 1..].contains(kind), "{kind:?} stands for two yields");
        }
        // Faith was the sun, drawn like the settings gear: it reads as a cog on the map.
        assert_eq!(kinds[5], Icon::Faith);
        assert!(!kinds.contains(&Icon::Sun) && !kinds.contains(&Icon::Gear));
    }

    #[test]
    fn yield_groups_use_two_rows_and_stay_beside_each_other() {
        assert_eq!(YieldQuantity::new(1).icon_positions(), &[[6.0, 6.0]]);
        assert_eq!(
            YieldQuantity::new(2).icon_positions(),
            &[[6.0, 6.0], [6.0, 18.0]]
        );
        assert_eq!(
            YieldQuantity::new(3).icon_positions(),
            &[[12.0, 6.0], [6.0, 18.0], [18.0, 18.0]]
        );
        assert_eq!(
            YieldQuantity::new(4).icon_positions(),
            &[[6.0, 6.0], [18.0, 6.0], [6.0, 18.0], [18.0, 18.0]]
        );
        let layout = YieldLayout::new([4, 4, 0, 0, 0, 0], 65.0).unwrap();
        assert_eq!(layout.origins[0].y, layout.origins[1].y);
        assert!(layout.origins[1].x > layout.origins[0].x + 24.0);
        assert!(layout.width <= 65.0);
        assert_eq!(layout.height, 28.0);
        assert!(YieldLayout::new([4, 4, 0, 0, 0, 0], 40.0).is_none());
        let five = YieldLayout::new([5, 0, 0, 0, 0, 0], 40.0).unwrap();
        assert_eq!(five.quantities[0], YieldQuantity::Numbered(5));
        assert_eq!(five.quantities[0].icon_positions().len(), 1);
        assert_eq!(five.height, 16.0);
    }

    #[test]
    fn the_modules_numbers_replace_the_worlds() {
        let cell = |terrain| TerrainCell { tile: TileId(0), terrain, owner: None, resource: None };
        let mut world = RenderWorld { width: 2, height: 1, terrain: vec![cell(1), cell(1)], ..Default::default() };
        let (food, production) = terrain_icon_food_prod(1);
        // No yield layer and no module numbers: the bare terrain.
        assert_eq!(tile_amounts(None, &world, 0, &world.terrain[0]), [food, production, 0, 0, 0, 0]);
        // The module's numbers win over the terrain...
        assert_eq!(tile_amounts(Some([0, 0, 3, 2, 0, 1]), &world, 0, &world.terrain[0]), [0, 0, 3, 2, 0, 1]);
        // ...and over the yield layer.
        world.yields = vec![Default::default(); 2];
        let layer = tile_amounts(None, &world, 1, &world.terrain[1]);
        assert_eq!(layer, world.yields[1].map(i32::from));
        assert_eq!(tile_amounts(Some([7, 0, 0, 0, 0, 0]), &world, 1, &world.terrain[1]), [7, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn yields_require_current_visibility_and_valid_projection() {
        let mut world = RenderWorld {
            width: 3,
            height: 1,
            visible: vec![1, 0],
            explored: vec![1; 3],
            terrain: (0..3)
                .map(|id| TerrainCell {
                    tile: TileId(id),
                    terrain: 1,
                    owner: None,
                    resource: None,
                })
                .collect(),
            ..Default::default()
        };
        assert!(visible_cell(&world, 0).is_some());
        for tile in [1, 2, 3] {
            assert!(visible_cell(&world, tile).is_none());
        }
        world.visible[0] = 0; // Immediate hide even if revision is unchanged.
        assert!(visible_cell(&world, 0).is_none());
    }

    #[test]
    fn a_remembered_tile_shows_the_modules_numbers_dimmer_and_an_unexplored_one_nothing() {
        let world = RenderWorld {
            width: 3,
            height: 1,
            visible: vec![1, 0, 0],
            explored: vec![1, 1, 0],
            terrain: (0..3).map(|id| TerrainCell { tile: TileId(id), terrain: 1, owner: None, resource: None }).collect(),
            yields: vec![[9, 9, 0, 0, 0, 0]; 3],
            ..Default::default()
        };
        let given = Some([4, 1, 0, 0, 0, 0]);
        // In sight: the module's numbers, in full.
        assert_eq!(shown_amounts(&world, 0, given), Some(([4, 1, 0, 0, 0, 0], true)));
        // Explored, out of sight: the module's remembered numbers, dimmer.
        assert_eq!(shown_amounts(&world, 1, given), Some(([4, 1, 0, 0, 0, 0], false)));
        // Unexplored: nothing, whatever the module says.
        assert_eq!(shown_amounts(&world, 2, given), None);
        // A module with no numbers of its own (the classic one): tiles in sight as before, none
        // out of sight.
        assert_eq!(shown_amounts(&world, 0, None), Some(([9, 9, 0, 0, 0, 0], true)));
        assert_eq!(shown_amounts(&world, 1, None), None);
        // A tile that yields nothing has no icons.
        assert_eq!(shown_amounts(&world, 0, Some([0; 6])), None);
    }

    #[test]
    fn a_remembered_quantity_is_drawn_in_a_dimmer_colour() {
        let alphas = |fade: f32| {
            let ctx = egui::Context::default();
            let mut alphas = Vec::new();
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(200.0, 100.0))), ..Default::default() }, |ui| {
                let painter = ui.painter().clone();
                draw_quantity(&painter, pos2(20.0, 20.0), YieldQuantity::Numbered(7), Icon::Apple, Color32::from_rgb(157, 215, 49).gamma_multiply(fade), fade);
            });
            output.textures_delta.clear();
            for clipped in &output.shapes {
                if let egui::Shape::Text(t) = &clipped.shape {
                    alphas.push(t.galley.job.sections[0].format.color.a().max(t.fallback_color.a()));
                }
            }
            alphas
        };
        let (full, dim) = (alphas(1.0), alphas(REMEMBERED));
        assert!(!full.is_empty() && full.len() == dim.len());
        assert!(full.iter().zip(&dim).all(|(f, d)| d < f), "{full:?} against {dim:?}");
    }

    #[test]
    fn anchors_use_cached_hill_surface_and_disappear_when_fog_changes() {
        let mut world = RenderWorld {
            visual_surface: true,
            width: 7,
            height: 7,
            visible: vec![1; 49],
            explored: vec![1; 49],
            terrain: (0..49)
                .map(|id| TerrainCell {
                    tile: TileId(id),
                    terrain: if id == 24 { 2 } else { 1 },
                    owner: None,
                    resource: None,
                })
                .collect(),
            ..Default::default()
        };
        let style =
            render::scene::style::compile(render::scene::style::default_style()).unwrap();
        let surface = render::scene::terrain::surface::build(&world, &style);
        let p = surface_anchor(&surface, &world, 24).expect("known hill anchor");
        let (center_x, center_z) = tile_to_world(3, 3);
        assert_eq!([p[0], p[2]], [center_x, center_z]);
        let ground = surface.sample_display(p[0], p[2]).unwrap();
        assert!((p[1] - ground.y - 0.025).abs() < 0.0001);
        assert!(
            (p[1] - 1.65).abs() > 0.1,
            "must not use a fixed billboard altitude"
        );
        world.visible[24] = 0;
        assert!(surface_anchor(&surface, &world, 24).is_none());
        assert!(known_anchor(&surface, &world, 24).is_some(), "a remembered tile has the known anchor");
        world.explored[24] = 0;
        assert!(known_anchor(&surface, &world, 24).is_none(), "an unexplored tile has none");
    }

    #[test]
    fn labels_keep_their_size_unless_the_module_asks_them_to_follow_the_zoom() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 900.0));
        let mut camera = CameraController::default();
        for zoom in [0.45, 1.0, 2.0] {
            camera.zoom = zoom;
            assert_eq!(label_fit(&camera, screen, false), Fit::CLASSIC, "Nation's labels do not change at zoom {zoom}");
        }
        camera.zoom = 2.0;
        let near = label_fit(&camera, screen, true);
        camera.zoom = 0.45;
        let far = label_fit(&camera, screen, true);
        assert!(near.above && far.above);
        assert!(far.k < near.k, "{far:?} {near:?}");
        assert_eq!(far.k, 0.6, "a far camera keeps the smallest label");
        assert!(near.banners && !far.banners, "a far camera has no banners: the labels would not read");
    }

    /// Where the banners stand at the camera's zooms (the measured hex widths at 1280×720:
    /// 38 points at 1.4, 55 at 2.0, 62 at the shell's start zoom 2.25): the start view has
    /// banners of a readable size, the far one has none.
    #[test]
    fn banners_stand_at_the_start_zoom_and_not_far_out() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0));
        let mut camera = CameraController::default();
        let at = |camera: &mut CameraController, zoom: f32| {
            camera.zoom = zoom;
            label_fit(camera, screen, true)
        };
        let start = at(&mut camera, 2.25);
        assert!(start.banners && 15. * start.k >= 9., "{start:?}");
        assert!(at(&mut camera, 3.0).banners);
        assert!(!at(&mut camera, 1.4).banners, "a hex of 38 points is too narrow for a banner");
        let (shown, hidden): (Vec<f32>, Vec<f32>) = (0..40).map(|i| 0.4 + i as f32 * 0.1).partition(|&zoom| at(&mut camera, zoom).banners);
        assert!(shown.iter().all(|s| hidden.iter().all(|h| s > h)), "banners stand from one zoom on: {shown:?} / {hidden:?}");
        assert!(shown.iter().all(|&zoom| 15. * at(&mut camera, zoom).k >= 9. - 1e-4), "a banner that is drawn has a name of 9 points at least");
        for zoom in [0.4, 1.4, 2.25, 4.0] {
            camera.zoom = zoom;
            assert_eq!(label_fit(&camera, screen, false), Fit::CLASSIC, "Nation's labels are the same at {zoom}");
        }
    }

    #[test]
    fn camera_projects_height_and_hides_yields_at_strategic_zoom() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 900.0));
        let mut camera = CameraController::default();
        camera.zoom = 2.0;
        let near = camera.view_proj(screen.aspect_ratio());
        let ground = project(&near, screen, [0.0, 0.0, 0.0]).unwrap();
        let hill = project(&near, screen, [0.0, 1.4, 0.0]).unwrap();
        assert!(hill.y < ground.y);
        assert!(tile_screen_width(&near, screen, [0.0; 3]).unwrap() >= 54.0);
        camera.zoom = 0.45;
        assert!(
            tile_screen_width(&camera.view_proj(screen.aspect_ratio()), screen, [0.0; 3]).unwrap()
                < 45.0
        );
        assert!(VisualHudState::default().show_yields);
    }
}
