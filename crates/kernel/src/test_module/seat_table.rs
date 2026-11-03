//! The table of seats (`agent::SeatTable`) on the toy module: who is polled, what a refusal
//! does, when a round is reported done, and that a seat without an agent waits.

use super::{Order, TestModule};
use crate::agent::{AgentPoll, ScriptedAgent, SeatTable};
use crate::ai::StepOutcome;
use crate::command::KernelCommand;
use crate::ids::{PlayerId, TileId};
use crate::rules::compile::compile_rules;
use crate::rules::source::SourceRules;
use crate::rules::CompiledRuleset;
use crate::world::state::{GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn rules() -> CompiledRuleset<TestModule> {
    compile_rules(SourceRules::default()).expect("empty rules compile")
}

fn game(rules: &CompiledRuleset<TestModule>) -> GameState<TestModule> {
    GameState::new(
        rules,
        GameConfig {
            players: 2,
            width: 12,
            height: 10,
            rng_seed: 7,
        },
    )
}

fn end(seat: PlayerId) -> AgentPoll<Order> {
    AgentPoll::Command(Order::Kernel(KernelCommand::EndTurn { player: seat }))
}

#[test]
fn a_seat_without_an_agent_waits_and_a_pending_agent_waits() {
    let rules = rules();
    let mut game = game(&rules);
    let mut table: SeatTable<TestModule> = SeatTable::new();
    assert_eq!(
        table.step(&mut game, &rules),
        StepOutcome::Waiting,
        "nobody plays player 0"
    );
    table.assign(
        &[P0],
        Box::new(ScriptedAgent::<TestModule>::new(Vec::new())),
    );
    assert_eq!(
        table.step(&mut game, &rules),
        StepOutcome::Waiting,
        "a script that ran dry is Pending"
    );
    assert_eq!(game.view().auth.active_player(), P0, "nothing happened");
}

#[test]
fn ending_a_turn_with_a_command_passes_the_seat_and_the_round() {
    let rules = rules();
    let mut game = game(&rules);
    let mut table: SeatTable<TestModule> = SeatTable::new();
    table.assign(
        &[P0],
        Box::new(ScriptedAgent::<TestModule>::new(vec![end(P0)])),
    );
    table.assign(
        &[P1],
        Box::new(ScriptedAgent::<TestModule>::new(vec![end(P1)])),
    );
    assert_eq!(table.step(&mut game, &rules), StepOutcome::Worked);
    assert_eq!(
        game.view().auth.active_player(),
        P1,
        "player 0's agent ended its turn"
    );
    assert_eq!(table.step(&mut game, &rules), StepOutcome::Worked);
    assert_eq!(
        game.view().auth.turn().round,
        1,
        "player 1's agent ended the round"
    );
}

#[test]
fn the_end_turn_answer_executes_the_modules_end_turn_command() {
    let rules = rules();
    let mut game = game(&rules);
    let mut table: SeatTable<TestModule> = SeatTable::new();
    let slot = table.assign(
        &[P0],
        Box::new(ScriptedAgent::<TestModule>::new(vec![AgentPoll::EndTurn])),
    );
    // The toy module's end of turn is a no-op `Pass`: applied, and the seat reported done.
    assert_eq!(table.step(&mut game, &rules), StepOutcome::PlayerDone(P0));
    let agent = table
        .agent_ref::<ScriptedAgent<TestModule>>(slot)
        .expect("the agent");
    assert_eq!(agent.counts(), (1, 0));
}

#[test]
fn a_refused_command_reaches_the_agent_and_changes_nothing() {
    let rules = rules();
    let mut game = game(&rules);
    let mut table: SeatTable<TestModule> = SeatTable::new();
    // Player 0 orders a unit that does not exist.
    let ghost = Order::Kernel(KernelCommand::MoveUnit {
        unit: crate::ids::UnitHandle {
            index: 9999,
            generation: 0,
        },
        destination: TileId(3),
    });
    let slot = table.assign(
        &[P0],
        Box::new(ScriptedAgent::<TestModule>::new(vec![AgentPoll::Command(
            ghost,
        )])),
    );
    assert_eq!(table.step(&mut game, &rules), StepOutcome::Worked);
    let agent = table
        .agent_ref::<ScriptedAgent<TestModule>>(slot)
        .expect("the agent");
    assert_eq!(agent.counts(), (0, 1), "one command refused, none applied");
    assert_eq!(game.view().auth.active_player(), P0);
}

#[test]
fn replacing_a_seat_hands_it_to_the_new_agent() {
    let rules = rules();
    let mut game = game(&rules);
    let mut table: SeatTable<TestModule> = SeatTable::new();
    let both = table.assign(
        &[P0, P1],
        Box::new(ScriptedAgent::<TestModule>::new(vec![end(P0)])),
    );
    assert_eq!(table.slot_of(P1), Some(both));
    let own = table.replace(
        P1,
        Box::new(ScriptedAgent::<TestModule>::new(vec![end(P1)])),
    );
    assert_ne!(own, both);
    assert_eq!(
        table.slot_of(P0),
        Some(both),
        "the old agent keeps its other seat"
    );
    assert_eq!(table.step(&mut game, &rules), StepOutcome::Worked);
    assert_eq!(table.step(&mut game, &rules), StepOutcome::Worked);
    assert_eq!(
        game.view().auth.turn().round,
        1,
        "both seats ended their turns, each by its own agent"
    );
    let counts = |slot| {
        table
            .agent_ref::<ScriptedAgent<TestModule>>(slot)
            .expect("agent")
            .counts()
    };
    assert_eq!((counts(both), counts(own)), ((1, 0), (1, 0)));
    table.remove(P0);
    table.remove(P1);
    assert!(!table.has_agent(P0) && !table.has_agent(P1));
    assert!(
        table.agent(both).is_none(),
        "an agent without seats is dropped"
    );
}
