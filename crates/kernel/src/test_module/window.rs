//! The simultaneous window (`GameState::open_window`, `execute_as`): the players of the window
//! order their own units at once while the turn stays where it is, a seat can order only what is
//! its own, a seat never ends a turn, the window is a record of the command journal, and a
//! replay reaches the same hash.

use super::{Order, TestModule};
use crate::command::{CommandError, KernelCommand};
use crate::ids::{PlayerId, TileId, UnitKindId};
use crate::persistence::codec::to_bytes;
use crate::persistence::encode::encode;
use crate::replay::hash::authoritative_hash;
use crate::replay::journal::replay;
use crate::rules::compile::compile_rules;
use crate::rules::source::SourceRules;
use crate::rules::CompiledRuleset;
use crate::world::state::{GameConfig, GameState};

const SCOUT: UnitKindId = UnitKindId(0);
const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn rules() -> CompiledRuleset<TestModule> {
    compile_rules(SourceRules::default()).expect("empty rules compile")
}

fn game(rules: &CompiledRuleset<TestModule>) -> GameState<TestModule> {
    GameState::new(rules, GameConfig { players: 2, width: 12, height: 10, rng_seed: 7 })
}

fn kernel(order: KernelCommand) -> Order {
    Order::Kernel(order)
}

#[test]
fn a_seat_of_the_window_orders_its_units_while_it_is_not_its_turn() {
    let rules = rules();
    let mut game = game(&rules);
    let mine = game.debug_spawn_unit(&rules, P1, TileId(40), SCOUT);
    let theirs = game.debug_spawn_unit(&rules, P0, TileId(13), SCOUT);
    let order = kernel(KernelCommand::MoveUnit { unit: mine, destination: TileId(41) });
    assert_eq!(game.view().auth.active_player(), P0, "round 0 opens with player 0");
    game.execute(&rules, order.clone()).expect_err("player 1 is out of turn without a window");
    game.execute_as(P1, &rules, order.clone()).expect_err("and a window that is not open does not help");

    game.open_window(0b11);
    game.execute_as(P1, &rules, order).expect("player 1 moves in the window");
    assert_eq!(game.view().auth.units().get(mine).unwrap().core.tile, TileId(41));
    assert_eq!(game.view().auth.active_player(), P0, "the turn did not move");
    game.execute_as(P0, &rules, kernel(KernelCommand::MoveUnit { unit: theirs, destination: TileId(14) }))
        .expect("player 0 moves at the same time");

    let foreign = kernel(KernelCommand::MoveUnit { unit: theirs, destination: TileId(15) });
    let refused = game.execute_as(P1, &rules, foreign);
    assert!(matches!(refused, Err(CommandError::RuleViolation(_))), "a seat orders only its own: {refused:?}");
    assert_eq!(game.view().auth.units().get(theirs).unwrap().core.tile, TileId(14));
}

#[test]
fn a_seat_never_ends_a_turn_and_a_closed_window_refuses_everybody() {
    let rules = rules();
    let mut game = game(&rules);
    game.debug_spawn_unit(&rules, P0, TileId(13), SCOUT);
    let unit = game.debug_spawn_unit(&rules, P1, TileId(40), SCOUT);
    game.open_window(0b11);
    for seat in [P0, P1] {
        let refused = game.execute_as(seat, &rules, kernel(KernelCommand::EndTurn { player: seat }));
        assert!(matches!(refused, Err(CommandError::RuleViolation(_))), "{seat:?}: {refused:?}");
    }
    assert_eq!(game.view().auth.active_player(), P0);
    assert_eq!(game.view().auth.turn().round, 0);
    game.close_window();
    assert_eq!(game.view().auth.turn().window(), 0);
    let order = kernel(KernelCommand::MoveUnit { unit, destination: TileId(41) });
    game.execute_as(P1, &rules, order).expect_err("closed");
    // The resolution still ends turns one by one, in player order.
    game.execute(&rules, kernel(KernelCommand::EndTurn { player: P0 })).expect("player 0 ends");
    assert_eq!(game.view().auth.active_player(), P1);
    game.execute(&rules, kernel(KernelCommand::EndTurn { player: P1 })).expect("player 1 ends");
    assert_eq!(game.view().auth.turn().round, 1);
}

/// The window is part of the journal: a replay opens it where the game did, plays the seats'
/// commands as the seats, and checks the same hashes. Orders that do not touch each other give
/// one world in either order.
#[test]
fn the_window_and_its_commands_replay_to_the_same_hash() {
    let rules = rules();
    let mut base = game(&rules);
    let a = base.debug_spawn_unit(&rules, P0, TileId(13), SCOUT);
    let b = base.debug_spawn_unit(&rules, P1, TileId(40), SCOUT);
    let save = to_bytes(&encode(&base, &rules));
    let order_a = kernel(KernelCommand::MoveUnit { unit: a, destination: TileId(14) });
    let order_b = kernel(KernelCommand::MoveUnit { unit: b, destination: TileId(41) });
    let mut hashes = Vec::new();
    for swapped in [false, true] {
        let mut game = base.clone();
        let mut journal = game.begin_journal(&rules);
        game.open_window(0b11);
        let (first, second) = if swapped { ((P1, &order_b), (P0, &order_a)) } else { ((P0, &order_a), (P1, &order_b)) };
        game.execute_as(first.0, &rules, first.1.clone()).expect("first");
        game.execute_as(second.0, &rules, second.1.clone()).expect("second");
        game.close_window();
        game.execute(&rules, kernel(KernelCommand::EndTurn { player: P0 })).expect("end 0");
        game.execute(&rules, kernel(KernelCommand::EndTurn { player: P1 })).expect("end 1");
        journal.extend(game.take_journal());
        let replayed = replay(&save, &journal, &rules).expect("the journal replays");
        assert_eq!((replayed.commands, replayed.rounds), (4, 1), "two seat commands, two ends, one round checked");
        assert_eq!(authoritative_hash(&replayed.game), authoritative_hash(&game));
        hashes.push(authoritative_hash(&game));
    }
    assert_eq!(hashes[0], hashes[1], "orders on different units commute");
}

#[test]
fn a_window_is_not_saved_and_not_hashed() {
    let rules = rules();
    let mut game = game(&rules);
    let before = authoritative_hash(&game);
    game.open_window(0b11);
    assert_eq!(authoritative_hash(&game), before, "the hash does not know the window");
    let loaded = crate::persistence::decode::decode(&encode(&game, &rules), &rules).expect("a save loads");
    assert_eq!(loaded.view().auth.turn().window(), 0, "a loaded game starts without a window");
}
