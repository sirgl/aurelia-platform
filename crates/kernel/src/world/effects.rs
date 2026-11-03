//! Timed effects (Duration::Turns): authoritative state — persisted, hashed.
//!
//! A triggered effect with `Turns(n)` becomes one entry per selected target.
//! The entry references its compiled effect by (source, trigger, slot), so
//! the store holds no copy of rules data.
//!
//! Duration counts resolutions: `Turns(n)` takes part in the next `n`
//! end-turn resolutions of its owner. The end-turn sequence remembers how
//! many entries existed when it started and ticks only those at its end, so
//! an effect started inside a resolution is not charged for it.

use crate::module::RulesModule;
use crate::ids::{CityHandle, PlayerId};
use crate::rules::effects::{CompiledEffect, SourceRef, Trigger};
use crate::rules::CompiledRuleset;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectTarget {
    Player(PlayerId),
    City(CityHandle),
    /// Every unit of the player, present and future (a timed Unit-scope
    /// effect with `OwnerUnits`: Clausewitz's Legacy).
    Units(PlayerId),
}

/// Reference to one triggered effect of a content source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectRef<M: RulesModule> {
    pub source: SourceRef<M>,
    pub trigger: Trigger<M>,
    /// Position inside the source's bucket of this trigger.
    pub slot: u16,
}

impl<M: RulesModule> EffectRef<M> {
    /// None only for a reference that does not match the ruleset (decode
    /// validates every entry, so live entries always resolve).
    pub fn resolve(self, rules: &CompiledRuleset<M>) -> Option<&CompiledEffect<M>> {
        let set = rules.effect_set(self.source)?;
        set.triggered(self.trigger).get(self.slot as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActiveEffect<M: RulesModule> {
    /// Player whose end-turn sequence ticks the entry.
    pub owner: PlayerId,
    pub target: EffectTarget,
    pub effect: EffectRef<M>,
    /// Turns left including the current one (> 0 while stored).
    pub turns_left: u16,
}

/// Entries in insertion order (deterministic; hashed and saved in order).
#[derive(Debug, Clone, Default)]
pub struct ActiveEffectStore<M: RulesModule> {
    entries: Vec<ActiveEffect<M>>,
}

impl<M: RulesModule> ActiveEffectStore<M> {
    pub fn iter(&self) -> impl Iterator<Item = &ActiveEffect<M>> {
        self.entries.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Unique stacking: an entry of the same effect on the same target is
    /// refreshed instead of duplicated.
    pub fn start(&mut self, entry: ActiveEffect<M>, unique: bool) {
        if unique {
            let same = |e: &&mut ActiveEffect<M>| e.target == entry.target && e.effect == entry.effect;
            if let Some(existing) = self.entries.iter_mut().find(same) {
                existing.turns_left = existing.turns_left.max(entry.turns_left);
                return;
            }
        }
        self.entries.push(entry);
    }

    /// One resolution of `owner` passed: decrement the entries that existed
    /// when it started (`existing` = len at that moment; new entries are
    /// appended), drop expired. Returns whether any entry expired.
    pub fn tick(&mut self, owner: PlayerId, existing: usize) -> bool {
        let before = self.entries.len();
        for entry in self.entries.iter_mut().take(existing) {
            if entry.owner == owner {
                entry.turns_left -= 1;
            }
        }
        self.entries.retain(|e| e.turns_left > 0);
        self.entries.len() != before
    }

    pub(crate) fn from_entries(entries: Vec<ActiveEffect<M>>) -> Self {
        Self { entries }
    }
}
