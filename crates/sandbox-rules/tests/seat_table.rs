//! PlayerAgent P1 (classic/docs/player-agent.md): the sandbox's pinned AI game played
//! through a `SeatTable` of `ScriptedAgent`s, one script per seat, reaches the
//! hash the direct AI driver reaches.

use kernel::agent::{AgentPoll, ScriptedAgent, SeatTable};
use kernel::ai::driver::AiDriver;
use kernel::ai::AiPlayers;
use kernel::ids::PlayerId;
use kernel::module::RulesModule;
use kernel::replay::hash::authoritative_hash;
use sandbox_rules::ai::SandboxMind;
use sandbox_rules::game::{new_game, Setup};
use sandbox_rules::{Command, Game, Rules, Sandbox};

const SEED: u64 = 7_2026;
/// The same pin as `ai_game.rs`.
const FINAL_HASH: u64 = 0xa03355889320baa3;

type Ai = AiDriver<Sandbox, SandboxMind>;

fn rules() -> Rules {
    sandbox_content::rules().expect("the sandbox pack compiles")
}

/// The direct AI game: its commands in order.
fn ai_commands(rules: &Rules) -> Vec<Command> {
    let mut game = new_game(rules, Setup::standard(SEED));
    let mut ai = Ai::for_game(rules, rules.default_difficulty(), Vec::new());
    ai.record_commands();
    ai.configure_game(&mut game);
    while !game.is_over() {
        let report = ai.run(&mut game, rules, u32::MAX);
        assert!(!report.waiting || game.is_over());
    }
    assert_eq!(authoritative_hash(&game), FINAL_HASH);
    ai.commands().to_vec()
}

/// Each command goes to the script of the seat whose turn it is.
fn scripts(rules: &Rules, commands: &[Command]) -> Vec<Vec<AgentPoll<Command>>> {
    let mut game: Game = new_game(rules, Setup::standard(SEED));
    let ai = Ai::for_game(rules, rules.default_difficulty(), Vec::new());
    ai.configure_game(&mut game);
    let players = game.view().auth.players().count();
    let mut scripts: Vec<Vec<AgentPoll<Command>>> = vec![Vec::new(); players];
    for command in commands {
        let seat = game.view().auth.active_player();
        let answer = if *command == Sandbox::end_turn(seat) {
            AgentPoll::EndTurn
        } else {
            AgentPoll::Command(*command)
        };
        scripts[seat.0 as usize].push(answer);
        game.execute(rules, *command)
            .expect("the recorded command applies");
    }
    scripts
}

fn play_scripted(rules: &Rules, scripts: Vec<Vec<AgentPoll<Command>>>, slice: u32) -> (Game, u32) {
    let mut game = new_game(rules, Setup::standard(SEED));
    // The settings of a game the AI played: every player is an AI player.
    Ai::for_game(rules, rules.default_difficulty(), Vec::new()).configure_game(&mut game);
    let mut table: SeatTable<Sandbox> = SeatTable::new();
    for (seat, script) in scripts.into_iter().enumerate() {
        table.assign(
            &[PlayerId(seat as u8)],
            Box::new(ScriptedAgent::new(script)),
        );
    }
    let mut rounds = 0;
    while !game.is_over() {
        let report = table.run(&mut game, rules, slice);
        assert!(
            !report.waiting || game.is_over(),
            "a script ran dry before the game ended"
        );
        rounds += u32::from(report.round_done);
    }
    (game, rounds)
}

#[test]
fn scripted_seats_reach_the_hash_of_the_direct_ai_game() {
    let rules = rules();
    let commands = ai_commands(&rules);
    let (game, rounds) = play_scripted(&rules, scripts(&rules, &commands), u32::MAX);
    assert_eq!(authoritative_hash(&game), FINAL_HASH);
    assert!(rounds > 10, "the table reported the rounds ({rounds})");
}

#[test]
fn the_slice_of_the_calls_changes_nothing() {
    let rules = rules();
    let commands = ai_commands(&rules);
    let (game, _) = play_scripted(&rules, scripts(&rules, &commands), 7);
    assert_eq!(authoritative_hash(&game), FINAL_HASH);
}

// P2: the AI itself as an agent.

use kernel::agent::AiAgent;

/// A new AI-only game and a table that plays all its seats with one `AiAgent`.
fn ai_table(rules: &Rules) -> (Game, SeatTable<Sandbox>, usize) {
    let game = new_game(rules, Setup::standard(SEED));
    let mut ai = Ai::for_game(rules, rules.default_difficulty(), Vec::new());
    ai.record_commands();
    let seats: Vec<PlayerId> = (0..game.view().auth.players().count() as u8)
        .map(PlayerId)
        .collect();
    let mut table: SeatTable<Sandbox> = SeatTable::new();
    let slot = table.assign(&seats, Box::new(AiAgent::<Sandbox>::new(ai)));
    (game, table, slot)
}

#[test]
fn the_ai_agent_plays_the_pinned_game_through_the_seat_table() {
    let rules = rules();
    for slice in [u32::MAX, 5] {
        let (mut game, mut table, slot) = ai_table(&rules);
        while !game.is_over() {
            let report = table.run(&mut game, &rules, slice);
            assert!(
                !report.waiting || game.is_over(),
                "an AI-only game never waits"
            );
        }
        assert_eq!(authoritative_hash(&game), FINAL_HASH, "slice {slice}");
        let agent = table
            .agent_ref::<AiAgent<Sandbox>>(slot)
            .expect("the AI agent");
        let direct = ai_commands(&rules);
        assert_eq!(
            agent.ai().commands(),
            direct.as_slice(),
            "the same commands in the same order"
        );
    }
}
