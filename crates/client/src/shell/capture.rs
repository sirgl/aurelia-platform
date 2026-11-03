//! Offscreen PNG readback for `--screenshot` captures of both binaries.
use render::gpu::context::GpuCore;
use render::gpu::memory::{Category, GpuTexture};
use std::time::Duration;

/// Render target that can be copied back to the CPU. AURELIA_GAME_PITCH=<degrees> (captures only)
/// looks at the map from that angle above the horizon instead of the game's 45.
pub fn capture_texture(
    core: &GpuCore,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
) -> GpuTexture {
    if let Some(degrees) = std::env::var("AURELIA_GAME_PITCH").ok().and_then(|p| p.parse::<f32>().ok()) {
        render::camera::controller::set_pitch_override(Some(degrees));
    }
    core.texture(Category::RenderTargets, &wgpu::TextureDescriptor {
        label: Some("capture"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The density of the screen a capture pretends to have (`AURELIA_VISUAL_NATIVE_SCALE`, 1 when
/// unset): the shell lays out for `size / scale` points and paints at `scale` pixels per point.
pub fn simulated_native_scale() -> f32 {
    std::env::var("AURELIA_VISUAL_NATIVE_SCALE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|s| s.is_finite() && (0.5..=6.0).contains(s))
        .unwrap_or(1.0)
}

/// Block until the GPU finished all submitted work (honest frame timing offscreen).
pub fn wait_idle(core: &GpuCore) {
    core.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })
        .unwrap();
}

/// The share of the window the HUD covered in the last noted frame, in tenths of a
/// percent (`note_coverage`); `u32::MAX`: none noted.
static COVERAGE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

/// Remember how much of the window the HUD covers in the frame about to be saved
/// (`layout::covered_share`); `save_capture` prints it beside the file's name.
pub fn note_coverage(share: f32) {
    COVERAGE.store((share * 1000.).round() as u32, std::sync::atomic::Ordering::Relaxed);
}

pub fn save_capture(core: &GpuCore, tex: &wgpu::Texture, w: u32, h: u32, path: &str) {
    let pixels = capture_pixels(core, tex, w, h);
    image::save_buffer(path, &pixels, w, h, image::ColorType::Rgba8).unwrap();
    println!("Saved {path}");
    let noted = COVERAGE.load(std::sync::atomic::Ordering::Relaxed);
    if noted != u32::MAX {
        println!("hud coverage {path}: {:.1} %", noted as f32 / 10.);
    }
}

/// The texture's pixels as RGBA bytes, row after row (tests compare frames with it).
pub fn capture_pixels(core: &GpuCore, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
    let pitch = (w * 4).div_ceil(256) * 256;
    let buf = core.buffer(Category::Readback, &wgpu::BufferDescriptor {
        label: None,
        size: (pitch * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = core.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    core.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    core.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(30)),
        })
        .unwrap();
    rx.recv_timeout(Duration::from_secs(30)).unwrap().unwrap();
    let data = slice.get_mapped_range().unwrap();
    let pixels = data
        .chunks(pitch as usize)
        .flat_map(|r| r[..(w * 4) as usize].iter().copied())
        .collect::<Vec<_>>();
    pixels
}
