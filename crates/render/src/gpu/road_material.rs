//! Revision-driven R8 road coverage. No draw call or fragment path search.
use super::{context::GpuCore, visual_river};
use crate::{
    scene::{layout::tile_to_world, terrain::surface::SurfaceMesh},
    world::state::RenderWorld,
};

const SIZE: u32 = 512;
type Quad = [[f32; 2]; 4];
#[derive(Clone, PartialEq)]
struct MaskInput {
    bounds: [f32; 4],
    quads: Vec<Quad>,
}
#[derive(Default)]
struct MaskCache {
    input: Option<MaskInput>,
}
impl MaskCache {
    fn update(&mut self, input: MaskInput) -> Option<Vec<u8>> {
        if self.input.as_ref() == Some(&input) {
            return None;
        }
        let pixels = rasterize(&input);
        self.input = Some(input);
        Some(pixels)
    }
}
pub struct RoadMaterial {
    pub layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
    texture: crate::gpu::memory::GpuTexture,
    bounds: crate::gpu::memory::GpuBuffer,
    cache: MaskCache,
    enabled: bool,
}
impl RoadMaterial {
    pub fn new(core: &crate::gpu::context::GpuCore) -> Self {
        let device = &core.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("road mask layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let texture = core.texture(crate::gpu::memory::Category::Materials, &wgpu::TextureDescriptor {
            label: Some("road coverage R8"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("road coverage sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let bounds = core.buffer(crate::gpu::memory::Category::Uniforms, &wgpu::BufferDescriptor {
            label: Some("road world UV bounds"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("road coverage bind"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: bounds.as_entire_binding(),
                },
            ],
        });
        Self {
            layout,
            bind_group,
            texture,
            bounds,
            cache: MaskCache::default(),
            enabled: visual_river::shader_roads(),
        }
    }
    /// Called only on a world revision. Identical footprint/bounds performs no upload.
    pub fn update(
        &mut self,
        core: &GpuCore,
        world: &RenderWorld,
        surface: Option<&SurfaceMesh>,
    ) -> u64 {
        let (x, z) = tile_to_world(
            world.width.saturating_sub(1),
            world.height.saturating_sub(1),
        );
        let bounds = [-1.1, -1.1, 1.0 / (x + 2.2), 1.0 / (z + 2.2)];
        let quads = if self.enabled && world.visual_surface {
            surface.map_or_else(Vec::new, |s| visual_river::road_quads(world, s))
        } else {
            vec![]
        };
        let Some(pixels) = self.cache.update(MaskInput { bounds, quads }) else {
            return 0;
        };
        core.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE),
                rows_per_image: Some(SIZE),
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        core.queue
            .write_buffer(&self.bounds, 0, bytemuck::bytes_of(&bounds));
        pixels.len() as u64 + 16
    }
}
fn in_triangle(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let edge =
        |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let d = [edge(a, b), edge(b, c), edge(c, a)];
    d.iter().all(|v| *v >= -1e-6) || d.iter().all(|v| *v <= 1e-6)
}
fn rasterize(input: &MaskInput) -> Vec<u8> {
    // Union subpixel coverage rather than max alpha: shared quad edges have no seam.
    let mut coverage = vec![0u16; (SIZE * SIZE) as usize];
    for quad in &input.quads {
        let q = quad.map(|p| {
            [
                (p[0] - input.bounds[0]) * input.bounds[2] * SIZE as f32,
                (p[1] - input.bounds[1]) * input.bounds[3] * SIZE as f32,
            ]
        });
        let min_x = q
            .iter()
            .map(|p| p[0])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let max_x = q
            .iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(SIZE as f32) as u32;
        let min_y = q
            .iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as u32;
        let max_y = q
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(SIZE as f32) as u32;
        for y in min_y..max_y {
            for x in min_x..max_x {
                let bits = &mut coverage[(y * SIZE + x) as usize];
                for sy in 0..4 {
                    for sx in 0..4 {
                        let p = [
                            x as f32 + (sx as f32 + 0.5) * 0.25,
                            y as f32 + (sy as f32 + 0.5) * 0.25,
                        ];
                        if in_triangle(p, q[0], q[1], q[2]) || in_triangle(p, q[0], q[2], q[3]) {
                            *bits |= 1 << (sy * 4 + sx);
                        }
                    }
                }
            }
        }
    }
    coverage
        .into_iter()
        .map(|bits| ((bits.count_ones() * 255 + 8) / 16) as u8)
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> MaskInput {
        MaskInput {
            bounds: [0.0, 0.0, 1.0, 1.0],
            quads: vec![
                [[0.25, 0.4], [0.5, 0.4], [0.5, 0.6], [0.25, 0.6]],
                [[0.5, 0.4], [0.75, 0.4], [0.75, 0.6], [0.5, 0.6]],
            ],
        }
    }
    #[test]
    fn mask_preserves_width_join_and_empty_background() {
        let mask = rasterize(&input());
        assert_eq!(
            mask[(256 * SIZE + 256) as usize],
            255,
            "shared join has no transparent seam"
        );
        assert_eq!(mask[(256 * SIZE + 150) as usize], 255);
        assert_eq!(mask[(180 * SIZE + 150) as usize], 0);
        assert_eq!(mask[(256 * SIZE + 390) as usize], 0);
        let area = mask.iter().map(|v| *v as f64 / 255.0).sum::<f64>();
        assert!(
            (area - 0.1 * (SIZE * SIZE) as f64).abs() < SIZE as f64 * 0.25,
            "road footprint area {area}"
        );
    }
    #[test]
    fn unchanged_footprint_skips_upload_but_removed_road_clears_mask() {
        let mut cache = MaskCache::default();
        assert!(cache.update(input()).is_some());
        assert!(cache.update(input()).is_none());
        let mut changed = input();
        changed.quads.clear();
        let cleared = cache.update(changed.clone()).unwrap();
        assert!(cleared.iter().all(|v| *v == 0));
        assert!(cache.update(changed).is_none());
    }
}
