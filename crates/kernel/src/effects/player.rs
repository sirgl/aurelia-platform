//! Modifiers that reach one player (Player scope): empire yields, research
//! cost, unlocks, bans and the rules module's numbers.

use crate::module::RulesModule;
use crate::effects::{effect_holds, visit_owner_sources, ConditionContext};
use crate::ids::{BuildingKindId, PlayerId, UnitKindId};
use crate::effects::module::{ModuleEffects, ModulePlayerModifiers};
use crate::rules::effects::{CompiledEffect, CostTarget, EffectSet, Modifier, MAX_COST_TARGETS, MAX_YIELD_KINDS};
use crate::rules::modifiers::StageTotals;
use crate::rules::CompiledRuleset;
use crate::world::effects::EffectTarget;
use crate::world::state::AuthWorld;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlayerModifiers<M: RulesModule> {
    /// Index = `YieldKind::index()` (only the player yields are used).
    pub yields: [StageTotals; MAX_YIELD_KINDS],
    pub tech_cost: StageTotals,
    /// Gold price of unit upgrades (`unit.upgrade_cost.pct`).
    pub upgrade_cost: StageTotals,
    /// Additive percents of the other player costs, index =
    /// `CostTarget::index()` (Policies, PolicyCityPenalty).
    pub cost_percent: [i32; MAX_COST_TARGETS],
    /// Stats of the economy and society systems (`rules::stats`).
    pub stats: crate::rules::stats::StatTotals<M>,
    /// What the player may not do (`Modifier::Forbid`).
    pub forbidden: Vec<crate::rules::effects::Forbidden>,
    /// The rules module's numbers (`effects::module`).
    pub module: ModulePlayerModifiers<M>,
}

impl<M: RulesModule> PlayerModifiers<M> {
    fn apply(&mut self, modifier: &Modifier<M>, times: u32) {
        let n = times as i32;
        match *modifier {
            Modifier::AddYield { yield_kind, amount } => {
                self.yields[yield_kind.index()].add_flat(amount * n)
            }
            Modifier::AddYieldPercent {
                yield_kind,
                percent,
            } => self.yields[yield_kind.index()].add_percent(percent * n),
            Modifier::MultiplyYield {
                yield_kind,
                percent,
            } => self.yields[yield_kind.index()].add_multiplier(percent),
            Modifier::ModifyCost { target, percent } => match target {
                CostTarget::Technologies => self.tech_cost.add_percent(percent * n),
                CostTarget::UnitUpgrade => self.upgrade_cost.add_percent(percent * n),
                _ => self.cost_percent[target.index()] += percent * n,
            },
            // Unlocks are answered by `unit_unlocked` / `building_unlocked`.
            Modifier::UnlockUnit(_) | Modifier::UnlockBuilding(_) => {}
            Modifier::AddStat { stat, amount } => self.stats.add(stat, amount * n),
            Modifier::ModifyGrowth { .. }
            | Modifier::ModifyCombat { .. }
            | Modifier::ModifyMovement { .. }
            | Modifier::AddYieldPerPopulation { .. }
            | Modifier::FreeUnit { .. }
            | Modifier::FreeBuilding(_)
            | Modifier::AddPromotion(_)
            | Modifier::ModifyUnitStat { .. }
            | Modifier::GrantAbility(_)
            | Modifier::PostCombatPromotion(_)
            | Modifier::PassivePromotion(_)
            | Modifier::ModifyCityCombat { .. }
            | Modifier::GrantExperience(_) => {
                unreachable!("compiler rejects {modifier:?} for Player scope")
            }
            Modifier::Forbid(what) => self.forbidden.push(what),
            // Instant grants are applied by systems::effects, never evaluated.
            Modifier::FreeBuildingInCities { .. } | Modifier::Grant(_) | Modifier::YieldFromKill { .. } => {
                unreachable!("{modifier:?} is instant, never passive")
            }
            Modifier::AddYieldPerCount { .. } | Modifier::AddYieldPercentPerCount { .. } => {
                unreachable!("{modifier:?} is converted before it is applied")
            }
            Modifier::Module(modifier) => M::Vocabulary::apply_player(&mut self.module, modifier, times),
        }
    }
}

fn player_bucket<M: RulesModule>(set: &EffectSet<M>) -> &[CompiledEffect<M>] {
    &set.passive_player
}

/// Visit every Player-scope effect of `player` whose condition holds.
fn visit_player_effects<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    visit: &mut dyn FnMut(&CompiledEffect<M>, u32),
) {
    let at = ConditionContext {
        owner: player,
        city: None,
        tile: None,
        unit: None,
        producing: None,
    };
    let mut gated = |effect: &CompiledEffect<M>, times: u32| {
        if effect_holds(auth, rules, at, effect) {
            let effect = crate::effects::count::resolve(
                auth,
                rules,
                crate::effects::count::Counted::Player(player),
                effect,
            );
            visit(&effect, times);
        }
    };
    if rules.effect_index.has_passive_player {
        visit_owner_sources(auth, rules, player, player_bucket, &mut gated);
        M::Vocabulary::visit_player_sources(auth, rules, player, &mut gated);
    }
    for entry in auth.active_effects().iter() {
        if entry.target != EffectTarget::Player(player) {
            continue;
        }
        if let Some(effect) = entry.effect.resolve(rules) {
            gated(effect, 1);
        }
    }
}

/// Everything that reaches `player` as a Player-scope effect. Answered from
/// the world's memo while the world does not change (`effects::memo`).
pub fn player_modifiers<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
) -> std::sync::Arc<PlayerModifiers<M>> {
    auth.memo()
        .player_modifiers(player, || compute_player_modifiers(auth, rules, player))
}

/// `player_modifiers` computed from the world, never from the memo.
pub fn compute_player_modifiers<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
) -> PlayerModifiers<M> {
    let mut mods = PlayerModifiers::default();
    visit_player_effects(auth, rules, player, &mut |effect, times| {
        mods.apply(&effect.modifier, times)
    });
    mods
}

/// The player may train `kind` regardless of its required technology.
pub(crate) fn unit_unlocked<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    kind: UnitKindId,
) -> bool {
    let mut found = false;
    visit_player_effects(auth, rules, player, &mut |effect, _| {
        if effect.modifier == Modifier::UnlockUnit(kind) {
            found = true;
        }
    });
    found
}

/// The player may not do `what` (a `Forbid` effect reaches it).
pub fn forbidden<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    what: crate::rules::effects::Forbidden,
) -> bool {
    let mut found = false;
    visit_player_effects(auth, rules, player, &mut |effect, _| {
        if effect.modifier == Modifier::Forbid(what) {
            found = true;
        }
    });
    found
}

/// The player may not train or buy `kind`: a `Forbid(UnitClassOf)` names a
/// unit of its class (unique replacements of the class included).
pub(crate) fn unit_class_forbidden<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    kind: UnitKindId,
) -> bool {
    let Some(class) = rules.units.get(kind.0 as usize).and_then(|d| d.class.as_ref()) else {
        return false;
    };
    let mut found = false;
    visit_player_effects(auth, rules, player, &mut |effect, _| {
        let Modifier::Forbid(crate::rules::effects::Forbidden::UnitClassOf(named)) = effect.modifier else {
            return;
        };
        let named_class = rules.units.get(named.0 as usize).and_then(|d| d.class.as_ref());
        if named_class == Some(class) {
            found = true;
        }
    });
    found
}

/// The player may build `kind` regardless of its required technology.
pub(crate) fn building_unlocked<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    kind: BuildingKindId,
) -> bool {
    let mut found = false;
    visit_player_effects(auth, rules, player, &mut |effect, _| {
        if effect.modifier == Modifier::UnlockBuilding(kind) {
            found = true;
        }
    });
    found
}
