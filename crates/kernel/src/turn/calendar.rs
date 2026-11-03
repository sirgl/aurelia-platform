//! The game calendar of Classic at Standard speed (its table of
//! turns per stretch; another speed's calendar: `speed_months_at`, from the
//! speed's own table): round 0 is 4000 BC and each round advances the
//! year by the step of its stretch. The Maya Long Count (`OnLongCountCycle`)
//! reads it.
//!
//! ```text
//! rounds    years per round   reaches
//! 0–74      40                1000 BC
//! 75–134    25                500 AD
//! 135–159   20                1000 AD
//! 160–209   10                1500 AD
//! 210–269   5                 1800 AD
//! 270–319   2                 1900 AD
//! 320–439   1                 2020 AD
//! 440–      ½                 2050 AD at round 500
//! ```
//!
//! Long Count: a baktun is 144 000 days counted from 11 August 3114 BC; at
//! 365.2422 days a year a baktun is ≈ 394.25 years, so cycles end in about
//! 3902 BC, 3508 BC, …, 748 BC, 354 BC, 41 AD, 435, 829, 1224, 1618 and 2012 (Classic
//! the Maya Long Count calendar).

/// Months of each stretch of rounds: (rounds, months per round).
const STRETCHES: [(u32, i64); 7] = [
    (75, 480),
    (60, 300),
    (25, 240),
    (50, 120),
    (60, 60),
    (50, 24),
    (120, 12),
];
/// Months per round after the stretches (half a year).
const LAST_STEP_MONTHS: i64 = 6;
/// The year of round 0 (4000 BC; negative years are BC).
/// Unciv: Speeds.json (`startYear` -4000), equal.
pub const START_YEAR: i64 = -4000;

/// Months since 1 January of `START_YEAR` at the start of `round`.
pub fn months_at(round: u32) -> i64 {
    let mut left = round;
    let mut months = 0i64;
    for (rounds, step) in STRETCHES {
        let here = left.min(rounds);
        months += i64::from(here) * step;
        left -= here;
        if left == 0 {
            return months;
        }
    }
    months + i64::from(left) * LAST_STEP_MONTHS
}

/// The game year of `round` (negative: BC), rounded down.
pub fn year(round: u32) -> i64 {
    START_YEAR + months_at(round).div_euclid(12)
}

/// Months since 1 January of the speed's start year at the start of `round`
/// at a game speed: each stretch of the speed's calendar (`SpeedValues::calendar`)
/// at its step, rounds after the last at the last step. Unciv:
/// Speed.turnToYear, equal. At Standard the same as `months_at`.
pub fn speed_months_at(speed: &crate::rules::speeds::SpeedValues, round: u32) -> i64 {
    let mut months = 0i64;
    let mut start = 0u32;
    let mut step = 0i64;
    for (until, months_per_round) in speed.stretches() {
        step = i64::from(months_per_round);
        if round <= until {
            return months + i64::from(round - start) * step;
        }
        months += i64::from(until - start) * step;
        start = until;
    }
    months + i64::from(round.saturating_sub(start)) * step
}

/// The game year of `round` at a game speed (negative: BC), rounded down.
pub fn speed_year(speed: &crate::rules::speeds::SpeedValues, round: u32) -> i64 {
    i64::from(speed.start_year) + speed_months_at(speed, round).div_euclid(12)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_calendar_matches_nation_v() {
        assert_eq!(year(0), -4000);
        assert_eq!(year(75), -1000);
        assert_eq!(year(135), 500);
        assert_eq!(year(160), 1000);
        assert_eq!(year(210), 1500);
        assert_eq!(year(270), 1800);
        assert_eq!(year(320), 1900);
        assert_eq!(year(440), 2020);
        assert_eq!(year(500), 2050);
    }

    #[test]
    fn the_speed_calendar_is_the_standard_one_at_standard() {
        let standard = crate::rules::speeds::SpeedValues::STANDARD;
        for round in 0..700 {
            assert_eq!(speed_months_at(&standard, round), months_at(round), "round {round}");
            assert_eq!(speed_year(&standard, round), year(round), "round {round}");
        }
    }
}
