//! The client side of the protocol as a state machine: frames in, state and events out, frames
//! to send. No I/O here; the native client, the browser (a WebSocket of the page) and Android
//! feed it bytes. It keeps the seat's view up to date from the snapshot and the deltas, and the
//! chat, the roster and the round status.

use kernel::replay::codec::CommandCodec;
use kernel::rules::CompiledRuleset;

use crate::chat::Entry;
use crate::proto::{
    ChatTarget, ClientMsg, Hello, Phase, RejectCode, Role, RosterEntry, ServerMsg, Settings, Token,
    PROTO_VERSION,
};
use crate::view::SeatView;
use crate::wire::WireError;

/// The Hello of a client built from `rules`: the build's versions, the match code, who it is.
pub fn hello_for<M: CommandCodec>(
    rules: &CompiledRuleset<M>,
    name: &str,
    code: &str,
    role: Role,
    token: Option<Token>,
) -> Hello {
    Hello {
        proto: PROTO_VERSION,
        module: M::ID.name.to_owned(),
        module_version: M::ID.version,
        command_codec: M::COMMAND_CODEC_VERSION,
        rules_hash: rules.hash(),
        name: name.to_owned(),
        role,
        code: code.to_owned(),
        token,
    }
}

/// What a frame meant, for the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Welcome,
    Refused {
        code: RejectCode,
        text: String,
    },
    /// The view changed.
    View,
    CommandResult {
        seq: u32,
        ok: bool,
        text: String,
    },
    Status,
    Roster,
    Chat(Entry),
    ChatRefused(String),
    Over {
        winner: Option<u8>,
    },
    Pong(u64),
    Settings,
}

#[derive(Debug)]
pub struct Client {
    hello: Hello,
    next_seq: u32,
    pub part: Option<u8>,
    pub token: Option<Token>,
    pub host: bool,
    pub phase: Phase,
    pub round: u32,
    pub view: Option<SeatView>,
    pub roster: Vec<RosterEntry>,
    pub ready: Vec<u8>,
    pub closes_in_ms: Option<u32>,
    pub chat: Vec<Entry>,
    pub winner: Option<Option<u8>>,
    /// The settings in force (the host changes them).
    pub settings: Settings,
    /// Bytes received, for the measurements.
    pub bytes_in: u64,
}

impl Client {
    pub fn new(hello: Hello) -> Self {
        Self {
            hello,
            next_seq: 1,
            part: None,
            token: None,
            host: false,
            phase: Phase::Lobby,
            round: 0,
            view: None,
            roster: Vec::new(),
            ready: Vec::new(),
            closes_in_ms: None,
            chat: Vec::new(),
            winner: None,
            settings: Settings::default(),
            bytes_in: 0,
        }
    }

    /// The first frame to send.
    pub fn hello_frame(&self) -> Vec<u8> {
        ClientMsg::Hello(self.hello.clone()).encode()
    }

    /// A command in the journal's bytes; returns its number and the frame.
    pub fn command_frame(&mut self, bytes: Vec<u8>) -> (u32, Vec<u8>) {
        let seq = self.next_seq;
        self.next_seq += 1;
        (seq, ClientMsg::Command { seq, bytes }.encode())
    }

    pub fn end_turn_frame(&self) -> Vec<u8> {
        ClientMsg::EndTurn { round: self.round }.encode()
    }

    /// The host's new settings.
    pub fn settings_frame(&self, settings: Settings) -> Vec<u8> {
        ClientMsg::Settings(settings).encode()
    }

    pub fn chat_frame(&self, target: ChatTarget, text: &str) -> Vec<u8> {
        ClientMsg::Chat {
            target,
            text: text.to_owned(),
        }
        .encode()
    }

    /// A frame from the server.
    pub fn handle(&mut self, frame: &[u8]) -> Result<Event, WireError> {
        self.bytes_in += frame.len() as u64;
        Ok(match ServerMsg::decode(frame)? {
            ServerMsg::Welcome {
                part,
                token,
                host,
                phase,
                round,
                roster,
            } => {
                self.part = Some(part);
                self.token = Some(token);
                self.host = host;
                self.phase = phase;
                self.round = round;
                self.roster = roster;
                Event::Welcome
            }
            ServerMsg::Reject { code, text } => Event::Refused { code, text },
            ServerMsg::Snapshot(view) => {
                self.round = view.round;
                self.view = Some(view);
                Event::View
            }
            ServerMsg::Delta(delta) => {
                let view = self
                    .view
                    .as_mut()
                    .ok_or(WireError::Invalid("delta before snapshot"))?;
                view.apply(&delta)?;
                self.round = delta.round;
                Event::View
            }
            ServerMsg::CommandResult { seq, ok, text } => Event::CommandResult { seq, ok, text },
            ServerMsg::Status {
                round,
                phase,
                ready,
                closes_in_ms,
            } => {
                self.round = round;
                self.phase = phase;
                self.ready = ready;
                self.closes_in_ms = closes_in_ms;
                Event::Status
            }
            ServerMsg::Roster(roster) => {
                self.roster = roster;
                Event::Roster
            }
            ServerMsg::Chat(entry) => {
                self.chat.push(entry.clone());
                Event::Chat(entry)
            }
            ServerMsg::ChatHistory(entries) => {
                self.chat = entries;
                Event::Roster
            }
            ServerMsg::ChatRefused(text) => Event::ChatRefused(text),
            ServerMsg::Over { winner } => {
                self.winner = Some(winner);
                self.phase = Phase::Over;
                Event::Over { winner }
            }
            ServerMsg::Pong(n) => Event::Pong(n),
            ServerMsg::Settings(settings) => {
                self.settings = settings;
                Event::Settings
            }
        })
    }
}
