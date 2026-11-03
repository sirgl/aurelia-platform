//! The messages of the protocol, one WebSocket binary message each (`wire::frame`).
//!
//! A client says who it is and which build it runs (`Hello`); the server answers with `Welcome`
//! and the seat's view, or refuses with a reason and closes. Commands travel as the bytes of the
//! command journal (`kernel::replay::codec`): the layout of a rules module's commands is
//! versioned by `CommandCodec::COMMAND_CODEC_VERSION`, and the Hello carries it together with
//! the module's save version and the rules' hash, so clients of another build or other rules
//! are refused at the door.

use crate::chat::{Channel, Entry};
use crate::view::{SeatDelta, SeatView};
use crate::wire::{frame, unframe, Reader, Wire, WireError, Writer};

/// The version of this protocol: a change of any message below bumps it.
pub const PROTO_VERSION: u16 = 2;

/// A per-game secret that proves a returning player is the one who took the seat.
pub type Token = [u8; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Player,
    Spectator,
}

/// What a client says first: which build and rules it runs and who it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub proto: u16,
    /// `RulesModule::ID.name` and `.version` (the save layout).
    pub module: String,
    pub module_version: u32,
    pub command_codec: u32,
    /// `CompiledRuleset::hash`.
    pub rules_hash: u64,
    pub name: String,
    pub role: Role,
    /// The match code the player was given.
    pub code: String,
    /// A returning player's token (None: a new player).
    pub token: Option<Token>,
}

/// Where a chat message goes, as the client names it; the server stamps the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatTarget {
    All,
    /// The sender's own team.
    Team,
    /// A player by seat.
    Dm(u8),
}

/// The settings of a match the host may change (in the lobby; the timer also during the game).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Settings {
    /// Seconds a round stays open once it begins (None: it waits for every human). When it runs out,
    /// the AI plays the seats that have not ended their turn.
    pub round_timer_secs: Option<u32>,
    /// Spectators are shown the whole map (off by default: a spectator could tell a player what he
    /// sees). Settable in the lobby only.
    pub spectators_see_all: bool,
}

/// The shortest and longest round timer the host may set, in seconds.
pub const TIMER_SECS: std::ops::RangeInclusive<u32> = 5..=3600;

impl Settings {
    fn put(&self, w: &mut Writer) {
        w.opt(&self.round_timer_secs);
        w.bool(self.spectators_see_all);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            round_timer_secs: r.opt()?,
            spectators_see_all: r.bool()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientMsg {
    Hello(Hello),
    /// A command of the sender's seat, in the journal's bytes. `seq` comes back in the result.
    Command {
        seq: u32,
        bytes: Vec<u8>,
    },
    /// The seat is done with this round.
    EndTurn {
        round: u32,
    },
    /// Take it back, while the round is open.
    Unready {
        round: u32,
    },
    Chat {
        target: ChatTarget,
        text: String,
    },
    /// Stop (or resume) receiving the messages of a participant.
    Block {
        part: u8,
        on: bool,
    },
    /// Lobby: ready or not.
    Ready(bool),
    /// Lobby: the host starts the match.
    Start,
    /// The host closes the round now, whoever has not ended the turn.
    ForceClose {
        round: u32,
    },
    Ping(u64),
    /// The host changes the settings of the match.
    Settings(Settings),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Lobby,
    /// The window is open: the humans act at once.
    Planning,
    /// The turns end in player order and the AI plays.
    Resolving,
    /// Nobody plays themselves: waiting for a player to come back.
    Paused,
    Over,
}

impl Phase {
    fn code(self) -> u8 {
        match self {
            Phase::Lobby => 0,
            Phase::Planning => 1,
            Phase::Resolving => 2,
            Phase::Paused => 3,
            Phase::Over => 4,
        }
    }
    fn from_code(c: u8) -> Result<Self, WireError> {
        Ok(match c {
            0 => Phase::Lobby,
            1 => Phase::Planning,
            2 => Phase::Resolving,
            3 => Phase::Paused,
            4 => Phase::Over,
            _ => return Err(WireError::Invalid("phase")),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectCode {
    /// Protocol, module, save layout, command layout or rules differ.
    VersionMismatch,
    WrongCode,
    BadToken,
    /// No seat for a new player.
    Full,
    Malformed,
    NotAllowed,
}

impl RejectCode {
    fn code(self) -> u8 {
        self as u8
    }
    fn from_code(c: u8) -> Result<Self, WireError> {
        Ok(match c {
            0 => Self::VersionMismatch,
            1 => Self::WrongCode,
            2 => Self::BadToken,
            3 => Self::Full,
            4 => Self::Malformed,
            5 => Self::NotAllowed,
            _ => return Err(WireError::Invalid("reject code")),
        })
    }
}

/// One seat of the match as everybody sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    pub seat: u8,
    pub name: String,
    /// The seat is meant for a human (it may be played by the AI now).
    pub human: bool,
    pub connected: bool,
    /// The AI plays the seat this round.
    pub ai_plays: bool,
    /// Ended the turn (or is ready, in the lobby).
    pub ready: bool,
    pub team: Option<u8>,
}

impl Wire for RosterEntry {
    fn put(&self, w: &mut Writer) {
        w.u8(self.seat);
        w.str(&self.name);
        w.u8(self.human as u8
            | (self.connected as u8) << 1
            | (self.ai_plays as u8) << 2
            | (self.ready as u8) << 3);
        w.opt(&self.team);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let (seat, name, f) = (r.u8()?, r.str()?, r.u8()?);
        if f > 15 {
            return Err(WireError::Invalid("roster flags"));
        }
        Ok(Self {
            seat,
            name,
            human: f & 1 != 0,
            connected: f & 2 != 0,
            ai_plays: f & 4 != 0,
            ready: f & 8 != 0,
            team: r.opt()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerMsg {
    Welcome {
        part: u8,
        token: Token,
        host: bool,
        phase: Phase,
        round: u32,
        roster: Vec<RosterEntry>,
    },
    Reject {
        code: RejectCode,
        text: String,
    },
    Snapshot(SeatView),
    Delta(SeatDelta),
    CommandResult {
        seq: u32,
        ok: bool,
        text: String,
    },
    /// The round and who has ended the turn; `closes_in_ms` is the timer, when there is one.
    Status {
        round: u32,
        phase: Phase,
        ready: Vec<u8>,
        closes_in_ms: Option<u32>,
    },
    Roster(Vec<RosterEntry>),
    Chat(Entry),
    ChatHistory(Vec<Entry>),
    ChatRefused(String),
    Over {
        winner: Option<u8>,
    },
    Pong(u64),
    /// The settings in force (sent on joining and whenever the host changes them).
    Settings(Settings),
}

impl Wire for Channel {
    fn put(&self, w: &mut Writer) {
        match *self {
            Channel::All => w.u8(0),
            Channel::Team(t) => {
                w.u8(1);
                w.u8(t);
            }
            Channel::Dm(a, b) => {
                w.u8(2);
                w.u8(a);
                w.u8(b);
            }
            Channel::System => w.u8(3),
        }
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(match r.u8()? {
            0 => Channel::All,
            1 => Channel::Team(r.u8()?),
            2 => Channel::Dm(r.u8()?, r.u8()?),
            3 => Channel::System,
            _ => return Err(WireError::Invalid("channel")),
        })
    }
}

impl Wire for Entry {
    fn put(&self, w: &mut Writer) {
        w.u64(self.id);
        w.u64(self.at_ms);
        self.channel.put(w);
        w.opt(&self.from);
        w.str(&self.text);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            id: r.u64()?,
            at_ms: r.u64()?,
            channel: Channel::take(r)?,
            from: r.opt()?,
            text: r.str()?,
        })
    }
}

fn put_token(w: &mut Writer, t: &Token) {
    w.bytes.extend_from_slice(t);
}

fn take_token(r: &mut Reader<'_>) -> Result<Token, WireError> {
    let mut t = [0u8; 16];
    for b in &mut t {
        *b = r.u8()?;
    }
    Ok(t)
}

impl ClientMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        let kind = match self {
            ClientMsg::Hello(h) => {
                w.u16(h.proto);
                w.str(&h.module);
                w.u32(h.module_version);
                w.u32(h.command_codec);
                w.u64(h.rules_hash);
                w.str(&h.name);
                w.u8(matches!(h.role, Role::Spectator) as u8);
                w.str(&h.code);
                match &h.token {
                    None => w.u8(0),
                    Some(t) => {
                        w.u8(1);
                        put_token(&mut w, t);
                    }
                }
                0
            }
            ClientMsg::Command { seq, bytes } => {
                w.u32(*seq);
                w.blob(bytes);
                1
            }
            ClientMsg::EndTurn { round } => {
                w.u32(*round);
                2
            }
            ClientMsg::Unready { round } => {
                w.u32(*round);
                3
            }
            ClientMsg::Chat { target, text } => {
                match *target {
                    ChatTarget::All => w.u8(0),
                    ChatTarget::Team => w.u8(1),
                    ChatTarget::Dm(seat) => {
                        w.u8(2);
                        w.u8(seat);
                    }
                }
                w.str(text);
                4
            }
            ClientMsg::Block { part, on } => {
                w.u8(*part);
                w.bool(*on);
                5
            }
            ClientMsg::Ready(on) => {
                w.bool(*on);
                6
            }
            ClientMsg::Start => 7,
            ClientMsg::ForceClose { round } => {
                w.u32(*round);
                8
            }
            ClientMsg::Ping(n) => {
                w.u64(*n);
                9
            }
            ClientMsg::Settings(settings) => {
                settings.put(&mut w);
                10
            }
        };
        frame(kind, &w.bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let (kind, payload) = unframe(bytes)?;
        let r = &mut Reader::new(&payload);
        let msg = match kind {
            0 => ClientMsg::Hello(Hello {
                proto: r.u16()?,
                module: r.str()?,
                module_version: r.u32()?,
                command_codec: r.u32()?,
                rules_hash: r.u64()?,
                name: r.str()?,
                role: if r.u8()? == 1 {
                    Role::Spectator
                } else {
                    Role::Player
                },
                code: r.str()?,
                token: match r.u8()? {
                    0 => None,
                    1 => Some(take_token(r)?),
                    _ => return Err(WireError::Invalid("token tag")),
                },
            }),
            1 => ClientMsg::Command {
                seq: r.u32()?,
                bytes: r.blob()?,
            },
            2 => ClientMsg::EndTurn { round: r.u32()? },
            3 => ClientMsg::Unready { round: r.u32()? },
            4 => ClientMsg::Chat {
                target: match r.u8()? {
                    0 => ChatTarget::All,
                    1 => ChatTarget::Team,
                    2 => ChatTarget::Dm(r.u8()?),
                    _ => return Err(WireError::Invalid("chat target")),
                },
                text: r.str()?,
            },
            5 => ClientMsg::Block {
                part: r.u8()?,
                on: r.bool()?,
            },
            6 => ClientMsg::Ready(r.bool()?),
            7 => ClientMsg::Start,
            8 => ClientMsg::ForceClose { round: r.u32()? },
            9 => ClientMsg::Ping(r.u64()?),
            10 => ClientMsg::Settings(Settings::take(r)?),
            _ => return Err(WireError::Invalid("client message")),
        };
        if !r.is_at_end() {
            return Err(WireError::Invalid("trailing bytes"));
        }
        Ok(msg)
    }
}

impl ServerMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        let kind = match self {
            ServerMsg::Welcome {
                part,
                token,
                host,
                phase,
                round,
                roster,
            } => {
                w.u8(*part);
                put_token(&mut w, token);
                w.bool(*host);
                w.u8(phase.code());
                w.u32(*round);
                w.list(roster);
                0
            }
            ServerMsg::Reject { code, text } => {
                w.u8(code.code());
                w.str(text);
                1
            }
            ServerMsg::Snapshot(view) => {
                view.put(&mut w);
                2
            }
            ServerMsg::Delta(delta) => {
                delta.put(&mut w);
                3
            }
            ServerMsg::CommandResult { seq, ok, text } => {
                w.u32(*seq);
                w.bool(*ok);
                w.str(text);
                4
            }
            ServerMsg::Status {
                round,
                phase,
                ready,
                closes_in_ms,
            } => {
                w.u32(*round);
                w.u8(phase.code());
                w.list(ready);
                w.opt(closes_in_ms);
                5
            }
            ServerMsg::Roster(roster) => {
                w.list(roster);
                6
            }
            ServerMsg::Chat(entry) => {
                entry.put(&mut w);
                7
            }
            ServerMsg::ChatHistory(entries) => {
                w.list(entries);
                8
            }
            ServerMsg::ChatRefused(text) => {
                w.str(text);
                9
            }
            ServerMsg::Over { winner } => {
                w.opt(winner);
                10
            }
            ServerMsg::Pong(n) => {
                w.u64(*n);
                11
            }
            ServerMsg::Settings(settings) => {
                settings.put(&mut w);
                12
            }
        };
        frame(kind, &w.bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let (kind, payload) = unframe(bytes)?;
        let r = &mut Reader::new(&payload);
        let msg = match kind {
            0 => ServerMsg::Welcome {
                part: r.u8()?,
                token: take_token(r)?,
                host: r.bool()?,
                phase: Phase::from_code(r.u8()?)?,
                round: r.u32()?,
                roster: r.list()?,
            },
            1 => ServerMsg::Reject {
                code: RejectCode::from_code(r.u8()?)?,
                text: r.str()?,
            },
            2 => ServerMsg::Snapshot(SeatView::take(r)?),
            3 => ServerMsg::Delta(SeatDelta::take(r)?),
            4 => ServerMsg::CommandResult {
                seq: r.u32()?,
                ok: r.bool()?,
                text: r.str()?,
            },
            5 => ServerMsg::Status {
                round: r.u32()?,
                phase: Phase::from_code(r.u8()?)?,
                ready: r.list()?,
                closes_in_ms: r.opt()?,
            },
            6 => ServerMsg::Roster(r.list()?),
            7 => ServerMsg::Chat(Entry::take(r)?),
            8 => ServerMsg::ChatHistory(r.list()?),
            9 => ServerMsg::ChatRefused(r.str()?),
            10 => ServerMsg::Over { winner: r.opt()? },
            11 => ServerMsg::Pong(r.u64()?),
            12 => ServerMsg::Settings(Settings::take(r)?),
            _ => return Err(WireError::Invalid("server message")),
        };
        if !r.is_at_end() {
            return Err(WireError::Invalid("trailing bytes"));
        }
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Hello {
        Hello {
            proto: PROTO_VERSION,
            module: "sandbox".into(),
            module_version: 2,
            command_codec: 1,
            rules_hash: 0xdead_beef,
            name: "Ada".into(),
            role: Role::Player,
            code: "K7QF".into(),
            token: Some([9; 16]),
        }
    }

    #[test]
    fn client_messages_round_trip() {
        let all = vec![
            ClientMsg::Hello(hello()),
            ClientMsg::Command {
                seq: 4,
                bytes: vec![1, 2, 3],
            },
            ClientMsg::EndTurn { round: 12 },
            ClientMsg::Unready { round: 12 },
            ClientMsg::Chat {
                target: ChatTarget::Dm(3),
                text: "hi".into(),
            },
            ClientMsg::Chat {
                target: ChatTarget::Team,
                text: "hi".into(),
            },
            ClientMsg::Block { part: 2, on: true },
            ClientMsg::Ready(true),
            ClientMsg::Start,
            ClientMsg::ForceClose { round: 3 },
            ClientMsg::Ping(77),
            ClientMsg::Settings(Settings {
                round_timer_secs: Some(90),
                spectators_see_all: true,
            }),
        ];
        for msg in all {
            assert_eq!(ClientMsg::decode(&msg.encode()), Ok(msg.clone()), "{msg:?}");
        }
    }

    #[test]
    fn server_messages_round_trip() {
        let roster = vec![RosterEntry {
            seat: 0,
            name: "Ada".into(),
            human: true,
            connected: true,
            ai_plays: false,
            ready: true,
            team: Some(1),
        }];
        let all = vec![
            ServerMsg::Welcome {
                part: 0,
                token: [3; 16],
                host: true,
                phase: Phase::Planning,
                round: 5,
                roster: roster.clone(),
            },
            ServerMsg::Reject {
                code: RejectCode::VersionMismatch,
                text: "other rules".into(),
            },
            ServerMsg::CommandResult {
                seq: 1,
                ok: false,
                text: "not your unit".into(),
            },
            ServerMsg::Status {
                round: 5,
                phase: Phase::Resolving,
                ready: vec![0, 1],
                closes_in_ms: Some(1500),
            },
            ServerMsg::Roster(roster),
            ServerMsg::Chat(Entry {
                id: 3,
                at_ms: 9,
                channel: Channel::Dm(0, 2),
                from: Some(0),
                text: "x".into(),
            }),
            ServerMsg::ChatHistory(vec![Entry {
                id: 1,
                at_ms: 0,
                channel: Channel::System,
                from: None,
                text: "y".into(),
            }]),
            ServerMsg::ChatRefused("slow down".into()),
            ServerMsg::Over { winner: Some(1) },
            ServerMsg::Pong(5),
            ServerMsg::Settings(Settings {
                round_timer_secs: None,
                spectators_see_all: false,
            }),
        ];
        for msg in all {
            assert_eq!(ServerMsg::decode(&msg.encode()), Ok(msg.clone()), "{msg:?}");
        }
    }

    #[test]
    fn garbage_is_an_error_and_never_a_panic() {
        assert!(ClientMsg::decode(&[]).is_err());
        assert!(ClientMsg::decode(&[0, 99]).is_err());
        assert!(ClientMsg::decode(&[0, 1, 255, 255, 255, 255, 255, 255, 255, 255]).is_err());
        let mut hello = ClientMsg::Hello(hello()).encode();
        hello.truncate(hello.len() - 3);
        assert!(ClientMsg::decode(&hello).is_err());
        // Every prefix and every one-byte corruption of a real frame is handled.
        let real = ClientMsg::Hello(self::hello()).encode();
        for cut in 0..real.len() {
            let _ = ClientMsg::decode(&real[..cut]);
        }
        for at in 0..real.len() {
            for bit in 0..8 {
                let mut bad = real.clone();
                bad[at] ^= 1 << bit;
                let _ = ClientMsg::decode(&bad);
            }
        }
    }
}
