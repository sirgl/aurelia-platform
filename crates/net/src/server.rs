//! The match server, transport-agnostic: connections come in as events (`connected`,
//! `receive`, `disconnected`), time comes in as a number (`tick`), frames go out through
//! `drain`. It owns the one authoritative game and the AI of its seats.
//!
//! A round is two phases (platform/docs/multiplayer.md):
//!
//! * **Planning.** The window of the kernel is open (`GameState::open_window`): every human seat
//!   orders its own units and cities at once, each command executed at once, in the order the
//!   server received them (`GameState::execute_as`), with the result sent back and the changed
//!   views sent to the seats that see the change. A seat ends its turn with `EndTurn`; the phase
//!   ends when every human seat has, or the timer runs out, or the host closes it.
//! * **Resolving.** The window closes and the turns end one by one in player order, exactly as
//!   in a sequential game: a human seat's `EndTurn` is executed by the server, an AI seat (a
//!   seat of the match, a barbarian, or the seat of a human who left) plays its turn through the
//!   AI driver, and when the last turn ends the round has advanced.
//!
//! Every command, every window and every end of a turn goes through the game's journal
//! (platform/docs/saves.md §3), so a match is its save and its journal.

use std::collections::BTreeMap;
use std::sync::Arc;

use kernel::ai::{AiPlayers, DriverPhase, StepOutcome};
use kernel::ids::PlayerId;
use kernel::persistence::codec::{self, Reader};
use kernel::persistence::decode::RulesMatch;
use kernel::persistence::encode::encode;
use kernel::persistence::load::load;
use kernel::replay::codec::{CommandCodec, CommandField};
use kernel::replay::hash::authoritative_hash;
use kernel::rules::CompiledRuleset;
use kernel::world::state::GameState;

use crate::chat::{self, ChatConfig, ChatHub, Part};
use crate::proto::{
    ChatTarget, ClientMsg, Hello, Phase, RejectCode, Role, RosterEntry, ServerMsg, Settings, Token,
    PROTO_VERSION, TIMER_SECS,
};
use crate::view::SeatView;

pub type ConnId = u64;

/// Where the server's randomness comes from (tokens): the host's operating system.
pub trait Entropy {
    fn fill(&mut self, out: &mut [u8]);
}

/// What a spectator is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpectatorView {
    /// Chat and the status line only. The default: a live view of every seat would let a player's
    /// friend read the whole map to them.
    Nothing,
    /// The whole map open (`PlayerView::revealed`).
    Revealed,
}

/// The settings of a match.
#[derive(Debug, Clone)]
pub struct MatchSpec {
    pub code: String,
    pub seed: u64,
    /// Seats meant for humans: players `0..humans`.
    pub humans: u8,
    /// Major seats the AI plays: players `humans..humans + ai`.
    pub ai: u8,
    /// The handicap key; empty: the rules' default level.
    pub difficulty: String,
    /// The team of each major seat (None: alone).
    pub teams: Vec<Option<u8>>,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// The round closes after this long, whoever has not ended the turn (None: it waits).
    pub round_timer_ms: Option<u64>,
    /// A seat's changed view is sent at most this often.
    pub flush_ms: u64,
    /// AI and end-turn steps taken per `tick` while resolving (bounds how long one tick runs).
    pub resolve_steps: u32,
    pub chat: ChatConfig,
    pub spectators: SpectatorView,
    pub max_spectators: usize,
    /// A seat nobody took at the start may be taken later.
    pub late_join: bool,
    /// Commands: this many at once, one more per `command_refill_ms`.
    pub command_burst: u32,
    pub command_refill_ms: u64,
    pub max_command_bytes: usize,
    /// Start a new save and journal segment every this many rounds (0: one segment).
    pub segment_rounds: u32,
    /// Keep playing rounds when nobody is connected.
    pub run_unattended: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            round_timer_ms: None,
            flush_ms: 50,
            resolve_steps: 64,
            chat: ChatConfig::default(),
            spectators: SpectatorView::Nothing,
            max_spectators: 8,
            late_join: true,
            command_burst: 200,
            command_refill_ms: 5,
            max_command_bytes: 4096,
            segment_rounds: 0,
            run_unattended: false,
        }
    }
}

/// A frame for a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub conn: ConnId,
    pub frame: Vec<u8>,
    /// Close the connection after this frame.
    pub close: bool,
}

/// A save and the journal that continues it: a match from `base` on.
#[derive(Debug, Clone)]
pub struct Segment {
    pub round: u32,
    pub save: Vec<u8>,
    pub journal: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct Metrics {
    pub frames_out: u64,
    pub bytes_out: u64,
    pub snapshots: u64,
    pub deltas: u64,
    pub delta_bytes: u64,
    pub snapshot_bytes: u64,
    pub commands_applied: u64,
    pub commands_refused: u64,
    pub rounds: u32,
}

#[derive(Debug)]
struct Seat {
    name: String,
    token: Token,
    team: Option<u8>,
    /// The slot is for a human.
    human: bool,
    claimed: bool,
    conn: Option<ConnId>,
    ready: bool,
    /// The AI plays this seat in the round that is running.
    ai_plays: bool,
    tokens: u32,
    refilled_ms: u64,
}

#[derive(Debug)]
struct Conn {
    part: Option<Part>,
    last: Option<SeatView>,
    dirty: bool,
    flushed_ms: u64,
}

pub struct Server<M: CommandCodec> {
    rules: Arc<CompiledRuleset<M>>,
    spec: MatchSpec,
    cfg: ServerConfig,
    /// The settings the host may change; they start from `cfg`.
    settings: Settings,
    entropy: Box<dyn Entropy>,
    make: Box<dyn Fn(&CompiledRuleset<M>, &MatchSpec) -> GameState<M>>,
    game: Option<GameState<M>>,
    ai: Option<M::Ai>,
    phase: Phase,
    seats: Vec<Seat>,
    spectators: BTreeMap<Part, ConnId>,
    conns: BTreeMap<ConnId, Conn>,
    chat: ChatHub,
    out: Vec<Output>,
    metrics: Metrics,
    now_ms: u64,
    host: Option<Part>,
    /// The turns of the round are ending (Resolving), as opposed to the round opening.
    sweeping: bool,
    round: u32,
    deadline_ms: Option<u64>,
    /// The first seat that may still act when the window opened.
    origin: u8,
    segment: Segment,
    archive: Vec<Segment>,
    segment_start_round: u32,
    /// Send the changed views at the next tick, not after `flush_ms` (a round just opened).
    flush_now: bool,
}

impl<M: CommandCodec> Server<M> {
    pub fn new(
        rules: Arc<CompiledRuleset<M>>,
        spec: MatchSpec,
        cfg: ServerConfig,
        entropy: Box<dyn Entropy>,
        make: Box<dyn Fn(&CompiledRuleset<M>, &MatchSpec) -> GameState<M>>,
    ) -> Self {
        let total = usize::from(spec.humans) + usize::from(spec.ai);
        let seats = (0..total)
            .map(|i| Seat {
                name: if i < usize::from(spec.humans) {
                    String::new()
                } else {
                    format!("AI {}", i + 1)
                },
                token: [0; 16],
                team: spec.teams.get(i).copied().flatten(),
                human: i < usize::from(spec.humans),
                claimed: false,
                conn: None,
                ready: false,
                ai_plays: true,
                tokens: cfg.command_burst,
                refilled_ms: 0,
            })
            .collect();
        let chat = ChatHub::new(cfg.chat.clone());
        let settings = Settings {
            round_timer_secs: cfg
                .round_timer_ms
                .map(|ms| (ms / 1000).clamp(1, u64::from(u32::MAX)) as u32),
            spectators_see_all: cfg.spectators == SpectatorView::Revealed,
        };
        Self {
            rules,
            spec,
            cfg,
            settings,
            entropy,
            make,
            game: None,
            ai: None,
            phase: Phase::Lobby,
            seats,
            spectators: BTreeMap::new(),
            conns: BTreeMap::new(),
            chat,
            out: Vec::new(),
            metrics: Metrics::default(),
            now_ms: 0,
            host: None,
            sweeping: false,
            round: 0,
            deadline_ms: None,
            origin: 0,
            segment: Segment {
                round: 0,
                save: Vec::new(),
                journal: Vec::new(),
            },
            archive: Vec::new(),
            segment_start_round: 0,
            flush_now: false,
        }
    }

    // ----- what the host and the tests may look at -----

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn round(&self) -> u32 {
        self.round
    }
    pub fn game(&self) -> Option<&GameState<M>> {
        self.game.as_ref()
    }
    pub fn rules(&self) -> &CompiledRuleset<M> {
        &self.rules
    }
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }
    pub fn chat(&self) -> &ChatHub {
        &self.chat
    }
    pub fn chat_mut(&mut self) -> &mut ChatHub {
        &mut self.chat
    }
    /// The save and the journal of the segment being played.
    pub fn segment(&self) -> &Segment {
        &self.segment
    }
    /// Segments that ended (the host writes them to disk and may drop them).
    pub fn take_archive(&mut self) -> Vec<Segment> {
        std::mem::take(&mut self.archive)
    }
    /// The seat of a connection.
    pub fn seat_of(&self, conn: ConnId) -> Option<u8> {
        self.conns
            .get(&conn)
            .and_then(|c| c.part)
            .filter(|p| !chat::is_spectator(*p))
    }
    /// The AI plays this seat now.
    pub fn ai_plays(&self, seat: u8) -> bool {
        self.seats
            .get(usize::from(seat))
            .is_some_and(|s| s.ai_plays)
    }
    /// The AI memory as a save keeps it (between AI turns).
    pub fn ai_memory(&self) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        kernel::persistence::codec::Field::put(&self.ai.as_ref()?.save(), &mut out);
        Some(out)
    }
    /// The settings in force.
    pub fn settings(&self) -> Settings {
        self.settings
    }
    pub fn hash(&self) -> Option<u64> {
        self.game.as_ref().map(authoritative_hash)
    }
    /// Take the frames to send.
    pub fn drain(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.out)
    }

    // ----- output -----

    fn send(&mut self, conn: ConnId, msg: &ServerMsg) {
        self.push(conn, msg, false);
    }

    fn push(&mut self, conn: ConnId, msg: &ServerMsg, close: bool) {
        let frame = msg.encode();
        self.metrics.frames_out += 1;
        self.metrics.bytes_out += frame.len() as u64;
        match msg {
            ServerMsg::Snapshot(_) => {
                self.metrics.snapshots += 1;
                self.metrics.snapshot_bytes += frame.len() as u64;
            }
            ServerMsg::Delta(_) => {
                self.metrics.deltas += 1;
                self.metrics.delta_bytes += frame.len() as u64;
            }
            _ => {}
        }
        self.out.push(Output { conn, frame, close });
    }

    fn conn_of(&self, part: Part) -> Option<ConnId> {
        if chat::is_spectator(part) {
            self.spectators.get(&part).copied()
        } else {
            self.seats.get(usize::from(part)).and_then(|s| s.conn)
        }
    }

    fn broadcast(&mut self, msg: &ServerMsg) {
        let conns: Vec<ConnId> = self
            .conns
            .iter()
            .filter(|(_, c)| c.part.is_some())
            .map(|(id, _)| *id)
            .collect();
        for conn in conns {
            self.send(conn, msg);
        }
    }

    fn roster(&self) -> Vec<RosterEntry> {
        self.seats
            .iter()
            .enumerate()
            .map(|(i, s)| RosterEntry {
                seat: i as u8,
                name: s.name.clone(),
                human: s.human,
                connected: s.conn.is_some(),
                ai_plays: s.ai_plays,
                ready: s.ready,
                team: s.team,
            })
            .collect()
    }

    fn broadcast_roster(&mut self) {
        let roster = ServerMsg::Roster(self.roster());
        self.broadcast(&roster);
    }

    fn status(&self) -> ServerMsg {
        ServerMsg::Status {
            round: self.round,
            phase: self.phase,
            ready: self
                .seats
                .iter()
                .enumerate()
                .filter(|(_, s)| s.ready)
                .map(|(i, _)| i as u8)
                .collect(),
            closes_in_ms: self
                .deadline_ms
                .map(|d| d.saturating_sub(self.now_ms).min(u64::from(u32::MAX)) as u32),
        }
    }

    fn broadcast_status(&mut self) {
        let status = self.status();
        self.broadcast(&status);
    }

    fn deliver_chat(&mut self, delivery: chat::Delivery) {
        for part in delivery.to {
            if let Some(conn) = self.conn_of(part) {
                self.send(conn, &ServerMsg::Chat(delivery.entry.clone()));
            }
        }
    }

    fn notice(&mut self, text: &str) {
        let delivery = self.chat.system(text, self.now_ms);
        self.deliver_chat(delivery);
    }

    fn refuse(&mut self, conn: ConnId, code: RejectCode, text: impl Into<String>) {
        self.push(
            conn,
            &ServerMsg::Reject {
                code,
                text: text.into(),
            },
            true,
        );
        self.conns.remove(&conn);
    }

    // ----- connections -----

    /// A connection opened: it has to say `Hello` before anything else.
    pub fn connected(&mut self, conn: ConnId, now_ms: u64) {
        self.now_ms = now_ms;
        self.conns.insert(
            conn,
            Conn {
                part: None,
                last: None,
                dirty: false,
                flushed_ms: now_ms,
            },
        );
    }

    /// A connection closed. A seat stays the player's; if it is not ready when the round closes the AI
    /// plays it, and when the player comes back it plays itself again.
    pub fn disconnected(&mut self, conn: ConnId, now_ms: u64) {
        self.now_ms = now_ms;
        let Some(c) = self.conns.remove(&conn) else {
            return;
        };
        let Some(part) = c.part else { return };
        if chat::is_spectator(part) {
            self.spectators.remove(&part);
            self.chat.leave(part);
            return;
        }
        if let Some(seat) = self.seats.get_mut(usize::from(part)) {
            if seat.conn == Some(conn) {
                seat.conn = None;
                let name = seat.name.clone();
                self.notice(&format!("{name} disconnected"));
                self.broadcast_roster();
                self.progress();
            }
        }
    }

    fn version_problem(&self, hello: &Hello) -> Option<String> {
        let ours = (
            PROTO_VERSION,
            M::ID.name,
            M::ID.version,
            M::COMMAND_CODEC_VERSION,
            self.rules.hash(),
        );
        let theirs = (
            hello.proto,
            hello.module.as_str(),
            hello.module_version,
            hello.command_codec,
            hello.rules_hash,
        );
        if ours.0 != theirs.0 {
            return Some(format!("protocol {} here, {} there", ours.0, theirs.0));
        }
        if ours.1 != theirs.1 {
            return Some(format!("rules module {} here, {} there", ours.1, theirs.1));
        }
        if ours.2 != theirs.2 {
            return Some(format!("save layout {} here, {} there", ours.2, theirs.2));
        }
        if ours.3 != theirs.3 {
            return Some(format!(
                "command layout {} here, {} there",
                ours.3, theirs.3
            ));
        }
        if ours.4 != theirs.4 {
            return Some("other rules (the content differs)".to_owned());
        }
        None
    }

    fn new_token(&mut self) -> Token {
        let mut token = [0u8; 16];
        self.entropy.fill(&mut token);
        token
    }

    fn hello(&mut self, conn: ConnId, hello: Hello) {
        if self.conns.get(&conn).is_some_and(|c| c.part.is_some()) {
            return self.refuse(conn, RejectCode::Malformed, "hello twice");
        }
        if let Some(problem) = self.version_problem(&hello) {
            return self.refuse(conn, RejectCode::VersionMismatch, problem);
        }
        if hello.code != self.spec.code {
            return self.refuse(conn, RejectCode::WrongCode, "no such match");
        }
        let name = crate::wire::truncate(hello.name.trim(), 32).to_owned();
        if name.is_empty() {
            return self.refuse(conn, RejectCode::Malformed, "a name is needed");
        }
        let part = match hello.role {
            Role::Spectator => {
                if self.spectators.len() >= self.cfg.max_spectators {
                    return self.refuse(conn, RejectCode::Full, "no room for spectators");
                }
                let part = (0..)
                    .map(|i| chat::SPECTATOR_BASE + i)
                    .find(|p| !self.spectators.contains_key(p));
                let Some(part) = part.filter(|p| *p < u8::MAX) else {
                    return self.refuse(conn, RejectCode::Full, "no room for spectators");
                };
                self.spectators.insert(part, conn);
                self.chat.join(part, None);
                part
            }
            Role::Player => match self.claim(conn, &hello, &name) {
                Ok(part) => part,
                Err((code, text)) => return self.refuse(conn, code, text),
            },
        };
        if let Some(c) = self.conns.get_mut(&conn) {
            c.part = Some(part);
        }
        let token = if chat::is_spectator(part) {
            [0; 16]
        } else {
            self.seats[usize::from(part)].token
        };
        if self.host.is_none() && !chat::is_spectator(part) {
            self.host = Some(part);
        }
        let welcome = ServerMsg::Welcome {
            part,
            token,
            host: self.host == Some(part),
            phase: self.phase,
            round: self.round,
            roster: self.roster(),
        };
        self.send(conn, &welcome);
        let history = self.chat.history_for(part);
        self.send(conn, &ServerMsg::ChatHistory(history));
        self.send_snapshot(conn);
        self.send(conn, &ServerMsg::Settings(self.settings));
        self.send(conn, &self.status());
        let who = if chat::is_spectator(part) {
            format!("{name} is watching")
        } else {
            format!("{name} joined")
        };
        self.notice(&who);
        self.broadcast_roster();
        self.progress();
    }

    /// The seat a player takes: the one its token names, or the first seat nobody took.
    fn claim(
        &mut self,
        conn: ConnId,
        hello: &Hello,
        name: &str,
    ) -> Result<Part, (RejectCode, String)> {
        if let Some(token) = hello.token {
            let at = self
                .seats
                .iter()
                .position(|s| s.claimed && s.token == token);
            let Some(at) = at else {
                return Err((RejectCode::BadToken, "that seat is not yours".into()));
            };
            let old = self.seats[at].conn.replace(conn);
            if let Some(old) = old {
                // The same player on a new connection: the old one is dropped.
                self.push(
                    old,
                    &ServerMsg::Reject {
                        code: RejectCode::NotAllowed,
                        text: "joined from elsewhere".into(),
                    },
                    true,
                );
                self.conns.remove(&old);
            }
            self.rejoin(at);
            self.chat.join(at as u8, self.seats[at].team);
            return Ok(at as u8);
        }
        if self.phase != Phase::Lobby && !self.cfg.late_join {
            return Err((RejectCode::Full, "the match has started".into()));
        }
        let at = self.seats.iter().position(|s| s.human && !s.claimed);
        let Some(at) = at else {
            return Err((RejectCode::Full, "no free seat".into()));
        };
        let token = self.new_token();
        let seat = &mut self.seats[at];
        seat.claimed = true;
        seat.token = token;
        seat.name = name.to_owned();
        seat.conn = Some(conn);
        self.rejoin(at);
        self.chat.join(at as u8, self.seats[at].team);
        Ok(at as u8)
    }

    /// A seat has a player again. If its turn has not come it plays itself: during planning, or
    /// while the round is only opening (the AI has not reached the seat yet).
    fn rejoin(&mut self, at: usize) {
        if !self.seats[at].ai_plays {
            return;
        }
        let turn_to_come = match self.phase {
            Phase::Planning => at as u8 >= self.origin,
            Phase::Resolving if !self.sweeping => match (self.game.as_ref(), self.ai.as_ref()) {
                (Some(game), Some(ai)) => {
                    ai.phase() == DriverPhase::Idle
                        && at as u8 >= game.view().auth.active_player().0
                }
                _ => false,
            },
            _ => false,
        };
        if !turn_to_come {
            return;
        }
        self.seats[at].ai_plays = false;
        self.seats[at].ready = false;
        self.set_ai_humans();
        if self.phase == Phase::Planning {
            self.sync_window();
        }
    }

    fn send_snapshot(&mut self, conn: ConnId) {
        let Some(part) = self.conns.get(&conn).and_then(|c| c.part) else {
            return;
        };
        let Some(view) = self.view_for(part) else {
            return;
        };
        self.send(conn, &ServerMsg::Snapshot(view.clone()));
        if let Some(c) = self.conns.get_mut(&conn) {
            c.last = Some(view);
            c.dirty = false;
            c.flushed_ms = self.now_ms;
        }
    }

    fn view_for(&self, part: Part) -> Option<SeatView> {
        let game = self.game.as_ref()?;
        if chat::is_spectator(part) {
            return self
                .settings
                .spectators_see_all
                .then(|| SeatView::build_revealed(game, &self.rules));
        }
        Some(SeatView::build(game, &self.rules, PlayerId(part)))
    }

    // ----- messages -----

    /// A frame came in. A frame that does not decode drops the connection.
    pub fn receive(&mut self, conn: ConnId, bytes: &[u8], now_ms: u64) {
        self.now_ms = now_ms;
        if !self.conns.contains_key(&conn) {
            return;
        }
        let msg = match ClientMsg::decode(bytes) {
            Ok(msg) => msg,
            Err(e) => return self.refuse(conn, RejectCode::Malformed, e.to_string()),
        };
        let part = self.conns.get(&conn).and_then(|c| c.part);
        if let ClientMsg::Hello(hello) = msg {
            return self.hello(conn, hello);
        }
        let Some(part) = part else {
            return self.refuse(conn, RejectCode::Malformed, "say hello first");
        };
        match msg {
            ClientMsg::Hello(_) => unreachable!("handled above"),
            ClientMsg::Ping(n) => self.send(conn, &ServerMsg::Pong(n)),
            ClientMsg::Chat { target, text } => self.chat_message(conn, part, target, &text),
            ClientMsg::Block { part: other, on } => {
                if on {
                    self.chat.block(part, other)
                } else {
                    self.chat.unblock(part, other)
                }
            }
            ClientMsg::Ready(on) => self.lobby_ready(part, on),
            ClientMsg::Start => self.lobby_start(conn, part),
            ClientMsg::Settings(settings) => self.change_settings(conn, part, settings),
            ClientMsg::Command { seq, bytes } => self.command(conn, part, seq, &bytes),
            ClientMsg::EndTurn { round } => self.end_turn(part, round, true),
            ClientMsg::Unready { round } => self.end_turn(part, round, false),
            ClientMsg::ForceClose { round } => {
                if self.host == Some(part) && self.phase == Phase::Planning && round == self.round {
                    self.close_window();
                    self.progress();
                }
            }
        }
    }

    fn chat_message(&mut self, conn: ConnId, part: Part, target: ChatTarget, text: &str) {
        let channel = match target {
            ChatTarget::All => chat::Channel::All,
            ChatTarget::Team => match self.seats.get(usize::from(part)).and_then(|s| s.team) {
                Some(team) if !chat::is_spectator(part) => chat::Channel::Team(team),
                _ => return self.send(conn, &ServerMsg::ChatRefused("you are in no team".into())),
            },
            ChatTarget::Dm(seat) => chat::Channel::dm(part, seat),
        };
        match self.chat.post(part, channel, text, self.now_ms) {
            Ok(delivery) => self.deliver_chat(delivery),
            Err(refusal) => self.send(conn, &ServerMsg::ChatRefused(refusal.to_string())),
        }
    }

    // ----- lobby -----

    fn lobby_ready(&mut self, part: Part, on: bool) {
        if self.phase != Phase::Lobby || chat::is_spectator(part) {
            return;
        }
        self.seats[usize::from(part)].ready = on;
        self.broadcast_roster();
    }

    /// The host changes the settings: the round timer any time (it applies from the next round that
    /// opens), who spectators see only in the lobby (changing it in a live game would hand a player's
    /// friend the map).
    fn change_settings(&mut self, conn: ConnId, part: Part, new: Settings) {
        let refuse =
            |me: &mut Self, why: &str| me.send(conn, &ServerMsg::ChatRefused(why.to_owned()));
        if self.host != Some(part) {
            return refuse(self, "only the host changes the settings");
        }
        if new
            .round_timer_secs
            .is_some_and(|t| !TIMER_SECS.contains(&t))
        {
            return refuse(
                self,
                &format!(
                    "the round timer is {} to {} seconds",
                    TIMER_SECS.start(),
                    TIMER_SECS.end()
                ),
            );
        }
        if new.spectators_see_all != self.settings.spectators_see_all && self.phase != Phase::Lobby
        {
            return refuse(self, "who spectators see is set in the lobby");
        }
        if new == self.settings {
            return;
        }
        self.settings = new;
        self.broadcast(&ServerMsg::Settings(new));
        let text = match new.round_timer_secs {
            Some(t) => format!("the host set the round timer to {t} s"),
            None => "the host turned the round timer off".to_owned(),
        };
        self.notice(&text);
    }

    fn lobby_start(&mut self, conn: ConnId, part: Part) {
        if self.phase != Phase::Lobby || self.host != Some(part) {
            return self.send(
                conn,
                &ServerMsg::ChatRefused("only the host starts the match, from the lobby".into()),
            );
        }
        let waiting: Vec<String> = self
            .seats
            .iter()
            .filter(|s| s.claimed && !s.ready)
            .map(|s| s.name.clone())
            .collect();
        if !waiting.is_empty() {
            return self.send(
                conn,
                &ServerMsg::ChatRefused(format!("not ready: {}", waiting.join(", "))),
            );
        }
        let unclaimed = self.seats.iter().filter(|s| s.human && !s.claimed).count();
        if unclaimed > 0 && !self.cfg.late_join {
            return self.send(conn, &ServerMsg::ChatRefused("seats are still free".into()));
        }
        self.start();
    }

    /// Build the game and begin the first round.
    pub fn start(&mut self) {
        let mut game = (self.make)(&self.rules, &self.spec);
        let humans: Vec<PlayerId> = (0..self.spec.humans).map(PlayerId).collect();
        let difficulty = if self.spec.difficulty.is_empty() {
            self.rules.default_difficulty().to_owned()
        } else {
            self.spec.difficulty.clone()
        };
        let ai = M::Ai::for_game(&self.rules, &difficulty, humans);
        ai.configure_game(&mut game);
        self.round = game.view().auth.turn().round;
        self.game = Some(game);
        self.ai = Some(ai);
        for seat in &mut self.seats {
            seat.ready = false;
        }
        self.begin_round();
        self.new_segment();
        self.phase = Phase::Resolving;
        self.sweeping = false;
        self.notice("the match begins");
        self.broadcast_roster();
        self.progress();
    }

    // ----- commands -----

    fn command(&mut self, conn: ConnId, part: Part, seq: u32, bytes: &[u8]) {
        let result = self.run_command(part, bytes);
        match &result {
            Ok(()) => self.metrics.commands_applied += 1,
            Err(_) => self.metrics.commands_refused += 1,
        }
        let (ok, text) = match result {
            Ok(()) => (true, String::new()),
            Err(text) => (false, text),
        };
        self.send(conn, &ServerMsg::CommandResult { seq, ok, text });
    }

    fn run_command(&mut self, part: Part, bytes: &[u8]) -> Result<(), String> {
        if chat::is_spectator(part) {
            return Err("spectators do not play".into());
        }
        if self.phase != Phase::Planning {
            return Err("the round is not open".into());
        }
        let now = self.now_ms;
        let (burst, refill, max) = (
            self.cfg.command_burst,
            self.cfg.command_refill_ms.max(1),
            self.cfg.max_command_bytes,
        );
        let seat = &mut self.seats[usize::from(part)];
        if seat.ai_plays || seat.ready {
            return Err(if seat.ready {
                "you ended the turn".into()
            } else {
                "the AI plays this seat".into()
            });
        }
        let gained = now.saturating_sub(seat.refilled_ms) / refill;
        if gained > 0 {
            seat.tokens = (u64::from(seat.tokens) + gained).min(u64::from(burst)) as u32;
            seat.refilled_ms += gained * refill;
        }
        if seat.tokens == 0 {
            return Err("too many commands, slow down".into());
        }
        seat.tokens -= 1;
        if bytes.len() > max {
            return Err("command too long".into());
        }
        let game = self.game.as_mut().ok_or("no game")?;
        let mut reader = Reader::new(bytes, 0);
        let command = <M::Command as CommandField>::take(&mut reader, game.view().auth)
            .map_err(|e| format!("unreadable command ({e:?})"))?;
        if !reader.is_at_end() {
            return Err("unreadable command (trailing bytes)".into());
        }
        game.execute_as(PlayerId(part), &self.rules, command)
            .map_err(|e| format!("{e:?}"))?;
        self.collect_journal();
        for c in self.conns.values_mut() {
            c.dirty = true;
        }
        Ok(())
    }

    fn end_turn(&mut self, part: Part, round: u32, ready: bool) {
        if self.phase != Phase::Planning || round != self.round || chat::is_spectator(part) {
            return;
        }
        let seat = &mut self.seats[usize::from(part)];
        if seat.ai_plays || seat.ready == ready {
            return;
        }
        seat.ready = ready;
        self.broadcast_status();
        self.progress();
    }

    fn collect_journal(&mut self) {
        if let Some(game) = self.game.as_mut() {
            let more = game.take_journal();
            self.segment.journal.extend(more);
        }
    }

    // ----- rounds -----

    /// Who plays themselves this round: the seats with a player connected.
    fn begin_round(&mut self) {
        for seat in &mut self.seats {
            seat.ai_plays = !(seat.claimed && seat.conn.is_some());
            seat.ready = false;
        }
        self.set_ai_humans();
    }

    fn set_ai_humans(&mut self) {
        let humans: Vec<PlayerId> = self
            .seats
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.ai_plays)
            .map(|(i, _)| PlayerId(i as u8))
            .collect();
        if let Some(ai) = self.ai.as_mut() {
            ai.set_humans(humans);
        }
    }

    fn window_mask(&self) -> u64 {
        let Some(game) = self.game.as_ref() else {
            return 0;
        };
        let players = game.view().auth.players();
        self.seats
            .iter()
            .enumerate()
            .filter(|(i, s)| !s.ai_plays && players.is_alive(PlayerId(*i as u8)))
            .fold(0, |mask, (i, _)| mask | 1u64 << i)
    }

    fn sync_window(&mut self) {
        let mask = self.window_mask();
        if let Some(game) = self.game.as_mut() {
            game.open_window(mask);
        }
        self.collect_journal();
    }

    fn new_segment(&mut self) {
        let (Some(game), Some(ai)) = (self.game.as_mut(), self.ai.as_ref()) else {
            return;
        };
        if !self.segment.save.is_empty() {
            self.archive.push(std::mem::replace(
                &mut self.segment,
                Segment {
                    round: 0,
                    save: Vec::new(),
                    journal: Vec::new(),
                },
            ));
        }
        let mut save = encode(game, &self.rules);
        save.ai = ai.save();
        self.segment = Segment {
            round: game.view().auth.turn().round,
            save: codec::to_bytes(&save),
            journal: game.begin_journal(&self.rules),
        };
        self.segment_start_round = self.round;
    }

    fn open_planning(&mut self) {
        if self.cfg.segment_rounds > 0
            && self.round >= self.segment_start_round + self.cfg.segment_rounds
        {
            self.collect_journal();
            self.new_segment();
        }
        let Some(game) = self.game.as_ref() else {
            return;
        };
        self.origin = game.view().auth.active_player().0;
        self.sync_window();
        self.phase = Phase::Planning;
        self.deadline_ms = self
            .settings
            .round_timer_secs
            .map(|t| self.now_ms + u64::from(t) * 1000);
        self.flush_now = true;
        for c in self.conns.values_mut() {
            c.dirty = true;
        }
        self.broadcast_status();
        self.broadcast_roster();
    }

    fn close_window(&mut self) {
        if self.phase != Phase::Planning {
            return;
        }
        // A seat that did not end the turn (its player is gone, idle, or the host closed the round
        // or the timer ran out) is played by the AI for the resolution of this round.
        let alive = |game: &Option<GameState<M>>, seat: usize| {
            game.as_ref()
                .is_some_and(|g| g.view().auth.players().is_alive(PlayerId(seat as u8)))
        };
        let idle: Vec<String> = (0..self.seats.len())
            .filter(|&i| {
                let s = &self.seats[i];
                !s.ai_plays && !s.ready && s.conn.is_some() && alive(&self.game, i)
            })
            .map(|i| self.seats[i].name.clone())
            .collect();
        for seat in &mut self.seats {
            if !seat.ai_plays && !seat.ready {
                seat.ai_plays = true;
            }
        }
        if !idle.is_empty() {
            self.notice(&format!(
                "time is up: the AI plays this round for {}",
                idle.join(", ")
            ));
        }
        self.set_ai_humans();
        if let Some(game) = self.game.as_mut() {
            game.close_window();
        }
        self.collect_journal();
        self.phase = Phase::Resolving;
        self.sweeping = true;
        self.deadline_ms = None;
        self.broadcast_status();
        self.broadcast_roster();
    }

    fn all_ready(&self) -> bool {
        let Some(game) = self.game.as_ref() else {
            return false;
        };
        let players = game.view().auth.players();
        self.seats
            .iter()
            .enumerate()
            .filter(|(i, s)| !s.ai_plays && players.is_alive(PlayerId(*i as u8)))
            .all(|(_, s)| s.ready || s.conn.is_none())
    }

    fn anyone_here(&self) -> bool {
        self.seats.iter().any(|s| s.claimed && s.conn.is_some())
    }

    /// Move the match on as far as the events allow (closing a full round, resuming a paused
    /// match); the slow part, the AI turns, runs in `tick`.
    fn progress(&mut self) {
        match self.phase {
            Phase::Planning if self.all_ready() => {
                self.close_window();
            }
            Phase::Paused if self.anyone_here() => {
                self.begin_round();
                self.phase = Phase::Resolving;
                self.broadcast_status();
                self.broadcast_roster();
            }
            _ => {}
        }
    }

    /// Time passes: timers, the AI turns of a resolving round (at most `resolve_steps` steps),
    /// and the changed views.
    pub fn tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        if self.phase == Phase::Planning {
            if self.deadline_ms.is_some_and(|d| now_ms >= d) {
                self.close_window();
            } else {
                self.progress();
            }
        }
        if self.phase == Phase::Resolving {
            self.resolve(self.cfg.resolve_steps);
        }
        self.flush();
    }

    /// Run the round on: end the human turns in order, let the AI play its seats.
    fn resolve(&mut self, steps: u32) {
        for _ in 0..steps {
            let anyone = self.anyone_here();
            let (Some(game), Some(ai)) = (self.game.as_mut(), self.ai.as_mut()) else {
                return;
            };
            if game.is_over() {
                self.collect_journal();
                self.finish();
                return;
            }
            // The AI finishes the turn it is in the middle of before anything else is decided.
            if ai.phase() != DriverPhase::Idle {
                ai.step(game, &self.rules);
                continue;
            }
            let round = game.view().auth.turn().round;
            if round != self.round {
                self.round = round;
                self.metrics.rounds += 1;
                self.sweeping = false;
                self.begin_round_after_sweep();
                continue;
            }
            let pointer = game.view().auth.active_player();
            let human = !self
                .seats
                .get(usize::from(pointer.0))
                .is_none_or(|s| s.ai_plays);
            if human {
                if !self.sweeping {
                    self.collect_journal();
                    self.open_planning();
                    return;
                }
                let end = M::end_turn(pointer);
                if let Err(e) = game.execute(&self.rules, end) {
                    self.notice(&format!(
                        "the server could not end the turn of seat {}: {e:?}",
                        pointer.0
                    ));
                    self.collect_journal();
                    self.phase = Phase::Paused;
                    return;
                }
                continue;
            }
            if !self.sweeping && !anyone && !self.cfg.run_unattended {
                self.collect_journal();
                self.phase = Phase::Paused;
                self.broadcast_status();
                return;
            }
            match ai.step(game, &self.rules) {
                StepOutcome::Waiting => {
                    // The pointer is neither a human nor an AI that can act: nothing more can be played.
                    self.collect_journal();
                    self.notice("the match cannot go on");
                    self.phase = Phase::Paused;
                    return;
                }
                StepOutcome::Worked | StepOutcome::PlayerDone(_) | StepOutcome::RoundDone => {}
            }
        }
        self.collect_journal();
    }

    fn begin_round_after_sweep(&mut self) {
        self.begin_round();
        self.collect_journal();
        for c in self.conns.values_mut() {
            c.dirty = true;
        }
    }

    fn finish(&mut self) {
        self.phase = Phase::Over;
        let winner = self.game.as_ref().and_then(|g| g.winner()).map(|p| p.0);
        self.broadcast_status();
        self.broadcast(&ServerMsg::Over { winner });
        for c in self.conns.values_mut() {
            c.dirty = true;
        }
    }

    /// Send each seat the difference between what it was last told and what it may know now.
    /// While resolving nothing is sent but at the end: the views are built when the round opens.
    fn flush(&mut self) {
        if !matches!(self.phase, Phase::Planning | Phase::Over | Phase::Paused) {
            return;
        }
        let due: Vec<ConnId> = self
            .conns
            .iter()
            .filter(|(_, c)| {
                c.dirty
                    && c.part.is_some()
                    && (self.flush_now
                        || self.now_ms >= c.flushed_ms + self.cfg.flush_ms
                        || self.phase != Phase::Planning)
            })
            .map(|(id, _)| *id)
            .collect();
        for conn in due {
            let Some(part) = self.conns.get(&conn).and_then(|c| c.part) else {
                continue;
            };
            let Some(next) = self.view_for(part) else {
                if let Some(c) = self.conns.get_mut(&conn) {
                    c.dirty = false;
                }
                continue;
            };
            let delta = match self.conns.get(&conn).and_then(|c| c.last.as_ref()) {
                Some(last) => last.diff(&next),
                None => {
                    self.send(conn, &ServerMsg::Snapshot(next.clone()));
                    if let Some(c) = self.conns.get_mut(&conn) {
                        c.last = Some(next);
                        c.dirty = false;
                        c.flushed_ms = self.now_ms;
                    }
                    continue;
                }
            };
            if !delta.is_empty()
                || delta.round != self.conns[&conn].last.as_ref().map_or(0, |l| l.round)
            {
                self.send(conn, &ServerMsg::Delta(delta));
            }
            if let Some(c) = self.conns.get_mut(&conn) {
                c.last = Some(next);
                c.dirty = false;
                c.flushed_ms = self.now_ms;
            }
        }
        self.flush_now = false;
    }

    // ----- save and resume -----

    /// The seats of the match for a restart: name, token, team.
    pub fn seat_records(&self) -> Vec<(String, Token, Option<u8>)> {
        self.seats
            .iter()
            .filter(|s| s.claimed)
            .map(|s| (s.name.clone(), s.token, s.team))
            .collect()
    }

    /// A match from a save: the game and the AI memory are the save's, the seats are the ones
    /// `seat_records` listed (their players come back with their tokens), and the match waits
    /// for the first of them.
    pub fn resume(
        rules: Arc<CompiledRuleset<M>>,
        spec: MatchSpec,
        cfg: ServerConfig,
        entropy: Box<dyn Entropy>,
        save: &[u8],
        seats: &[(String, Token, Option<u8>)],
    ) -> Result<Self, String> {
        let loaded = load(save, &rules, RulesMatch::Exact).map_err(|e| e.to_string())?;
        let mut server = Self::new(
            rules,
            spec,
            cfg,
            entropy,
            Box::new(|_, _| unreachable!("a resumed match has its game")),
        );
        let humans: Vec<PlayerId> = (0..server.spec.humans).map(PlayerId).collect();
        let difficulty = if server.spec.difficulty.is_empty() {
            server.rules.default_difficulty().to_owned()
        } else {
            server.spec.difficulty.clone()
        };
        let mut ai = M::Ai::for_game(&server.rules, &difficulty, humans);
        let tiles = loaded.game.view().auth.map().tile_count();
        ai.load(&loaded.ai, tiles, &server.rules)
            .map_err(|e| e.to_string())?;
        for (i, (name, token, team)) in seats.iter().enumerate() {
            let Some(seat) = server.seats.get_mut(i) else {
                break;
            };
            seat.name = name.clone();
            seat.token = *token;
            seat.team = *team;
            seat.claimed = true;
            server.chat.join(i as u8, *team);
        }
        server.host = Some(0).filter(|_| !seats.is_empty());
        server.round = loaded.game.view().auth.turn().round;
        server.game = Some(loaded.game);
        server.ai = Some(ai);
        server.phase = Phase::Paused;
        server.new_segment();
        Ok(server)
    }
}
