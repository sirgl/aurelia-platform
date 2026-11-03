//! The sandbox's part of the kernel's systems: the order of a turn, the
//! numbers of its formulas and its terms of the city yields.
//!
//! Food grows a city (two per citizen are eaten); production builds the
//! queue; every city makes a little culture (its borders grow one tile at a
//! time); a battered city mends at the end of its owner's turn. There is no
//! gold, science, research or purchase: those kernel steps run and find
//! nothing to do.

use kernel::command::CommandError;
use kernel::effects::city::CityModifiers;
use kernel::empty::NoWord;
use kernel::ids::{CityHandle, PlayerId, TileId};
use kernel::rules::content::Yields;
use kernel::rules::effects::YieldKind;
use kernel::systems::turn::{CityStepInput, CityTurn, EndTurnInput};
use kernel::systems::{CityGrowth, ModuleSystems};
use kernel::transaction::journal::ChangeJournal;
use kernel::transaction::WorldTransaction;
use kernel::turn::resolution::{KernelCityStep, KernelStep, Step, TurnSchedule};
use kernel::turn::TurnPhase;
use kernel::world::city::CityCore;
use kernel::world::state::AuthWorld;
use kernel::world::unit::UnitCore;
use kernel::derived::yields::YieldTerms;

use crate::combat::{strength, CITY_MAX_HP};
use crate::{Rules, Sandbox};

/// Food a citizen eats per turn.
pub const FOOD_PER_CITIZEN: i64 = 2;
/// Hit points a unit regains per turn in its own territory, and elsewhere.
pub const HEAL_HOME: i32 = 15;
pub const HEAL_AWAY: i32 = 5;
/// Damage a city mends per turn.
pub const CITY_MEND: u16 = 10;

/// The sandbox's step of a city's resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CityStep {
    /// The city mends its walls.
    Mend,
}

const END_TURN: &[(TurnPhase, Step<KernelStep, NoWord>)] = &[
    (TurnPhase::Start, Step::Kernel(KernelStep::HealUnits)),
    (TurnPhase::Start, Step::Kernel(KernelStep::RestoreMoves)),
    (TurnPhase::Start, Step::Kernel(KernelStep::CapitalConnections)),
    (TurnPhase::Start, Step::Kernel(KernelStep::TurnStartEffects)),
    (TurnPhase::Resolution, Step::Kernel(KernelStep::Cities)),
    (TurnPhase::Resolution, Step::Kernel(KernelStep::Gold)),
    (TurnPhase::Resolution, Step::Kernel(KernelStep::Science)),
    (TurnPhase::Resolution, Step::Kernel(KernelStep::Research)),
    (TurnPhase::Resolution, Step::Kernel(KernelStep::WorkerJobs)),
    (TurnPhase::End, Step::Kernel(KernelStep::TimedEffects)),
];

const CITY: &[Step<KernelCityStep, CityStep>] = &[
    Step::Kernel(KernelCityStep::Growth),
    Step::Kernel(KernelCityStep::Production),
    Step::Kernel(KernelCityStep::Borders),
    Step::Module(CityStep::Mend),
];

/// The order of an end turn and of a city's resolution.
pub const SCHEDULE: TurnSchedule<NoWord, CityStep> = TurnSchedule { end_turn: END_TURN, city: CITY };

/// The sandbox's formulas and reactions.
#[derive(Debug, Clone, Copy, Default)]
pub struct SandboxSystems;

impl ModuleSystems<Sandbox> for SandboxSystems {
    fn run_step(_: &mut WorldTransaction<'_, Sandbox>, step: NoWord, _: &EndTurnInput<'_, Sandbox>) {
        match step {}
    }

    fn run_city_step(tx: &mut WorldTransaction<'_, Sandbox>, step: CityStep, input: &CityStepInput<'_, Sandbox>) {
        match step {
            CityStep::Mend => {
                if let Some(city) = tx.world.cities.get_mut(input.city) {
                    city.damage = city.damage.saturating_sub(CITY_MEND);
                }
            }
        }
    }

    fn heal_amount(auth: &AuthWorld<Sandbox>, _: &Rules, unit: &UnitCore) -> i32 {
        let home = auth.map().owner(unit.tile) == Some(Some(unit.owner));
        if home {
            HEAL_HOME
        } else {
            HEAL_AWAY
        }
    }

    /// Food beyond what the citizens eat fills the store; the store that
    /// grows the city rises with its size.
    fn city_growth(_: &AuthWorld<Sandbox>, _: &Rules, city: &CityTurn<Sandbox>) -> CityGrowth {
        let eaten = FOOD_PER_CITIZEN * i64::from(city.population_now);
        let surplus = i64::from(city.yields.rates.food) - eaten;
        CityGrowth { difference_times100: surplus * 100, kept_percent: 0, threshold: 10 + 5 * city.population_now }
    }

    /// Culture of the next border tile.
    fn border_threshold(level: u32, _: &CityModifiers<Sandbox>) -> u32 {
        10 + 8 * level
    }

    fn attacks_left(_: &AuthWorld<Sandbox>, rules: &Rules, unit: &UnitCore) -> u8 {
        let ready = strength(rules, unit.kind.0) > 0 && unit.moves_left > 0 && unit.attacks_made == 0;
        u8::from(ready)
    }

    fn city_max_hit_points(_: &AuthWorld<Sandbox>, _: &Rules, _: &CityCore) -> u16 {
        CITY_MAX_HP
    }

    fn fortify_problem(_: &AuthWorld<Sandbox>, rules: &Rules, unit: &UnitCore) -> Option<&'static str> {
        if strength(rules, unit.kind.0) == 0 {
            return Some("civilians do not fortify");
        }
        unit.stance.is_fortified().then_some("already fortified")
    }

    fn hurry_problem(_: &AuthWorld<Sandbox>, _: &Rules, _: PlayerId, _: &CityCore) -> Result<(), CommandError> {
        Err(CommandError::RuleViolation("nothing is bought in the sandbox".into()))
    }

}

/// The sandbox's terms of the city yields: culture for the borders.
#[derive(Debug, Clone, Copy, Default)]
pub struct SandboxYields;

impl YieldTerms<Sandbox> for SandboxYields {
    type Empire = ();

    fn empire(_: &AuthWorld<Sandbox>, _: PlayerId) {}

    fn tile_base(_: &AuthWorld<Sandbox>, _: &Rules, _: &CityCore, _: TileId) -> Yields {
        Yields::default()
    }

    fn tile_final(_: &(), _: &mut Yields) {}

    /// Two culture a turn, one more for every three citizens.
    fn city_base(_: &AuthWorld<Sandbox>, _: &Rules, city: &CityCore, _: &CityModifiers<Sandbox>) -> Yields {
        let culture = 2 + city.population as i32 / 3;
        Yields { culture, ..Yields::default() }
    }

    fn channel_percent(_: &(), _: YieldKind<Sandbox>) -> i32 {
        0
    }

    fn city_after(_: &AuthWorld<Sandbox>, _: CityHandle) -> Yields {
        Yields::default()
    }

    fn stale_players(_: &ChangeJournal<Sandbox>, _: &AuthWorld<Sandbox>, _: &Rules, _: &mut Vec<PlayerId>) {}
}
