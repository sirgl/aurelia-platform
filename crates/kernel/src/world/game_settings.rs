//! Settings of a game that rules read: the difficulty level and which
//! players the AI plays (Classic difficulty levels). A human player plays
//! with the game's level; an AI player with the AI row of the difficulty table,
//! whose `ai_*` percentages come from the game's level.
//! Set once at game creation (`GameState::configure_players`), saved in
//! the economy section and hashed.

use crate::module::RulesModule;
use crate::ids::PlayerId;
use crate::rules::rows::HandicapExtension;
use crate::rules::source::HandicapValues;
use crate::rules::CompiledRuleset;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameSettings {
    /// Content key of the game's level (empty: the rules' default level).
    pub difficulty: String,
    /// Players the AI plays, by player index.
    pub ai: Vec<bool>,
    /// Content key of the game's speed (`rules::speeds`; empty: Standard).
    /// A Standard game keeps it empty, so its save and hash are the ones of a
    /// game of before speeds.
    pub speed: String,
    /// The numbers of `speed`, read from the rules when the speed is set
    /// (`set_speed`, a load), for code that has the world and not the rules.
    pub speed_values: crate::rules::speeds::SpeedValues,
}

impl GameSettings {
    /// The numbers of the game's speed: Standard when it names none or the
    /// rules have no such speed.
    pub fn speed<M: RulesModule>(&self, rules: &CompiledRuleset<M>) -> crate::rules::speeds::SpeedValues {
        Self::values_of(rules, &self.speed)
    }

    /// The numbers of the game's speed as read when it was set (equal to
    /// `speed` of the game's rules).
    pub fn speed_values(&self) -> crate::rules::speeds::SpeedValues {
        self.speed_values
    }

    /// The numbers of the speed `key` of `rules` (Standard for an empty or unknown key).
    pub fn values_of<M: RulesModule>(rules: &CompiledRuleset<M>, key: &str) -> crate::rules::speeds::SpeedValues {
        if key.is_empty() {
            return crate::rules::speeds::SpeedValues::STANDARD;
        }
        rules.speed(key).map_or(crate::rules::speeds::SpeedValues::STANDARD, |s| s.values)
    }

    /// The key a game of `speed` keeps: empty for Standard numbers.
    pub fn speed_key<M: RulesModule>(rules: &CompiledRuleset<M>, speed: &str) -> String {
        match rules.speed(speed) {
            Some(s) if !s.values.is_standard() => s.key.clone(),
            _ => String::new(),
        }
    }

    pub fn is_ai(&self, player: PlayerId) -> bool {
        self.ai.get(player.0 as usize).copied().unwrap_or(false)
    }

    fn level<'r, M: RulesModule>(&self, rules: &'r CompiledRuleset<M>) -> Option<&'r crate::rules::compiled::HandicapDefinition<M>> {
        if self.difficulty.is_empty() {
            rules.roles.default_difficulty(rules)
        } else {
            rules.handicap(&self.difficulty)
        }
    }

    /// Index of the game's difficulty level (0 the easiest), the level every
    /// player's game-wide rules read; `None` without a handicap table.
    pub fn level_index<M: RulesModule>(&self, rules: &CompiledRuleset<M>) -> Option<u8> {
        self.level(rules).map(|h| h.index)
    }

    /// The handicap numbers a player plays with (all zero when the rules
    /// have no handicap table, as the legacy fixtures).
    pub fn handicap<M: RulesModule>(&self, rules: &CompiledRuleset<M>, player: PlayerId) -> HandicapValues<M> {
        let level = self.level(rules).map(|h| h.values).unwrap_or_else(neutral);
        if !self.is_ai(player) {
            return level;
        }
        let Some(ai) = rules.ai_handicap().map(|h| h.values) else {
            return level;
        };
        HandicapValues {
            module: HandicapExtension::for_ai(&level.module, &ai.module),
            ai_growth_percent: level.ai_growth_percent,
            ai_train_percent: level.ai_train_percent,
            ai_world_train_percent: level.ai_world_train_percent,
            ai_construct_percent: level.ai_construct_percent,
            ai_world_construct_percent: level.ai_world_construct_percent,
            ai_create_percent: level.ai_create_percent,
            ai_world_create_percent: level.ai_world_create_percent,
            ai_building_cost_percent: level.ai_building_cost_percent,
            ai_unit_cost_percent: level.ai_unit_cost_percent,
            ai_unit_supply_percent: level.ai_unit_supply_percent,
            ai_unit_upgrade_percent: level.ai_unit_upgrade_percent,
            ai_per_era_modifier: level.ai_per_era_modifier,
            ..ai
        }
    }
}

/// The numbers of content without a handicap table (the legacy fixtures):
/// no maintenance (the cost percents are 0), the AI's growth and cost
/// percentages at 100, the module's neutral numbers.
fn neutral<M: RulesModule>() -> HandicapValues<M> {
    HandicapValues {
        research_percent: 100,
        module: HandicapExtension::neutral(),
        ai_growth_percent: 100,
        ai_train_percent: 100,
        ai_world_train_percent: 100,
        ai_construct_percent: 100,
        ai_world_construct_percent: 100,
        ai_create_percent: 100,
        ai_world_create_percent: 100,
        ai_unit_supply_percent: 100,
        ai_unit_upgrade_percent: 100,
        ..HandicapValues::default()
    }
}
