//! The shell writes a command journal beside every autosave of a played match
//! (platform/docs/saves.md §3), and the journal replays from its save to the world of the next
//! autosave: the path `desktop --replay` takes, through the app's real frames.

use client::shell::app::App;
use client::shell::capture::{capture_texture, wait_idle};
use client::shell::module::{ModuleUi, ShellAction};
use client::shell::saves::{journal_path, read_tagged, ModuleSaves};
use kernel::persistence::decode::RulesMatch;
use kernel::persistence::load::load;
use kernel::replay::authoritative_hash;
use kernel::replay::journal::replay;
use sandbox_rules::game::Setup;
use sandbox_ui::{SandboxUi, MENU_SEED};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SIZE: (u32, u32) = (320, 180);
const ROUNDS: u32 = 4;

#[test]
fn every_autosave_has_a_journal_that_replays_to_the_next_one() {
    let core = match client::shell::gpu::open() {
        Ok(core) => core,
        Err(error) => return eprintln!("skipped: no GPU ({error})"),
    };
    let dir = std::env::temp_dir().join(format!("aurelia-journal-app-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // The only test of this binary: the save directory is the process's.
    std::env::set_var("AURELIA_SAVE_DIR", &dir);
    let texture = capture_texture(&core, SIZE.0, SIZE.1, FORMAT);
    let target = texture.create_view(&Default::default());
    let mut app = App::new(&core, FORMAT, None, SandboxUi::game(), &SandboxUi::terrain_layers());
    app.screens.settings.autosave_every = 1;
    app.module.start(Setup::standard(MENU_SEED), 1);
    app.apply_module_actions();
    let mut frame = 0;
    let mut next_frame = |app: &mut App<SandboxUi>| {
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
    for _ in 0..ROUNDS {
        app.apply(ShellAction::EndTurn);
        next_frame(&mut app);
        while app.module.turn_running() {
            next_frame(&mut app);
        }
    }

    let saves = ModuleSaves::under(&dir, app.module.module_id()).autosaves().list();
    assert_eq!(saves.len(), ROUNDS as usize, "one autosave a round");
    assert!(saves.iter().all(|(_, save)| journal_path(save).is_file()), "every autosave has its journal");
    let rules = sandbox_content::rules().unwrap();
    for pair in saves.windows(2) {
        let (module, save) = read_tagged(&pair[0].1).unwrap();
        assert_eq!(module, "sandbox");
        let journal = std::fs::read(journal_path(&pair[0].1)).unwrap();
        let replayed = replay(&save, &journal, &rules).expect("the journal replays");
        assert_eq!(replayed.rounds, 1, "an autosave a round: one round per journal");
        let (_, next) = read_tagged(&pair[1].1).unwrap();
        let next = load(&next, &rules, RulesMatch::Exact).unwrap().game;
        assert_eq!(authoritative_hash(&replayed.game), authoritative_hash(&next), "the journal ends at the next save");
    }
    std::fs::remove_dir_all(dir).unwrap();
}
