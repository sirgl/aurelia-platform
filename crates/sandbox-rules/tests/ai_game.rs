//! The platform proof (platform/PLATFORM.md stage 7): an AI-only sandbox game on a
//! fixed seed plays to its end, replays to the same world hash and continues
//! from a save exactly as the original did.

use kernel::ai::driver::AiDriver;
use kernel::ai::AiPlayers;
use kernel::ids::PlayerId;
use kernel::persistence::{codec, decode::decode, encode::encode};
use kernel::replay::hash::authoritative_hash;
use sandbox_rules::ai::SandboxMind;
use sandbox_rules::game::{new_game, Setup};
use sandbox_rules::victory::{score, ROUND_LIMIT};
use sandbox_rules::{Command, Game, Rules, Sandbox};

const SEED: u64 = 7_2026;
/// The world hash at the end of the seed's game (pinned: a change of the
/// sandbox rules, its content or the kernel's behaviour shows here).
/// Re-pinned in stage 5c from 0x6e5ae45a493bec12: the kernel no longer
/// writes the Classic great person points (zeros for the sandbox) into every
/// city's record; a city state writing those zeros reproduces the old pin.
const FINAL_HASH: u64 = 0xa03355889320baa3;
/// The world is validated every this many rounds.
const VALIDATE_EVERY: u32 = 10;

type Ai = AiDriver<Sandbox, SandboxMind>;

fn rules() -> Rules {
    sandbox_content::rules().expect("the sandbox pack compiles")
}

/// A new game with every player played by the AI.
fn start(rules: &Rules) -> (Game, Ai) {
    let mut game = new_game(rules, Setup::standard(SEED));
    let mut ai = Ai::for_game(rules, rules.default_difficulty(), Vec::new());
    ai.record_commands();
    ai.configure_game(&mut game);
    (game, ai)
}

/// Play until the game is over or the round reaches `until`; the world is
/// validated every `VALIDATE_EVERY` rounds.
fn play(game: &mut Game, ai: &mut Ai, rules: &Rules, until: u32) {
    while !game.is_over() && game.view().auth.turn().round < until {
        let round = game.view().auth.turn().round;
        let report = ai.run(game, rules, u32::MAX);
        assert!(!report.waiting || game.is_over(), "an AI-only game never waits");
        if round.is_multiple_of(VALIDATE_EVERY) {
            game.validate(rules).unwrap_or_else(|e| panic!("round {round}: {e}"));
        }
    }
}

fn summary(game: &Game) -> String {
    let auth = game.view().auth;
    let players = (0..auth.players().count() as u8).map(PlayerId);
    let lines: Vec<String> = players
        .map(|p| {
            let (cities, citizens) = score(auth, p);
            let units = auth.indexes().units_of(p).len();
            format!("P{}: {cities} cities, {citizens} citizens, {units} units", p.0)
        })
        .collect();
    lines.join("; ")
}

#[test]
fn an_ai_game_ends_by_the_round_limit_and_replays_to_the_same_hash() {
    let rules = rules();
    let (mut game, mut ai) = start(&rules);
    play(&mut game, &mut ai, &rules, u32::MAX);
    let auth = game.view().auth;
    let hash = authoritative_hash(&game);
    eprintln!(
        "round {}, winner {:?}, hash {hash:#018x}, commands {:?}\n{}",
        auth.turn().round,
        game.winner(),
        ai.command_counts(),
        summary(&game)
    );
    assert!(game.is_over(), "the game ends");
    assert_eq!(auth.turn().round, ROUND_LIMIT, "nobody held every capital: the round limit ends it");
    assert_eq!(game.winner(), Some(PlayerId(0)), "the largest empire wins");
    game.validate(&rules).expect("the final world is valid");

    let commands: Vec<Command> = ai.commands().to_vec();
    let (mut replay, _) = start(&rules);
    for (i, command) in commands.iter().enumerate() {
        replay.execute(&rules, *command).unwrap_or_else(|e| panic!("command {i} {command:?}: {e}"));
    }
    assert_eq!(authoritative_hash(&replay), hash, "the replay ends in the same world");
    assert_eq!(hash, FINAL_HASH, "the seed's game is pinned");
}

#[test]
fn a_game_saved_mid_way_continues_like_the_original() {
    let rules = rules();
    let (mut game, mut ai) = start(&rules);
    play(&mut game, &mut ai, &rules, 40);

    let mut save = encode(&game, &rules);
    save.ai = ai.save();
    let bytes = codec::to_bytes(&save);
    let loaded_save = codec::from_bytes::<Sandbox>(&bytes).expect("the save reads back");
    let mut loaded = decode(&loaded_save, &rules).expect("the save loads");
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game), "loading keeps the world");
    let mut loaded_ai = Ai::for_game(&rules, rules.default_difficulty(), Vec::new());
    loaded_ai.load(&loaded_save.ai, loaded.view().auth.map().tile_count(), &rules).expect("the AI memory loads");

    play(&mut game, &mut ai, &rules, u32::MAX);
    play(&mut loaded, &mut loaded_ai, &rules, u32::MAX);
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game), "the loaded game continues alike");
}
