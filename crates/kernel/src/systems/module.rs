//! The rules module's part of the kernel's systems (platform/PLATFORM.md, stage 5b).
//!
//! The kernel's systems (units, cities, research, the turn) run the generic
//! 4X mechanism; a rules module joins them at fixed points:
//!
//! - its steps of the turn ([`ModuleSystems::run_step`],
//!   [`ModuleSystems::run_city_step`], [`ModuleSystems::end_round`]), in the
//!   order of its `RulesModule::SCHEDULE`;
//! - its reactions to the kernel's events ([`KernelEvent`]: Classic fires its
//!   triggered effects there, places great works, settles finds);
//! - the numbers of its formulas (healing, growth, border cost, gold and
//!   science of the empire, processes, what a queue may hold);
//! - its rules of moves and commands (whose land a unit enters, the price
//!   of a step, where a city may be founded). Each default is the kernel's
//!   own (Classic) rule.
//!
//! The kernel calls them with its own transaction; the module reaches its
//! own systems from it.

use crate::command::CommandError;
use crate::effects::city::CityModifiers;
use crate::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TechId, TileId, UnitHandle, UnitKindId};
use crate::module::RulesModule;
use crate::navigation::cost::StepCost;
use crate::navigation::MoveProfile;
use crate::rules::content::ProjectId;
use crate::rules::CompiledRuleset;
use crate::systems::turn::{CityStepInput, CityTurn, EndTurnInput};
use crate::transaction::WorldTransaction;
use crate::world::city::CityCore;
use crate::world::city_economy::Process;
use crate::world::state::AuthWorld;
use crate::world::unit::UnitCore;

/// Something a kernel system did that the rules module may react to, at the
/// point it happened (inside the same transaction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelEvent {
    /// A unit appeared by any way (trained, granted, spawned).
    UnitSpawned { player: PlayerId, unit: UnitHandle },
    /// A unit stepped onto a tile (a move, a march, a move order).
    UnitEntered { player: PlayerId, unit: UnitHandle, tile: TileId },
    /// A city trained `unit` (after `UnitSpawned`).
    UnitTrained { city: CityHandle, unit: UnitHandle, kind: UnitKindId },
    /// A city's owner bought `unit` with gold (after `UnitSpawned`).
    UnitPurchased { city: CityHandle, unit: UnitHandle, kind: UnitKindId },
    /// A city completed a building (by production or as the harness's).
    BuildingCompleted { city: CityHandle, building: BuildingKindId },
    /// A city completed a project of the module's tables (it has left the
    /// head of the city's queue).
    ProjectCompleted { player: PlayerId, city: CityHandle, project: ProjectId },
    /// A city was founded: its ring, and the Palace of a first city, stand.
    CityFounded { player: PlayerId, city: CityHandle },
    /// The player learned a technology (`new_era`: of a later era than
    /// any it knew).
    TechnologyLearned { player: PlayerId, tech: TechId, new_era: bool },
    /// The start phase of the player's end turn (`TurnStartEffects`).
    TurnStarted { player: PlayerId },
    /// The science of the player's turn went into its research stock.
    ScienceCollected { player: PlayerId, science: u32 },
}

/// The food of one city's turn, by the module's formulas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CityGrowth {
    /// What the store gains (or loses), in hundredths.
    pub difference_times100: i64,
    /// Share of the threshold kept on growth, percent.
    pub kept_percent: u32,
    /// The store that grows the city by one citizen.
    pub threshold: u32,
}

/// What the rules module adds to the kernel's systems. The defaults are a
/// module without reactions to events, gold, science, processes,
/// purchases or players that live without cities: such a module writes
/// only its turn steps, healing, growth, borders and combat facts.
pub trait ModuleSystems<M: RulesModule> {
    // ---- the turn (`systems::turn`) ----

    /// Run one of the module's steps of a player's end turn.
    fn run_step(tx: &mut WorldTransaction<'_, M>, step: M::Step, input: &EndTurnInput<'_, M>);

    /// Run one of the module's steps of a city's resolution.
    fn run_city_step(tx: &mut WorldTransaction<'_, M>, step: M::CityStep, input: &CityStepInput<'_, M>);

    /// After the last player's turn, before the round number grows.
    fn end_round(_tx: &mut WorldTransaction<'_, M>, _rules: &CompiledRuleset<M>) {}

    // ---- events ----

    /// React to `event` inside the transaction.
    fn on_event(_tx: &mut WorldTransaction<'_, M>, _rules: &CompiledRuleset<M>, _event: KernelEvent) {}

    /// A unit is about to leave the world (killed, consumed, disbanded,
    /// replaced by an upgrade): it still stands, so the module can remove
    /// what depends on it (Classic: the cargo of a carrier).
    fn unit_removing(_tx: &mut WorldTransaction<'_, M>, _unit: UnitHandle) {}

    // ---- formulas ----

    /// HP the unit regains at the end of its owner's turn (0: none).
    fn heal_amount(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, unit: &UnitCore) -> i32;

    /// The city's food this turn, from its snapshot.
    fn city_growth(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, city: &CityTurn<M>) -> CityGrowth;

    /// Production a food unit (settler) at the head of the queue gets from
    /// the city's surplus this turn.
    fn food_production(_city: &CityTurn<M>) -> i32 {
        0
    }

    /// Culture the city needs for its next border level.
    fn border_threshold(level: u32, modifiers: &CityModifiers<M>) -> u32;

    /// The city with an empty queue runs `process` with `production`:
    /// the gold and science it makes this turn.
    fn run_process(
        _tx: &mut WorldTransaction<'_, M>,
        _owner: PlayerId,
        _city: CityHandle,
        _process: Process,
        _production: u32,
    ) -> (i32, i32) {
        (0, 0)
    }

    /// An order the empire cannot produce now leaves the queue (Classic: the
    /// resources it needs are gone).
    fn queue_item_blocked(_auth: &AuthWorld<M>, _rules: &CompiledRuleset<M>, _owner: PlayerId, _item: ProductionItem) -> bool {
        false
    }

    /// The empire's gold balance of the turn: `cities_gold` through the
    /// module's income and upkeep.
    fn gold_balance(_auth: &AuthWorld<M>, _rules: &CompiledRuleset<M>, _player: PlayerId, _cities_gold: i32) -> i64 {
        0
    }

    /// The treasury could not pay `balance` (< 0, the treasury is empty
    /// now): the module's consequences.
    fn treasury_deficit(_tx: &mut WorldTransaction<'_, M>, _rules: &CompiledRuleset<M>, _player: PlayerId, _balance: i64) {}

    /// The empire's science of the turn from `cities_science`.
    fn science(_auth: &AuthWorld<M>, _rules: &CompiledRuleset<M>, _player: PlayerId, _cities_science: i32) -> i32 {
        0
    }

    /// Science `player` needs for `tech` now: the content cost after the
    /// player's ModifyCost(Technologies) unless the module prices research
    /// by its own rules (Classic: difficulty, map size, city count, known
    /// by others). At least 1.
    fn research_cost(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, player: PlayerId, tech: TechId) -> u32 {
        let base = rules.technologies.get(tech.0 as usize).map_or(0, |def| def.cost);
        crate::systems::research::technology_cost(auth, rules, player, base)
    }

    // ---- combat facts the kernel shows (`ai::observation`) ----

    /// Attacks the unit has left this turn.
    fn attacks_left(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, unit: &UnitCore) -> u8;

    /// The city's hit points when undamaged.
    fn city_max_hit_points(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, city: &CityCore) -> u16;

    // ---- moves (`world::territory`, `navigation::cost`) ----

    /// May `player`'s units enter the land of `host`, another player? The
    /// one border rule of moves, paths, validation and of what a player
    /// knows (diplomacy is public). By default at war, or when the host lets
    /// the player in (`territory::lets_in`: open land kinds such as
    /// city-states, or an open-borders grant) (Classic).
    fn may_enter(auth: &AuthWorld<M>, player: PlayerId, host: PlayerId) -> bool {
        crate::world::territory::lets_in(auth, host, player) || auth.diplomacy().at_war(player, host)
    }

    /// The price of the step `from → to` for a unit of `player` moving by
    /// `profile`; `kernel` is the kernel's price (None: the unit may not
    /// enter `to`). Asked for the truth (moves, paths) and for a player's
    /// knowledge (its AI's map), so it reads only what the player may know
    /// of the two tiles. A price below `navigation::cost::min_step_points`
    /// makes path search inexact. By default the kernel's price.
    fn step_cost(
        _auth: &AuthWorld<M>,
        _rules: &CompiledRuleset<M>,
        _player: PlayerId,
        _profile: &MoveProfile<M>,
        _from: TileId,
        _to: TileId,
        kernel: Option<StepCost>,
    ) -> Option<StepCost> {
        kernel
    }

    /// Does a player's AI price the steps of a land unit that never embarks
    /// by the unit's whole movement profile (as the units move), or as a
    /// plain land unit (the estimate it has always made)? Ships and units
    /// that embark are priced by their profile either way. Default: plain.
    fn ai_prices_by_profile() -> bool {
        false
    }

    // ---- the module's rules of kernel commands (`command::validate`) ----

    /// A unit of this kind that is not a `founder` may still found cities
    /// (Classic: a Conquistador abroad); `unit_founding_problem` limits it.
    fn founds_cities(_rules: &CompiledRuleset<M>, _kind: UnitKindId) -> bool {
        false
    }

    /// Why this unit may not found a city on `tile`, by the module's rules
    /// of the unit itself; None: it may. Applies to every founding unit.
    fn unit_founding_problem(
        _auth: &AuthWorld<M>,
        _rules: &CompiledRuleset<M>,
        _owner: PlayerId,
        _kind: UnitKindId,
        _tile: TileId,
    ) -> Option<&'static str> {
        None
    }

    /// Why `owner` may not found a city on `tile` now, by the module's
    /// rules (its distance between cities, its terrain flags); None: it
    /// may. The kernel's own site rules (`territory::city_site_check`)
    /// apply after it.
    fn founding_problem(
        _auth: &AuthWorld<M>,
        _rules: &CompiledRuleset<M>,
        _owner: PlayerId,
        _tile: TileId,
    ) -> Option<&'static str> {
        None
    }

    /// Why the unit may not fortify; None: it may.
    fn fortify_problem(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, unit: &UnitCore) -> Option<&'static str>;

    /// The module's rules of an order after the kernel's (technology,
    /// uniqueness, site): what the empire must have or be.
    fn production_problem(
        _auth: &AuthWorld<M>,
        _rules: &CompiledRuleset<M>,
        _player: PlayerId,
        _city: &CityCore,
        _item: ProductionItem,
    ) -> Result<(), CommandError> {
        Ok(())
    }

    /// The unit may be bought with gold.
    fn unit_for_sale(_auth: &AuthWorld<M>, _rules: &CompiledRuleset<M>, _owner: PlayerId, _unit: UnitKindId) -> bool {
        false
    }

    /// The module's rules of buying the head of a city's queue (before its
    /// price).
    fn hurry_problem(_auth: &AuthWorld<M>, _rules: &CompiledRuleset<M>, _player: PlayerId, _city: &CityCore) -> Result<(), CommandError> {
        Err(CommandError::RuleViolation("nothing is bought".into()))
    }

    // ---- life ----

    /// The player lives without cities and units (Classic: the barbarians
    /// while barbarians are on).
    fn stays_alive(_auth: &AuthWorld<M>, _player: PlayerId) -> bool {
        false
    }

    /// The player had a city once by the module's memory (beside a city it
    /// founded that another holds now).
    fn had_city(_auth: &AuthWorld<M>, _player: PlayerId) -> bool {
        false
    }
}

