//! Save form of timed effects and unit promotions (schema v20).
//!
//! City targets are saved as the city's position in iteration order (the
//! same order the cities vec is written in); handles are never persisted.

use crate::module::RulesModule;
use crate::ids::{BuildingKindId, CityHandle, PlayerId, PromotionId, TechId, UnitKindId};
use crate::persistence::decode::DecodeError;
use crate::persistence::schema::SavedEffect;
use crate::rules::effects::{Duration, SourceRef, Trigger};
use crate::rules::vocabulary::{ModuleSource, SourceWord, Word};
use crate::rules::CompiledRuleset;
use crate::world::effects::{ActiveEffect, ActiveEffectStore, EffectRef, EffectTarget};
use crate::world::state::AuthWorld;
use crate::world::unit::PromotionSet;

const TARGET_PLAYER: u8 = 0;
const TARGET_CITY: u8 = 1;
const TARGET_UNITS: u8 = 2;

pub(crate) fn encode_effects<M: RulesModule>(auth: &AuthWorld<M>) -> Vec<SavedEffect> {
    let city_order: Vec<CityHandle> = auth.cities().iter().map(|(h, _)| h).collect();
    let mut out = Vec::with_capacity(auth.active_effects().len());
    for entry in auth.active_effects().iter() {
        let (target_tag, target) = match entry.target {
            EffectTarget::Player(p) => (TARGET_PLAYER, u32::from(p.0)),
            EffectTarget::Units(p) => (TARGET_UNITS, u32::from(p.0)),
            EffectTarget::City(c) => {
                let ordinal = city_order.iter().position(|h| *h == c);
                (
                    TARGET_CITY,
                    ordinal.expect("timed effect on a live city") as u32,
                )
            }
        };
        let (source_tag, source_id) = source_parts(entry.effect.source);
        out.push(SavedEffect {
            owner: entry.owner.0,
            target_tag,
            target,
            source_tag,
            source_id,
            trigger: entry.effect.trigger.index() as u8,
            slot: entry.effect.slot,
            turns_left: entry.turns_left,
        });
    }
    out
}

/// Stable (tag, id) of a source — the save and hash form.
pub(crate) fn source_parts<M: RulesModule>(source: SourceRef<M>) -> (u8, u16) {
    match source {
        SourceRef::Nation(i) => (0, u16::from(i)),
        SourceRef::Technology(id) => (3, id.0),
        SourceRef::Building(id) => (4, id.0),
        SourceRef::Unit(id) => (5, id.0),
        SourceRef::Promotion(id) => (6, id.0),
        SourceRef::Module(source) => (source.tag(), source.row()),
    }
}

fn source_from_parts<M: RulesModule>(tag: u8, id: u16) -> Option<SourceRef<M>> {
    Some(match tag {
        0 => SourceRef::Nation(u8::try_from(id).ok()?),
        3 => SourceRef::Technology(TechId(id)),
        4 => SourceRef::Building(BuildingKindId(id)),
        5 => SourceRef::Unit(UnitKindId(id)),
        6 => SourceRef::Promotion(PromotionId(id)),
        _ => SourceRef::Module(ModuleSource::<M>::from_parts(tag, id)?),
    })
}

/// Saves know timed effects of the triggers tagged 1 to 7 (OnBuilt ..
/// OnAdopt): the triggers with timed effects when the form was fixed.
fn trigger_from_index<M: RulesModule>(index: u8) -> Option<Trigger<M>> {
    (1..=7).contains(&index).then(|| Trigger::from_tag(index)).flatten()
}

/// Untrusted data: every reference must resolve to a timed effect.
pub(crate) fn decode_effects<M: RulesModule>(
    saved: &[SavedEffect],
    rules: &CompiledRuleset<M>,
    players: usize,
    cities: &[CityHandle],
) -> Result<ActiveEffectStore<M>, DecodeError> {
    let mut entries = Vec::with_capacity(saved.len());
    for (i, s) in saved.iter().enumerate() {
        let corrupt = |what: &str| DecodeError::Corrupt(format!("effect[{i}] {what}"));
        if s.owner as usize >= players || s.turns_left == 0 {
            return Err(corrupt("owner/turns"));
        }
        let target = decode_target(s, players, cities).ok_or_else(|| corrupt("target"))?;
        let effect = decode_reference(s, rules).ok_or_else(|| corrupt("reference"))?;
        if !target_fits(target, effect, rules) {
            return Err(corrupt("target of another scope"));
        }
        entries.push(ActiveEffect {
            owner: PlayerId(s.owner),
            target,
            effect,
            turns_left: s.turns_left,
        });
    }
    Ok(ActiveEffectStore::from_entries(entries))
}

fn decode_target(s: &SavedEffect, players: usize, cities: &[CityHandle]) -> Option<EffectTarget> {
    match s.target_tag {
        TARGET_PLAYER if (s.target as usize) < players => {
            Some(EffectTarget::Player(PlayerId(s.target as u8)))
        }
        TARGET_CITY => cities
            .get(s.target as usize)
            .map(|c| EffectTarget::City(*c)),
        TARGET_UNITS if (s.target as usize) < players => {
            Some(EffectTarget::Units(PlayerId(s.target as u8)))
        }
        _ => None,
    }
}

/// The target kind matches the scope of the effect (a player entry of a
/// unit effect would reach the player's own numbers).
fn target_fits<M: RulesModule>(target: EffectTarget, effect: EffectRef<M>, rules: &CompiledRuleset<M>) -> bool {
    use crate::rules::effects::Scope;
    let Some(compiled) = effect.resolve(rules) else {
        return false;
    };
    match target {
        EffectTarget::Player(_) => compiled.scope == Scope::Player,
        EffectTarget::City(_) => compiled.scope == Scope::City,
        EffectTarget::Units(_) => compiled.scope == Scope::Unit,
    }
}


/// The reference must resolve to a timed effect of the ruleset.
fn decode_reference<M: RulesModule>(s: &SavedEffect, rules: &CompiledRuleset<M>) -> Option<EffectRef<M>> {
    let effect = EffectRef {
        source: source_from_parts(s.source_tag, s.source_id)?,
        trigger: trigger_from_index(s.trigger)?,
        slot: s.slot,
    };
    let compiled = effect.resolve(rules)?;
    match compiled.duration {
        Duration::Turns(_) => Some(effect),
        Duration::WhileSourceExists | Duration::Once => None,
    }
}

pub(crate) fn encode_promotions(set: &PromotionSet) -> Vec<u16> {
    set.iter().map(|p| p.0).collect()
}

pub(crate) fn decode_promotions<M: RulesModule>(
    saved: &[u16],
    rules: &CompiledRuleset<M>,
    unit: usize,
) -> Result<PromotionSet, DecodeError> {
    let mut set = PromotionSet::default();
    for id in saved {
        if *id as usize >= rules.promotions.len() {
            return Err(DecodeError::Corrupt(format!("unit[{unit}] promotion {id}")));
        }
        set.insert(PromotionId(*id));
    }
    Ok(set)
}
