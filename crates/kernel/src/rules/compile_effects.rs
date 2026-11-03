//! Effect compilation (platform/project.md §19): resolve references, reject invalid
//! scope/selector/condition/trigger/duration/stacking combinations, bucket
//! the effects and build the runtime indexes. Runs after every table is
//! keyed and sorted, so ids are final. Inert effects (`rules::content`)
//! only get their references resolved.

use crate::module::RulesModule;
use crate::ids::{
    BuildingKindId, FeatureId, ImprovementId, PromotionId, ResourceId, TechId,
    UnitKindId,
};
use crate::rules::compile::RulesError;
use crate::rules::compiled::CompiledRuleset;
use crate::rules::tables::ModuleTables;
use crate::rules::content::{ContentTable, InertEffect, SourceInertEffect};
use crate::rules::effects::{
    CompiledEffect, Condition, CostTarget, Duration, EffectIndex, EffectSet, Forbidden,
    Grant, Modifier, ProductionFilter, Scope, Selector, Stacking, Trigger, TriggerSources,
    YieldKind, MAX_YIELD_KINDS,
};
use crate::rules::key_index::KeyIndex;
use crate::rules::modifiers::MAX_MULTIPLIERS;
use crate::rules::vocabulary::{
    ConditionWord, CostWord, ModuleTable, TableWord, TriggerWord,
    Vocabulary,
};
use crate::rules::source::{
    SourceCondition, SourceEffect, SourceModifier, SourceProductionFilter, SourceForbidden,
};

/// Kind of content that owns an effect (selector and trigger rules differ).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind<M: RulesModule> {
    Nation,
    Technology,
    Building,
    Unit,
    Promotion,
    /// A table of the rules module.
    Module(ModuleTable<M>),
}

/// Compile the effects of one source. `key` names the source in errors.
pub(crate) fn compile_effect_set<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    ix: &KeyIndex,
    kind: SourceKind<M>,
    key: &str,
    effects: &[SourceEffect<M>],
) -> Result<EffectSet<M>, RulesError> {
    let mut set = EffectSet::default();
    for (i, source) in effects.iter().enumerate() {
        let at = |reason: &str| format!("{key} effect #{i}: {reason}");
        let effect = compile_effect(ix, source).map_err(|e| relabel(e, &at))?;
        check_effect(rules, kind, &effect).map_err(|e| relabel(e, &at))?;
        push_into_bucket(&mut set, effect);
    }
    Ok(set)
}

/// Compile the inert effects of one source: a mechanic name and resolved
/// references (an unknown key is an error, as for typed effects).
pub fn compile_inert(
    ix: &KeyIndex,
    key: &str,
    inert: &[SourceInertEffect],
) -> Result<Vec<InertEffect>, RulesError> {
    let mut out = Vec::with_capacity(inert.len());
    for (i, source) in inert.iter().enumerate() {
        let at = |reason: &str| format!("{key} inert effect #{i}: {reason}");
        if source.needs_mechanic.trim().is_empty() || source.modifier.trim().is_empty() {
            return Err(RulesError::InvalidModifier(at(
                "needs_mechanic and modifier are required",
            )));
        }
        let mut refs = Vec::with_capacity(source.refs.len());
        for r in &source.refs {
            refs.push(ix.any(&r.0).map_err(|e| relabel(e, &at))?);
        }
        out.push(InertEffect {
            needs_mechanic: source.needs_mechanic.clone(),
            modifier: source.modifier.clone(),
            scope: source.scope.clone(),
            selector: source.selector.clone(),
            trigger: source.trigger.clone(),
            amount: source.amount,
            refs,
        });
    }
    Ok(out)
}

fn relabel(error: RulesError, at: &dyn Fn(&str) -> String) -> RulesError {
    match error {
        RulesError::UnknownReference(r) => RulesError::UnknownReference(at(&r)),
        RulesError::InvalidModifier(r) => RulesError::InvalidModifier(at(&r)),
        RulesError::InvalidSelector(r) => RulesError::InvalidSelector(at(&r)),
        RulesError::InvalidCondition(r) => RulesError::InvalidCondition(at(&r)),
        RulesError::InvalidTrigger(r) => RulesError::InvalidTrigger(at(&r)),
        RulesError::InvalidDuration(r) => RulesError::InvalidDuration(at(&r)),
        RulesError::InvalidStacking(r) => RulesError::InvalidStacking(at(&r)),
        other => other,
    }
}

fn push_into_bucket<M: RulesModule>(set: &mut EffectSet<M>, effect: CompiledEffect<M>) {
    if effect.trigger != Trigger::Passive {
        set.triggered[effect.trigger.index()].push(effect);
        return;
    }
    match effect.scope {
        Scope::Player => set.passive_player.push(effect),
        Scope::Module(_) => set.passive_module.push(effect),
        Scope::City => set.passive_city.push(effect),
        Scope::Unit => set.passive_unit.push(effect),
        Scope::Tile => set.passive_tile.push(effect),
    }
}

// ---- reference resolution ----

fn compile_effect<M: RulesModule>(ix: &KeyIndex, source: &SourceEffect<M>) -> Result<CompiledEffect<M>, RulesError> {
    Ok(CompiledEffect {
        scope: source.scope,
        selector: source.selector,
        condition: resolve_condition(ix, &source.condition)?,
        second_condition: resolve_condition(ix, &source.second_condition)?,
        trigger: source.trigger,
        modifier: resolve_modifier(ix, &source.modifier)?,
        duration: source.duration,
        stacking: source.stacking,
    })
}

fn tech(ix: &KeyIndex, key: &str) -> Result<TechId, RulesError> {
    ix.id(ContentTable::Technology, key).map(TechId)
}

fn building(ix: &KeyIndex, key: &str) -> Result<BuildingKindId, RulesError> {
    ix.id(ContentTable::Building, key).map(BuildingKindId)
}

fn unit(ix: &KeyIndex, key: &str) -> Result<UnitKindId, RulesError> {
    ix.id(ContentTable::Unit, key).map(UnitKindId)
}

fn resource(ix: &KeyIndex, key: &str) -> Result<ResourceId, RulesError> {
    ix.id(ContentTable::Resource, key).map(ResourceId)
}

fn promotion(ix: &KeyIndex, key: &str) -> Result<PromotionId, RulesError> {
    ix.id(ContentTable::Promotion, key).map(PromotionId)
}

fn resolve_condition<M: RulesModule>(ix: &KeyIndex, condition: &SourceCondition<M>) -> Result<Condition<M>, RulesError> {
    Ok(match condition {
        SourceCondition::Always => Condition::Always,
        SourceCondition::HasTechnology(k) => Condition::HasTechnology(tech(ix, &k.0)?),
        SourceCondition::LacksTechnology(k) => Condition::LacksTechnology(tech(ix, &k.0)?),
        SourceCondition::MinPopulation(n) => Condition::MinPopulation(*n),
        SourceCondition::HasBuilding(k) => Condition::HasBuilding(building(ix, &k.0)?),
        SourceCondition::TileImprovement(k) => {
            Condition::TileImprovement(ImprovementId(ix.id(ContentTable::Improvement, &k.0)?))
        }
        SourceCondition::TileResource(k) => {
            Condition::TileResource(ResourceId(ix.id(ContentTable::Resource, &k.0)?))
        }
        SourceCondition::TileFeature(k) => {
            Condition::TileFeature(FeatureId(ix.id(ContentTable::Feature, &k.0)?))
        }
        SourceCondition::TileTerrain(k) => Condition::TileTerrain(
            crate::rules::content::TerrainKindId(ix.id(ContentTable::Terrain, &k.0)?),
        ),
        SourceCondition::TileTerrainUntil(k, t) => Condition::TileTerrainUntil(
            crate::rules::content::TerrainKindId(ix.id(ContentTable::Terrain, &k.0)?),
            tech(ix, &t.0)?,
        ),
        SourceCondition::OpponentRole(role) => Condition::OpponentRole(*role),
        SourceCondition::OpponentDomain(domain) => Condition::OpponentDomain(*domain),
        SourceCondition::OpponentClassOf(k) => Condition::OpponentClassOf(unit(ix, &k.0)?),
        SourceCondition::UnitDomain(d) => Condition::UnitDomain(*d),
        SourceCondition::UnitRole(r) => Condition::UnitRole(*r),
        SourceCondition::EventNearSourceCity(n) => Condition::EventNearSourceCity(*n),
        SourceCondition::TilePlot(plot) => Condition::TilePlot(*plot),
        SourceCondition::TileResourceClass(class) => Condition::TileResourceClass(*class),
        SourceCondition::TileFeatureUnimproved(k) => {
            Condition::TileFeatureUnimproved(FeatureId(ix.id(ContentTable::Feature, &k.0)?))
        }
        SourceCondition::CityAdjacentUnimprovedFeature { feature, at_least } => {
            Condition::CityAdjacentUnimprovedFeature {
                feature: FeatureId(ix.id(ContentTable::Feature, &feature.0)?),
                at_least: *at_least,
            }
        }
        SourceCondition::Producing(filter) => Condition::Producing(resolve_production_filter(ix, filter)?),
        SourceCondition::CityGarrisoned => Condition::CityGarrisoned,
        SourceCondition::CityCoastal => Condition::CityCoastal,
        SourceCondition::LearnedTechnology(k) => Condition::LearnedTechnology(tech(ix, &k.0)?),
        SourceCondition::CityOnRiver => Condition::CityOnRiver,
        SourceCondition::AtPeace => Condition::AtPeace,
        SourceCondition::ConnectedToCapital => Condition::ConnectedToCapital,
        SourceCondition::NearOwnImprovement { improvement, range } => Condition::NearOwnImprovement {
            improvement: ImprovementId(ix.id(ContentTable::Improvement, &improvement.0)?),
            range: *range,
        },
        SourceCondition::Module(c) => Condition::Module(M::Vocabulary::resolve_condition(ix, c)?),
    })
}

fn resolve_production_filter(
    ix: &KeyIndex,
    filter: &SourceProductionFilter,
) -> Result<ProductionFilter, RulesError> {
    Ok(match filter {
        SourceProductionFilter::Units => ProductionFilter::Units,
        SourceProductionFilter::UnitDomain(d) => ProductionFilter::UnitDomain(*d),
        SourceProductionFilter::UnitRole(r) => ProductionFilter::UnitRole(*r),
        SourceProductionFilter::UnitClassOf(k) => ProductionFilter::UnitClassOf(unit(ix, &k.0)?),
        SourceProductionFilter::MilitaryUnits => ProductionFilter::MilitaryUnits,
        SourceProductionFilter::SpaceshipParts => ProductionFilter::SpaceshipParts,
        SourceProductionFilter::Buildings => ProductionFilter::Buildings,
        SourceProductionFilter::WorldWonders { before_era } => {
            let before_era = match before_era {
                Some(k) => Some(crate::rules::content::EraId(ix.id(ContentTable::Era, &k.0)?)),
                None => None,
            };
            ProductionFilter::WorldWonders { before_era }
        }
        SourceProductionFilter::BuildingClassOf(k) => {
            ProductionFilter::BuildingClassOf(building(ix, &k.0)?)
        }
        SourceProductionFilter::BuiltInCapital => ProductionFilter::BuiltInCapital,
    })
}

fn resolve_modifier<M: RulesModule>(ix: &KeyIndex, modifier: &SourceModifier<M>) -> Result<Modifier<M>, RulesError> {
    Ok(match modifier {
        SourceModifier::AddYield { yield_kind, amount } => Modifier::AddYield {
            yield_kind: *yield_kind,
            amount: *amount,
        },
        SourceModifier::AddYieldPercent {
            yield_kind,
            percent,
        } => Modifier::AddYieldPercent {
            yield_kind: *yield_kind,
            percent: *percent,
        },
        SourceModifier::MultiplyYield {
            yield_kind,
            percent,
        } => Modifier::MultiplyYield {
            yield_kind: *yield_kind,
            percent: *percent,
        },
        SourceModifier::ModifyGrowth { percent } => Modifier::ModifyGrowth { percent: *percent },
        SourceModifier::ModifyCost { target, percent } => Modifier::ModifyCost {
            target: *target,
            percent: *percent,
        },
        SourceModifier::AddYieldPerPopulation {
            yield_kind,
            hundredths,
        } => Modifier::AddYieldPerPopulation {
            yield_kind: *yield_kind,
            hundredths: *hundredths,
        },
        SourceModifier::ModifyCombat { context, percent } => Modifier::ModifyCombat {
            context: *context,
            percent: *percent,
        },
        SourceModifier::ModifyMovement { amount } => Modifier::ModifyMovement { amount: *amount },
        SourceModifier::UnlockUnit(k) => Modifier::UnlockUnit(unit(ix, &k.0)?),
        SourceModifier::UnlockBuilding(k) => Modifier::UnlockBuilding(building(ix, &k.0)?),
        SourceModifier::FreeUnit { unit: k, count } => Modifier::FreeUnit {
            unit: unit(ix, &k.0)?,
            count: *count,
        },
        SourceModifier::FreeBuilding(k) => Modifier::FreeBuilding(building(ix, &k.0)?),
        SourceModifier::AddPromotion(k) => Modifier::AddPromotion(promotion(ix, &k.0)?),
        SourceModifier::ModifyUnitStat { stat, amount } => Modifier::ModifyUnitStat {
            stat: *stat,
            amount: *amount,
        },
        SourceModifier::GrantAbility(ability) => Modifier::GrantAbility(*ability),
        SourceModifier::PostCombatPromotion(k) => {
            Modifier::PostCombatPromotion(promotion(ix, &k.0)?)
        }
        SourceModifier::PassivePromotion(k) => Modifier::PassivePromotion(promotion(ix, &k.0)?),
        SourceModifier::ModifyCityCombat { stat, amount } => Modifier::ModifyCityCombat {
            stat: *stat,
            amount: *amount,
        },
        SourceModifier::GrantExperience(amount) => Modifier::GrantExperience(*amount),
        SourceModifier::FreeBuildingInCities { building: k, count } => {
            Modifier::FreeBuildingInCities {
                building: building(ix, &k.0)?,
                count: *count,
            }
        }
        SourceModifier::AddStat { stat, amount } => Modifier::AddStat {
            stat: resolve_stat(ix, stat)?,
            amount: *amount,
        },
        SourceModifier::AddYieldPerCount {
            yield_kind,
            amount,
            per,
        } => Modifier::AddYieldPerCount {
            yield_kind: *yield_kind,
            amount: *amount,
            per: *per,
        },
        SourceModifier::AddYieldPercentPerCount {
            yield_kind,
            percent,
            per,
        } => Modifier::AddYieldPercentPerCount {
            yield_kind: *yield_kind,
            percent: *percent,
            per: *per,
        },
        SourceModifier::YieldFromKill {
            yield_kind,
            percent,
        } => Modifier::YieldFromKill {
            yield_kind: *yield_kind,
            percent: *percent,
        },
        SourceModifier::Grant(grant) => Modifier::Grant(*grant),
        SourceModifier::Forbid(what) => Modifier::Forbid(match what {
            SourceForbidden::UnitClassOf(k) => Forbidden::UnitClassOf(unit(ix, &k.0)?),
            SourceForbidden::Annexing => Forbidden::Annexing,
        }),
        SourceModifier::Module(m) => Modifier::Module(M::Vocabulary::resolve_modifier(ix, m)?),
    })
}

fn resolve_stat<M: RulesModule>(
    ix: &KeyIndex,
    stat: &crate::rules::stats::SourceStat<M>,
) -> Result<crate::rules::stats::Stat<M>, RulesError> {
    use crate::rules::stats::{SourceStat, Stat};
    Ok(match stat {
        SourceStat::FoodKept => Stat::FoodKept,
        SourceStat::ResourceCopies(k) => Stat::ResourceCopies(resource(ix, &k.0)?),
        SourceStat::ResourceQuantityPercent(k) => Stat::ResourceQuantityPercent(resource(ix, &k.0)?),
        SourceStat::StrategicQuantityPercent => Stat::StrategicQuantityPercent,
        SourceStat::ExtraLuxuryCopies => Stat::ExtraLuxuryCopies,
        SourceStat::WorkerSpeedPercent => Stat::WorkerSpeedPercent,
        SourceStat::RouteMaintenancePercent => Stat::RouteMaintenancePercent,
        SourceStat::AnySpecialistYield(y) => Stat::AnySpecialistYield(*y),
        SourceStat::Module(stat) => Stat::Module(*stat),
        SourceStat::SpecialistYield {
            specialist,
            yield_kind,
        } => Stat::SpecialistYield {
            specialist: ix
                .opt_id(ContentTable::Specialist, specialist)?
                .map(crate::rules::content::SpecialistId),
            yield_kind: *yield_kind,
        },
    })
}

// ---- validity rules ----

fn check_effect<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    kind: SourceKind<M>,
    effect: &CompiledEffect<M>,
) -> Result<(), RulesError> {
    check_selector(kind, effect)?;
    check_condition(effect.condition, kind, effect)?;
    check_second_condition(kind, effect)?;
    check_modifier(rules, effect)?;
    check_trigger(kind, effect)?;
    check_duration(effect)?;
    check_stacking(effect)?;
    M::Vocabulary::check_effect(kind, effect)
}

fn check_selector<M: RulesModule>(kind: SourceKind<M>, effect: &CompiledEffect<M>) -> Result<(), RulesError> {
    let selector = effect.selector;
    if selector.scope() != effect.scope {
        let reason = format!(
            "selector {selector:?} does not address scope {:?}",
            effect.scope
        );
        return Err(RulesError::InvalidSelector(reason));
    }
    let fits = match selector {
        Selector::SourceCity | Selector::SourceCityTiles => kind == SourceKind::Building,
        Selector::SourceUnit => matches!(kind, SourceKind::Unit | SourceKind::Promotion),
        Selector::EventCity => matches!(
            effect.trigger,
            Trigger::OnCityFounded | Trigger::OnCityConquered
        ),
        Selector::EventUnit => match effect.trigger {
            Trigger::OnUnitKilled | Trigger::OnUnitTrained => true,
            Trigger::Module(trigger) => trigger.has_event_unit(),
            _ => false,
        },
        Selector::Owner | Selector::OwnerCities | Selector::OwnerUnits => true,
        Selector::OwnerCityTiles | Selector::Capital => true,
        Selector::Module(selector) => M::Vocabulary::selector_fits(selector, kind, effect),
    };
    // A unit or promotion exists per unit: its passive effects stay on it.
    let per_unit_source = matches!(kind, SourceKind::Unit | SourceKind::Promotion);
    let passive = effect.trigger == Trigger::Passive;
    if fits && !(per_unit_source && passive && selector.is_owner_wide()) {
        return Ok(());
    }
    let reason = format!("selector {selector:?} is not available to a {kind:?} source");
    Err(RulesError::InvalidSelector(reason))
}

/// A condition (the first or the second of `effect`) on the effect's scope
/// for a source of `kind`.
fn check_condition<M: RulesModule>(condition: Condition<M>, kind: SourceKind<M>, effect: &CompiledEffect<M>) -> Result<(), RulesError> {
    let scope = effect.scope;
    let on_kill = effect.trigger == Trigger::OnUnitKilled;
    let fits = match condition {
        Condition::Always | Condition::HasTechnology(_) | Condition::LacksTechnology(_) => true,
        Condition::MinPopulation(_) => matches!(scope, Scope::City | Scope::Tile),
        Condition::HasBuilding(_) => matches!(scope, Scope::City | Scope::Tile),
        Condition::TileImprovement(_) | Condition::TileResource(_) => scope == Scope::Tile,
        Condition::TileFeature(_) | Condition::TileTerrain(_) => {
            matches!(scope, Scope::Tile | Scope::Unit)
        }
        // The opponent of a kill is its victim, whatever the scope.
        Condition::OpponentRole(_) | Condition::OpponentDomain(_) | Condition::OpponentClassOf(_) => {
            scope == Scope::Unit || on_kill
        }
        Condition::TileTerrainUntil(..) | Condition::UnitDomain(_) | Condition::UnitRole(_) => {
            scope == Scope::Unit
        }
        Condition::CityOnRiver => matches!(scope, Scope::City | Scope::Tile),
        Condition::AtPeace => matches!(scope, Scope::Player | Scope::City | Scope::Tile),
        Condition::ConnectedToCapital => scope == Scope::City,
        // A source with a city: a building's own, a module row's.
        Condition::EventNearSourceCity(_) => {
            let has_city = match kind {
                SourceKind::Building => true,
                SourceKind::Module(table) => table.has_source_city(),
                _ => false,
            };
            on_kill && has_city
        }
        Condition::NearOwnImprovement { .. } => scope == Scope::Unit,
        Condition::TilePlot(_)
        | Condition::TileResourceClass(_)
        | Condition::TileFeatureUnimproved(_) => scope == Scope::Tile,
        // A city with fewer than one such tile is every city: at least 1.
        Condition::CityAdjacentUnimprovedFeature { at_least, .. } => {
            scope == Scope::City && at_least > 0
        }
        // The production rate of the item being produced: a percent of the
        // city's production, nothing else.
        Condition::Producing(_) => {
            let production_percent = matches!(
                effect.modifier,
                Modifier::AddYieldPercent { yield_kind: YieldKind::Production, .. }
            );
            scope == Scope::City && production_percent && effect.trigger == Trigger::Passive
        }
        Condition::CityGarrisoned | Condition::CityCoastal => scope == Scope::City,
        Condition::LearnedTechnology(_) => effect.trigger == Trigger::OnTechnologyUnlocked,
        Condition::Module(condition) => M::Vocabulary::condition_fits(condition, kind, effect),
    };
    if fits {
        return Ok(());
    }
    let reason = format!("condition {condition:?} needs another scope than {scope:?}");
    Err(RulesError::InvalidCondition(reason))
}

/// The second gate: owner-level only (never Unit scope, never
/// situational), so unit and combat evaluation need not know it.
fn check_second_condition<M: RulesModule>(kind: SourceKind<M>, effect: &CompiledEffect<M>) -> Result<(), RulesError> {
    let second = effect.second_condition;
    if second == Condition::Always {
        return Ok(());
    }
    if effect.scope == Scope::Unit || second.is_situational() || second.is_kind_level() {
        let reason = format!("second condition {second:?} is not available on scope {:?}", effect.scope);
        return Err(RulesError::InvalidCondition(reason));
    }
    if effect.condition == Condition::Always {
        return Err(RulesError::InvalidCondition(
            "a second condition needs a first one".into(),
        ));
    }
    check_condition(second, kind, effect)
}

/// Which scopes each modifier may target.
fn modifier_fits_scope<M: RulesModule>(modifier: &Modifier<M>, scope: Scope<M>) -> bool {
    // Yields of a player (its pools), a city, or a tile.
    let yield_fits = |yield_kind: YieldKind<M>, tile: bool| match scope {
        Scope::Player => yield_kind.is_player_yield(),
        Scope::City => true,
        Scope::Tile => tile,
        Scope::Unit | Scope::Module(_) => false,
    };
    match modifier {
        // A Tile-scope percent scales the tile's yield (Spain: natural
        // wonder tiles); tiles have no multiplicative stage.
        Modifier::AddYield { yield_kind, .. } | Modifier::AddYieldPercent { yield_kind, .. } => {
            yield_fits(*yield_kind, true)
        }
        Modifier::MultiplyYield { yield_kind, .. } => yield_fits(*yield_kind, false),
        Modifier::ModifyCost { target, .. } => match target {
            CostTarget::Technologies | CostTarget::UnitUpgrade => scope == Scope::Player,
            CostTarget::Units
            | CostTarget::Buildings
            | CostTarget::TilePurchase
            | CostTarget::Hurry
            | CostTarget::BorderGrowth
            | CostTarget::BorderGrowthExponent
            | CostTarget::UnitPurchase
            | CostTarget::BuildingPurchase => scope == Scope::City,
            CostTarget::Module(cost) => {
                scope == if cost.is_player_cost() { Scope::Player } else { Scope::City }
            }
        },
        Modifier::ModifyGrowth { .. } => scope == Scope::City,
        Modifier::FreeUnit { .. } | Modifier::FreeBuilding(_) => scope == Scope::City,
        Modifier::AddYieldPerPopulation { .. } => scope == Scope::City,
        Modifier::ModifyCombat { .. } | Modifier::ModifyMovement { .. } => scope == Scope::Unit,
        Modifier::AddPromotion(_) => scope == Scope::Unit,
        Modifier::ModifyUnitStat { .. }
        | Modifier::GrantAbility(_)
        | Modifier::PostCombatPromotion(_)
        | Modifier::PassivePromotion(_)
        | Modifier::GrantExperience(_) => scope == Scope::Unit,
        Modifier::ModifyCityCombat { .. } => scope == Scope::City,
        Modifier::UnlockUnit(_) | Modifier::UnlockBuilding(_) => scope == Scope::Player,
        Modifier::AddStat { stat, .. } => stat.fits(scope),
        Modifier::FreeBuildingInCities { .. } => scope == Scope::Player,
        Modifier::AddYieldPerCount { yield_kind, per, .. }
        | Modifier::AddYieldPercentPerCount { yield_kind, per, .. } => {
            yield_fits(*yield_kind, false) && per.count.fits(scope)
        }
        Modifier::YieldFromKill { yield_kind, .. } => {
            scope == Scope::Player && yield_kind.has_player_stock()
        }
        Modifier::Grant(grant) => grant.scope() == scope,
        Modifier::Forbid(_) => scope == Scope::Player,
        Modifier::Module(modifier) => M::Vocabulary::modifier_fits_scope(*modifier, scope),
    }
}

/// The step of a per-count modifier of the kernel (None: not one).
pub fn per_count<M: RulesModule>(modifier: &Modifier<M>) -> Option<crate::rules::effects::PerCount<M>> {
    match *modifier {
        Modifier::AddYieldPerCount { per, .. } | Modifier::AddYieldPercentPerCount { per, .. } => {
            Some(per)
        }
        _ => None,
    }
}

fn check_modifier<M: RulesModule>(rules: &CompiledRuleset<M>, effect: &CompiledEffect<M>) -> Result<(), RulesError> {
    if !modifier_fits_scope(&effect.modifier, effect.scope) {
        let reason = format!(
            "{:?} cannot target scope {:?}",
            effect.modifier, effect.scope
        );
        return Err(RulesError::InvalidModifier(reason));
    }
    // Tile-scope terrain conditions are evaluated on the tile itself; only
    // unit effects wait for a situation. A kill brings its own opponent.
    let kill_opponent = effect.trigger == Trigger::OnUnitKilled && effect.condition.is_opponent();
    let situational =
        effect.scope == Scope::Unit && effect.condition.is_situational() && !kill_opponent;
    if situational && !situational_fits(&effect.modifier, effect.condition) {
        let reason = format!(
            "{:?} is not evaluated in a situation (battle, entered tile): {:?}",
            effect.modifier, effect.condition
        );
        return Err(RulesError::InvalidCondition(reason));
    }
    if per_count(&effect.modifier).is_some_and(|per| per.every == 0) {
        return Err(RulesError::InvalidModifier(
            "a per-count modifier needs every > 0".into(),
        ));
    }
    if let Modifier::FreeUnit { count: 0, .. } = effect.modifier {
        return Err(RulesError::InvalidModifier(
            "FreeUnit needs count > 0".into(),
        ));
    }
    let free_building = match effect.modifier {
        Modifier::FreeBuilding(b) | Modifier::FreeBuildingInCities { building: b, .. } => Some(b),
        _ => None,
    };
    if let Some(b) = free_building {
        if rules.buildings[b.0 as usize].wonder {
            return Err(RulesError::InvalidModifier(
                "a wonder cannot be free".into(),
            ));
        }
    }
    let zero_count = matches!(
        effect.modifier,
        Modifier::FreeBuildingInCities { count: 0, .. }
            | Modifier::Grant(Grant::FreeTechnologies(0) | Grant::Population(0))
    );
    if zero_count {
        return Err(RulesError::InvalidModifier(
            "a grant needs count > 0".into(),
        ));
    }
    Ok(())
}

/// Modifiers whose system evaluates situational conditions: combat
/// percents (battle tile, opponent), the terrain abilities of movement
/// (entered tile) and the healing by place (the unit's tile, beliefs).
fn situational_fits<M: RulesModule>(modifier: &Modifier<M>, condition: Condition<M>) -> bool {
    use crate::rules::effects::{UnitAbility, UnitStat};
    let near_city = matches!(condition, Condition::Module(c) if c.is_territory());
    match modifier {
        Modifier::ModifyCombat { .. } => true,
        Modifier::GrantAbility(
            UnitAbility::DoubleMove | UnitAbility::Impassable | UnitAbility::RoadInFriendlyLand,
        ) => !near_city,
        Modifier::ModifyUnitStat {
            stat: UnitStat::HealFriendlyLand | UnitStat::HealNeutralLand | UnitStat::HealEnemyLand,
            ..
        } => near_city,
        _ => false,
    }
}

fn check_trigger<M: RulesModule>(kind: SourceKind<M>, effect: &CompiledEffect<M>) -> Result<(), RulesError> {
    let trigger = effect.trigger;
    let fits = match trigger {
        Trigger::OnBuilt => matches!(kind, SourceKind::Building | SourceKind::Unit),
        // A unit or promotion does not exist before the unit is trained.
        Trigger::OnUnitTrained => !matches!(kind, SourceKind::Unit | SourceKind::Promotion),
        Trigger::Passive
        | Trigger::OnTurnStart
        | Trigger::OnTechnologyUnlocked
        | Trigger::OnCityFounded
        | Trigger::OnUnitKilled
        | Trigger::OnCityConquered
        | Trigger::OnEraEntered => true,
        Trigger::Module(trigger) => M::Vocabulary::trigger_fits(trigger, kind),
    };
    if !fits {
        let reason = format!("trigger {trigger:?} is not available to a {kind:?} source");
        return Err(RulesError::InvalidTrigger(reason));
    }
    let unlock = matches!(
        effect.modifier,
        Modifier::UnlockUnit(_) | Modifier::UnlockBuilding(_) | Modifier::Forbid(_)
    );
    if unlock && trigger != Trigger::Passive {
        return Err(RulesError::InvalidTrigger("unlocks and bans are passive".into()));
    }
    let conquest = effect.modifier == Modifier::Grant(Grant::ConquestTechnology);
    if conquest && trigger != Trigger::OnCityConquered {
        return Err(RulesError::InvalidTrigger(
            "a conquest technology needs OnCityConquered".into(),
        ));
    }
    if effect.modifier.is_instant() && trigger == Trigger::Passive {
        let reason = format!("{:?} is instant and needs a trigger", effect.modifier);
        return Err(RulesError::InvalidTrigger(reason));
    }
    let from_kill = matches!(effect.modifier, Modifier::YieldFromKill { .. });
    if from_kill && trigger != Trigger::OnUnitKilled {
        return Err(RulesError::InvalidTrigger(
            "YieldFromKill needs OnUnitKilled (it reads the victim)".into(),
        ));
    }
    Ok(())
}

fn check_duration<M: RulesModule>(effect: &CompiledEffect<M>) -> Result<(), RulesError> {
    let passive = effect.trigger == Trigger::Passive;
    let ok = match effect.duration {
        Duration::WhileSourceExists => passive,
        Duration::Once => !passive && once_fits(effect),
        Duration::Turns(n) => !passive && n > 0 && timed_fits(effect),
    };
    if ok {
        return Ok(());
    }
    let reason = format!(
        "duration {:?} does not fit trigger {:?} with {:?}",
        effect.duration, effect.trigger, effect.modifier
    );
    Err(RulesError::InvalidDuration(reason))
}

/// Once = instant modifier, or a one-shot grant of a yield that has a stock
/// (a flat amount, or an amount per counted thing of the event).
fn once_fits<M: RulesModule>(effect: &CompiledEffect<M>) -> bool {
    let target_ok = matches!(effect.scope, Scope::Player | Scope::City);
    match effect.modifier {
        Modifier::AddYield { yield_kind, amount }
        | Modifier::AddYieldPerCount {
            yield_kind, amount, ..
        } => yield_kind.has_stock() && target_ok && amount > 0,
        other => other.is_instant(),
    }
}

/// Turns(n) = a continuous modifier on a player, a city, or all units of
/// the owner (one entry for them all, `EffectTarget::Units`).
fn timed_fits<M: RulesModule>(effect: &CompiledEffect<M>) -> bool {
    let target_ok = match effect.scope {
        Scope::Player | Scope::City => true,
        Scope::Unit => effect.selector == Selector::OwnerUnits,
        Scope::Tile | Scope::Module(_) => false,
    };
    target_ok && !effect.modifier.is_instant()
}

fn check_stacking<M: RulesModule>(effect: &CompiledEffect<M>) -> Result<(), RulesError> {
    let multiplicative = matches!(effect.modifier, Modifier::MultiplyYield { .. });
    if multiplicative && effect.stacking != Stacking::Unique {
        return Err(RulesError::InvalidStacking(
            "multiplicative modifiers never compound: use Unique".into(),
        ));
    }
    if effect.duration == Duration::Once && effect.stacking == Stacking::Unique {
        return Err(RulesError::InvalidStacking(
            "a one-shot effect has nothing to deduplicate: use Stack".into(),
        ));
    }
    Ok(())
}

// ---- runtime indexes ----

/// Build the trigger and owner-wide indexes; enforce the global multiplier
/// bound (see `rules::modifiers::MAX_MULTIPLIERS`).
pub(crate) fn build_effect_index<M: RulesModule>(rules: &CompiledRuleset<M>) -> Result<EffectIndex<M>, RulesError> {
    let mut index = EffectIndex::default();
    for trigger in triggers() {
        index.by_trigger[trigger.index()] = trigger_sources(rules, trigger);
    }
    index.owner_wide_slot = vec![u16::MAX; rules.buildings.len()];
    for (i, b) in rules.buildings.iter().enumerate() {
        if b.effects.has_owner_wide_passive() {
            index.owner_wide_slot[i] = index.owner_wide_buildings.len() as u16;
            index.owner_wide_buildings.push(BuildingKindId(i as u16));
        }
    }
    let sets = all_effect_sets(rules);
    index.has_passive_city = sets.iter().any(|s| !s.passive_city.is_empty());
    index.has_passive_tile = sets.iter().any(|s| !s.passive_tile.is_empty());
    // Timed unit effects (Turns on OwnerUnits) are read with the passive ones.
    let timed_unit = |s: &&EffectSet<M>| {
        let triggered = s.triggered.iter().flatten();
        triggered.into_iter().any(|e| e.scope == Scope::Unit && matches!(e.duration, Duration::Turns(_)))
    };
    index.has_passive_unit = sets.iter().any(|s| !s.passive_unit.is_empty() || timed_unit(s));
    index.has_passive_player = sets.iter().any(|s| !s.passive_player.is_empty());
    for (i, b) in rules.buildings.iter().enumerate() {
        if crate::rules::rows::BuildingExtension::border_obstacle(&b.module) {
            index.border_obstacle_buildings.push(BuildingKindId(i as u16));
        }
    }
    index.has_garrison_condition = sets
        .iter()
        .flat_map(|s| s.passive_city.iter())
        .any(|e| e.condition == Condition::CityGarrisoned);
    index_production(rules, &sets, &mut index);
    index_conditions(&sets, &mut index);
    index.module = <M::Vocabulary as Vocabulary<M>>::index(rules);
    check_multiplier_bound(rules)?;
    Ok(index)
}

/// Whether production percents depend on the item being produced.
fn index_production<M: RulesModule>(rules: &CompiledRuleset<M>, sets: &[&EffectSet<M>], index: &mut EffectIndex<M>) {
    for set in sets {
        for effect in &set.passive_city {
            match effect.condition {
                Condition::Producing(ProductionFilter::BuiltInCapital) => {
                    index.has_production_percents = true;
                    index.has_capital_production = true;
                }
                Condition::Producing(_) => index.has_production_percents = true,
                _ => {}
            }
        }
    }
    for (i, resource) in rules.resources.iter().enumerate() {
        if resource.wonder_production.is_some() {
            index.wonder_resources.push(crate::ids::ResourceId(i as u16));
            index.has_production_percents = true;
        }
    }
}

/// Conditions whose state the turn keeps or refreshes only when some
/// effect reads them (capital connections, peace).
fn index_conditions<M: RulesModule>(sets: &[&EffectSet<M>], index: &mut EffectIndex<M>) {
    for set in sets {
        let passive = [
            &set.passive_player,
            &set.passive_module,
            &set.passive_city,
            &set.passive_unit,
            &set.passive_tile,
        ];
        let effects = passive.into_iter().chain(set.triggered.iter()).flatten();
        for effect in effects {
            for condition in [effect.condition, effect.second_condition] {
                if condition == Condition::ConnectedToCapital {
                    index.uses_capital_connection = true;
                }
                if condition == Condition::AtPeace {
                    index.uses_at_peace = true;
                }
            }
        }
    }
}

/// Every trigger but Passive.
fn triggers<M: RulesModule>() -> impl Iterator<Item = Trigger<M>> {
    Trigger::all().skip(1)
}

fn trigger_sources<M: RulesModule>(rules: &CompiledRuleset<M>, trigger: Trigger<M>) -> TriggerSources<M> {
    let has = |set: &EffectSet<M>| !set.triggered(trigger).is_empty();
    let mut sources = TriggerSources::default();
    for (i, c) in rules.nations.iter().enumerate() {
        if has(&c.effects) {
            sources.nations.push(i as u8);
        }
    }
    for (source, set) in rules.module.effect_sets() {
        if has(set) {
            sources.module.push(source);
        }
    }
    for (i, t) in rules.technologies.iter().enumerate() {
        if has(&t.effects) {
            sources.technologies.push(TechId(i as u16));
        }
    }
    for (i, b) in rules.buildings.iter().enumerate() {
        if has(&b.effects) {
            sources.buildings.push(BuildingKindId(i as u16));
        }
    }
    for (i, u) in rules.units.iter().enumerate() {
        if has(&u.effects) {
            sources.units.push(UnitKindId(i as u16));
        }
    }
    for (i, p) in rules.promotions.iter().enumerate() {
        if has(&p.effects) {
            sources.promotions.push(PromotionId(i as u16));
        }
    }
    sources
}

pub fn all_effect_sets<M: RulesModule>(rules: &CompiledRuleset<M>) -> Vec<&EffectSet<M>> {
    let mut sets: Vec<&EffectSet<M>> = Vec::new();
    sets.extend(rules.nations.iter().map(|c| &c.effects));
    sets.extend(rules.module.effect_sets().into_iter().map(|(_, set)| set));
    sets.extend(rules.technologies.iter().map(|t| &t.effects));
    sets.extend(rules.buildings.iter().map(|b| &b.effects));
    sets.extend(rules.units.iter().map(|u| &u.effects));
    sets.extend(rules.promotions.iter().map(|p| &p.effects));
    sets
}

fn check_multiplier_bound<M: RulesModule>(rules: &CompiledRuleset<M>) -> Result<(), RulesError> {
    let mut per_yield = [0usize; MAX_YIELD_KINDS];
    for set in all_effect_sets(rules) {
        let passive = [&set.passive_player, &set.passive_city];
        let effects = passive.into_iter().chain(set.triggered.iter());
        for effect in effects.flatten() {
            if let Modifier::MultiplyYield { yield_kind, .. } = effect.modifier {
                per_yield[yield_kind.index()] += 1;
            }
        }
    }
    for (i, count) in per_yield.iter().enumerate() {
        if *count > MAX_MULTIPLIERS {
            let reason =
                format!("{count} multiplicative modifiers on yield #{i} (max {MAX_MULTIPLIERS})");
            return Err(RulesError::InvalidStacking(reason));
        }
    }
    Ok(())
}

