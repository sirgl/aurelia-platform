//! Resolution order (platform/project.md §22, platform/structure.md §20, platform/PLATFORM.md stage 3):
//! the steps of a player's end turn and of each city inside it.
//!
//! The kernel owns the generic 4X steps ([`KernelStep`], [`KernelCityStep`],
//! [`StartStep`]); the rules module owns its mechanic steps and the order
//! of both: its [`TurnSchedule`] interleaves the two kinds in one table per
//! sequence (the Classic table and the reason for each slot: `classic::turn`).
//! [`TurnSchedule::validate`] checks a schedule against the kernel's
//! invariants; the kernel runs it on the game's module at compile time
//! (`GameState::new`). The module's turn driver executes the steps (Classic:
//! `classic_rules::systems::turn`, with the kernel steps' systems, until a later
//! stage moves those into the kernel).

use crate::ids::UnitHandle;
use crate::turn::TurnPhase;

/// A kernel step of the end-turn sequence, in the order the kernel needs
/// them (a schedule may put module steps between them, never reorder them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelStep {
    /// Units heal; reads «did not act this turn» before the moves return.
    HealUnits,
    RestoreMoves,
    /// Which cities reach the capital this turn (kept only when an effect
    /// reads the city status).
    CapitalConnections,
    /// The OnTurnStart effects fire.
    TurnStartEffects,
    /// Every city runs the city schedule; the cities' yields are summed
    /// into the [`TurnLedger`].
    Cities,
    /// The cities' gold into the treasury; a deficit is recorded.
    Gold,
    /// The cities' science, less the deficit, into the research stock.
    Science,
    /// The stock buys technologies of the queue.
    Research,
    /// Workers' builds progress.
    WorkerJobs,
    /// Timed effects count down.
    TimedEffects,
}

impl KernelStep {
    pub const ALL: [KernelStep; 10] = [
        KernelStep::HealUnits,
        KernelStep::RestoreMoves,
        KernelStep::CapitalConnections,
        KernelStep::TurnStartEffects,
        KernelStep::Cities,
        KernelStep::Gold,
        KernelStep::Science,
        KernelStep::Research,
        KernelStep::WorkerJobs,
        KernelStep::TimedEffects,
    ];

    /// The phase the step belongs to.
    pub const fn phase(self) -> TurnPhase {
        match self {
            KernelStep::HealUnits
            | KernelStep::RestoreMoves
            | KernelStep::CapitalConnections
            | KernelStep::TurnStartEffects => TurnPhase::Start,
            KernelStep::Cities
            | KernelStep::Gold
            | KernelStep::Science
            | KernelStep::Research
            | KernelStep::WorkerJobs => TurnPhase::Resolution,
            KernelStep::TimedEffects => TurnPhase::End,
        }
    }
}

/// A kernel step of one city's resolution, in kernel order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelCityStep {
    Growth,
    /// The queue, or with an empty queue the city's process.
    Production,
    Borders,
}

impl KernelCityStep {
    pub const ALL: [KernelCityStep; 3] = [
        KernelCityStep::Growth,
        KernelCityStep::Production,
        KernelCityStep::Borders,
    ];
}

/// One slot of a schedule: a kernel step or a step of the rules module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step<K, M> {
    Kernel(K),
    Module(M),
}

/// The order of a player's end turn and of each city's resolution, owned
/// by the rules module (`M`: its end-turn steps, `C`: its city steps).
#[derive(Debug, Clone, Copy)]
pub struct TurnSchedule<M: 'static, C: 'static> {
    pub end_turn: &'static [(TurnPhase, Step<KernelStep, M>)],
    pub city: &'static [Step<KernelCityStep, C>],
}

impl<M, C> TurnSchedule<M, C> {
    /// Panics unless the schedule keeps the kernel's invariants: every
    /// kernel step at most once, in kernel order and in its own phase;
    /// phases never go back and no step runs in `PlayerActions`. A module
    /// leaves out a kernel step it has no use for (Lands & Laws runs its
    /// economy once a round, not in each player's turn): that system does
    /// not run. A const fn, so a broken schedule fails the build.
    pub const fn validate(&self) {
        let mut next_kernel = 0;
        let mut phase = TurnPhase::Start as u8;
        let mut i = 0;
        while i < self.end_turn.len() {
            let (step_phase, step) = &self.end_turn[i];
            let rank = *step_phase as u8;
            assert!(rank >= phase, "turn schedule: a phase goes back");
            assert!(
                rank != TurnPhase::PlayerActions as u8,
                "turn schedule: a step runs in PlayerActions"
            );
            phase = rank;
            if let Step::Kernel(kernel) = step {
                // Kernel order is the declaration order (`KernelStep::ALL`).
                assert!(
                    *kernel as usize >= next_kernel,
                    "turn schedule: kernel steps repeated or out of order"
                );
                assert!(
                    kernel.phase() as u8 == rank,
                    "turn schedule: a kernel step outside its phase"
                );
                next_kernel = *kernel as usize + 1;
            }
            i += 1;
        }
        let mut next_city = 0;
        let mut i = 0;
        while i < self.city.len() {
            if let Step::Kernel(kernel) = &self.city[i] {
                assert!(
                    *kernel as usize >= next_city,
                    "city schedule: kernel steps repeated or out of order"
                );
                next_city = *kernel as usize + 1;
            }
            i += 1;
        }
    }
}

/// What the steps of one end turn pass on to the later ones: the kernel
/// writes it, module steps read it.
#[derive(Debug)]
pub struct TurnLedger {
    /// Timed effects that existed when the sequence started (only these
    /// are charged for this resolution).
    pub timed_before: usize,
    /// The player's units at turn start (heal and restore use one list).
    pub units: Vec<UnitHandle>,
    pub gold: i32,
    pub science: i32,
    /// Gold the treasury lacked this turn (≤ 0): taken from science.
    pub deficit: i32,
    /// Culture of all cities this turn.
    pub culture: u32,
    /// The module's yield of all cities this turn (Classic: faith).
    pub module_yield: u32,
}

/// One step at the start of a player's turn (kernel only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartStep {
    /// Fortified units gain a turn of fortification.
    Fortification,
    /// Units with a move order walk on.
    MoveOrders,
}

/// What runs when a player's turn begins (after the previous player ended
/// its turn): units with a multi-turn order move at turn start.
pub const TURN_START_SEQUENCE: [StartStep; 2] = [StartStep::Fortification, StartStep::MoveOrders];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::TurnPhase::{Resolution, Start};

    #[test]
    fn kernel_order_is_the_declaration_order() {
        for (i, step) in KernelStep::ALL.iter().enumerate() {
            assert_eq!(*step as usize, i, "{step:?}");
        }
        for (i, step) in KernelCityStep::ALL.iter().enumerate() {
            assert_eq!(*step as usize, i, "{step:?}");
        }
    }

    #[test]
    fn a_schedule_may_leave_kernel_steps_out() {
        let schedule: TurnSchedule<(), ()> = TurnSchedule {
            end_turn: &[(Start, Step::Kernel(KernelStep::HealUnits)), (Resolution, Step::Kernel(KernelStep::Research))],
            city: &[Step::Kernel(KernelCityStep::Borders)],
        };
        schedule.validate();
        TurnSchedule::<(), ()> { end_turn: &[], city: &[] }.validate();
    }

    #[test]
    #[should_panic(expected = "turn schedule: kernel steps repeated or out of order")]
    fn a_schedule_may_not_repeat_a_kernel_step() {
        let schedule: TurnSchedule<(), ()> = TurnSchedule {
            end_turn: &[(Start, Step::Kernel(KernelStep::HealUnits)), (Start, Step::Kernel(KernelStep::HealUnits))],
            city: &[],
        };
        schedule.validate();
    }

    #[test]
    #[should_panic(expected = "city schedule: kernel steps repeated or out of order")]
    fn a_city_schedule_may_not_reorder_kernel_steps() {
        let schedule: TurnSchedule<(), ()> = TurnSchedule {
            end_turn: &[],
            city: &[Step::Kernel(KernelCityStep::Borders), Step::Kernel(KernelCityStep::Growth)],
        };
        schedule.validate();
    }
}
