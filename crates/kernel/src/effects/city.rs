//! Modifiers that reach one city (City scope) and its worked tiles (Tile
//! scope). One pass over the sources collects every channel at once.

use crate::rules::tables::ModuleTables;
use crate::module::RulesModule;
use crate::effects::count::{resolve as resolve_count, Counted};
use crate::effects::module::{ModuleCityModifiers, ModuleEffects};
use crate::effects::{effect_holds, visit_owner_sources, Bucket, ConditionContext};
use crate::ids::{ProductionItem, TileId};
use crate::rules::effects::{
    CompiledEffect, Condition, CostTarget, Modifier, Scope, Selector, YieldKind, MAX_COST_TARGETS,
    MAX_YIELD_KINDS,
};
use crate::rules::modifiers::{Caps, StageTotals};
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::effects::EffectTarget;
use crate::world::state::AuthWorld;

/// Accumulated City-scope modifiers of one city.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CityModifiers<M: RulesModule> {
    /// Index = `YieldKind::index()`.
    pub yields: [StageTotals; MAX_YIELD_KINDS],
    /// Additive percent of the food surplus (ModifyGrowth).
    pub growth_percent: i32,
    /// Additive cost percents, index = `CostTarget::index()` (all but
    /// Technologies, a player cost). Costs have no flat/multiplicative stage.
    pub cost_percent: [i32; MAX_COST_TARGETS],
    /// Stats of the economy and society systems (`rules::stats`).
    pub stats: crate::rules::stats::StatTotals<M>,
    /// The AI handicap's production cost percents of the owner (None: a
    /// human).
    pub handicap_cost: Option<HandicapCost>,
    /// The game's speed (`rules::speeds`; Standard in a default value).
    pub speed: crate::rules::speeds::SpeedValues,
    /// Cities of the owner: a building's cost grows by its
    /// `cost_per_city_pct` for each (national wonders).
    pub owner_cities: u32,
    /// The part of the Production additive percent (in `yields`) that is
    /// for the item heading the queue (`Producing` effects, Marble): the AI
    /// and the HUD swap it for another item's (`item_production_percent`).
    pub item_production_percent: i32,
    /// The rules module's numbers (`effects::module`).
    pub module: ModuleCityModifiers<M>,
}

impl<M: RulesModule> CityModifiers<M> {
    /// `population` feeds the per-citizen modifiers (truncated once).
    fn apply(&mut self, modifier: &Modifier<M>, times: u32, population: u32) {
        let n = times as i32;
        match *modifier {
            Modifier::AddYield { yield_kind, .. }
            | Modifier::AddYieldPerPopulation { yield_kind, .. }
            | Modifier::AddYieldPercent { yield_kind, .. }
            | Modifier::MultiplyYield { yield_kind, .. } => apply_yield(
                &mut self.yields[yield_kind.index()],
                modifier,
                times,
                population,
            ),
            Modifier::ModifyGrowth { percent } => self.growth_percent += percent * n,
            Modifier::ModifyCost { target, percent } => {
                self.cost_percent[target.index()] += percent * n
            }
            Modifier::AddStat { stat, amount } => self.stats.add(stat, amount * n),
            // City combat stats are read by `crate::combat::city`.
            Modifier::ModifyCityCombat { .. } => {}
            Modifier::ModifyCombat { .. }
            | Modifier::ModifyMovement { .. }
            | Modifier::UnlockUnit(_)
            | Modifier::UnlockBuilding(_)
            | Modifier::FreeUnit { .. }
            | Modifier::FreeBuilding(_)
            | Modifier::AddPromotion(_)
            | Modifier::ModifyUnitStat { .. }
            | Modifier::GrantAbility(_)
            | Modifier::PostCombatPromotion(_)
            | Modifier::PassivePromotion(_)
            | Modifier::GrantExperience(_)
            | Modifier::FreeBuildingInCities { .. }
            | Modifier::Forbid(_)
            | Modifier::YieldFromKill { .. } => {
                unreachable!("compiler rejects {modifier:?} for City scope")
            }
            Modifier::Grant(_) => unreachable!("{modifier:?} is instant, never passive"),
            // Count-based amounts arrive converted (effects::count::resolve).
            Modifier::AddYieldPerCount { .. } | Modifier::AddYieldPercentPerCount { .. } => {
                unreachable!("{modifier:?} is converted before it is applied")
            }
            Modifier::Module(modifier) => M::Vocabulary::apply_city(&mut self.module, modifier, times),
        }
    }
}

/// Stage totals with only the additive percentage stage.
fn percent_stage(percent: i32) -> StageTotals {
    let mut totals = StageTotals::default();
    totals.add_percent(percent);
    totals
}

/// One yield modifier into the stage totals of its channel.
fn apply_yield<M: RulesModule>(totals: &mut StageTotals, modifier: &Modifier<M>, times: u32, population: u32) {
    let n = times as i32;
    match *modifier {
        Modifier::AddYield { amount, .. } => totals.add_flat(amount * n),
        Modifier::AddYieldPerPopulation { hundredths, .. } => {
            totals.add_flat(hundredths * n * population as i32 / 100)
        }
        Modifier::AddYieldPercent { percent, .. } => totals.add_percent(percent * n),
        Modifier::MultiplyYield { percent, .. } => totals.add_multiplier(percent),
        _ => {}
    }
}

fn city_bucket<M: RulesModule>(set: &crate::rules::effects::EffectSet<M>) -> &[CompiledEffect<M>] {
    &set.passive_city
}

fn tile_bucket<M: RulesModule>(set: &crate::rules::effects::EffectSet<M>) -> &[CompiledEffect<M>] {
    &set.passive_tile
}

/// Visit the passive effects of `bucket` reaching `core`: owner sources
/// (nation, technologies, owner-wide buildings) and the city's own
/// buildings with source-local selectors.
fn visit_city_sources<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    bucket: Bucket<M>,
    visit: &mut dyn FnMut(&CompiledEffect<M>, u32),
) {
    visit_owner_sources(auth, rules, core.owner, bucket, visit);
    M::Vocabulary::visit_city_sources(auth, rules, core, bucket, visit);
    for building in &core.buildings {
        let Some(def) = rules.buildings.get(building.0 as usize) else {
            continue;
        };
        for effect in bucket(&def.effects) {
            if !effect.selector.is_owner_wide() {
                visit(effect, 1);
            }
        }
    }
}

/// All City-scope modifiers of `core` (passive sources + timed effects).
/// Gold price to hurry the head of the city's queue now (Classic purchase):
/// the item's cost after the city's cost modifiers, the hurry formula on
/// the missing production, then the city's Hurry modifiers. None: empty
/// queue or unknown item. The one price of validation, commit, HUD and AI.
pub fn head_hurry_price<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, core: &CityCore) -> Option<u32> {
    let head = *core.queue.first()?;
    let mods = city_modifiers(auth, rules, core);
    let cost = mods.production_cost(rules, head)?;
    let (modifier, kind) = match head {
        ProductionItem::Building(b) => {
            (rules.buildings.get(b.0 as usize)?.hurry_cost_modifier, CostTarget::<M>::BuildingPurchase)
        }
        ProductionItem::Unit(u) => {
            if !<M::Systems as crate::systems::module::ModuleSystems<M>>::unit_for_sale(auth, rules, core.owner, u) {
                return None;
            }
            (0, CostTarget::UnitPurchase)
        }
        // Projects are not for sale.
        ProductionItem::Project(_) => return None,
    };
    let percents = crate::world::city::PurchasePercents {
        hurry: mods.cost_percent[CostTarget::<M>::Hurry.index()],
        kind: mods.cost_percent[kind.index()],
    };
    crate::world::city::gold_purchase_price(cost, modifier, percents)
}

pub fn city_modifiers<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, core: &CityCore) -> CityModifiers<M> {
    let mut mods = CityModifiers::default();
    let head = core.queue.first().copied();
    let at = ConditionContext {
        owner: core.owner,
        city: Some(core),
        tile: None,
        unit: None,
        producing: head,
    };
    let capital = crate::world::city::is_capital(rules, core);
    let mut visit = |effect: &CompiledEffect<M>, times: u32| {
        if effect.selector == Selector::Capital && !capital {
            return;
        }
        if effect_holds(auth, rules, at, effect) {
            let effect = resolve_count(auth, rules, Counted::City(core), effect);
            mods.apply(&effect.modifier, times, core.population);
            if let Condition::Producing(_) = effect.condition {
                mods.item_production_percent += production_percent(&effect.modifier) * times as i32;
            }
        }
    };
    if rules.effect_index.has_passive_city {
        visit_city_sources(auth, rules, core, city_bucket, &mut visit);
    }
    visit_timed_city_effects(auth, rules, core, &mut visit);
    if let Some(item) = head {
        let resources = crate::effects::production::resource_percent(auth, rules, core, item);
        mods.yields[YieldKind::<M>::Production.index()].add_percent(resources);
        mods.item_production_percent += resources;
    }
    mods.handicap_cost = handicap_cost(auth, rules, core.owner);
    mods.speed = auth.settings().speed_values();
    mods.owner_cities = auth.indexes().cities_of(core.owner).len() as u32;
    mods
}

/// The percent of a production percent modifier (0 for any other).
fn production_percent<M: RulesModule>(modifier: &Modifier<M>) -> i32 {
    match *modifier {
        Modifier::AddYieldPercent {
            yield_kind: YieldKind::Production,
            percent,
        } => percent,
        _ => 0,
    }
}

/// The additive production percent `core` would have toward `item` if it
/// headed the queue (`Producing` effects and Marble; what
/// `CityModifiers::item_production_percent` is for the real head). The AI
/// and the HUD estimate turns with it.
pub fn item_production_percent<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    item: ProductionItem,
) -> i32 {
    if !rules.effect_index.has_production_percents {
        return 0;
    }
    let at = ConditionContext {
        owner: core.owner,
        city: Some(core),
        tile: None,
        unit: None,
        producing: Some(item),
    };
    let capital = crate::world::city::is_capital(rules, core);
    let mut percent = 0;
    let mut visit = |effect: &CompiledEffect<M>, times: u32| {
        let gated = matches!(effect.condition, Condition::Producing(_));
        if !gated || (effect.selector == Selector::Capital && !capital) {
            return;
        }
        if effect_holds(auth, rules, at, effect) {
            percent += production_percent(&effect.modifier) * times as i32;
        }
    };
    if rules.effect_index.has_passive_city {
        visit_city_sources(auth, rules, core, city_bucket, &mut visit);
    }
    visit_timed_city_effects(auth, rules, core, &mut visit);
    percent + crate::effects::production::resource_percent(auth, rules, core, item)
}

/// The AI handicap's production cost percents (classic/docs/classic/rules.md),
/// each already × (100 + AIPerEraModifier × era) %.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandicapCost {
    /// AITrainPercent.
    pub units: u32,
    /// AIConstructPercent.
    pub buildings: u32,
    /// AIWorldConstructPercent.
    pub wonders: u32,
    /// AICreatePercent.
    pub projects: u32,
}

/// The production cost percents of an AI owner (rules.md); None for a
/// human: humans pay no difficulty percent on production.
fn handicap_cost<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, owner: crate::ids::PlayerId) -> Option<HandicapCost> {
    if !auth.settings().is_ai(owner) {
        return None;
    }
    let h = auth.settings().handicap(rules, owner);
    let era = i64::from(crate::world::player::player_era(auth, rules, owner));
    let era_percent = (i64::from(h.ai_per_era_modifier) * era + 100).max(0);
    let scale = |p: i32| (i64::from(p.max(0)) * era_percent / 100).max(1) as u32;
    Some(HandicapCost {
        units: scale(h.ai_train_percent),
        buildings: scale(h.ai_construct_percent),
        wonders: scale(h.ai_world_construct_percent),
        projects: scale(h.ai_create_percent),
    })
}

/// The total of one stat over the City-scope effects reaching `core`
/// (passive and timed), without the rest of the city's modifiers.
pub fn city_stat<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    stat: crate::rules::stats::Stat<M>,
) -> i32 {
    let mut total = 0;
    let at = ConditionContext {
        owner: core.owner,
        city: Some(core),
        tile: None,
        unit: None,
        producing: None,
    };
    let capital = crate::world::city::is_capital(rules, core);
    let mut visit = |effect: &CompiledEffect<M>, times: u32| {
        if effect.selector == Selector::Capital && !capital {
            return;
        }
        let Modifier::AddStat { stat: s, amount } = effect.modifier else {
            return;
        };
        if s == stat && effect_holds(auth, rules, at, effect) {
            total += amount * times as i32;
        }
    };
    if rules.effect_index.has_passive_city {
        visit_city_sources(auth, rules, core, city_bucket, &mut visit);
    }
    visit_timed_city_effects(auth, rules, core, &mut visit);
    total
}

fn visit_timed_city_effects<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    visit: &mut dyn FnMut(&CompiledEffect<M>, u32),
) {
    let store = auth.active_effects();
    if store.is_empty() {
        return;
    }
    let Some(handle) = auth.indexes().city_on_tile(core.tile) else {
        return;
    };
    for entry in store.iter() {
        if entry.target != EffectTarget::City(handle) {
            continue;
        }
        if let Some(effect) = entry.effect.resolve(rules) {
            debug_assert_eq!(effect.scope, Scope::City);
            visit(effect, 1);
        }
    }
}

/// Tile-scope effects of one city, gathered once and evaluated per tile.
#[derive(Debug, Clone, Default)]
pub struct TileEffects<M: RulesModule> {
    effects: Vec<(CompiledEffect<M>, u32)>,
}

impl<M: RulesModule> TileEffects<M> {
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// Flat additions and percents per yield kind on `tile` worked by `core`.
    pub fn bonus(
        &self,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
        core: &CityCore,
        tile: TileId,
    ) -> TileBonus {
        let mut out = TileBonus::default();
        let at = ConditionContext {
            owner: core.owner,
            city: Some(core),
            tile: Some(tile),
            unit: None,
            producing: None,
        };
        for (effect, times) in &self.effects {
            if !effect_holds(auth, rules, at, effect) {
                continue;
            }
            let n = *times as i32;
            match effect.modifier {
                Modifier::AddYield { yield_kind, amount } => out.flat[yield_kind.index()] += amount * n,
                Modifier::AddYieldPercent { yield_kind, percent } => {
                    out.percent[yield_kind.index()] += percent * n
                }
                _ => {}
            }
        }
        out
    }
}

/// What a city's Tile-scope effects add to one tile: the flat stage, then
/// the additive percent of the tile's yield after it (Spain's natural
/// wonders: the percent scales the wonder's yield,
/// religious yields included).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileBonus {
    /// Index = `YieldKind::index()`.
    pub flat: [i32; MAX_YIELD_KINDS],
    /// Index = `YieldKind::index()`.
    pub percent: [i32; MAX_YIELD_KINDS],
}

impl TileBonus {
    /// `yields` with the flat additions, then the percents (truncated).
    /// `Output`, the legacy combined channel, lands on production first.
    pub fn apply<M: RulesModule>(&self, mut yields: crate::rules::content::Yields) -> crate::rules::content::Yields {
        for kind in YieldKind::<M>::all() {
            let i = kind.index();
            let before = yields.of(kind);
            let after = percent_stage(self.percent[i]).evaluate(before + self.flat[i], Caps::NONE);
            yields.add(kind, after - before);
        }
        yields
    }
}

pub fn city_tile_effects<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    core: &CityCore,
) -> TileEffects<M> {
    let mut gathered = TileEffects::default();
    if !rules.effect_index.has_passive_tile {
        return gathered;
    }

    let mut visit = |effect: &CompiledEffect<M>, times: u32| gathered.effects.push((*effect, times));
    visit_city_sources(auth, rules, core, tile_bucket, &mut visit);
    gathered
}

impl<M: RulesModule> CityModifiers<M> {
    /// Production cost of `item` (rules.md): the content cost, plus
    /// `cost_per_city_pct` per city of the owner for a building (national
    /// wonders), × the AI handicap percent — a founder's cost is the
    /// price of a new city and takes none — then ModifyCost (at least 1: a
    /// free item would loop the production queue). Projects take the
    /// handicap and no city cost modifier. None for an unknown kind. The
    /// content cost is first scaled by the game speed (Unciv: Building.kt and
    /// BaseUnitCost.kt, `productionCostModifier`), at least 1.
    pub fn production_cost(&self, rules: &CompiledRuleset<M>, item: ProductionItem) -> Option<u32> {
        let speed = match self.speed.production_percent {
            100 => None,
            p => Some(p),
        };
        let handicap = |cost: u32, percent: fn(&HandicapCost) -> u32| {
            let cost = match speed {
                None => cost,
                Some(p) => crate::rules::speeds::SpeedValues::scale(cost, p).max(1),
            };
            match self.handicap_cost.as_ref() {
                None => cost,
                Some(h) => (u64::from(cost) * u64::from(percent(h)) / 100).max(1) as u32,
            }
        };
        let (base, target) = match item {
            ProductionItem::Unit(kind) => {
                let def = rules.units.get(kind.0 as usize)?;
                let cost = if def.founder {
                    speed.map_or(def.cost, |p| crate::rules::speeds::SpeedValues::scale(def.cost, p).max(1))
                } else {
                    handicap(def.cost, |h| h.units)
                };
                (cost, CostTarget::Units)
            }
            ProductionItem::Building(kind) => {
                let def = rules.buildings.get(kind.0 as usize)?;
                let cost = def.cost + def.cost_per_city_pct.max(0) as u32 * self.owner_cities;
                let percent: fn(&HandicapCost) -> u32 = if def.wonder { |h| h.wonders } else { |h| h.buildings };
                (handicap(cost, percent), CostTarget::Buildings)
            }
            ProductionItem::Project(id) => return Some(handicap(rules.module.project(id)?.cost, |h| h.projects).max(1)),
        };
        Some(self.adjusted_cost(target, base))
    }

    /// A cost after the city's ModifyCost of `target` (at least 1).
    pub fn adjusted_cost(&self, target: CostTarget<M>, base: u32) -> u32 {
        let base = base.min(i32::MAX as u32) as i32;
        percent_stage(self.cost_percent[target.index()]).evaluate(base, Caps::at_least(1)) as u32
    }
}
