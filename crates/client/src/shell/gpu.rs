//! Opening the GPU without panicking: the preferred adapter with the
//! renderer's limits, then the adapter's own limits, then a fallback
//! (software) adapter. Every failure becomes a sentence for the player.
use render::gpu::context::GpuCore;

/// The GPU the game draws with, or why there is none.
#[cfg(not(target_arch = "wasm32"))]
pub fn open() -> Result<GpuCore, String> {
    pollster::block_on(open_async())
}

/// A page cannot wait for the GPU; its entry point awaits `open_async`.
#[cfg(target_arch = "wasm32")]
pub fn open() -> Result<GpuCore, String> {
    Err("a browser opens the GPU with open_async".into())
}

/// `open` for a browser, where nothing may block: the same ladder, awaited.
pub async fn open_async() -> Result<GpuCore, String> {
    open_on(wgpu::Instance::default(), None).await
}

/// The same ladder on an instance the caller made. WebGL needs the surface before the
/// adapter (the context belongs to the canvas), so a page passes its canvas surface.
pub async fn open_on(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<GpuCore, String> {
    let mut reasons = Vec::new();
    for fallback in [false, true] {
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface,
            force_fallback_adapter: fallback,
            apply_limit_buckets: false,
        };
        let adapter = match instance.request_adapter(&options).await {
            Ok(adapter) => adapter,
            Err(error) => {
                reasons.push(format!("no {}adapter ({error})", if fallback { "fallback " } else { "" }));
                continue;
            }
        };
        match device(&adapter).await {
            Ok((device, queue)) => return Ok(GpuCore::from_parts(instance, adapter, device, queue)),
            Err(error) => reasons.push(error),
        }
    }
    Err(format!(
        "Aurelia needs a graphics card with Metal, Vulkan or DirectX 12 support. {}.",
        reasons.join("; ")
    ))
}

/// A device with the default limits, else with what the adapter offers.
async fn device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), String> {
    // AURELIA_GPU_TIMING=1: pass timers for the benches (`render::gpu::timing`).
    let timed = std::env::var_os("AURELIA_GPU_TIMING").is_some();
    let features = if timed { adapter.features() & wgpu::Features::TIMESTAMP_QUERY } else { wgpu::Features::empty() };
    let preferred = adapter.request_device(&wgpu::DeviceDescriptor { required_features: features, ..Default::default() }).await;
    if let Ok(pair) = preferred {
        return Ok(pair);
    }
    // WebGL2 is the floor of a browser fallback; its limits are lower than the desktop's.
    let floor = if adapter.get_info().backend == wgpu::Backend::Gl {
        wgpu::Limits::downlevel_webgl2_defaults()
    } else {
        wgpu::Limits::downlevel_defaults()
    };
    let modest = wgpu::DeviceDescriptor {
        required_limits: floor.using_resolution(adapter.limits()),
        ..Default::default()
    };
    adapter.request_device(&modest).await
        .map_err(|e| format!("{} cannot open a device ({e})", adapter.get_info().name))
}

/// The surface format to present with: sRGB when offered, else the first.
pub fn surface_format(caps: &wgpu::SurfaceCapabilities) -> Result<wgpu::TextureFormat, String> {
    let first = caps.formats.first().copied().ok_or("the window cannot be drawn to (no surface format)")?;
    Ok(caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(first))
}

/// The format the world pass draws in on a surface of `surface` format: the sRGB twin of a
/// plain one. The world shaders write linear colour and rely on the target to encode it; a
/// browser's WebGPU canvas offers only `Rgba8Unorm` / `Bgra8Unorm`, so the page configures it with
/// `view_formats` ([`view_formats`]) and draws the world through a view of this format (the
/// egui pass keeps the plain view: it writes the display values itself). `surface` itself when
/// it is sRGB already (every desktop surface) or has no sRGB twin.
pub fn world_format(surface: wgpu::TextureFormat) -> wgpu::TextureFormat {
    surface.add_srgb_suffix()
}

/// The `view_formats` a surface of `surface` format is configured with: the world's view format,
/// when it is another than the surface's.
pub fn view_formats(surface: wgpu::TextureFormat) -> Vec<wgpu::TextureFormat> {
    let world = world_format(surface);
    if world == surface { Vec::new() } else { vec![world] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_without_formats_is_an_error_not_a_panic() {
        let caps = wgpu::SurfaceCapabilities::default();
        assert!(surface_format(&caps).is_err());
        let caps = wgpu::SurfaceCapabilities {
            formats: vec![wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Bgra8UnormSrgb],
            ..Default::default()
        };
        assert_eq!(surface_format(&caps), Ok(wgpu::TextureFormat::Bgra8UnormSrgb));
    }

    #[test]
    fn a_plain_surface_is_drawn_through_its_srgb_twin() {
        use wgpu::TextureFormat as F;
        assert_eq!(world_format(F::Bgra8Unorm), F::Bgra8UnormSrgb);
        assert_eq!(view_formats(F::Rgba8Unorm), vec![F::Rgba8UnormSrgb]);
        // An sRGB surface (desktop, WebGL2) and a format without a twin are drawn as they are.
        assert_eq!(world_format(F::Bgra8UnormSrgb), F::Bgra8UnormSrgb);
        assert!(view_formats(F::Bgra8UnormSrgb).is_empty());
        assert!(view_formats(F::Rgba16Float).is_empty());
    }
}
