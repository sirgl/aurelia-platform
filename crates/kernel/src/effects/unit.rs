//! Modifiers that reach one unit (Unit scope): combat percents, movement,
//! unit stats, abilities.
//!
//! Owner-level conditions (technologies) are checked when the modifiers are
//! collected; situational conditions (battle tile, opponent, entered tile)
//! stay with their term and are evaluated by the system of the situation
//! (`crate::combat`, navigation). The owner part is collected once per
//! player and reused for all of its units.

use crate::module::RulesModule;
use crate::effects::module::ModuleEffects;
use crate::effects::{condition_holds, visit_owner_sources, ConditionContext};
use crate::ids::{PlayerId, PromotionId, TileId, UnitKindId};
use crate::rules::content::Domain;
use crate::rules::compiled::{DEFAULT_STRENGTH, MOVEMENT_DEFAULT};
use crate::rules::effects::{
    CombatContext, CompiledEffect, Condition, EffectSet, Modifier, UnitAbility, UnitStat,
};
use crate::rules::modifiers::{Caps, StageTotals};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::unit::{PromotionSet, UnitCore};

/// Which side of a strike the unit is on (selects the always-on combat
/// contexts; the combat system adds the situational ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatRole {
    Attack,
    Defense,
    AttackCity,
}

/// Number of `CombatContext` variants.
pub const COMBAT_CONTEXTS: usize = 23;

/// Index of a combat context in `UnitModifiers::combat`.
pub fn combat_slot(context: CombatContext) -> usize {
    match context {
        CombatContext::Any => 0,
        CombatContext::Attack => 1,
        CombatContext::Defense => 2,
        CombatContext::VsCity => 3,
        CombatContext::AttackOpen => 4,
        CombatContext::AttackRough => 5,
        CombatContext::DefenseOpen => 6,
        CombatContext::DefenseRough => 7,
        CombatContext::RangedAttackOpen => 8,
        CombatContext::RangedAttackRough => 9,
        CombatContext::DefenseVsRanged => 10,
        CombatContext::RangedAttack => 11,
        CombatContext::VsWounded => 12,
        CombatContext::VsFortified => 13,
        CombatContext::FriendlyLand => 14,
        CombatContext::OutsideFriendlyLand => 15,
        CombatContext::AttackFriendlyLand => 16,
        CombatContext::Flanking => 17,
        CombatContext::AdjacentFriendly => 18,
        CombatContext::NearCapital => 19,
        CombatContext::EmbarkedDefense => 20,
        CombatContext::Interception => 21,
        CombatContext::AirSweep => 22,
    }
}

/// Capacity of per-stat arrays: the kernel's 25 unit stats and room for the
/// module's (`Vocabulary::UNIT_STATS`).
pub const MAX_UNIT_STATS: usize = 40;

/// The slot of a stat in per-stat arrays: its tag.
pub(crate) fn stat_slot<M: RulesModule>(stat: UnitStat<M>) -> usize {
    usize::from(stat.tag())
}

/// Bit of an ability in `AbilitySet`.
fn ability_bit<M: RulesModule>(ability: UnitAbility<M>) -> u64 {
    1u64 << ability.tag()
}

/// Abilities of a unit (the unconditional ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AbilitySet(u64);

impl AbilitySet {
    pub fn has<M: RulesModule>(&self, ability: UnitAbility<M>) -> bool {
        self.0 & ability_bit(ability) != 0
    }

    fn insert<M: RulesModule>(&mut self, ability: UnitAbility<M>) {
        self.0 |= ability_bit(ability);
    }
}

/// A combat percent that holds only in a situation (an opponent, a battle
/// tile); the combat system checks `condition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatTerm<M: RulesModule> {
    pub context: CombatContext,
    pub condition: Condition<M>,
    pub percent: i32,
}

/// A unit stat that holds only in a situation (the unit's tile: the
/// healing of beliefs); the system of the stat checks `condition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatTerm<M: RulesModule> {
    pub stat: UnitStat<M>,
    pub condition: Condition<M>,
    pub amount: i32,
}

/// A movement ability tied to the entered tile (double move, impassable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainRule<M: RulesModule> {
    pub ability: UnitAbility<M>,
    pub condition: Condition<M>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitModifiers<M: RulesModule> {
    /// Unconditional combat percent per context, index = `combat_slot`.
    pub combat: [i32; COMBAT_CONTEXTS],
    /// Combat percents with a situational condition.
    pub situational: Vec<CombatTerm<M>>,
    pub movement: StageTotals,
    /// Flat unit stats, index = `stat_slot`.
    pub stats: [i32; MAX_UNIT_STATS],
    /// Unit stats with a situational condition.
    pub situational_stats: Vec<StatTerm<M>>,
    pub abilities: AbilitySet,
    /// Terrain-dependent movement abilities.
    pub terrain_rules: Vec<TerrainRule<M>>,
    /// Candidates of the random promotion after the first combat.
    pub post_combat: Vec<PromotionId>,
    /// Promotions granted by owner sources while they exist (policies,
    /// wonders); expanded per unit when its kind is known.
    pub granted: Vec<PromotionId>,
    /// Owner effects with a kind-level condition (unit domain or role),
    /// applied per unit when its kind is known.
    pub deferred: Vec<(CompiledEffect<M>, u32)>,
}

// By hand: arrays longer than 32 have no derived Default.
impl<M: RulesModule> Default for UnitModifiers<M> {
    fn default() -> Self {
        Self {
            combat: [0; COMBAT_CONTEXTS],
            situational: Vec::new(),
            movement: StageTotals::default(),
            stats: [0; MAX_UNIT_STATS],
            situational_stats: Vec::new(),
            abilities: AbilitySet::default(),
            terrain_rules: Vec::new(),
            post_combat: Vec::new(),
            granted: Vec::new(),
            deferred: Vec::new(),
        }
    }
}

impl<M: RulesModule> UnitModifiers<M> {
    fn apply(&mut self, effect: &CompiledEffect<M>, times: u32) {
        let n = times as i32;
        let situational = effect.condition.is_situational();
        match effect.modifier {
            Modifier::ModifyCombat { context, percent } if situational => {
                self.situational.push(CombatTerm {
                    context,
                    condition: effect.condition,
                    percent: percent * n,
                });
            }
            Modifier::ModifyCombat { context, percent } => {
                self.combat[combat_slot(context)] += percent * n
            }
            Modifier::ModifyMovement { amount } => self.movement.add_flat(i32::from(amount) * n),
            Modifier::ModifyUnitStat { stat, amount } if situational => {
                self.situational_stats.push(StatTerm {
                    stat,
                    condition: effect.condition,
                    amount: amount * n,
                });
            }
            Modifier::ModifyUnitStat { stat, amount } => self.stats[stat_slot(stat)] += amount * n,
            Modifier::GrantAbility(ability) if situational => {
                self.terrain_rules.push(TerrainRule {
                    ability,
                    condition: effect.condition,
                });
            }
            Modifier::GrantAbility(ability) => self.abilities.insert(ability),
            Modifier::PostCombatPromotion(promotion) => self.post_combat.push(promotion),
            Modifier::PassivePromotion(promotion) => self.granted.push(promotion),
            Modifier::AddYield { .. }
            | Modifier::AddYieldPercent { .. }
            | Modifier::MultiplyYield { .. }
            | Modifier::ModifyGrowth { .. }
            | Modifier::ModifyCost { .. }
            | Modifier::UnlockUnit(_)
            | Modifier::UnlockBuilding(_)
            | Modifier::AddYieldPerPopulation { .. }
            | Modifier::FreeUnit { .. }
            | Modifier::FreeBuilding(_)
            | Modifier::AddPromotion(_)
            | Modifier::AddStat { .. }
            | Modifier::ModifyCityCombat { .. }
            | Modifier::GrantExperience(_)
            | Modifier::FreeBuildingInCities { .. }
            | Modifier::AddYieldPerCount { .. }
            | Modifier::AddYieldPercentPerCount { .. }
            | Modifier::YieldFromKill { .. }
            | Modifier::Grant(_)
            | Modifier::Forbid(_)
            | Modifier::Module(_) => {
                unreachable!("compiler rejects {:?} for Unit scope", effect.modifier)
            }
        }
    }

    /// Unconditional percent of one context.
    pub fn percent(&self, context: CombatContext) -> i32 {
        self.combat[combat_slot(context)]
    }

    pub fn stat(&self, stat: UnitStat<M>) -> i32 {
        self.stats[stat_slot(stat)]
    }

    /// The stat in a situation: the unconditional amount plus the
    /// situational terms whose condition `holds`.
    pub fn stat_where(&self, stat: UnitStat<M>, holds: impl Fn(Condition<M>) -> bool) -> i32 {
        let mut total = self.stat(stat);
        for term in &self.situational_stats {
            if term.stat == stat && holds(term.condition) {
                total += term.amount;
            }
        }
        total
    }

    pub fn has(&self, ability: UnitAbility<M>) -> bool {
        self.abilities.has(ability)
    }

    /// Strength of `kind` in `role` without a situation: Any plus the
    /// always-on contexts of the role (the combat system adds terrain,
    /// opponent, flanking and the rest).
    pub fn strength(&self, rules: &CompiledRuleset<M>, kind: UnitKindId, role: CombatRole) -> i32 {
        let base = match rules.units.get(kind.0 as usize) {
            Some(def) => i32::from(def.strength),
            None => i32::from(DEFAULT_STRENGTH),
        };
        let mut totals = StageTotals::default();
        totals.add_percent(self.combat_percent(role));
        totals.evaluate(base, Caps::at_least(0))
    }

    fn combat_percent(&self, role: CombatRole) -> i32 {
        let any = self.percent(CombatContext::Any);
        let attack = self.percent(CombatContext::Attack);
        match role {
            CombatRole::Attack => any + attack,
            CombatRole::Defense => any + self.percent(CombatContext::Defense),
            CombatRole::AttackCity => any + attack + self.percent(CombatContext::VsCity),
        }
    }

    /// Move points per turn of `kind` in its own domain: whole movement
    /// after the movement modifiers (ships add their extra moves at sea),
    /// times MOVE_DENOMINATOR (the one conversion point).
    pub fn movement(&self, rules: &CompiledRuleset<M>, kind: UnitKindId) -> u16 {
        let def = rules.units.get(kind.0 as usize);
        let base = def.map_or(MOVEMENT_DEFAULT, |d| d.movement);
        let mut whole = self.movement.evaluate(i32::from(base), Caps::at_least(0));
        if def.is_some_and(|d| d.domain == Domain::Sea) {
            whole = (whole + self.stat(UnitStat::NavalMoves)).max(0);
        }
        to_points(whole)
    }

    /// Move points per turn of a land unit while embarked (classic/docs/classic/rules.md): `EMBARKED_UNIT_MOVEMENT` plus its extra embarked and sea moves;
    /// its own movement modifiers do not apply on water.
    pub fn embarked_movement(&self) -> u16 {
        let whole = EMBARKED_UNIT_MOVEMENT + self.stat(UnitStat::EmbarkedMoves) + self.stat(UnitStat::NavalMoves);
        to_points(whole.max(1))
    }

    /// Is a unit of `kind` with these modifiers embarked on `tile`: a land
    /// unit on water that does not hover?
    pub fn embarked_on(&self, rules: &CompiledRuleset<M>, map: &crate::world::map::MapState, kind: UnitKindId, tile: TileId) -> bool {
        let land = rules.units.get(kind.0 as usize).is_some_and(|d| d.domain == Domain::Land);
        land && !self.has(UnitAbility::Hovering) && crate::navigation::terrain::is_water(map, tile)
    }

    /// Move points per turn of a unit of `kind` standing on `tile`: the
    /// embarked allowance on water, else its own.
    pub fn moves_on(&self, rules: &CompiledRuleset<M>, map: &crate::world::map::MapState, kind: UnitKindId, tile: TileId) -> u16 {
        if self.embarked_on(rules, map, kind, tile) {
            self.embarked_movement()
        } else {
            self.movement(rules, kind)
        }
    }
}

/// Whole moves of an embarked unit before its extra embarked moves (Classic
/// Classic: 2).
pub const EMBARKED_UNIT_MOVEMENT: i32 = 2;

/// Whole movement points → move points (MOVE_DENOMINATOR each).
fn to_points(whole: i32) -> u16 {
    let points = whole.saturating_mul(i32::from(crate::world::unit::MOVE_DENOMINATOR));
    points.clamp(0, i32::from(u16::MAX)) as u16
}

fn unit_bucket<M: RulesModule>(set: &EffectSet<M>) -> &[CompiledEffect<M>] {
    &set.passive_unit
}

/// Owner-level part: nation, technologies, owner-wide buildings.
pub fn owner_unit_modifiers<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
) -> UnitModifiers<M> {
    let mut mods = UnitModifiers::default();
    if !rules.effect_index.has_passive_unit {
        return mods;
    }
    let at = ConditionContext {
        owner,
        city: None,
        tile: None,
        unit: None,
        producing: None,
    };
    let mut visit = |effect: &CompiledEffect<M>, times: u32| {
        if effect.condition.is_kind_level() {
            mods.deferred.push((*effect, times));
        } else if effect.condition.is_situational() || condition_holds(auth, rules, at, effect.condition) {
            mods.apply(effect, times);
        }
    };
    visit_owner_sources(auth, rules, owner, unit_bucket, &mut visit);
    M::Vocabulary::visit_unit_sources(auth, rules, owner, &mut visit);
    visit_timed_unit_effects(auth, rules, owner, &mut visit);
    mods
}

/// Timed effects on all of `owner`'s units (Clausewitz's Legacy: +25 %
/// attack for 50 turns after adoption).
fn visit_timed_unit_effects<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    visit: &mut dyn FnMut(&CompiledEffect<M>, u32),
) {
    for entry in auth.active_effects().iter() {
        if entry.target != crate::world::effects::EffectTarget::Units(owner) {
            continue;
        }
        if let Some(effect) = entry.effect.resolve(rules) {
            visit(effect, 1);
        }
    }
}

/// A stat of a unit of `kind` from its owner's sources only
/// (`owner_part` from `owner_unit_modifiers`), kind-level conditions
/// (domain, role) resolved; the kind's own effects and promotions are not
/// read. For stats that only policies and nation traits change
/// (gold maintenance).
pub fn owner_stat_for_kind<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner_part: &UnitModifiers<M>,
    owner: PlayerId,
    kind: UnitKindId,
    stat: UnitStat<M>,
) -> i32 {
    if owner_part.deferred.is_empty() {
        return owner_part.stat(stat);
    }
    let at = ConditionContext {
        owner,
        city: None,
        tile: None,
        unit: Some(crate::effects::UnitTraits::of(rules, kind)),
        producing: None,
    };
    let mut mods = owner_part.clone();
    let deferred = std::mem::take(&mut mods.deferred);
    for (effect, times) in &deferred {
        if condition_holds(auth, rules, at, effect.condition) {
            mods.apply(effect, *times);
        }
    }
    mods.stat(stat)
}

/// Add the unit's own sources (its kind and its promotions).
pub fn with_own_sources<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner_part: UnitModifiers<M>,
    unit: &UnitCore,
) -> UnitModifiers<M> {
    with_kind_and_promotions(
        auth,
        rules,
        owner_part,
        unit.owner,
        unit.kind,
        unit.promotions,
    )
}

fn with_kind_and_promotions<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner_part: UnitModifiers<M>,
    owner: PlayerId,
    kind: UnitKindId,
    promotions: PromotionSet,
) -> UnitModifiers<M> {
    let mut mods = owner_part;
    if !rules.effect_index.has_passive_unit {
        return mods;
    }
    let at = ConditionContext {
        owner,
        city: None,
        tile: None,
        unit: Some(crate::effects::UnitTraits::of(rules, kind)),
        producing: None,
    };
    let deferred = std::mem::take(&mut mods.deferred);
    for (effect, times) in &deferred {
        if condition_holds(auth, rules, at, effect.condition) {
            mods.apply(effect, *times);
        }
    }
    let mut visit = |effects: &[CompiledEffect<M>]| {
        for effect in effects {
            if effect.condition.is_situational() || condition_holds(auth, rules, at, effect.condition) {
                mods.apply(effect, 1);
            }
        }
    };
    if let Some(def) = rules.units.get(kind.0 as usize) {
        visit(&def.effects.passive_unit);
    }
    for promotion in promotions.iter() {
        if let Some(def) = rules.promotions.get(promotion.0 as usize) {
            visit(&def.effects.passive_unit);
        }
    }
    let granted = std::mem::take(&mut mods.granted);
    for promotion in &granted {
        if promotions.contains(*promotion) || !promotion_fits(rules, *promotion, kind) {
            continue;
        }
        if let Some(def) = rules.promotions.get(promotion.0 as usize) {
            for effect in &def.effects.passive_unit {
                if effect.condition.is_situational() || condition_holds(auth, rules, at, effect.condition) {
                    mods.apply(effect, 1);
                }
            }
        }
    }
    mods.granted = granted;
    mods
}

/// May a unit of `kind` have the promotion (its combat role is among the
/// promotion's roles, or it is one of the promotion's civilian units)?
/// Promotions without roles fit every unit.
pub fn promotion_fits<M: RulesModule>(rules: &CompiledRuleset<M>, promotion: PromotionId, kind: UnitKindId) -> bool {
    let Some(def) = rules.promotions.get(promotion.0 as usize) else {
        return false;
    };
    if def.unit_roles.is_empty() && def.civilian_units.is_empty() {
        return true;
    }
    if def.civilian_units.contains(&kind) {
        return true;
    }
    let role = rules.units.get(kind.0 as usize).and_then(|u| u.role);
    role.is_some_and(|r| def.unit_roles.contains(&r))
}

/// Does the unit's kind or one of its own promotions grant `ability`?
/// (Owner sources never grant the leader and aura abilities; this cheap
/// check skips the owner part.)
pub fn own_ability<M: RulesModule>(rules: &CompiledRuleset<M>, unit: &UnitCore, ability: UnitAbility<M>) -> bool {
    let grants = |effects: &[CompiledEffect<M>]| {
        effects
            .iter()
            .any(|e| e.modifier == Modifier::GrantAbility(ability) && e.condition == Condition::Always)
    };
    if rules.units.get(unit.kind.0 as usize).is_some_and(|d| grants(&d.effects.passive_unit)) {
        return true;
    }
    unit.promotions.iter().any(|p| {
        rules
            .promotions
            .get(p.0 as usize)
            .is_some_and(|d| grants(&d.effects.passive_unit))
    })
}

/// Does any unit kind or promotion of the rules have a passive Unit-scope
/// effect `matches`? A cheap guard for systems that look for a rare effect
/// on every unit of a player.
pub fn any_unit_effect<M: RulesModule>(rules: &CompiledRuleset<M>, matches: impl Fn(&CompiledEffect<M>) -> bool) -> bool {
    let units = rules.units.iter().map(|d| &d.effects.passive_unit);
    let promotions = rules.promotions.iter().map(|d| &d.effects.passive_unit);
    units.chain(promotions).any(|effects| effects.iter().any(&matches))
}

/// Σ of a stat from the unit's kind and own promotions (auras, stacked
/// general bonus — never owner-level).
pub fn own_stat<M: RulesModule>(rules: &CompiledRuleset<M>, unit: &UnitCore, stat: UnitStat<M>) -> i32 {
    let sum = |effects: &[CompiledEffect<M>]| -> i32 {
        effects
            .iter()
            .filter_map(|e| match e.modifier {
                Modifier::ModifyUnitStat { stat: s, amount } if s == stat => Some(amount),
                _ => None,
            })
            .sum()
    };
    let mut total = rules
        .units
        .get(unit.kind.0 as usize)
        .map(|d| sum(&d.effects.passive_unit))
        .unwrap_or(0);
    for p in unit.promotions.iter() {
        if let Some(def) = rules.promotions.get(p.0 as usize) {
            total += sum(&def.effects.passive_unit);
        }
    }
    total
}

/// All Unit-scope modifiers of one unit, the module's combat percent
/// (`ModuleEffects::unit_combat_percent`) included.
pub fn unit_modifiers<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, unit: &UnitCore) -> UnitModifiers<M> {
    let owner_part = owner_unit_modifiers(auth, rules, unit.owner);
    let mut mods = with_own_sources(auth, rules, owner_part, unit);
    mods.combat[combat_slot(CombatContext::Any)] += M::Vocabulary::unit_combat_percent(auth, rules, unit);
    mods
}

/// Movement points of a unit of `kind` that `owner` is about to create
/// (its free promotions included).
pub(crate) fn new_unit_movement<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    kind: UnitKindId,
) -> u16 {
    let owner_part = owner_unit_modifiers(auth, rules, owner);
    let mods = with_kind_and_promotions(
        auth,
        rules,
        owner_part,
        owner,
        kind,
        free_promotions(rules, kind),
    );
    mods.movement(rules, kind)
}

/// The promotions a new unit of `kind` starts with (its content free
/// promotions).
pub fn free_promotions<M: RulesModule>(rules: &CompiledRuleset<M>, kind: UnitKindId) -> PromotionSet {
    let mut set = PromotionSet::default();
    if let Some(def) = rules.units.get(kind.0 as usize) {
        for promotion in &def.free_promotions {
            set.insert(*promotion);
        }
    }
    set
}
