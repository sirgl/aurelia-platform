// Recipe of sandbox-codec1.aurelia: written by commit 6c7cc0c (sandbox codec 1) as platform/crates/sandbox-rules/tests/write_fixture.rs,
// run with FIXTURE=<path>. Rules 0x8df41e7b9edf2b22, world 0xcdba5e41c157b53f, 33037 bytes, round 12, AI memory included.
//! Writes a sandbox save of this build (codec 1, with the AI memory) to $FIXTURE.
use kernel::ai::driver::AiDriver;
use kernel::ai::AiPlayers;
use kernel::persistence::{codec, encode::encode};
use sandbox_rules::ai::SandboxMind;
use sandbox_rules::game::{new_game, Setup};
use sandbox_rules::Sandbox;

#[test]
fn write_fixture() {
    let rules = sandbox_content::rules().unwrap();
    let mut game = new_game(&rules, Setup::standard(7_2026));
    let mut ai = AiDriver::<Sandbox, SandboxMind>::for_game(&rules, rules.default_difficulty(), Vec::new());
    ai.configure_game(&mut game);
    while game.view().auth.turn().round < 12 {
        ai.run(&mut game, &rules, u32::MAX);
    }
    let mut save = encode(&game, &rules);
    save.ai = ai.save();
    let bytes = codec::to_bytes(&save);
    eprintln!("rules {:#x} world {:#x} bytes {}", rules.hash(), kernel::replay::hash::authoritative_hash(&game), bytes.len());
    std::fs::write(std::env::var("FIXTURE").unwrap(), bytes).unwrap();
}
