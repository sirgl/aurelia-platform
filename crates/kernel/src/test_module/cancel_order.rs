//! A standing move order is cancelled by `KernelCommand::CancelOrder`, in
//! the kernel and for any rules module: the unit stops where it stands with
//! the moves it has, its march is cleared, a cancel with no order is
//! refused, and the cancel is a command of the journal (a replay reaches the
//! same hash).

use super::{Order, TestModule};
use crate::command::{CommandError, KernelCommand};
use crate::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
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

fn rules() -> CompiledRuleset<TestModule> {
    compile_rules(SourceRules::default()).expect("empty rules compile")
}

fn game(rules: &CompiledRuleset<TestModule>) -> GameState<TestModule> {
    GameState::new(rules, GameConfig { players: 2, width: 12, height: 10, rng_seed: 7 })
}

fn kernel(order: KernelCommand) -> Order {
    Order::Kernel(order)
}

/// A unit on its way to the far side of the map: the march ordered, the
/// first turn's steps made.
fn marching(game: &mut GameState<TestModule>, rules: &CompiledRuleset<TestModule>) -> (UnitHandle, TileId) {
    let unit = game.debug_spawn_unit(rules, P0, TileId(13), SCOUT);
    let far = TileId(22);
    game.execute(rules, kernel(KernelCommand::MarchTo { unit, destination: far })).expect("the march is accepted");
    (unit, far)
}

#[test]
fn a_march_is_a_standing_order_until_it_is_cancelled() {
    let rules = rules();
    let mut game = game(&rules);
    let (unit, far) = marching(&mut game, &rules);
    let walking = *game.view().auth.units().get(unit).unwrap().core;
    assert_eq!(walking.goal, Some(far), "the order stands: the way is longer than a turn");
    assert!(walking.has_order());
    assert_ne!(walking.tile, TileId(13), "the first turn's steps were made");

    let before = authoritative_hash(&game);
    let treasury = game.view().auth.players().treasury_of(P0);
    game.execute(&rules, kernel(KernelCommand::CancelOrder { unit })).expect("the cancel is accepted");
    let stopped = *game.view().auth.units().get(unit).unwrap().core;
    assert_eq!((stopped.tile, stopped.moves_left, stopped.moves_max), (walking.tile, walking.moves_left, walking.moves_max), "stands where it stands, moves as they were");
    assert_eq!(stopped.goal, None, "the march is cleared");
    assert!(!stopped.has_order());
    assert_ne!(authoritative_hash(&game), before, "the order was part of the world");
    assert_eq!(game.view().auth.players().treasury_of(P0), treasury, "free");
    game.validate(&rules).expect("a valid world");
}

/// The cancel is free: a unit that still has moves keeps all of them (an order set before its
/// owner's turn began, as a load or an earlier round leaves it).
#[test]
fn a_unit_with_moves_left_keeps_every_one() {
    let rules = rules();
    let mut game = game(&rules);
    let unit = game.debug_spawn_unit(&rules, P0, TileId(13), SCOUT);
    game.world_mut().units.get_mut_core(unit).expect("a live unit").goal = Some(TileId(22));
    let ready = *game.view().auth.units().get(unit).unwrap().core;
    assert!(ready.moves_left > 0 && ready.has_order());
    game.execute(&rules, kernel(KernelCommand::CancelOrder { unit })).expect("cancel");
    let stopped = *game.view().auth.units().get(unit).unwrap().core;
    assert_eq!((stopped.tile, stopped.moves_left, stopped.goal), (ready.tile, ready.moves_left, None));
}

#[test]
fn a_cancel_with_no_order_or_for_no_unit_changes_nothing() {
    let rules = rules();
    let mut game = game(&rules);
    let idle = game.debug_spawn_unit(&rules, P0, TileId(13), SCOUT);
    let other = game.debug_spawn_unit(&rules, PlayerId(1), TileId(40), SCOUT);
    let before = authoritative_hash(&game);
    let refused = game.execute(&rules, kernel(KernelCommand::CancelOrder { unit: idle }));
    assert!(matches!(refused, Err(CommandError::RuleViolation(_))), "{refused:?}");
    game.execute(&rules, kernel(KernelCommand::CancelOrder { unit: other })).expect_err("not P1's turn, and no order");
    assert_eq!(authoritative_hash(&game), before);
}

#[test]
fn a_cancel_is_in_the_journal_and_the_replay_reaches_the_same_hash() {
    let rules = rules();
    let mut game = game(&rules);
    let (unit, _) = marching(&mut game, &rules);
    // The save holds the unit on its way; the journal records the cancel (a second one is refused and not recorded).
    let save = to_bytes(&encode(&game, &rules));
    let mut journal = game.begin_journal(&rules);
    game.execute(&rules, kernel(KernelCommand::CancelOrder { unit })).expect("cancel");
    game.execute(&rules, kernel(KernelCommand::CancelOrder { unit })).expect_err("nothing is left to cancel");
    game.execute(&rules, Order::Pass).expect("pass");
    journal.extend(game.take_journal());
    let replayed = replay(&save, &journal, &rules).expect("the journal replays");
    assert_eq!(replayed.commands, 2, "the cancel and the pass; the refused cancel is not recorded");
    assert_eq!(authoritative_hash(&replayed.game), authoritative_hash(&game));
    let id = game.view().auth.units().get(unit).unwrap().core.id;
    let again = replayed.game.view().auth.units().iter_handles().find(|(_, core)| core.id == id).map(|(_, core)| *core).expect("the unit");
    assert_eq!(again.goal, None);
}
