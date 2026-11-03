//! Hidden state never reaches a client: what a seat is sent is a function of what the seat may
//! know. Three checks: the view agrees with the AI's observation and with the visibility rules
//! unit by unit; a change in the world that the seat cannot see leaves its bytes untouched; and
//! the view a client builds from the snapshot and the deltas is the view the server would build.

mod common;

use common::*;
use kernel::ai::observation;
use kernel::ids::{PlayerId, TileId};
use kernel::knowledge::view::is_visible;
use kernel::metrics::WorkCounters;
use kernel::world::state::GameState;
use net::proto::ServerMsg;
use net::server::ServerConfig;
use net::view::{SeatView, TileView};
use sandbox_rules::Sandbox;

fn game(seed: u64) -> GameState<Sandbox> {
    let rules = rules();
    standard_game()(&rules, &spec(2, 2, seed))
}

fn bytes(view: &SeatView) -> Vec<u8> {
    ServerMsg::Snapshot(view.clone()).encode()
}

fn view(game: &GameState<Sandbox>, seat: u8) -> SeatView {
    SeatView::build(game, &rules(), PlayerId(seat))
}

#[test]
fn the_view_is_the_ai_observation_and_the_visibility_rules_and_nothing_more() {
    let rules = rules();
    let mut net = Net::new(spec(2, 2, 14), ServerConfig::default(), standard_game());
    net.player("Ada");
    net.player("Bob");
    net.start();
    // Some play, so that units have met and tiles are remembered.
    for _ in 0..4 {
        for seat in 0..2 {
            for unit in own_units(&net.clients[seat].client) {
                let game = net.server.game().unwrap();
                let step = game
                    .view()
                    .auth
                    .map()
                    .dimensions
                    .neighbors(TileId(unit.tile))
                    .into_iter()
                    .nth(seat + 1)
                    .unwrap()
                    .0;
                net.command(seat, move_unit(unit.id, step));
            }
        }
        net.round_after_end(&[0, 1]);
    }
    let game = net.server.game().unwrap();
    let auth = game.view().auth;
    for seat in 0..4u8 {
        let me = PlayerId(seat);
        let v = view(game, seat);
        let pv = game.player_view(&rules, me);
        let (own, foreign) = observation::units(auth, &pv, &mut WorkCounters::default());
        // Units: exactly the AI's own and observed ones.
        let mut ids: Vec<u64> = v.units.iter().map(|u| u.id).collect();
        let mut expected: Vec<u64> = own
            .iter()
            .map(|u| u.id.0)
            .chain(
                foreign
                    .iter()
                    .map(|u| auth.units().get(u.handle).unwrap().core.id.0),
            )
            .collect();
        ids.sort();
        expected.sort();
        assert_eq!(ids, expected, "seat {seat}: units");
        // And against the visibility rules themselves: a foreign unit is in the view iff it stands on a tile in sight.
        for core in auth.units().iter_cores() {
            let listed = v.units.iter().any(|u| u.id == core.id.0);
            let expected = core.owner == me || is_visible(game.derived(), me, core.tile);
            assert_eq!(
                listed, expected,
                "seat {seat}: unit {} of {:?} at {:?}",
                core.id.0, core.owner, core.tile
            );
        }
        for unit in &v.units {
            assert_eq!(
                unit.own.is_some(),
                unit.owner == seat,
                "only a seat's own units carry its details"
            );
        }
        // Tiles: unknown tiles say nothing; a foreign unit or city stands on a tile that is in sight.
        for unit in v.units.iter().filter(|u| u.owner != seat) {
            assert!(
                matches!(v.tiles[unit.tile as usize], TileView::Visible(_)),
                "seat {seat}: a foreign unit on a tile out of sight"
            );
        }
        for city in &v.cities {
            let tile = &v.tiles[city.tile as usize];
            assert!(
                !matches!(tile, TileView::Unknown),
                "seat {seat}: a city on an unknown tile"
            );
            assert_eq!(
                city.seen_now.is_some(),
                matches!(tile, TileView::Visible(_)) || city.owner == seat && city.own.is_some()
            );
            if city.owner != seat {
                assert!(
                    city.own.is_none(),
                    "another player's city shows no production queue"
                );
            }
        }
        for (i, tile) in v.tiles.iter().enumerate() {
            assert_eq!(
                matches!(tile, TileView::Visible(_)),
                is_visible(game.derived(), me, TileId(i as u32)),
                "seat {seat}: tile {i}"
            );
        }
        // Another player's treasury, techs and research are not in the view: only the seat's own.
        assert_eq!(v.me.treasury, auth.players().treasury_of(me));
    }
}

#[test]
fn what_a_seat_cannot_see_does_not_change_a_byte_of_its_view() {
    let rules = rules();
    let base = game(14);
    let seat0 = view(&base, 0);
    let dark: Vec<TileId> = seat0
        .tiles
        .iter()
        .enumerate()
        .filter(|(_, t)| matches!(t, TileView::Unknown))
        .map(|(i, _)| TileId(i as u32))
        .collect();
    assert!(
        dark.len() > seat0.tiles.len() / 2,
        "most of the map is unknown at the start"
    );
    let land_dark: Vec<TileId> = dark
        .iter()
        .copied()
        .filter(|t| {
            !base.view().auth.map().is_water(*t)
                && base.view().auth.units().iter_cores().all(|u| u.tile != *t)
        })
        .collect();
    let before = bytes(&seat0);
    let spearman = kernel::ids::UnitKindId(kind_named(&rules, "spearman"));

    let mut changed = base.clone();
    changed.debug_spawn_unit(&rules, PlayerId(2), land_dark[0], spearman);
    changed.debug_spawn_unit(
        &rules,
        PlayerId(3),
        land_dark[land_dark.len() / 2],
        spearman,
    );
    changed.debug_found_city(&rules, PlayerId(1), land_dark[land_dark.len() - 1]);
    changed.debug_earn(PlayerId(1), 500);
    changed.debug_earn(PlayerId(2), 90);
    assert_ne!(
        kernel::replay::hash::authoritative_hash(&base),
        kernel::replay::hash::authoritative_hash(&changed),
        "the worlds do differ"
    );
    assert_eq!(
        bytes(&view(&changed, 0)),
        before,
        "hidden units, a hidden city and other players' gold leave seat 0's view as it was"
    );

    // The same changes seen from the seat whose property they are: that is a difference.
    assert_ne!(
        bytes(&view(&changed, 1)),
        bytes(&view(&base, 1)),
        "the control: its own gold and city do show"
    );

    // A unit placed next to seat 0's own does show: the check can fail.
    let mut seen = base.clone();
    let mine = base
        .view()
        .auth
        .units()
        .iter_cores()
        .find(|u| u.owner == PlayerId(0))
        .unwrap()
        .tile;
    let next = base
        .view()
        .auth
        .map()
        .dimensions
        .neighbors(mine)
        .into_iter()
        .find(|t| {
            !base.view().auth.map().is_water(*t)
                && base.view().auth.units().iter_cores().all(|u| u.tile != *t)
        })
        .unwrap();
    seen.debug_spawn_unit(&rules, PlayerId(2), next, spearman);
    assert_ne!(bytes(&view(&seen, 0)), before);
}

#[test]
fn deltas_rebuild_the_view_the_server_would_build() {
    let rules = rules();
    let mut net = Net::new(spec(2, 2, 14), ServerConfig::default(), standard_game());
    net.player("Ada");
    net.player("Bob");
    net.start();
    let mut delta_bytes = 0;
    for _ in 0..6 {
        for seat in 0..2 {
            for unit in own_units(&net.clients[seat].client) {
                let game = net.server.game().unwrap();
                let step = game
                    .view()
                    .auth
                    .map()
                    .dimensions
                    .neighbors(TileId(unit.tile))
                    .into_iter()
                    .nth(seat + 2)
                    .unwrap()
                    .0;
                net.command(seat, move_unit(unit.id, step));
            }
            net.tick(60);
            let built = SeatView::build(net.server.game().unwrap(), &rules, PlayerId(seat as u8));
            assert_eq!(
                net.clients[seat].client.view.as_ref(),
                Some(&built),
                "mid-round, seat {seat}"
            );
        }
        net.round_after_end(&[0, 1]);
        for seat in 0..2 {
            let built = SeatView::build(net.server.game().unwrap(), &rules, PlayerId(seat as u8));
            assert_eq!(
                net.clients[seat].client.view.as_ref(),
                Some(&built),
                "at the round's start, seat {seat}"
            );
        }
        delta_bytes = net.server.metrics().delta_bytes;
    }
    assert!(net.server.metrics().deltas > 6 && delta_bytes > 0);
}
