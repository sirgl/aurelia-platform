//! Observer knowledge on the GPU: a per-tile state texture (0 unknown, 1 explored,
//! 2 visible) and a world-space fog field baked from it by `fog_field.wgsl`.
//!
//! Every receiver shader samples the field through `map_fog` (cartography.wgsl), so a
//! visibility change uploads only the changed rows of the tile mask and re-bakes the
//! affected band on the GPU. Terrain, water and feature buffers are never touched.
use crate::gpu::context::GpuCore;
use crate::gpu::memory::{Category, GpuBuffer, GpuTexture};
use crate::gpu::parchment::Parchment;
use crate::world::state::RenderWorld;

const BAKE_WGSL: &str = include_str!("../../../../assets/shaders/fog_field.wgsl");
/// Field resolution; comparable to the 0.32 surface vertex spacing it replaces.
pub const TEXELS_PER_UNIT: f32 = 4.0;
/// Field margin around the map, beyond the 3.1 unit reach of the wash.
const MARGIN: f32 = 4.0;
/// Rows of influence of one tile state on the field, in world Z (radius + width ≤ 3.11).
const REACH: f32 = 3.2;
const FIELD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BakeUniform {
    origin_texel: [f32; 4],
    map: [u32; 4],
}

/// World placement of the field for receivers: origin XZ and inverse extent XZ.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FieldBounds(pub [f32; 4]);

struct Placement {
    map: (u32, u32),
    field: (u32, u32),
    origin: [f32; 2],
    texel: f32,
}

impl Placement {
    fn new(map: (u32, u32), max_dimension: u32) -> Self {
        let (w, h) = (map.0.max(1) as f32, map.1.max(1) as f32);
        let extent_x = 1.732_050_8 * ((w - 1.0) + (h - 1.0) * 0.5) + 2.0 * MARGIN;
        let extent_z = 1.5 * (h - 1.0) + 2.0 * MARGIN;
        let largest = extent_x.max(extent_z);
        let per_unit = TEXELS_PER_UNIT.min(max_dimension as f32 / largest);
        let field = (
            ((extent_x * per_unit).ceil() as u32).clamp(1, max_dimension),
            ((extent_z * per_unit).ceil() as u32).clamp(1, max_dimension),
        );
        Self {
            map,
            field,
            origin: [-MARGIN, -MARGIN],
            texel: 1.0 / per_unit,
        }
    }

    fn bounds(&self) -> FieldBounds {
        FieldBounds([
            self.origin[0],
            self.origin[1],
            1.0 / (self.field.0 as f32 * self.texel),
            1.0 / (self.field.1 as f32 * self.texel),
        ])
    }

    /// Field texel rows covering world Z of tile rows `first..=last` plus the wash reach.
    fn texel_rows(&self, first: u32, last: u32) -> (u32, u32) {
        let low = 1.5 * first as f32 - REACH - self.origin[1];
        let high = 1.5 * last as f32 + REACH - self.origin[1];
        let top = ((low / self.texel).floor().max(0.0) as u32).min(self.field.1);
        let bottom = ((high / self.texel).ceil().max(0.0) as u32).min(self.field.1);
        (top, bottom.max(top + 1).min(self.field.1))
    }
}

pub struct FogField {
    /// Layout of group 0 in every scene pipeline: camera + fog field.
    pub layout: wgpu::BindGroupLayout,
    states: GpuTexture,
    field_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    bounds: GpuBuffer,
    bake_layout: wgpu::BindGroupLayout,
    bake_bind: wgpu::BindGroup,
    bake_uniform: GpuBuffer,
    bake_pipeline: wgpu::RenderPipeline,
    placement: Placement,
    pub parchment: Parchment,
    field: GpuTexture,
    uploaded: Vec<u8>,
    /// Bumped whenever the field texture is recreated (bind groups must follow).
    pub generation: u64,
}

/// Bytes and texel rows touched by one knowledge update.
#[derive(Debug, Clone, Copy, Default)]
pub struct FogUpdate {
    pub bytes: u64,
    pub baked_rows: u32,
}

fn tile_states(world: &RenderWorld) -> Vec<u8> {
    (0..(world.width * world.height) as usize)
        .map(|i| {
            if world.visible.get(i) == Some(&1) {
                2
            } else if world.explored.get(i) == Some(&1) {
                1
            } else {
                0
            }
        })
        .collect()
}

fn texture_2d(
    core: &GpuCore,
    label: &str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> GpuTexture {
    core.texture(Category::Fog, &wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn scene_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let uniform = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let mut entries = vec![
        uniform(0),
        wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
        uniform(3),
    ];
    // Bindings 4..=8: the baked parchment (cartography.wgsl).
    entries.extend(crate::gpu::parchment::layout_entries());
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("camera + fog field layout"),
        entries: &entries,
    })
}

fn bake_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fog bake layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

fn bake_pipeline(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fog field bake"),
        source: wgpu::ShaderSource::Wgsl(BAKE_WGSL.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fog field bake"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fog field bake"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FIELD_FORMAT,
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

impl FogField {
    pub fn new(core: &GpuCore) -> Self {
        let device = &core.device;
        let layout = scene_layout(device);
        let bake_layout = bake_layout(device);
        let bake_pipeline = bake_pipeline(device, &bake_layout);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fog field"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bounds = core.buffer(Category::Uniforms, &wgpu::BufferDescriptor {
            label: Some("fog field bounds"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bake_uniform = core.buffer(Category::Uniforms, &wgpu::BufferDescriptor {
            label: Some("fog bake uniform"),
            size: std::mem::size_of::<BakeUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let placement = Placement::new((0, 0), device.limits().max_texture_dimension_2d);
        let (states, field, field_view, bake_bind) =
            Self::create_targets(core, &bake_layout, &bake_uniform, &placement);
        let field_state = Self {
            layout,
            states,
            field_view,
            sampler,
            bounds,
            bake_layout,
            bake_bind,
            bake_uniform,
            bake_pipeline,
            placement,
            parchment: Parchment::new(core),
            field,
            uploaded: Vec::new(),
            generation: 0,
        };
        field_state.write_placement(&core.queue);
        field_state
    }

    fn create_targets(
        core: &GpuCore,
        bake_layout: &wgpu::BindGroupLayout,
        bake_uniform: &wgpu::Buffer,
        placement: &Placement,
    ) -> (GpuTexture, GpuTexture, wgpu::TextureView, wgpu::BindGroup) {
        let states = texture_2d(
            core,
            "fog tile states",
            placement.map,
            wgpu::TextureFormat::R8Uint,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        );
        let field = texture_2d(
            core,
            "fog field",
            placement.field,
            FIELD_FORMAT,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC,
        );
        let field_view = field.create_view(&Default::default());
        let states_view = states.create_view(&Default::default());
        let bake_bind = core.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog bake"),
            layout: bake_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&states_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: bake_uniform.as_entire_binding(),
                },
            ],
        });
        (states, field, field_view, bake_bind)
    }

    fn write_placement(&self, queue: &wgpu::Queue) {
        let p = &self.placement;
        let bake = BakeUniform {
            origin_texel: [p.origin[0], p.origin[1], p.texel, 0.0],
            map: [p.map.0, p.map.1, 0, 0],
        };
        queue.write_buffer(&self.bake_uniform, 0, bytemuck::bytes_of(&bake));
        queue.write_buffer(&self.bounds, 0, bytemuck::bytes_of(&p.bounds()));
    }

    /// Group 0 bind group of the scene pipelines for this field and a camera buffer.
    pub fn scene_bind_group(&self, device: &wgpu::Device, camera: &wgpu::Buffer) -> wgpu::BindGroup {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&self.field_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: self.bounds.as_entire_binding(),
            },
        ];
        entries.extend(self.parchment.bind_entries());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera + fog field"),
            layout: &self.layout,
            entries: &entries,
        })
    }

    /// Uploads changed tile rows and re-bakes the band of the field they influence.
    /// A new map size recreates the textures and re-bakes everything.
    pub fn update(
        &mut self,
        core: &GpuCore,
        encoder: &mut wgpu::CommandEncoder,
        world: &RenderWorld,
    ) -> FogUpdate {
        let map = (world.width, world.height);
        let states = tile_states(world);
        let resized = self.placement.map != map;
        if resized {
            self.placement = Placement::new(map, core.device.limits().max_texture_dimension_2d);
            let (s, f, v, b) =
                Self::create_targets(core, &self.bake_layout, &self.bake_uniform, &self.placement);
            (self.states, self.field, self.field_view, self.bake_bind) = (s, f, v, b);
            self.generation += 1;
            self.parchment.rebuild(core, encoder, map);
            self.write_placement(&core.queue);
            self.uploaded = vec![u8::MAX; states.len()];
        }
        let Some((first, last)) = changed_rows(&self.uploaded, &states, map.0) else {
            return FogUpdate::default();
        };
        let width = map.0 as usize;
        let rows = &states[first as usize * width..(last as usize + 1) * width];
        core.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.states,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: first, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rows,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(map.0),
                rows_per_image: Some(last - first + 1),
            },
            wgpu::Extent3d {
                width: map.0,
                height: last - first + 1,
                depth_or_array_layers: 1,
            },
        );
        let bytes = rows.len() as u64 + if resized { 64 } else { 0 };
        self.uploaded = states;
        let band = if resized {
            (0, self.placement.field.1)
        } else {
            self.placement.texel_rows(first, last)
        };
        self.bake(encoder, band);
        FogUpdate {
            bytes,
            baked_rows: band.1 - band.0,
        }
    }

    fn bake(&self, encoder: &mut wgpu::CommandEncoder, rows: (u32, u32)) {
        let full = rows == (0, self.placement.field.1);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fog field bake"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.field_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if full {
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.bake_pipeline);
        pass.set_bind_group(0, &self.bake_bind, &[]);
        pass.set_scissor_rect(0, rows.0, self.placement.field.0, rows.1 - rows.0);
        pass.draw(0..3, 0..1);
    }
}

/// First and last tile rows whose states differ (None when identical).
fn changed_rows(old: &[u8], new: &[u8], width: u32) -> Option<(u32, u32)> {
    let width = width.max(1) as usize;
    if old.len() != new.len() {
        return Some((0, (new.len().div_ceil(width)).saturating_sub(1) as u32));
    }
    let first = old.iter().zip(new).position(|(a, b)| a != b)?;
    let last = old.iter().zip(new).rposition(|(a, b)| a != b)?;
    Some(((first / width) as u32, (last / width) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_rows_cover_exactly_the_differing_band() {
        let old = vec![0u8; 12];
        let mut new = old.clone();
        assert_eq!(changed_rows(&old, &new, 4), None);
        new[5] = 2;
        new[9] = 1;
        assert_eq!(changed_rows(&old, &new, 4), Some((1, 2)));
        assert_eq!(changed_rows(&[], &new, 4), Some((0, 2)));
    }

    #[test]
    fn bake_band_covers_the_wash_reach_and_stays_in_the_field() {
        let p = Placement::new((40, 30), 8192);
        let (top, bottom) = p.texel_rows(10, 10);
        let z_top = p.origin[1] + top as f32 * p.texel;
        let z_bottom = p.origin[1] + bottom as f32 * p.texel;
        assert!(z_top <= 15.0 - 3.11 && z_bottom >= 15.0 + 3.11);
        let (_, end) = p.texel_rows(0, 29);
        assert!(end <= p.field.1);
        let bounds = p.bounds().0;
        let far_x = 1.732_050_8 * (39.0 + 29.0 * 0.5);
        assert!((far_x - bounds[0]) * bounds[2] < 1.0, "field covers the map");
    }

    fn half_to_f32(bits: u16) -> f32 {
        let exponent = ((bits >> 10) & 0x1f) as i32;
        let mantissa = (bits & 0x3ff) as f32;
        let value = match exponent {
            0 => mantissa / 1024.0 * 2f32.powi(-14),
            _ => (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15),
        };
        if bits & 0x8000 != 0 {
            -value
        } else {
            value
        }
    }

    fn read_field(core: &crate::gpu::context::GpuCore, field: &FogField) -> Vec<f32> {
        let (w, h) = field.placement.field;
        let pitch = (w * 2).div_ceil(256) * 256;
        let buffer = core.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fog field readback"),
            size: (pitch * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = core.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            field.field.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(pitch),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        core.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
        core.device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: None })
            .unwrap();
        let bytes = buffer.slice(..).get_mapped_range().unwrap().to_vec();
        let mut out = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                let i = (y * pitch + x * 2) as usize;
                out.push(half_to_f32(u16::from_le_bytes([bytes[i], bytes[i + 1]])));
            }
        }
        out
    }

    /// The GPU bake is an exact port of scene::fog::fog_at_position, including after a
    /// partial (dirty band) re-bake.
    #[test]
    fn gpu_bake_matches_cpu_fog_after_full_and_partial_updates() {
        let core = crate::gpu::context::GpuCore::new_blocking().expect("GPU required");
        let (w, h) = (14u32, 12u32);
        let n = (w * h) as usize;
        let mut world = RenderWorld {
            width: w,
            height: h,
            visible: vec![0; n],
            explored: vec![0; n],
            ..Default::default()
        };
        for i in 0..n {
            let hash = crate::scene::terrain::fields::hash_u32(i as u32 ^ 0x51ed);
            world.explored[i] = u8::from(hash % 3 != 0);
            world.visible[i] = u8::from(world.explored[i] == 1 && hash % 5 < 2);
        }
        let mut field = FogField::new(&core);
        for step in 0..2 {
            if step == 1 {
                // Visibility moves inside rows 5..=6 only: a partial band re-bake.
                for i in (5 * w as usize)..(7 * w as usize) {
                    world.visible[i] = u8::from(world.visible[i] == 0 && world.explored[i] == 1);
                }
            }
            let mut encoder = core.device.create_command_encoder(&Default::default());
            let update = field.update(&core, &mut encoder, &world);
            core.queue.submit([encoder.finish()]);
            assert!(update.bytes > 0);
            let baked = read_field(&core, &field);
            let p = &field.placement;
            let mut worst = 0.0f32;
            for y in 0..p.field.1 {
                for x in 0..p.field.0 {
                    let wx = p.origin[0] + (x as f32 + 0.5) * p.texel;
                    let wz = p.origin[1] + (y as f32 + 0.5) * p.texel;
                    let cpu = crate::scene::fog::fog_at_position(&world, wx, wz);
                    let gpu = baked[(y * p.field.0 + x) as usize];
                    worst = worst.max((cpu - gpu).abs());
                }
            }
            assert!(worst < 2e-3, "step {step}: GPU fog differs from CPU by {worst}");
        }
    }

    #[test]
    fn huge_maps_clamp_the_field_to_device_limits() {
        let p = Placement::new((4000, 3000), 2048);
        assert!(p.field.0 <= 2048 && p.field.1 <= 2048);
        assert!(p.texel > 1.0 / TEXELS_PER_UNIT);
    }
}
