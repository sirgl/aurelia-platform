//! The parchment the knowledge fog reveals the map from, baked into textures.
//!
//! `cartography.wgsl` defines the paper from world XZ alone. Computing it per pixel costs about
//! 16 value noises on the start view (8.6 ms of the terrain pass on a UHD 630 at 2880x1800), so the
//! host bakes it once and the receivers sample it:
//!
//! * two seamless world-space **tiles** of periodic value noise (repeat sampler, mips), built once
//!   when the renderer starts: `fine` (R8, 512^2, period 8 world units) holds the grain and fibre
//!   residual of the paper colour, `mid` (RG8, 512^2, period 10 units) the pigment flecks and the
//!   feather of the fog edge. The noise lattice wraps at the period (grain 190 cells, fibres 392 x
//!   66, flecks 170, feather 73), so the tile is seamless by construction. The fine pattern adds at
//!   most +-0.0135 to a colour (3 of 255 levels, mostly 1) and has no structure, the mid pattern
//!   shows only inside the soft fog edge band. A repetition after 8 or 10 units is measurable
//!   (the difference of the high-pass picture at that lag is 20 % below its average) but not
//!   visible (ru/notes/devlog/ideas.md). The tile is 512^2, not 1024^2: the same screenshot to 0.13
//!   of 255 levels and 0.7 ms less on the UHD 630, since the cost of a tap is the texture
//!   cache, not the arithmetic (256^2 saves another 0.4 ms but loses fibres).
//! * a **low-frequency field** over the map (RGBA16F, `TEXELS_PER_UNIT` texels per world unit,
//!   at most `MAX_FIELD` per side): the wash, the wash of the fog edge (scale 1.17), the broad
//!   tint and the meadow tint. Baked like the fog field: when the map size changes.
//!
//! The bake pass runs the functions of `cartography.wgsl` (the shader source is appended to it),
//! so the baked result is the per-pixel result up to texture filtering. `AURELIA_PARCHMENT=procedural`
//! keeps the per-pixel path (A/B comparison); it is also the fallback where the device cannot hold
//! the tile. Both paths bind placeholder textures, so the bind group layout never changes.
use crate::gpu::context::GpuCore;
use crate::gpu::memory::{Category, GpuBuffer, GpuTexture};

const BAKE_WGSL: &str = include_str!("../../../../assets/shaders/parchment_bake.wgsl");
const CARTOGRAPHY_WGSL: &str = include_str!("../../../../assets/shaders/cartography.wgsl");
/// Edge of the fine tile in texels (`FINE_TEXELS` in cartography.wgsl) and of the mid tile.
pub const FINE_TEXELS: u32 = 512;
pub const MID_TEXELS: u32 = 512;
const FINE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const MID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg8Unorm;
const LOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The setting `AURELIA_<name>`.
fn setting(name: &str) -> Option<String> {
    std::env::var(format!("AURELIA_{name}")).ok()
}
/// Dev knob (experiments): the texel count of a tile, a power of two.
fn tile_texels(var: &str, default: u32) -> u32 {
    setting(var)
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| v.is_power_of_two() && *v >= 64)
        .unwrap_or(default)
}
fn fine_texels() -> u32 {
    tile_texels("PARCHMENT_FINE", FINE_TEXELS)
}
fn mid_texels() -> u32 {
    tile_texels("PARCHMENT_MID", MID_TEXELS)
}
/// Low field resolution. The finest wash octave has a 0.27 unit cell (0.23 at the edge's 1.17
/// scale): eight texels per unit give it about two texels per cell.
pub const TEXELS_PER_UNIT: f32 = 8.0;
/// Largest side of the low field: the guaranteed minimum texture size of WebGL2 and mobile.
const MAX_FIELD: u32 = 2048;
/// Field margin around the map, in world units.
const MARGIN: f32 = 8.0;

/// `AURELIA_PARCHMENT_TIMING=1` (native): a bake is submitted alone, waited for and its wall time printed.
#[cfg(not(target_arch = "wasm32"))]
fn timing_start() -> Option<std::time::Instant> {
    setting("PARCHMENT_TIMING").map(|_| std::time::Instant::now())
}
#[cfg(target_arch = "wasm32")]
fn timing_start() -> Option<()> {
    None
}
#[cfg(not(target_arch = "wasm32"))]
fn timing_report(core: &GpuCore, started: Option<std::time::Instant>, what: &str) {
    if let Some(started) = started {
        let _ = core.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        });
        eprintln!(
            "parchment bake: {what} {:.2} ms",
            started.elapsed().as_secs_f64() * 1e3
        );
    }
}
#[cfg(target_arch = "wasm32")]
fn timing_report(_core: &GpuCore, _started: Option<()>, _what: &str) {}

/// Where the parchment comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Baked,
    Procedural,
}

/// `AURELIA_PARCHMENT=procedural|baked` (default baked).
pub fn requested() -> Source {
    match setting("PARCHMENT").as_deref() {
        Some("procedural") => Source::Procedural,
        _ => Source::Baked,
    }
}

/// The source this device really uses: baked unless asked otherwise or the tile does not fit.
pub fn source(device: &wgpu::Device) -> Source {
    if device.limits().max_texture_dimension_2d < FINE_TEXELS {
        return Source::Procedural;
    }
    requested()
}

/// The receiver shaders with their `PARCHMENT_BAKED` constant set for `source`.
pub fn configure_shader(source_wgsl: &str, source: Source) -> String {
    let source_wgsl = source_wgsl
        .replace(
            "const FINE_TEXELS: f32 = 512.0;",
            &format!("const FINE_TEXELS: f32 = {}.0;", fine_texels()),
        )
        .replace(
            "const MID_TEXELS: f32 = 512.0;",
            &format!("const MID_TEXELS: f32 = {}.0;", mid_texels()),
        );
    match source {
        Source::Baked => source_wgsl,
        Source::Procedural => source_wgsl.replace(
            "const PARCHMENT_BAKED: bool = true;",
            "const PARCHMENT_BAKED: bool = false;",
        ),
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BakeUniform {
    origin_texel: [f32; 4],
}

/// World placement of the low field: the same layout the fog field uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub field: (u32, u32),
    pub origin: [f32; 2],
    pub texel: f32,
}

impl Placement {
    pub fn new(map: (u32, u32), per_unit: f32, max_dimension: u32) -> Self {
        let (w, h) = (map.0.max(1) as f32, map.1.max(1) as f32);
        let extent_x = 1.732_050_8 * ((w - 1.0) + (h - 1.0) * 0.5) + 2.0 * MARGIN;
        let extent_z = 1.5 * (h - 1.0) + 2.0 * MARGIN;
        let per_unit = per_unit.min(max_dimension as f32 / extent_x.max(extent_z));
        Self {
            field: (
                ((extent_x * per_unit).ceil() as u32).clamp(1, max_dimension),
                ((extent_z * per_unit).ceil() as u32).clamp(1, max_dimension),
            ),
            origin: [-MARGIN, -MARGIN],
            texel: 1.0 / per_unit,
        }
    }

    /// Origin XZ and inverse extent XZ for the receivers.
    pub fn bounds(&self) -> [f32; 4] {
        [
            self.origin[0],
            self.origin[1],
            1.0 / (self.field.0 as f32 * self.texel),
            1.0 / (self.field.1 as f32 * self.texel),
        ]
    }
}

fn texture(
    core: &GpuCore,
    label: &str,
    size: (u32, u32),
    mips: u32,
    format: wgpu::TextureFormat,
) -> GpuTexture {
    core.texture(
        Category::Parchment,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        },
    )
}

fn shader(device: &wgpu::Device) -> wgpu::ShaderModule {
    let source = configure_shader(&format!("{CARTOGRAPHY_WGSL}\n{BAKE_WGSL}"), Source::Baked);
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("parchment bake"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    label: &str,
    entry: &str,
    layout: Option<&wgpu::BindGroupLayout>,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let one = [layout];
    let groups: &[Option<&wgpu::BindGroupLayout>] = if layout.is_some() { &one } else { &[] };
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: groups,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn uniform_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("parchment bake"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 12,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

fn mip_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("parchment mip"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 13,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 14,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

fn begin<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// Entries 4..=8 of the scene bind group layout (cartography.wgsl).
pub fn layout_entries() -> [wgpu::BindGroupLayoutEntry; 5] {
    let texture = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    [
        texture(4),
        texture(5),
        texture(6),
        wgpu::BindGroupLayoutEntry {
            binding: 7,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 8,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
    ]
}

/// A seamless tile: its texture, its view and how it is baked.
struct Tile {
    texture: GpuTexture,
    view: wgpu::TextureView,
    format: wgpu::TextureFormat,
    entry: &'static str,
}

impl Tile {
    fn new(
        core: &GpuCore,
        label: &str,
        texels: u32,
        format: wgpu::TextureFormat,
        entry: &'static str,
    ) -> Self {
        let texture = texture(core, label, (texels, texels), texels.ilog2() + 1, format);
        let view = texture.create_view(&Default::default());
        Self {
            texture,
            view,
            format,
            entry,
        }
    }

    /// Noise into mip 0, then each smaller level as the box average of the one above.
    fn bake(
        &self,
        core: &GpuCore,
        module: &wgpu::ShaderModule,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let device = &core.device;
        let noise = pipeline(
            device,
            module,
            "parchment tile",
            self.entry,
            None,
            self.format,
        );
        let mip_layout = mip_layout(device);
        let mip = pipeline(
            device,
            module,
            "parchment mip",
            "fs_mip",
            Some(&mip_layout),
            self.format,
        );
        let level_view = |level: u32| {
            self.texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        {
            let view = level_view(0);
            let mut pass = begin(encoder, "parchment tile", &view);
            pass.set_pipeline(&noise);
            pass.draw(0..3, 0..1);
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("parchment mip"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        for level in 1..self.texture.mip_level_count() {
            let source = level_view(level - 1);
            let target = level_view(level);
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("parchment mip"),
                layout: &mip_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 13,
                        resource: wgpu::BindingResource::TextureView(&source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 14,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            let mut pass = begin(encoder, "parchment mip", &target);
            pass.set_pipeline(&mip);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

pub struct Parchment {
    source: Source,
    per_unit: f32,
    fine: Tile,
    mid: Tile,
    sampler: wgpu::Sampler,
    low: GpuTexture,
    low_view: wgpu::TextureView,
    bounds: GpuBuffer,
    bake_uniform: GpuBuffer,
    low_bind: wgpu::BindGroup,
    low_pipeline: Option<wgpu::RenderPipeline>,
    pub placement: Placement,
    /// Texels of the last low field bake.
    pub last_bake_texels: u32,
}

impl Parchment {
    /// Placeholders for every texture; with the baked source the tiles are built and submitted now.
    pub fn new(core: &GpuCore) -> Self {
        let device = &core.device;
        let source = source(device);
        let baked = source == Source::Baked;
        let per_unit = setting("PARCHMENT_TEXELS")
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(TEXELS_PER_UNIT);
        // Placeholders (1x1) when the per-pixel path is used.
        let (fine_size, mid_size) = if baked {
            (fine_texels(), mid_texels())
        } else {
            (1, 1)
        };
        let fine = Tile::new(
            core,
            "parchment fine tile",
            fine_size,
            FINE_FORMAT,
            "fs_fine",
        );
        let mid = Tile::new(core, "parchment mid tile", mid_size, MID_FORMAT, "fs_mid");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("parchment tiles"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: setting("PARCHMENT_ANISO")
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(1),
            ..Default::default()
        });
        let placement = Placement::new((0, 0), per_unit, MAX_FIELD);
        let low = texture(core, "parchment low field", (1, 1), 1, LOW_FORMAT);
        let low_view = low.create_view(&Default::default());
        let bounds = core.buffer(
            Category::Uniforms,
            &wgpu::BufferDescriptor {
                label: Some("parchment bounds"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        );
        core.queue
            .write_buffer(&bounds, 0, bytemuck::cast_slice(&[0.0f32, 0.0, 1.0, 1.0]));
        let bake_uniform = core.buffer(
            Category::Uniforms,
            &wgpu::BufferDescriptor {
                label: Some("parchment bake uniform"),
                size: std::mem::size_of::<BakeUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        );
        let low_layout = uniform_layout(device);
        let low_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("parchment bake"),
            layout: &low_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 12,
                resource: bake_uniform.as_entire_binding(),
            }],
        });
        let mut low_pipeline = None;
        if baked {
            let module = shader(device);
            low_pipeline = Some(pipeline(
                device,
                &module,
                "parchment low field",
                "fs_low",
                Some(&low_layout),
                LOW_FORMAT,
            ));
            let started = timing_start();
            let mut encoder = device.create_command_encoder(&Default::default());
            fine.bake(core, &module, &mut encoder);
            mid.bake(core, &module, &mut encoder);
            core.queue.submit([encoder.finish()]);
            timing_report(core, started, "tiles (pipelines, noise, mips)");
        }
        Self {
            source,
            per_unit,
            fine,
            mid,
            sampler,
            low,
            low_view,
            bounds,
            bake_uniform,
            low_bind,
            low_pipeline,
            placement,
            last_bake_texels: 0,
        }
    }

    pub fn source(&self) -> Source {
        self.source
    }

    /// Bakes the low field for a map of `map` tiles (the map size changed). Returns its texel count.
    pub fn rebuild(
        &mut self,
        core: &GpuCore,
        encoder: &mut wgpu::CommandEncoder,
        map: (u32, u32),
    ) -> u32 {
        let Some(low_pipeline) = &self.low_pipeline else {
            return 0;
        };
        let limit = core.device.limits().max_texture_dimension_2d.min(MAX_FIELD);
        self.placement = Placement::new(map, self.per_unit, limit);
        let p = self.placement;
        self.low = texture(core, "parchment low field", p.field, 1, LOW_FORMAT);
        self.low_view = self.low.create_view(&Default::default());
        let bake = BakeUniform {
            origin_texel: [p.origin[0], p.origin[1], p.texel, 0.0],
        };
        core.queue
            .write_buffer(&self.bake_uniform, 0, bytemuck::bytes_of(&bake));
        core.queue
            .write_buffer(&self.bounds, 0, bytemuck::cast_slice(&p.bounds()));
        let record = |encoder: &mut wgpu::CommandEncoder| {
            let mut pass = begin(encoder, "parchment low field", &self.low_view);
            pass.set_pipeline(low_pipeline);
            pass.set_bind_group(0, &self.low_bind, &[]);
            pass.draw(0..3, 0..1);
        };
        if let Some(started) = timing_start() {
            // Probe: the pass alone in its own submission, waited for.
            let mut own = core.device.create_command_encoder(&Default::default());
            record(&mut own);
            core.queue.submit([own.finish()]);
            timing_report(
                core,
                Some(started),
                &format!("low field {}x{}", p.field.0, p.field.1),
            );
        } else {
            record(encoder);
        }
        self.last_bake_texels = p.field.0 * p.field.1;
        self.last_bake_texels
    }

    /// Entries 4..=8 of the scene bind group.
    pub fn bind_entries(&self) -> [wgpu::BindGroupEntry<'_>; 5] {
        [
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&self.low_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&self.fine.view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&self.mid.view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: self.bounds.as_entire_binding(),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_constants_agree_with_the_shader() {
        let wgsl = CARTOGRAPHY_WGSL;
        assert!(wgsl.contains(&format!(
            "const FINE_TEXELS: f32 = {:.1};",
            FINE_TEXELS as f32
        )));
        assert!(wgsl.contains(&format!(
            "const MID_TEXELS: f32 = {:.1};",
            MID_TEXELS as f32
        )));
        assert!(wgsl.contains("const FINE_PERIOD: f32 = 8.0;"));
        assert!(wgsl.contains("const MID_PERIOD: f32 = 10.0;"));
    }

    /// The lattice periods of the tiles are whole numbers of cells per period, so the noise (and
    /// the baked tile) is seamless: frequency x period must be an integer for every pattern.
    #[test]
    fn pattern_frequencies_fit_the_tile_periods() {
        for (freq, period, cells) in [
            (23.75f32, 8.0f32, 190.0f32),
            (49.0, 8.0, 392.0),
            (8.25, 8.0, 66.0),
            (17.0, 10.0, 170.0),
            (7.3, 10.0, 73.0),
        ] {
            assert!(
                (freq * period - cells).abs() < 1e-4f32,
                "{freq} x {period} != {cells}"
            );
            assert!(
                CARTOGRAPHY_WGSL.contains(&format!("{cells:.1}")),
                "{cells} cells in the shader"
            );
        }
    }

    #[test]
    fn low_field_covers_the_map_and_fits_the_limit() {
        let p = Placement::new((80, 52), TEXELS_PER_UNIT, 2048);
        let far_x = 1.732_050_8 * (79.0 + 51.0 * 0.5);
        let far_z = 1.5 * 51.0;
        let b = p.bounds();
        assert!((far_x - b[0]) * b[2] < 1.0 && (far_z - b[1]) * b[3] < 1.0);
        assert!(p.field.0 <= 2048 && p.field.1 <= 2048);
        let huge = Placement::new((4000, 3000), TEXELS_PER_UNIT, 2048);
        assert!(huge.field.0 <= 2048 && huge.field.1 <= 2048 && huge.texel > p.texel);
    }

    #[test]
    fn procedural_source_flips_only_the_constant() {
        let src = "a\nconst PARCHMENT_BAKED: bool = true;\nb";
        assert_eq!(configure_shader(src, Source::Baked), src);
        assert!(
            configure_shader(src, Source::Procedural).contains("PARCHMENT_BAKED: bool = false;")
        );
    }
}
