//! The sandbox UI drives a match through the shell's contract alone (no
//! window, no GPU): a city is founded, its production chosen, the turn ends
//! and the AI plays, a save loads back to the same world.

use client::shell::module::{AiProgress, ModuleUi, Outcome, ShellAction};
use kernel::replay::hash::authoritative_hash;
use sandbox_rules::game::Setup;
use sandbox_ui::SandboxUi;

fn hash(ui: &SandboxUi) -> u64 {
    authoritative_hash(&ui.session.as_ref().expect("a match runs").client.game)
}

#[test]
fn a_player_founds_a_city_ends_turns_and_reloads_the_save() {
    let mut ui = SandboxUi::game();
    ui.start(Setup::standard(5), 1);
    assert!(matches!(ui.take_actions().as_slice(), [ShellAction::MatchStarted { .. }]));
    assert!(ui.view().is_some(), "the map shows the match");

    let settler = ui.session.as_ref().and_then(|s| s.selected_unit()).expect("a unit is selected");
    let session = ui.session.as_mut().unwrap();
    session.found_city();
    assert_eq!(session.message, None, "the settler founds where it starts");
    ui.select_city(settler.tile.0);
    assert!(matches!(ui.take_actions().as_slice(), [ShellAction::Open(_)]), "the new city opens");
    let session = ui.session.as_mut().unwrap();
    let offer = session.offers().into_iter().next().expect("the city offers something");
    session.produce(offer.item);
    assert_eq!(session.city().expect("own city").queue.first(), Some(&offer.item));

    for _ in 0..3 {
        ui.begin_end_turn();
        while ui.tick_ai(std::time::Duration::from_secs(60)) == AiProgress::Busy {}
        assert!(!ui.turn_running(), "the AI turns are over");
    }
    assert_eq!(ui.turn(), 3);
    assert_eq!(ui.outcome(), Outcome::Playing);
    let feed = ui.refresh(client::shell::module::FrameInput {
        hovered_tile: None,
        hovered_badge: None,
        shell: client::shell::module::ShellFlags {
            game_mode: true,
            has_session: true,
            dev_mode: false,
            screen: client::shell::module::Screen::Map,
            brightness: 1.0,
        },
    });
    let feed = feed.expect("the HUD has a match to show");
    assert_eq!(feed.top_bar.len(), 3, "food, production, cities");
    assert!(feed.banners.iter().any(|b| b.own), "the own city has a banner");

    let bytes = ui.save_bytes().expect("saved between turns");
    let before = hash(&ui);
    let mut loaded = SandboxUi::game();
    loaded.load_bytes(&bytes).expect("the save loads");
    assert_eq!(hash(&loaded), before);
}

#[test]
fn every_unit_of_the_pack_has_a_model() {
    let pack = sandbox_content::pack();
    let rules = sandbox_content::rules().expect("compiles");
    for (unit, model) in rules.units.iter().zip(sandbox_content::art::unit_models(&pack, &rules)) {
        let model = model.unwrap_or_else(|| panic!("{} names no model", unit.key));
        assert!(render::unit_models::model_by_id(model).is_some(), "{}: no model {model}", unit.key);
    }
    assert_eq!(SandboxUi::terrain_layers().len(), 6, "the renderer's six ground layers");
}
