//! One real directional depth map shared by all surface receivers.
//!
//! Casters are drawn straight from the renderer's existing GPU buffers (terrain chunks,
//! feature chunks, instanced models), culled to the shadow area: re-rendering the map
//! after a unit move uploads only the 80-byte fit uniform. The area follows the camera
//! (`shadow_fit.rs`), so resolution follows what the player looks at; the texel grid is
//! fixed in the world, so a moved area does not make the shadows shimmer.
use glam::{Mat4, Vec3};

use crate::gpu::context::GpuCore;
use crate::gpu::memory::{Category, GpuBuffer, GpuTexture};

pub const SHADOW_SIZE: u32 = 2048;
pub const RECEIVER_WGSL: &str = include_str!("../../../../assets/shaders/surface_shadow.wgsl");
const DEPTH_WGSL: &str = r#"
struct Shadow { view_proj: mat4x4<f32>, settings: vec4<f32> };
@group(0) @binding(0) var<uniform> shadow: Shadow;
@vertex fn vs_depth(@location(0) world: vec3<f32>) -> @builtin(position) vec4<f32> {
    return shadow.view_proj * vec4<f32>(world, 1.0);
}
@vertex fn vs_model(@location(0) local: vec3<f32>, @location(4) offset: vec3<f32>,
                    @location(6) scale_height: vec2<f32>, @location(8) yaw: f32,
                    @location(9) slope: vec2<f32>, @location(10) anim: vec4<f32>,
                    @location(11) rig: vec4<f32>) -> @builtin(position) vec4<f32> {
    let c = cos(yaw);
    let s = sin(yaw);
    var pos = local;
    if (rig.y > 0.5) {
        pos = animate_unit(local, vec3<f32>(0.0, 1.0, 0.0), anim, rig, 0.0, unit_hash(offset.x * 0.37 + offset.z * 0.91)).pos;
    }
    let dx = (pos.x * c - pos.z * s) * scale_height.x;
    let dz = (pos.x * s + pos.z * c) * scale_height.x;
    let dy = pos.y * scale_height.y + slope.x * dx + slope.y * dz;
    return shadow.view_proj * vec4<f32>(offset + vec3<f32>(dx, dy, dz), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct ShadowUniform {
    view_proj: [[f32; 4]; 4],
    settings: [f32; 4],
}

/// Instanced model caster: mesh, vertex range, instances, instance range.
pub type ModelCaster<'a> = (
    &'a wgpu::Buffer,
    std::ops::Range<u32>,
    &'a wgpu::Buffer,
    std::ops::Range<u32>,
);

/// Shadow-casting draws taken from buffers the renderer already owns.
#[derive(Default)]
pub struct Casters<'a> {
    /// Indexed terrain chunks: vertices (position at offset 0), indices, index count.
    pub terrain: Vec<(&'a wgpu::Buffer, &'a wgpu::Buffer, u32)>,
    /// Non-indexed feature vertices (position at offset 0) and their count.
    pub features: Vec<(&'a wgpu::Buffer, u32)>,
    pub models: Vec<ModelCaster<'a>>,
}

/// Vertex strides of the caster sources; positions are always the first Float32x3.
#[derive(Clone, Copy)]
pub struct CasterStrides {
    pub terrain: u64,
    pub feature: u64,
    pub model: u64,
    /// Instance layout: offset Float32x3 at 0, scale_height Float32x2 at 24.
    pub instance: u64,
}

pub struct SurfaceShadows {
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
    uniform: GpuBuffer,
    /// Terrain, features and cities: re-rendered only when they or the fit change.
    static_depth: GpuTexture,
    static_view: wgpu::TextureView,
    /// Sampled map: the static map plus units, recomposed when units move.
    depth: GpuTexture,
    depth_view: wgpu::TextureView,
    depth_bind_group: wgpu::BindGroup,
    /// Static map plus the resting units: the posed units are drawn over a copy of it, only
    /// where they stood and stand (`dirty`), not over the whole map.
    rest_depth: GpuTexture,
    /// The draw that restores a patch of `depth` from `rest_depth`. None on WebGL2/GLES, whose
    /// shader translator cannot `textureLoad` a depth texture: the whole map is copied instead.
    restore: Option<(wgpu::RenderPipeline, wgpu::BindGroup)>,
    /// Texel rectangle (x0, y0, x1, y1) of the sampled map the posed units were drawn in.
    dirty: Option<[u32; 4]>,
    terrain_pipeline: wgpu::RenderPipeline,
    feature_pipeline: wgpu::RenderPipeline,
    model_pipeline: wgpu::RenderPipeline,
    current: Option<ShadowUniform>,
    timing: Option<crate::gpu::timing::GpuTiming>,
}

fn uniform_entry(visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

const POSITION: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];
const INSTANCE: [wgpu::VertexAttribute; 6] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 4,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 24,
        shader_location: 6,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32,
        offset: 36,
        shader_location: 8,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x2,
        offset: 40,
        shader_location: 9,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 48,
        shader_location: 10,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 64,
        shader_location: 11,
    },
];

fn positions(stride: u64) -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: stride,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &POSITION,
    }
}

fn depth_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    entry: &str,
    buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("surface directional shadow depth"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            buffers,
        },
        fragment: None,
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: wgpu::DepthBiasState {
                constant: 1,
                slope_scale: 1.25,
                clamp: 0.,
            },
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

const RESTORE_WGSL: &str = r#"
@group(0) @binding(0) var rest: texture_depth_2d;
@vertex fn vs_restore(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs_restore(@builtin(position) pos: vec4<f32>) -> @builtin(frag_depth) f32 {
    return textureLoad(rest, vec2<i32>(pos.xy), 0);
}
"#;

/// Writes the depth of `rest` into the bound depth target inside the scissor rectangle (a
/// depth texture cannot be copied in parts, so the patch is restored by a draw).
fn restore_pipeline(device: &wgpu::Device, rest: &GpuTexture) -> (wgpu::RenderPipeline, wgpu::BindGroup) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shadow restore layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        }],
    });
    let view = rest.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow restore"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("shadow restore shader"), source: wgpu::ShaderSource::Wgsl(RESTORE_WGSL.into()) });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("shadow restore pipeline layout"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shadow restore"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_restore"), compilation_options: Default::default(), buffers: &[] },
        fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("fs_restore"), compilation_options: Default::default(), targets: &[] }),
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    (pipeline, bind_group)
}

fn receiver_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("surface shadow receiver layout"),
        entries: &[
            uniform_entry(wgpu::ShaderStages::FRAGMENT),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
        ],
    })
}

fn depth_map(core: &GpuCore, label: &str, usage: wgpu::TextureUsages) -> GpuTexture {
    core
        .texture(Category::Shadows, &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: SHADOW_SIZE,
                height: SHADOW_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | usage,
            view_formats: &[],
        })
}

impl SurfaceShadows {
    pub fn new(core: &GpuCore, strides: CasterStrides) -> Self {
        let device = &core.device;
        let layout = receiver_layout(device);
        let uniform = core.buffer(Category::Uniforms, &wgpu::BufferDescriptor {
            label: Some("surface shadow uniform"),
            size: std::mem::size_of::<ShadowUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let static_depth =
            depth_map(core, "static shadow casters", wgpu::TextureUsages::COPY_SRC);
        let depth = depth_map(
            core,
            "directional shadow map",
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        );
        let depth_view = depth.create_view(&Default::default());
        let static_view = static_depth.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow PCF comparison"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("surface shadow receiver"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&depth_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let depth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow depth uniform layout"),
            entries: &[uniform_entry(wgpu::ShaderStages::VERTEX)],
        });
        let depth_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow depth uniform"),
            layout: &depth_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow depth shader"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{}", super::pipelines::UNIT_ANIM_WGSL, DEPTH_WGSL).into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow depth pipeline layout"),
            bind_group_layouts: &[Some(&depth_layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
            depth_pipeline(device, &pipeline_layout, &shader, entry, buffers)
        };
        let instances = wgpu::VertexBufferLayout {
            array_stride: strides.instance,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE,
        };
        // naga's GLSL output has no `textureLoad` of a depth texture (WebGL2, GLES): there the patch is
        // restored by a copy of the whole map (`compose_posed`), and the map is a copy source.
        let gl = core.adapter.get_info().backend == wgpu::Backend::Gl;
        let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        let rest_depth = depth_map(core, "static casters and resting units", if gl { usage | wgpu::TextureUsages::COPY_SRC } else { usage });
        let restore = (!gl).then(|| restore_pipeline(device, &rest_depth));
        Self {
            terrain_pipeline: pipeline("vs_depth", &[Some(positions(strides.terrain))]),
            feature_pipeline: pipeline("vs_depth", &[Some(positions(strides.feature))]),
            model_pipeline: pipeline("vs_model", &[Some(positions(strides.model)), Some(instances)]),
            layout,
            bind_group,
            uniform,
            static_depth,
            static_view,
            depth,
            depth_view,
            depth_bind_group,
            restore,
            rest_depth,
            dirty: None,
            current: None,
            timing: core.timing.clone(),
        }
    }

    /// Fits the light frustum to a world box (min, max corners); None disables shadows.
    /// Returns the bytes written (zero when the fit is unchanged).
    pub fn set_fit(
        &mut self,
        queue: &wgpu::Queue,
        bounds: Option<([f32; 3], [f32; 3])>,
        sun: [f32; 3],
    ) -> u64 {
        let uniform = match bounds {
            Some((low, high)) => fit(&box_corners(low, high), sun),
            None => fit(&[], sun),
        };
        if self.current == Some(uniform) {
            return 0;
        }
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        self.current = Some(uniform);
        std::mem::size_of::<ShadowUniform>() as u64
    }

    pub fn enabled(&self) -> bool {
        self.current.is_some_and(|u| u.settings[3] > 0.5)
    }

    fn depth_pass<'a>(
        &self,
        encoder: &'a mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        load: wgpu::LoadOp<f32>,
        stage: crate::gpu::timing::Stage,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("directional shadow depth pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: self.timing.as_ref().map(|t| t.writes(stage)),
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// Clears the static map and draws the static casters; returns submitted triangles.
    pub fn render_static(&self, encoder: &mut wgpu::CommandEncoder, casters: &Casters<'_>) -> usize {
        let mut pass = self.depth_pass(encoder, &self.static_view, wgpu::LoadOp::Clear(1.), crate::gpu::timing::Stage::ShadowStatic);
        if !self.enabled() {
            return 0;
        }
        let mut triangles = 0usize;
        pass.set_bind_group(0, &self.depth_bind_group, &[]);
        pass.set_pipeline(&self.terrain_pipeline);
        for (vertices, indices, count) in &casters.terrain {
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..*count, 0, 0..1);
            triangles += *count as usize / 3;
        }
        pass.set_pipeline(&self.feature_pipeline);
        for (vertices, count) in &casters.features {
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(0..*count, 0..1);
            triangles += *count as usize / 3;
        }
        triangles + self.draw_models(&mut pass, &casters.models)
    }

    /// Copies the static map into the sampled one and draws the resting units on top; the
    /// result is kept for `compose_posed`. Forgets where posed units were drawn.
    pub fn compose_rest(&mut self, encoder: &mut wgpu::CommandEncoder, models: &[ModelCaster<'_>]) -> usize {
        self.dirty = None;
        if !self.enabled() {
            // No shadows: the sampled map is never read, so it needs neither the copy nor a pass.
            return 0;
        }
        encoder.copy_texture_to_texture(self.static_depth.as_image_copy(), self.depth.as_image_copy(), self.depth.size());
        let triangles = {
            let mut pass = self.depth_pass(encoder, &self.depth_view, wgpu::LoadOp::Load, crate::gpu::timing::Stage::ShadowUnits);
            pass.set_bind_group(0, &self.depth_bind_group, &[]);
            self.draw_models(&mut pass, models)
        };
        encoder.copy_texture_to_texture(self.depth.as_image_copy(), self.rest_depth.as_image_copy(), self.depth.size());
        triangles
    }

    /// Draws the posed units over the map `compose_rest` left. Only the rectangle `rect`
    /// (where they are now) and the one of the previous call (where they were) are restored
    /// and drawn: moving units cost a patch of the map, not 2048 x 2048 texels of copy and
    /// load every frame. `rect` None: no posed unit in the shadow area.
    pub fn compose_posed(&mut self, encoder: &mut wgpu::CommandEncoder, models: &[ModelCaster<'_>], rect: Option<[u32; 4]>) -> usize {
        if !self.enabled() {
            return 0;
        }
        let region = match (self.dirty, rect) {
            (Some(a), Some(b)) => Some([a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]),
            (a, b) => a.or(b),
        };
        self.dirty = rect;
        let Some([x0, y0, x1, y1]) = region else {
            return 0;
        };
        let (width, height) = (x1 - x0, y1 - y0);
        if width == 0 || height == 0 {
            return 0;
        }
        if self.restore.is_none() {
            // The patch is the whole map again; outside the patch it holds what `rest_depth` holds already.
            encoder.copy_texture_to_texture(self.rest_depth.as_image_copy(), self.depth.as_image_copy(), self.depth.size());
        }
        let mut pass = self.depth_pass(encoder, &self.depth_view, wgpu::LoadOp::Load, crate::gpu::timing::Stage::ShadowUnits);
        pass.set_scissor_rect(x0, y0, width, height);
        if let Some((pipeline, bind_group)) = &self.restore {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        if rect.is_none() {
            return 0;
        }
        pass.set_bind_group(0, &self.depth_bind_group, &[]);
        self.draw_models(&mut pass, models)
    }

    /// The texel rectangle of the sampled map that figures standing at `points` (world
    /// positions, each with `radius` world units of reach) can darken or light, clamped to
    /// the map; None when no point is given or none reaches the map.
    pub fn texel_rect(&self, points: impl Iterator<Item = [f32; 3]>, radius: f32) -> Option<[u32; 4]> {
        let uniform = self.current.filter(|u| u.settings[3] > 0.5)?;
        let m = Mat4::from_cols_array_2d(&uniform.view_proj);
        let half = SHADOW_SIZE as f32 * 0.5;
        // Texels per world unit along the light's x and y (the projection is orthographic).
        let reach = |row: usize| Vec3::new(m.col(0)[row], m.col(1)[row], m.col(2)[row]).length() * half * radius + 3.0;
        let (rx, ry) = (reach(0), reach(1));
        let (mut low, mut high) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for p in points {
            let clip = m * glam::Vec4::new(p[0], p[1], p[2], 1.0);
            let (u, v) = ((clip.x / clip.w + 1.0) * half, (1.0 - clip.y / clip.w) * half);
            low = [low[0].min(u - rx), low[1].min(v - ry)];
            high = [high[0].max(u + rx), high[1].max(v + ry)];
        }
        let size = SHADOW_SIZE as f32;
        let clamp = |v: f32| v.clamp(0.0, size);
        let rect = [clamp(low[0]).floor() as u32, clamp(low[1]).floor() as u32, clamp(high[0]).ceil() as u32, clamp(high[1]).ceil() as u32];
        (rect[2] > rect[0] && rect[3] > rect[1]).then_some(rect)
    }

    fn draw_models(&self, pass: &mut wgpu::RenderPass<'_>, models: &[ModelCaster<'_>]) -> usize {
        let mut triangles = 0usize;
        pass.set_pipeline(&self.model_pipeline);
        for (mesh, vertices, instances, range) in models {
            pass.set_vertex_buffer(0, mesh.slice(..));
            pass.set_vertex_buffer(1, instances.slice(..));
            pass.draw(vertices.clone(), range.clone());
            triangles += vertices.len() / 3 * range.len();
        }
        triangles
    }
}

fn box_corners(low: [f32; 3], high: [f32; 3]) -> [[f32; 3]; 8] {
    std::array::from_fn(|i| {
        [
            if i & 1 == 0 { low[0] } else { high[0] },
            if i & 2 == 0 { low[1] } else { high[1] },
            if i & 4 == 0 { low[2] } else { high[2] },
        ]
    })
}

fn fit(positions: &[[f32; 3]], sun: [f32; 3]) -> ShadowUniform {
    if positions.is_empty() {
        return ShadowUniform {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            settings: [1. / SHADOW_SIZE as f32, 0., 0., 0.],
        };
    }
    let (low, high) = bounds(positions.iter().map(|p| Vec3::from_array(*p)));
    let radius = (high - low).length().max(1.) * 0.5;
    let sun = Vec3::from_array(sun).try_normalize().unwrap_or(Vec3::Y);
    let up = if sun.dot(Vec3::Y).abs() > 0.95 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    // The light looks at the world origin, whatever the fit: a world point always has
    // the same light-space position, so the texel grid below is fixed in the world.
    let view = glam::camera::rh::view::look_at_mat4(sun * LIGHT_DISTANCE, Vec3::ZERO, up);
    let (low, high) = bounds(positions.iter().map(|p| view.transform_point3(Vec3::from_array(*p))));
    let pad = (radius * 0.035).max(0.3);
    let (x, y) = snapped_square(low.x - pad, high.x + pad, low.y - pad, high.y + pad);
    let projection = glam::camera::rh::proj::directx::orthographic(
        x[0],
        x[1],
        y[0],
        y[1],
        -high.z - pad,
        -low.z + pad,
    );
    let texel_world = (x[1] - x[0]) / SHADOW_SIZE as f32;
    ShadowUniform {
        view_proj: (projection * view).to_cols_array_2d(),
        settings: [1. / SHADOW_SIZE as f32, 0.00012, texel_world * 0.65, 1.],
    }
}

/// Componentwise minimum and maximum of points.
fn bounds(points: impl Iterator<Item = Vec3>) -> (Vec3, Vec3) {
    points.fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(low, high), p| (low.min(p), high.max(p)))
}

/// Distance of the light's eye from the world origin. The projection is orthographic, so
/// any eye on the light axis works (near may be negative); a small one keeps depth precise.
const LIGHT_DISTANCE: f32 = 1.0;
/// Steps per doubling of the light-space size: zooming changes the texel size in
/// steps of 19 %, not continuously, so a resting zoom never resamples the map.
const SIZE_STEPS: f32 = 4.0;

/// A square covering x0..x1 × y0..y1 in light space whose side is a quantized size and
/// whose edges lie on the texel grid of that size: the same texels in every fit.
fn snapped_square(x0: f32, x1: f32, y0: f32, y1: f32) -> ([f32; 2], [f32; 2]) {
    // Two texels of room (2 / SHADOW_SIZE of the side, under 0.2 %): the snapped square
    // still covers the box after its start moved down to the grid.
    let extent = (x1 - x0).max(y1 - y0).max(1.0) * 1.002;
    let side = 2f32.powf((extent.log2() * SIZE_STEPS).ceil() / SIZE_STEPS);
    let texel = side / SHADOW_SIZE as f32;
    let snap = |low: f32, high: f32| {
        // Whole texels from the origin; the side is SHADOW_SIZE texels.
        let start = (((low + high) * 0.5 - side * 0.5) / texel).floor() * texel;
        [start, start + side]
    };
    (snap(x0, x1), snap(y0, y1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::util::DeviceExt;

    fn scene() -> Vec<[f32; 3]> {
        vec![
            [-2., 0., -2.],
            [2., 0., -2.],
            [2., 0., 2.],
            [-2., 0., -2.],
            [2., 0., 2.],
            [-2., 0., 2.],
            [-0.8, 1., -0.8],
            [0.8, 1., -0.8],
            [0., 1., 0.8],
        ]
    }

    #[test]
    fn fitted_depth_contains_all_casters_and_handles_vertical_sun() {
        let vertices = scene();
        for sun in [[0., 1., 0.], [-0.4, 0.8, 0.3], [0., 0., 0.]] {
            let u = fit(&vertices, sun);
            let m = Mat4::from_cols_array_2d(&u.view_proj);
            for p in &vertices {
                let p = m.project_point3(Vec3::from_array(*p));
                assert!(p.is_finite());
                assert!(p.x.abs() < 1. && p.y.abs() < 1. && p.z > 0. && p.z < 1.);
            }
        }
        assert_eq!(fit(&[], [0., 1., 0.]).settings[3], 0.);
    }

    /// Two fits of the same texel size put texel corners on the same world grid: moving
    /// the fitted box shifts the map by whole texels, so the shadows do not shimmer.
    #[test]
    fn moved_fits_share_the_texel_grid() {
        let sun = [-0.42, 0.8, 0.35];
        let a = fit(&box_corners([0., 0., 0.], [40., 2., 30.]), sun);
        let b = fit(&box_corners([3.3, 0., 1.7], [43.3, 2., 31.7]), sun);
        assert_eq!(a.settings[2], b.settings[2], "same texel size");
        let (ma, mb) = (Mat4::from_cols_array_2d(&a.view_proj), Mat4::from_cols_array_2d(&b.view_proj));
        for p in [[10., 0.5, 10.], [25.3, 1.1, 7.9], [33.0, 0.2, 21.4]] {
            let (pa, pb) = (ma.project_point3(Vec3::from_array(p)), mb.project_point3(Vec3::from_array(p)));
            // NDC to texels: the offset between the two maps is a whole number of texels.
            for (u, v) in [(pa.x, pb.x), (pa.y, pb.y)] {
                let texels = (u - v) * 0.5 * SHADOW_SIZE as f32;
                assert!((texels - texels.round()).abs() < 0.02, "{texels} texels apart");
            }
        }
    }

    #[test]
    fn box_fit_contains_every_point_inside_the_box() {
        let (low, high) = ([-3., -0.5, -2.], [4., 2., 5.]);
        let u = fit(&box_corners(low, high), [-0.42, 0.8, 0.35]);
        let m = Mat4::from_cols_array_2d(&u.view_proj);
        for p in scene() {
            let p = m.project_point3(Vec3::from_array(p));
            assert!(p.x.abs() < 1. && p.y.abs() < 1. && p.z > 0. && p.z < 1.);
        }
    }

    #[test]
    fn gpu_depth_comparison_casts_shadow_and_empty_scene_removes_it() {
        let core = crate::gpu::context::GpuCore::new_blocking()
            .expect("GPU required for actual shadow validation");
        let error_scope = core.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let strides = CasterStrides {
            terrain: 12,
            feature: 12,
            model: 12,
            instance: std::mem::size_of::<crate::gpu::buffers::HexInstance>() as u64,
        };
        let mut shadows = SurfaceShadows::new(&core, strides);
        let source = format!(
            "{}\n{}",
            RECEIVER_WGSL,
            r#"
struct Out { @builtin(position) clip: vec4<f32>, @location(0) world: vec3<f32> };
@vertex fn vs_main(@builtin(vertex_index) i:u32) -> Out {
    let points = array<vec2<f32>,3>(vec2<f32>(-1.,-1.),vec2<f32>(3.,-1.),vec2<f32>(-1.,3.));
    var o:Out; o.clip=vec4<f32>(points[i],0.,1.); o.world=vec3<f32>(points[i].x*2.,0.,points[i].y*2.); return o;
}
@fragment fn fs_main(i:Out)->@location(0) vec4<f32> {
    let light=surface_shadow(i.world,vec3<f32>(0.,1.,0.));return vec4<f32>(light,light,light,1.);
}
"#
        );
        let shader = core
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("shadow test receiver"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let layout = core
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow test receiver layout"),
                bind_group_layouts: &[None, None, Some(&shadows.layout)],
                immediate_size: 0,
            });
        let pipeline = core
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shadow test receiver pipeline"),
                layout: Some(&layout),
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
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
        let texture = core.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow test output"),
            size: wgpu::Extent3d {
                width: 32,
                height: 32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let readback = core.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow test readback"),
            size: 256 * 32,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let caster_scene = scene();
        let caster_buffer = core
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shadow test casters"),
                contents: bytemuck::cast_slice(&caster_scene),
                usage: wgpu::BufferUsages::VERTEX,
            });
        for enabled in [true, false] {
            let bounds = enabled.then_some(([-2., 0., -2.], [2., 1., 2.]));
            shadows.set_fit(&core.queue, bounds, [0., 1., 0.]);
            let casters = Casters {
                features: vec![(&caster_buffer, caster_scene.len() as u32)],
                ..Default::default()
            };
            let mut encoder = core.device.create_command_encoder(&Default::default());
            shadows.render_static(&mut encoder, &casters);
            shadows.compose_rest(&mut encoder, &[]);
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shadow test receiver pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(2, &shadows.bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(32),
                    },
                },
                wgpu::Extent3d {
                    width: 32,
                    height: 32,
                    depth_or_array_layers: 1,
                },
            );
            core.queue.submit([encoder.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    tx.send(result).unwrap();
                });
            core.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: None,
                })
                .unwrap();
            rx.recv().unwrap().unwrap();
            let bytes = readback.slice(..).get_mapped_range().unwrap();
            let center = bytes[16 * 256 + 16 * 4];
            let corner = bytes[3 * 256 + 3 * 4];
            assert!(
                corner > 245,
                "unoccluded receiver must remain lit, got {corner}"
            );
            if enabled {
                assert!(
                    center < 20,
                    "elevated geometry must cast an actual dark shadow, got {center}"
                );
            } else {
                assert!(
                    center > 245,
                    "disabled shadows must clear the previous shadow, got {center}"
                );
            }
            drop(bytes);
            readback.unmap();
        }
        assert!(
            pollster::block_on(error_scope.pop()).is_none(),
            "GPU shadow validation failed"
        );
    }
}
