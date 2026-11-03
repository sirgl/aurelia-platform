//! The app shell's frame offscreen, as `desktop --screenshot` runs it. Tests build with
//! debug assertions, so egui's check that every texture change of a frame is applied
//! (`TexturesDelta`'s drop) runs here: the shell once dropped them and a debug build
//! panicked on its first frame.

use client::shell::app::App;
use client::shell::capture::{capture_texture, wait_idle};
use sandbox_rules::game::Setup;
use sandbox_ui::{SandboxUi, MENU_SEED};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: (u32, u32) = (320, 180);

fn raw_input(app: &App<SandboxUi>, frame: u32) -> egui::RawInput {
    let screen = egui::vec2(SIZE.0 as f32, SIZE.1 as f32) / app.ctx.pixels_per_point();
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
        time: Some(f64::from(frame) / 60.0),
        focused: true,
        ..Default::default()
    }
}

#[test]
fn frames_apply_every_texture_change() {
    let core = match client::shell::gpu::open() {
        Ok(core) => core,
        Err(error) => return eprintln!("skipped: no GPU ({error})"),
    };
    let texture = capture_texture(&core, SIZE.0, SIZE.1, FORMAT);
    let target = texture.create_view(&Default::default());
    let mut app = App::new(&core, FORMAT, None, SandboxUi::game(), &SandboxUi::terrain_layers());
    app.screens.settings.autosave_every = 0;
    // The menu's first frame uploads the font atlas.
    app.frame(&core, &target, SIZE, raw_input(&app, 0));
    app.module.start(Setup::standard(MENU_SEED), 1);
    app.apply_module_actions();
    for frame in 1..4 {
        app.frame(&core, &target, SIZE, raw_input(&app, frame));
        wait_idle(&core);
    }
    // The UI-only frame of `--bench-ui` paints nothing and drops its changes on purpose.
    let (_, vertices) = app.ui_frame(raw_input(&app, 4), SIZE);
    assert!(vertices > 0, "the map frame draws the HUD");
}
