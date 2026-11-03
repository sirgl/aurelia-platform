//! Renderer: terrain pieces, water, features, instanced models and overlays.
//!
//! The terrain (ground, water and grid of each surface region) is built off the frame
//! by the terrain worker and published whole (`gpu::terrain_gpu`); `ground` below is
//! the generation of the published terrain. Every other GPU product remembers the
//! layer revisions it was built from and is rebuilt only when one of them changes (see
//! `world::state::Layer`):
//!
//! | product                         | inputs                                         |
//! |---------------------------------|------------------------------------------------|
//! | terrain pieces                  | terrain (worker), per changed region           |
//! | road material                   | ground, materials                              |
//! | borders                         | ground, borders                                |
//! | feature chunks (trees, props)   | ground (changed areas), materials, features, units, cities |
//! | unit instances                  | ground (changed areas), units, cities, knowledge |
//! | city tiles and world wonders    | ground (changed areas), cities, features       |
//! | world props (improvements, resources, natural wonders) | ground (changed areas), materials, features + wonder sites |
//! | fog field                       | knowledge                                      |
//! | shadow map (GPU only)           | casters above + knowledge + sun + shadow area  |
//!
//! CPU builders read the knowledge view (`scene::fog::KnowledgeView`), so their output
//! depends on knowledge, never on current visibility; shaders apply visibility from the
//! fog field. A unit move in explored land therefore uploads unit instances, a few
//! feature chunks and the changed rows of the tile mask — no terrain. While a newer
//! terrain is being built, the fog field and the products of the knowledge view keep
//! showing the published one, so the map never shows knowledge its ground lacks.
//! GPU state is disposable and rebuilt from the CPU RenderWorld after device loss (§38).

use crate::camera::CameraController;
use crate::gpu::buffers::{
    build_border_lines, build_city_highlight, build_ribbon_tris, build_yield_instances, HexInstance,
    TerrainVertex, BORDER_LIFT, BORDER_WIDTH, SELECTION_LIFT, SELECTION_WIDTH,
};
use crate::gpu::context::GpuCore;
use crate::gpu::dynamic_buffer::DynamicBuffer;
use crate::gpu::memory::{Category, GpuBuffer, GpuTexture};
use crate::gpu::budget::Budget;
use crate::gpu::feature_chunks::FeatureChunks;
use crate::gpu::fog_field::FogField;
use crate::gpu::pipelines;
use crate::gpu::scene_scale::{self, ScaleBlit, ScaledColor};
use crate::gpu::surface_shadows::{CasterStrides, Casters, SurfaceShadows};
use crate::gpu::textures::TerrainTextures;
use crate::scene::fog::KnowledgeView;
use crate::gpu::terrain_gpu::{Terrain, TerrainSync};
use crate::gpu::terrain_job::Changed;
use crate::scene::heightfield::Heightfield;
use crate::scene::style::{compile, default_style, CompiledTerrainStyle};
use crate::scene::terrain::fields::TerrainGeometry;
use crate::world::state::{LayerRevisions, RenderWorld};

const TERRAIN_SHADER: &str = include_str!("../../../assets/shaders/terrain.wgsl");
const UNIT_SHADER: &str = include_str!("../../../assets/shaders/hex.wgsl");
const GRID_SHADER: &str = include_str!("../../../assets/shaders/grid.wgsl");
const BORDER_SHADER: &str = include_str!("../../../assets/shaders/border.wgsl");
const ICON_SHADER: &str = include_str!("../../../assets/shaders/icon.wgsl");
const WATER_SHADER: &str = include_str!("../../../assets/shaders/water.wgsl");
const EFFECTS_SHADER: &str = include_str!("../../../assets/shaders/effects.wgsl");
const FEATURE_SHADER: &str = include_str!("../../../assets/shaders/feature.wgsl");

/// Direction TO the sun + ambient at noon (AURELIA_DAY_T = 0.5).
const SUN_DIR: [f32; 3] = [-0.42, 0.80, 0.35];
/// Per frame, rebuilds of feature and border chunks stop starting this long after the
/// frame's preparation began (a terrain publish included); the rest wait for the next
/// frames (`gpu::budget`).
const CHUNK_ALLOWANCE: std::time::Duration = std::time::Duration::from_millis(5);
const AMBIENT: f32 = 0.32;

/// Sun direction and ambient for a time of day t (0..1; 0.5 is noon = the constants).
fn sun_for_time_of_day(t: f32) -> ([f32; 3], f32) {
    let t = t.clamp(0.0, 1.0);
    let h = (std::f32::consts::PI * t).sin().max(0.0);
    let dir = [
        SUN_DIR[0] + (0.5 - t) * 1.2,
        (SUN_DIR[1] * (0.15 + 0.85 * h)).max(0.1),
        SUN_DIR[2],
    ];
    (dir, AMBIENT * (0.35 + 0.65 * h))
}

fn current_sun() -> ([f32; 3], f32) {
    let t: f32 = std::env::var("AURELIA_DAY_T")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.5);
    sun_for_time_of_day(t)
}

/// Frame metrics (§40): work and uploads, not only FPS.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderStats {
    pub draw_calls: u32,
    pub chunks_total: usize,
    pub chunks_drawn: usize,
    pub terrain_tris: usize,
    pub water_tris: usize,
    /// Shadow-map triangles submitted this frame (zero when the map is unchanged).
    pub shadow_tris: usize,
    pub unit_instances: usize,
    /// Figures of the units the presentation poses this frame (uploaded every such frame).
    pub posed_figures: usize,
    /// Effect particles drawn this frame (explosions, projectiles, smoke).
    pub effect_particles: u32,
    /// Unit triangles submitted this frame (all formations, chosen LODs).
    pub unit_tris: usize,
    /// City piece and world wonder instances.
    pub city_instances: usize,
    /// Improvement, resource and natural wonder instances.
    pub prop_instances: usize,
    /// World object triangles submitted this frame (props and cities, chosen LODs).
    pub world_tris: usize,
    pub world_draws: u32,
    pub unit_draws: u32,
    pub city_draws: u32,
    /// Static feature, feature chunk and decal draws.
    pub feature_draws: u32,
    pub grid_segments: usize,
    pub border_segments: usize,
    pub river_segments: usize,
    pub selected_segments: usize,
    /// Resident feature triangles (static + chunks + legacy decals).
    pub feature_tris: usize,
    pub icon_instances: usize,
    pub texture_layers: u32,
    /// All bytes written to GPU memory this frame, except the camera uniform.
    pub bytes_uploaded: u64,
    pub camera_bytes: u64,
    /// Terrain chunks, water and grid.
    pub terrain_bytes: u64,
    /// Feature chunks, static features and legacy decals.
    pub feature_bytes: u64,
    /// Unit and city instances.
    pub instance_bytes: u64,
    /// Tile knowledge mask (the fog field itself is baked on the GPU).
    pub knowledge_bytes: u64,
    /// Borders, road material, selection, icons and shadow fit.
    pub overlay_bytes: u64,
    /// Terrain updates applied this frame (0 or 1, more when the worker caught up).
    pub geometry_rebuilds: u32,
    /// Regions the applied updates triangulated again, and the worker time they took.
    pub regions_rebuilt: u32,
    pub terrain_build_ms: f32,
    /// A newer terrain is being built: the frame shows the published one.
    pub terrain_pending: bool,
    /// Any terrain request (new terrain or new detail levels) is being built.
    pub terrain_building: bool,
    /// Feature or border chunks wait for a later frame's budget.
    pub chunks_pending: bool,
    pub feature_chunks_rebuilt: u32,
    /// Border chunks rebuilt this frame.
    pub border_chunks_rebuilt: u32,
    /// Fog field texel rows re-baked on the GPU this frame.
    pub fog_rows_baked: u32,
    /// CPU time from the start of `draw` to queue submission.
    pub prepare_ms: f32,
    /// Of `prepare_ms`: the terrain step (publishing built terrain, requesting levels).
    pub terrain_step_ms: f32,
    /// Of `prepare_ms`: the shadow fit, the static map and the units drawn over it.
    pub shadow_ms: f32,
    /// Of `prepare_ms`: recording the scene pass (culling and draw calls).
    pub encode_ms: f32,
}

/// UI frame flags (layers on/off without touching the simulation, §39).
#[derive(Debug, Clone, Copy)]
pub struct FrameFlags {
    pub grid: bool,
    /// Highlight the work area of the city on this tile. Raw tile id.
    pub selected_city: Option<u32>,
    /// Yield icons on tiles.
    pub yields: bool,
}

impl Default for FrameFlags {
    fn default() -> Self {
        Self {
            grid: true,
            selected_city: None,
            yields: true,
        }
    }
}

/// Per-frame camera inputs of scene encoding.
struct SceneView {
    planes: [glam::Vec4; 6],
    /// Screen pixels per hex unit at the view centre (unit LOD choice).
    px_per_unit: f32,
    /// Eye position and its distance to the view centre (world-object LOD per cell).
    eye: [f32; 3],
    focus: f32,
}

/// Layer revisions each product was last built from.
#[derive(Default)]
struct Built {
    road: Option<[u64; 2]>,
    borders: Option<[u64; 2]>,
    static_features: Option<[u64; 2]>,
    feature_chunks: Option<[u64; 5]>,
    decals: Option<[u64; 5]>,
    units: Option<[u64; 4]>,
    cities: Option<[u64; 3]>,
    props: Option<([u64; 3], u64)>,
    icons: Option<[u64; 5]>,
    knowledge: Option<u64>,
    shadows: Option<ShadowKey>,
    static_shadows: Option<StaticShadowKey>,
    selection: Option<(Option<u32>, [u64; 2])>,
}

#[derive(Clone, Copy, PartialEq)]
struct ShadowKey {
    layers: [u64; 4],
    poses: u64,
    /// Rebuilds of the resting unit instances (the posed set changes them).
    units_built: u64,
    features: u64,
    sun: [u32; 3],
    area: Option<[[u32; 2]; 2]>,
}

/// World units around a posed figure's position that its shadow caster can reach (a lunge, a
/// long gun, a ship's hull): the patch of the shadow map redrawn each frame.
const POSED_SHADOW_REACH: f32 = 1.8;

/// Terrain and cities revisions, feature generation, sun and shadow area bits.
type StaticShadowKey = ([u64; 2], u64, [u32; 3], Option<[[u32; 2]; 2]>);

/// Product keys: layer revisions, with `terrain` standing for the generation of the
/// published ground (products stand on what is on screen, not on what is building).
struct Keys(LayerRevisions);

impl Keys {
    fn new(mut layers: LayerRevisions, ground: u64) -> Self {
        layers.terrain = ground;
        Self(layers)
    }
    fn road(&self) -> [u64; 2] {
        [self.0.terrain, self.0.materials]
    }
    fn borders(&self) -> [u64; 2] {
        [self.0.terrain, self.0.borders]
    }
    fn static_features(&self) -> [u64; 2] {
        // Legacy coast props avoid city tiles.
        [self.0.terrain, self.0.cities]
    }
    fn feature_chunks(&self) -> [u64; 5] {
        let l = &self.0;
        [l.terrain, l.materials, l.features, l.units, l.cities]
    }
    fn decals(&self) -> [u64; 5] {
        let l = &self.0;
        [l.terrain, l.features, l.units, l.cities, l.knowledge]
    }
    fn units(&self) -> [u64; 4] {
        let l = &self.0;
        [l.terrain, l.units, l.cities, l.knowledge]
    }
    fn cities(&self) -> [u64; 3] {
        // Wonder sites avoid natural wonders (features); knowledge reaches the GPU as fog.
        [self.0.terrain, self.0.cities, self.0.features]
    }
    fn props(&self) -> [u64; 3] {
        [self.0.terrain, self.0.materials, self.0.features]
    }
    fn icons(&self) -> [u64; 5] {
        let l = &self.0;
        [l.terrain, l.materials, l.features, l.borders, l.cities]
    }
    fn selection(&self) -> [u64; 2] {
        [self.0.terrain, self.0.cities]
    }
}

struct Pipelines {
    terrain: wgpu::RenderPipeline,
    unit: wgpu::RenderPipeline,
    grid: wgpu::RenderPipeline,
    ribbon: wgpu::RenderPipeline,
    soft_border: wgpu::RenderPipeline,
    icon: wgpu::RenderPipeline,
    water: wgpu::RenderPipeline,
    feature: wgpu::RenderPipeline,
    effects: wgpu::RenderPipeline,
}

pub struct GpuRenderer {
    pipelines: Pipelines,
    effects_gpu: crate::gpu::effects::EffectsGpu,
    /// Ambient emitters and the pillage cues (ambient_fx.rs).
    ambient: crate::gpu::ambient_fx::Ambient,
    /// Per tile: in sight and pillaged at the last build of the props (for the cues).
    seen_pillaged: Vec<(u8, bool)>,
    road_material: crate::gpu::road_material::RoadMaterial,
    started_at: web_time::Instant,
    animation_time_override: Option<f32>,
    texture_bind: wgpu::BindGroup,
    textures: TerrainTextures,
    icons_bind: wgpu::BindGroup,
    /// The icon atlas lives as long as the renderer.
    _icons_tex: crate::gpu::textures::IconTextures,
    camera_buf: GpuBuffer,
    fog: FogField,
    /// Group 0 of every scene pipeline: camera + fog field.
    scene_bind: wgpu::BindGroup,
    scene_bind_generation: u64,
    style: CompiledTerrainStyle,
    /// Ground, water and grid of the published terrain, and the worker building the next.
    terrain: Terrain,
    /// Far regions are meshed coarser (platform/MESHING.md §2.5, LOD).
    terrain_lod: bool,
    /// World XZ of the view centre: deferred rebuilds start nearest to it.
    focus: [f32; 2],
    /// Territory bands in chunks of one region of cells.
    borders: crate::gpu::soft_borders::BorderChunks,
    selection: DynamicBuffer,
    selection_segments: usize,
    static_features: DynamicBuffer,
    feature_chunks: FeatureChunks,
    /// Legacy flat-renderer shadow decals (alpha-blended, drawn last).
    decals: DynamicBuffer,
    static_generation: u64,
    /// Every LOD of every unit model (crate::unit_models::vertices()).
    unit_mesh: GpuBuffer,
    unit_instances: DynamicBuffer,
    /// Instance batches per model and the per-frame LOD choice.
    unit_batches: crate::gpu::unit_batches::UnitBatches,
    /// Units the presentation draws this frame, their figures and instances.
    poses: Vec<crate::gpu::unit_poses::UnitPose>,
    posed: crate::gpu::unit_poses::PosedFigures,
    posed_instances: DynamicBuffer,
    /// Bumped every frame that draws posed units (and once when they end): the unit
    /// shadows follow them.
    pose_generation: u64,
    units_generation: u64,
    /// Every LOD of every world model (crate::world_models::vertices()).
    world_mesh: GpuBuffer,
    city_instances: DynamicBuffer,
    prop_instances: DynamicBuffer,
    /// City tiles, wonders and tile props grouped by model, with their LOD choice.
    world_objects: crate::gpu::world_objects::WorldObjects,
    /// Bumped when city or prop instances change (static shadow casters follow).
    world_generation: u64,
    icon_mesh_buf: GpuBuffer,
    icon_mesh_count: u32,
    icons: DynamicBuffer,
    /// The knowledge view CPU builders read, synced layer by layer.
    knowledge: KnowledgeView,
    shadows: SurfaceShadows,
    /// The part of the world the shadow map covers (follows the camera).
    shadow_area: crate::gpu::shadow_fit::ShadowArea,
    /// Box of the known tiles and the knowledge revision it was measured at.
    known_area: Option<(u64, Option<crate::gpu::shadow_fit::Area>)>,
    rock_pick_positions: Vec<[f32; 3]>,
    built: Built,
    depth_view: Option<wgpu::TextureView>,
    msaa_view: Option<wgpu::TextureView>,
    /// The depth and MSAA textures of the views (counted in the GPU memory ledger).
    targets: Vec<GpuTexture>,
    color_format: wgpu::TextureFormat,
    depth_size: (u32, u32),
    /// Samples per pixel the scene pass is drawn with (1 or 4); the pipelines and the
    /// depth and color targets follow it at the next frame.
    samples: u32,
    /// Samples the pipelines were built for.
    pipeline_samples: u32,
    /// Samples of the depth target as built.
    depth_samples: u32,
    /// Bind group layouts the pipelines are rebuilt from when `samples` changes.
    texture_layout: wgpu::BindGroupLayout,
    icons_layout: wgpu::BindGroupLayout,
    /// Fraction of the target size the scene pass renders at (1.0: straight into the target).
    render_scale: f32,
    /// Upscale pass for `render_scale` < 1.
    scale_blit: ScaleBlit,
    /// Single-sample scene color at the scaled size; None at full scale.
    scaled_color: Option<ScaledColor>,
    stats: RenderStats,
    timing: Option<crate::gpu::timing::GpuTiming>,
    /// The device's ledger of live buffer and texture bytes.
    memory: std::sync::Arc<crate::gpu::memory::GpuMemory>,
}

fn texture_bind_group(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// Bind group layouts shared by the scene pipelines (groups 0..=3).
struct Layouts<'a> {
    scene: &'a wgpu::BindGroupLayout,
    textures: &'a wgpu::BindGroupLayout,
    icons: &'a wgpu::BindGroupLayout,
    shadow: &'a wgpu::BindGroupLayout,
    road: &'a wgpu::BindGroupLayout,
    effects: &'a wgpu::BindGroupLayout,
}

fn create_pipelines(device: &wgpu::Device, format: wgpu::TextureFormat, samples: u32, l: &Layouts<'_>) -> Pipelines {
    let (scene, textures, shadow) = (l.scene, l.textures, l.shadow);
    Pipelines {
        terrain: pipelines::terrain_pipeline(
            device,
            TERRAIN_SHADER,
            scene,
            textures,
            shadow,
            l.road,
            format,
            samples,
        ),
        unit: pipelines::unit_pipeline(device, UNIT_SHADER, scene, textures, shadow, format, samples),
        grid: pipelines::grid_pipeline(device, GRID_SHADER, scene, format, samples),
        ribbon: pipelines::ribbon_pipeline(device, BORDER_SHADER, scene, format, samples),
        soft_border: pipelines::soft_border_pipeline(device, scene, format, samples),
        icon: pipelines::icon_pipeline(device, ICON_SHADER, scene, l.icons, format, samples),
        water: pipelines::water_pipeline(device, WATER_SHADER, scene, textures, shadow, format, samples),
        feature: pipelines::feature_pipeline(device, FEATURE_SHADER, scene, textures, shadow, format, samples),
        effects: pipelines::effects_pipeline(device, EFFECTS_SHADER, scene, l.effects, format, samples),
    }
}

impl GpuRenderer {
    /// layers: (width, height, rgba) raw pixels (`ground::layers`).
    pub fn new(
        core: &GpuCore,
        format: wgpu::TextureFormat,
        layers: &[(u32, u32, Vec<u8>)],
    ) -> Self {
        let device = &core.device;
        let style = compile(default_style()).expect("default terrain style must compile");
        // Camera: view_proj (64B) + sun xyz + ambient (16B) + time (16B) = 96B.
        let camera_buf = core.buffer(
            Category::Uniforms,
            &wgpu::BufferDescriptor {
                label: Some("camera"),
                size: 96,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        );
        let fog = FogField::new(core);
        let scene_bind = fog.scene_bind_group(device, &camera_buf);
        let shadows = SurfaceShadows::new(
            core,
            CasterStrides {
                terrain: std::mem::size_of::<TerrainVertex>() as u64,
                feature: std::mem::size_of::<crate::gpu::feature::GpuFeatureVertex>() as u64,
                model: 40,
                instance: std::mem::size_of::<HexInstance>() as u64,
            },
        );
        let textures =
            crate::gpu::textures::build_array(core, layers).expect("terrain textures must build");
        let texture_layout = pipelines::terrain_texture_layout(device);
        let texture_bind = texture_bind_group(
            device,
            "terrain texture bind",
            &texture_layout,
            &textures.view,
            &textures.sampler,
        );
        let road_material = crate::gpu::road_material::RoadMaterial::new(core);
        let icons_layout = pipelines::icons_texture_layout(device);
        let effects_gpu = crate::gpu::effects::EffectsGpu::new(core);
        let layouts = Layouts {
            scene: &fog.layout,
            textures: &texture_layout,
            icons: &icons_layout,
            shadow: &shadows.layout,
            road: &road_material.layout,
            effects: &effects_gpu.layout,
        };
        let samples = pipelines::msaa_samples();
        let pipelines = create_pipelines(device, format, samples, &layouts);
        let (aw, ah, apx) = crate::gpu::buffers::build_yield_atlas();
        let icons_tex =
            crate::gpu::textures::build_icons(core, aw, ah, &apx).expect("icons must build");
        let icons_bind = texture_bind_group(
            device,
            "icons bind",
            &icons_layout,
            &icons_tex.view,
            &icons_tex.sampler,
        );
        // Quad: two triangles with uv corners.
        let icon_mesh: [[f32; 2]; 6] = [
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 1.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        ];
        let icon_mesh_buf = core.buffer(
            Category::Overlays,
            &wgpu::BufferDescriptor {
                label: Some("icon mesh"),
                size: (icon_mesh.len() * 8) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        );
        core.queue
            .write_buffer(&icon_mesh_buf, 0, bytemuck::cast_slice(&icon_mesh));
        Self {
            pipelines,
            effects_gpu,
            ambient: Default::default(),
            seen_pillaged: Vec::new(),
            road_material,
            texture_bind,
            textures,
            icons_bind,
            _icons_tex: icons_tex,
            camera_buf,
            scene_bind,
            scene_bind_generation: fog.generation,
            fog,
            terrain: Terrain::new(style.clone()),
            terrain_lod: false,
            focus: [0.0; 2],
            style,
            borders: Default::default(),
            selection: DynamicBuffer::vertex(core, Category::Overlays, "selection"),
            selection_segments: 0,
            static_features: DynamicBuffer::vertex(core, Category::Features, "static features"),
            feature_chunks: FeatureChunks::default(),
            decals: DynamicBuffer::vertex(core, Category::Features, "legacy shadow decals"),
            static_generation: 0,
            unit_mesh: buffer_with(
                core,
                Category::Models,
                "unit mesh",
                wgpu::BufferUsages::VERTEX,
                crate::unit_models::vertices(),
            ),
            unit_instances: DynamicBuffer::vertex(core, Category::UnitInstances, "unit instances"),
            unit_batches: Default::default(),
            poses: Vec::new(),
            posed: Default::default(),
            posed_instances: DynamicBuffer::vertex(core, Category::UnitInstances, "posed unit instances"),
            pose_generation: 0,
            units_generation: 0,
            world_mesh: buffer_with(
                core,
                Category::Models,
                "world model mesh",
                wgpu::BufferUsages::VERTEX,
                &crate::gpu::world_anim::encoded_vertices(),
            ),
            city_instances: DynamicBuffer::vertex(core, Category::WorldInstances, "city instances"),
            prop_instances: DynamicBuffer::vertex(core, Category::WorldInstances, "world prop instances"),
            world_objects: Default::default(),
            world_generation: 0,
            icon_mesh_buf,
            icon_mesh_count: icon_mesh.len() as u32,
            icons: DynamicBuffer::vertex(core, Category::Overlays, "icon instances"),
            knowledge: KnowledgeView::default(),
            shadows,
            shadow_area: Default::default(),
            known_area: None,
            rock_pick_positions: Vec::new(),
            built: Built::default(),
            depth_view: None,
            msaa_view: None,
            targets: Vec::new(),
            color_format: format,
            depth_size: (0, 0),
            samples,
            pipeline_samples: samples,
            depth_samples: samples,
            texture_layout,
            icons_layout,
            render_scale: scene_scale::scale_from_env(),
            scale_blit: ScaleBlit::new(device, format),
            scaled_color: None,
            stats: RenderStats::default(),
            timing: core.timing.clone(),
            memory: core.memory.clone(),
            started_at: web_time::Instant::now(),
            animation_time_override: None,
        }
    }

    fn geom(&self) -> &TerrainGeometry {
        self.terrain
            .geometry()
            .expect("terrain geometry is published before dependent products")
    }

    fn hf(&self) -> &Heightfield {
        self.terrain
            .heightfield()
            .expect("terrain geometry is published before dependent products")
    }

    /// Owner borders: soft ribbons draped on the surface, rebuilt chunk by chunk where
    /// owners or the ground under them changed.
    fn refresh_borders(&mut self, core: &GpuCore, world: &RenderWorld, ground: Option<&Changed>, budget: Budget) -> u64 {
        let geometry = self.terrain.published().map(|p| p.geometry.clone()).expect("geometry published first");
        match &geometry.surface {
            Some(surface) => {
                let update = self.borders.update(core, world, surface, ground, budget);
                self.stats.border_chunks_rebuilt = update.chunks_rebuilt as u32;
                update.bytes
            }
            None => {
                let lines = build_border_lines(world, self.hf());
                let tris = crate::gpu::soft_borders::legacy(&build_ribbon_tris(&lines, BORDER_WIDTH, self.hf(), BORDER_LIFT));
                self.borders.set_legacy(core, &tris, lines.len() / 2)
            }
        }
    }

    /// Geometry-bound features: legacy rocks and coast, showcase rivers and bridge.
    fn refresh_static_features(&mut self, core: &GpuCore, view: &RenderWorld) -> u64 {
        let geom = self.geom();
        let mut mesh = crate::gpu::feature::build_rock_mesh(view, geom);
        let rocks: Vec<[f32; 3]> = mesh.iter().map(|v| v.pos).collect();
        if geom.surface.is_none() {
            mesh.extend(crate::gpu::visual_coast::build(view, geom));
        }
        mesh.extend(crate::gpu::visual_river::build(view, geom));
        for vertex in &mut mesh {
            let wash = crate::scene::fog::fog_at_position(view, vertex.pos[0], vertex.pos[2]);
            vertex.fog = vertex.fog.min(wash);
        }
        self.rock_pick_positions = rocks;
        self.static_generation += 1;
        self.static_features.write(core, &crate::gpu::feature::to_gpu(&mesh))
    }

    /// Legacy flat-renderer drop shadows under trees, cities and units.
    fn refresh_decals(&mut self, core: &GpuCore, world: &RenderWorld) -> u64 {
        if self.geom().surface.is_some() {
            return self.decals.write::<crate::gpu::feature::GpuFeatureVertex>(core, &[]);
        }
        let mesh = crate::gpu::visual_shadows::build(world, self.geom());
        self.decals.write(core, &crate::gpu::feature::to_gpu(&mesh))
    }

    /// Formation figures grouped by model (model chosen by content key); only the
    /// instances that changed are written.
    fn refresh_units(&mut self, core: &GpuCore, world: &RenderWorld) -> u64 {
        let geom = self.terrain.geometry().expect("geometry published first");
        let Some(changed) = self.unit_batches.build(world, &self.style, geom) else {
            return 0;
        };
        self.unit_instances.write_range(core, self.unit_batches.instances(), changed)
    }

    /// City tiles from the kit and world wonders on their rings (also the wonder sites).
    fn refresh_cities(&mut self, core: &GpuCore, world: &RenderWorld) -> u64 {
        let geom = self.terrain.geometry().expect("geometry published first");
        let instances = self.world_objects.build_cities(world, geom);
        self.world_generation += 1;
        self.city_instances.write(core, &instances)
    }

    /// Improvements, resources and natural wonders of known tiles.
    fn refresh_props(&mut self, core: &GpuCore, world: &RenderWorld) -> u64 {
        let geom = self.terrain.geometry().expect("geometry published first");
        let instances = self.world_objects.build_props(world, geom);
        self.world_generation += 1;
        let cues = pillage_cues(&mut self.seen_pillaged, world, geom);
        self.ambient.set_sites(self.world_objects.emitter_sites(), cues);
        self.prop_instances.write(core, &instances)
    }

    /// Selected city work area: cached by (tile, terrain, cities).
    fn refresh_selection(
        &mut self,
        core: &GpuCore,
        world: &RenderWorld,
        selected: Option<u32>,
    ) -> u64 {
        let lines = match selected {
            Some(t) if (t as usize) < world.terrain.len() => {
                let tile = kernel::ids::TileId(t);
                let unworked: Vec<kernel::ids::TileId> = world
                    .cities
                    .iter()
                    .find(|m| m.tile == tile)
                    .map(|m| m.unworked.iter().map(|u| kernel::ids::TileId(*u)).collect())
                    .unwrap_or_default();
                let radius = kernel::world::city::CITY_WORK_RADIUS;
                build_city_highlight(world, self.hf(), tile, radius, &unworked)
            }
            _ => Vec::new(),
        };
        let surface = self.terrain.geometry().and_then(|g| g.surface.as_ref());
        let lines = match surface {
            Some(_) => crate::gpu::surface_mesh::subdivide_lines(&lines),
            None => lines,
        };
        let mut tris = build_ribbon_tris(&lines, SELECTION_WIDTH, self.hf(), SELECTION_LIFT);
        if let Some(surface) = surface {
            crate::gpu::surface_mesh::drape_ribbon(surface, &mut tris, SELECTION_LIFT);
        }
        self.selection_segments = lines.len() / 2;
        self.selection.write(core, &tris)
    }

    /// Builds the pipelines again when the sample count changed.
    fn sync_samples(&mut self, core: &GpuCore) {
        if self.pipeline_samples == self.samples {
            return;
        }
        let layouts = Layouts {
            scene: &self.fog.layout,
            textures: &self.texture_layout,
            icons: &self.icons_layout,
            shadow: &self.shadows.layout,
            road: &self.road_material.layout,
            effects: &self.effects_gpu.layout,
        };
        self.pipelines = create_pipelines(&core.device, self.color_format, self.samples, &layouts);
        self.pipeline_samples = self.samples;
    }

    /// Depth for a scene pass of `size` pixels (the scaled size when the render scale is below 1).
    fn depth_view(&mut self, core: &GpuCore, size: (u32, u32)) -> wgpu::TextureView {
        let scaled = self.render_scale < scene_scale::MAX_SCALE;
        if self.depth_size != size
            || self.depth_view.is_none()
            || self.depth_samples != self.samples
            || self.scaled_color.is_some() != scaled
        {
            self.rebuild_depth(core, size);
        }
        self.depth_view.clone().expect("depth just created")
    }

    /// Recreates depth and MSAA targets (disposable GPU state, §38). Both live only
    /// inside the scene pass (cleared at its start, resolved or dropped at its end), so
    /// they are transient: on Apple GPUs they stay in tile memory and take no RAM,
    /// which at a Retina window size is hundreds of megabytes.
    fn rebuild_depth(&mut self, core: &GpuCore, size: (u32, u32)) {
        let target = |label, format, sample_count| {
            core.texture(
                Category::RenderTargets,
                &wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size.0.max(1),
                        height: size.1.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: core.pass_attachment_usage(),
                    view_formats: &[],
                },
            )
        };
        let samples = self.samples;
        let depth = target("depth", pipelines::DEPTH_FORMAT, samples);
        self.depth_view = Some(depth.create_view(&Default::default()));
        self.targets = vec![depth];
        // One sample draws straight into the target: no multisampled color, no resolve.
        self.msaa_view = (samples > 1).then(|| {
            let msaa = target("MSAA scene color", self.color_format, samples);
            let view = msaa.create_view(&Default::default());
            self.targets.push(msaa);
            view
        });
        // Below full scale the scene resolves into this instead of the target.
        self.scaled_color = (self.render_scale < scene_scale::MAX_SCALE)
            .then(|| self.scale_blit.color(core, self.color_format, size));
        self.depth_samples = samples;
        self.depth_size = size;
    }

    /// Surface height by terrain id (picking; the style is compiled).
    pub fn style_height(&self, terrain: u16) -> f32 {
        self.style.height(terrain).unwrap_or(0.3)
    }

    fn write_camera(&mut self, core: &GpuCore, camera: &CameraController, aspect: f32, sun: [f32; 4]) {
        let vp = camera.view_proj(aspect);
        core.queue
            .write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&vp));
        core.queue
            .write_buffer(&self.camera_buf, 64, bytemuck::bytes_of(&sun));
        let t = self
            .animation_time_override
            .unwrap_or_else(|| self.started_at.elapsed().as_secs_f32());
        // time_: x = time (water), y = aspect (icon billboards).
        let time = [t, aspect, 0.0, 0.0];
        core.queue
            .write_buffer(&self.camera_buf, 80, bytemuck::bytes_of(&time));
        self.stats.camera_bytes = 96;
    }

    /// Brings every GPU product up to date with the world's layer revisions.
    fn prepare(
        &mut self,
        core: &GpuCore,
        encoder: &mut wgpu::CommandEncoder,
        world: &RenderWorld,
        flags: FrameFlags,
        sun: [f32; 3],
        footprint: crate::gpu::shadow_fit::Area,
        levels: &[(crate::scene::terrain::surface::RegionId, u8)],
    ) {
        let budget = self.budget();
        let stepping = web_time::Instant::now();
        let mut step = self.terrain.step(core, world, levels);
        self.stats.terrain_step_ms = stepping.elapsed().as_secs_f32() * 1000.0;
        for prebuilt in std::mem::take(&mut step.features) {
            self.feature_chunks.adopt(prebuilt);
        }
        self.stats.terrain_bytes += step.bytes;
        self.stats.geometry_rebuilds = step.updates;
        self.stats.regions_rebuilt = step.regions_rebuilt as u32;
        self.stats.terrain_build_ms = step.build_ms;
        self.stats.terrain_pending = self.terrain.pending();
        self.stats.terrain_building = self.terrain.building();
        if self.terrain.geometry().is_none() {
            return;
        }
        if let Some(changed) = &step.changed {
            // Cached placements on ground that changed are placed again.
            self.unit_batches.follow_ground(changed);
            self.world_objects.follow_ground(changed);
        }
        let keys = Keys::new(world.layers, self.terrain.generation);
        // Knowledge-view products and the fog wait for the terrain being built.
        if !self.terrain.pending() {
            self.prepare_from_view(core, world, &keys, flags, step.changed.as_ref(), budget);
            self.prepare_knowledge(core, encoder, world);
            self.prepare_world_objects(core, world, &keys);
        }
        self.prepare_entities(core, world, &keys, step.changed.as_ref(), budget);
        let selection = (flags.selected_city, keys.selection());
        if self.built.selection != Some(selection) {
            self.stats.overlay_bytes += self.refresh_selection(core, world, flags.selected_city);
            self.built.selection = Some(selection);
        }
        let shadowing = web_time::Instant::now();
        self.prepare_shadows(core, encoder, world, sun, footprint);
        self.stats.shadow_ms = shadowing.elapsed().as_secs_f32() * 1000.0;
    }

    /// Products built from the knowledge view (materials, features, icons).
    fn prepare_from_view(
        &mut self,
        core: &GpuCore,
        world: &RenderWorld,
        keys: &Keys,
        flags: FrameFlags,
        changed: Option<&Changed>,
        budget: Budget,
    ) {
        let needs_view = self.built.road != Some(keys.road())
            || self.built.static_features != Some(keys.static_features())
            || self.built.feature_chunks != Some(keys.feature_chunks())
            || self.feature_chunks.has_pending()
            || (flags.yields && self.built.icons != Some(keys.icons()));
        if !needs_view {
            return;
        }
        let mut knowledge = std::mem::take(&mut self.knowledge);
        let view = knowledge.sync(world);
        if self.built.road != Some(keys.road()) {
            let surface = self.terrain.geometry().and_then(|g| g.surface.as_ref());
            self.stats.overlay_bytes += self.road_material.update(core, view, surface);
            self.built.road = Some(keys.road());
        }
        if self.built.static_features != Some(keys.static_features()) {
            self.stats.feature_bytes += self.refresh_static_features(core, view);
            self.built.static_features = Some(keys.static_features());
        }
        if self.built.feature_chunks != Some(keys.feature_chunks()) || self.feature_chunks.has_pending() {
            let geom = self.terrain.geometry().expect("geometry published first");
            let update = self.feature_chunks.update(core, view, geom, changed, budget);
            self.stats.feature_bytes += update.bytes;
            self.stats.feature_chunks_rebuilt = update.chunks_rebuilt as u32;
            self.built.feature_chunks = Some(keys.feature_chunks());
        }
        if flags.yields && self.built.icons != Some(keys.icons()) {
            let icons = build_yield_instances(view, &self.style, self.geom());
            self.stats.overlay_bytes += self.icons.write(core, &icons);
            self.built.icons = Some(keys.icons());
        }
        self.knowledge = knowledge;
    }

    /// City pieces, wonders and tile props: shown for known tiles, so they wait for the
    /// terrain being built like the fog.
    fn prepare_world_objects(&mut self, core: &GpuCore, world: &RenderWorld, keys: &Keys) {
        if self.built.cities != Some(keys.cities()) {
            self.stats.instance_bytes += self.refresh_cities(core, world);
            self.built.cities = Some(keys.cities());
        }
        let props = (keys.props(), self.world_objects.sites_generation);
        if self.built.props != Some(props) {
            self.stats.instance_bytes += self.refresh_props(core, world);
            self.built.props = Some(props);
        }
    }

    /// Products that read the real world: borders, decals and units.
    fn prepare_entities(&mut self, core: &GpuCore, world: &RenderWorld, keys: &Keys, ground: Option<&Changed>, budget: Budget) {
        if self.built.borders != Some(keys.borders()) || self.borders.has_pending() {
            self.stats.overlay_bytes += self.refresh_borders(core, world, ground, budget);
            self.built.borders = Some(keys.borders());
        }
        if self.built.decals != Some(keys.decals()) {
            self.stats.feature_bytes += self.refresh_decals(core, world);
            self.built.decals = Some(keys.decals());
        }
        let posed_ids = self.poses.iter().map(|p| p.id).collect();
        if self.unit_batches.set_posed(posed_ids) {
            self.built.units = None;
        }
        if self.built.units != Some(keys.units()) {
            self.stats.instance_bytes += self.refresh_units(core, world);
            self.units_generation += 1;
            self.built.units = Some(keys.units());
        }
        self.stats.instance_bytes += self.refresh_posed(core, world);
    }

    /// Figures of the posed units, placed on the surface every frame they exist.
    fn refresh_posed(&mut self, core: &GpuCore, world: &RenderWorld) -> u64 {
        if self.poses.is_empty() && self.posed.is_empty() {
            return 0;
        }
        let geom = self.terrain.geometry().expect("geometry published first");
        let batches = &self.unit_batches;
        self.posed.build(&self.poses, batches.rests(), batches.book(), world, &self.style, geom);
        self.pose_generation += 1;
        self.stats.posed_figures = self.posed.instances().len();
        self.posed_instances.write(core, self.posed.instances())
    }

    /// Tile mask upload and fog field bake; rebinds group 0 when the field is recreated.
    fn prepare_knowledge(
        &mut self,
        core: &GpuCore,
        encoder: &mut wgpu::CommandEncoder,
        world: &RenderWorld,
    ) {
        if self.built.knowledge == Some(world.layers.knowledge) {
            return;
        }
        let update = self.fog.update(core, encoder, world);
        self.stats.knowledge_bytes += update.bytes;
        self.stats.fog_rows_baked = update.baked_rows;
        if self.fog.generation != self.scene_bind_generation {
            self.scene_bind = self.fog.scene_bind_group(&core.device, &self.camera_buf);
            self.scene_bind_generation = self.fog.generation;
        }
        self.built.knowledge = Some(world.layers.knowledge);
    }

    /// Re-renders the shadow map from resident buffers when casters, fit or sun change.
    fn prepare_shadows(
        &mut self,
        core: &GpuCore,
        encoder: &mut wgpu::CommandEncoder,
        world: &RenderWorld,
        sun: [f32; 3],
        footprint: crate::gpu::shadow_fit::Area,
    ) {
        let l = Keys::new(world.layers, self.terrain.generation).0;
        let area = self.fit_shadow_area(world, footprint);
        let area_bits = area.map(|a| a.map(|p| p.map(f32::to_bits)));
        let key = ShadowKey {
            layers: [l.terrain, l.units, l.cities, l.knowledge],
            poses: self.pose_generation,
            units_built: self.units_generation,
            features: self.feature_chunks.generation + self.static_generation + self.world_generation,
            sun: sun.map(f32::to_bits),
            area: area_bits,
        };
        if self.built.shadows == Some(key) {
            return;
        }
        let rest_changed = self.built.shadows.is_none_or(|old| ShadowKey { poses: key.poses, ..old } != key);
        self.built.shadows = Some(key);
        // The static map depends on everything but units; a move or a pan inside the
        // shadow area keeps it, so only the units are redrawn over a copy of it.
        let static_key = ([l.terrain, l.cities], key.features, key.sun, area_bits);
        let picked = |grid: &crate::gpu::cells::CellGrid| match area {
            Some([low, high]) => grid.in_box(low, high),
            None => vec![false; grid.len()],
        };
        if self.built.static_shadows != Some(static_key) {
            self.built.static_shadows = Some(static_key);
            self.render_static_shadows(core, encoder, area, sun, &picked);
        }
        if rest_changed {
            let units: Vec<_> = self
                .unit_batches
                .caster_draws(&picked(self.unit_batches.grid()))
                .into_iter()
                .map(|d| (&*self.unit_mesh, d.vertices, self.unit_instances.buffer(), d.instances))
                .collect();
            self.stats.shadow_tris += self.shadows.compose_rest(encoder, &units);
        }
        // The posed figures: only the patch of the map they move in is restored and drawn.
        let rect = if self.posed.is_empty() { None } else { self.shadows.texel_rect(self.posed.instances().iter().map(|i| i.offset), POSED_SHADOW_REACH) };
        let posed: Vec<_> = if rect.is_some() {
            self.posed.caster_draws().into_iter().map(|d| (&*self.unit_mesh, d.vertices, self.posed_instances.buffer(), d.instances)).collect()
        } else {
            Vec::new()
        };
        self.stats.shadow_tris += self.shadows.compose_posed(encoder, &posed, rect);
    }

    /// The area the shadow map covers: the camera's ground footprint within the known
    /// tiles (none on the flat legacy ground).
    fn fit_shadow_area(
        &mut self,
        world: &RenderWorld,
        footprint: crate::gpu::shadow_fit::Area,
    ) -> Option<crate::gpu::shadow_fit::Area> {
        let surface = self.terrain.geometry().is_some_and(|g| g.surface.is_some());
        if !(surface && world.visual_surface) || !crate::debug_view::shadows_enabled() {
            return None;
        }
        let known = self.known_area(world);
        self.shadow_area.update(footprint, known)
    }

    /// Fits the light to `area` and renders the static casters in it.
    fn render_static_shadows(
        &mut self,
        core: &GpuCore,
        encoder: &mut wgpu::CommandEncoder,
        area: Option<crate::gpu::shadow_fit::Area>,
        sun: [f32; 3],
        picked: &impl Fn(&crate::gpu::cells::CellGrid) -> Vec<bool>,
    ) {
        let bounds = area.map(|[low, high]| {
            let ground = self.terrain.ground_y;
            ([low[0], ground.0 - 0.1, low[1]], [high[0], ground.1 + 1.5, high[1]])
        });
        self.stats.overlay_bytes += self.shadows.set_fit(&core.queue, bounds, sun);
        let casters = match area {
            Some(area) => self.static_casters(area, picked),
            None => Casters::default(),
        };
        self.stats.shadow_tris += self.shadows.render_static(encoder, &casters);
    }

    /// This frame's allowance for chunk rebuilds that may wait (none with immediate
    /// terrain: such frames show everything they ask for).
    fn budget(&self) -> Budget {
        match self.terrain.sync {
            TerrainSync::Immediate => Budget::unlimited(self.focus),
            TerrainSync::Background => Budget::from_now(CHUNK_ALLOWANCE, self.focus),
        }
    }

    /// Box of the known tiles, measured once per knowledge revision.
    fn known_area(&mut self, world: &RenderWorld) -> Option<crate::gpu::shadow_fit::Area> {
        let revision = world.layers.knowledge;
        match self.known_area {
            Some((at, area)) if at == revision => area,
            _ => {
                let area = crate::gpu::shadow_fit::known_area(world);
                self.known_area = Some((revision, area));
                area
            }
        }
    }

    /// Resident buffers except units that may cast into the shadow area.
    fn static_casters(
        &self,
        [low, high]: crate::gpu::shadow_fit::Area,
        picked: &impl Fn(&crate::gpu::cells::CellGrid) -> Vec<bool>,
    ) -> Casters<'_> {
        let overlaps = |center: [f32; 3], radius: f32| {
            center[0] + radius >= low[0]
                && center[0] - radius <= high[0]
                && center[2] + radius >= low[1]
                && center[2] - radius <= high[1]
        };
        let mut casters = Casters::default();
        for land in self.terrain.lands().filter(|land| overlaps(land.center, land.radius)) {
            casters
                .terrain
                .push((&land.vertex_buf, &land.index_buf, land.index_count));
        }
        casters.features.extend(self.feature_chunks.within([low[0], 0.0, low[1]], [high[0], 0.0, high[1]]));
        if !self.static_features.is_empty() {
            casters
                .features
                .push((self.static_features.buffer(), self.static_features.len));
        }
        let world = &self.world_objects;
        let batches = [(&world.cities, &self.city_instances), (&world.props, &self.prop_instances)];
        for (batch, instances) in batches {
            for draw in batch.caster_draws(&picked(batch.grid())) {
                casters
                    .models
                    .push((&*self.world_mesh, draw.vertices, instances.buffer(), draw.instances));
            }
        }
        casters
    }

    /// Draws the world into target. Returns frame metrics.
    pub fn draw(
        &mut self,
        core: &GpuCore,
        world: &RenderWorld,
        camera: &CameraController,
        target: &wgpu::TextureView,
        size: (u32, u32),
        flags: FrameFlags,
        effects: &mut crate::gpu::effects::Effects,
    ) -> RenderStats {
        let started = web_time::Instant::now();
        // The camera sees the full-size aspect; depth, MSAA and scene color are `scene_size`.
        let aspect = size.0 as f32 / size.1.max(1) as f32;
        let scene_size = scene_scale::scaled_size(size, self.render_scale);
        self.sync_samples(core);
        let (sun_dir, ambient) = current_sun();
        self.stats = RenderStats::default();
        self.write_camera(core, camera, aspect, [sun_dir[0], sun_dir[1], sun_dir[2], ambient]);
        {
            let planes = camera.frustum_planes(aspect);
            let visible = |p: [f32; 3]| CameraController::sphere_visible(&planes, p, 1.2);
            self.ambient.update(effects, &visible, [camera.x, camera.y], camera.pixels_per_unit(scene_size.1 as f32));
        }
        self.stats.overlay_bytes += self.effects_gpu.prepare(core, effects, &glam::Mat4::from_cols_array_2d(&camera.view_proj(aspect)));
        self.stats.effect_particles = self.effects_gpu.drawn();
        let mut encoder = core
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        let footprint = camera.ground_footprint(aspect, self.terrain.ground_y);
        self.focus = [camera.x, camera.y];
        let distance = camera.distance();
        let planes = camera.frustum_planes(aspect);
        // The eye of CameraController::view_proj: 45 degrees above the view centre.
        let eye = [camera.x, distance, camera.y + distance];
        let focus = distance * std::f32::consts::SQRT_2;
        let levels = if self.terrain_lod && world.visual_surface {
            let detail = crate::gpu::terrain_gpu::DetailView { planes, eye, focus, px_per_unit: camera.pixels_per_unit(scene_size.1 as f32) };
            crate::gpu::terrain_gpu::detail_levels(&detail, world.width, world.height, &self.terrain.levels())
        } else {
            Vec::new()
        };
        self.prepare(core, &mut encoder, world, flags, sun_dir, footprint, &levels);
        let depth = self.depth_view(core, scene_size);
        let msaa = self.msaa_view.clone();
        let view = SceneView {
            planes,
            px_per_unit: camera.pixels_per_unit(scene_size.1 as f32),
            eye,
            focus,
        };
        let encoding = web_time::Instant::now();
        match &self.scaled_color {
            Some(color) => {
                let scene_target = color.view.clone();
                self.encode_scene(&mut encoder, &scene_target, msaa.as_ref(), &depth, &view, flags);
                if let Some(color) = &self.scaled_color {
                    self.scale_blit.blit(&mut encoder, color, target);
                }
            }
            None => self.encode_scene(&mut encoder, target, msaa.as_ref(), &depth, &view, flags),
        }
        self.stats.encode_ms = encoding.elapsed().as_secs_f32() * 1000.0;
        core.queue.submit(std::iter::once(encoder.finish()));
        self.finish_stats(started);
        self.stats
    }

    fn finish_stats(&mut self, started: web_time::Instant) {
        let s = &mut self.stats;
        s.bytes_uploaded =
            s.terrain_bytes + s.feature_bytes + s.instance_bytes + s.knowledge_bytes + s.overlay_bytes;
        s.chunks_total = self.terrain.land_count();
        s.water_tris = self.terrain.water_vertices() as usize / 3;
        s.river_segments = s.water_tris;
        s.border_segments = self.borders.edges();
        s.selected_segments = self.selection_segments;
        s.feature_tris = (self.static_features.len + self.decals.len) as usize / 3
            + self.feature_chunks.triangles();
        s.unit_instances = self.unit_instances.len as usize;
        s.city_instances = self.world_objects.cities.instances();
        s.prop_instances = self.world_objects.props.instances();
        s.icon_instances = self.icons.len as usize;
        s.texture_layers = self.textures.layers;
        s.grid_segments = self.terrain.grid_vertices() as usize / 2;
        s.chunks_pending = self.feature_chunks.has_pending() || self.borders.has_pending();
        s.prepare_ms = started.elapsed().as_secs_f32() * 1000.0;
    }

    fn encode_scene(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        msaa: Option<&wgpu::TextureView>,
        depth: &wgpu::TextureView,
        view: &SceneView,
        flags: FrameFlags,
    ) {
        let planes = &view.planes;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: msaa.unwrap_or(target),
                depth_slice: None,
                resolve_target: msaa.map(|_| target),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.54,
                        g: 0.38,
                        b: 0.224,
                        a: 1.0,
                    }),
                    // With MSAA only the resolve target outlives the pass; without, the
                    // color attachment is the target itself.
                    store: if msaa.is_some() { wgpu::StoreOp::Discard } else { wgpu::StoreOp::Store },
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: self.timing.as_ref().map(|t| t.writes(crate::gpu::timing::Stage::Scene)),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.scene_bind, &[]);
        pass.set_bind_group(1, &self.texture_bind, &[]);
        pass.set_bind_group(2, &self.shadows.bind_group, &[]);
        self.draw_terrain(&mut pass, planes);
        self.draw_models(&mut pass, view);
        if flags.grid {
            pass.set_pipeline(&self.pipelines.grid);
            for (buffer, vertices) in self.terrain.visible_grid(planes) {
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..vertices, 0..1);
                self.stats.draw_calls += 1;
            }
        }
        pass.set_pipeline(&self.pipelines.water);
        for (buffer, vertices) in self.terrain.visible_water(planes) {
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..vertices, 0..1);
            self.stats.draw_calls += 1;
        }
        self.draw_features(&mut pass, planes);
        self.draw_overlays(&mut pass, planes, flags);
        // Particles last: blended over everything, tested against the scene depth.
        self.effects_gpu.draw(&mut pass, &self.pipelines.effects);
    }

    fn draw_terrain(&mut self, pass: &mut wgpu::RenderPass<'_>, planes: &[glam::Vec4; 6]) {
        pass.set_pipeline(&self.pipelines.terrain);
        pass.set_bind_group(3, &self.road_material.bind_group, &[]);
        for land in self.terrain.visible_lands(planes) {
            pass.set_vertex_buffer(0, land.vertex_buf.slice(..));
            pass.set_index_buffer(land.index_buf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..land.index_count, 0, 0..1);
            self.stats.draw_calls += 1;
            self.stats.terrain_tris += land.index_count as usize / 3;
            self.stats.chunks_drawn += 1;
        }
    }

    /// Units, then city tiles and world props, through the shared model pipeline:
    /// one draw per model group at its LOD.
    fn draw_models(&mut self, pass: &mut wgpu::RenderPass<'_>, view: &SceneView) {
        let px_per_unit = view.px_per_unit;
        let model_view = crate::gpu::world_objects::ModelView {
            planes: view.planes,
            px_per_unit,
            eye: view.eye,
            focus: view.focus,
        };
        pass.set_pipeline(&self.pipelines.unit);
        pass.set_vertex_buffer(0, self.unit_mesh.slice(..));
        pass.set_vertex_buffer(1, self.unit_instances.slice());
        let visible = self.unit_batches.grid().in_frustum(&view.planes);
        for draw in self.unit_batches.draws(px_per_unit, &visible) {
            self.stats.unit_tris += draw.triangles();
            pass.draw(draw.vertices, draw.instances);
            self.stats.draw_calls += 1;
            self.stats.unit_draws += 1;
        }
        if !self.posed.is_empty() {
            pass.set_vertex_buffer(1, self.posed_instances.slice());
            for draw in self.posed.draws(px_per_unit) {
                self.stats.unit_tris += draw.triangles();
                pass.draw(draw.vertices, draw.instances);
                self.stats.draw_calls += 1;
                self.stats.unit_draws += 1;
            }
        }
        pass.set_vertex_buffer(0, self.world_mesh.slice(..));
        let world = &mut self.world_objects;
        for (batch, instances, is_city) in [
            (&mut world.cities, &self.city_instances, true),
            (&mut world.props, &self.prop_instances, false),
        ] {
            if instances.is_empty() {
                continue;
            }
            pass.set_vertex_buffer(1, instances.slice());
            for draw in batch.draws(&model_view) {
                self.stats.world_tris += draw.triangles();
                pass.draw(draw.vertices, draw.instances);
                self.stats.draw_calls += 1;
                self.stats.world_draws += 1;
                if is_city {
                    self.stats.city_draws += 1;
                }
            }
        }
    }

    /// Static features, culled feature chunks, then legacy alpha decals.
    fn draw_features(&mut self, pass: &mut wgpu::RenderPass<'_>, planes: &[glam::Vec4; 6]) {
        pass.set_pipeline(&self.pipelines.feature);
        let before = self.stats.draw_calls;
        if !self.static_features.is_empty() {
            pass.set_vertex_buffer(0, self.static_features.slice());
            pass.draw(0..self.static_features.len, 0..1);
            self.stats.draw_calls += 1;
        }
        for (buffer, vertices) in self.feature_chunks.visible(planes) {
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..vertices, 0..1);
            self.stats.draw_calls += 1;
        }
        if !self.decals.is_empty() {
            pass.set_vertex_buffer(0, self.decals.slice());
            pass.draw(0..self.decals.len, 0..1);
            self.stats.draw_calls += 1;
        }
        self.stats.feature_draws = self.stats.draw_calls - before;
    }

    /// Borders, selection and yield icons over the scene.
    fn draw_overlays(&mut self, pass: &mut wgpu::RenderPass<'_>, planes: &[glam::Vec4; 6], flags: FrameFlags) {
        pass.set_pipeline(&self.pipelines.soft_border);
        for (buffer, vertices) in self.borders.visible(planes) {
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..vertices, 0..1);
            self.stats.draw_calls += 1;
        }
        if !self.selection.is_empty() {
            pass.set_pipeline(&self.pipelines.ribbon);
            pass.set_vertex_buffer(0, self.selection.slice());
            pass.draw(0..self.selection.len, 0..1);
            self.stats.draw_calls += 1;
        }
        if flags.yields && !self.icons.is_empty() {
            pass.set_pipeline(&self.pipelines.icon);
            pass.set_bind_group(1, &self.icons_bind, &[]);
            pass.set_vertex_buffer(0, self.icon_mesh_buf.slice(..));
            pass.set_vertex_buffer(1, self.icons.slice());
            pass.draw(0..self.icon_mesh_count, 0..self.icons.len);
            self.stats.draw_calls += 1;
        }
    }

    /// Offscreen render for headless tests: texture -> RGBA8 bytes.
    pub fn render_offscreen(
        &mut self,
        core: &GpuCore,
        world: &RenderWorld,
        camera: &CameraController,
        width: u32,
        height: u32,
        flags: FrameFlags,
        effects: &mut crate::gpu::effects::Effects,
    ) -> Vec<u8> {
        let tex = core.texture(Category::RenderTargets, &wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.draw(core, world, camera, &view, (width, height), flags, effects);
        read_texture(core, &tex, width, height)
    }
}

/// A work that became pillaged, or was repaired, on a tile that was in sight before and now:
/// the cue is played where it stands. The first build, a changed map and tiles that only
/// came into sight raise none.
fn pillage_cues(seen: &mut Vec<(u8, bool)>, world: &RenderWorld, geom: &crate::scene::terrain::fields::TerrainGeometry) -> Vec<crate::gpu::ambient_fx::Cue> {
    use crate::gpu::ambient_fx::Cue;
    let now: Vec<(u8, bool)> = (0..world.terrain.len())
        .map(|i| (world.visible.get(i).copied().unwrap_or(0), world.pillaged.get(i).copied().unwrap_or(false)))
        .collect();
    let mut cues = Vec::new();
    if seen.len() == now.len() {
        for (i, (before, after)) in seen.iter().zip(&now).enumerate() {
            if before.0 != 1 || after.0 != 1 || before.1 == after.1 {
                continue;
            }
            let (x, z) = crate::gpu::world_objects::WorldObjects::tile_centre(world, i as u32);
            let y = crate::scene::terrain::evaluator::sample_surface(geom, x, z).map_or(0.4, |s| s.y);
            if after.1 {
                cues.push(Cue::Pillaged { pos: [x, y, z] });
            } else if world.improvements.get(i).copied().flatten().is_some() {
                cues.push(Cue::Repaired { pos: [x, y, z] });
            }
        }
    }
    *seen = now;
    cues
}


fn buffer_with<T: bytemuck::Pod>(
    core: &GpuCore,
    category: Category,
    label: &str,
    usage: wgpu::BufferUsages,
    data: &[T],
) -> GpuBuffer {
    let buffer = core.buffer(category, &wgpu::BufferDescriptor {
        label: Some(label),
        size: std::mem::size_of_val(data).max(4) as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    core.queue
        .write_buffer(&buffer, 0, bytemuck::cast_slice(data));
    buffer
}

fn read_texture(core: &GpuCore, tex: &wgpu::Texture, width: u32, height: u32) -> Vec<u8> {
    let pitch = width * 4;
    let buf = core.buffer(Category::Readback, &wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (pitch * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = core
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    core.queue.submit(std::iter::once(encoder.finish()));
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    let _ = core.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv().unwrap().unwrap();
    let out = slice.get_mapped_range().unwrap().to_vec();
    buf.unmap();
    out
}

/// Public facade: GPU or a stub (web before async init, tests).
pub struct Renderer {
    inner: Option<GpuRenderer>,
    stats: RenderStats,
    effects: crate::gpu::effects::Effects,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::headless()
    }
}

impl Renderer {
    pub fn headless() -> Self {
        Self {
            inner: None,
            stats: RenderStats::default(),
            effects: Default::default(),
        }
    }

    pub fn with_gpu(
        core: &GpuCore,
        format: wgpu::TextureFormat,
        layers: &[(u32, u32, Vec<u8>)],
    ) -> Self {
        Self {
            inner: Some(GpuRenderer::new(core, format, layers)),
            stats: RenderStats::default(),
            effects: Default::default(),
        }
    }

    pub fn is_gpu(&self) -> bool {
        self.inner.is_some()
    }

    /// The effects pool: explosions, projectiles, smoke (platform/docs/animation.md). Works without
    /// a GPU too (nothing is drawn); spawn with world coordinates or `play_fx`.
    pub fn effects(&mut self) -> &mut crate::gpu::effects::Effects {
        &mut self.effects
    }

    /// Plays a presentation event on the displayed ground.
    pub fn play_fx(&mut self, event: &crate::gpu::effects::FxEvent) {
        let ground = |x: f32, z: f32| self.inner.as_ref().and_then(|g| {
            let geom = g.terrain.geometry()?;
            crate::scene::terrain::evaluator::sample_surface(geom, x, z).map(|s| s.y)
        }).unwrap_or(0.4);
        // The closure borrows `self.inner`; the pool is a separate field.
        let effects = &mut self.effects;
        effects.play(event, &ground);
    }

    /// Moves the effects clock (call once per frame with the frame's seconds).
    pub fn advance_effects(&mut self, seconds: f32) {
        self.effects.advance(seconds);
    }

    /// Unit kinds of the running ruleset -> models (by content key). Until it is set,
    /// every unit draws the explicit fallback banner. Rebuilds unit instances and shadows.
    pub fn set_unit_models(&mut self, book: crate::unit_models::UnitModelBook) {
        if let Some(gpu) = &mut self.inner {
            gpu.unit_batches.set_book(book);
            gpu.built.units = None;
            gpu.built.shadows = None;
        }
    }

    /// Content of the running ruleset -> world models (improvements, resources, natural
    /// wonders, wonders, city kit landmarks). Rebuilds city and prop instances.
    pub fn set_world_models(&mut self, book: crate::world_models::WorldBook) {
        if let Some(gpu) = &mut self.inner {
            gpu.world_objects.set_book(book);
            gpu.built.cities = None;
            gpu.built.props = None;
            gpu.built.shadows = None;
        }
    }

    /// When terrain updates reach the frame: in the background (the game) or before
    /// every frame that needs them (screenshots, tests, benches of whole frames).
    pub fn set_terrain_sync(&mut self, sync: TerrainSync) {
        if let Some(gpu) = &mut self.inner {
            gpu.terrain.sync = sync;
        }
    }

    /// Fraction (0.5..=1.0) of the target size the 3D scene renders at before it is
    /// upscaled into the target; the UI stays at full size. Default 1.0 (or `AURELIA_RENDER_SCALE`).
    /// Camera, projection and picking are unaffected. Takes effect on the next frame.
    pub fn set_render_scale(&mut self, scale: f32) {
        if let Some(gpu) = &mut self.inner {
            gpu.render_scale = scene_scale::clamp_scale(scale);
        }
    }

    /// Current render scale (1.0 without a GPU).
    pub fn render_scale(&self) -> f32 {
        self.inner.as_ref().map_or(1.0, |gpu| gpu.render_scale)
    }

    /// Samples per pixel of the scene pass: 1 (no anti-aliasing) or 4 (anything else
    /// means 4). The pipelines are built again at the next frame when it changes.
    /// Default 4, or 1 with `AURELIA_MSAA=1`.
    pub fn set_msaa_samples(&mut self, samples: u32) {
        if let Some(gpu) = &mut self.inner {
            gpu.samples = pipelines::clamp_samples(samples);
        }
    }

    /// Current samples per pixel (4 without a GPU).
    pub fn msaa_samples(&self) -> u32 {
        self.inner.as_ref().map_or(4, |gpu| gpu.samples)
    }

    /// Mesh far regions coarser (level of detail by projected hex size). Off by default.
    pub fn set_terrain_lod(&mut self, on: bool) {
        if let Some(gpu) = &mut self.inner {
            gpu.terrain_lod = on;
        }
    }

    /// Freeze shader animation at a deterministic time for visual comparisons.
    /// None resumes the renderer clock; non-finite values also resume it.
    pub fn set_animation_time_override(&mut self, seconds: Option<f32>) {
        if let Some(gpu) = &mut self.inner {
            gpu.animation_time_override = seconds.filter(|value| value.is_finite());
        }
    }

    /// Units the presentation draws in the next frames (empty: every unit rests). Their
    /// resting figures collapse while posed; nothing else is rebuilt.
    pub fn set_unit_poses(&mut self, poses: &[crate::gpu::unit_poses::UnitPose]) {
        if let Some(gpu) = &mut self.inner {
            gpu.poses.clear();
            gpu.poses.extend_from_slice(poses);
        }
    }

    /// City tiles and world-wonder sites of the last frame (the gallery hangs labels on them).
    pub fn occupied_tiles(&self) -> &[u32] {
        self.inner.as_ref().map_or(&[], |i| i.world_objects.occupied_tiles())
    }

    /// Height of the displayed ground (or water) at world XZ: overlays stand on it.
    pub fn ground_height(&self, x: f32, z: f32) -> Option<f32> {
        let geom = self.inner.as_ref()?.terrain.geometry()?;
        crate::scene::terrain::evaluator::sample_surface(geom, x, z).map(|s| s.y)
    }

    /// Borrow the rendered surface for overlays; never rebuild geometry in UI.
    pub fn visual_surface(&self) -> Option<&crate::scene::terrain::surface::SurfaceMesh> {
        self.inner.as_ref()?.terrain.geometry()?.surface.as_ref()
    }

    /// Terrain surface height (picking). Headless: a constant.
    pub fn terrain_height(&self, terrain: u16) -> f32 {
        match &self.inner {
            Some(g) => g.style_height(terrain),
            None => 0.35,
        }
    }

    /// Picks the actual continuous surface; logical hex IDs remain unchanged.
    pub fn pick_tile(
        &self,
        world: &RenderWorld,
        camera: &CameraController,
        screen: (f32, f32),
        size: (f32, f32),
    ) -> Option<kernel::ids::TileId> {
        let map = (world.width, world.height);
        let aspect = size.0 / size.1.max(1.0);
        let surface = self.inner.as_ref().and_then(|gpu| {
            gpu.terrain
                .geometry()?
                .surface
                .as_ref()
                .map(|surface| (surface, gpu.rock_pick_positions.as_slice()))
        });
        let tile = if let Some((surface, rocks)) = surface {
            crate::scene::picking::pick_surface_with_features(
                camera, aspect, screen, size, map, surface, rocks,
            )
        } else {
            crate::scene::picking::pick_tile(camera, aspect, screen, size, map, |id| {
                self.terrain_height(world.terrain[id.0 as usize].terrain)
            })
        };
        tile.filter(|id| world.is_known(id.0 as usize))
    }

    pub fn draw(
        &mut self,
        core: &GpuCore,
        world: &RenderWorld,
        camera: &CameraController,
        target: &wgpu::TextureView,
        size: (u32, u32),
        flags: FrameFlags,
    ) -> RenderStats {
        match &mut self.inner {
            Some(g) => {
                self.stats = g.draw(core, world, camera, target, size, flags, &mut self.effects);
                self.stats
            }
            None => RenderStats::default(),
        }
    }

    pub fn stats(&self) -> RenderStats {
        self.stats
    }

    /// Ambient particles (geysers, craters, fires, ruins) on or off: the shell turns them off on
    /// Quality::Low and with reduced motion (nothing then moves by itself).
    pub fn set_ambient_effects(&mut self, on: bool) {
        if let Some(gpu) = &mut self.inner {
            gpu.ambient.set_enabled(on);
        }
    }

    /// The effects pool was emptied or the scene replaced: the emitters write their cycles again.
    pub fn restart_ambient_effects(&mut self) {
        if let Some(gpu) = &mut self.inner {
            gpu.ambient.restart();
        }
    }

    /// Particles the last `update` of the ambient emitters wrote.
    pub fn ambient_sites(&self) -> usize {
        self.inner.as_ref().map_or(0, |gpu| gpu.ambient.sites().len())
    }

    /// The last frame left work for the next ones: terrain being built or uploaded in
    /// the background, chunk rebuilds over the frame budget, or a terrain publish after
    /// which the detail levels may move on. The window draws until it is done.
    pub fn has_pending_work(&self) -> bool {
        let s = &self.stats;
        s.terrain_building || s.chunks_pending || s.geometry_rebuilds > 0
    }

    /// Live GPU memory of the device by category (everything made through
    /// `GpuCore::buffer` / `GpuCore::texture`, the UI and capture targets included);
    /// empty without a GPU.
    pub fn gpu_memory(&self) -> crate::gpu::memory::GpuMemoryReport {
        self.inner.as_ref().map(|gpu| gpu.memory.report()).unwrap_or_default()
    }

    /// One frame into an RGBA8 (unorm) image; empty without a GPU. The renderer must
    /// have been made for `Rgba8Unorm`.
    pub fn render_offscreen(
        &mut self,
        core: &GpuCore,
        world: &RenderWorld,
        camera: &CameraController,
        width: u32,
        height: u32,
        flags: FrameFlags,
    ) -> Vec<u8> {
        match &mut self.inner {
            Some(g) => {
                let pixels = g.render_offscreen(core, world, camera, width, height, flags, &mut self.effects);
                self.stats = g.stats;
                pixels
            }
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{sun_for_time_of_day, AMBIENT, SUN_DIR};

    #[test]
    fn noon_is_legacy_sun_and_edges_dim() {
        // Noon is bit-for-bit the old constants.
        let (dir, amb) = sun_for_time_of_day(0.5);
        assert_eq!((dir, amb), (SUN_DIR, AMBIENT));
        // Morning/evening: lower sun to the side, darker ambient.
        let (m_dir, m_amb) = sun_for_time_of_day(0.15);
        assert!(m_dir[1] < dir[1] && m_dir[1] > 0.0);
        assert!(m_amb < amb && m_amb > 0.0);
        assert_eq!(
            sun_for_time_of_day(-2.0),
            sun_for_time_of_day(0.0),
            "clamped below"
        );
        assert_eq!(
            sun_for_time_of_day(7.0),
            sun_for_time_of_day(1.0),
            "clamped above"
        );
    }
}
