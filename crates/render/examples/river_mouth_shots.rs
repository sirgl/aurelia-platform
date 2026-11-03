//! Offscreen frames of the river junction fixtures (`surface::mouths`): the same worlds the
//! geometry tests build, seen by the real renderer (water shader, terrain shader, shadows).
//!
//!   AURELIA_CAMERA_ELEVATION=45 cargo run --release --offline -p aurelia-render --example river_mouth_shots -- <out dir> <prefix> [case ...]
//!
//! Writes <out dir>/<prefix>-<case>.png (1280x720). AURELIA_CAMERA_ELEVATION is the eye's elevation in
//! degrees above the horizon (the game's is 45).
use render::camera::CameraController;
use render::gpu::context::GpuCore;
use render::gpu::terrain_gpu::TerrainSync;
use render::renderer::{FrameFlags, Renderer};
use render::scene::layout::tile_to_world;
use render::scene::terrain::surface::mouths;
use render::RenderWorld;

const SIZE: (u32, u32) = (1280, 720);

/// A case: its name, the world and the (q, r) of the camera's centre, zoom.
fn cases() -> Vec<(&'static str, RenderWorld, (f32, f32), f32)> {
    let at = |q: f32, r: f32| tile_to_world(q as u32, r as u32);
    let mid = |q: f32, r: f32| {
        let (x, z) = at(q, r);
        (x + if (r as u32) % 2 == 1 { 0.0 } else { 0.0 }, z)
    };
    vec![
        ("sea", mouths::beach_mouth(), mid(53., 18.), 4.5),
        ("cliff", mouths::cliff_mouth(), mid(10., 11.), 4.5),
        ("stream", mouths::stream_mouth(), mid(8., 17.), 4.5),
        ("lake", mouths::lake_mouth(), mid(53., 18.), 4.5),
        ("source", mouths::source_window(), mid(46., 20.), 6.0),
        ("meander", mouths::plain_river(), mid(50., 18.), 3.5),
        ("slope", mouths::slope_river(), mid(49., 18.), 3.5),
        ("confluence", mouths::confluence(), mid(47., 21.), 4.0),
    ]
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "/tmp".into());
    let prefix = std::env::args().nth(2).unwrap_or_else(|| "frame".into());
    let wanted: Vec<String> = std::env::args().skip(3).collect();
    // AURELIA_CAMERA_ELEVATION: degrees above the horizon (the game's is 45).
    if let Some(degrees) = std::env::var("AURELIA_CAMERA_ELEVATION").ok().and_then(|v| v.parse::<f32>().ok()) {
        render::camera::controller::set_pitch_override(Some(degrees));
    }
    let core = GpuCore::new_blocking().expect("GPU adapter");
    let layers = render::ground::layers();
    let mut renderer = Renderer::with_gpu(&core, wgpu::TextureFormat::Rgba8Unorm, &layers);
    renderer.set_terrain_sync(TerrainSync::Immediate);
    renderer.set_animation_time_override(Some(1.0));
    let flags = FrameFlags { grid: false, selected_city: None, yields: false };
    // AURELIA_SHOT_AT="q,r,zoom" moves every case's camera to that tile (fractions allowed).
    let at = std::env::var("AURELIA_SHOT_AT").ok().and_then(|v| {
        let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (n.len() == 3).then(|| (n[0], n[1], n[2]))
    });
    let scale = std::env::var("AURELIA_SHOT_ZOOM_SCALE").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
    for (name, mut world, (x, z), zoom) in cases() {
        let (x, z, zoom) = match at {
            Some((q, r, zoom)) => {
                let (x, z) = render::scene::layout::tile_to_world(q.floor() as u32, r.floor() as u32);
                (x + (q - q.floor()) * 1.7320508, z + (r - r.floor()) * 1.5, zoom)
            }
            None => (x, z, zoom * scale),
        };
        if !wanted.is_empty() && !wanted.iter().any(|w| w == name) {
            continue;
        }
        world.visual_surface = true;
        world.layers = render::world::state::LayerRevisions::fresh();
        let camera = CameraController { x, y: z, zoom };
        for _ in 0..3 {
            renderer.render_offscreen(&core, &world, &camera, SIZE.0, SIZE.1, flags);
        }
        let pixels = renderer.render_offscreen(&core, &world, &camera, SIZE.0, SIZE.1, flags);
        let path = format!("{out}/{prefix}-{name}.png");
        image::save_buffer(&path, &pixels, SIZE.0, SIZE.1, image::ColorType::Rgba8).expect("write png");
        let stats = renderer.stats();
        println!("{path}: terrain {} tris, {} draws", stats.terrain_tris, stats.draw_calls);
    }
}
