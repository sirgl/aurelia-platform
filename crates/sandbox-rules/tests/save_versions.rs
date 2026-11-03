//! A sandbox save written before save versions (platform/docs/saves.md) loads as it
//! did: the same codec, the same world, the same bytes when saved again.
//! The fixture was written by the build before (`fixtures/*.recipe.rs`).

use kernel::ai::driver::AiDriver;
use kernel::ai::AiPlayers;
use kernel::persistence::codec;
use kernel::persistence::decode::{RulesCheck, RulesMatch};
use kernel::persistence::encode::encode;
use kernel::persistence::load::load;
use kernel::replay::hash::authoritative_hash;
use sandbox_rules::ai::SandboxMind;
use sandbox_rules::Sandbox;

/// Round 12 of an AI game, with the AI memory, written by codec 1.
const CODEC1: &[u8] = include_bytes!("fixtures/sandbox-codec1.aurelia");
/// The world of `CODEC1` as this build reads it (the hash folds in the deposits and city
/// sightings of the knowledge since codec 2; a codec 1 save gets the map's deposits).
const CODEC1_WORLD: u64 = 0x7f775a3f3e22e794;

#[test]
fn a_save_of_the_build_before_loads_unchanged() {
    let rules = sandbox_content::rules().unwrap();
    let loaded = load::<Sandbox>(CODEC1, &rules, RulesMatch::SameRules).expect("the save loads");
    assert_eq!(loaded.rules, RulesCheck::Same);
    assert_eq!(authoritative_hash(&loaded.game), CODEC1_WORLD);
    loaded.game.validate(&rules).unwrap();
    let mut ai = AiDriver::<Sandbox, SandboxMind>::for_game(&rules, rules.default_difficulty(), Vec::new());
    ai.load(&loaded.ai, loaded.game.view().auth.map().tile_count(), &rules).expect("the AI memory loads");

    let mut save = encode(&loaded.game, &rules);
    save.ai = loaded.ai.clone();
    // Codec 2 added the deposits and city sightings of the knowledge: the save is written in it,
    // and reads back to the same world and the same bytes.
    let again = codec::to_bytes(&save);
    assert_eq!(u32::from_le_bytes(again[8..12].try_into().unwrap()), 2);
    let reloaded = load::<Sandbox>(&again, &rules, RulesMatch::SameRules).expect("the new save loads");
    assert_eq!(authoritative_hash(&reloaded.game), CODEC1_WORLD);
    let mut resaved = encode(&reloaded.game, &rules);
    resaved.ai = reloaded.ai.clone();
    assert_eq!(codec::to_bytes(&resaved), again);
}
