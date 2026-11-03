//! Pipeline constructors (§30 platform/structure.md: gpu/pipelines.rs).
//!
//! Frame orchestration is in renderer.rs; here only the creation of GPU objects:
//! layouts, three pipes (terrain with textures, units, grid), the depth config.

use crate::gpu::buffers::{HexInstance, TerrainVertex};

/// The procedural unit animation (walk, attack, hit), shared by the scene and shadow passes.
pub const UNIT_ANIM_WGSL: &str = include_str!("../../../../assets/shaders/unit_anim.wgsl");

/// Depth buffer format (no stencil).
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;

/// Samples per pixel of the scene pass by default: 4, or 1 with `AURELIA_MSAA=1` (2 is not
/// portable: WebGPU guarantees 1 and 4). Read once; the player's setting overrides it.
pub fn msaa_samples() -> u32 {
    static SAMPLES: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *SAMPLES.get_or_init(
        || match std::env::var("AURELIA_MSAA").ok().and_then(|v| v.parse().ok()) {
            Some(1) => 1,
            _ => 4,
        },
    )
}

/// Samples the scene pass supports: 1 or 4 (anything else means 4).
pub fn clamp_samples(samples: u32) -> u32 {
    if samples == 1 {
        1
    } else {
        4
    }
}

/// Icon atlas layout (group 1): a 2D texture + sampler.
/// Separate from terrain: there it is an array, here a flat atlas.
pub fn icons_texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("icons texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// Terrain texture layout (group 1): array + sampler.
pub fn terrain_texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("terrain texture layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// Development only (shader bisecting without a rebuild): with AURELIA_SHADER_DIR set, the receiver
/// shaders (terrain, water, feature, units) are assembled from the files of that directory, and
/// every name of AURELIA_SHADER_STUBS (comma-separated) turns `const STUB_<name>: bool = false;`
/// into `true`.
fn shader_dev_override(label: &str, source: String) -> String {
    let file = match label {
        "terrain" => "terrain.wgsl",
        "water" => "water.wgsl",
        "feature" => "feature.wgsl",
        "units" => "hex.wgsl",
        _ => return source,
    };
    let Some(dir) = std::env::var_os("AURELIA_SHADER_DIR") else {
        return source;
    };
    let dir = std::path::PathBuf::from(dir);
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let anim = if label == "units" { read("unit_anim.wgsl") } else { String::new() };
    let mut source = format!("{}\n{}\n{}\n{}", read("surface_shadow.wgsl"), anim, read("cartography.wgsl"), read(file));
    for stub in std::env::var("AURELIA_SHADER_STUBS").unwrap_or_default().split(',').filter(|s| !s.is_empty()) {
        source = source.replace(&format!("const STUB_{stub}: bool = false;"), &format!("const STUB_{stub}: bool = true;"));
    }
    source
}

fn make_pipeline(
    device: &wgpu::Device,
    label: &str,
    shader_src: &str,
    topology: wgpu::PrimitiveTopology,
    vertex_layouts: &[Option<wgpu::VertexBufferLayout<'_>>],
    bind_layouts: &[Option<&wgpu::BindGroupLayout>],
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    // A browser's WGSL validator (Tint) rejects `fwidth` under non-uniform control flow, which
    // the native compilers accept (lands/docs/web.md: the shaders should compute it uniformly).
    let directive = if cfg!(target_arch = "wasm32") { "diagnostic(off, derivative_uniformity);" } else { "" };
    let source = format!(
        "{directive}\n{}\n{}\n{}\n{}",
        if matches!(label, "terrain" | "feature" | "units" | "water") {
            super::surface_shadows::RECEIVER_WGSL
        } else {
            ""
        },
        if label == "units" { UNIT_ANIM_WGSL } else { "" },
        include_str!("../../../../assets/shaders/cartography.wgsl"),
        shader_src
    );
    let source = shader_dev_override(label, source);
    let source = super::parchment::configure_shader(&source, super::parchment::source(device));
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: bind_layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: vertex_layouts,
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: if label == "feature" || label == "grid" || label == "soft_borders" {
                    Some(wgpu::BlendState::ALPHA_BLENDING)
                } else if label == "effects" {
                    Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)
                } else {
                    None
                },
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(label != "soft_borders" && label != "effects"),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}

pub fn terrain_pipeline(
    device: &wgpu::Device,
    shader_src: &str,
    camera_layout: &wgpu::BindGroupLayout,
    texture_layout: &wgpu::BindGroupLayout,
    shadow_layout: &wgpu::BindGroupLayout,
    road_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    let shader_src = if super::visual_river::shader_roads() {
        shader_src.to_owned()
    } else {
        shader_src.replace(
            "const ROAD_MASK_ENABLED: bool = true;",
            "const ROAD_MASK_ENABLED: bool = false;",
        )
    };
    let shader_src = if std::env::var("AURELIA_VISUAL_GROUND").as_deref() == Ok("plain") {
        shader_src.replace(
            "const GROUND_DETAIL_ENABLED: bool = true;",
            "const GROUND_DETAIL_ENABLED: bool = false;",
        )
    } else {
        shader_src
    };
    make_pipeline(
        device,
        "terrain",
        &shader_src,
        wgpu::PrimitiveTopology::TriangleList,
        &[Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3,
                1 => Float32x3,
                2 => Float32x2,
                3 => Uint32,
                4 => Uint32,
                5 => Float32,
                6 => Float32,
                7 => Float32x4
            ],
        })],
        &[
            Some(camera_layout),
            Some(texture_layout),
            Some(shadow_layout),
            Some(road_layout),
        ],
        format,
        samples,
    )
}

/// Hard features (mountains): pos + normal + color + fog, flat shading.
pub fn feature_pipeline(
    device: &wgpu::Device,
    shader_src: &str,
    camera_layout: &wgpu::BindGroupLayout,
    texture_layout: &wgpu::BindGroupLayout,
    shadow_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    make_pipeline(
        device,
        "feature",
        shader_src,
        wgpu::PrimitiveTopology::TriangleList,
        &[Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::gpu::feature::GpuFeatureVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3,
                1 => Float32,
                2 => Float16x4,
                3 => Float16x4
            ],
        })],
        &[
            Some(camera_layout),
            Some(texture_layout),
            Some(shadow_layout),
        ],
        format,
        samples,
    )
}

/// Entity models (Blender bake, v36): pos + normal + color + team,
/// instance + owner colour + scale + fog. The shader is lambert + fog
/// (hex.wgsl), light as for terrain/features.
pub fn unit_pipeline(
    device: &wgpu::Device,
    shader_src: &str,
    camera_layout: &wgpu::BindGroupLayout,
    texture_layout: &wgpu::BindGroupLayout,
    shadow_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    make_pipeline(
        device,
        "units",
        shader_src,
        wgpu::PrimitiveTopology::TriangleList,
        &[
            Some(wgpu::VertexBufferLayout {
                array_stride: 40,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<HexInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![4 => Float32x3, 5 => Float32x3, 6 => Float32x2, 7 => Float32, 8 => Float32, 9 => Float32x2, 10 => Float32x4, 11 => Float32x4, 12 => Float32x4],
            }),
        ],
        &[
            Some(camera_layout),
            Some(texture_layout),
            Some(shadow_layout),
        ],
        format,
        samples,
    )
}

pub fn grid_pipeline(
    device: &wgpu::Device,
    shader_src: &str,
    camera_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    make_pipeline(
        device,
        "grid",
        shader_src,
        wgpu::PrimitiveTopology::LineList,
        &[Some(wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        })],
        &[Some(camera_layout)],
        format,
        samples,
    )
}

/// Ribbons (borders/thick highlight): pos+color vertices, a flat shader.
pub fn ribbon_pipeline(
    device: &wgpu::Device,
    shader_src: &str,
    camera_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    make_pipeline(
        device,
        "ribbons",
        shader_src,
        wgpu::PrimitiveTopology::TriangleList,
        &[Some(wgpu::VertexBufferLayout {
            array_stride: 24,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3],
        })],
        &[Some(camera_layout)],
        format,
        samples,
    )
}

/// Yield icons: a quad (uv) + instances (offset, cell+scale).
/// The second bind is the icon atlas (2D, not an array: a separate layout).
pub fn icon_pipeline(
    device: &wgpu::Device,
    shader_src: &str,
    camera_layout: &wgpu::BindGroupLayout,
    icons_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    make_pipeline(
        device,
        "icons",
        shader_src,
        wgpu::PrimitiveTopology::TriangleList,
        &[
            Some(wgpu::VertexBufferLayout {
                array_stride: 8,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x2],
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 20,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![1 => Float32x3, 2 => Float32x2],
            }),
        ],
        &[Some(camera_layout), Some(icons_layout)],
        format,
        samples,
    )
}

/// Live water: pos + uv (u along the world, v across: 0/1 shore).
/// Camera only (no textures: everything is procedural in the shader).
/// v35: +shade (ocean depth), stride 24->28; +tone (river mouth, palette, white water), 32->44.
pub fn water_pipeline(
    device: &wgpu::Device,
    shader_src: &str,
    camera_layout: &wgpu::BindGroupLayout,
    texture_layout: &wgpu::BindGroupLayout,
    shadow_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    make_pipeline(
        device,
        "water",
        shader_src,
        wgpu::PrimitiveTopology::TriangleList,
        &[Some(wgpu::VertexBufferLayout {
            array_stride: 44,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32, 3 => Float32, 4 => Float32, 5 => Float32x3],
        })],
        &[
            Some(camera_layout),
            Some(texture_layout),
            Some(shadow_layout),
        ],
        format,
        samples,
    )
}

pub fn soft_border_pipeline(
    device: &wgpu::Device,
    camera_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    make_pipeline(
        device,
        "soft_borders",
        include_str!("../../../../assets/shaders/soft_border.wgsl"),
        wgpu::PrimitiveTopology::TriangleList,
        &[Some(wgpu::VertexBufferLayout {
            array_stride: 32,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32, 3 => Float32],
        })],
        &[Some(camera_layout)],
        format,
        samples,
    )
}

/// Particles of the effects system (`gpu/effects.rs`): one 80-byte instance each, a quad
/// built in the vertex shader, premultiplied blending, tested against the scene depth.
pub fn effects_pipeline(
    device: &wgpu::Device,
    shader_src: &str,
    camera_layout: &wgpu::BindGroupLayout,
    effects_layout: &wgpu::BindGroupLayout,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::RenderPipeline {
    make_pipeline(
        device,
        "effects",
        shader_src,
        wgpu::PrimitiveTopology::TriangleList,
        &[Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::gpu::effects::Particle>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x4,
                1 => Float32x4,
                2 => Float32x4,
                3 => Float32x4,
                4 => Unorm8x4,
                5 => Unorm8x4,
                6 => Float32x2
            ],
        })],
        &[Some(camera_layout), Some(effects_layout)],
        format,
        samples,
    )
}
