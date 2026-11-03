//! What the AI's decisions are made of (PLAN M6, R-07 AI §3–4): the
//! deterministic budgets and settings of a game's AI, the tools a decision
//! may use, one decision, and what a unit kind is to the AI. The module's
//! planners fill decisions; the driver (`ai::driver`) runs them.

use crate::ai::map::{MapScratch, SearchLimit};
use crate::ai::trace::{Actor, AiCounters, Reason};
use crate::ids::UnitKindId;
use crate::module::RulesModule;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;

/// What the planner may spend on one decision: deterministic limits, the
/// same on every machine and at every frame rate (R-07 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecisionBudget {
    /// Nodes of one path or area search.
    pub path_nodes: u32,
    /// Candidates (sites, jobs, targets) scored by one decision.
    pub candidates: u32,
    /// Commands one unit may issue in one turn.
    pub commands_per_unit: u32,
    /// Decisions of one player turn: this base plus 4 per unit and 2 per
    /// city (the end of the turn when it runs out).
    pub decisions_base: u32,
    /// Radius of the local observation of a unit decision.
    pub local_radius: i32,
}

impl Default for DecisionBudget {
    fn default() -> Self {
        Self {
            path_nodes: 3000,
            candidates: 48,
            commands_per_unit: 6,
            decisions_base: 64,
            local_radius: 5,
        }
    }
}

impl DecisionBudget {
    pub fn search(&self) -> SearchLimit {
        SearchLimit {
            max_nodes: self.path_nodes,
        }
    }
}

/// Unit supply of the game's difficulty (rules.md): units an empire
/// keeps without a production penalty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitSupply {
    pub free_units: u32,
    pub per_city: u32,
    pub population_percent: u32,
    /// AI bonus percent on the supply (a difficulty column).
    pub ai_percent: u32,
}

impl UnitSupply {
    /// Units supplied without a penalty for an AI empire.
    pub fn supplied(&self, cities: u32, population: u32) -> u32 {
        let base =
            self.free_units + self.per_city * cities + population * self.population_percent / 100;
        base * (100 + self.ai_percent) / 100
    }
}

impl UnitSupply {
    /// Content without a difficulty table (old test fixtures): the AI row's
    /// supply at the default level. Own value, not compared with a public
    /// source (unverified).
    pub const WITHOUT_HANDICAPS: UnitSupply = UnitSupply {
        free_units: 7,
        per_city: 3,
        population_percent: 50,
        ai_percent: 20,
    };
}

/// Settings of the AI players of one game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiSettings {
    /// Content key of the difficulty (`core:handicap/<level>`).
    pub difficulty: String,
    pub supply: UnitSupply,
    pub budget: DecisionBudget,
}

impl AiSettings {
    /// The AI settings of a game at `difficulty` (a handicap key): every AI
    /// player supplies units by the AI row's
    /// supply, with the AI supply bonus of the game's level (classic/docs/classic/rules.md). Data, not code
    /// branches: a level differs only by its row.
    pub fn for_rules<M: RulesModule>(rules: &CompiledRuleset<M>, difficulty: &str) -> Self {
        let ai = rules.ai_handicap();
        let game = rules.handicap(difficulty);
        let supply = match (ai, game) {
            (Some(ai), Some(game)) => UnitSupply {
                free_units: ai.values.production_free_units.max(0) as u32,
                per_city: ai.values.production_free_units_per_city.max(0) as u32,
                population_percent: ai.values.production_free_units_population_percent.max(0)
                    as u32,
                ai_percent: game.values.ai_unit_supply_percent.max(0) as u32,
            },
            _ => UnitSupply::WITHOUT_HANDICAPS,
        };
        Self {
            difficulty: difficulty.to_owned(),
            supply,
            budget: DecisionBudget::default(),
        }
    }
}

/// Everything a decision may use besides the observation and the memory.
pub struct PlanTools<'a, M: RulesModule> {
    pub rules: &'a CompiledRuleset<M>,
    pub settings: &'a AiSettings,
    pub scratch: &'a mut MapScratch,
    pub counters: &'a mut AiCounters,
}

impl<M: RulesModule> PlanTools<'_, M> {
}

/// One decision: a command with its reason, or a reason for none.
#[derive(Debug, Clone)]
pub struct Decision<M: RulesModule> {
    pub actor: Actor,
    pub reason: Reason,
    pub command: Option<M::Command>,
}

impl<M: RulesModule> Decision<M> {
    pub fn act(actor: Actor, reason: Reason, command: M::Command) -> Self {
        Self {
            actor,
            reason,
            command: Some(command),
        }
    }

    pub fn refuse(actor: Actor, reason: Reason) -> Self {
        Self {
            actor,
            reason,
            command: None,
        }
    }
}

/// What a unit kind is to the AI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiRole {
    Settler,
    Worker,
    /// Land combat unit.
    Military,
    /// Sea or air unit: land paths do not apply (naval AI waits for M4).
    Naval,
    /// Great people, missionaries, other civilians.
    Civilian,
}

pub fn role_of<M: RulesModule>(rules: &CompiledRuleset<M>, kind: UnitKindId) -> AiRole {
    let Some(def) = rules.units.get(kind.0 as usize) else {
        return AiRole::Military;
    };
    if def.founder {
        return AiRole::Settler;
    }
    if def.builder {
        return AiRole::Worker;
    }
    if def.domain != Domain::Land {
        return AiRole::Naval;
    }
    if rules.is_civilian(kind) || def.role.is_some_and(|r| !r.is_military()) {
        return AiRole::Civilian;
    }
    AiRole::Military
}

/// Combat strength of a unit kind for AI estimates (content strength, the
/// rules' default for kinds without one).
pub fn kind_strength<M: RulesModule>(rules: &CompiledRuleset<M>, kind: UnitKindId) -> u32 {
    match rules.units.get(kind.0 as usize) {
        Some(def) if def.strength > 0 => u32::from(def.strength),
        Some(_) => 0,
        None => u32::from(crate::rules::compiled::DEFAULT_STRENGTH),
    }
}


/// Strength × health / 100: the AI's measure of a unit's fighting weight.
pub fn unit_power<M: RulesModule>(rules: &CompiledRuleset<M>, kind: UnitKindId, hp: u16) -> u32 {
    kind_strength(rules, kind) * u32::from(hp) / 100
}
