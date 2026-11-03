//! The command journal (platform/docs/saves.md §3): the commands played after a save
//! and the world's hash after every round, so a game replays from its save
//! without its players, and a bug report is the save and its journal.
//!
//! Layout: `AURLOG\0\1`, the module's command layout version (u32), the
//! rules' fingerprint (u64), the hash of the world it starts from (u64), the
//! rules' label (a string: the module's name for the variant of its rules,
//! `CommandCodec::rules_label`, so a reader picks the rules before it
//! replays; Lands & Laws writes its mode there), then records to the end of
//! the file. A journal that starts `AURLOG\0\0` is the first layout: the same
//! without the label, which reads as empty. The records are: a command (tag 0 and the command,
//! `replay::codec`) or a checkpoint (tag 1, the round that ended (u32) and
//! the world's hash after it (u64)). Only applied commands are recorded: a
//! rejected command leaves the world unchanged (`RulesModule::execute`), so
//! the AI's refused attempts cost nothing. A file cut inside a record (the
//! game stopped while writing) reads to its last whole record.
//!
//! A replay runs on the rules the journal was recorded with only
//! (`RulesMatch::Exact`): the content ids in its commands are rows of those
//! rules.

use crate::command::CommandError;
use crate::module::RulesModule;
use crate::persistence::codec::{CodecError, Field, Reader};
use crate::persistence::decode::RulesMatch;
use crate::persistence::load::{load, LoadError};
use crate::replay::codec::{CommandCodec, CommandField};
use crate::replay::hash::authoritative_hash;
use crate::rules::CompiledRuleset;
use crate::world::state::{AuthWorld, GameState};

const MAGIC: &[u8; 8] = b"AURLOG\0\x01";
/// The first layout of the header: no label.
const MAGIC_UNLABELLED: &[u8; 8] = b"AURLOG\0\0";
const COMMAND: u8 = 0;
const CHECKPOINT: u8 = 1;
/// A command of a seat of the open window (tag 2, the seat (u8), the command).
const COMMAND_AS: u8 = 2;
/// The window opened or closed (tag 3, the seats as a bit set (u64); 0 closes).
const WINDOW: u8 = 3;

/// What a journal starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalHeader {
    /// `CommandCodec::COMMAND_CODEC_VERSION` of the build that wrote it.
    pub command_codec: u32,
    /// The rules' fingerprint (`CompiledRuleset::hash`), as in its save's header.
    pub rules: u64,
    /// `authoritative_hash` of the world it starts from: its save's.
    pub world: u64,
    /// `CommandCodec::rules_label` of the rules it was recorded with (empty:
    /// the module names no variant, or the journal is of the first layout).
    pub label: String,
}

impl JournalHeader {
    /// The header of a journal of `game` as it is now.
    pub fn of<M: CommandCodec>(game: &GameState<M>, rules: &CompiledRuleset<M>) -> Self {
        Self { command_codec: M::COMMAND_CODEC_VERSION, rules: rules.hash(), world: authoritative_hash(game), label: M::rules_label(rules) }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        Field::put(&self.command_codec, &mut out);
        Field::put(&self.rules, &mut out);
        Field::put(&self.world, &mut out);
        Field::put(&self.label, &mut out);
        out
    }

    /// The header of `journal` and a reader at its first record.
    pub fn read(journal: &[u8]) -> Result<(Self, Reader<'_>), ReplayError> {
        let (records, labelled) = match journal.strip_prefix(MAGIC.as_slice()) {
            Some(records) => (records, true),
            None => (journal.strip_prefix(MAGIC_UNLABELLED.as_slice()).ok_or(ReplayError::NotAJournal)?, false),
        };
        let mut r = Reader::new(records, 0);
        let not_a_journal = |_| ReplayError::NotAJournal;
        let header = Self {
            command_codec: Field::take(&mut r).map_err(not_a_journal)?,
            rules: Field::take(&mut r).map_err(not_a_journal)?,
            world: Field::take(&mut r).map_err(not_a_journal)?,
            label: if labelled { Field::take(&mut r).map_err(not_a_journal)? } else { String::new() },
        };
        Ok((header, r))
    }
}

/// The journal a game records (`GameState::begin_journal`). A copy of the
/// game (a probe, a forecast) records nothing: its commands are not the
/// game's.
pub(crate) struct Recording<M: RulesModule>(Option<Recorder<M>>);

struct Recorder<M: RulesModule> {
    put: fn(&M::Command, &AuthWorld<M>, &mut Vec<u8>),
    /// Records since the last `take`.
    records: Vec<u8>,
}

impl<M: RulesModule> Recording<M> {
    pub(crate) fn off() -> Self {
        Self(None)
    }

    /// A new journal of a module whose commands have a layout.
    pub(crate) fn begin() -> Self
    where
        M: CommandCodec,
    {
        Self(Some(Recorder { put: |command, world, out| command.put(world, out), records: Vec::new() }))
    }

    pub(crate) fn is_on(&self) -> bool {
        self.0.is_some()
    }

    /// Write `command` as the next record (`world` is before it); returns
    /// where the record starts, to drop it if the command is rejected.
    pub(crate) fn command(&mut self, command: &M::Command, world: &AuthWorld<M>) -> Option<usize> {
        let recorder = self.0.as_mut()?;
        let start = recorder.records.len();
        recorder.records.push(COMMAND);
        (recorder.put)(command, world, &mut recorder.records);
        Some(start)
    }

    /// Drop the record from `start` on: its command was rejected.
    pub(crate) fn discard(&mut self, start: Option<usize>) {
        if let (Some(recorder), Some(start)) = (self.0.as_mut(), start) {
            recorder.records.truncate(start);
        }
    }

    /// Like `command`, for a seat of the open window.
    pub(crate) fn command_as(&mut self, seat: crate::ids::PlayerId, command: &M::Command, world: &AuthWorld<M>) -> Option<usize> {
        let recorder = self.0.as_mut()?;
        let start = recorder.records.len();
        recorder.records.push(COMMAND_AS);
        recorder.records.push(seat.0);
        (recorder.put)(command, world, &mut recorder.records);
        Some(start)
    }

    pub(crate) fn window(&mut self, seats: u64) {
        if let Some(recorder) = self.0.as_mut() {
            recorder.records.push(WINDOW);
            Field::put(&seats, &mut recorder.records);
        }
    }

    pub(crate) fn checkpoint(&mut self, round: u32, hash: u64) {
        if let Some(recorder) = self.0.as_mut() {
            recorder.records.push(CHECKPOINT);
            Field::put(&round, &mut recorder.records);
            Field::put(&hash, &mut recorder.records);
        }
    }

    /// The records since the last call.
    pub(crate) fn take(&mut self) -> Vec<u8> {
        self.0.as_mut().map(|recorder| std::mem::take(&mut recorder.records)).unwrap_or_default()
    }
}

impl<M: RulesModule> Clone for Recording<M> {
    fn clone(&self) -> Self {
        Self::off()
    }
}

impl<M: RulesModule> std::fmt::Debug for Recording<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(recorder) => write!(f, "Recording({} bytes)", recorder.records.len()),
            None => f.write_str("Recording(off)"),
        }
    }
}

/// A journal replayed to its end.
#[derive(Debug)]
pub struct Replayed<M: RulesModule> {
    /// The world after the last command.
    pub game: GameState<M>,
    pub commands: usize,
    /// Rounds whose checkpoint matched.
    pub rounds: usize,
    /// The file ends inside a record: it was read to its last whole one.
    pub cut: bool,
}

/// Why a journal does not replay to its end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayError {
    NotAJournal,
    /// Another layout of the module's commands.
    OtherCommands { journal: u32, current: u32 },
    /// Recorded with other rules.
    OtherRules,
    /// Its save does not load.
    Save(LoadError),
    /// A record does not read (records count from 1).
    Damaged { record: usize, error: CodecError },
    /// The replayed game is not the recorded one.
    Diverged(Divergence),
}

/// Where a replay first left the recorded game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divergence {
    /// The round being played.
    pub round: u32,
    /// The record (from 1; 0: the save itself).
    pub record: usize,
    pub mismatch: Mismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    /// The save loads to another world than the one the journal started from.
    Save { journal: u64, loaded: u64 },
    /// A recorded command is refused.
    Rejected(CommandError),
    /// The world's hash after the round is not the recorded one.
    Hash { journal: u64, replayed: u64 },
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAJournal => f.write_str("not a command journal"),
            Self::OtherCommands { journal, current } => {
                write!(f, "the journal has command layout {journal}, this build reads {current}")
            }
            Self::OtherRules => f.write_str("the journal was recorded with other rules"),
            Self::Save(error) => write!(f, "its save does not load ({error})"),
            Self::Damaged { record, error } => write!(f, "record {record} is damaged ({error})"),
            Self::Diverged(d) => write!(f, "diverged in round {} at record {}: {}", d.round, d.record, d.mismatch),
        }
    }
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Save { journal, loaded } => {
                write!(f, "the save loads to world {loaded:016x}, the journal starts from {journal:016x}")
            }
            Self::Rejected(error) => write!(f, "the command is refused ({error:?})"),
            Self::Hash { journal, replayed } => write!(f, "world {replayed:016x}, recorded {journal:016x}"),
        }
    }
}

impl std::error::Error for ReplayError {}

/// One record of a journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Record<C> {
    Command(C),
    CommandAs { seat: crate::ids::PlayerId, command: C },
    Window(u64),
    Checkpoint { round: u32, hash: u64 },
}

/// The next record of a journal, read against `world`: the world the record is to be
/// played in (a command names its units and cities by entity id, which `world` turns
/// into its own handles). A host that plays a journal recorded elsewhere on its own game
/// (the AI turns of a worker) reads and plays it record by record.
pub fn take_record<M: CommandCodec>(r: &mut Reader<'_>, world: &AuthWorld<M>) -> Result<Record<M::Command>, CodecError> {
    match <u8 as Field>::take(r)? {
        COMMAND => Ok(Record::Command(CommandField::take(r, world)?)),
        COMMAND_AS => {
            let seat = crate::ids::PlayerId(Field::take(r)?);
            Ok(Record::CommandAs { seat, command: CommandField::take(r, world)? })
        }
        WINDOW => Ok(Record::Window(Field::take(r)?)),
        CHECKPOINT => Ok(Record::Checkpoint { round: Field::take(r)?, hash: Field::take(r)? }),
        _ => Err(CodecError::Invalid("record tag")),
    }
}

/// Play `journal` from `save` on `rules`: every command and every checkpoint
/// in order, to the first divergence.
pub fn replay<M: CommandCodec>(
    save: &[u8],
    journal: &[u8],
    rules: &CompiledRuleset<M>,
) -> Result<Replayed<M>, ReplayError> {
    let (header, mut r) = JournalHeader::read(journal)?;
    if header.command_codec != M::COMMAND_CODEC_VERSION {
        return Err(ReplayError::OtherCommands { journal: header.command_codec, current: M::COMMAND_CODEC_VERSION });
    }
    if header.rules != rules.hash() {
        return Err(ReplayError::OtherRules);
    }
    let game = load(save, rules, RulesMatch::Exact).map_err(ReplayError::Save)?.game;
    let loaded = authoritative_hash(&game);
    let mut replayed = Replayed { game, commands: 0, rounds: 0, cut: false };
    let diverged = |game: &GameState<M>, record, mismatch| {
        ReplayError::Diverged(Divergence { round: game.view().auth.turn().round, record, mismatch })
    };
    if loaded != header.world {
        return Err(diverged(&replayed.game, 0, Mismatch::Save { journal: header.world, loaded }));
    }
    let mut record = 0;
    while !r.is_at_end() {
        record += 1;
        let game = &mut replayed.game;
        match take_record::<M>(&mut r, game.view().auth) {
            Ok(Record::Command(command)) => {
                let refused = game.execute(rules, command).err();
                if let Some(error) = refused {
                    return Err(diverged(game, record, Mismatch::Rejected(error)));
                }
                replayed.commands += 1;
            }
            Ok(Record::CommandAs { seat, command }) => {
                let refused = game.execute_as(seat, rules, command).err();
                if let Some(error) = refused {
                    return Err(diverged(game, record, Mismatch::Rejected(error)));
                }
                replayed.commands += 1;
            }
            Ok(Record::Window(seats)) => game.open_window(seats),
            Ok(Record::Checkpoint { round, hash }) => {
                let now = authoritative_hash(game);
                if now != hash {
                    let mismatch = Mismatch::Hash { journal: hash, replayed: now };
                    return Err(ReplayError::Diverged(Divergence { round, record, mismatch }));
                }
                replayed.rounds += 1;
            }
            Err(CodecError::Truncated) => {
                replayed.cut = true;
                break;
            }
            Err(error) => return Err(ReplayError::Damaged { record, error }),
        }
    }
    Ok(replayed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{TerrainId, TileId};
    use crate::persistence::codec::to_bytes;
    use crate::persistence::encode::encode;
    use crate::rules::compile::compile_rules;
    use crate::rules::source::SourceRules;
    use crate::test_module::{Order, TestModule};
    use crate::world::state::GameConfig;

    fn rules() -> CompiledRuleset<TestModule> {
        compile_rules(SourceRules::default()).expect("empty rules compile")
    }

    /// A game, its save and the journal of three commands played after it.
    fn recorded(rules: &CompiledRuleset<TestModule>) -> (GameState<TestModule>, Vec<u8>, Vec<u8>) {
        let mut game = GameState::new(rules, GameConfig { players: 2, width: 12, height: 10, rng_seed: 7 });
        let save = to_bytes(&encode(&game, rules));
        let mut journal = game.begin_journal(rules);
        for _ in 0..3 {
            game.execute(rules, Order::Pass).expect("the test module accepts its command");
        }
        journal.extend(game.take_journal());
        (game, save, journal)
    }

    #[test]
    fn a_journal_replays_its_commands_from_its_save() {
        let rules = rules();
        let (game, save, journal) = recorded(&rules);
        let replayed = replay(&save, &journal, &rules).expect("own journal replays");
        assert_eq!((replayed.commands, replayed.rounds, replayed.cut), (3, 0, false));
        assert_eq!(authoritative_hash(&replayed.game), authoritative_hash(&game));
        assert!(game.clone().take_journal().is_empty(), "a copy of the game records nothing");
    }

    #[test]
    fn a_journal_cut_inside_a_record_reads_to_its_last_whole_one() {
        let rules = rules();
        let (_, save, mut journal) = recorded(&rules);
        journal.extend_from_slice(&[CHECKPOINT, 7, 0]);
        let replayed = replay(&save, &journal, &rules).expect("the whole records replay");
        assert_eq!((replayed.commands, replayed.cut), (3, true));
    }

    #[test]
    fn a_journal_of_other_rules_or_another_layout_does_not_replay() {
        let rules = rules();
        let (game, save, journal) = recorded(&rules);
        let records = &journal[JournalHeader::of(&game, &rules).to_bytes().len()..];
        let other = |header: JournalHeader| [header.to_bytes(), records.to_vec()].concat();
        let (header, _) = JournalHeader::read(&journal).expect("own header reads");
        let other_rules = other(JournalHeader { rules: header.rules ^ 1, ..header.clone() });
        assert_eq!(replay(&save, &other_rules, &rules).err(), Some(ReplayError::OtherRules));
        let other_layout = other(JournalHeader { command_codec: 9, ..header });
        let refused = replay(&save, &other_layout, &rules).err();
        assert_eq!(refused, Some(ReplayError::OtherCommands { journal: 9, current: 1 }));
        assert_eq!(replay(&save, b"AURELIA\0", &rules).err(), Some(ReplayError::NotAJournal));
    }

    #[test]
    fn the_header_carries_the_rules_label_and_the_first_layout_reads_without_one() {
        let rules = rules();
        let (game, save, journal) = recorded(&rules);
        let (header, _) = JournalHeader::read(&journal).expect("own header reads");
        assert_eq!(header.label, "", "the test module names no variant of its rules");
        let labelled = JournalHeader { label: "full_200".into(), ..header.clone() };
        let (read, _) = JournalHeader::read(&labelled.to_bytes()).expect("a labelled header reads");
        assert_eq!(read, labelled);
        // The first layout: its own magic and no label after the world's hash.
        let head = JournalHeader::of(&game, &rules).to_bytes();
        let mut empty = Vec::new();
        Field::put(&String::new(), &mut empty);
        let first = [MAGIC_UNLABELLED.as_slice(), &head[MAGIC.len()..head.len() - empty.len()], &journal[head.len()..]].concat();
        let (old, _) = JournalHeader::read(&first).expect("a journal of the first layout reads");
        assert_eq!(old, header);
        let replayed = replay(&save, &first, &rules).expect("a journal of the first layout replays");
        assert_eq!(replayed.commands, 3);
    }

    #[test]
    fn a_damaged_record_or_a_wrong_hash_names_its_record_and_round() {
        let rules = rules();
        let (game, save, journal) = recorded(&rules);
        let damaged = [journal.clone(), vec![9]].concat();
        let error = replay(&save, &damaged, &rules).err();
        assert_eq!(error, Some(ReplayError::Damaged { record: 4, error: CodecError::Invalid("record tag") }));

        let mut forged = journal.clone();
        let mut recording = Recording::<TestModule>::begin();
        recording.checkpoint(1, authoritative_hash(&game) ^ 1);
        forged.extend(recording.take());
        let Some(ReplayError::Diverged(divergence)) = replay(&save, &forged, &rules).err() else {
            panic!("a wrong checkpoint diverges");
        };
        assert_eq!((divergence.round, divergence.record), (1, 4));
    }

    #[test]
    fn a_save_of_another_world_than_the_journal_start_diverges_at_once() {
        let rules = rules();
        let (mut game, _, journal) = recorded(&rules);
        game.debug_set_terrain_batch(&rules, &[(TileId(3), TerrainId(1))]);
        let other_save = to_bytes(&encode(&game, &rules));
        let Some(ReplayError::Diverged(divergence)) = replay(&other_save, &journal, &rules).err() else {
            panic!("another world diverges");
        };
        assert_eq!(divergence.record, 0);
        assert!(matches!(divergence.mismatch, Mismatch::Save { .. }));
    }
}
