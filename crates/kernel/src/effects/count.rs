//! Per-count modifiers (`Modifier::AddYieldPerCount`, ...): the count of
//! the target is read here and the modifier becomes its plain form
//! (`AddYield`, `AddYieldPercent`, a module's), so the consumers of city
//! and player modifiers never see a count. Adding a count: one `Count`
//! variant, its scopes (`Count::fits`) and one arm here; the rules module
//! counts its own (`effects::module`).

use crate::module::RulesModule;
use crate::ids::PlayerId;
use crate::effects::module::ModuleEffects;
use crate::rules::effects::{CompiledEffect, Count, Modifier};
use crate::rules::CompiledRuleset;
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;

/// What a count is read on: a city (City scope), a player, or the city of
/// one of the module's events with the event's subject, a row of one of
/// the module's tables (Classic: the city a religion spreads into and the
/// religion, `OnReligionSpread`).
#[derive(Clone, Copy)]
pub enum Counted<'a> {
    City(&'a CityCore),
    Player(PlayerId),
    Event {
        city: &'a CityCore,
        subject: u16,
    },
}

/// The effect with a per-count modifier turned into its plain form for
/// `target` (any other effect is returned unchanged).
pub fn resolve<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    target: Counted<'_>,
    effect: &CompiledEffect<M>,
) -> CompiledEffect<M> {
    let mut counted = |what: Count<M>| count(auth, rules, target, what);
    let modifier = match effect.modifier {
        Modifier::AddYieldPerCount {
            yield_kind,
            amount,
            per,
        } => Modifier::AddYield {
            yield_kind,
            amount: per.scale(amount, counted(per.count)),
        },
        Modifier::AddYieldPercentPerCount {
            yield_kind,
            percent,
            per,
        } => Modifier::AddYieldPercent {
            yield_kind,
            percent: per.scale(percent, counted(per.count)),
        },
        Modifier::Module(modifier) => match M::Vocabulary::resolve_count(modifier, &mut counted) {
            Some(plain) => Modifier::Module(plain),
            None => return *effect,
        },
        _ => return *effect,
    };
    CompiledEffect { modifier, ..*effect }
}

/// The number of `what` on the target; 0 where the count is not defined
/// (the compiler keeps every count on its scopes).
fn count<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, target: Counted<'_>, what: Count<M>) -> u32 {
    match (what, target) {
        (Count::Population, Counted::City(core)) => core.population,
        (Count::WorldWonders, Counted::City(core)) => core
            .buildings
            .iter()
            .filter(|b| rules.buildings.get(b.0 as usize).is_some_and(|d| d.wonder))
            .count() as u32,
        (Count::ConnectedCities, Counted::Player(player)) => connected_cities(auth, rules, player),
        (Count::Module(what), target) => M::Vocabulary::count(auth, rules, target, what),
        _ => 0,
    }
}

/// Cities of the player connected to its capital, the capital not counted
/// (0 without a capital).
fn connected_cities<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, player: PlayerId) -> u32 {
    let Some(capital) = crate::world::city::capital_of(auth, rules, player) else {
        return 0;
    };
    let reached = crate::derived::connections::connected_cities(auth, rules, player, capital);
    reached.len().saturating_sub(1) as u32
}
