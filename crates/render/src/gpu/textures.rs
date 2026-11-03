//! Terrain texture array (§30 platform/structure.md: gpu/textures.rs).
//!
//! Layers 0..=4 are procedural low-poly masses from `ground`,
//! layer WATER_LAYER is procedural (flat + deterministic noise against banding
//! in mips). Mips are a CPU box filter (no image dep: clean code + tests),
//! rows are padded to 256B for write_texture. Sampler: Repeat + trilinear +
//! anisotropy by the adapter limit.

use crate::gpu::context::GpuCore;

/// The procedural water layer (matches style::WATER_LAYER; a test checks it).
pub const WATER_LAYER: u32 = 6;
/// Number of layers: 5 photos + water.
pub const LAYER_COUNT: u32 = 7;

pub struct TerrainTextures {
    pub texture: crate::gpu::memory::GpuTexture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub layers: u32,
    pub mip_levels: u32,
    pub base_size: u32,
}

/// Yield icon atlas (2D, no mips since it is small; Clamp + Linear).
/// Pixels come from build_yield_atlas (deterministic, no assets).
pub struct IconTextures {
    pub texture: crate::gpu::memory::GpuTexture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

pub fn build_icons(core: &GpuCore, w: u32, h: u32, rgba: &[u8]) -> Result<IconTextures, String> {
    if rgba.len() != (w * h * 4) as usize {
        return Err(format!("icon atlas byte size mismatch: {}x{}", w, h));
    }
    let texture = core.texture(crate::gpu::memory::Category::Materials, &wgpu::TextureDescriptor {
        label: Some("yield icons"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let (padded, stride) = pad_rows(rgba, w, h);
    core.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &padded,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(stride),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = core.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("icons sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        // 16px pixel art: Nearest keeps sharpness (Linear smears into mush).
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    Ok(IconTextures {
        texture,
        view,
        sampler,
    })
}

/// Box downscale 2x (even edges; odd ones clamp the last pixel).
/// A pure function, tested without GPU. sRGB bytes are averaged as they are
/// (physically inexact, visually indistinguishable for an albedo mip; documented).
pub fn box_downscale_mip(prev: &[u8], w: u32, h: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            for c in 0..4 {
                let mut acc = 0u32;
                for dy in 0..2 {
                    for dx in 0..2 {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        acc += prev[((sy * w + sx) * 4 + c) as usize] as u32;
                    }
                }
                out[((y * nw + x) * 4 + c) as usize] = (acc / 4) as u8;
            }
        }
    }
    (out, nw, nh)
}

/// Row padding to 256B for write_texture. Returns (buffer, stride).
pub fn pad_rows(rgba: &[u8], w: u32, h: u32) -> (Vec<u8>, u32) {
    let row_bytes = w * 4;
    let stride = row_bytes.div_ceil(256) * 256;
    if stride == row_bytes {
        return (rgba.to_vec(), stride);
    }
    let mut out = vec![0u8; (stride * h) as usize];
    for y in 0..h {
        let src = &rgba[(y * row_bytes) as usize..((y + 1) * row_bytes) as usize];
        let dst = &mut out[(y * stride) as usize..(y * stride + row_bytes) as usize];
        dst.copy_from_slice(src);
    }
    (out, stride)
}

/// Procedural water: flat + deterministic noise (+-3 LSB by an index hash).
pub fn make_water_layer(size: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for i in 0..size * size {
        // xorshift from the index: deterministic, no RNG state.
        let mut x = (i as u64).wrapping_add(0x9e3779b97f4a7c15).max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let n = ((x % 7) as i16) - 3; // -3..=3
        out.push((38i16 + n).max(0) as u8);
        out.push((89i16 + n).max(0) as u8);
        out.push((158i16 + n).max(0) as u8);
        out.push(255);
    }
    out
}

fn mip_count(size: u32) -> u32 {
    let mut levels = 1;
    let mut s = size;
    while s > 1 {
        s /= 2;
        levels += 1;
    }
    levels
}

/// Assemble the array texture: photo layers + water, mips, sampler.
/// layers: (width, height, rgba); the sizes must match (we check).
pub fn build_array(
    core: &GpuCore,
    layers: &[(u32, u32, Vec<u8>)],
) -> Result<TerrainTextures, String> {
    if layers.is_empty() {
        return Err("no texture layers".into());
    }
    let (w0, h0, _) = &layers[0];
    let (w0, h0) = (*w0, *h0);
    if w0 != h0 || w0 < 64 {
        return Err(format!(
            "base layer must be square >= 64, got {}x{}",
            w0, h0
        ));
    }
    for (i, (w, h, pixels)) in layers.iter().enumerate() {
        if (*w, *h) != (w0, h0) {
            return Err(format!(
                "layer {} size {:?} != base {:?}",
                i,
                (w, h),
                (w0, h0)
            ));
        }
        if pixels.len() != (w * h * 4) as usize {
            return Err(format!("layer {} byte size mismatch", i));
        }
    }
    let levels = mip_count(w0);
    // The water layer of the same size (mips as for all).
    let water = make_water_layer(w0);
    let total_layers = layers.len() as u32 + 1;
    assert_eq!(total_layers, LAYER_COUNT, "land layers + water");

    let texture = core.texture(crate::gpu::memory::Category::Materials, &wgpu::TextureDescriptor {
        label: Some("terrain array"),
        size: wgpu::Extent3d {
            width: w0,
            height: h0,
            depth_or_array_layers: total_layers,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Rgba8Unorm, NOT Srgb: JPEG bytes are treated as display space,
        // like the earlier flat colours (consistent with the whole pipeline).
        // An honest linear workflow (sRGB decode + encode on output) is
        // separate, with a gamma test.
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    // Level 0 of all layers + water.
    let mut level_data: Vec<(u32, u32, Vec<u8>)> =
        layers.iter().map(|(w, h, p)| (*w, *h, p.clone())).collect();
    level_data.push((w0, h0, water));
    upload_level(core, &texture, &level_data, 0);

    // Mips: a box filter along the chain of each layer.
    let mut cur = level_data;
    for level in 1..levels {
        let mut next = Vec::with_capacity(cur.len());
        for (w, h, pixels) in &cur {
            let (down, nw, nh) = box_downscale_mip(pixels, *w, *h);
            next.push((nw, nh, down));
        }
        upload_level(core, &texture, &next, level);
        cur = next;
    }

    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        label: Some("terrain array view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = core.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("terrain sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        address_mode_w: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        // Fix 4: there is no anisotropy limit in Limits v30, 16 is the spec max.
        // Higher after checking on hardware.
        anisotropy_clamp: 4,
        ..Default::default()
    });
    Ok(TerrainTextures {
        texture,
        view,
        sampler,
        layers: total_layers,
        mip_levels: levels,
        base_size: w0,
    })
}

fn upload_level(
    core: &GpuCore,
    texture: &wgpu::Texture,
    layers: &[(u32, u32, Vec<u8>)],
    level: u32,
) {
    for (i, (w, h, pixels)) in layers.iter().enumerate() {
        let (padded, stride) = pad_rows(pixels, *w, *h);
        core.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: level,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: i as u32,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &padded,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(*h),
            },
            wgpu::Extent3d {
                width: *w,
                height: *h,
                depth_or_array_layers: 1,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn striped(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let c = if (x + y) % 2 == 0 { 200 } else { 50 };
                v.extend_from_slice(&[c, c, c, 255]);
            }
        }
        v
    }

    #[test]
    fn box_mip_halves_and_averages() {
        let (down, nw, nh) = box_downscale_mip(&striped(8, 8), 8, 8);
        assert_eq!((nw, nh), (4, 4));
        // A 200/50 checkerboard averages to ~125.
        assert!(down.chunks(4).all(|px| (px[0] as i16 - 125).abs() <= 1));
        // Odd size: clamp the edge.
        let (down, nw, nh) = box_downscale_mip(&striped(7, 5), 7, 5);
        assert_eq!((nw, nh), (3, 2));
        assert_eq!(down.len(), 3 * 2 * 4);
    }

    #[test]
    fn pad_rows_aligns_to_256() {
        // 1254×4 = 5016 → stride 5120.
        let src = vec![7u8; 5016 * 2];
        let (padded, stride) = pad_rows(&src, 1254, 2);
        assert_eq!(stride, 5120);
        assert_eq!(padded.len(), 5120 * 2);
        assert_eq!(&padded[..5016], &src[..5016]);
        assert!(padded[5016..5120].iter().all(|b| *b == 0));
        // Already aligned: a copy unchanged.
        let src = vec![1u8; 1024 * 4];
        let (same, stride) = pad_rows(&src, 1024, 1);
        assert_eq!(stride, 4096);
        assert_eq!(same, src);
    }

    #[test]
    fn water_layer_is_stable_and_blue() {
        let (a, b) = (make_water_layer(64), make_water_layer(64));
        assert_eq!(a, b);
        assert!(a.chunks(4).all(|px| px[2] > px[0] + 60 && px[3] == 255));
    }

    #[test]
    fn loader_palette_matches_style_colors() {
        // Palette sync: the layer average ~ style color (both bytes<->float).
        // Layer order is TEXTURE_LAYERS (snow is layer 4, terrain 5;
        // mountain is layer 5, terrain 6; water is procedural, layer 6).
        // Change only as a pair (ground.rs PALETTE + default_style).
        use crate::scene::style::{compile, default_style, TEXTURE_LAYERS};
        let style = compile(default_style()).unwrap();
        let ids = [0u16, 1, 2, 3, 5, 6];
        let set = crate::ground::layers();
        assert_eq!(set.len(), TEXTURE_LAYERS.len());
        for (i, (width, height, rgba)) in set.iter().enumerate() {
            assert_eq!(crate::ground::PALETTE[i].0, TEXTURE_LAYERS[i], "layer order");
            let n = (width * height) as usize;
            let mut sum = [0u64; 3];
            for p in 0..n {
                sum[0] += rgba[p * 4] as u64;
                sum[1] += rgba[p * 4 + 1] as u64;
                sum[2] += rgba[p * 4 + 2] as u64;
            }
            let sc = style.color(ids[i]).unwrap();
            for c in 0..3 {
                let mean = sum[c] as f64 / n as f64;
                let expect = sc[c] as f64 * 255.0;
                assert!(
                    (mean - expect).abs() < 8.0,
                    "layer {i} channel {c}: mean {mean:.1} vs style {expect:.1}"
                );
            }
        }
    }
}
