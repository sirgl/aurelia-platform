//! The command journal (platform/docs/saves.md §3) on the sandbox: ten rounds of an
//! AI game recorded after a save replay from that save to the same world,
//! and a journal with a wrong command names the round it diverged in.

use kernel::ai::driver::AiDriver;
use kernel::ai::AiPlayers;
use kernel::command::KernelCommand;
use kernel::persistence::codec::{to_bytes, Reader};
use kernel::persistence::encode::encode;
use kernel::replay::codec::CommandField;
use kernel::replay::hash::authoritative_hash;
use kernel::replay::journal::{replay, Divergence, ReplayError};
use sandbox_rules::ai::SandboxMind;
use sandbox_rules::game::{new_game, Setup};
use sandbox_rules::{Command, Game, Rules, Sandbox};

const SEED: u64 = 7_2026;
/// The round the save is made in (the map has cities and armies by then).
const SAVED: u32 = 20;
/// The round whose first command the wrong journal replaces.
const WRONG: u32 = 25;
const RECORDED: u32 = 30;

type Ai = AiDriver<Sandbox, SandboxMind>;

/// A recorded game: the save, the journal's header and records, the records
/// cut where round `WRONG` begins, the world there, and the final world.
struct Recorded {
    rules: Rules,
    save: Vec<u8>,
    head: Vec<u8>,
    tail: Vec<u8>,
    at_wrong: Game,
    game: Game,
}

fn play(game: &mut Game, ai: &mut Ai, rules: &Rules, until: u32) {
    while !game.is_over() && game.view().auth.turn().round < until {
        ai.run(game, rules, u32::MAX);
    }
}

fn recorded() -> Recorded {
    let rules = sandbox_content::rules().expect("the sandbox pack compiles");
    let mut game = new_game(&rules, Setup::standard(SEED));
    let mut ai = Ai::for_game(&rules, rules.default_difficulty(), Vec::new());
    ai.configure_game(&mut game);
    play(&mut game, &mut ai, &rules, SAVED);
    let save = to_bytes(&encode(&game, &rules));
    let mut head = game.begin_journal(&rules);
    play(&mut game, &mut ai, &rules, WRONG);
    head.extend(game.take_journal());
    let at_wrong = game.clone();
    play(&mut game, &mut ai, &rules, RECORDED);
    let tail = game.take_journal();
    Recorded { rules, save, head, tail, at_wrong, game }
}

#[test]
fn ten_recorded_rounds_replay_from_the_save_to_the_same_world() {
    let r = recorded();
    let journal = [r.head.clone(), r.tail.clone()].concat();
    let replayed = replay(&r.save, &journal, &r.rules).expect("the journal replays");
    assert_eq!(replayed.rounds, (RECORDED - SAVED) as usize, "every round's hash is checked");
    assert!(replayed.commands > 0 && !replayed.cut);
    assert_eq!(authoritative_hash(&replayed.game), authoritative_hash(&r.game));
    let per_round = journal.len() / replayed.rounds;
    eprintln!("{} commands, {} bytes, {per_round} bytes a round", replayed.commands, journal.len());
}

#[test]
fn a_wrong_command_names_the_round_it_diverged_in() {
    let r = recorded();
    let world = r.at_wrong.view().auth;
    let first = Command::take(&mut Reader::new(&r.tail[1..], 0), world).expect("round 25 starts with a command");
    let active = world.active_player();
    let wrong = Command::Kernel(KernelCommand::EndTurn { player: active });
    assert_ne!(first, wrong, "the AI acts before it ends its turn");
    let encoded = |command: &Command| {
        let mut out = Vec::new();
        command.put(world, &mut out);
        out
    };
    let rest = &r.tail[1 + encoded(&first).len()..];
    let journal = [r.head.clone(), vec![0], encoded(&wrong), rest.to_vec()].concat();
    let Err(ReplayError::Diverged(Divergence { round, .. })) = replay(&r.save, &journal, &r.rules) else {
        panic!("a wrong command diverges");
    };
    assert_eq!(round, WRONG);
}
