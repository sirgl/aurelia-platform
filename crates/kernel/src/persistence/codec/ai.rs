//! Bytes of the AI memory section of a save (`crate::persistence::ai`),
//! written after the world in the same field-by-field layout. Version 2
//! appends every player's operations after the players: a version 1 memory
//! keeps its bytes.

use crate::persistence::ai::{
    SavedAiState, SavedAiStates, SavedObjective, SavedOperation, SavedRelation, SavedSeenCity,
    SavedAim, SavedTask, SavedWarPlan, AIM_VERSION, MIND_MEMORY_VERSION, OPERATIONS_VERSION,
    WAR_PLAN_VERSION,
};

use super::{CodecError, Field, Reader};

impl Field for SavedTask {
    fn put(&self, out: &mut Vec<u8>) {
        self.unit.put(out);
        self.kind.put(out);
        self.tile.put(out);
        self.detail.put(out);
        self.since.put(out);
        self.failures.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            unit: Field::take(r)?,
            kind: Field::take(r)?,
            tile: Field::take(r)?,
            detail: Field::take(r)?,
            since: Field::take(r)?,
            failures: Field::take(r)?,
        })
    }
}

impl Field for SavedRelation {
    fn put(&self, out: &mut Vec<u8>) {
        self.other.put(out);
        self.at_war.put(out);
        self.last_change.put(out);
        self.objective.put(out);
        self.threat.put(out);
        self.threat_round.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            other: Field::take(r)?,
            at_war: Field::take(r)?,
            last_change: Field::take(r)?,
            objective: Field::take(r)?,
            threat: Field::take(r)?,
            threat_round: Field::take(r)?,
        })
    }
}

impl Field for SavedObjective {
    fn put(&self, out: &mut Vec<u8>) {
        self.tile.put(out);
        self.kind.put(out);
        self.round.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            tile: Field::take(r)?,
            kind: Field::take(r)?,
            round: Field::take(r)?,
        })
    }
}

impl Field for SavedSeenCity {
    fn put(&self, out: &mut Vec<u8>) {
        self.tile.put(out);
        self.owner.put(out);
        self.population.put(out);
        self.last_seen_round.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            tile: Field::take(r)?,
            owner: Field::take(r)?,
            population: Field::take(r)?,
            last_seen_round: Field::take(r)?,
        })
    }
}

impl Field for SavedAiState {
    fn put(&self, out: &mut Vec<u8>) {
        self.player.put(out);
        self.strategy.put(out);
        self.strategy_since.put(out);
        self.next_review.put(out);
        self.peak_cities.put(out);
        self.no_site_round.put(out);
        self.tasks.put(out);
        self.relations.put(out);
        self.seen_cities.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            player: Field::take(r)?,
            strategy: Field::take(r)?,
            strategy_since: Field::take(r)?,
            next_review: Field::take(r)?,
            peak_cities: Field::take(r)?,
            no_site_round: Field::take(r)?,
            tasks: Field::take(r)?,
            relations: Field::take(r)?,
            seen_cities: Field::take(r)?,
            operations: Vec::new(),
            war_plan: None,
            aim: None,
            mind_memory: Vec::new(),
        })
    }
}

impl Field for SavedAim {
    fn put(&self, out: &mut Vec<u8>) {
        self.goal.put(out);
        self.since.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self { goal: Field::take(r)?, since: Field::take(r)? })
    }
}

impl Field for SavedOperation {
    fn put(&self, out: &mut Vec<u8>) {
        self.target.put(out);
        self.enemy.put(out);
        self.muster.put(out);
        self.melee.put(out);
        self.ranged.put(out);
        self.siege.put(out);
        self.phase.put(out);
        self.since.put(out);
        self.phase_round.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            target: Field::take(r)?,
            enemy: Field::take(r)?,
            muster: Field::take(r)?,
            melee: Field::take(r)?,
            ranged: Field::take(r)?,
            siege: Field::take(r)?,
            phase: Field::take(r)?,
            since: Field::take(r)?,
            phase_round: Field::take(r)?,
        })
    }
}

impl Field for SavedWarPlan {
    fn put(&self, out: &mut Vec<u8>) {
        self.target.put(out);
        self.enemy.put(out);
        self.chest.put(out);
        self.value.put(out);
        self.since.put(out);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            target: Field::take(r)?,
            enemy: Field::take(r)?,
            chest: Field::take(r)?,
            value: Field::take(r)?,
            since: Field::take(r)?,
        })
    }
}

impl Field for SavedAiStates {
    fn put(&self, out: &mut Vec<u8>) {
        self.version.put(out);
        self.difficulty.put(out);
        self.players.put(out);
        if self.version >= OPERATIONS_VERSION {
            for player in &self.players {
                player.operations.put(out);
            }
        }
        if self.version >= WAR_PLAN_VERSION {
            for player in &self.players {
                player.war_plan.put(out);
            }
        }
        if self.version >= AIM_VERSION {
            for player in &self.players {
                player.aim.put(out);
            }
        }
        if self.version >= MIND_MEMORY_VERSION {
            for player in &self.players {
                player.mind_memory.put(out);
            }
        }
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, CodecError> {
        let mut saved = Self {
            version: Field::take(r)?,
            difficulty: Field::take(r)?,
            players: Field::take(r)?,
        };
        if saved.version >= OPERATIONS_VERSION {
            for player in &mut saved.players {
                player.operations = Field::take(r)?;
            }
        }
        if saved.version >= WAR_PLAN_VERSION {
            for player in &mut saved.players {
                player.war_plan = Field::take(r)?;
            }
        }
        if saved.version >= AIM_VERSION {
            for player in &mut saved.players {
                player.aim = Field::take(r)?;
            }
        }
        if saved.version >= MIND_MEMORY_VERSION {
            for player in &mut saved.players {
                player.mind_memory = Field::take(r)?;
            }
        }
        Ok(saved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(operations: Vec<SavedOperation>) -> SavedAiState {
        SavedAiState {
            player: 1,
            strategy: 3,
            strategy_since: 40,
            next_review: 50,
            peak_cities: 3,
            no_site_round: None,
            tasks: vec![SavedTask { unit: 7, kind: 7, tile: 44, detail: 0, since: 41, failures: 0 }],
            relations: Vec::new(),
            seen_cities: Vec::new(),
            operations,
            war_plan: None,
            aim: None,
            mind_memory: Vec::new(),
        }
    }

    fn bytes(saved: &SavedAiStates) -> Vec<u8> {
        let mut out = Vec::new();
        saved.put(&mut out);
        out
    }

    #[test]
    fn version_6_bytes_round_trip_and_version_5_bytes_load_without_mind_memory() {
        let mut thinking = player(Vec::new());
        thinking.aim = Some(SavedAim { goal: 1, since: 60 });
        thinking.mind_memory = vec![1, 2, 3, 0, 255];
        let v6 = SavedAiStates { version: 6, difficulty: "d".into(), players: vec![thinking.clone(), player(Vec::new())] };
        let read = SavedAiStates::take(&mut Reader { bytes: &bytes(&v6), at: 0, version: 0 }).unwrap();
        assert_eq!(read, v6);
        thinking.mind_memory.clear();
        let v5 = SavedAiStates { version: 5, difficulty: "d".into(), players: vec![thinking] };
        let v5_bytes = bytes(&v5);
        let read = SavedAiStates::take(&mut Reader { bytes: &v5_bytes, at: 0, version: 0 }).unwrap();
        assert_eq!(read, v5);
        assert!(read.players[0].mind_memory.is_empty());
    }

    #[test]
    fn version_4_bytes_round_trip_and_version_3_bytes_load_without_aims() {
        let mut aiming = player(Vec::new());
        aiming.aim = Some(SavedAim { goal: 1, since: 60 });
        aiming.war_plan = Some(SavedWarPlan { target: 44, enemy: 0, chest: 320, value: 900, since: 61 });
        let v4 = SavedAiStates { version: 4, difficulty: "d".into(), players: vec![aiming.clone(), player(Vec::new())] };
        let read = SavedAiStates::take(&mut Reader { bytes: &bytes(&v4), at: 0, version: 0 }).unwrap();
        assert_eq!(read, v4);
        aiming.aim = None;
        let v3 = SavedAiStates { version: 3, difficulty: "d".into(), players: vec![aiming] };
        let v3_bytes = bytes(&v3);
        assert!(v3_bytes.len() < bytes(&v4).len());
        let read = SavedAiStates::take(&mut Reader { bytes: &v3_bytes, at: 0, version: 0 }).unwrap();
        assert_eq!(read, v3);
        assert!(read.players[0].aim.is_none());
    }

    #[test]
    fn version_3_bytes_round_trip_and_version_2_bytes_load_without_plans() {
        let operation = SavedOperation { target: 44, enemy: 0, muster: 12, melee: 3, ranged: 2, siege: 1, phase: 0, since: 40, phase_round: 40 };
        let mut planner = player(vec![operation.clone()]);
        planner.war_plan = Some(SavedWarPlan { target: 44, enemy: 0, chest: 320, value: -7, since: 41 });
        let v3 = SavedAiStates { version: 3, difficulty: "d".into(), players: vec![planner, player(Vec::new())] };
        let read = SavedAiStates::take(&mut Reader { bytes: &bytes(&v3), at: 0, version: 0 }).unwrap();
        assert_eq!(read, v3);
        let v2 = SavedAiStates { version: 2, difficulty: "d".into(), players: vec![player(vec![operation])] };
        let read = SavedAiStates::take(&mut Reader { bytes: &bytes(&v2), at: 0, version: 0 }).unwrap();
        assert_eq!(read, v2);
        assert!(read.players[0].war_plan.is_none());
    }

    #[test]
    fn version_2_bytes_round_trip_and_version_1_bytes_load_without_operations() {
        let operation = SavedOperation { target: 44, enemy: 0, muster: 12, melee: 3, ranged: 2, siege: 1, phase: 1, since: 40, phase_round: 52 };
        let v2 = SavedAiStates { version: 2, difficulty: "d".into(), players: vec![player(vec![operation])] };
        let read = SavedAiStates::take(&mut Reader { bytes: &bytes(&v2), at: 0, version: 0 }).unwrap();
        assert_eq!(read, v2);
        let v1 = SavedAiStates { version: 1, difficulty: "d".into(), players: vec![player(Vec::new())] };
        let v1_bytes = bytes(&v1);
        assert!(v1_bytes.len() < bytes(&v2).len());
        let read = SavedAiStates::take(&mut Reader { bytes: &v1_bytes, at: 0, version: 0 }).unwrap();
        assert_eq!(read, v1);
    }
}
