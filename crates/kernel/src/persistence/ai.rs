//! Save schema of the AI memory (R-07 AI §3: the AI state changes future
//! commands, so a loaded game continues only if it is saved). Versioned on
//! its own (`AI_STATE_VERSION`): the world schema does not change when the
//! AI memory does. Plain values only: persistent ids and tiles.

use crate::ai::state::{
    Aim, AiState, AiStates, Composition, ObjectiveKind, Operation, OperationPhase, RelationMemory,
    SeenCity, Strategy, StrategyState, TaskRecord, UnitTask, WarObjective, WarPlan, WorkerJob,
};
use crate::ids::{EntityId, ImprovementId, PlayerId, TileId};
use crate::persistence::DecodeError;

/// 1: strategy, unit tasks, relation memory, seen cities, difficulty;
/// 2: war operations after the players (classic/docs/ai-operations.md);
/// 3: war plans after the operations (classic/docs/ai-allocator.md);
/// 4: the grand strategy's aims after the war plans (classic/docs/ai-leaders.md);
/// 5: the raid task (code 8; classic/docs/ai-raids.md), the layout of 4;
/// 6: every player's mind memory (`AiState::mind_memory`, opaque bytes of the
/// module) after the aims. A memory is written in the oldest version that
/// holds it, byte for byte as before.
pub const AI_STATE_VERSION: u32 = 6;
/// The first version with operations, with war plans, with aims, and with raids.
pub const OPERATIONS_VERSION: u32 = 2;
pub const WAR_PLAN_VERSION: u32 = 3;
pub const AIM_VERSION: u32 = 4;
pub const RAID_VERSION: u32 = 5;
/// The first version with the minds' own memory.
pub const MIND_MEMORY_VERSION: u32 = 6;
/// The task code of a raid.
const RAID_TASK: u8 = 8;
/// The oldest version that loads.
pub const AI_STATE_OLDEST: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAiStates {
    pub version: u32,
    /// Content key of the game's difficulty (`core:handicap/<level>`).
    pub difficulty: String,
    pub players: Vec<SavedAiState>,
}

impl Default for SavedAiStates {
    fn default() -> Self {
        Self {
            version: AI_STATE_OLDEST,
            difficulty: String::new(),
            players: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAiState {
    pub player: u8,
    /// `Strategy::code`.
    pub strategy: u8,
    pub strategy_since: u32,
    pub next_review: u32,
    pub peak_cities: u32,
    pub no_site_round: Option<u32>,
    pub tasks: Vec<SavedTask>,
    pub relations: Vec<SavedRelation>,
    pub seen_cities: Vec<SavedSeenCity>,
    /// Version 2 (empty in version 1).
    pub operations: Vec<SavedOperation>,
    /// Version 3 (None before).
    pub war_plan: Option<SavedWarPlan>,
    /// Version 4 (None before).
    pub aim: Option<SavedAim>,
    /// Version 6 (empty before): the mind's own memory, as the module wrote it.
    pub mind_memory: Vec<u8>,
}

/// The grand strategy's goal: the module's code and the round it was
/// chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedAim {
    pub goal: u8,
    pub since: u32,
}

/// A war plan: the target tile, the enemy, the chest in gold, the value it
/// was chosen at (hundredths of score points) and its round.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedWarPlan {
    pub target: u32,
    pub enemy: u8,
    pub chest: u32,
    pub value: i64,
    pub since: u32,
}

/// A war operation: `phase` 0 forming, 1 marching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedOperation {
    pub target: u32,
    pub enemy: u8,
    pub muster: u32,
    pub melee: u8,
    pub ranged: u8,
    pub siege: u8,
    pub phase: u8,
    pub since: u32,
    pub phase_round: u32,
}

/// Worker job code of a chop (above every improvement code).
const CHOP_JOB: u64 = 2 + u16::MAX as u64;

/// A unit task: `kind` 0 scout, 1 settle, 2 escort, 3 improve, 4 defend,
/// 5 assault, 6 heal, 7 operation (version 2), 8 raid (version 5); `tile` its tile; `detail` the escorted unit id
/// (escort) or the worker job (0 road, 1 + improvement id, `CHOP_JOB`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedTask {
    pub unit: u64,
    pub kind: u8,
    pub tile: u32,
    pub detail: u64,
    pub since: u32,
    pub failures: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedRelation {
    pub other: u8,
    pub at_war: bool,
    pub last_change: Option<u32>,
    pub objective: Option<SavedObjective>,
    pub threat: u32,
    pub threat_round: u32,
}

/// A war objective: its tile, `kind` 0 city / 1 territory, the round it
/// was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedObjective {
    pub tile: u32,
    pub kind: u8,
    pub round: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSeenCity {
    pub tile: u32,
    pub owner: u8,
    pub population: u32,
    pub last_seen_round: u32,
}

pub(crate) fn encode_ai(states: &AiStates, difficulty: &str) -> SavedAiStates {
    let players: Vec<SavedAiState> = states.states().iter().map(encode_state).collect();
    let operations = players.iter().any(|p| !p.operations.is_empty());
    let plans = players.iter().any(|p| p.war_plan.is_some());
    let aims = players.iter().any(|p| p.aim.is_some());
    let raids = players.iter().any(|p| p.tasks.iter().any(|t| t.kind == RAID_TASK));
    let minds = players.iter().any(|p| !p.mind_memory.is_empty());
    let version = if minds {
        MIND_MEMORY_VERSION
    } else if raids {
        RAID_VERSION
    } else if aims {
        AIM_VERSION
    } else if plans {
        WAR_PLAN_VERSION
    } else if operations {
        OPERATIONS_VERSION
    } else {
        AI_STATE_OLDEST
    };
    SavedAiStates {
        version,
        difficulty: difficulty.to_owned(),
        players,
    }
}

fn encode_state(state: &AiState) -> SavedAiState {
    SavedAiState {
        player: state.player.0,
        strategy: state.strategy.current.code(),
        strategy_since: state.strategy.since_round,
        next_review: state.strategy.next_review_round,
        peak_cities: state.strategy.peak_cities,
        no_site_round: state.strategy.no_site_round,
        tasks: state.tasks().iter().map(encode_task).collect(),
        relations: state.relations().iter().map(encode_relation).collect(),
        seen_cities: state.seen_cities().iter().map(encode_city).collect(),
        operations: state.operations().iter().map(encode_operation).collect(),
        war_plan: state.war_plan.map(|plan| SavedWarPlan {
            target: plan.target.0,
            enemy: plan.enemy.0,
            chest: plan.chest,
            value: plan.value,
            since: plan.since_round,
        }),
        aim: state.aim.map(|aim| SavedAim { goal: aim.goal, since: aim.since_round }),
        mind_memory: state.mind_memory.clone(),
    }
}

fn encode_operation(operation: &Operation) -> SavedOperation {
    SavedOperation {
        target: operation.target.0,
        enemy: operation.enemy.0,
        muster: operation.muster.0,
        melee: operation.need.melee,
        ranged: operation.need.ranged,
        siege: operation.need.siege,
        phase: match operation.phase {
            OperationPhase::Forming => 0,
            OperationPhase::Marching => 1,
        },
        since: operation.since_round,
        phase_round: operation.phase_round,
    }
}

fn encode_task(record: &TaskRecord) -> SavedTask {
    let (kind, tile, detail) = match record.task {
        UnitTask::Scout { target } => (0, target.0, 0),
        UnitTask::Settle { site } => (1, site.0, 0),
        UnitTask::Escort { ward } => (2, 0, ward.0),
        UnitTask::Improve { tile, job } => (3, tile.0, encode_job(job)),
        UnitTask::Defend { city } => (4, city.0, 0),
        UnitTask::Assault { target } => (5, target.0, 0),
        UnitTask::Heal { at } => (6, at.0, 0),
        UnitTask::Operation { target } => (7, target.0, 0),
        UnitTask::Raid { target } => (RAID_TASK, target.0, 0),
    };
    SavedTask {
        unit: record.unit.0,
        kind,
        tile,
        detail,
        since: record.since_round,
        failures: record.failures,
    }
}

fn encode_job(job: WorkerJob) -> u64 {
    match job {
        WorkerJob::Road => 0,
        WorkerJob::Chop => CHOP_JOB,
        WorkerJob::Improvement(id) => 1 + u64::from(id.0),
    }
}

fn encode_relation(memory: &RelationMemory) -> SavedRelation {
    SavedRelation {
        other: memory.other.0,
        at_war: memory.at_war,
        last_change: memory.last_change_round,
        objective: memory.objective.map(|o| SavedObjective {
            tile: o.tile.0,
            kind: match o.kind {
                ObjectiveKind::City => 0,
                ObjectiveKind::Territory => 1,
            },
            round: o.chosen_round,
        }),
        threat: memory.threat,
        threat_round: memory.threat_round,
    }
}

fn encode_city(city: &SeenCity) -> SavedSeenCity {
    SavedSeenCity {
        tile: city.tile.0,
        owner: city.owner.0,
        population: city.population,
        last_seen_round: city.last_seen_round,
    }
}

/// Untrusted input: an unknown code is `DecodeError::Corrupt`, a different
/// version `SchemaMismatch`. `tiles` bounds every tile.
pub(crate) fn decode_ai(saved: &SavedAiStates, tiles: usize) -> Result<AiStates, DecodeError> {
    if !(AI_STATE_OLDEST..=AI_STATE_VERSION).contains(&saved.version) {
        return Err(DecodeError::SchemaMismatch(saved.version));
    }
    let mut states = Vec::with_capacity(saved.players.len());
    for player in &saved.players {
        states.push(decode_state(player, tiles)?);
    }
    Ok(AiStates::from_states(states))
}

fn decode_state(saved: &SavedAiState, tiles: usize) -> Result<AiState, DecodeError> {
    let current = Strategy::from_code(saved.strategy).ok_or_else(|| corrupt("ai strategy"))?;
    let strategy = StrategyState {
        current,
        since_round: saved.strategy_since,
        next_review_round: saved.next_review,
        peak_cities: saved.peak_cities,
        no_site_round: saved.no_site_round,
    };
    let mut tasks = Vec::with_capacity(saved.tasks.len());
    for task in &saved.tasks {
        tasks.push(decode_task(task, tiles)?);
    }
    let mut relations = Vec::with_capacity(saved.relations.len());
    for relation in &saved.relations {
        relations.push(decode_relation(relation, tiles)?);
    }
    let mut cities = Vec::with_capacity(saved.seen_cities.len());
    for city in &saved.seen_cities {
        cities.push(SeenCity {
            tile: tile(city.tile, tiles)?,
            owner: PlayerId(city.owner),
            population: city.population,
            last_seen_round: city.last_seen_round,
        });
    }
    let mut operations = Vec::with_capacity(saved.operations.len());
    for operation in &saved.operations {
        operations.push(decode_operation(operation, tiles)?);
    }
    let mut state = AiState::from_parts(PlayerId(saved.player), strategy, tasks, relations, cities, operations);
    state.war_plan = match &saved.war_plan {
        Some(plan) => Some(WarPlan {
            target: tile(plan.target, tiles)?,
            enemy: PlayerId(plan.enemy),
            chest: plan.chest,
            value: plan.value,
            since_round: plan.since,
        }),
        None => None,
    };
    state.aim = saved.aim.as_ref().map(|aim| Aim { goal: aim.goal, since_round: aim.since });
    state.mind_memory = saved.mind_memory.clone();
    Ok(state)
}

fn decode_operation(saved: &SavedOperation, tiles: usize) -> Result<Operation, DecodeError> {
    let phase = match saved.phase {
        0 => OperationPhase::Forming,
        1 => OperationPhase::Marching,
        _ => return Err(corrupt("ai operation phase")),
    };
    Ok(Operation {
        target: tile(saved.target, tiles)?,
        enemy: PlayerId(saved.enemy),
        muster: tile(saved.muster, tiles)?,
        need: Composition {
            melee: saved.melee,
            ranged: saved.ranged,
            siege: saved.siege,
        },
        phase,
        since_round: saved.since,
        phase_round: saved.phase_round,
    })
}

fn decode_task(saved: &SavedTask, tiles: usize) -> Result<TaskRecord, DecodeError> {
    let at = || tile(saved.tile, tiles);
    let task = match saved.kind {
        0 => UnitTask::Scout { target: at()? },
        1 => UnitTask::Settle { site: at()? },
        2 => UnitTask::Escort {
            ward: EntityId(saved.detail),
        },
        3 => UnitTask::Improve {
            tile: at()?,
            job: decode_job(saved.detail)?,
        },
        4 => UnitTask::Defend { city: at()? },
        5 => UnitTask::Assault { target: at()? },
        6 => UnitTask::Heal { at: at()? },
        7 => UnitTask::Operation { target: at()? },
        RAID_TASK => UnitTask::Raid { target: at()? },
        _ => return Err(corrupt("ai task kind")),
    };
    Ok(TaskRecord {
        unit: EntityId(saved.unit),
        task,
        since_round: saved.since,
        failures: saved.failures,
    })
}

fn decode_job(detail: u64) -> Result<WorkerJob, DecodeError> {
    match detail {
        0 => Ok(WorkerJob::Road),
        CHOP_JOB => Ok(WorkerJob::Chop),
        d if d <= 1 + u64::from(u16::MAX) => {
            Ok(WorkerJob::Improvement(ImprovementId((d - 1) as u16)))
        }
        _ => Err(corrupt("ai worker job")),
    }
}

fn decode_relation(saved: &SavedRelation, tiles: usize) -> Result<RelationMemory, DecodeError> {
    let objective = match &saved.objective {
        Some(objective) => {
            let kind = match objective.kind {
                0 => ObjectiveKind::City,
                1 => ObjectiveKind::Territory,
                _ => return Err(corrupt("ai objective kind")),
            };
            Some(WarObjective {
                tile: tile(objective.tile, tiles)?,
                kind,
                chosen_round: objective.round,
            })
        }
        None => None,
    };
    Ok(RelationMemory {
        other: PlayerId(saved.other),
        at_war: saved.at_war,
        last_change_round: saved.last_change,
        objective,
        threat: saved.threat,
        threat_round: saved.threat_round,
    })
}

fn tile(value: u32, tiles: usize) -> Result<TileId, DecodeError> {
    if (value as usize) < tiles {
        Ok(TileId(value))
    } else {
        Err(corrupt("ai tile out of map"))
    }
}

fn corrupt(what: &str) -> DecodeError {
    DecodeError::Corrupt(what.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_memory_round_trips_and_rejects_damage() {
        let mut state = AiState::new(PlayerId(2), 4);
        state.set_task(EntityId(7), UnitTask::Scout { target: TileId(30) }, 4);
        let job = WorkerJob::Improvement(ImprovementId(3));
        state.set_task(
            EntityId(8),
            UnitTask::Improve {
                tile: TileId(31),
                job,
            },
            5,
        );
        state.set_task(EntityId(9), UnitTask::Escort { ward: EntityId(8) }, 5);
        state.relation_mut(PlayerId(0)).objective = Some(WarObjective {
            tile: TileId(44),
            kind: ObjectiveKind::Territory,
            chosen_round: 3,
        });
        state.see_city(SeenCity {
            tile: TileId(44),
            owner: PlayerId(0),
            population: 3,
            last_seen_round: 4,
        });
        let states = AiStates::from_states(vec![state]);
        let saved = encode_ai(&states, "core:handicap/prince");
        assert_eq!(decode_ai(&saved, 100).unwrap(), states);
        assert!(decode_ai(&saved, 40).is_err(), "tile beyond a smaller map");
        let mut damaged = saved.clone();
        damaged.players[0].tasks[0].kind = 42;
        assert!(decode_ai(&damaged, 100).is_err());
        let mut future = saved;
        future.version = 99;
        assert_eq!(
            decode_ai(&future, 100),
            Err(DecodeError::SchemaMismatch(99))
        );
    }

    fn operation() -> Operation {
        Operation {
            target: TileId(44),
            enemy: PlayerId(0),
            muster: TileId(12),
            need: Composition { melee: 3, ranged: 2, siege: 1 },
            phase: OperationPhase::Marching,
            since_round: 50,
            phase_round: 61,
        }
    }

    #[test]
    fn a_memory_without_operations_is_version_1() {
        let mut state = AiState::new(PlayerId(1), 0);
        state.set_task(EntityId(4), UnitTask::Defend { city: TileId(9) }, 0);
        let saved = encode_ai(&AiStates::from_states(vec![state]), "core:handicap/prince");
        assert_eq!(saved.version, AI_STATE_OLDEST);
    }

    #[test]
    fn a_war_plan_round_trips_as_version_3_and_operations_stay_version_2() {
        let mut state = AiState::new(PlayerId(2), 40);
        state.start_operation(operation());
        let saved = encode_ai(&AiStates::from_states(vec![state.clone()]), "core:handicap/prince");
        assert_eq!(saved.version, OPERATIONS_VERSION, "no plan: the version 2 bytes as before");
        state.war_plan = Some(WarPlan { target: TileId(44), enemy: PlayerId(0), chest: 320, value: 1_250, since_round: 41 });
        let states = AiStates::from_states(vec![state]);
        let saved = encode_ai(&states, "core:handicap/prince");
        assert_eq!(saved.version, WAR_PLAN_VERSION);
        assert_eq!(decode_ai(&saved, 100).unwrap(), states);
        assert!(decode_ai(&saved, 40).is_err(), "plan target beyond a smaller map");
    }

    #[test]
    fn an_aim_round_trips_as_version_4_and_plans_stay_version_3() {
        let mut state = AiState::new(PlayerId(2), 40);
        state.war_plan = Some(WarPlan { target: TileId(44), enemy: PlayerId(0), chest: 320, value: 1_250, since_round: 41 });
        let saved = encode_ai(&AiStates::from_states(vec![state.clone()]), "core:handicap/prince");
        assert_eq!(saved.version, WAR_PLAN_VERSION, "no aim: the version 3 bytes as before");
        state.aim = Some(Aim { goal: 1, since_round: 50 });
        let states = AiStates::from_states(vec![state]);
        let saved = encode_ai(&states, "core:handicap/prince");
        assert_eq!(saved.version, AIM_VERSION);
        assert_eq!(decode_ai(&saved, 100).unwrap(), states);
    }

    #[test]
    fn a_raid_task_round_trips_as_version_5_and_other_memory_keeps_its_version() {
        let mut state = AiState::new(PlayerId(2), 40);
        state.aim = Some(Aim { goal: 1, since_round: 50 });
        let saved = encode_ai(&AiStates::from_states(vec![state.clone()]), "core:handicap/prince");
        assert_eq!(saved.version, AIM_VERSION, "no raid: the version 4 bytes as before");
        state.set_task(EntityId(9), UnitTask::Raid { target: TileId(44) }, 60);
        let states = AiStates::from_states(vec![state]);
        let saved = encode_ai(&states, "core:handicap/prince");
        assert_eq!(saved.version, RAID_VERSION);
        assert_eq!(decode_ai(&saved, 100).unwrap(), states);
        assert!(decode_ai(&saved, 40).is_err(), "raid target beyond a smaller map");
    }

    #[test]
    fn operations_round_trip_and_end_with_their_members() {
        let mut state = AiState::new(PlayerId(2), 40);
        state.start_operation(operation());
        state.set_task(EntityId(7), UnitTask::Operation { target: TileId(44) }, 50);
        state.set_task(EntityId(8), UnitTask::Defend { city: TileId(12) }, 50);
        let states = AiStates::from_states(vec![state.clone()]);
        let saved = encode_ai(&states, "core:handicap/prince");
        assert_eq!(saved.version, OPERATIONS_VERSION);
        assert_eq!(decode_ai(&saved, 100).unwrap(), states);
        let mut damaged = saved.clone();
        damaged.players[0].operations[0].phase = 9;
        assert!(decode_ai(&damaged, 100).is_err());
        state.end_operation(TileId(44));
        assert!(state.operations().is_empty());
        assert!(state.task_of(EntityId(7)).is_none(), "members are released");
        assert!(state.task_of(EntityId(8)).is_some());
    }
}
