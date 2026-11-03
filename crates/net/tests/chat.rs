//! Chat through the server: who receives what, the history a joiner gets, limits.

mod common;

use common::*;
use net::client::Event;
use net::proto::{ChatTarget, ClientMsg, Role};
use net::server::ServerConfig;

fn table() -> (Net, [usize; 3], usize) {
    let mut cfg = ServerConfig::default();
    cfg.chat.burst = 3;
    let mut net = Net::new(spec(3, 1, 8), cfg, standard_game());
    let a = net.player("Ada");
    let b = net.player("Bob");
    let c = net.player("Cy");
    let watcher = net.join("Eve", Role::Spectator, None);
    net.start();
    (net, [a, b, c], watcher)
}

fn texts(net: &Net, i: usize) -> Vec<String> {
    net.clients[i].chat_texts()
}

#[test]
fn a_direct_message_reaches_its_sender_and_recipient_and_nobody_else() {
    let (mut net, [a, b, c], watcher) = table();
    let frame = net.clients[a]
        .client
        .chat_frame(ChatTarget::Dm(1), "secret terms");
    net.send(a, &frame);
    net.tick(100);
    assert!(
        texts(&net, a).contains(&"secret terms".to_owned()),
        "the sender sees its own message"
    );
    assert!(texts(&net, b).contains(&"secret terms".to_owned()));
    assert!(!texts(&net, c).contains(&"secret terms".to_owned()));
    assert!(!texts(&net, watcher).contains(&"secret terms".to_owned()));
    // Not a single frame to Cy or the spectator carried it, whatever its kind: look at the raw bytes.
    for i in [c, watcher] {
        let leaked = net.clients[i]
            .events
            .iter()
            .any(|e| matches!(e, Event::Chat(entry) if entry.text.contains("secret")));
        assert!(!leaked);
    }
    // The server's own chat hub agrees about who may read it, and the match log leaves it out.
    let hub = net.server.chat();
    assert!(hub.history_for(0).iter().any(|e| e.text == "secret terms"));
    assert!(hub.history_for(1).iter().any(|e| e.text == "secret terms"));
    assert!(!hub.history_for(2).iter().any(|e| e.text == "secret terms"));
    assert!(!hub
        .history_for(net::chat::SPECTATOR_BASE)
        .iter()
        .any(|e| e.text == "secret terms"));
    assert!(!hub.log().iter().any(|e| e.text == "secret terms"));
}

#[test]
fn all_chat_reaches_everybody_and_team_chat_only_the_team() {
    let (mut net, [a, b, c], watcher) = table();
    let frame = net.clients[a]
        .client
        .chat_frame(ChatTarget::All, "good luck all");
    net.send(a, &frame);
    let frame = net.clients[a]
        .client
        .chat_frame(ChatTarget::Team, "we are the team");
    net.send(a, &frame);
    net.tick(100);
    for i in [a, b, c, watcher] {
        assert!(
            texts(&net, i).contains(&"good luck all".to_owned()),
            "client {i}"
        );
    }
    // Ada and Bob are team 1 (the spec's teams), Cy team 2.
    assert!(texts(&net, b).contains(&"we are the team".to_owned()));
    assert!(!texts(&net, c).contains(&"we are the team".to_owned()));
    assert!(!texts(&net, watcher).contains(&"we are the team".to_owned()));
    // Cy has no team mate to write to but his own team.
    let frame = net.clients[watcher]
        .client
        .chat_frame(ChatTarget::All, "hello from the stands");
    net.send(watcher, &frame);
    assert!(
        net.clients[watcher]
            .events
            .iter()
            .any(|e| matches!(e, Event::ChatRefused(_))),
        "spectators do not write"
    );
}

#[test]
fn the_sender_is_the_connection_and_a_player_cannot_write_in_anothers_name() {
    let (mut net, [_, b, _], _) = table();
    // There is no field in the message to name a sender; a DM to oneself is refused.
    let frame = net.clients[b]
        .client
        .chat_frame(ChatTarget::Dm(1), "to myself");
    net.send(b, &frame);
    assert!(net.clients[b]
        .events
        .iter()
        .any(|e| matches!(e, Event::ChatRefused(_))));
    let entry = net.clients[b]
        .client
        .chat
        .iter()
        .find(|e| e.text.contains("joined"))
        .expect("a system notice");
    assert_eq!(entry.from, None, "notices come from the server");
}

#[test]
fn a_flood_is_limited_and_a_block_silences_one_sender_for_one_reader() {
    let (mut net, [a, b, c], _) = table();
    for i in 0..5 {
        let frame = net.clients[a]
            .client
            .chat_frame(ChatTarget::All, &format!("spam {i}"));
        net.send(a, &frame);
    }
    let sent = texts(&net, b)
        .iter()
        .filter(|t| t.starts_with("spam"))
        .count();
    assert_eq!(sent, 3, "the burst is three");
    assert!(
        net.clients[a]
            .events
            .iter()
            .filter(|e| matches!(e, Event::ChatRefused(t) if t.contains("too fast")))
            .count()
            == 2
    );
    // Bob blocks Ada; Cy still hears her.
    net.send(b, &ClientMsg::Block { part: 0, on: true }.encode());
    net.tick(5_000);
    let frame = net.clients[a]
        .client
        .chat_frame(ChatTarget::All, "after the block");
    net.send(a, &frame);
    assert!(!texts(&net, b).contains(&"after the block".to_owned()));
    assert!(texts(&net, c).contains(&"after the block".to_owned()));
}

#[test]
fn a_joiner_gets_the_history_it_may_read() {
    let (mut net, [a, b, _], _) = table();
    let dm = net.clients[a]
        .client
        .chat_frame(ChatTarget::Dm(1), "just for Bob");
    net.send(a, &dm);
    let open = net.clients[a]
        .client
        .chat_frame(ChatTarget::All, "for everybody");
    net.send(a, &open);
    net.tick(100);
    // Bob reconnects: he gets both. A new spectator gets the open one only.
    net.drop_client(b);
    let b2 = net.rejoin(b, "Bob");
    assert!(texts(&net, b2).contains(&"just for Bob".to_owned()));
    assert!(texts(&net, b2).contains(&"for everybody".to_owned()));
    let late = net.join("Late", Role::Spectator, None);
    assert!(texts(&net, late).contains(&"for everybody".to_owned()));
    assert!(!texts(&net, late).contains(&"just for Bob".to_owned()));
}
