//! A seat whose player leaves is played by the AI and comes back to its player; late joiners;
//! a match resumed from its save.

mod common;

use common::*;
use kernel::replay::hash::authoritative_hash;
use kernel::replay::journal::replay;
use net::proto::{Phase, RejectCode, Role};
use net::server::{Server, ServerConfig};

fn replayed_hash(net: &Net) -> u64 {
    let segment = net.server.segment();
    let replayed = replay(&segment.save, &segment.journal, &net.rules)
        .unwrap_or_else(|e| panic!("the journal does not replay: {e}"));
    authoritative_hash(&replayed.game)
}

fn cities(net: &Net, seat: u8) -> usize {
    net.server
        .game()
        .unwrap()
        .view()
        .auth
        .indexes()
        .cities_of(pid(seat))
        .len()
}

fn two_humans() -> Net {
    let mut net = Net::new(spec(2, 2, 21), ServerConfig::default(), standard_game());
    net.player("Ada");
    net.player("Bob");
    net.start();
    net
}

#[test]
fn a_player_who_leaves_is_replaced_by_the_ai_for_the_round_and_gets_the_seat_back() {
    let mut net = two_humans();
    assert_eq!(cities(&net, 1), 0);
    net.drop_client(1);
    assert_eq!(
        net.server.phase(),
        Phase::Planning,
        "the round waits for Ada, not for Bob"
    );
    assert!(
        !net.server.ai_plays(1),
        "until the round closes the seat is still Bob's"
    );
    net.round_after_end(&[0]);
    assert_eq!(
        cities(&net, 1),
        1,
        "the AI played Bob's turn: it founded his capital"
    );
    assert_eq!(
        replayed_hash(&net),
        net.server.hash().unwrap(),
        "the AI's turn is in the journal"
    );

    // Bob comes back with his token: the new round is his again, with his city on his map.
    let back = net.rejoin(1, "Bob");
    assert_eq!(net.clients[back].client.part, Some(1));
    assert!(net.clients[back]
        .client
        .view
        .as_ref()
        .unwrap()
        .cities
        .iter()
        .any(|c| c.owner == 1 && c.own.is_some()));
    assert!(!net.server.ai_plays(1));
    let guard = own_units(&net.clients[back].client)
        .into_iter()
        .find(|u| u.kind == kind_named(&net.rules, "spearman"))
        .unwrap();
    let seq = net.command(back, fortify(guard.id));
    net.tick(50);
    assert!(
        net.clients[back]
            .results()
            .iter()
            .any(|r| r.0 == seq && r.1),
        "{:?}",
        net.clients[back].results()
    );
    net.round_after_end(&[0, back]);
    assert_eq!(replayed_hash(&net), net.server.hash().unwrap());
}

#[test]
fn a_player_who_comes_back_before_the_round_closes_plays_it_himself() {
    let mut net = two_humans();
    net.drop_client(1);
    let back = net.rejoin(1, "Bob");
    assert!(!net.server.ai_plays(1));
    net.round_after_end(&[0, back]);
    assert_eq!(cities(&net, 1), 0, "nobody founded Bob's city for him");
}

#[test]
fn a_player_who_ended_the_turn_and_left_is_not_played_by_the_ai() {
    let mut net = two_humans();
    net.end_turn(1);
    net.drop_client(1);
    net.round_after_end(&[0]);
    assert_eq!(
        cities(&net, 1),
        0,
        "he had ended his turn: the AI adds nothing to it"
    );
    assert_eq!(net.server.round(), 1);
}

#[test]
fn the_match_waits_when_nobody_is_there_and_goes_on_when_somebody_returns() {
    let mut net = two_humans();
    net.drop_client(0);
    net.drop_client(1);
    assert!(
        net.run_until(500, |n| n.server.phase() == Phase::Paused),
        "{:?}",
        net.server.phase()
    );
    let round = net.server.round();
    assert!(round >= 1, "the AI finished the round it was left with");
    net.tick(5_000);
    assert_eq!(net.server.round(), round, "and no further");
    let back = net.rejoin(0, "Ada");
    assert!(net.run_until(500, |n| n.server.phase() == Phase::Planning));
    assert!(!net.server.ai_plays(0));
    assert!(net.server.ai_plays(1), "Bob is still away");
    net.round_after_end(&[back]);
    assert_eq!(net.server.round(), round + 1);
    assert_eq!(replayed_hash(&net), net.server.hash().unwrap());
}

#[test]
fn a_second_connection_with_the_same_token_takes_the_seat_over() {
    let mut net = two_humans();
    let again = net.rejoin(1, "Bob on the phone");
    assert!(net.clients[1].closed, "the first connection is dropped");
    assert!(net.clients[again].client.part == Some(1));
    // A stranger cannot take it without the token.
    let thief = net.join("Mallory", Role::Player, None);
    assert_eq!(
        net.clients[thief].refused().map(|r| r.0),
        Some(RejectCode::Full)
    );
}

#[test]
fn a_seat_nobody_took_is_played_by_the_ai_until_a_late_joiner_takes_it() {
    let mut net = Net::new(spec(3, 1, 21), ServerConfig::default(), standard_game());
    net.player("Ada");
    net.player("Bob");
    net.start();
    assert!(net.server.ai_plays(2), "seat 2 is open");
    let carol = net.player("Carol");
    assert_eq!(net.clients[carol].client.part, Some(2));
    assert!(
        !net.server.ai_plays(2),
        "her turn has not come: she plays this round"
    );
    let settler = own_units(&net.clients[carol].client)
        .into_iter()
        .find(|u| u.kind == kind_named(&net.rules, "settler"))
        .unwrap();
    let seq = net.command(carol, found_city(settler.id));
    net.tick(50);
    assert!(net.clients[carol]
        .results()
        .iter()
        .any(|r| r.0 == seq && r.1));
    net.round_after_end(&[0, 1, carol]);
    assert_eq!(replayed_hash(&net), net.server.hash().unwrap());

    let mut closed = Net::new(
        spec(3, 1, 21),
        ServerConfig {
            late_join: false,
            ..ServerConfig::default()
        },
        standard_game(),
    );
    closed.player("Ada");
    closed.player("Bob");
    closed.send(0, &net::proto::ClientMsg::Ready(true).encode());
    closed.send(1, &net::proto::ClientMsg::Ready(true).encode());
    closed.send(0, &net::proto::ClientMsg::Start.encode());
    assert_eq!(
        closed.server.phase(),
        Phase::Lobby,
        "free seats and no late joining: the host cannot start"
    );
}

#[test]
fn a_match_resumes_from_a_segment_save_and_plays_on_as_the_original_does() {
    let cfg = ServerConfig {
        segment_rounds: 1,
        ..ServerConfig::default()
    };
    let mut net = Net::new(spec(2, 2, 33), cfg.clone(), standard_game());
    net.player("Ada");
    net.player("Bob");
    net.start();
    net.round_after_end(&[0, 1]);
    net.round_after_end(&[0, 1]);
    assert!(
        !net.server.take_archive().is_empty(),
        "segments of past rounds went to the archive"
    );

    // A restart: the newest save and the seats' tokens.
    let save = net.server.segment().save.clone();
    let records = net.server.seat_records();
    let rules = net.rules.clone();
    let server = Server::resume(
        rules.clone(),
        spec(2, 2, 33),
        cfg,
        Box::new(Counter(77)),
        &save,
        &records,
    )
    .expect("the save resumes");
    let mut twin = Net::from_server(server, rules);
    let tokens: Vec<_> = (0..2).map(|i| net.clients[i].client.token).collect();
    for (i, token) in tokens.iter().enumerate() {
        twin.join(&["Ada", "Bob"][i], Role::Player, *token);
    }
    assert!(twin.run_until(500, |n| n.server.phase() == Phase::Planning));
    assert_eq!(twin.server.round(), net.server.round());
    assert_eq!(
        twin.clients[0].client.view, net.clients[0].client.view,
        "a rejoined player sees what he saw"
    );

    assert_eq!(
        net.server.ai_memory(),
        twin.server.ai_memory(),
        "the AI memory is the save's"
    );
    assert_eq!(net.server.hash(), twin.server.hash());
    // Both play the same round; the AI memory in the save makes the AI play the same turns.
    let guard = own_units(&net.clients[0].client)
        .into_iter()
        .find(|u| u.kind == kind_named(&net.rules, "spearman"))
        .unwrap();
    net.command(0, fortify(guard.id));
    twin.command(0, fortify(guard.id));
    assert_eq!(net.server.hash(), twin.server.hash());
    net.round_after_end(&[0, 1]);
    twin.round_after_end(&[0, 1]);
    assert_eq!(
        twin.server.hash(),
        net.server.hash(),
        "the resumed match is the original match"
    );
    let bad = Server::resume(
        net.rules.clone(),
        spec(2, 2, 33),
        ServerConfig::default(),
        Box::new(Counter(1)),
        b"nonsense",
        &records,
    );
    assert!(bad.is_err());
}
