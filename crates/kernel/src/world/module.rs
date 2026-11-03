//! The world state of the game's rules module (platform/PLATFORM.md, stage 2).
//!
//! The kernel's world is the map, the players, cities and units, war and
//! open borders, the turn, knowledge, timed effects, the RNG and the game
//! settings. Everything a rules module adds on top (Classic: trade routes,
//! religion, city-states, the World Congress, ...) is one value of the
//! module's type that `AuthWorld` holds without knowing its parts. The
//! kernel reaches it only through [`ModuleWorld`]: create it for a new game,
//! mix it into the world hash, validate it, save it and load it back. The
//! module's own systems reach their state directly, through
//! `AuthWorld::module()` and, inside a transaction, the `module` field.
//! A module's state of one player, one city or one unit lives beside the
//! kernel's record of it ([`ModulePlayer`], [`ModuleCity`], [`ModuleUnit`]),
//! so it is born, saved and dropped with that record.
//!
//! One module per game: `RulesModule::World` is its world state.

use crate::ids::{CityHandle, EntityId, PlayerId};
use crate::module::RulesModule;
use crate::persistence::decode::DecodeError;
use crate::rules::CompiledRuleset;
use crate::validation::world::ValidationError;
use crate::world::state::AuthWorld;

/// The save sections of a module's world state (`SavedWorldV1::module`).
pub type SavedModule<M> = <<M as RulesModule>::World as ModuleWorld<M>>::Saved;

/// The module's state of one player (`PlayerCore::economy`).
pub type ModulePlayerOf<M> = <<M as RulesModule>::World as ModuleWorld<M>>::Player;

/// A player's record of the economy section in a save (`ModulePlayer::Saved`).
pub type SavedPlayerOf<M> = <ModulePlayerOf<M> as ModulePlayer<M>>::Saved;

/// The module's state of one city (`CityStore::module_of`).
pub type ModuleCityOf<M> = <<M as RulesModule>::World as ModuleWorld<M>>::City;

/// The module's part of a city's record in the economy section.
pub type SavedCityOf<M> = <ModuleCityOf<M> as ModuleCity<M>>::Saved;

/// The module's state of one unit (`UnitStore::module_of`).
pub type ModuleUnitOf<M> = <<M as RulesModule>::World as ModuleWorld<M>>::Unit;

/// The module's part of a unit's save record (`SavedUnit::module`).
pub type SavedUnitOf<M> = <ModuleUnitOf<M> as ModuleUnit<M>>::Saved;

/// What other players see of a unit's module state (`PlayerView::unit_state_seen`).
pub type SeenUnitOf<M> = <ModuleUnitOf<M> as ModuleUnit<M>>::Seen;

/// A change of a module's world state (`ChangeJournal::module_changes`).
pub type ModuleChangeOf<M> = <<M as RulesModule>::World as ModuleWorld<M>>::Change;

/// Name and version of a rules module. Both are part of the ruleset
/// fingerprint, so a save or a replay never loads under another module or
/// another version of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId {
    pub name: &'static str,
    pub version: u32,
}

/// What a new game's module state is made from.
#[derive(Debug, Clone, Copy)]
pub struct NewModuleWorld<'a, M: RulesModule> {
    pub players: usize,
    /// The game's RNG seed: a module's own random streams start from it.
    pub seed: u64,
    pub rules: &'a CompiledRuleset<M>,
}

/// Where in the authoritative hash the module mixes its state.
///
/// The hash is one fixed sequence, pinned by the game hash tests. The
/// module's sections stay where they were before the split, so the pinned
/// hashes hold; a deliberate re-pin may merge the points into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashPoint {
    /// Inside the record of one player, after its kind.
    Player(PlayerId),
    /// After the war and open-border matrices.
    Diplomacy,
    /// Inside the economy section's fold, last.
    Economy,
    /// After the economy section's fold.
    AfterEconomy,
    /// After the combat and event RNG streams.
    AfterRng,
}

/// The derived city yields a module change makes stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleYields {
    Nothing,
    City(CityHandle),
    /// Every city of the player.
    Player(PlayerId),
}

/// One change of module state in the journal: the module's systems write
/// it, derived state, knowledge and the client's presentation read it.
pub trait ModuleChange: Clone + std::fmt::Debug {
    /// What the kernel's derived yields must recompute.
    fn stale_yields(&self) -> StaleYields;

    /// The player whose unit sources (policies, golden ages, timed
    /// effects) this change may have altered: their units' sight is
    /// computed again (`derived::visibility`). Default: none.
    fn changes_unit_sources(&self) -> Option<crate::ids::PlayerId> {
        None
    }
}

/// The kernel's fields of a player's record in the economy section: the
/// module's record holds them where they always were.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KernelPlayerRecord {
    /// Tiles the player bought (`PlayerCore::plots_bought`).
    pub plots_bought: u16,
    /// The AI plays the player (`GameSettings::ai`).
    pub ai: bool,
}

impl KernelPlayerRecord {
    /// Mix the kernel's fields (a module without player state saves and
    /// hashes this record as it is).
    pub fn hash(&self, mix: &mut dyn FnMut(u64)) {
        mix(u64::from(self.plots_bought));
        mix(u64::from(self.ai));
    }
}

impl crate::persistence::codec::Field for KernelPlayerRecord {
    fn put(&self, out: &mut Vec<u8>) {
        self.plots_bought.put(out);
        self.ai.put(out);
    }
    fn take(r: &mut crate::persistence::codec::Reader<'_>) -> Result<Self, crate::persistence::codec::CodecError> {
        use crate::persistence::codec::Field;
        Ok(Self { plots_bought: Field::take(r)?, ai: Field::take(r)? })
    }
}

/// The rules module's state of one player (`PlayerCore::economy`). It is
/// saved and hashed as the player's record of the economy section, with
/// the kernel's fields of that record ([`KernelPlayerRecord`]) at their
/// places, so saves and hashes keep their bytes.
pub trait ModulePlayer<M: RulesModule>: Clone + std::fmt::Debug + Default + PartialEq + Eq {
    /// The player's record of the economy section.
    type Saved: Clone + std::fmt::Debug + Default + PartialEq + Eq + crate::persistence::codec::Field;

    fn encode(&self, kernel: KernelPlayerRecord) -> Self::Saved;

    /// The kernel's fields of a saved record.
    fn kernel_part(saved: &Self::Saved) -> KernelPlayerRecord;

    /// The module's state of an untrusted record (`players`: the player
    /// count; `auth`: the loaded kernel world). Errors, never panics.
    fn decode(saved: &Self::Saved, players: usize, auth: &AuthWorld<M>) -> Result<Self, String>;

    /// Mix the record into the economy section's hash fold, its kernel
    /// fields included.
    fn hash(saved: &Self::Saved, mix: &mut dyn FnMut(u64));
}

/// The rules module's state of one city (Classic: great person points),
/// beside the kernel's (`CityStore::module_of`). A new city starts with
/// the default. It is saved and hashed in the city's record of the economy
/// section, after the food kept, where it always was.
pub trait ModuleCity<M: RulesModule>: Clone + std::fmt::Debug + Default + PartialEq + Eq {
    /// The module's part of the city's record.
    type Saved: Clone + std::fmt::Debug + Default + PartialEq + Eq + crate::persistence::codec::Field;

    fn encode(&self) -> Self::Saved;

    /// The state of an untrusted record. Errors (what is wrong), never panics.
    fn decode(saved: &Self::Saved, rules: &CompiledRuleset<M>) -> Result<Self, &'static str>;

    /// Mix the record into the economy section's hash fold.
    fn hash(saved: &Self::Saved, mix: &mut dyn FnMut(u64));
}

/// A module without city state.
impl<M: RulesModule> ModuleCity<M> for () {
    type Saved = ();
    fn encode(&self) {}
    fn decode(_: &(), _: &CompiledRuleset<M>) -> Result<(), &'static str> {
        Ok(())
    }
    fn hash(_: &(), _: &mut dyn FnMut(u64)) {}
}

/// The rules module's state of one unit (Lands & Laws: experience,
/// abilities, healing), beside the kernel's (`UnitStore::module_of`).
///
/// It lives exactly as long as the unit: a new unit starts with the
/// default, moving and fighting keep it, a removed unit's state goes with
/// it. An upgrade or a capture makes a new unit (`destroy_unit` and
/// `spawn_unit`): a module that keeps the state across them copies it
/// itself. The module's systems change it through `UnitStore::module_mut`.
///
/// It is saved last in the unit's record, from codec version
/// [`Self::SAVED_SINCE`] on, and hashed with the unit. Knowledge: the
/// owner sees all of it; another player sees [`Self::Seen`] while the unit
/// stands on a tile it sees, and remembers nothing once it is out of sight
/// (`knowledge::view::PlayerView::unit_state_seen`). A module without unit
/// state binds `()`: no bytes, no hash, nothing seen.
pub trait ModuleUnit<M: RulesModule>: Clone + std::fmt::Debug + Default + PartialEq + Eq {
    /// The module's part of the unit's save record.
    type Saved: Clone + std::fmt::Debug + Default + PartialEq + Eq + crate::persistence::codec::Field;

    /// What another player sees of the unit while the unit is in its sight.
    type Seen: Clone + std::fmt::Debug + PartialEq + Eq;

    /// The first codec version (`ModuleSections::CODEC_VERSION`) whose unit
    /// records carry the module's part. Older saves give every unit the
    /// default state (platform/docs/saves.md, "Format versions").
    const SAVED_SINCE: u32;

    fn encode(&self) -> Self::Saved;

    /// The state of an untrusted record. Errors (what is wrong), never panics.
    fn decode(saved: &Self::Saved, rules: &CompiledRuleset<M>) -> Result<Self, &'static str>;

    /// Mix the state into the unit's hash. The default state should mix
    /// nothing, so a game whose units never use it keeps its hash.
    fn hash(&self, mix: &mut dyn FnMut(u64));

    /// What other players see of it.
    fn seen(&self) -> Self::Seen;
}

/// A module without unit state.
impl<M: RulesModule> ModuleUnit<M> for () {
    type Saved = ();
    type Seen = ();
    const SAVED_SINCE: u32 = 0;
    fn encode(&self) {}
    fn decode(_: &(), _: &CompiledRuleset<M>) -> Result<(), &'static str> {
        Ok(())
    }
    fn hash(&self, _: &mut dyn FnMut(u64)) {}
    fn seen(&self) {}
}

/// The contract between the kernel and the module's world state.
pub trait ModuleWorld<M: RulesModule>: Clone + std::fmt::Debug + Sized {
    const ID: ModuleId;

    /// The module's save sections; the byte codec writes them at the
    /// module's places (`persistence::codec::ModuleSections`).
    type Saved: Clone + std::fmt::Debug + Default + PartialEq + Eq + crate::persistence::codec::ModuleSections;

    type Change: ModuleChange;

    /// The module's state of each player (`PlayerCore::economy`).
    type Player: ModulePlayer<M>;

    /// The module's state of each city (a module without: `()`).
    type City: ModuleCity<M>;

    /// The module's state of each unit (a module without: `()`).
    type Unit: ModuleUnit<M>;

    /// The kinds of player the module adds (`PlayerKind::Module`; a module
    /// without: `world::player::NoKind`).
    type PlayerKind: crate::world::player::ModulePlayerKind<M>;

    fn new(game: NewModuleWorld<'_, M>) -> Self;

    /// Mix the state belonging to `point` (see [`HashPoint`]). `auth` is the
    /// whole world: sections refer to cities and units by their save order.
    fn hash(&self, point: HashPoint, auth: &AuthWorld<M>, mix: &mut dyn FnMut(u64));

    /// The module's invariants against the kernel world (debug and tests).
    fn validate(&self, auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> Result<(), ValidationError>;

    /// A unit left the world (killed, consumed, disbanded): the module drops
    /// what it kept of it outside the unit's own state (`Self::Unit` goes
    /// with the unit).
    fn unit_removed(&mut self, unit: EntityId);

    fn encode(&self, auth: &AuthWorld<M>) -> Self::Saved;

    /// The module state of an untrusted save. `auth` is the loaded kernel
    /// world (its module state is a placeholder): cities and units in save
    /// order, the players, the map. Errors, never panics.
    fn decode(
        saved: &Self::Saved,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
    ) -> Result<Self, DecodeError>;
}
