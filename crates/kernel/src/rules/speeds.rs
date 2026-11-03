//! Game speeds (classic/docs/classic/rules.md "Game speeds"): how much every
//! cost, length and interval of a game is scaled, and its calendar. A game
//! plays at one speed (`world::game_settings::GameSettings::speed`); a game
//! that names none, and rules without a speed table, play at
//! [`SpeedValues::STANDARD`], the numbers every rule constant is written at.
//!
//! The numbers are percents of Standard and whole rounds, so a rule scales
//! with integer arithmetic and is unchanged at Standard (`x × 100 / 100`).

use std::collections::HashSet;

use crate::rules::compile::RulesError;
use crate::rules::source::StableContentKey;

/// The most stretches a speed's calendar has. Own design: room for Unciv's
/// Speeds.json, whose longest calendars have 8.
pub const SPEED_STRETCHES: usize = 8;

/// The numbers of one game speed. Unciv: Speeds.json (each field names the
/// Unciv field it is; factors are written as percents, years as months).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpeedValues {
    /// The general pace (`modifier`): food to grow, great person points,
    /// golden age happiness, upgrade prices, quest and gift countdowns.
    pub pace_percent: u32,
    /// Production cost of units, buildings and projects (`productionCostModifier`).
    pub production_percent: u32,
    /// Gold prices that are not production bought (`goldCostModifier`).
    pub gold_percent: u32,
    /// Technology cost (`scienceCostModifier`).
    pub science_percent: u32,
    /// Policy and border culture cost (`cultureCostModifier`).
    pub culture_percent: u32,
    /// Faith cost of pantheons, prophets and faith purchases (`faithCostModifier`).
    pub faith_percent: u32,
    /// Turns a worker spends on an improvement (`improvementBuildLengthModifier`).
    pub improvement_percent: u32,
    /// Rounds between barbarian spawns (`barbarianModifier`).
    pub barbarian_percent: u32,
    /// Influence bought with gold and the tribute base (`goldGiftModifier`).
    pub gold_gift_percent: u32,
    /// Rounds × 10 between two growths of the tribute a bully takes
    /// (`cityStateTributeScalingInterval`).
    pub tribute_interval_times10: u32,
    /// Length of a golden age (`goldenAgeLengthModifier`).
    pub golden_age_percent: u32,
    /// Religious pressure a city spreads to a city in range each round, in
    /// Unciv's units (`religiousPressureAdjacentCity`).
    pub adjacent_city_pressure: u32,
    /// Rounds a peace treaty forbids war (`peaceDealDuration`).
    pub peace_rounds: u32,
    /// Rounds a timed deal lasts (`dealDuration`).
    pub deal_rounds: u32,
    /// The year of round 0 (`startYear`; negative: BC).
    pub start_year: i32,
    /// The calendar (`turns`): (the last round of the stretch, months per
    /// round), in order; unused places are (0, 0). Rounds after the last
    /// stretch keep its step. A year step is written in months (½ year = 6,
    /// 1/12 year = 1).
    pub calendar: [(u32, u32); SPEED_STRETCHES],
}

impl Default for SpeedValues {
    fn default() -> Self {
        Self::STANDARD
    }
}

impl SpeedValues {
    /// Standard speed: every percent 100, the numbers every rule constant is
    /// written at. Unciv: Speeds.json (Standard), equal; the classic content's
    /// `core:speed/standard` row holds the same numbers (a test checks it).
    pub const STANDARD: SpeedValues = SpeedValues {
        pace_percent: 100,
        production_percent: 100,
        gold_percent: 100,
        science_percent: 100,
        culture_percent: 100,
        faith_percent: 100,
        improvement_percent: 100,
        barbarian_percent: 100,
        gold_gift_percent: 100,
        tribute_interval_times10: 65,
        golden_age_percent: 100,
        adjacent_city_pressure: 6,
        peace_rounds: 10,
        deal_rounds: 30,
        start_year: -4000,
        calendar: [(75, 480), (135, 300), (160, 240), (210, 120), (270, 60), (320, 24), (440, 12), (500, 6)],
    };

    /// The stretches of the calendar in use.
    pub fn stretches(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.calendar.iter().copied().take_while(|&(until, _)| until > 0)
    }

    /// The length of a game at this speed (the last round of the calendar;
    /// Unciv `numTotalTurns`): the last round of a time victory.
    pub fn max_rounds(&self) -> u32 {
        self.stretches().last().map_or(0, |(until, _)| until)
    }

    /// Whether this is Standard (every rule plays its own constant).
    pub fn is_standard(&self) -> bool {
        *self == Self::STANDARD
    }

    /// `value` × `percent` / 100 (rounded down; `value` itself at 100).
    pub fn scale(value: u32, percent: u32) -> u32 {
        (u64::from(value) * u64::from(percent) / 100).min(u64::from(u32::MAX)) as u32
    }

    /// `value` × `percent` / 100 for a signed amount (toward zero).
    pub fn scale_i32(value: i32, percent: u32) -> i32 {
        (i64::from(value) * i64::from(percent) / 100).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    }

    /// A Standard length in rounds at this speed's pace, at least 1 for a
    /// positive length (`rounds × pace / 100`).
    pub fn pace_rounds(&self, rounds: u32) -> u32 {
        if rounds == 0 {
            return 0;
        }
        Self::scale(rounds, self.pace_percent).max(1)
    }

    /// A round of a Standard game (a gate such as "from round 40") moved to
    /// the same share of a game at this speed: `round × max_rounds / 500`.
    pub fn game_round(&self, standard_round: u32) -> u32 {
        let standard = Self::STANDARD.max_rounds();
        let here = self.max_rounds();
        if here == standard || standard == 0 {
            return standard_round;
        }
        (u64::from(standard_round) * u64::from(here) / u64::from(standard)) as u32
    }

    /// The round of a Standard game at the same share of the game as
    /// `round` of a game at this speed (`round × 500 / max_rounds`; the
    /// inverse of `game_round`): gates written in Standard rounds read it.
    pub fn standard_round(&self, round: u32) -> u32 {
        let standard = Self::STANDARD.max_rounds();
        let here = self.max_rounds();
        if here == standard || here == 0 {
            return round;
        }
        (u64::from(round) * u64::from(standard) / u64::from(here)).min(u64::from(u32::MAX)) as u32
    }
}

/// A game speed of the content.
#[derive(Debug, Clone, Default)]
pub struct SourceSpeed {
    pub id: Option<StableContentKey>,
    pub name: String,
    /// Order from the fastest (0) to the slowest.
    pub index: u8,
    pub values: SpeedValues,
}

/// A game speed, ordered by `index` (fastest first).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpeedDefinition {
    pub key: String,
    pub name: String,
    pub index: u8,
    pub values: SpeedValues,
}

/// Compile the speeds: keys and indexes unique, a calendar of at least one
/// stretch with rising rounds and positive steps, positive percents; rows
/// ordered by index.
pub(crate) fn compile_speeds(rows: &[SourceSpeed]) -> Result<Vec<SpeedDefinition>, RulesError> {
    let mut out = Vec::with_capacity(rows.len());
    let mut keys = HashSet::new();
    let mut indexes = HashSet::new();
    for row in rows {
        let Some(id) = &row.id else {
            return Err(RulesError::Inconsistent(format!("speed {} has no key", row.name)));
        };
        let bad = |why: &str| RulesError::Inconsistent(format!("{}: {why}", id.0));
        if !keys.insert(id.0.clone()) || !indexes.insert(row.index) {
            return Err(bad("speed key or index used twice"));
        }
        let v = &row.values;
        let percents = [
            v.pace_percent, v.production_percent, v.gold_percent, v.science_percent, v.culture_percent,
            v.faith_percent, v.improvement_percent, v.barbarian_percent, v.gold_gift_percent,
            v.golden_age_percent, v.tribute_interval_times10, v.peace_rounds, v.deal_rounds,
        ];
        if percents.contains(&0) {
            return Err(bad("a speed percent, interval or duration is 0"));
        }
        let stretches: Vec<_> = v.stretches().collect();
        let rising = stretches.windows(2).all(|w| w[0].0 < w[1].0);
        let used = stretches.len();
        let padded = v.calendar[used..].iter().all(|&s| s == (0, 0));
        if stretches.is_empty() || !rising || !padded || stretches.iter().any(|&(_, months)| months == 0) {
            return Err(bad("the calendar's stretches must rise and step forward"));
        }
        out.push(SpeedDefinition {
            key: id.0.clone(),
            name: row.name.clone(),
            index: row.index,
            values: row.values,
        });
    }
    out.sort_by_key(|s| s.index);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_is_neutral() {
        let s = SpeedValues::STANDARD;
        assert_eq!(s.max_rounds(), 500);
        assert!(s.is_standard());
        for x in [0, 1, 7, 99, 12345] {
            assert_eq!(SpeedValues::scale(x, s.production_percent), x);
            assert_eq!(s.game_round(x), x);
            assert_eq!(s.standard_round(x), x);
            assert_eq!(s.pace_rounds(x), x);
        }
    }
}
