//! Offscreen capture of a sandbox match through the real frame of the app
//! shell: `AURELIA_RULES=sandbox desktop --screenshot <path>`. No window opens.
//!
//! Every player, the observed one included, is played by the AI for
//! AURELIA_GAME_TURNS rounds (default 40) of the game on AURELIA_GAME_SEED (default
//! `MENU_SEED`); the map is then captured around the observer's first city
//! at AURELIA_VISUAL_SIZE (WxH, default 1664x936); AURELIA_GAME_SCREEN=city shows that
//! city's screen instead.

use client::shell::app::App;
use client::shell::capture::{capture_texture, save_capture, wait_idle};
use client::shell::module::ModuleUi;
use render::gpu::terrain_gpu::TerrainSync;
use sandbox_rules::game::Setup;

use crate::{SandboxUi, MENU_SEED};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub fn screenshot(path: &str) {
    let core = match client::shell::gpu::open() {
        Ok(core) => core,
        Err(error) => {
            eprintln!("offscreen run impossible: {error}");
            std::process::exit(2);
        }
    };
    let (width, height) = env_size();
    let texture = capture_texture(&core, width, height, FORMAT);
    let target = texture.create_view(&Default::default());
    let mut app = App::new(&core, FORMAT, None, SandboxUi::game(), &SandboxUi::terrain_layers());
    app.screens.settings.autosave_every = 0;
    app.fixed_animation_time = Some(1.0);
    app.renderer.set_terrain_sync(TerrainSync::Immediate);

    app.module.start(Setup::standard(env_number("AURELIA_GAME_SEED", MENU_SEED)), 1);
    app.apply_module_actions();
    let session = app.module.session.as_mut().expect("the match started");
    session.set_humans(0);
    for _ in 0..env_number("AURELIA_GAME_TURNS", 40) {
        session.play_round();
    }
    // The observer plays by hand again: the frames below advance no turn.
    session.set_humans(1);
    session.client.refresh_projection();
    let auth = session.client.game.view().auth;
    let observer = session.client.observer;
    let first_city = auth.indexes().cities_of(observer).first().and_then(|c| auth.cities().get(*c)).map(|c| c.tile.0);
    let first_unit = auth.indexes().units_of(observer).first().and_then(|u| auth.units().get(*u)).map(|u| u.core.tile.0);
    let round = session.round();
    let hash = kernel::replay::hash::authoritative_hash(&session.client.game);
    if let Some(tile) = first_city.or(first_unit) {
        app.center_on(tile);
    }
    if let (Ok("city"), Some(tile)) = (std::env::var("AURELIA_GAME_SCREEN").as_deref(), first_city) {
        app.module.select_city(tile);
        app.apply_module_actions();
    }
    app.camera.zoom = 1.6;
    for frame in 0..8u32 {
        let screen = egui::vec2(width as f32, height as f32) / app.ctx.pixels_per_point();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            time: Some(f64::from(frame) / 60.0),
            focused: true,
            ..Default::default()
        };
        app.frame(&core, &target, (width, height), raw);
        wait_idle(&core);
    }
    println!("sandbox round {round}  hash {hash:#018x}");
    save_capture(&core, &texture, width, height, path);
}

fn env_number(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn env_size() -> (u32, u32) {
    let value = std::env::var("AURELIA_VISUAL_SIZE").unwrap_or_default();
    match value.split_once('x').map(|(w, h)| (w.parse(), h.parse())) {
        Some((Ok(w), Ok(h))) => (w, h),
        _ => (1664, 936),
    }
}
