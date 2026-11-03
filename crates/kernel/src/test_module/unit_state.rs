//! The module's state of a unit (`world::module::ModuleUnit`) on the test
//! module: it travels with its unit through moves, fights and saves, goes
//! with a dead unit, and other players see only its public part while they
//! see the unit.

use super::{Marks, Order, TestModule};
use crate::command::KernelCommand;
use crate::ids::{EntityId, PlayerId, TileId, UnitHandle, UnitKindId};
use crate::persistence::codec::{from_bytes, to_bytes};
use crate::persistence::decode::{decode, DecodeError};
use crate::persistence::encode::encode;
use crate::replay::hash::authoritative_hash;
use crate::rules::compile::compile_rules;
use crate::rules::source::SourceRules;
use crate::rules::CompiledRuleset;
use crate::world::state::{GameConfig, GameState};

const SCOUT: UnitKindId = UnitKindId(0);
const MARKED: Marks = Marks { badge: 7, fights: 2 };

fn rules() -> CompiledRuleset<TestModule> {
    compile_rules(SourceRules::default()).expect("empty rules compile")
}

fn game(rules: &CompiledRuleset<TestModule>) -> GameState<TestModule> {
    GameState::new(rules, GameConfig { players: 2, width: 12, height: 10, rng_seed: 7 })
}

/// A unit of `owner` on `tile` with the given marks.
fn marked_unit(game: &mut GameState<TestModule>, rules: &CompiledRuleset<TestModule>, owner: PlayerId, tile: TileId, marks: Marks) -> UnitHandle {
    let unit = game.debug_spawn_unit(rules, owner, tile, SCOUT);
    *game.world_mut().units.module_mut(unit).expect("a live unit") = marks;
    unit
}

fn marks_of(game: &GameState<TestModule>, unit: UnitHandle) -> Option<Marks> {
    game.view().auth.units().module_of(unit).copied()
}

fn handle_of(game: &GameState<TestModule>, id: EntityId) -> UnitHandle {
    let units = game.view().auth.units();
    units.iter_handles().find(|(_, core)| core.id == id).map(|(h, _)| h).expect("the unit is there")
}

#[test]
fn a_new_unit_starts_with_the_default_state() {
    let rules = rules();
    let mut game = game(&rules);
    let unit = game.debug_spawn_unit(&rules, PlayerId(0), TileId(13), SCOUT);
    assert_eq!(marks_of(&game, unit), Some(Marks::default()));
}

#[test]
fn a_move_keeps_the_state() {
    let rules = rules();
    let mut game = game(&rules);
    let unit = marked_unit(&mut game, &rules, PlayerId(0), TileId(13), MARKED);
    let order = KernelCommand::MoveUnit { unit, destination: TileId(14) };
    game.execute(&rules, Order::Kernel(order)).expect("a step to a neighbour");
    assert_eq!(game.view().auth.units().get(unit).map(|u| u.core.tile), Some(TileId(14)), "the unit moved");
    assert_eq!(marks_of(&game, unit), Some(MARKED));
}

#[test]
fn a_survivor_keeps_its_state_and_the_dead_unit_takes_its_own_along() {
    let rules = rules();
    let mut game = game(&rules);
    let survivor = marked_unit(&mut game, &rules, PlayerId(0), TileId(13), MARKED);
    let victim = marked_unit(&mut game, &rules, PlayerId(1), TileId(14), MARKED);

    game.execute(&rules, Order::Fight { unit: survivor, damage: 30 }).expect("a wound");
    game.execute(&rules, Order::Fight { unit: victim, damage: 100 }).expect("a death");

    assert_eq!(game.view().auth.units().get(survivor).map(|u| u.core.hp), Some(70));
    assert_eq!(marks_of(&game, survivor), Some(Marks { fights: MARKED.fights + 1, ..MARKED }), "the module counted the fight");
    assert_eq!(marks_of(&game, victim), None, "the dead unit has no state");
    let newcomer = game.debug_spawn_unit(&rules, PlayerId(1), TileId(14), SCOUT);
    assert_eq!(newcomer.index, victim.index, "the new unit takes the dead unit's slot");
    assert_eq!(marks_of(&game, newcomer), Some(Marks::default()), "and starts afresh");
    game.validate(&rules).expect("valid world");
}

#[test]
fn a_save_file_keeps_the_state_and_the_hash() {
    let rules = rules();
    let mut game = game(&rules);
    let plain = game.debug_spawn_unit(&rules, PlayerId(0), TileId(13), SCOUT);
    let marked = marked_unit(&mut game, &rules, PlayerId(1), TileId(40), MARKED);
    let ids = [plain, marked].map(|h| game.view().auth.units().get(h).expect("live").core.id);

    let bytes = to_bytes(&encode(&game, &rules));
    let loaded = decode(&from_bytes(&bytes).expect("own file reads"), &rules).expect("own save loads");

    assert_eq!(marks_of(&loaded, handle_of(&loaded, ids[0])), Some(Marks::default()));
    assert_eq!(marks_of(&loaded, handle_of(&loaded, ids[1])), Some(MARKED));
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    loaded.validate(&rules).expect("loaded world is valid");
}

#[test]
fn the_hash_sees_the_state_and_the_default_mixes_nothing() {
    let rules = rules();
    let mut game = game(&rules);
    let unit = game.debug_spawn_unit(&rules, PlayerId(0), TileId(13), SCOUT);
    let plain = authoritative_hash(&game);
    *game.world_mut().units.module_mut(unit).expect("live") = MARKED;
    assert_ne!(authoritative_hash(&game), plain, "the state is part of the truth");
    *game.world_mut().units.module_mut(unit).expect("live") = Marks::default();
    assert_eq!(authoritative_hash(&game), plain);
}

#[test]
fn a_damaged_state_in_a_save_is_corrupt() {
    let rules = rules();
    let mut game = game(&rules);
    marked_unit(&mut game, &rules, PlayerId(0), TileId(13), MARKED);
    let mut save = encode(&game, &rules);
    save.world.units[0].module = (MARKED.badge, Marks::MAX_FIGHTS + 1);
    let error = decode(&save, &rules).expect_err("too many fights");
    assert_eq!(error, DecodeError::Corrupt("unit[0] fights".into()));
}

#[test]
fn the_owner_reads_the_state_others_see_the_badge_while_they_see_the_unit() {
    let rules = rules();
    let mut game = game(&rules);
    let (owner, other) = (PlayerId(0), PlayerId(1));
    let unit = marked_unit(&mut game, &rules, owner, TileId(13), MARKED);
    let far = game.debug_spawn_unit(&rules, other, TileId(118), SCOUT);

    let mine = game.player_view(&rules, owner);
    assert_eq!(mine.own_unit_state(unit), Some(&MARKED));
    assert_eq!(mine.unit_state_seen(unit), Some(MARKED.badge));
    assert_eq!(mine.own_unit_state(far), None, "not its unit");
    let theirs = game.player_view(&rules, other);
    assert!(!theirs.is_visible(TileId(13)));
    assert_eq!(theirs.unit_state_seen(unit), None, "out of sight: nothing");
    assert_eq!(theirs.own_unit_state(unit), None);

    let watcher = game.debug_spawn_unit(&rules, other, TileId(14), SCOUT);
    let theirs = game.player_view(&rules, other);
    assert_eq!(theirs.unit_state_seen(unit), Some(MARKED.badge), "in sight: the badge");
    assert_eq!(theirs.own_unit_state(unit), None, "never the fights");

    game.execute(&rules, Order::Fight { unit: watcher, damage: 100 }).expect("the watcher dies");
    let theirs = game.player_view(&rules, other);
    assert_eq!(theirs.unit_state_seen(unit), None, "out of sight again: nothing remembered");
}
