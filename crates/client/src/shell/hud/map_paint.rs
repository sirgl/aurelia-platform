//! What a module paints on the map: lenses and overlays in tiles (tinted hexes, route
//! lines with a width and a colour, icons with a label, warnings). The module fills a
//! `MapPaint` from its rules (`ModuleUi::map_paint`); the shell places it on the
//! displayed ground every frame, so the module never projects anything itself.
//!
//! It draws in the map feedback's ground layer before the selection ring and the route
//! preview (egui orders painter-only layers by id, so one shared layer keeps the order),
//! over the world and under the HUD's widgets; tiles under fog are skipped.
use egui::{Align2, Color32, FontId, Pos2, Rect, Shape, Stroke};
use render::camera::CameraController;
use render::gpu::buffers::hex_corners;
use render::scene::layout::tile_to_world;
use render::scene::terrain::surface::SurfaceMesh;
use render::world::state::RenderWorld;

use super::tile_yields::{known_anchor, project, surface_anchor};
use super::{icon, Icon};
use crate::shell::theme::Theme;

/// A tinted hex: a lens's region, a city's land, a cell's load.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileTint {
    pub tile: u32,
    pub fill: Color32,
    /// The hex's outline (None: fill only).
    pub edge: Option<Stroke>,
}

/// A line through tile centres: a supply route, a trade path, a front.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteLine {
    pub tiles: Vec<u32>,
    /// Logical pixels (the load of a route, say).
    pub width: f32,
    pub color: Color32,
}

/// An icon on a tile with an optional line under it.
#[derive(Debug, Clone, PartialEq)]
pub struct TileIcon {
    pub tile: u32,
    pub icon: Icon,
    pub tint: Color32,
    pub label: Option<String>,
}

/// Where a mark stands: on a tile, or on the edge between two neighbouring tiles
/// (a bridge, a ford, a link of a route).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    Tile(u32),
    Edge(u32, u32),
}

/// A warning: a marked badge and its reason ("Bridge: load 6/6").
#[derive(Debug, Clone, PartialEq)]
pub struct MapWarning {
    pub at: Spot,
    pub text: String,
}

/// Everything a module paints on the map this frame, drawn in this order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapPaint {
    pub tints: Vec<TileTint>,
    pub routes: Vec<RouteLine>,
    pub icons: Vec<TileIcon>,
    pub warnings: Vec<MapWarning>,
    /// The paint stands on remembered land too (explored, out of sight), not only on land
    /// in sight; a module that paints only what the player's map holds sets it.
    pub known_land: bool,
}

impl MapPaint {
    pub fn is_empty(&self) -> bool {
        self.tints.is_empty() && self.routes.is_empty() && self.icons.is_empty() && self.warnings.is_empty()
    }
}

/// Share of the hex a tint covers (neighbouring tints keep a seam).
const TINT_INSET: f32 = 0.94;

/// The ground as the shell shows it this frame.
struct Ground<'a> {
    world: &'a RenderWorld,
    surface: &'a SurfaceMesh,
    matrix: [[f32; 4]; 4],
    screen: Rect,
    /// Remembered land counts (`MapPaint::known_land`).
    known: bool,
}

impl Ground<'_> {
    fn centre(&self, tile: u32) -> Option<Pos2> {
        if tile >= self.world.width * self.world.height {
            return None;
        }
        project(&self.matrix, self.screen, self.anchor(tile)?)
    }

    fn anchor(&self, tile: u32) -> Option<[f32; 3]> {
        match self.known {
            true => known_anchor(self.surface, self.world, tile),
            false => surface_anchor(self.surface, self.world, tile),
        }
    }

    fn spot(&self, spot: Spot) -> Option<Pos2> {
        match spot {
            Spot::Tile(tile) => self.centre(tile),
            Spot::Edge(a, b) => Some(self.centre(a)?.lerp(self.centre(b)?, 0.5)),
        }
    }

    /// The hex's corners on the ground (None: the tile is under fog or behind the camera).
    fn hex(&self, tile: u32) -> Option<Vec<Pos2>> {
        if tile >= self.world.width * self.world.height {
            return None;
        }
        let centre = self.anchor(tile)?;
        let (x, z) = tile_to_world(tile % self.world.width, tile / self.world.width);
        hex_corners()
            .into_iter()
            .map(|[dx, dz]| {
                let (cx, cz) = (x + TINT_INSET * dx, z + TINT_INSET * dz);
                let y = self.surface.sample_display(cx, cz).map_or(centre[1], |s| s.y + 0.025);
                project(&self.matrix, self.screen, [cx, y, cz])
            })
            .collect()
    }
}

/// Paint `paint` over the map.
pub fn draw(ctx: &egui::Context, world: &RenderWorld, camera: &CameraController, surface: Option<&SurfaceMesh>, paint: &MapPaint) {
    let Some(surface) = surface else { return };
    if paint.is_empty() || world.width == 0 || world.height == 0 {
        return;
    }
    let screen = ctx.content_rect();
    let ground = Ground { world, surface, matrix: camera.view_proj(screen.aspect_ratio()), screen, known: paint.known_land };
    let painter = crate::shell::map_feedback::ground_painter(ctx).with_clip_rect(screen);
    for tint in &paint.tints {
        if let Some(hex) = ground.hex(tint.tile) {
            painter.add(Shape::convex_polygon(hex.clone(), tint.fill, Stroke::NONE));
            if let Some(edge) = tint.edge {
                painter.add(Shape::closed_line(hex, edge));
            }
        }
    }
    for route in &paint.routes {
        route_line(&painter, &ground, route);
    }
    let theme = Theme::of(ctx);
    for mark in &paint.icons {
        if let Some(at) = ground.centre(mark.tile) {
            tile_icon(&painter, at, mark, theme);
        }
    }
    for warning in &paint.warnings {
        if let Some(at) = ground.spot(warning.at) {
            tile_warning(&painter, at, &warning.text, theme);
        }
    }
}

/// The line with a dark band under it, broken where a tile is under fog.
fn route_line(painter: &egui::Painter, ground: &Ground<'_>, route: &RouteLine) {
    let points: Vec<Option<Pos2>> = route.tiles.iter().map(|&tile| ground.centre(tile)).collect();
    for pair in points.windows(2) {
        if let [Some(a), Some(b)] = *pair {
            painter.line_segment([a, b], Stroke::new(route.width + 3.0, Color32::from_black_alpha(110)));
            painter.line_segment([a, b], Stroke::new(route.width, route.color));
        }
    }
}

fn tile_icon(painter: &egui::Painter, at: Pos2, mark: &TileIcon, theme: &Theme) {
    painter.circle_filled(at, 12.0, theme.colors.hud_fill);
    painter.circle_stroke(at, 12.0, Stroke::new(1.2, mark.tint));
    icon(painter, at, 7.0, mark.icon, mark.tint);
    if let Some(label) = &mark.label {
        outlined(painter, at + egui::vec2(0.0, 15.0), label, theme.colors.hud_text);
    }
}

fn tile_warning(painter: &egui::Painter, at: Pos2, text: &str, theme: &Theme) {
    let danger = theme.colors.danger;
    painter.circle_filled(at, 10.0, danger);
    painter.circle_stroke(at, 10.0, Stroke::new(1.5, theme.colors.hud_text));
    painter.text(at, Align2::CENTER_CENTER, "!", FontId::proportional(14.0), theme.colors.hud_text);
    outlined(painter, at + egui::vec2(0.0, 13.0), text, danger.lerp_to_gamma(Color32::WHITE, 0.35));
}

/// A line of text under a point, legible on any ground.
fn outlined(painter: &egui::Painter, top: Pos2, text: &str, color: Color32) {
    let font = FontId::proportional(12.0);
    for offset in [egui::vec2(1.0, 1.0), egui::vec2(-1.0, 1.0)] {
        painter.text(top + offset, Align2::CENTER_TOP, text, font.clone(), Color32::from_black_alpha(220));
    }
    painter.text(top, Align2::CENTER_TOP, text, font, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::ids::TileId;
    use render::world::state::TerrainCell;

    fn world() -> RenderWorld {
        RenderWorld {
            visual_surface: true,
            width: 7,
            height: 7,
            visible: vec![1; 49],
            explored: vec![1; 49],
            terrain: (0..49).map(|id| TerrainCell { tile: TileId(id), terrain: 1, owner: None, resource: None }).collect(),
            ..Default::default()
        }
    }

    const SCREEN: Rect = Rect { min: Pos2::ZERO, max: egui::pos2(1600.0, 900.0) };

    /// The ground of `world` with the camera on its middle tile.
    fn view(world: &RenderWorld) -> (SurfaceMesh, CameraController) {
        let style = render::scene::style::compile(render::scene::style::default_style()).unwrap();
        let mut camera = CameraController::default();
        (camera.x, camera.y) = tile_to_world(3, 3);
        camera.zoom = 2.0;
        (render::scene::terrain::surface::build(world, &style), camera)
    }

    /// The shapes `paint` puts on screen over `world`.
    fn painted(world: &RenderWorld, paint: &MapPaint) -> Vec<Shape> {
        let (surface, camera) = view(world);
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(SCREEN), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| draw(ui.ctx(), world, &camera, Some(&surface), paint));
        output.textures_delta.clear();
        output.shapes.into_iter().map(|clipped| clipped.shape).collect()
    }

    fn texts(shapes: &[Shape]) -> Vec<String> {
        shapes.iter().filter_map(|s| if let Shape::Text(t) = s { Some(t.galley.text().to_owned()) } else { None }).collect()
    }

    #[test]
    fn tints_routes_icons_and_warnings_land_on_their_tiles() {
        let red = Color32::from_rgb(200, 40, 40);
        let paint = MapPaint {
            tints: vec![TileTint { tile: 24, fill: red, edge: Some(Stroke::new(1.0, Color32::WHITE)) }],
            routes: vec![RouteLine { tiles: vec![23, 24, 25], width: 4.0, color: red }],
            icons: vec![TileIcon { tile: 24, icon: Icon::Apple, tint: red, label: Some("+3".into()) }],
            warnings: vec![MapWarning { at: Spot::Edge(24, 25), text: "Bridge: load 6/6".into() }],
            ..Default::default()
        };
        let world = world();
        let shapes = painted(&world, &paint);
        let hex = shapes.iter().find_map(|s| match s {
            Shape::Path(path) if path.fill == red => Some(path.points.clone()),
            _ => None,
        });
        let hex = hex.expect("the tinted hex");
        assert_eq!(hex.len(), 6);
        let (surface, camera) = view(&world);
        let centre = super::super::tile_on_screen(&world, &camera, &surface, SCREEN, 24).unwrap();
        let inside = hex.iter().fold(Rect::NOTHING, |r, p| r.union(Rect::from_min_max(*p, *p)));
        assert!(inside.contains(centre), "the hex surrounds its tile's centre");
        let route = shapes.iter().filter(|s| matches!(s, Shape::LineSegment { stroke, .. } if stroke.color == red && stroke.width == 4.0));
        assert_eq!(route.count(), 2, "two segments through three tiles");
        let words = texts(&shapes);
        assert!(words.iter().any(|t| t == "+3") && words.iter().any(|t| t == "Bridge: load 6/6") && words.iter().any(|t| t == "!"));
    }

    #[test]
    fn fog_and_unknown_tiles_paint_nothing() {
        let mut world = world();
        world.visible[24] = 0;
        let paint = MapPaint {
            tints: vec![TileTint { tile: 24, fill: Color32::RED, edge: None }, TileTint { tile: 999, fill: Color32::RED, edge: None }],
            routes: vec![RouteLine { tiles: vec![24, 25], width: 3.0, color: Color32::RED }],
            icons: vec![TileIcon { tile: 999, icon: Icon::Coin, tint: Color32::RED, label: None }],
            warnings: vec![MapWarning { at: Spot::Tile(24), text: "hidden".into() }, MapWarning { at: Spot::Edge(25, 24), text: "half hidden".into() }],
            ..Default::default()
        };
        assert!(painted(&world, &paint).is_empty());
        assert!(painted(&world, &MapPaint::default()).is_empty());
    }

    #[test]
    fn remembered_land_takes_a_mark_only_from_a_paint_that_asks_for_it() {
        let mark = |tile: u32, label: &str| TileIcon { tile, icon: Icon::Apple, tint: Color32::RED, label: Some(label.into()) };
        let mut world = world();
        world.visible[24] = 0;
        world.explored[25] = 0;
        world.visible[25] = 0;
        let paint = |known_land| MapPaint { icons: vec![mark(24, "remembered"), mark(25, "unexplored")], known_land, ..Default::default() };
        assert!(painted(&world, &paint(false)).is_empty(), "the common paint stays off remembered land");
        let words = texts(&painted(&world, &paint(true)));
        assert!(words.iter().any(|w| w == "remembered") && !words.iter().any(|w| w == "unexplored"), "remembered land has its mark, unexplored land none: {words:?}");
    }
}
