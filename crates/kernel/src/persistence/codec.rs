//! Byte layer over the save schema (`crate::persistence::schema`).
//!
//! The kernel owns the versioned schema (`SaveGameV1` with its header); this file only
//! turns it into bytes and back. Layout: magic, codec version, then every field in
//! declaration order (little-endian integers, length-prefixed vectors and strings).
//! The rules module writes its own sections ([`ModuleSections`]) at two fixed
//! places: last in the economy section and after the player kinds.
//! Untrusted input never panics: every read is bounded and returns `CodecError`.
//!
//! A module reads a range of its codec versions (`ModuleSections::OLDEST_CODEC_VERSION`
//! up to the current): a field added at version `n` reads as its default from
//! older files (`Reader::since`), so a save survives the build that extends
//! the layout. Writing is always the current version (platform/docs/saves.md).

use crate::module::RulesModule;
use crate::persistence::schema::{SaveGameV1, SaveHeader, SavedCity, SavedEffect, SavedUnit, SavedWorldV1};
use crate::rules::fingerprint::RulesDigest;
use crate::world::module::{ModuleUnit, ModuleUnitOf, SavedModule};

mod ai;
mod economy;

const MAGIC: &[u8; 8] = b"AURELIA\0";
/// The codec version is the module's (`ModuleSections::CODEC_VERSION`): it
/// counts changes of the kernel's layout and the module's together. Classic:
/// 4: the AI memory section after the world (`codec::ai`);
/// 5: culture pools and social policies appended to the world;
/// 6: faith and religion appended;
/// 7: great works, influence and archaeology appended;
/// 8: M4 track C appended (combat, diplomacy, barbarians, city-states,
/// victory); 9: the World Congress; 10: espionage; 11: the M4-A economy
/// section and resource deposit amounts; 12: trade routes; 13: natural
/// wonders found (economy section); 14: owed free technologies and great
/// people (economy section); 15: World Congress projects and their rewards;
/// 16: a city's capital connection and its paid first-conversion bonus;
/// 17: the Patronage finisher's countdown per city-state friend; 18: the
/// round a city was acquired; 19: pillaged improvements.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    NotASave,
    UnsupportedCodec(u32),
    Truncated,
    Invalid(&'static str),
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotASave => write!(f, "not a save file"),
            Self::UnsupportedCodec(v) => write!(f, "unsupported save format {v}"),
            Self::Truncated => write!(f, "save file is truncated"),
            Self::Invalid(what) => write!(f, "save file is damaged ({what})"),
        }
    }
}

impl std::error::Error for CodecError {}

/// The byte layout of a rules module's save sections.
pub trait ModuleSections: Sized {
    /// The codec version of a save of this module: the layout of the
    /// kernel's sections and the module's together (a change of either
    /// bumps it).
    const CODEC_VERSION: u32;

    /// The oldest codec version the module still reads: its sections and
    /// records read the older layouts by `Reader::version` (a module with one
    /// layout: the current only).
    const OLDEST_CODEC_VERSION: u32 = Self::CODEC_VERSION;

    /// The first codec version whose kernel layout holds the knowledge of
    /// resources and city sightings (`SavedWorldV1::knowledge_resource`,
    /// `knowledge_cities`); an older save reads both empty. A module with one
    /// layout writes them from its first version.
    const SIGHTINGS_SINCE: u32 = Self::CODEC_VERSION;

    /// The first codec version whose economy section holds the game's speed
    /// (`SavedEconomy::speed`); an older save reads Standard. A module that
    /// has no game speeds keeps the default: it never writes one.
    const SPEED_SINCE: u32 = u32::MAX;

    /// What the module writes last in the economy section (read back before
    /// the rest of its sections).
    type EconomyPart;

    fn put_economy_part(&self, out: &mut Vec<u8>);

    /// The module's sections after the player kinds, last in the world.
    fn put_sections(&self, out: &mut Vec<u8>);

    fn take_economy_part(r: &mut Reader<'_>) -> Result<Self::EconomyPart, CodecError>;

    fn take_sections(economy_part: Self::EconomyPart, r: &mut Reader<'_>) -> Result<Self, CodecError>;

    /// The digest of the rules the save was written with, when the module
    /// keeps one (None: it keeps none, or the save predates it). A load on
    /// other rules names the parts that changed from it (`decode::RulesDrift`).
    fn rules_digest(&self) -> Option<&RulesDigest> {
        None
    }

    /// Keep the digest of the rules the save is written with (called by
    /// `encode`; a module that keeps none never computes it).
    fn keep_rules_digest(&mut self, _digest: impl FnOnce() -> RulesDigest) {}
}

/// A module without save sections. Version 2 added the kernel's deposits and city sightings.
impl ModuleSections for () {
    const CODEC_VERSION: u32 = 2;
    const OLDEST_CODEC_VERSION: u32 = 1;
    const SIGHTINGS_SINCE: u32 = 2;
    type EconomyPart = ();
    fn put_economy_part(&self, _: &mut Vec<u8>) {}
    fn put_sections(&self, _: &mut Vec<u8>) {}
    fn take_economy_part(_: &mut Reader<'_>) -> Result<(), CodecError> {
        Ok(())
    }
    fn take_sections(_: (), _: &mut Reader<'_>) -> Result<(), CodecError> {
        Ok(())
    }
}

pub fn to_bytes<M: RulesModule>(save: &SaveGameV1<M>) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    <SavedModule<M> as ModuleSections>::CODEC_VERSION.put(&mut out);
    put_header(&save.header, &mut out);
    put_world(&save.world, &mut out);
    save.ai.put(&mut out);
    out
}

pub fn from_bytes<M: RulesModule>(bytes: &[u8]) -> Result<SaveGameV1<M>, CodecError> {
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err(CodecError::NotASave);
    }
    let mut reader = Reader {
        bytes,
        at: MAGIC.len(),
        version: 0,
    };
    reader.version = u32::take(&mut reader)?;
    let readable = SavedModule::<M>::OLDEST_CODEC_VERSION..=SavedModule::<M>::CODEC_VERSION;
    if !readable.contains(&reader.version) {
        return Err(CodecError::UnsupportedCodec(reader.version));
    }
    let header = take_header(&mut reader)?;
    let world = take_world(&mut reader)?;
    let ai = Field::take(&mut reader)?;
    if reader.at != bytes.len() {
        return Err(CodecError::Invalid("trailing bytes"));
    }
    Ok(SaveGameV1 { header, world, ai })
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// The codec version of the file being read.
    version: u32,
}

impl<'a> Reader<'a> {
    /// A reader at the start of `bytes`, a layout of `version` (a command
    /// journal reads its records with it).
    pub fn new(bytes: &'a [u8], version: u32) -> Self {
        Self { bytes, at: 0, version }
    }

    /// Every byte was read.
    pub fn is_at_end(&self) -> bool {
        self.remaining() == 0
    }

    /// The codec version of the file being read (`ModuleSections::CODEC_VERSION`
    /// of the build that wrote it).
    pub fn version(&self) -> u32 {
        self.version
    }

    /// A field the layout gained at codec version `since`: read from files of
    /// that version and later, the default from older ones.
    pub fn since<T: Field + Default>(&mut self, since: u32) -> Result<T, CodecError> {
        if self.version >= since {
            T::take(self)
        } else {
            Ok(T::default())
        }
    }

    fn take(&mut self, n: usize) -> Result<&[u8], CodecError> {
        let end = self.at.checked_add(n).ok_or(CodecError::Truncated)?;
        let slice = self.bytes.get(self.at..end).ok_or(CodecError::Truncated)?;
        self.at = end;
        Ok(slice)
    }

    /// How many bytes were read.
    pub fn position(&self) -> usize {
        self.at
    }

    /// How many bytes are left.
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
}

/// One value of the schema: written and read symmetrically.
pub trait Field: Sized {
    fn put(&self, out: &mut Vec<u8>);
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError>;
}

macro_rules! integer_field {
    ($($t:ty),*) => {$(
        impl Field for $t {
            fn put(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
            fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
                let bytes = r.take(std::mem::size_of::<$t>())?;
                Ok(<$t>::from_le_bytes(bytes.try_into().expect("exact size")))
            }
        }
    )*};
}
integer_field!(u8, u16, u32, u64, i32, i64);

/// Nothing: a module without a record writes no bytes.
impl Field for () {
    fn put(&self, _: &mut Vec<u8>) {}
    fn take(_: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(())
    }
}

impl Field for bool {
    fn put(&self, out: &mut Vec<u8>) {
        u8::from(*self).put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        match u8::take(r)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CodecError::Invalid("bool")),
        }
    }
}

impl<T: Field> Field for Option<T> {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            None => 0u8.put(out),
            Some(value) => {
                1u8.put(out);
                value.put(out);
            }
        }
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        match u8::take(r)? {
            0 => Ok(None),
            1 => Ok(Some(T::take(r)?)),
            _ => Err(CodecError::Invalid("option tag")),
        }
    }
}

impl<T: Field> Field for Vec<T> {
    fn put(&self, out: &mut Vec<u8>) {
        (self.len() as u32).put(out);
        for item in self {
            item.put(out);
        }
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        let len = u32::take(r)? as usize;
        // Every element takes at least one byte: a larger count is damage, not data.
        if len > r.remaining() {
            return Err(CodecError::Truncated);
        }
        let mut items = Vec::with_capacity(len);
        for _ in 0..len {
            items.push(T::take(r)?);
        }
        Ok(items)
    }
}

impl Field for String {
    fn put(&self, out: &mut Vec<u8>) {
        self.as_bytes().to_vec().put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        String::from_utf8(Vec::<u8>::take(r)?).map_err(|_| CodecError::Invalid("text"))
    }
}

impl Field for RulesDigest {
    fn put(&self, out: &mut Vec<u8>) {
        self.0.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(RulesDigest(Field::take(r)?))
    }
}

impl<A: Field, B: Field> Field for (A, B) {
    fn put(&self, out: &mut Vec<u8>) {
        self.0.put(out);
        self.1.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok((A::take(r)?, B::take(r)?))
    }
}

impl<A: Field, B: Field, C: Field, D: Field, E: Field> Field for (A, B, C, D, E) {
    fn put(&self, out: &mut Vec<u8>) {
        self.0.put(out);
        self.1.put(out);
        self.2.put(out);
        self.3.put(out);
        self.4.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok((A::take(r)?, B::take(r)?, C::take(r)?, D::take(r)?, E::take(r)?))
    }
}

impl<A: Field, B: Field, C: Field> Field for (A, B, C) {
    fn put(&self, out: &mut Vec<u8>) {
        self.0.put(out);
        self.1.put(out);
        self.2.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok((Field::take(r)?, Field::take(r)?, Field::take(r)?))
    }
}

impl<A: Field, B: Field, C: Field, D: Field> Field for (A, B, C, D) {
    fn put(&self, out: &mut Vec<u8>) {
        self.0.put(out);
        self.1.put(out);
        self.2.put(out);
        self.3.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok((A::take(r)?, B::take(r)?, C::take(r)?, D::take(r)?))
    }
}

fn put_header(h: &SaveHeader, out: &mut Vec<u8>) {
    h.save_schema_version.put(out);
    h.simulation_version.put(out);
    h.ruleset_hash.put(out);
}

fn take_header(r: &mut Reader<'_>) -> Result<SaveHeader, CodecError> {
    Ok(SaveHeader {
        save_schema_version: Field::take(r)?,
        simulation_version: Field::take(r)?,
        ruleset_hash: Field::take(r)?,
    })
}

impl<M: RulesModule> Field for SavedUnit<M> {
    fn put(&self, out: &mut Vec<u8>) {
        self.id.put(out);
        self.tile.put(out);
        self.owner.put(out);
        self.kind.put(out);
        self.hp.put(out);
        self.moves_left.put(out);
        self.moves_max.put(out);
        self.stance.put(out);
        self.work.put(out);
        self.promotions.put(out);
        self.goal.put(out);
        self.experience.put(out);
        self.level.put(out);
        self.attacks_made.put(out);
        self.interceptions_made.put(out);
        self.module.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            id: Field::take(r)?,
            tile: Field::take(r)?,
            owner: Field::take(r)?,
            kind: Field::take(r)?,
            hp: Field::take(r)?,
            moves_left: Field::take(r)?,
            moves_max: Field::take(r)?,
            stance: Field::take(r)?,
            work: Field::take(r)?,
            promotions: Field::take(r)?,
            goal: Field::take(r)?,
            experience: Field::take(r)?,
            level: Field::take(r)?,
            attacks_made: Field::take(r)?,
            interceptions_made: Field::take(r)?,
            module: r.since(<ModuleUnitOf<M> as ModuleUnit<M>>::SAVED_SINCE)?,
        })
    }
}

impl Field for SavedEffect {
    fn put(&self, out: &mut Vec<u8>) {
        self.owner.put(out);
        self.target_tag.put(out);
        self.target.put(out);
        self.source_tag.put(out);
        self.source_id.put(out);
        self.trigger.put(out);
        self.slot.put(out);
        self.turns_left.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            owner: Field::take(r)?,
            target_tag: Field::take(r)?,
            target: Field::take(r)?,
            source_tag: Field::take(r)?,
            source_id: Field::take(r)?,
            trigger: Field::take(r)?,
            slot: Field::take(r)?,
            turns_left: Field::take(r)?,
        })
    }
}

impl Field for SavedCity {
    fn put(&self, out: &mut Vec<u8>) {
        self.id.put(out);
        self.tile.put(out);
        self.owner.put(out);
        self.name.put(out);
        self.population.put(out);
        self.food.put(out);
        self.buildings.put(out);
        self.production.put(out);
        self.queue.put(out);
        self.unworked.put(out);
        self.culture.put(out);
        self.acquired.put(out);
        self.radius.put(out);
        self.damage.put(out);
        self.original_owner.put(out);
        self.original_capital.put(out);
        self.status.put(out);
        self.resistance_turns.put(out);
        self.strike_made.put(out);
        self.connected_to_capital.put(out);
        self.acquired_round.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            id: Field::take(r)?,
            tile: Field::take(r)?,
            owner: Field::take(r)?,
            name: Field::take(r)?,
            population: Field::take(r)?,
            food: Field::take(r)?,
            buildings: Field::take(r)?,
            production: Field::take(r)?,
            queue: Field::take(r)?,
            unworked: Field::take(r)?,
            culture: Field::take(r)?,
            acquired: Field::take(r)?,
            radius: Field::take(r)?,
            damage: Field::take(r)?,
            original_owner: Field::take(r)?,
            original_capital: Field::take(r)?,
            status: Field::take(r)?,
            resistance_turns: Field::take(r)?,
            strike_made: Field::take(r)?,
            connected_to_capital: Field::take(r)?,
            acquired_round: Field::take(r)?,
        })
    }
}

fn put_world<M: RulesModule>(w: &SavedWorldV1<M>, out: &mut Vec<u8>) {
    put_map(w, out);
    w.units.put(out);
    w.cities.put(out);
    w.diplomacy.put(out);
    w.diplomacy_open.put(out);
    w.turn_round.put(out);
    w.turn_phase.put(out);
    w.active_player.put(out);
    put_players(w, out);
    w.knowledge_explored.put(out);
    w.knowledge_memory.put(out);
    w.knowledge_contacts.put(out);
    w.knowledge_resource.put(out);
    w.knowledge_cities.put(out);
    w.rng_combat.put(out);
    w.rng_events.put(out);
    w.active_effects.put(out);
    w.next_entity.put(out);
    w.economy.put(out);
    // The module's part closes the economy block, as before the platform
    // split (Classic: its trade routes).
    w.module.put_economy_part(out);
    w.player_kinds.put(out);
    w.module.put_sections(out);
}

fn put_map<M: RulesModule>(w: &SavedWorldV1<M>, out: &mut Vec<u8>) {
    w.width.put(out);
    w.height.put(out);
    w.players.put(out);
    w.terrain.put(out);
    w.feature.put(out);
    w.resource.put(out);
    w.resource_amount.put(out);
    w.owner.put(out);
    w.tile_city.put(out);
    w.improvement.put(out);
    w.pillaged.put(out);
    w.road.put(out);
    w.natural_wonder.put(out);
    w.river.put(out);
    w.river_edges.put(out);
}

fn put_players<M: RulesModule>(w: &SavedWorldV1<M>, out: &mut Vec<u8>) {
    w.players_alive.put(out);
    w.player_techs.put(out);
    w.player_gold.put(out);
    w.player_science.put(out);
    w.player_research.put(out);
    w.player_nations.put(out);
}

/// Reads fields in exactly the order `put_world` writes them.
fn take_world<M: RulesModule>(r: &mut Reader<'_>) -> Result<SavedWorldV1<M>, CodecError> {
    let kernel = take_kernel(r)?;
    let economy = Field::take(r)?;
    let economy_part = SavedModule::<M>::take_economy_part(r)?;
    let player_kinds = Field::take(r)?;
    let module = SavedModule::<M>::take_sections(economy_part, r)?;
    Ok(SavedWorldV1 {
        economy,
        player_kinds,
        module,
        ..kernel
    })
}

/// The kernel fields before the economy section (the rest stays empty).
fn take_kernel<M: RulesModule>(r: &mut Reader<'_>) -> Result<SavedWorldV1<M>, CodecError> {
    Ok(SavedWorldV1 {
        width: Field::take(r)?,
        height: Field::take(r)?,
        players: Field::take(r)?,
        terrain: Field::take(r)?,
        feature: Field::take(r)?,
        resource: Field::take(r)?,
        resource_amount: Field::take(r)?,
        owner: Field::take(r)?,
        tile_city: Field::take(r)?,
        improvement: Field::take(r)?,
        pillaged: Field::take(r)?,
        road: Field::take(r)?,
        natural_wonder: Field::take(r)?,
        river: Field::take(r)?,
        river_edges: Field::take(r)?,
        units: Field::take(r)?,
        cities: Field::take(r)?,
        diplomacy: Field::take(r)?,
        diplomacy_open: Field::take(r)?,
        turn_round: Field::take(r)?,
        turn_phase: Field::take(r)?,
        active_player: Field::take(r)?,
        players_alive: Field::take(r)?,
        player_techs: Field::take(r)?,
        player_gold: Field::take(r)?,
        player_science: Field::take(r)?,
        player_research: Field::take(r)?,
        player_nations: Field::take(r)?,
        knowledge_explored: Field::take(r)?,
        knowledge_memory: Field::take(r)?,
        knowledge_contacts: Field::take(r)?,
        knowledge_resource: r.since(SavedModule::<M>::SIGHTINGS_SINCE)?,
        knowledge_cities: r.since(SavedModule::<M>::SIGHTINGS_SINCE)?,
        rng_combat: Field::take(r)?,
        rng_events: Field::take(r)?,
        active_effects: Field::take(r)?,
        next_entity: Field::take(r)?,
        // Read after the kernel fields by `take_world`.
        economy: Default::default(),
        player_kinds: Vec::new(),
        module: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_module::{Marks, TestModule};

    fn unit(module: (u16, u16)) -> SavedUnit<TestModule> {
        SavedUnit {
            id: 3,
            tile: 13,
            owner: 1,
            kind: 0,
            hp: 70,
            moves_left: 60,
            moves_max: 120,
            stance: (0, 0),
            work: None,
            promotions: vec![2],
            goal: Some(14),
            experience: 5,
            level: 1,
            attacks_made: 0,
            interceptions_made: 0,
            module,
        }
    }

    #[test]
    fn a_unit_record_ends_with_the_module_state_from_its_first_version() {
        let since = <Marks as ModuleUnit<TestModule>>::SAVED_SINCE;
        let saved = unit((7, 2));
        let mut bytes = Vec::new();
        saved.put(&mut bytes);
        let read = SavedUnit::<TestModule>::take(&mut Reader { bytes: &bytes, at: 0, version: since }).unwrap();
        assert_eq!(read, saved);

        let older = &bytes[..bytes.len() - 4];
        let mut reader = Reader { bytes: older, at: 0, version: since - 1 };
        let read = SavedUnit::<TestModule>::take(&mut reader).unwrap();
        assert_eq!(read, unit((0, 0)), "an older record: the default state");
        assert_eq!(reader.remaining(), 0, "and no byte of it is the module's");
    }
}
