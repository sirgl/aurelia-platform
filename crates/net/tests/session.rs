//! A match end to end through the sans-IO server and scripted clients: the door (versions, code,
//! tokens), the lobby, a round of two humans acting at once, and the proof that the server's
//! journal replays to its hash.

mod common;

use common::*;
use kernel::replay::hash::authoritative_hash;
use kernel::replay::journal::replay;
use net::client::{hello_for, Event};
use net::proto::{Phase, RejectCode, Role, Settings};
use net::server::ServerConfig;

fn two_humans_two_ai() -> Net {
    let mut net = Net::new(spec(2, 2, 11), ServerConfig::default(), standard_game());
    net.player("Ada");
    net.player("Bob");
    net
}

/// The journal of the segment being played, replayed from its save on fresh rules.
pub fn replayed_hash(net: &Net) -> u64 {
    let segment = net.server.segment();
    let replayed = replay(&segment.save, &segment.journal, &net.rules)
        .unwrap_or_else(|e| panic!("the server's journal does not replay: {e}"));
    authoritative_hash(&replayed.game)
}

#[test]
fn a_client_of_another_build_or_other_rules_is_refused_at_the_door() {
    let cases: Vec<(&str, Box<dyn Fn(&mut net::proto::Hello)>)> = vec![
        ("protocol", Box::new(|h| h.proto += 1)),
        ("module", Box::new(|h| h.module = "classic".into())),
        ("save layout", Box::new(|h| h.module_version += 1)),
        ("command layout", Box::new(|h| h.command_codec += 1)),
        ("rules", Box::new(|h| h.rules_hash ^= 1)),
    ];
    for (what, mutate) in cases {
        let mut net = Net::new(spec(2, 2, 11), ServerConfig::default(), standard_game());
        let mut hello = hello_for(&net.rules, "Eve", CODE, Role::Player, None);
        mutate(&mut hello);
        let i = net.join_with(hello);
        let (code, text) = net.clients[i]
            .refused()
            .unwrap_or_else(|| panic!("{what}: refused"));
        assert_eq!(code, RejectCode::VersionMismatch, "{what}: {text}");
        assert!(net.clients[i].closed, "{what}: the connection is closed");
        assert!(net.clients[i].client.part.is_none(), "{what}: no seat");
    }
    // The right build with the wrong code, and a token nobody holds.
    let mut net = Net::new(spec(2, 2, 11), ServerConfig::default(), standard_game());
    let wrong = hello_for(&net.rules, "Eve", "NOPE", Role::Player, None);
    let i = net.join_with(wrong);
    assert_eq!(
        net.clients[i].refused().map(|r| r.0),
        Some(RejectCode::WrongCode)
    );
    let forged = net.join(&"Eve".to_owned(), Role::Player, Some([7; 16]));
    assert_eq!(
        net.clients[forged].refused().map(|r| r.0),
        Some(RejectCode::BadToken)
    );
}

#[test]
fn garbage_frames_drop_the_connection_and_nothing_else() {
    let mut net = two_humans_two_ai();
    let stranger = net.clients.len();
    let conn = 99;
    net.server.connected(conn, net.now);
    net.server.receive(conn, &[0xff, 0xff, 1, 2, 3], net.now);
    net.server.receive(conn, &[], net.now);
    let outs = net.server.drain();
    assert!(outs.iter().any(|o| o.conn == conn && o.close));
    let _ = stranger;
    // A command before hello is refused too.
    net.server.connected(100, net.now);
    net.server
        .receive(100, &net::proto::ClientMsg::Ping(1).encode(), net.now);
    assert!(net.server.drain().iter().any(|o| o.conn == 100 && o.close));
    assert_eq!(net.server.phase(), Phase::Lobby);
}

#[test]
fn the_lobby_waits_for_the_ready_check_and_only_the_host_starts() {
    let mut net = two_humans_two_ai();
    net.send(1, &net::proto::ClientMsg::Start.encode());
    assert_eq!(net.server.phase(), Phase::Lobby, "a guest cannot start");
    net.send(0, &net::proto::ClientMsg::Ready(true).encode());
    net.send(0, &net::proto::ClientMsg::Start.encode());
    assert_eq!(net.server.phase(), Phase::Lobby, "Bob is not ready");
    assert!(net.clients[0]
        .events
        .iter()
        .any(|e| matches!(e, Event::ChatRefused(t) if t.contains("Bob"))));
    net.start();
    assert_eq!(net.server.phase(), Phase::Planning);
    assert!(net.clients[0].client.host && !net.clients[1].client.host);
    assert_eq!(net.clients[0].client.part, Some(0));
    assert_eq!(net.clients[1].client.part, Some(1));
}

#[test]
fn each_seat_gets_its_own_view_and_only_its_own() {
    let mut net = two_humans_two_ai();
    net.start();
    for i in 0..2 {
        let view = net.clients[i].client.view.as_ref().expect("a snapshot");
        assert_eq!(view.seat as usize, i);
        let own = own_units(&net.clients[i].client);
        assert!(own.len() >= 2, "a seat starts with units");
        assert!(own.iter().all(|u| u.owner as usize == i));
        // The seat sees only itself at the start: the starts are far apart.
        assert!(
            view.units.iter().all(|u| u.owner as usize == i),
            "no foreign unit in the first view"
        );
    }
    let a: Vec<u64> = own_units(&net.clients[0].client)
        .iter()
        .map(|u| u.id)
        .collect();
    let b: Vec<u64> = own_units(&net.clients[1].client)
        .iter()
        .map(|u| u.id)
        .collect();
    assert!(a.iter().all(|id| !b.contains(id)));
}

#[test]
fn two_humans_act_at_the_same_time_and_the_round_resolves_when_both_end_it() {
    let mut net = two_humans_two_ai();
    net.start();
    let round = net.server.round();
    let rules = net.rules.clone();
    let settler = kind_named(&rules, "settler");
    // Both found a city in the same round, interleaved, with no turn order between them.
    let a = own_units(&net.clients[0].client)
        .into_iter()
        .find(|u| u.kind == settler)
        .expect("Ada's settler");
    let b = own_units(&net.clients[1].client)
        .into_iter()
        .find(|u| u.kind == settler)
        .expect("Bob's settler");
    let sa = net.command(0, found_city(a.id));
    let sb = net.command(1, found_city(b.id));
    net.tick(100);
    assert_eq!(net.clients[0].results(), vec![(sa, true, String::new())]);
    assert_eq!(net.clients[1].results(), vec![(sb, true, String::new())]);
    let game = net.server.game().unwrap();
    assert_eq!(
        game.view().auth.active_player().0,
        0,
        "the turn pointer did not move during planning"
    );
    assert_eq!(game.view().auth.indexes().cities_of(pid(0)).len(), 1);
    assert_eq!(game.view().auth.indexes().cities_of(pid(1)).len(), 1);

    // Ada ends; the round stays open for Bob.
    net.end_turn(0);
    net.tick(100);
    assert_eq!(net.server.phase(), Phase::Planning);
    assert_eq!(net.clients[1].client.ready, vec![0]);
    let late = net.command(0, fortify(own_units(&net.clients[0].client)[0].id));
    assert_eq!(
        net.clients[0].results().last(),
        Some(&(late, false, "you ended the turn".to_owned()))
    );
    net.round_after_end(&[1]);
    assert_eq!(net.server.round(), round + 1);
    assert!(net.server.metrics().rounds >= 1);
    // The AI seats played their turns in the resolution.
    let game = net.server.game().unwrap();
    assert!(
        game.view().auth.indexes().units_of(pid(2)).len()
            + game.view().auth.indexes().cities_of(pid(2)).len()
            > 0
    );
    // Both clients are in step with the server, and the journal replays to the server's hash.
    assert_eq!(net.clients[0].client.round, round + 1);
    assert_eq!(replayed_hash(&net), net.server.hash().unwrap());
}

#[test]
fn a_seat_cannot_order_what_is_not_its_own_or_end_a_turn() {
    let mut net = two_humans_two_ai();
    net.start();
    let theirs = own_units(&net.clients[1].client)[0].id;
    let mine = own_units(&net.clients[0].client)[0].id;
    let tile = net.clients[0]
        .client
        .view
        .as_ref()
        .unwrap()
        .units
        .iter()
        .find(|u| u.id == mine)
        .unwrap()
        .tile;
    let before = net.server.hash();
    let cases = vec![
        ("another seat's unit", move_unit(theirs, tile + 1)),
        ("ending a turn", end_turn_bytes(0)),
        ("ending another seat's turn", end_turn_bytes(1)),
        ("a unit that does not exist", fortify(9_999_999)),
        ("trailing bytes", [fortify(mine), vec![0]].concat()),
        ("an unknown tag", vec![0, 200]),
        ("nothing", vec![]),
        ("too long", vec![0; 5000]),
    ];
    for (what, bytes) in cases {
        let seq = net.command(0, bytes);
        let result = net.clients[0]
            .results()
            .into_iter()
            .find(|r| r.0 == seq)
            .unwrap();
        assert!(!result.1, "{what} must be refused");
    }
    assert_eq!(net.server.hash(), before, "refused commands change nothing");
    assert_eq!(net.server.round(), 0);
}

#[test]
fn spectators_and_a_resolving_round_take_no_commands() {
    let mut net = two_humans_two_ai();
    let spec = net.join("Watcher", Role::Spectator, None);
    net.start();
    let unit = own_units(&net.clients[0].client)[0].id;
    let seq = net.command(spec, fortify(unit));
    let result = net.clients[spec]
        .results()
        .into_iter()
        .find(|r| r.0 == seq)
        .unwrap();
    assert!(!result.1);
    assert!(
        net.clients[spec].client.view.is_none(),
        "a spectator sees no seat's map by default"
    );
    net.end_turn(0);
    net.end_turn(1);
    let seq = net.command(0, fortify(unit));
    // Whatever phase the server is in right after the last end of turn, the command is not played.
    net.tick(0);
    let ok = net.clients[0]
        .results()
        .into_iter()
        .find(|r| r.0 == seq)
        .map(|r| r.1);
    assert_eq!(ok, Some(false));
}

#[test]
fn the_round_timer_closes_a_round_whoever_has_not_ended_the_turn() {
    let cfg = ServerConfig {
        round_timer_ms: Some(30_000),
        ..ServerConfig::default()
    };
    let mut net = Net::new(spec(2, 2, 11), cfg, standard_game());
    net.player("Ada");
    net.player("Bob");
    net.start();
    let round = net.server.round();
    net.end_turn(0);
    net.tick(10_000);
    assert_eq!(net.server.phase(), Phase::Planning, "Bob has time left");
    assert!(net.clients[0].client.closes_in_ms.is_some());
    net.tick(25_000);
    assert!(net.run_until(2_000, |n| n.server.round() > round
        && n.server.phase() == Phase::Planning));
    assert_eq!(replayed_hash(&net), net.server.hash().unwrap());
}

fn set(net: &mut Net, who: usize, settings: Settings) {
    let frame = net.clients[who].client.settings_frame(settings);
    net.send(who, &frame);
}

fn refusals(net: &Net, who: usize) -> Vec<String> {
    net.clients[who]
        .events
        .iter()
        .filter_map(|e| match e {
            Event::ChatRefused(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_host_sets_the_round_timer_in_the_lobby_and_everybody_hears_it() {
    let mut net = two_humans_two_ai();
    // Default: the round waits for every human.
    assert_eq!(net.server.settings().round_timer_secs, None);
    assert_eq!(net.clients[0].client.settings, Settings::default());
    let on = Settings {
        round_timer_secs: Some(90),
        spectators_see_all: false,
    };
    // Bob is not the host: refused, nothing changes.
    set(&mut net, 1, on);
    assert_eq!(net.server.settings(), Settings::default());
    assert!(refusals(&net, 1).iter().any(|t| t.contains("host")));
    // The host's range is checked.
    for secs in [0, 4, 3601] {
        set(
            &mut net,
            0,
            Settings {
                round_timer_secs: Some(secs),
                ..on
            },
        );
        assert_eq!(
            net.server.settings().round_timer_secs,
            None,
            "{secs} s is out of range"
        );
    }
    assert_eq!(refusals(&net, 0).len(), 3);
    // The host's own setting reaches both players.
    set(&mut net, 0, on);
    assert_eq!(net.server.settings(), on);
    for who in 0..2 {
        assert_eq!(
            net.clients[who].client.settings, on,
            "player {who} was told"
        );
    }
    // A player who joins later is told the settings in force.
    let late = net.join("Eve", Role::Spectator, None);
    assert_eq!(net.clients[late].client.settings, on);
    net.start();
    assert!(
        net.clients[0]
            .client
            .closes_in_ms
            .is_some_and(|ms| ms > 80_000 && ms <= 90_000),
        "{:?}",
        net.clients[0].client.closes_in_ms
    );
    // Turned off during the game: the next round waits for everybody again.
    set(
        &mut net,
        0,
        Settings {
            round_timer_secs: None,
            ..on
        },
    );
    assert_eq!(net.server.settings().round_timer_secs, None);
    net.end_turn(0);
    net.end_turn(1);
    assert!(net.run_until(2_000, |n| n.server.round() == 1
        && n.server.phase() == Phase::Planning));
    assert_eq!(
        net.clients[0].client.closes_in_ms, None,
        "no timer in the new round"
    );
    assert_eq!(replayed_hash(&net), net.server.hash().unwrap());
}

#[test]
fn when_the_timer_runs_out_the_ai_plays_the_seats_that_did_not_end_their_turn() {
    let mut net = two_humans_two_ai();
    set(
        &mut net,
        0,
        Settings {
            round_timer_secs: Some(30),
            spectators_see_all: false,
        },
    );
    net.start();
    let round = net.server.round();
    net.end_turn(0);
    net.tick(29_000);
    assert_eq!(
        net.server.phase(),
        Phase::Planning,
        "Bob still has a second"
    );
    net.tick(2_000);
    assert!(
        net.clients[0]
            .chat_texts()
            .iter()
            .any(|t| t.contains("the AI plays this round for Bob")),
        "{:?}",
        net.clients[0].chat_texts()
    );
    assert!(
        !net.clients[0]
            .chat_texts()
            .iter()
            .any(|t| t.contains("for Ada")),
        "Ada ended her turn and plays herself"
    );
    assert!(net.run_until(2_000, |n| n.server.round() > round
        && n.server.phase() == Phase::Planning));
    // The next round is Bob's again.
    assert!(!net.server.ai_plays(1) && !net.server.ai_plays(0));
    assert_eq!(replayed_hash(&net), net.server.hash().unwrap());
}

#[test]
fn spectators_see_the_open_map_only_if_the_host_asks_in_the_lobby() {
    // Default: a spectator gets no map (chat and the status line only).
    let mut net = two_humans_two_ai();
    let eve = net.join("Eve", Role::Spectator, None);
    assert!(!net.server.settings().spectators_see_all);
    net.start();
    assert!(net.clients[eve].client.view.is_none());

    // The host turns it on in the lobby; a player cannot.
    let mut net = two_humans_two_ai();
    let eve = net.join("Eve", Role::Spectator, None);
    let on = Settings {
        round_timer_secs: None,
        spectators_see_all: true,
    };
    set(&mut net, 1, on);
    assert!(!net.server.settings().spectators_see_all, "Bob is no host");
    set(&mut net, 0, on);
    assert!(
        net.clients[eve].client.settings.spectators_see_all,
        "the spectator was told"
    );
    net.start();
    let view = net.clients[eve]
        .client
        .view
        .as_ref()
        .expect("the spectator has a view now");
    use net::view::TileView;
    assert!(
        view.tiles.iter().all(|t| matches!(t, TileView::Visible(_))),
        "the whole map is open"
    );
    assert!(
        net.clients[0]
            .client
            .view
            .as_ref()
            .unwrap()
            .tiles
            .iter()
            .any(|t| matches!(t, TileView::Unknown)),
        "the players' own views are unchanged"
    );

    // In a live game it cannot be changed, either way.
    set(
        &mut net,
        0,
        Settings {
            spectators_see_all: false,
            ..on
        },
    );
    assert!(net.server.settings().spectators_see_all);
    assert!(refusals(&net, 0).iter().any(|t| t.contains("lobby")));
    let mut net = two_humans_two_ai();
    net.start();
    set(&mut net, 0, on);
    assert!(
        !net.server.settings().spectators_see_all,
        "refused once the match runs"
    );
}
