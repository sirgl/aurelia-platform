//! The menu's "Save game" asks for a name and "Load game" lists the saves, through the app's
//! real frames and keys: a named save, the list newest first, an unreadable file as a row, a
//! load by Enter, a delete after its confirmation.

use client::shell::app::App;
use client::shell::capture::{capture_texture, wait_idle};
use client::shell::module::{ModuleUi, Screen, ShellAction};
use client::shell::saves::{ModuleSaves, SaveKind};
use sandbox_rules::game::Setup;
use sandbox_ui::{SandboxUi, MENU_SEED};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: (u32, u32) = (640, 400);

#[test]
fn the_menu_names_a_save_lists_it_loads_it_and_deletes_it() {
    let core = match client::shell::gpu::open() {
        Ok(core) => core,
        Err(error) => return eprintln!("skipped: no GPU ({error})"),
    };
    let dir = std::env::temp_dir().join(format!("aurelia-saves-list-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // The only test of this binary: the save directory is the process's.
    std::env::set_var("AURELIA_SAVE_DIR", &dir);
    let texture = capture_texture(&core, SIZE.0, SIZE.1, FORMAT);
    let target = texture.create_view(&Default::default());
    let mut app = App::new(&core, FORMAT, None, SandboxUi::game(), &SandboxUi::terrain_layers());
    app.screens.settings.autosave_every = 0;
    app.module.start(Setup::standard(MENU_SEED), 1);
    app.apply_module_actions();
    let saves = ModuleSaves::under(&dir, app.module.module_id());

    let mut frame = 0;
    let mut draw = |app: &mut App<SandboxUi>| {
        let screen = egui::vec2(SIZE.0 as f32, SIZE.1 as f32) / app.ctx.pixels_per_point();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
            time: Some(f64::from(frame) / 60.0),
            focused: true,
            ..Default::default()
        };
        app.frame(&core, &target, SIZE, raw);
        wait_idle(&core);
        frame += 1;
    };
    draw(&mut app);

    // Save game: the dialog opens over the menu with "Turn N", a typed name is saved.
    app.screens.open(Screen::Menu);
    app.apply(ShellAction::SaveDialog);
    draw(&mut app);
    let turn = app.module.turn();
    assert_eq!(app.screens.saves.name(), Some(format!("Turn {turn}").as_str()), "the module names no nation");
    app.screens.saves.set_name("First game");
    app.key(egui::Key::Enter);
    assert!(!app.screens.saves.is_open(), "the dialog closes with the save");
    assert!(saves.manual_slot("First game").unwrap().exists());
    assert!(app.screens.menu_status.as_deref().is_some_and(|s| s.contains(&turn.to_string())), "{:?}", app.screens.menu_status);
    std::thread::sleep(std::time::Duration::from_millis(30));
    app.apply(ShellAction::SaveDialog);
    app.screens.saves.set_name("Second game");
    app.key(egui::Key::Enter);
    // Escape leaves the naming dialog without a save.
    app.apply(ShellAction::SaveDialog);
    app.key(egui::Key::Escape);
    assert!(!app.screens.saves.is_open());
    assert_eq!(saves.entries().len(), 2);
    // A file that is no save is a row too.
    std::fs::write(saves.manual_slot("Broken one").unwrap().path(), b"no tag at all").unwrap();

    // Load game: newest first, the broken one named unreadable.
    app.apply(ShellAction::LoadDialog);
    for _ in 0..3 {
        draw(&mut app);
    }
    let rows = app.screens.saves.rows();
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.entry.kind == SaveKind::Manual && r.header.is_some()), "the frames read every header");
    let broken = rows.iter().position(|r| r.entry.name == "Broken one").unwrap();
    assert!(rows[broken].header.as_ref().unwrap().is_err());
    let second = rows.iter().position(|r| r.entry.name == "Second game").unwrap();
    let first = rows.iter().position(|r| r.entry.name == "First game").unwrap();
    assert!(second < first, "newest first");
    app.screens.saves.select(first);

    // Enter loads the selected save and closes the list.
    app.key(egui::Key::Enter);
    assert!(!app.screens.saves.is_open(), "the load closed the list");
    assert_eq!(app.screens.screen, Screen::Map);
    draw(&mut app);

    // A broken save cannot be loaded: the list stays, Delete asks, Enter confirms.
    app.screens.open(Screen::Menu);
    app.apply(ShellAction::LoadDialog);
    app.screens.saves.select(broken);
    app.key(egui::Key::Enter);
    assert!(app.screens.saves.is_open(), "an unreadable save is not loaded");
    app.key(egui::Key::Delete);
    draw(&mut app);
    assert!(app.screens.saves.confirming().is_some());
    app.key(egui::Key::Enter);
    draw(&mut app);
    assert!(!saves.manual_slot("Broken one").unwrap().exists());
    assert_eq!(app.screens.saves.rows().len(), 2);
    app.key(egui::Key::Escape);
    assert!(!app.screens.saves.is_open());
    std::fs::remove_dir_all(dir).unwrap();
}
