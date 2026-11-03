//! Numeric stats that effects add to: one effect modifier
//! (`Modifier::AddStat`) for a family of plain sums on a city or a player
//! (...). Each
//! stat names the scopes it may target; its consumer is the system that
//! owns it (growth, citizens, workers, ...). The kernel's stats are here;
//! a rules module adds its own as one more vocabulary family
//! (`Vocabulary::Stat`, a `Stat::Module` word; Classic: `classic::stats`).

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use crate::module::RulesModule;
use crate::ids::ResourceId;
use crate::rules::content::SpecialistId;
use crate::rules::effects::{Scope, YieldKind};
use crate::rules::source::StableContentKey;
use crate::rules::vocabulary::{hash_tag, ModuleStat, StatWord, Word};

/// A stat with its references resolved: the kernel's (yields of
/// specialists, food kept, resources, workers, routes) or a word of the
/// rules module (`Vocabulary::Stat`; Classic: happiness, great people, trade,
/// society, espionage, the World Congress, ...).
///
/// Every stat has a tag: its position in the one closed enum the stats were
/// before the split. `Hash` writes the tag as the derived `Hash` of that
/// enum wrote its discriminant, and `Ord` sorts by it, so the fingerprint
/// and the order of sums are what they were.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stat<M: RulesModule> {
    /// Percent of the growth threshold kept on growth (`FoodKept`).
    FoodKept,
    /// Flat yield of each specialist of a kind (None: every specialist but
    /// the unemployed citizen).
    SpecialistYield {
        specialist: Option<SpecialistId>,
        yield_kind: YieldKind<M>,
    },
    /// Copies of a resource the player gets from its buildings
    /// (Recycling Center, +2 aluminum).
    ResourceCopies(ResourceId),
    /// Percent of the copies of a resource on the player's tiles
    /// (Arabia's oil, Russia's iron).
    ResourceQuantityPercent(ResourceId),
    /// Percent of the copies of every strategic resource on the player's
    /// tiles, after the one above.
    StrategicQuantityPercent,
    /// Extra copies of each luxury on the city's tiles
    /// (Bazaar).
    ExtraLuxuryCopies,
    /// Percent of the build rate of the player's workers
    /// (Pyramids, Citizenship).
    WorkerSpeedPercent,
    /// Percent of the gold maintenance of the player's roads and railroads
    /// (Wagon Trains; Inca), classic/docs/classic/rules.md.
    RouteMaintenancePercent,
    /// Flat yield of a city that has at least one specialist, once per city
    /// («+2 Production if city has a
    /// Specialist»). Unverified: once per city, as the in-game text says, not
    /// per specialist.
    AnySpecialistYield(YieldKind<M>),
    /// A stat of the rules module.
    Module(ModuleStat<M>),
}

impl<M: RulesModule> Stat<M> {
    /// The stat's tag (see the type's docs).
    pub fn tag(self) -> u8 {
        match self {
            Stat::FoodKept => 2,
            Stat::SpecialistYield { .. } => 3,
            Stat::ResourceCopies(_) => 13,
            Stat::ResourceQuantityPercent(_) => 14,
            Stat::StrategicQuantityPercent => 15,
            Stat::ExtraLuxuryCopies => 16,
            Stat::WorkerSpeedPercent => 17,
            Stat::RouteMaintenancePercent => 18,
            Stat::AnySpecialistYield(_) => 23,
            Stat::Module(stat) => stat.tag(),
        }
    }

    /// Scopes the stat may target (the compiler checks every effect).
    pub fn fits(self, scope: Scope<M>) -> bool {
        match self {
            Stat::FoodKept | Stat::SpecialistYield { .. } | Stat::AnySpecialistYield(_) => scope == Scope::City,
            Stat::ResourceCopies(_) | Stat::ResourceQuantityPercent(_) | Stat::StrategicQuantityPercent => {
                scope == Scope::Player
            }
            Stat::ExtraLuxuryCopies => scope == Scope::City,
            Stat::WorkerSpeedPercent | Stat::RouteMaintenancePercent => scope == Scope::Player,
            Stat::Module(stat) => stat.fits(scope),
        }
    }
}

impl<M: RulesModule> Hash for Stat<M> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Stat::Module(stat) => stat.hash(state),
            kernel => {
                hash_tag(kernel.tag(), state);
                match kernel {
                    Stat::SpecialistYield { specialist, yield_kind } => {
                        specialist.hash(state);
                        yield_kind.hash(state);
                    }
                    Stat::ResourceCopies(r) | Stat::ResourceQuantityPercent(r) => r.hash(state),
                    Stat::AnySpecialistYield(y) => y.hash(state),
                    _ => {}
                }
            }
        }
    }
}

impl<M: RulesModule> PartialOrd for Stat<M> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<M: RulesModule> Ord for Stat<M> {
    /// By tag, then by the stat's fields (the derived order of the closed
    /// enum).
    fn cmp(&self, other: &Self) -> Ordering {
        let by_tag = self.tag().cmp(&other.tag());
        if by_tag != Ordering::Equal {
            return by_tag;
        }
        match (self, other) {
            (
                Stat::SpecialistYield { specialist: a, yield_kind: x },
                Stat::SpecialistYield { specialist: b, yield_kind: y },
            ) => a.cmp(b).then_with(|| x.cmp(y)),
            (Stat::ResourceCopies(a), Stat::ResourceCopies(b))
            | (Stat::ResourceQuantityPercent(a), Stat::ResourceQuantityPercent(b)) => a.cmp(b),
            (Stat::AnySpecialistYield(a), Stat::AnySpecialistYield(b)) => a.cmp(b),
            (Stat::Module(a), Stat::Module(b)) => a.cmp(b),
            _ => Ordering::Equal,
        }
    }
}

/// A stat as written in content (references by key).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize), serde(bound(deserialize = "crate::rules::vocabulary::ModuleScope<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSelector<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleTrigger<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleYield<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCost<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleUnitAbility<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleStat<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleCount<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleGrant<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceModifier<M>: serde::Deserialize<'de>, crate::rules::vocabulary::ModuleSourceCondition<M>: serde::Deserialize<'de>")))]
pub enum SourceStat<M: RulesModule> {
    FoodKept,
    SpecialistYield {
        specialist: Option<StableContentKey>,
        yield_kind: YieldKind<M>,
    },
    ResourceCopies(StableContentKey),
    ResourceQuantityPercent(StableContentKey),
    StrategicQuantityPercent,
    ExtraLuxuryCopies,
    WorkerSpeedPercent,
    RouteMaintenancePercent,
    AnySpecialistYield(YieldKind<M>),
    /// A stat of the rules module (it names no row).
    Module(ModuleStat<M>),
}

/// Sums of stats on one target (sparse: few stats reach one city).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatTotals<M: RulesModule> {
    entries: Vec<(Stat<M>, i32)>,
}

impl<M: RulesModule> StatTotals<M> {
    pub fn add(&mut self, stat: Stat<M>, amount: i32) {
        match self.entries.binary_search_by(|(s, _)| s.cmp(&stat)) {
            Ok(at) => self.entries[at].1 += amount,
            Err(at) => self.entries.insert(at, (stat, amount)),
        }
    }

    pub fn get(&self, stat: Stat<M>) -> i32 {
        match self.entries.binary_search_by(|(s, _)| s.cmp(&stat)) {
            Ok(at) => self.entries[at].1,
            Err(_) => 0,
        }
    }

    pub fn merge(&mut self, other: &StatTotals<M>) {
        for (stat, amount) in &other.entries {
            self.add(*stat, *amount);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Stat<M>, i32)> + '_ {
        self.entries.iter().copied()
    }

}
