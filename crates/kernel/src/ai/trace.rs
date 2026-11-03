//! Decision trace and counters (R-07 AI §3, §8): every choice and every
//! refusal carries a short reason code; the trace is diagnostics, never
//! saved and never read by a decision.

use crate::ai::map::SearchStats;
use crate::command::CommandError;
use crate::ids::{EntityId, PlayerId};
use crate::module::RulesModule;

/// Why the AI did something, or why it did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    // Units.
    FoundCity,
    MoveToSite,
    Explore,
    Garrison,
    Fortify,
    Escort,
    MoveToJob,
    BuildImprovement,
    BuildRoad,
    AttackUnit,
    AttackCity,
    Advance,
    Rally,
    Retreat,
    Heal,
    // Cities.
    ProduceSettler,
    ProduceWorker,
    ProduceDefender,
    ProduceArmy,
    ProduceBuilding,
    /// Apollo and the spaceship parts (science victory).
    ProduceProject,
    /// A unit that runs trade routes.
    ProduceTradeUnit,
    Hurry,
    /// A tile bought with gold.
    BuyTile,
    /// Gold given to a city-state.
    GiftGold,
    // Empire.
    Research,
    DeclareWar,
    MakePeace,
    OpenBorders,
    StrategyChanged,
    /// A war operation begins to form (classic/docs/ai-operations.md).
    FormOperation,
    /// A formed operation marches on its target.
    LaunchOperation,
    /// An operation ends: its target taken or lost, or given up.
    EndOperation,
    SystemAdvisor,
    EndTurn,
    // Refusals.
    NoSettleSite,
    ArmyCapReached,
    NoIncome,
    NoPath,
    PathOverBudget,
    NoFrontier,
    NoJob,
    TargetGone,
    Threatened,
    WarCooldown,
    PeaceCooldown,
    TooWeak,
    Idle,
    TurnBudgetExhausted,
    AttemptsExhausted,
    NothingToBuild,
    /// The queue is left empty: the city works on a project of the module
    /// (Classic: a project of the world's league).
    ContributeToProject,
    NoTechAvailable,
    Waiting,
    /// Own or foreign units stand on every way this turn.
    Blocked,
    // Deals between powers (a module with a diplomacy of its own). New
    // reasons go at the end: the order of the enum is the order of traces.
    /// A deal sent to the power is accepted.
    AcceptDeal,
    /// A deal sent to the power is refused.
    RefuseDeal,
    /// A deal the power sends for its own sake.
    ProposeDeal,
    /// A refused deal sent back with the gold that makes it fair.
    CounterDeal,
    /// Gold demanded of a weaker power.
    DemandTribute,
    /// An institution put up in a partner's city.
    EstablishInstitution,
    /// An institution taken down.
    CloseInstitution,
    /// Notice given of a treaty.
    GiveNotice,
    // Air units (a module with aircraft). New reasons go at the end.
    /// An aircraft moves to the city from which the enemy is in reach.
    Rebase,
    /// A fighter attacks the interceptors around a tile.
    AirSweep,
    /// An aircraft takes the interception posture.
    Intercept,
    // Orders the rules refuse. New reasons go at the end.
    /// The planner's choice was an order the rules refuse (asked with the
    /// interface's own check) and no other order took its place: nothing is
    /// sent, the refusal is counted here instead of at the core.
    Forbidden,
    // One-shot weapons, jumps and landings (a module with them). New reasons go at the end.
    /// A missile is fired at its target.
    FireMissile,
    /// A nuclear weapon is launched.
    LaunchNuke,
    /// A paratrooper jumps to a tile near its operation's target.
    Paradrop,
    /// A land unit of an army crosses the water to land on the target's shore.
    Landing,
    /// An aircraft or missile is based on a carrier.
    BaseOnCarrier,
    // Raids (classic/docs/ai-raids.md). New reasons go at the end.
    /// A raid group forms for an enemy's land.
    FormRaid,
    /// A raid ends: its group is gone, its time is up, its damage stops paying, or the war ended.
    EndRaid,
    /// A raider moves toward its prey.
    Raid,
    /// A raider pillages the improvement or road it stands on.
    Pillage,
    /// A raider plunders the trade route whose unit stands on its tile.
    Plunder,
    // The turn's order. New reasons go at the end.
    /// The unit decides again after the rest of the turn's queue: a mind's
    /// request the driver grants `driver::DEFERS_PER_UNIT` times a turn per
    /// unit (a request beyond them ends the unit's turn, as no command does).
    Deferred,
}

/// Who decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actor {
    Player,
    Unit(EntityId),
    City(EntityId),
}

/// What became of a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A choice without a command (stay, wait) or a refusal.
    NoCommand,
    Applied,
    Rejected(CommandError),
}

#[derive(Debug, Clone)]
pub struct TraceEntry<M: RulesModule> {
    pub round: u32,
    pub player: PlayerId,
    pub actor: Actor,
    pub reason: Reason,
    pub command: Option<M::Command>,
    pub outcome: Outcome,
}

impl<M: RulesModule> std::fmt::Display for TraceEntry<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "r{} P{} {:?} {:?}",
            self.round, self.player.0, self.actor, self.reason
        )?;
        if let Some(command) = &self.command {
            write!(f, " {command:?}")?;
        }
        match &self.outcome {
            Outcome::NoCommand => Ok(()),
            Outcome::Applied => write!(f, " ok"),
            Outcome::Rejected(error) => write!(f, " rejected: {error:?}"),
        }
    }
}

/// Bounded trace: the newest `capacity` entries, plus totals per reason
/// over the whole game.
#[derive(Debug, Clone)]
pub struct DecisionTrace<M: RulesModule> {
    entries: std::collections::VecDeque<TraceEntry<M>>,
    capacity: usize,
    totals: std::collections::BTreeMap<Reason, u64>,
}

impl<M: RulesModule> Default for DecisionTrace<M> {
    fn default() -> Self {
        Self::with_capacity(4096)
    }
}

impl<M: RulesModule> DecisionTrace<M> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: std::collections::VecDeque::with_capacity(capacity.min(4096)),
            capacity,
            totals: std::collections::BTreeMap::new(),
        }
    }

    pub fn push(&mut self, entry: TraceEntry<M>) {
        *self.totals.entry(entry.reason).or_default() += 1;
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn entries(&self) -> impl Iterator<Item = &TraceEntry<M>> {
        self.entries.iter()
    }

    pub fn total(&self, reason: Reason) -> u64 {
        self.totals.get(&reason).copied().unwrap_or(0)
    }

    pub fn totals(&self) -> &std::collections::BTreeMap<Reason, u64> {
        &self.totals
    }
}

/// Deterministic work counters of the AI (R-07 §8).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AiCounters {
    pub player_turns: u64,
    pub observations: u64,
    pub local_observations: u64,
    pub decisions: u64,
    pub candidates: u64,
    pub commands_applied: u64,
    pub commands_rejected: u64,
    /// Orders the planners chose and the rules' own check refused, so that
    /// another order took their place (`ai::legal`).
    pub orders_retried: u64,
    /// Orders the rules' own check refused and nothing took their place.
    pub orders_withheld: u64,
    /// Production orders for an item the city had queued already: not sent
    /// twice.
    pub orders_redundant: u64,
    /// Orders that were allowed when planned and refused when about to be
    /// sent, because an earlier order of the turn changed the game; not sent.
    pub orders_stale: u64,
    pub tasks_assigned: u64,
    pub tasks_dropped: u64,
    pub search: SearchStats,
}
