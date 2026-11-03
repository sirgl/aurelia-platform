//! The sandbox's own rules on a hand-built position: melee damage and the
//! capture of a city.

use kernel::ids::{PlayerId, TileId, UnitKindId};
use kernel::navigation::cost::enter_cost_on_map;
use kernel::module::RulesModule;
use sandbox_rules::combat::CITY_MAX_HP;
use sandbox_rules::game::{new_game, Setup};
use sandbox_rules::{Command, Game, Rules, Sandbox};

fn rules() -> Rules {
    sandbox_content::rules().expect("the sandbox pack compiles")
}

fn kind(rules: &Rules, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).expect("unit in the pack") as u16)
}

/// A land tile with land around it and nobody near: (city tile, neighbour).
fn open_ground(game: &Game) -> (TileId, TileId) {
    let auth = game.view().auth;
    let map = auth.map();
    let empty = |t: TileId| enter_cost_on_map(map, t).is_some() && auth.indexes().units_on(t).is_empty();
    let quiet = |t: TileId| kernel::math::hex_disc(map.dimensions, t, 3).all(|n| auth.indexes().units_on(n).is_empty());
    (0..map.tile_count() as u32)
        .map(TileId)
        .filter(|t| empty(*t) && quiet(*t) && map.owner(*t) == Some(None))
        .find_map(|t| map.dimensions.neighbors(t).into_iter().find(|n| empty(*n)).map(|n| (t, n)))
        .expect("open ground on the map")
}

#[test]
fn a_raider_batters_a_city_and_takes_it_at_zero_hit_points() {
    let rules = rules();
    let mut game = new_game(&rules, Setup::standard(11));
    let (city_tile, from) = open_ground(&game);
    let city = game.debug_found_city(&rules, PlayerId(1), city_tile);
    let raider = game.debug_spawn_unit(&rules, PlayerId(0), from, kind(&rules, "sandbox:unit/raider"));
    assert_eq!(game.view().auth.active_player(), PlayerId(0));

    game.execute(&rules, Command::Attack { unit: raider, target: city_tile }).expect("the attack is valid");
    let auth = game.view().auth;
    let damage = auth.cities().get(city).expect("the city stands").damage;
    assert!(damage > 0 && damage < CITY_MAX_HP, "the city is battered: {damage}");
    let attacker = *auth.units().get(raider).expect("the raider lives").core;
    assert_eq!(attacker.moves_left, 0, "an attack ends the moves");
    assert!(attacker.hp < 100, "the city strikes back");
    let again = game.execute(&rules, Command::Attack { unit: raider, target: city_tile });
    assert!(again.is_err(), "one attack a turn");

    // A new turn of player 0: every player ends one turn (the city mends meanwhile).
    for p in 0..4u8 {
        game.execute(&rules, Sandbox::end_turn(PlayerId(p))).expect("end turn");
    }
    assert!(game.view().auth.cities().get(city).expect("the city stands").damage < damage, "the city mended");
    game.debug_set_city_damage(city, CITY_MAX_HP - 1);
    game.debug_set_unit_hp(raider, 100);
    game.execute(&rules, Command::Attack { unit: raider, target: city_tile }).expect("the second attack");
    let auth = game.view().auth;
    let taken = auth.cities().get(city).expect("the city stands");
    assert_eq!(taken.owner, PlayerId(0), "the city changed hands");
    assert!(!taken.buildings.iter().any(|b| rules.buildings[b.0 as usize].unbuildable), "the seat is gone");
    assert_eq!(auth.units().get(raider).expect("the raider lives").core.tile, city_tile, "the winner moved in");
    game.validate(&rules).expect("the world stays valid");
}
