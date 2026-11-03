//! Chat: channels, delivery rules, limits and history. Not game state: nothing here is in the
//! command journal or the world hash, and the clock is passed in.
//!
//! Channels: `All` (everyone, spectators read it), `Team(t)` (the players of team `t`), `Dm`
//! (exactly two participants) and `System` (the server's notices). The server stamps the
//! sender from the connection; a client cannot choose it. A direct message is delivered to
//! its sender and its recipient and to nobody else, and it is part of nobody else's history:
//! `history_for` is the only way out of the hub, and it filters by the viewer.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A participant: a player's seat (0..=127) or a spectator (128..).
pub type Part = u8;
/// The first participant number of spectators.
pub const SPECTATOR_BASE: Part = 128;

pub fn is_spectator(part: Part) -> bool {
    part >= SPECTATOR_BASE
}

/// Where a message goes. A direct message names its two participants, smaller first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Channel {
    All,
    Team(u8),
    Dm(Part, Part),
    System,
}

impl Channel {
    /// The direct channel of two participants.
    pub fn dm(a: Part, b: Part) -> Self {
        Channel::Dm(a.min(b), a.max(b))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Grows by one per message of the match.
    pub id: u64,
    pub at_ms: u64,
    pub channel: Channel,
    /// None: the server.
    pub from: Option<Part>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Empty,
    TooLong {
        max: usize,
    },
    /// Try again after this many milliseconds.
    RateLimited {
        retry_ms: u64,
    },
    /// The sender may not write there (a spectator in the team channel, a message to oneself).
    NotAllowed(&'static str),
    UnknownRecipient,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("empty message"),
            Self::TooLong { max } => write!(f, "message longer than {max} characters"),
            Self::RateLimited { retry_ms } => write!(f, "too fast, try again in {retry_ms} ms"),
            Self::NotAllowed(why) => f.write_str(why),
            Self::UnknownRecipient => f.write_str("no such recipient"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatConfig {
    /// Characters per message.
    pub max_chars: usize,
    /// A sender may send this many messages at once …
    pub burst: u32,
    /// … and one more per this many milliseconds.
    pub refill_ms: u64,
    /// The newest messages a participant gets when it joins.
    pub history_on_join: usize,
    /// Messages kept for the match (the oldest go first).
    pub keep: usize,
    /// Spectators may write in `All`.
    pub spectators_write: bool,
    /// Direct messages are written to the match log. Off by default: a direct message lives in
    /// the memory of the server for the history of its two participants and nowhere else.
    pub log_dms: bool,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            max_chars: 400,
            burst: 5,
            refill_ms: 1_000,
            history_on_join: 50,
            keep: 1_000,
            spectators_write: false,
            log_dms: false,
        }
    }
}

#[derive(Debug, Clone)]
struct Member {
    team: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: u32,
    at_ms: u64,
}

/// What `post` decided: the entry and who gets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub entry: Entry,
    pub to: Vec<Part>,
}

pub type Filter = Box<dyn Fn(&str) -> String + Send>;

pub struct ChatHub {
    cfg: ChatConfig,
    members: BTreeMap<Part, Member>,
    /// (blocker, blocked): the blocker receives nothing from the blocked.
    blocks: BTreeSet<(Part, Part)>,
    buckets: BTreeMap<Part, Bucket>,
    entries: VecDeque<Entry>,
    next_id: u64,
    filter: Option<Filter>,
}

impl std::fmt::Debug for ChatHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ChatHub({} members, {} messages)",
            self.members.len(),
            self.entries.len()
        )
    }
}

impl ChatHub {
    pub fn new(cfg: ChatConfig) -> Self {
        Self {
            cfg,
            members: BTreeMap::new(),
            blocks: BTreeSet::new(),
            buckets: BTreeMap::new(),
            entries: VecDeque::new(),
            next_id: 1,
            filter: None,
        }
    }

    /// The profanity hook: every message text passes through it before it is delivered or kept.
    pub fn set_filter(&mut self, filter: Filter) {
        self.filter = Some(filter);
    }

    pub fn config(&self) -> &ChatConfig {
        &self.cfg
    }

    /// A participant is present (a seat of the match or a spectator) and in `team`.
    pub fn join(&mut self, part: Part, team: Option<u8>) {
        self.members.insert(part, Member { team });
    }

    /// A spectator leaves for good; a player's seat stays (it rejoins).
    pub fn leave(&mut self, part: Part) {
        if is_spectator(part) {
            self.members.remove(&part);
            self.blocks.retain(|(a, b)| *a != part && *b != part);
            self.buckets.remove(&part);
        }
    }

    /// `blocker` receives no more messages of `blocked`.
    pub fn block(&mut self, blocker: Part, blocked: Part) {
        if blocker != blocked {
            self.blocks.insert((blocker, blocked));
        }
    }

    pub fn unblock(&mut self, blocker: Part, blocked: Part) {
        self.blocks.remove(&(blocker, blocked));
    }

    pub fn blocks(&self, blocker: Part, blocked: Part) -> bool {
        self.blocks.contains(&(blocker, blocked))
    }

    fn clean(&self, text: &str) -> String {
        let text: String = text
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect();
        let text = text.trim().to_owned();
        match &self.filter {
            Some(filter) => filter(&text),
            None => text,
        }
    }

    /// Who would hear a message of `from` in `channel` (before the blocks).
    fn audience(&self, channel: Channel) -> Vec<Part> {
        match channel {
            Channel::All | Channel::System => self.members.keys().copied().collect(),
            Channel::Team(team) => self
                .members
                .iter()
                .filter(|(p, m)| !is_spectator(**p) && m.team == Some(team))
                .map(|(p, _)| *p)
                .collect(),
            Channel::Dm(a, b) => vec![a, b],
        }
    }

    fn take_token(&mut self, from: Part, now_ms: u64) -> Result<(), Refusal> {
        let (burst, refill) = (self.cfg.burst, self.cfg.refill_ms.max(1));
        let bucket = self.buckets.entry(from).or_insert(Bucket {
            tokens: burst,
            at_ms: now_ms,
        });
        let gained = now_ms.saturating_sub(bucket.at_ms) / refill;
        if gained > 0 {
            bucket.tokens = (bucket.tokens as u64 + gained).min(burst as u64) as u32;
            bucket.at_ms += gained * refill;
        }
        if bucket.tokens == 0 {
            return Err(Refusal::RateLimited {
                retry_ms: refill - now_ms.saturating_sub(bucket.at_ms).min(refill - 1),
            });
        }
        bucket.tokens -= 1;
        Ok(())
    }

    /// A message of `from` (stamped by the server) to `channel`. A direct channel is `Channel::dm`
    /// of the sender and the recipient; the sender must be one of the two.
    pub fn post(
        &mut self,
        from: Part,
        channel: Channel,
        text: &str,
        now_ms: u64,
    ) -> Result<Delivery, Refusal> {
        let sender = self
            .members
            .get(&from)
            .cloned()
            .ok_or(Refusal::NotAllowed("not in the match"))?;
        match channel {
            Channel::System => return Err(Refusal::NotAllowed("only the server writes there")),
            Channel::All if is_spectator(from) && !self.cfg.spectators_write => {
                return Err(Refusal::NotAllowed("spectators do not write"))
            }
            Channel::Team(team) if is_spectator(from) || sender.team != Some(team) => {
                return Err(Refusal::NotAllowed("not in that team"))
            }
            Channel::Dm(a, b) => {
                if a == b {
                    return Err(Refusal::NotAllowed("a message to oneself"));
                }
                if from != a && from != b {
                    return Err(Refusal::NotAllowed("not your conversation"));
                }
                let other = if from == a { b } else { a };
                if is_spectator(from) || is_spectator(other) || !self.members.contains_key(&other) {
                    return Err(Refusal::UnknownRecipient);
                }
            }
            _ => {}
        }
        let text = self.clean(text);
        if text.is_empty() {
            return Err(Refusal::Empty);
        }
        if text.chars().count() > self.cfg.max_chars {
            return Err(Refusal::TooLong {
                max: self.cfg.max_chars,
            });
        }
        self.take_token(from, now_ms)?;
        let entry = Entry {
            id: self.next_id,
            at_ms: now_ms,
            channel,
            from: Some(from),
            text,
        };
        self.next_id += 1;
        // A recipient who blocked the sender gets nothing, and the sender is not told.
        let to = self
            .audience(channel)
            .into_iter()
            .filter(|p| *p == from || !self.blocks.contains(&(*p, from)))
            .collect();
        self.keep(entry.clone());
        Ok(Delivery { entry, to })
    }

    /// A notice of the server to everybody (a player left, the AI took a seat, a round began).
    pub fn system(&mut self, text: &str, now_ms: u64) -> Delivery {
        let entry = Entry {
            id: self.next_id,
            at_ms: now_ms,
            channel: Channel::System,
            from: None,
            text: truncate_chars(text, self.cfg.max_chars),
        };
        self.next_id += 1;
        self.keep(entry.clone());
        Delivery {
            to: self.audience(Channel::System),
            entry,
        }
    }

    fn keep(&mut self, entry: Entry) {
        self.entries.push_back(entry);
        while self.entries.len() > self.cfg.keep {
            self.entries.pop_front();
        }
    }

    /// May `viewer` read a message of `channel`?
    pub fn can_read(&self, viewer: Part, channel: Channel) -> bool {
        match channel {
            Channel::All | Channel::System => self.members.contains_key(&viewer),
            Channel::Team(team) => {
                !is_spectator(viewer)
                    && self
                        .members
                        .get(&viewer)
                        .is_some_and(|m| m.team == Some(team))
            }
            Channel::Dm(a, b) => viewer == a || viewer == b,
        }
    }

    /// The newest messages `viewer` may read, oldest first, without those of senders it blocked.
    pub fn history_for(&self, viewer: Part) -> Vec<Entry> {
        let mut seen: Vec<&Entry> = self
            .entries
            .iter()
            .rev()
            .filter(|e| {
                self.can_read(viewer, e.channel)
                    && e.from
                        .is_none_or(|f| f == viewer || !self.blocks.contains(&(viewer, f)))
            })
            .take(self.cfg.history_on_join)
            .collect();
        seen.reverse();
        seen.into_iter().cloned().collect()
    }

    /// The match log: every message of the shared channels, and direct messages only when the
    /// configuration says so.
    pub fn log(&self) -> Vec<Entry> {
        self.entries
            .iter()
            .filter(|e| self.cfg.log_dms || !matches!(e.channel, Channel::Dm(..)))
            .cloned()
            .collect()
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Part = 0;
    const B: Part = 1;
    const C: Part = 2;
    const D: Part = 3;
    const SPEC: Part = SPECTATOR_BASE;

    /// Four players (A and B in team 1, C in team 2, D without a team) and a spectator.
    fn hub() -> ChatHub {
        let mut hub = ChatHub::new(ChatConfig::default());
        hub.join(A, Some(1));
        hub.join(B, Some(1));
        hub.join(C, Some(2));
        hub.join(D, None);
        hub.join(SPEC, None);
        hub
    }

    #[test]
    fn a_direct_message_goes_to_its_two_ends_and_nobody_else() {
        let mut hub = hub();
        let delivery = hub.post(A, Channel::dm(A, B), "pact?", 0).expect("sent");
        assert_eq!(delivery.to, vec![A, B]);
        assert_eq!(delivery.entry.from, Some(A));
        for outsider in [C, D, SPEC] {
            assert!(
                hub.history_for(outsider).iter().all(|e| e.text != "pact?"),
                "{outsider} must not read it"
            );
            assert!(!hub.can_read(outsider, Channel::dm(A, B)));
        }
        assert_eq!(hub.history_for(B).len(), 1);
        assert_eq!(hub.history_for(A).len(), 1);
        // A third party cannot post into the conversation either.
        assert_eq!(
            hub.post(C, Channel::dm(A, B), "hi", 0),
            Err(Refusal::NotAllowed("not your conversation"))
        );
    }

    #[test]
    fn a_direct_message_is_not_in_the_match_log_unless_asked_for() {
        let mut hub = hub();
        hub.post(A, Channel::dm(A, B), "secret", 0).unwrap();
        hub.post(A, Channel::All, "hello", 10).unwrap();
        assert_eq!(
            hub.log()
                .iter()
                .map(|e| e.text.as_str())
                .collect::<Vec<_>>(),
            vec!["hello"]
        );
        let mut open = ChatHub::new(ChatConfig {
            log_dms: true,
            ..ChatConfig::default()
        });
        open.join(A, None);
        open.join(B, None);
        open.post(A, Channel::dm(A, B), "secret", 0).unwrap();
        assert_eq!(open.log().len(), 1);
    }

    #[test]
    fn all_chat_reaches_everyone_and_team_chat_only_the_team() {
        let mut hub = hub();
        assert_eq!(
            hub.post(A, Channel::All, "gg", 0).unwrap().to,
            vec![A, B, C, D, SPEC]
        );
        assert_eq!(
            hub.post(A, Channel::Team(1), "attack at dawn", 0)
                .unwrap()
                .to,
            vec![A, B]
        );
        assert!(hub
            .history_for(C)
            .iter()
            .all(|e| e.text != "attack at dawn"));
        assert!(hub
            .history_for(SPEC)
            .iter()
            .all(|e| e.text != "attack at dawn"));
        assert_eq!(
            hub.post(C, Channel::Team(1), "let me in", 0),
            Err(Refusal::NotAllowed("not in that team"))
        );
        assert_eq!(
            hub.post(D, Channel::Team(1), "let me in", 0),
            Err(Refusal::NotAllowed("not in that team"))
        );
        assert_eq!(
            hub.post(SPEC, Channel::Team(1), "psst", 0),
            Err(Refusal::NotAllowed("not in that team"))
        );
    }

    #[test]
    fn spectators_read_the_open_channels_and_do_not_write() {
        let mut hub = hub();
        assert_eq!(
            hub.post(SPEC, Channel::All, "hi", 0),
            Err(Refusal::NotAllowed("spectators do not write"))
        );
        assert_eq!(
            hub.post(SPEC, Channel::dm(SPEC, A), "hi", 0),
            Err(Refusal::UnknownRecipient)
        );
        assert_eq!(
            hub.post(A, Channel::dm(A, SPEC), "hi", 0),
            Err(Refusal::UnknownRecipient)
        );
        hub.post(A, Channel::All, "gg", 0).unwrap();
        hub.system("round 2", 5);
        assert_eq!(hub.history_for(SPEC).len(), 2);
        let mut open = ChatHub::new(ChatConfig {
            spectators_write: true,
            ..ChatConfig::default()
        });
        open.join(SPEC, None);
        assert!(open.post(SPEC, Channel::All, "hi", 0).is_ok());
    }

    #[test]
    fn a_sender_is_limited_to_a_burst_and_a_refill() {
        let mut hub = hub();
        for i in 0..5 {
            hub.post(A, Channel::All, &format!("m{i}"), 100)
                .expect("within the burst");
        }
        assert_eq!(
            hub.post(A, Channel::All, "m5", 100),
            Err(Refusal::RateLimited { retry_ms: 1_000 })
        );
        assert!(
            hub.post(B, Channel::All, "other sender", 100).is_ok(),
            "the limit is per sender"
        );
        assert_eq!(
            hub.post(A, Channel::All, "m5", 600),
            Err(Refusal::RateLimited { retry_ms: 500 })
        );
        assert!(
            hub.post(A, Channel::All, "m5", 1_100).is_ok(),
            "one token came back"
        );
        assert!(hub.post(A, Channel::All, "m6", 1_100).is_err());
    }

    #[test]
    fn the_text_is_cleaned_limited_and_filtered() {
        let mut hub = hub();
        assert_eq!(hub.post(A, Channel::All, "   ", 0), Err(Refusal::Empty));
        assert_eq!(
            hub.post(A, Channel::All, "\u{7}\u{1b}", 0),
            Err(Refusal::Empty)
        );
        let long = "x".repeat(401);
        assert_eq!(
            hub.post(A, Channel::All, &long, 0),
            Err(Refusal::TooLong { max: 400 })
        );
        assert_eq!(
            hub.post(A, Channel::All, &"x".repeat(400), 0)
                .unwrap()
                .entry
                .text
                .len(),
            400
        );
        assert_eq!(
            hub.post(A, Channel::All, " hi\u{0}there ", 0)
                .unwrap()
                .entry
                .text,
            "hithere"
        );
        hub.set_filter(Box::new(|t| t.replace("darn", "****")));
        assert_eq!(
            hub.post(B, Channel::All, "darn it", 0).unwrap().entry.text,
            "**** it"
        );
        assert_eq!(
            hub.history_for(C).last().unwrap().text,
            "**** it",
            "the kept text is the filtered one"
        );
    }

    #[test]
    fn a_block_stops_delivery_and_history_and_the_sender_is_not_told() {
        let mut hub = hub();
        hub.block(B, A);
        let all = hub.post(A, Channel::All, "hello all", 0).unwrap();
        assert_eq!(all.to, vec![A, C, D, SPEC]);
        let dm = hub
            .post(A, Channel::dm(A, B), "hello b", 0)
            .expect("the sender sees success");
        assert_eq!(dm.to, vec![A], "only the echo to the sender");
        assert!(
            hub.history_for(B).is_empty(),
            "nor does it turn up in the history of the one who blocked"
        );
        hub.unblock(B, A);
        assert_eq!(hub.post(A, Channel::All, "again", 0).unwrap().to.len(), 5);
        // Only its own blocks count.
        hub.block(C, A);
        assert!(hub.history_for(D).iter().any(|e| e.text == "hello all"));
    }

    #[test]
    fn history_is_bounded_and_ordered() {
        let mut hub = ChatHub::new(ChatConfig {
            history_on_join: 3,
            keep: 5,
            burst: 100,
            ..ChatConfig::default()
        });
        hub.join(A, None);
        hub.join(B, None);
        for i in 0..8 {
            hub.post(A, Channel::All, &format!("m{i}"), i).unwrap();
        }
        let history = hub.history_for(B);
        assert_eq!(
            history.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            vec!["m5", "m6", "m7"]
        );
        assert_eq!(hub.log().len(), 5);
        assert!(history.windows(2).all(|w| w[0].id < w[1].id));
    }

    #[test]
    fn a_stranger_and_the_server_channel_are_refused() {
        let mut hub = hub();
        assert_eq!(
            hub.post(77, Channel::All, "hi", 0),
            Err(Refusal::NotAllowed("not in the match"))
        );
        assert_eq!(
            hub.post(A, Channel::System, "hi", 0),
            Err(Refusal::NotAllowed("only the server writes there"))
        );
        assert_eq!(
            hub.post(A, Channel::dm(A, A), "hi", 0),
            Err(Refusal::NotAllowed("a message to oneself"))
        );
        assert_eq!(
            hub.post(A, Channel::dm(A, 9), "hi", 0),
            Err(Refusal::UnknownRecipient)
        );
    }
}
