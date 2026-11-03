//! Simultaneous turns: the commands of seats that act at once are executed in the order the
//! server received them and recorded in that order. Commands that touch nothing in common give
//! one world in every arrival order; commands that conflict resolve by arrival (the first
//! wins and the second is refused by the kernel's own rules); and in every order the server's
//! journal replays to the server's hash.

mod common;

use common::*;
use kernel::ids::{PlayerId, TileId};
use kernel::replay::hash::authoritative_hash;
use kernel::replay::journal::replay;
use kernel::world::state::GameState;
use net::proto::Phase;
use net::server::ServerConfig;
use sandbox_rules::Sandbox;

fn replayed_hash(net: &Net) -> u64 {
    let segment = net.server.segment();
    let replayed = replay(&segment.save, &segment.journal, &net.rules)
        .unwrap_or_else(|e| panic!("the journal does not replay: {e}"));
    authoritative_hash(&replayed.game)
}

/// A seeded generator for the shuffles.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// A random merge of two sequences that keeps the order inside each (a connection delivers its
/// own frames in order; frames of different connections interleave as they like).
fn merge<T: Clone>(a: &[T], b: &[T], rng: &mut Lcg) -> Vec<T> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() || j < b.len() {
        let take_a = j == b.len() || (i < a.len() && rng.next() % 2 == 0);
        if take_a {
            out.push(a[i].clone());
            i += 1;
        } else {
            out.push(b[j].clone());
            j += 1;
        }
    }
    out
}

fn unit_at(game: &GameState<Sandbox>, owner: u8, tile: TileId) -> u64 {
    let auth = game.view().auth;
    auth.units()
        .iter_handles()
        .find(|(_, c)| c.tile == tile && c.owner == PlayerId(owner))
        .map(|(_, c)| c.id.0)
        .expect("the staged unit")
}

/// A land tile whose six neighbours are land with no unit or city anywhere near.
fn quiet_ground(game: &GameState<Sandbox>) -> (TileId, Vec<TileId>) {
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    let busy: Vec<TileId> = auth.units().iter_cores().map(|u| u.tile).collect();
    let near_busy = |t: TileId| {
        busy.iter()
            .any(|b| *b == t || dims.neighbors(t).into_iter().any(|n| n == *b))
    };
    for t in land_tiles(game) {
        let around: Vec<TileId> = dims.neighbors(t).into_iter().collect();
        if around.len() == 6
            && around.iter().all(|n| !auth.map().is_water(*n))
            && !near_busy(t)
            && !around.iter().any(|n| near_busy(*n))
        {
            return (t, around);
        }
    }
    panic!("no quiet ground on the map");
}

fn staged(stage: impl Fn(&mut GameState<Sandbox>, &sandbox_rules::Rules) + 'static) -> Make {
    let base = standard_game();
    Box::new(move |rules, spec| {
        let mut game = base(rules, spec);
        stage(&mut game, rules);
        game
    })
}

fn spearman(rules: &sandbox_rules::Rules) -> kernel::ids::UnitKindId {
    kernel::ids::UnitKindId(kind_named(rules, "spearman"))
}

fn arrange(net: &mut Net, humans: usize) {
    for i in 0..humans {
        net.player(&format!("P{i}"));
    }
    net.start();
}

fn two_seats() -> Net {
    Net::new(spec(2, 2, 5), ServerConfig::default(), standard_game())
}

/// Commands on each seat's own units, which touch nothing the other seat touches.
fn script(net: &Net, seat: usize, with_city: bool) -> Vec<Vec<u8>> {
    let rules = net.rules.clone();
    let own = own_units(&net.clients[seat].client);
    let settler = own
        .iter()
        .find(|u| u.kind == kind_named(&rules, "settler"))
        .unwrap();
    let scout = own
        .iter()
        .find(|u| u.kind == kind_named(&rules, "scout"))
        .unwrap();
    let guard = own
        .iter()
        .find(|u| u.kind == kind_named(&rules, "spearman"))
        .unwrap();
    let game = net.server.game().unwrap();
    let dims = game.view().auth.map().dimensions;
    let step = dims
        .neighbors(TileId(scout.tile))
        .into_iter()
        .find(|t| !game.view().auth.map().is_water(*t))
        .expect("a land neighbour")
        .0;
    let mut commands = vec![fortify(guard.id), move_unit(scout.id, step)];
    if with_city {
        commands.push(found_city(settler.id));
    }
    commands
}

#[test]
fn orders_that_touch_nothing_in_common_give_one_world_in_every_arrival_order() {
    commuting(false);
}

/// Founding a city takes the next entity id, so two cities founded in either order carry the two
/// ids the other way round: the worlds differ in the ids and in nothing else. The journal
/// carries the order, so each replays to its own hash.
#[test]
fn two_cities_founded_in_either_order_differ_in_ids_only() {
    let worlds = commuting(true);
    let distinct: std::collections::BTreeSet<_> = worlds.iter().map(|w| w.0).collect();
    assert_eq!(
        distinct.len(),
        2,
        "two arrival orders of the foundings, two worlds: {worlds:x?}"
    );
    let ids: std::collections::BTreeSet<(u64, u64)> = worlds.iter().map(|w| (w.1, w.2)).collect();
    let ids: Vec<_> = ids.into_iter().collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(
        (ids[0].0, ids[0].1),
        (ids[1].1, ids[1].0),
        "the two cities carry the two ids the other way round"
    );
}

fn commuting(with_city: bool) -> Vec<(u64, u64, u64)> {
    let mut rng = Lcg(42);
    let mut hashes = Vec::new();
    for round in 0..40 {
        let mut net = two_seats();
        arrange(&mut net, 2);
        let (a, b) = (script(&net, 0, with_city), script(&net, 1, with_city));
        let arrival: Vec<(usize, Vec<u8>)> = merge(
            &a.iter().map(|c| (0, c.clone())).collect::<Vec<_>>(),
            &b.iter().map(|c| (1, c.clone())).collect::<Vec<_>>(),
            &mut rng,
        );
        for (seat, bytes) in arrival {
            net.command(seat, bytes);
        }
        for seat in 0..2 {
            assert!(
                net.clients[seat].results().iter().all(|r| r.1),
                "round {round}: every command of seat {seat} is accepted: {:?}",
                net.clients[seat].results()
            );
        }
        // The two ends of the turn arrive in either order too.
        let ends: Vec<usize> = if rng.next() % 2 == 0 {
            vec![0, 1]
        } else {
            vec![1, 0]
        };
        net.round_after_end(&ends);
        assert_eq!(
            replayed_hash(&net),
            net.server.hash().unwrap(),
            "arrival order {round}"
        );
        let auth = net.server.game().unwrap().view().auth;
        let city_id = |p: u8| {
            auth.indexes()
                .cities_of(pid(p))
                .first()
                .and_then(|h| auth.cities().get(*h))
                .map_or(0, |c| c.id.0)
        };
        hashes.push((net.server.hash().unwrap(), city_id(0), city_id(1)));
    }
    if !with_city {
        assert!(hashes.windows(2).all(|w| w[0].0 == w[1].0), "the world does not depend on how the frames of two connections interleave: {hashes:x?}");
    }
    hashes
}

/// Two orders that cannot both stand: the first to arrive is executed, the second is refused by the
/// rules (an enemy on the tile, a city too near, nothing left to attack).
fn race(
    stage: impl Fn(&mut GameState<Sandbox>, &sandbox_rules::Rules) + 'static + Clone,
    orders: impl Fn(&Net) -> [Vec<u8>; 2],
) -> [(u64, [bool; 2]); 2] {
    let mut outcome = Vec::new();
    for swapped in [false, true] {
        let mut net = Net::new(
            spec(2, 2, 5),
            ServerConfig::default(),
            staged(stage.clone()),
        );
        arrange(&mut net, 2);
        let [for_a, for_b] = orders(&net);
        let arrival = if swapped {
            [(1, for_b), (0, for_a)]
        } else {
            [(0, for_a), (1, for_b)]
        };
        let mut seqs = [0, 0];
        for (seat, bytes) in arrival {
            seqs[seat] = net.command(seat, bytes);
        }
        let ok = |net: &Net, seat: usize| {
            net.clients[seat]
                .results()
                .into_iter()
                .find(|r| r.0 == seqs[seat])
                .map(|r| r.1)
                .unwrap()
        };
        let oks = [ok(&net, 0), ok(&net, 1)];
        net.round_after_end(&[0, 1]);
        assert_eq!(
            replayed_hash(&net),
            net.server.hash().unwrap(),
            "swapped={swapped}"
        );
        outcome.push((net.server.hash().unwrap(), oks));
    }
    [outcome[0], outcome[1]]
}

#[test]
fn two_units_ordered_onto_the_same_tile_the_first_arrival_gets_it() {
    let [first, swapped] = race(
        |game, rules| {
            let (target, around) = quiet_ground(game);
            let _ = target;
            game.debug_spawn_unit(rules, PlayerId(0), around[0], spearman(rules));
            game.debug_spawn_unit(rules, PlayerId(1), around[3], spearman(rules));
        },
        |net| {
            let game = net.server.game().unwrap();
            let (target, around) = quiet_ground_after(game);
            [
                move_unit(unit_at(game, 0, around[0]), target.0),
                move_unit(unit_at(game, 1, around[3]), target.0),
            ]
        },
    );
    assert_eq!(first.1, [true, false], "seat 0 arrived first");
    assert_eq!(swapped.1, [false, true], "seat 1 arrived first");
    assert_ne!(first.0, swapped.0, "the order decided the world");
}

/// The staged tile again, after the two spearmen stand on its neighbours: the same search finds
/// the same ground because the staged units are on the ring, not the centre.
fn quiet_ground_after(game: &GameState<Sandbox>) -> (TileId, Vec<TileId>) {
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    // The centre is the tile all the staged spearmen are adjacent to.
    let spearmen: Vec<TileId> = auth
        .units()
        .iter_cores()
        .filter(|u| u.owner.0 < 2)
        .map(|u| u.tile)
        .collect();
    for t in land_tiles(game) {
        let around: Vec<TileId> = dims.neighbors(t).into_iter().collect();
        if around.len() == 6
            && around[0] != around[3]
            && spearmen.contains(&around[0])
            && spearmen.contains(&around[3])
            && !auth.units().iter_cores().any(|u| u.tile == t)
        {
            return (t, around);
        }
    }
    panic!("the staged ground");
}

#[test]
fn two_cities_founded_too_close_together_the_first_arrival_gets_the_ground() {
    let rules = rules();
    let settler = kernel::ids::UnitKindId(kind_named(&rules, "settler"));
    let [first, swapped] = race(
        move |game, rules| {
            let (target, around) = quiet_ground(game);
            let _ = target;
            game.debug_spawn_unit(rules, PlayerId(0), around[0], settler);
            game.debug_spawn_unit(rules, PlayerId(1), around[1], settler);
        },
        |net| {
            let game = net.server.game().unwrap();
            let settlers: Vec<(u8, u64)> = game
                .view()
                .auth
                .units()
                .iter_cores()
                .filter(|u| u.owner.0 < 2 && u.kind.0 == kind_named(&net.rules, "settler"))
                .map(|u| (u.owner.0, u.id.0))
                .collect();
            // The staged settlers are the ones standing next to each other: found where they stand.
            let near = |owner: u8| {
                let auth = game.view().auth;
                settlers
                    .iter()
                    .filter(|(o, _)| *o == owner)
                    .map(|(_, id)| *id)
                    .find(|id| {
                        let core = auth.units().iter_cores().find(|c| c.id.0 == *id).unwrap();
                        auth.units().iter_cores().any(|o| {
                            o.owner.0 != owner
                                && o.owner.0 < 2
                                && o.kind == core.kind
                                && auth
                                    .map()
                                    .dimensions
                                    .neighbors(core.tile)
                                    .into_iter()
                                    .any(|n| n == o.tile)
                        })
                    })
                    .expect("a staged settler")
            };
            [found_city(near(0)), found_city(near(1))]
        },
    );
    assert_eq!(first.1, [true, false]);
    assert_eq!(swapped.1, [false, true]);
    assert_ne!(first.0, swapped.0);
}

#[test]
fn two_attackers_on_one_unit_the_first_kills_it_and_the_second_hits_the_winner() {
    let [first, swapped] = race(
        |game, rules| {
            let (target, around) = quiet_ground(game);
            game.debug_spawn_unit(rules, PlayerId(0), around[0], spearman(rules));
            game.debug_spawn_unit(rules, PlayerId(1), around[3], spearman(rules));
            let victim = game.debug_spawn_unit(rules, PlayerId(2), target, spearman(rules));
            game.debug_set_unit_hp(victim, 1);
        },
        |net| {
            let game = net.server.game().unwrap();
            let (target, around) = quiet_ground_after_victim(game);
            [
                attack(unit_at(game, 0, around[0]), target.0),
                attack(unit_at(game, 1, around[3]), target.0),
            ]
        },
    );
    // The winner advances into the tile it cleared (sandbox combat), so the second attacker hits
    // the winner: the first arrival's unit is hurt by the second.
    assert_eq!(first.1, [true, true]);
    assert_eq!(swapped.1, [true, true]);
    assert_ne!(first.0, swapped.0, "who struck first decided who was hurt");
}

fn quiet_ground_after_victim(game: &GameState<Sandbox>) -> (TileId, Vec<TileId>) {
    let auth = game.view().auth;
    let dims = auth.map().dimensions;
    let victim = auth
        .units()
        .iter_cores()
        .find(|u| u.owner.0 == 2 && u.hp == 1)
        .expect("the staged victim")
        .tile;
    (victim, dims.neighbors(victim).into_iter().collect())
}

#[test]
fn a_late_command_after_the_window_closed_is_not_played_and_the_round_is_unharmed() {
    let mut net = two_seats();
    arrange(&mut net, 2);
    let guard = own_units(&net.clients[0].client)
        .into_iter()
        .find(|u| u.kind == kind_named(&net.rules, "spearman"))
        .unwrap();
    net.end_turn(0);
    net.end_turn(1);
    // The window is closed now: the same command is refused even if it was in flight.
    let seq = net.command(0, fortify(guard.id));
    let r = net.clients[0]
        .results()
        .into_iter()
        .find(|r| r.0 == seq)
        .unwrap();
    assert!(!r.1, "{r:?}");
    assert!(net.run_until(2_000, |n| n.server.phase() == Phase::Planning
        && n.server.round() == 1));
    assert_eq!(replayed_hash(&net), net.server.hash().unwrap());
}
