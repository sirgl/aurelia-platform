//! Render scale: the scene is drawn at a fraction of the target size into an intermediate
//! color texture, then a fullscreen blit upscales it into the target (before the UI pass).

use super::context::GpuCore;
use super::memory::{Category, GpuTexture};

const SHADER: &str = include_str!("../../../../assets/shaders/scale_blit.wgsl");

/// Smallest and largest accepted scale.
pub const MIN_SCALE: f32 = 0.5;
pub const MAX_SCALE: f32 = 1.0;

/// Clamps to the accepted range; NaN and unusable values mean full resolution.
pub fn clamp_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(MIN_SCALE, MAX_SCALE)
    } else {
        MAX_SCALE
    }
}

/// `AURELIA_RENDER_SCALE`, or 1.0 when unset or unparsable.
pub fn scale_from_env() -> f32 {
    std::env::var("AURELIA_RENDER_SCALE")
        .ok()
        .and_then(|v| v.parse().ok())
        .map_or(MAX_SCALE, clamp_scale)
}

/// Size of the scene pass for a target size: `round(size * scale)`, at least 1.
pub fn scaled_size(size: (u32, u32), scale: f32) -> (u32, u32) {
    if scale >= MAX_SCALE {
        return (size.0.max(1), size.1.max(1));
    }
    let one = |n: u32| ((n as f32 * scale).round() as u32).max(1);
    (one(size.0), one(size.1))
}

/// The single-sample scene color at the reduced size, with the bind group the blit reads.
pub struct ScaledColor {
    _texture: GpuTexture,
    pub view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

/// Pipeline, layout and sampler of the upscale blit.
pub struct ScaleBlit {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl ScaleBlit {
    /// `format` is the target's color format.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = super::pipelines::icons_texture_layout(device);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scene scale sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene scale blit"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene scale layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scene scale blit"),
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
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }

    /// The intermediate color texture of `size` (counted in the GPU memory ledger).
    pub fn color(
        &self,
        core: &GpuCore,
        format: wgpu::TextureFormat,
        size: (u32, u32),
    ) -> ScaledColor {
        let texture = core.texture(
            Category::RenderTargets,
            &wgpu::TextureDescriptor {
                label: Some("scaled scene color"),
                size: wgpu::Extent3d {
                    width: size.0.max(1),
                    height: size.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        let view = texture.create_view(&Default::default());
        let bind = core.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene scale bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        ScaledColor {
            _texture: texture,
            view,
            bind,
        }
    }

    /// Upscales `color` over the whole `target`.
    pub fn blit(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color: &ScaledColor,
        target: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene upscale"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
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
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &color.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_size_rounds_and_never_reaches_zero() {
        assert_eq!(scaled_size((2880, 1800), 1.0), (2880, 1800));
        assert_eq!(scaled_size((2880, 1800), 0.5), (1440, 900));
        assert_eq!(scaled_size((2880, 1800), 0.667), (1921, 1201));
        assert_eq!(scaled_size((1, 1), 0.5), (1, 1));
        assert_eq!(scaled_size((0, 0), 0.5), (1, 1));
    }

    #[test]
    fn scale_is_clamped() {
        assert_eq!(clamp_scale(0.1), 0.5);
        assert_eq!(clamp_scale(3.0), 1.0);
        assert_eq!(clamp_scale(f32::NAN), 1.0);
    }
}
