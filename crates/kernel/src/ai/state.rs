//! AI memory of one player (R-07 AI §3, PLAN M6 stage B).
//!
//! The memory is decision state: it changes future commands, so it is saved
//! and versioned with the game (`persistence::ai`) and has one writer, the
//! AI turn driver (`ai::driver`). It holds only what the player observed or
//! decided — persistent ids and tiles, never runtime handles and never the
//! truth behind the fog. Every collection is kept sorted, so iteration order
//! (and with it every decision) is deterministic.

use crate::ids::{EntityId, ImprovementId, PlayerId, TileId};

/// What the AI pursues for a while (R-07 §3). Reviewed every
/// `STRATEGY_REVIEW_ROUNDS` own rounds or at once after a major event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Strategy {
    /// Build the core: improvements, buildings, a defender per city.
    Stabilize,
    /// Found new cities while good sites are known.
    Expand,
    /// An enemy army threatens our cities: defenders first.
    Defend,
    /// A war with an objective city: assault groups.
    Attack,
    /// A city was lost or the army is gone: rebuild defense and economy.
    Recover,
}

impl Strategy {
    pub const ALL: [Strategy; 5] = [
        Strategy::Stabilize,
        Strategy::Expand,
        Strategy::Defend,
        Strategy::Attack,
        Strategy::Recover,
    ];

    #[inline]
    pub fn code(self) -> u8 {
        match self {
            Strategy::Stabilize => 0,
            Strategy::Expand => 1,
            Strategy::Defend => 2,
            Strategy::Attack => 3,
            Strategy::Recover => 4,
        }
    }

    #[inline]
    pub fn from_code(code: u8) -> Option<Self> {
        Strategy::ALL.into_iter().find(|s| s.code() == code)
    }
}

/// Rounds between two periodic strategy reviews.
pub const STRATEGY_REVIEW_ROUNDS: u32 = 10;

/// The current strategy and when it is looked at again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StrategyState {
    pub current: Strategy,
    pub since_round: u32,
    pub next_review_round: u32,
    /// Most cities the player ever had (a smaller count now: a city lost).
    pub peak_cities: u32,
    /// Last round a settler found no site: no expansion strategy for a
    /// while after it (no settlers piling up without land).
    pub no_site_round: Option<u32>,
}

/// What a worker builds on its tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkerJob {
    Improvement(ImprovementId),
    Road,
    /// Remove the tile's feature (it blocks a wanted improvement).
    Chop,
}

/// The job of one unit; it lasts several turns until done, blocked or
/// replaced (R-07 §3). Targets are tiles and persistent ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitTask {
    /// Walk to a frontier tile of the known map.
    Scout { target: TileId },
    /// Walk a settler to a site and found a city there.
    Settle { site: TileId },
    /// Stay with a civilian unit.
    Escort { ward: EntityId },
    /// Walk a worker to a tile and build there.
    Improve { tile: TileId, job: WorkerJob },
    /// Garrison the city on this tile.
    Defend { city: TileId },
    /// Take part in the attack on the city on this tile.
    Assault { target: TileId },
    /// Rest on this tile until healed.
    Heal { at: TileId },
    /// Serve in the operation against the city on this tile (`Operation`).
    Operation { target: TileId },
    /// Serve in the raid on the land around the enemy city on this tile (the
    /// classic AI's `ai::raid`; the raid is the units that hold this task).
    Raid { target: TileId },
}

impl UnitTask {
    /// The tile this task holds for its unit, if any: no second unit gets
    /// the same scout target, settle site or worker job.
    pub(crate) fn reserved_tile(&self) -> Option<Reservation> {
        match *self {
            UnitTask::Scout { target } => Some(Reservation::Frontier(target)),
            UnitTask::Settle { site } => Some(Reservation::Site(site)),
            UnitTask::Improve { tile, .. } => Some(Reservation::Job(tile)),
            UnitTask::Defend { city } => Some(Reservation::Garrison(city)),
            UnitTask::Assault { target } => Some(Reservation::Attack(target)),
            UnitTask::Escort { .. } | UnitTask::Heal { .. } | UnitTask::Operation { .. } | UnitTask::Raid { .. } => None,
        }
    }
}

/// A target held by a task. Frontier, site and job tiles admit one holder;
/// garrisons and attack targets count their holders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reservation {
    Frontier(TileId),
    Site(TileId),
    Job(TileId),
    Garrison(TileId),
    Attack(TileId),
}

/// One unit's task and its history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskRecord {
    pub unit: EntityId,
    pub task: UnitTask,
    pub since_round: u32,
    /// Rejected commands while doing this task; the task is dropped at
    /// `TASK_FAILURE_LIMIT` (no endless repetition of a failing command).
    pub failures: u8,
}

/// Rejected commands after which a task is given up.
pub const TASK_FAILURE_LIMIT: u8 = 3;

/// What a war is fought for: a known city, or — while none of the enemy's
/// cities is known — the edge of its known territory, where the army
/// looks for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WarObjective {
    pub tile: TileId,
    pub kind: ObjectiveKind,
    pub chosen_round: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectiveKind {
    City,
    Territory,
}

/// What the AI remembers about another player beyond the public contact
/// (met, last seen — `knowledge::Contact`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RelationMemory {
    pub other: PlayerId,
    /// War as last observed (a change of it is a war or a peace).
    pub at_war: bool,
    /// Round of the last war or peace between us (cooldowns: no war/peace
    /// oscillation).
    pub last_change_round: Option<u32>,
    pub objective: Option<WarObjective>,
    /// Strongest military power of this player seen near us, and when.
    pub threat: u32,
    pub threat_round: u32,
}

impl RelationMemory {
    fn new(other: PlayerId) -> Self {
        Self {
            other,
            at_war: false,
            last_change_round: None,
            objective: None,
            threat: 0,
            threat_round: 0,
        }
    }
}

/// A city of another player as last seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SeenCity {
    pub tile: TileId,
    pub owner: PlayerId,
    pub population: u32,
    pub last_seen_round: u32,
}

/// What a fighter is to an operation's army.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArmyRole {
    /// Strikes next to its target and takes cities.
    Melee,
    /// Strikes from a distance.
    Ranged,
    /// Strikes cities from a distance, set up first.
    Siege,
}

/// Units of each role an army has or needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Composition {
    pub melee: u8,
    pub ranged: u8,
    pub siege: u8,
}

impl Composition {
    pub fn total(&self) -> u32 {
        u32::from(self.melee) + u32::from(self.ranged) + u32::from(self.siege)
    }

    pub fn of(&self, role: ArmyRole) -> u8 {
        match role {
            ArmyRole::Melee => self.melee,
            ArmyRole::Ranged => self.ranged,
            ArmyRole::Siege => self.siege,
        }
    }

    pub fn add(&mut self, role: ArmyRole) {
        let count = match role {
            ArmyRole::Melee => &mut self.melee,
            ArmyRole::Ranged => &mut self.ranged,
            ArmyRole::Siege => &mut self.siege,
        };
        *count = count.saturating_add(1);
    }
}

/// Where an operation stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationPhase {
    /// The army gathers at the muster point; cities build what it lacks.
    Forming,
    /// The army marches on the target and fights there.
    Marching,
}

/// A war operation (classic/docs/ai-operations.md): an army formed for one enemy
/// city. Its members are the units whose task is `UnitTask::Operation`
/// with its target, so a unit serves in at most one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Operation {
    /// The enemy city's tile: one operation per city.
    pub target: TileId,
    pub enemy: PlayerId,
    /// Where the army gathers (an own city).
    pub muster: TileId,
    /// The army the target needs.
    pub need: Composition,
    pub phase: OperationPhase,
    pub since_round: u32,
    pub phase_round: u32,
}

/// A war the AI saves gold for before its operation starts (the Classic
/// allocator, classic/docs/ai-allocator.md "Military treasury"): the target city, the
/// gold the operation's army needs in the treasury, the value (hundredths
/// of score points) it was chosen at, and when. It holds its gold until its
/// value falls materially or it waits too long.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WarPlan {
    pub target: TileId,
    pub enemy: PlayerId,
    pub chest: u32,
    pub value: i64,
    pub since_round: u32,
}

/// What a rules module's grand strategy pursues for a while (Classic: a path
/// to victory, `classic_rules::ai::grand_strategy`): the module's code of the
/// goal and the round it was chosen. Only the module's reviews change it,
/// so a goal holds between them (the hysteresis of the choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Aim {
    pub goal: u8,
    pub since_round: u32,
}

/// The AI memory of one player.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AiState {
    pub player: PlayerId,
    pub strategy: StrategyState,
    /// By unit id.
    tasks: Vec<TaskRecord>,
    /// By other player.
    relations: Vec<RelationMemory>,
    /// By tile.
    seen_cities: Vec<SeenCity>,
    /// By target.
    operations: Vec<Operation>,
    /// The war the treasury saves for, if any.
    pub war_plan: Option<WarPlan>,
    /// The goal of the grand strategy (None: the module has none, or has
    /// not reviewed it yet).
    pub aim: Option<Aim>,
    /// The mind's own memory, opaque to the kernel and versioned by the
    /// module (Classic: the end-to-end net's memory slots,
    /// classic/docs/ai-neural-architecture.md, Memory); empty: none.
    pub mind_memory: Vec<u8>,
}

impl AiState {
    pub fn new(player: PlayerId, round: u32) -> Self {
        Self {
            player,
            strategy: StrategyState {
                current: Strategy::Expand,
                since_round: round,
                next_review_round: round,
                peak_cities: 0,
                no_site_round: None,
            },
            tasks: Vec::new(),
            relations: Vec::new(),
            seen_cities: Vec::new(),
            operations: Vec::new(),
            war_plan: None,
            aim: None,
            mind_memory: Vec::new(),
        }
    }

    /// Restore from saved parts; the parts are sorted and deduplicated here.
    pub fn from_parts(
        player: PlayerId,
        strategy: StrategyState,
        mut tasks: Vec<TaskRecord>,
        mut relations: Vec<RelationMemory>,
        mut seen_cities: Vec<SeenCity>,
        mut operations: Vec<Operation>,
    ) -> Self {
        tasks.sort_by_key(|t| t.unit);
        tasks.dedup_by_key(|t| t.unit);
        relations.sort_by_key(|r| r.other);
        relations.dedup_by_key(|r| r.other);
        seen_cities.sort_by_key(|c| c.tile);
        seen_cities.dedup_by_key(|c| c.tile);
        operations.sort_by_key(|o| o.target);
        operations.dedup_by_key(|o| o.target);
        Self {
            player,
            strategy,
            tasks,
            relations,
            seen_cities,
            operations,
            war_plan: None,
            aim: None,
            mind_memory: Vec::new(),
        }
    }

    #[inline]
    pub fn tasks(&self) -> &[TaskRecord] {
        &self.tasks
    }

    #[inline]
    pub fn relations(&self) -> &[RelationMemory] {
        &self.relations
    }

    #[inline]
    pub fn seen_cities(&self) -> &[SeenCity] {
        &self.seen_cities
    }

    #[inline]
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    #[inline]
    pub fn operation(&self, target: TileId) -> Option<&Operation> {
        let at = self.operations.binary_search_by_key(&target, |o| o.target).ok()?;
        Some(&self.operations[at])
    }

    /// Start an operation (replaces one against the same target).
    pub fn start_operation(&mut self, operation: Operation) {
        match self.operations.binary_search_by_key(&operation.target, |o| o.target) {
            Ok(at) => self.operations[at] = operation,
            Err(at) => self.operations.insert(at, operation),
        }
    }

    /// Move an operation to `phase` from `round`.
    pub fn set_operation_phase(&mut self, target: TileId, phase: OperationPhase, round: u32) {
        if let Ok(at) = self.operations.binary_search_by_key(&target, |o| o.target) {
            self.operations[at].phase = phase;
            self.operations[at].phase_round = round;
        }
    }

    /// End an operation and release its members (their tasks go).
    pub fn end_operation(&mut self, target: TileId) {
        if let Ok(at) = self.operations.binary_search_by_key(&target, |o| o.target) {
            self.operations.remove(at);
        }
        self.tasks.retain(|t| t.task != UnitTask::Operation { target });
    }

    #[inline]
    pub fn task_of(&self, unit: EntityId) -> Option<&TaskRecord> {
        let at = self.tasks.binary_search_by_key(&unit, |t| t.unit).ok()?;
        Some(&self.tasks[at])
    }

    /// Give `unit` a new task (replaces the old one, failures reset).
    pub fn set_task(&mut self, unit: EntityId, task: UnitTask, round: u32) {
        let record = TaskRecord {
            unit,
            task,
            since_round: round,
            failures: 0,
        };
        match self.tasks.binary_search_by_key(&unit, |t| t.unit) {
            Ok(at) => self.tasks[at] = record,
            Err(at) => self.tasks.insert(at, record),
        }
    }

    #[inline]
    pub fn clear_task(&mut self, unit: EntityId) {
        if let Ok(at) = self.tasks.binary_search_by_key(&unit, |t| t.unit) {
            self.tasks.remove(at);
        }
    }

    /// Count a rejected command of the unit's task; the task is dropped at
    /// the limit. Returns true when it was dropped.
    pub fn note_failure(&mut self, unit: EntityId) -> bool {
        let Ok(at) = self.tasks.binary_search_by_key(&unit, |t| t.unit) else {
            return false;
        };
        self.tasks[at].failures += 1;
        if self.tasks[at].failures < TASK_FAILURE_LIMIT {
            return false;
        }
        self.tasks.remove(at);
        true
    }

    /// Forget the tasks of units that no longer exist (`alive` sorted).
    #[inline]
    pub fn retain_units(&mut self, alive: &[EntityId]) {
        self.tasks.retain(|t| alive.binary_search(&t.unit).is_ok());
    }

    /// Units holding `reservation`, other than `except`.
    #[inline]
    pub fn holders(&self, reservation: Reservation, except: Option<EntityId>) -> usize {
        let held =
            |t: &&TaskRecord| t.task.reserved_tile() == Some(reservation) && Some(t.unit) != except;
        self.tasks.iter().filter(held).count()
    }

    /// Every reservation of one kind (by `pick`), sorted, for fast lookups
    /// during one decision.
    pub fn reserved_tiles(
        &self,
        pick: fn(Reservation) -> Option<TileId>,
        except: Option<EntityId>,
    ) -> Vec<TileId> {
        let mut out = Vec::new();
        for record in &self.tasks {
            if Some(record.unit) == except {
                continue;
            }
            if let Some(tile) = record.task.reserved_tile().and_then(pick) {
                out.push(tile);
            }
        }
        out.sort();
        out
    }

    #[inline]
    pub fn relation(&self, other: PlayerId) -> Option<&RelationMemory> {
        let at = self
            .relations
            .binary_search_by_key(&other, |r| r.other)
            .ok()?;
        Some(&self.relations[at])
    }

    /// The memory of `other`, created on first use.
    pub fn relation_mut(&mut self, other: PlayerId) -> &mut RelationMemory {
        let at = match self.relations.binary_search_by_key(&other, |r| r.other) {
            Ok(at) => at,
            Err(at) => {
                self.relations.insert(at, RelationMemory::new(other));
                at
            }
        };
        &mut self.relations[at]
    }

    /// Remember a foreign city seen now (insert or refresh).
    #[inline]
    pub fn see_city(&mut self, city: SeenCity) {
        match self
            .seen_cities
            .binary_search_by_key(&city.tile, |c| c.tile)
        {
            Ok(at) => self.seen_cities[at] = city,
            Err(at) => self.seen_cities.insert(at, city),
        }
    }

    /// Forget a remembered city: its tile is visible and holds no foreign
    /// city any more (captured by us, or gone).
    #[inline]
    pub fn forget_city(&mut self, tile: TileId) {
        if let Ok(at) = self.seen_cities.binary_search_by_key(&tile, |c| c.tile) {
            self.seen_cities.remove(at);
        }
    }
}

/// The memory of every AI player of a game, by player id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct AiStates {
    players: Vec<AiState>,
}

impl AiStates {
    #[inline]
    pub(crate) fn from_states(mut players: Vec<AiState>) -> Self {
        players.sort_by_key(|s| s.player);
        players.dedup_by_key(|s| s.player);
        Self { players }
    }

    #[inline]
    pub fn states(&self) -> &[AiState] {
        &self.players
    }

    #[inline]
    pub fn get(&self, player: PlayerId) -> Option<&AiState> {
        let at = self
            .players
            .binary_search_by_key(&player, |s| s.player)
            .ok()?;
        Some(&self.players[at])
    }

    /// The memory of `player`, created at `round` on first use.
    pub(crate) fn get_or_create(&mut self, player: PlayerId, round: u32) -> &mut AiState {
        let at = match self.players.binary_search_by_key(&player, |s| s.player) {
            Ok(at) => at,
            Err(at) => {
                self.players.insert(at, AiState::new(player, round));
                at
            }
        };
        &mut self.players[at]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_stay_sorted_and_reserve_their_targets() {
        let mut state = AiState::new(PlayerId(1), 0);
        state.set_task(EntityId(9), UnitTask::Scout { target: TileId(40) }, 0);
        state.set_task(EntityId(3), UnitTask::Settle { site: TileId(12) }, 0);
        state.set_task(EntityId(5), UnitTask::Defend { city: TileId(7) }, 0);
        let ids: Vec<u64> = state.tasks().iter().map(|t| t.unit.0).collect();
        assert_eq!(ids, vec![3, 5, 9]);
        assert_eq!(state.holders(Reservation::Site(TileId(12)), None), 1);
        assert_eq!(
            state.holders(Reservation::Site(TileId(12)), Some(EntityId(3))),
            0
        );
        let frontier = |r| match r {
            Reservation::Frontier(t) => Some(t),
            _ => None,
        };
        assert_eq!(state.reserved_tiles(frontier, None), vec![TileId(40)]);
        state.retain_units(&[EntityId(5), EntityId(9)]);
        assert!(state.task_of(EntityId(3)).is_none());
    }

    #[test]
    fn a_failing_task_is_dropped_at_the_limit() {
        let mut state = AiState::new(PlayerId(0), 0);
        state.set_task(EntityId(1), UnitTask::Heal { at: TileId(2) }, 0);
        for _ in 1..TASK_FAILURE_LIMIT {
            assert!(!state.note_failure(EntityId(1)));
        }
        assert!(state.note_failure(EntityId(1)));
        assert!(state.task_of(EntityId(1)).is_none());
    }
}
