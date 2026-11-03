//! The panels' pictures: each power's map is drawn by a renderer of its own into a
//! texture, only when that power's picture changed, and the grid shows the textures.
//! A renderer keeps what it uploaded, so a round that changes a few tiles of a power's
//! view re-uploads those and not the world; nothing is shared between powers' renderers,
//! so a power's fog and knowledge cannot reach another's panel.
use std::sync::Arc;
use std::time::Duration;

use egui::TextureId;
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::gpu::memory::{Category, GpuTexture};
use render::gpu::terrain_gpu::TerrainSync;
use render::renderer::Renderer;
use render::world::state::RenderWorld;
use render::FrameFlags;

use super::contract::Books;
use super::fit;
use super::frame::Size;

/// The ground textures of the content, shared by the app and the panels' renderers.
pub type TerrainLayers = Arc<Vec<(u32, u32, Vec<u8>)>>;

/// The least time between two moves of a panel's camera (a growing empire is followed
/// a few times a second at most, however fast the rounds go).
const REFRAME_EVERY: Duration = Duration::from_millis(1500);

/// The most city names an enlarged panel writes over the map.
const MAX_LABELS: usize = 8;

/// The height above the ground the outline of an area is drawn at, in world units.
const OUTLINE_HEIGHT: f32 = 0.25;

/// An area of tiles drawn over a panel: every hexagon, and the edges that bound the area
/// (corner positions as shares of the picture across and down).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hexes {
    pub cells: Vec<Vec<egui::Pos2>>,
    pub edges: Vec<[egui::Pos2; 2]>,
}

/// A city's name and where it stands on the panel (0..1 across and down the picture).
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub name: String,
    pub at: egui::Pos2,
}

/// Who moves a panel's camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraMode {
    /// The panel frames the power's cities and sight by itself.
    #[default]
    Follow,
    /// The viewer moved the camera, and it stays where they left it.
    Free,
}

/// What a texture shows: the world's revision, the size it was drawn at and how often
/// the viewer had moved the camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drawn {
    revision: u64,
    size: Size,
    moves: u64,
}

/// The texture a panel draws into and the id egui knows it by.
struct Target {
    texture: GpuTexture,
    view: wgpu::TextureView,
    id: TextureId,
    size: Size,
}

/// One power's renderer, texture and camera.
pub struct PanelView {
    renderer: Renderer,
    camera: Option<CameraController>,
    mode: CameraMode,
    /// How often the viewer moved the camera.
    moves: u64,
    /// When the camera last moved, on the shell's time (`ObserverShell::pace`, the time the frames
    /// add up to, not the wall clock): it is not reframed more often than `REFRAME_EVERY`.
    moved: Option<Duration>,
    target: Option<Target>,
    drawn: Option<Drawn>,
}

impl PanelView {
    fn new(core: &GpuCore, format: wgpu::TextureFormat, layers: &TerrainLayers, books: &Books, sync: TerrainSync) -> Self {
        let mut renderer = Renderer::with_gpu(core, format, layers);
        renderer.set_terrain_sync(sync);
        renderer.set_terrain_lod(true);
        // The panels show a still picture of the round: the water does not move.
        renderer.set_animation_time_override(Some(1.0));
        renderer.set_unit_models(books.units.clone());
        renderer.set_world_models(books.world.clone());
        Self { renderer, camera: None, mode: CameraMode::Follow, moves: 0, moved: None, target: None, drawn: None }
    }

    /// The picture is not what `world` at `size` looks like, or the renderer has work left.
    pub fn stale(&self, world: &RenderWorld, size: Size) -> bool {
        self.drawn != Some(Drawn { revision: world.revision(), size, moves: self.moves }) || self.renderer.has_pending_work()
    }

    pub fn texture(&self) -> Option<TextureId> {
        self.target.as_ref().map(|t| t.id)
    }

    pub fn mode(&self) -> CameraMode {
        self.mode
    }

    /// Back to following (the camera is framed again at the next draw), or to the free
    /// camera, which keeps the camera as it is.
    pub fn set_mode(&mut self, mode: CameraMode) {
        if mode == CameraMode::Follow && self.mode == CameraMode::Free {
            self.camera = None;
            self.moves += 1;
        }
        self.mode = mode;
    }

    /// The viewer drags the map by `dx`, `dy` pixels: the camera is free from then on.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        if let Some(camera) = self.camera.as_mut() {
            camera.pan_by(dx, dy);
            self.mode = CameraMode::Free;
            self.moves += 1;
        }
    }

    /// The viewer turns the wheel: the camera is free from then on.
    pub fn zoom(&mut self, factor: f32) {
        if let Some(camera) = self.camera.as_mut() {
            camera.zoom_by(factor);
            self.mode = CameraMode::Free;
            self.moves += 1;
        }
    }

    /// The names of the power's biggest cities that are on the panel.
    pub fn labels(&self, world: &RenderWorld) -> Vec<Label> {
        let (Some(target), Some(camera)) = (&self.target, &self.camera) else { return Vec::new() };
        let (w, h) = (target.size.width.max(1) as f32, target.size.height.max(1) as f32);
        let mut own: Vec<_> = world.cities.iter().filter(|c| c.owner == world.observer && !c.name.is_empty()).collect();
        own.sort_by_key(|c| (std::cmp::Reverse(c.population), c.tile.0));
        let on_panel = |city: &&render::world::state::CityMarker| {
            let ground = fit::ground_point(world, city.tile.0);
            let (px, py) = camera.project_to_screen([ground.x, 0.4, ground.y], w / h, w, h)?;
            Some(Label { name: city.name.clone(), at: egui::pos2(px / w, py / h) })
        };
        own.iter().filter_map(on_panel).take(MAX_LABELS).collect()
    }

    /// The tile under a point of the picture, given as shares across and down (0..1); None
    /// off the known land, or before the panel was first drawn.
    pub fn pick(&self, world: &RenderWorld, at: egui::Pos2) -> Option<u32> {
        let (target, camera) = (self.target.as_ref()?, self.camera.as_ref()?);
        let (w, h) = (target.size.width as f32, target.size.height as f32);
        self.renderer.pick_tile(world, camera, (at.x * w, at.y * h), (w, h)).map(|tile| tile.0)
    }

    /// Where a tile's centre stands on the picture (shares across and down), if it is on it.
    pub fn project(&self, world: &RenderWorld, tile: u32) -> Option<egui::Pos2> {
        let (target, camera) = (self.target.as_ref()?, self.camera.as_ref()?);
        let (w, h) = (target.size.width as f32, target.size.height as f32);
        let ground = fit::ground_point(world, tile);
        let (x, y) = camera.project_to_screen([ground.x, OUTLINE_HEIGHT, ground.y], w / h, w, h)?;
        Some(egui::pos2(x / w, y / h))
    }

    /// The tiles as hexagons on the picture (shares across and down), each with the edges
    /// it shares with no other tile of the set: the outline of the area. A hexagon with a
    /// corner off the picture is left out.
    pub fn hexes(&self, world: &RenderWorld, tiles: &[u32]) -> Hexes {
        let mut found = Hexes::default();
        let (Some(target), Some(camera)) = (&self.target, &self.camera) else { return found };
        let (w, h) = (target.size.width as f32, target.size.height as f32);
        let width = world.width.max(1);
        let set: std::collections::HashSet<u32> = tiles.iter().copied().collect();
        let corners = render::gpu::buffers::hex_corners();
        for &tile in tiles {
            let ground = fit::ground_point(world, tile);
            let project = |corner: [f32; 2]| {
                let (x, y) = camera.project_to_screen([ground.x + corner[0], OUTLINE_HEIGHT, ground.y + corner[1]], w / h, w, h)?;
                Some(egui::pos2(x / w, y / h))
            };
            let points: Vec<Option<egui::Pos2>> = corners.iter().map(|c| project(*c)).collect();
            if points.iter().any(Option::is_none) {
                continue;
            }
            let ring: Vec<egui::Pos2> = points.into_iter().flatten().collect();
            let (q, r) = ((tile % width) as i32, (tile / width) as i32);
            for (i, (dq, dr)) in render::gpu::buffers::EDGE_NEIGHBORS.iter().enumerate() {
                let (nq, nr) = (q + dq, r + dr);
                let inside = nq >= 0 && nr >= 0 && (nq as u32) < width && (nr as u32) < world.height && set.contains(&(nr as u32 * width + nq as u32));
                if !inside {
                    found.edges.push([ring[i], ring[(i + 1) % 6]]);
                }
            }
            found.cells.push(ring);
        }
        found
    }

    /// The renderer finishes terrain over the next frames.
    pub fn working(&self) -> bool {
        self.renderer.has_pending_work()
    }

    /// Draw `world` at `size` pixels into the panel's texture.
    pub fn render(&mut self, core: &GpuCore, ui: &mut egui_wgpu::Renderer, format: wgpu::TextureFormat, world: &RenderWorld, size: Size, now: Duration) {
        self.ensure_target(core, ui, format, size);
        let key = Drawn { revision: world.revision(), size, moves: self.moves };
        if self.drawn != Some(key) && (self.mode == CameraMode::Follow || self.camera.is_none()) {
            self.reframe(world, size, now);
        }
        let (Some(target), Some(camera)) = (&self.target, &self.camera) else { return };
        let flags = FrameFlags { grid: false, selected_city: None, yields: false };
        self.renderer.draw(core, world, camera, &target.view, (size.width, size.height), flags);
        self.drawn = Some(key);
    }

    /// Frame the power's own cities and sight; the camera follows a growth in steps, and
    /// at high speeds not every round.
    fn reframe(&mut self, world: &RenderWorld, size: Size, now: Duration) {
        let ground = fit::ground(world);
        let target = fit::frame(ground, size);
        let due = self.moved.is_none_or(|at| now.saturating_sub(at) >= REFRAME_EVERY);
        let camera = fit::settle(self.camera.as_ref(), &target, ground, size, due);
        let changed = self.camera.as_ref().is_none_or(|old| (old.x, old.y, old.zoom) != (camera.x, camera.y, camera.zoom));
        if changed {
            self.moved = Some(now);
        }
        self.camera = Some(camera);
    }

    fn ensure_target(&mut self, core: &GpuCore, ui: &mut egui_wgpu::Renderer, format: wgpu::TextureFormat, size: Size) {
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let unorm = format.remove_srgb_suffix();
        let views = if unorm == format { Vec::new() } else { vec![unorm] };
        let texture = core.texture(Category::RenderTargets, &wgpu::TextureDescriptor {
            label: Some("observer panel"),
            size: wgpu::Extent3d { width: size.width, height: size.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &views,
        });
        let view = texture.create_view(&Default::default());
        // egui reads panel pixels as stored (gamma) values, as it does its own textures.
        let shown = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(unorm), ..Default::default() });
        let id = match self.target.take() {
            Some(old) => {
                ui.update_egui_texture_from_wgpu_texture(&core.device, &shown, wgpu::FilterMode::Linear, old.id);
                old.id
            }
            None => ui.register_native_texture(&core.device, &shown, wgpu::FilterMode::Linear),
        };
        self.target = Some(Target { texture, view, id, size });
        self.drawn = None;
        self.camera = None;
    }

    fn release(&mut self, ui: &mut egui_wgpu::Renderer) {
        if let Some(target) = self.target.take() {
            ui.free_texture(&target.id);
            drop(target.texture);
        }
        self.drawn = None;
    }
}

/// The panels of a match: one view per power, made when the power is first drawn.
pub struct Panels {
    views: Vec<Option<PanelView>>,
    format: wgpu::TextureFormat,
    layers: TerrainLayers,
    books: Books,
    sync: TerrainSync,
}

impl Panels {
    pub fn new(count: usize, format: wgpu::TextureFormat, layers: TerrainLayers, books: Books, sync: TerrainSync) -> Self {
        Self { views: (0..count).map(|_| None).collect(), format, layers, books, sync }
    }

    pub fn len(&self) -> usize {
        self.views.len()
    }

    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    pub fn view(&self, power: usize) -> Option<&PanelView> {
        self.views.get(power)?.as_ref()
    }

    /// Does the power's panel need drawing for `world` at `size`? A panel never drawn does.
    pub fn stale(&self, power: usize, world: &RenderWorld, size: Size) -> bool {
        self.view(power).is_none_or(|v| v.stale(world, size))
    }

    pub fn working(&self) -> bool {
        self.views.iter().flatten().any(PanelView::working)
    }

    pub fn texture(&self, power: usize) -> Option<TextureId> {
        self.view(power)?.texture()
    }

    pub fn mode(&self, power: usize) -> CameraMode {
        self.view(power).map_or_else(CameraMode::default, PanelView::mode)
    }

    pub fn set_mode(&mut self, power: usize, mode: CameraMode) {
        if let Some(Some(view)) = self.views.get_mut(power) {
            view.set_mode(mode);
        }
    }

    pub fn pan(&mut self, power: usize, dx: f32, dy: f32) {
        if let Some(Some(view)) = self.views.get_mut(power) {
            view.pan(dx, dy);
        }
    }

    pub fn zoom(&mut self, power: usize, factor: f32) {
        if let Some(Some(view)) = self.views.get_mut(power) {
            view.zoom(factor);
        }
    }

    pub fn pick(&self, power: usize, world: &RenderWorld, at: egui::Pos2) -> Option<u32> {
        self.view(power)?.pick(world, at)
    }

    pub fn project(&self, power: usize, world: &RenderWorld, tile: u32) -> Option<egui::Pos2> {
        self.view(power)?.project(world, tile)
    }

    pub fn hexes(&self, power: usize, world: &RenderWorld, tiles: &[u32]) -> Hexes {
        self.view(power).map(|v| v.hexes(world, tiles)).unwrap_or_default()
    }

    /// Where the power's camera stands (a key of what a cached pick means).
    pub fn pose(&self, power: usize) -> [u32; 3] {
        self.view(power).and_then(|v| v.camera.as_ref()).map_or([0; 3], |c| [c.x.to_bits(), c.y.to_bits(), c.zoom.to_bits()])
    }

    pub fn labels(&self, power: usize, world: &RenderWorld) -> Vec<Label> {
        self.view(power).map(|v| v.labels(world)).unwrap_or_default()
    }

    /// Draw one power's panel (its renderer is made on the first call); `now` is the shell's time.
    pub fn render(&mut self, core: &GpuCore, ui: &mut egui_wgpu::Renderer, power: usize, world: &RenderWorld, size: Size, now: Duration) {
        let (format, layers, books, sync) = (self.format, &self.layers, &self.books, self.sync);
        let Some(slot) = self.views.get_mut(power) else { return };
        let view = slot.get_or_insert_with(|| PanelView::new(core, format, layers, books, sync));
        view.render(core, ui, format, world, size, now);
    }

    /// Free the textures (a new match, or leaving the mode).
    pub fn release(&mut self, ui: &mut egui_wgpu::Renderer) {
        self.views.iter_mut().flatten().for_each(|v| v.release(ui));
    }
}
