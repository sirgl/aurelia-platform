//! A little network of one server and scripted clients in one thread: frames pass between them
//! as byte vectors, time is a number the test advances.
#![allow(dead_code)]

use std::sync::Arc;

use kernel::ids::{EntityId, PlayerId, TileId};
use kernel::persistence::codec::Field;
use kernel::world::state::GameState;
use net::client::{hello_for, Client, Event};
use net::proto::{Phase, Role, Token};
use net::server::{ConnId, Entropy, MatchSpec, Server, ServerConfig};
use sandbox_rules::game::{new_game, Setup};
use sandbox_rules::{Rules, Sandbox};

pub const CODE: &str = "TEST";

pub fn rules() -> Arc<Rules> {
    Arc::new(sandbox_content::rules().expect("the sandbox pack compiles"))
}

/// Tokens from a counter: the tests need them distinct, not secret.
pub struct Counter(pub u64);

impl Entropy for Counter {
    fn fill(&mut self, out: &mut [u8]) {
        for b in out.iter_mut() {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *b = (self.0 >> 33) as u8;
        }
    }
}

pub fn spec(humans: u8, ai: u8, seed: u64) -> MatchSpec {
    MatchSpec {
        code: CODE.into(),
        seed,
        humans,
        ai,
        difficulty: String::new(),
        teams: vec![Some(1), Some(1), Some(2), None, None, None, None, None],
    }
}

pub type Make = Box<dyn Fn(&Rules, &MatchSpec) -> GameState<Sandbox>>;

pub fn standard_game() -> Make {
    Box::new(|rules, spec| {
        let setup = Setup {
            players: spec.humans + spec.ai,
            ..Setup::standard(spec.seed)
        };
        new_game(rules, setup)
    })
}

pub struct TClient {
    pub conn: ConnId,
    pub client: Client,
    pub events: Vec<Event>,
    pub closed: bool,
}

impl TClient {
    pub fn results(&self) -> Vec<(u32, bool, String)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::CommandResult { seq, ok, text } => Some((*seq, *ok, text.clone())),
                _ => None,
            })
            .collect()
    }
    pub fn refused(&self) -> Option<(net::proto::RejectCode, String)> {
        self.events.iter().find_map(|e| match e {
            Event::Refused { code, text } => Some((*code, text.clone())),
            _ => None,
        })
    }
    pub fn chat_texts(&self) -> Vec<String> {
        self.client.chat.iter().map(|e| e.text.clone()).collect()
    }
}

pub struct Net {
    pub server: Server<Sandbox>,
    pub clients: Vec<TClient>,
    pub now: u64,
    next_conn: ConnId,
    pub rules: Arc<Rules>,
}

impl Net {
    pub fn new(spec: MatchSpec, cfg: ServerConfig, make: Make) -> Self {
        let rules = rules();
        let server = Server::new(rules.clone(), spec, cfg, Box::new(Counter(1)), make);
        Self {
            server,
            clients: Vec::new(),
            now: 1_000,
            next_conn: 1,
            rules,
        }
    }

    pub fn from_server(server: Server<Sandbox>, rules: Arc<Rules>) -> Self {
        Self {
            server,
            clients: Vec::new(),
            now: 1_000,
            next_conn: 1,
            rules,
        }
    }

    /// A client connects and says hello; returns its index.
    pub fn join(&mut self, name: &str, role: Role, token: Option<Token>) -> usize {
        self.join_with(hello_for(&self.rules, name, CODE, role, token))
    }

    pub fn join_with(&mut self, hello: net::proto::Hello) -> usize {
        let conn = self.next_conn;
        self.next_conn += 1;
        let client = Client::new(hello);
        let hello = client.hello_frame();
        self.clients.push(TClient {
            conn,
            client,
            events: Vec::new(),
            closed: false,
        });
        self.server.connected(conn, self.now);
        self.server.receive(conn, &hello, self.now);
        self.deliver();
        self.clients.len() - 1
    }

    pub fn player(&mut self, name: &str) -> usize {
        self.join(name, Role::Player, None)
    }

    /// Drop a client's connection.
    pub fn drop_client(&mut self, i: usize) {
        self.clients[i].closed = true;
        let conn = self.clients[i].conn;
        self.server.disconnected(conn, self.now);
        self.deliver();
    }

    /// Reconnect a player with its token; returns the new client's index.
    pub fn rejoin(&mut self, old: usize, name: &str) -> usize {
        let token = self.clients[old].client.token;
        self.join(name, Role::Player, token)
    }

    pub fn deliver(&mut self) {
        for out in self.server.drain() {
            if let Some(c) = self
                .clients
                .iter_mut()
                .find(|c| c.conn == out.conn && !c.closed)
            {
                match c.client.handle(&out.frame) {
                    Ok(event) => c.events.push(event),
                    Err(e) => panic!("a frame the server sent does not decode: {e}"),
                }
                if out.close {
                    c.closed = true;
                }
            }
        }
    }

    pub fn tick(&mut self, ms: u64) {
        self.now += ms;
        self.server.tick(self.now);
        self.deliver();
    }

    /// Tick until `done` or `limit` ticks of 10 ms pass.
    pub fn run_until(&mut self, limit: u32, done: impl Fn(&Net) -> bool) -> bool {
        for _ in 0..limit {
            if done(self) {
                return true;
            }
            self.tick(10);
        }
        done(self)
    }

    pub fn send(&mut self, i: usize, frame: &[u8]) {
        let conn = self.clients[i].conn;
        self.server.receive(conn, frame, self.now);
        self.deliver();
    }

    pub fn command(&mut self, i: usize, bytes: Vec<u8>) -> u32 {
        let (seq, frame) = self.clients[i].client.command_frame(bytes);
        self.send(i, &frame);
        seq
    }

    pub fn end_turn(&mut self, i: usize) {
        let frame = self.clients[i].client.end_turn_frame();
        self.send(i, &frame);
    }

    /// Everybody ready in the lobby, the host starts, the first round opens.
    pub fn start(&mut self) {
        let n = self.clients.len();
        for i in 0..n {
            self.send(i, &net::proto::ClientMsg::Ready(true).encode());
        }
        self.send(0, &net::proto::ClientMsg::Start.encode());
        assert!(
            self.run_until(200, |n| n.server.phase() == Phase::Planning),
            "the first round opens"
        );
        self.tick(100);
    }

    pub fn round_after_end(&mut self, ends: &[usize]) {
        let round = self.server.round();
        for &i in ends {
            self.end_turn(i);
        }
        assert!(
            self.run_until(2_000, |n| n.server.phase() == Phase::Planning
                && n.server.round() > round),
            "the round resolves"
        );
        self.tick(100);
    }
}

// ----- command bytes of the sandbox (the journal's layout) -----

fn kernel_command(tag: u8, fields: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut out = vec![0u8, tag];
    fields(&mut out);
    out
}

pub fn move_unit(unit: u64, to: u32) -> Vec<u8> {
    kernel_command(0, |o| {
        Field::put(&unit, o);
        Field::put(&to, o);
    })
}

pub fn found_city(settler: u64) -> Vec<u8> {
    kernel_command(2, |o| Field::put(&settler, o))
}

pub fn fortify(unit: u64) -> Vec<u8> {
    kernel_command(3, |o| Field::put(&unit, o))
}

pub fn end_turn_bytes(player: u8) -> Vec<u8> {
    kernel_command(17, |o| Field::put(&player, o))
}

pub fn attack(unit: u64, target: u32) -> Vec<u8> {
    let mut out = vec![1u8];
    Field::put(&unit, &mut out);
    Field::put(&target, &mut out);
    out
}

/// The unit the client sees as its own, by kind key fragment.
pub fn own_units(client: &Client) -> Vec<net::view::UnitView> {
    client
        .view
        .as_ref()
        .map(|v| {
            v.units
                .iter()
                .filter(|u| u.own.is_some())
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub fn kind_named(rules: &Rules, fragment: &str) -> u16 {
    rules
        .units
        .iter()
        .position(|u| u.key.as_str().contains(fragment))
        .unwrap_or_else(|| panic!("a unit kind with {fragment}")) as u16
}

/// A land tile with free land neighbours, found in the game itself.
pub fn land_tiles(game: &GameState<Sandbox>) -> Vec<TileId> {
    let auth = game.view().auth;
    (0..auth.map().tile_count() as u32)
        .map(TileId)
        .filter(|t| !auth.map().is_water(*t))
        .collect()
}

pub fn entity(id: u64) -> EntityId {
    EntityId(id)
}

pub fn pid(p: u8) -> PlayerId {
    PlayerId(p)
}
