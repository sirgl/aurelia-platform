//! Which tiles a city works and how many specialists it has (Classic
//! Classic citizens), derived from the city's orders (`world::citizens`):
//!
//! ```text
//! centre       always worked, free
//! specialists  hand-assigned ones first (up to the free slots)
//! locked tiles next, in tile order
//! the rest     one by one, the best candidate by the focus weights: an own
//!              tile in 3 rings, a free specialist slot, or unemployed
//!              (the default specialist, always available); food counts
//!              double while the city would not grow
//! ```
//!
//! Deterministic (ties: tiles before specialists, lower tile id / kind
//! first) and allocation-free: worked tiles live in a fixed array.

use crate::world::city::CITY_WORK_RADIUS;
use crate::derived::tile_yields::{finish_tile, plot_yields, TechSet};
use crate::derived::yields::YieldTerms;
use crate::effects::city::{city_tile_effects, TileEffects};
use crate::ids::{CityHandle, PlayerId, TileId};
use crate::math::hex_disc;
use crate::module::RulesModule;
use crate::rules::content::{SpecialistId, Yields};
use crate::rules::stats::{Stat, StatTotals};
use crate::rules::CompiledRuleset;
use crate::world::citizens::CityFocus;
use crate::world::city::{CityCore, MAX_SPECIALIST_KINDS, MAX_WORKED};
use crate::world::state::AuthWorld;

/// Food each citizen eats.
/// Unciv: logic/city/CityStats.kt (2 food per citizen), equal.
pub const FOOD_PER_CITIZEN: i32 = 2;

/// The module's empire terms of a player (`YieldTerms::Empire`).
pub type ModuleEmpire<M> = <<M as RulesModule>::Yields as YieldTerms<M>>::Empire;

/// What the city's owner knows and is, as far as tile yields go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmpireContext<M: RulesModule> {
    pub techs: TechSet,
    /// The rules module's terms of the empire (`YieldTerms::empire`).
    pub module: ModuleEmpire<M>,
}

impl<M: RulesModule> EmpireContext<M> {
    pub fn of(auth: &AuthWorld<M>, player: PlayerId) -> Self {
        EmpireContext {
            techs: TechSet::of_player(auth, player),
            module: M::Yields::empire(auth, player),
        }
    }
}

/// The assignment of one city.
#[derive(Debug, Clone, Copy)]
pub struct CityWork {
    worked: [(TileId, Yields); MAX_WORKED],
    len: u8,
    /// Citizens per specialist kind (index = `SpecialistId`), the default
    /// specialist (unemployed) included.
    pub specialists: [u8; MAX_SPECIALIST_KINDS],
    /// Sum of the worked tiles' yields (centre included).
    pub tile_yields: Yields,
    /// Sum of the specialists' yields.
    pub specialist_yields: Yields,
    /// Tiles looked at (work counters).
    pub visited: u64,
}

impl CityWork {
    /// Worked tiles, the centre first.
    pub fn tiles(&self) -> impl Iterator<Item = TileId> + '_ {
        self.worked[..self.len as usize].iter().map(|(t, _)| *t)
    }

    /// Worked tiles with their yields.
    pub fn worked(&self) -> &[(TileId, Yields)] {
        &self.worked[..self.len as usize]
    }

    pub fn works(&self, tile: TileId) -> bool {
        self.tiles().any(|t| t == tile)
    }

    /// Citizens that are specialists (unemployed included).
    pub fn specialist_count(&self) -> u32 {
        self.specialists.iter().map(|n| *n as u32).sum()
    }

    fn push_tile(&mut self, tile: TileId, yields: Yields) {
        self.worked[self.len as usize] = (tile, yields);
        self.len += 1;
        self.tile_yields = self.tile_yields.plus(yields);
    }
}

/// Free specialist slots of a city per kind (index = `SpecialistId`).
pub fn specialist_slots<M: RulesModule>(rules: &CompiledRuleset<M>, core: &CityCore) -> [u16; MAX_SPECIALIST_KINDS] {
    let mut slots = [0u16; MAX_SPECIALIST_KINDS];
    for building in &core.buildings {
        let Some(def) = rules.buildings.get(building.0 as usize) else {
            continue;
        };
        if let Some((kind, count)) = def.specialist_slots {
            if let Some(slot) = slots.get_mut(kind.0 as usize) {
                *slot = slot.saturating_add(count);
            }
        }
    }
    slots
}

/// The yields of one tile as worked by `core`: the plot, the city's tile
/// effects and the module's tile terms, the centre minimum, then the
/// module's final tile term.
pub fn worked_tile_yields<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    tile: TileId,
    effects: &TileEffects<M>,
    empire: &EmpireContext<M>,
) -> Yields {
    let facts = crate::world::facts::TruthFacts {
        auth,
        rules,
        techs: empire.techs,
    };
    let mut yields = plot_yields(&facts, rules, tile, &empire.techs);
    if !effects.is_empty() {
        yields = effects.bonus(auth, rules, core, tile).apply::<M>(yields);
    }
    yields = yields.plus(M::Yields::tile_base(auth, rules, core, tile));
    let mut yields = finish_tile::<M>(yields, tile == core.tile);
    M::Yields::tile_final(&empire.module, &mut yields);
    yields
}

/// Weights of the automatic assignment: food, production, gold, science,
/// culture, the module's yield, a specialist's points (per point).
pub fn weights(focus: CityFocus) -> [i32; 7] {
    let mut w = [12, 8, 5, 6, 6, 5, 4];
    let favoured = match focus {
        CityFocus::Default => None,
        CityFocus::Food => Some(0),
        CityFocus::Production => Some(1),
        CityFocus::Gold => Some(2),
        CityFocus::Science => Some(3),
        CityFocus::Culture => Some(4),
        CityFocus::ModuleYield => Some(5),
        CityFocus::SpecialistPoints => Some(6),
    };
    if let Some(i) = favoured {
        w[i] *= 3;
    }
    w
}

fn score(y: &Yields, points: i32, w: &[i32; 7], hungry: bool) -> i32 {
    let food_weight = if hungry { w[0] * 2 } else { w[0] };
    y.food * food_weight
        + y.production * w[1]
        + y.gold * w[2]
        + y.science * w[3]
        + y.culture * w[4]
        + y.module * w[5]
        + points * w[6]
}

/// A tile the city may choose.
#[derive(Clone, Copy)]
struct TileCandidate {
    tile: TileId,
    yields: Yields,
    taken: bool,
}

/// The tiles `core` may work beyond its centre: its own tiles in 3 rings
/// that the player did not take off work.
fn workable<M: RulesModule>(auth: &AuthWorld<M>, core: &CityCore, handle: Option<CityHandle>, tile: TileId) -> bool {
    tile != core.tile
        && handle.is_some()
        && auth.map().owning_city(tile) == handle
        && !core.unworked.contains(&tile)
}

/// Assign the citizens of `core` (see the module docs).
/// Yields of one specialist of each kind in a city: the specialist's own
/// plus the city's `SpecialistYield` stats (the kind's own, and the one for
/// every specialist but the unemployed citizen).
pub fn specialist_yield_table<M: RulesModule>(rules: &CompiledRuleset<M>, stats: &StatTotals<M>) -> [Yields; MAX_SPECIALIST_KINDS] {
    let mut table = [Yields::default(); MAX_SPECIALIST_KINDS];
    for (i, def) in rules.specialists.iter().enumerate().take(MAX_SPECIALIST_KINDS) {
        let kind = SpecialistId(i as u16);
        let mut yields = def.yields;
        for (stat, amount) in stats.iter() {
            if let Stat::SpecialistYield { specialist, yield_kind } = stat {
                let applies = match specialist {
                    Some(s) => s == kind,
                    None => Some(kind) != rules.roles.default_specialist,
                };
                if applies {
                    yields.add(yield_kind, amount);
                }
            }
        }
        table[i] = yields;
    }
    table
}

/// Assign the citizens of `core` (see the module docs); `city_stats` are the
/// city's stat totals (specialist yields).
pub fn assign<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    empire: &EmpireContext<M>,
    city_stats: &StatTotals<M>,
) -> CityWork {
    let handle = auth.indexes().city_on_tile(core.tile);
    let table = specialist_yield_table(rules, city_stats);
    let effects = city_tile_effects(auth, rules, core);
    let mut work = CityWork {
        worked: [(core.tile, Yields::default()); MAX_WORKED],
        len: 0,
        specialists: [0; MAX_SPECIALIST_KINDS],
        tile_yields: Yields::default(),
        specialist_yields: Yields::default(),
        visited: 0,
    };
    let centre = worked_tile_yields(auth, rules, core, core.tile, &effects, empire);
    work.push_tile(core.tile, centre);
    let mut candidates = [TileCandidate {
        tile: core.tile,
        yields: Yields::default(),
        taken: false,
    }; MAX_WORKED];
    let mut count = 0usize;
    for tile in hex_disc(auth.map().dimensions, core.tile, CITY_WORK_RADIUS) {
        work.visited += 1;
        if !workable(auth, core, handle, tile) {
            continue;
        }
        let yields = worked_tile_yields(auth, rules, core, tile, &effects, empire);
        candidates[count] = TileCandidate {
            tile,
            yields,
            taken: false,
        };
        count += 1;
    }
    let mut slots = specialist_slots(rules, core);
    let mut citizens = core.population;
    citizens -= assign_forced_specialists(&table, core, &mut work, &mut slots, citizens);
    citizens -= assign_locked_tiles(core, &mut work, &mut candidates[..count], citizens);
    assign_best(
        rules,
        &table,
        core,
        &mut work,
        &mut candidates[..count],
        &mut slots,
        citizens,
    );
    work
}

/// Hand-assigned specialists, up to the free slots and the citizens.
fn assign_forced_specialists(
    table: &[Yields; MAX_SPECIALIST_KINDS],
    core: &CityCore,
    work: &mut CityWork,
    slots: &mut [u16; MAX_SPECIALIST_KINDS],
    citizens: u32,
) -> u32 {
    let mut used = 0u32;
    for (kind, count) in &core.citizens.specialists {
        let i = kind.0 as usize;
        let Some(free) = slots.get_mut(i) else {
            continue;
        };
        let n = (*count as u32).min(*free as u32).min(citizens - used);
        for _ in 0..n {
            add_specialist(table, work, *kind);
        }
        *free -= n as u16;
        used += n;
    }
    used
}

/// Locked tiles the city may work, in tile order, up to the citizens.
fn assign_locked_tiles(
    core: &CityCore,
    work: &mut CityWork,
    candidates: &mut [TileCandidate],
    citizens: u32,
) -> u32 {
    let mut used = 0u32;
    for tile in &core.citizens.locked {
        if used == citizens {
            break;
        }
        let Some(candidate) = candidates.iter_mut().find(|c| c.tile == *tile) else {
            continue;
        };
        candidate.taken = true;
        work.push_tile(candidate.tile, candidate.yields);
        used += 1;
    }
    used
}

/// The remaining citizens, one by one, to the best candidate.
fn assign_best<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    table: &[Yields; MAX_SPECIALIST_KINDS],
    core: &CityCore,
    work: &mut CityWork,
    candidates: &mut [TileCandidate],
    slots: &mut [u16; MAX_SPECIALIST_KINDS],
    citizens: u32,
) {
    let w = weights(core.citizens.focus);
    // A city that avoids growth asks for the food its citizens eat only.
    let growth_food = if core.citizens.avoid_growth { 0 } else { FOOD_PER_CITIZEN };
    let food_needed = FOOD_PER_CITIZEN * core.population as i32 + growth_food;
    for _ in 0..citizens {
        let food_now = work.tile_yields.food + work.specialist_yields.food;
        let hungry = food_now < food_needed;
        let best_tile = best_tile(candidates, &w, hungry);
        let best_specialist = best_specialist(rules, table, slots, &w, hungry);
        match (best_tile, best_specialist) {
            (Some((_, tile_score)), Some((kind, spec_score))) if spec_score > tile_score => {
                take_specialist(rules, table, work, slots, kind)
            }
            (Some((at, _)), _) => {
                candidates[at].taken = true;
                work.push_tile(candidates[at].tile, candidates[at].yields);
            }
            (None, Some((kind, _))) => take_specialist(rules, table, work, slots, kind),
            (None, None) => return,
        }
    }
}

fn take_specialist<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    table: &[Yields; MAX_SPECIALIST_KINDS],
    work: &mut CityWork,
    slots: &mut [u16; MAX_SPECIALIST_KINDS],
    kind: SpecialistId,
) {
    if Some(kind) != rules.roles.default_specialist {
        slots[kind.0 as usize] -= 1;
    }
    add_specialist(table, work, kind);
}

fn add_specialist(table: &[Yields; MAX_SPECIALIST_KINDS], work: &mut CityWork, kind: SpecialistId) {
    let Some(yields) = table.get(kind.0 as usize) else {
        return;
    };
    if let Some(n) = work.specialists.get_mut(kind.0 as usize) {
        *n = n.saturating_add(1);
    }
    work.specialist_yields = work.specialist_yields.plus(*yields);
}

/// The best free tile: (position, score); ties go to the lower tile id.
fn best_tile(candidates: &[TileCandidate], w: &[i32; 7], hungry: bool) -> Option<(usize, i32)> {
    let mut best: Option<(usize, i32)> = None;
    for (at, c) in candidates.iter().enumerate() {
        if c.taken {
            continue;
        }
        let s = score(&c.yields, 0, w, hungry);
        let better = match best {
            None => true,
            Some((b, bs)) => s > bs || (s == bs && c.tile.0 < candidates[b].tile.0),
        };
        if better {
            best = Some((at, s));
        }
    }
    best
}

/// The best specialist with a free slot, or the default specialist
/// (always available): (kind, score); ties go to the lower kind.
fn best_specialist<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    table: &[Yields; MAX_SPECIALIST_KINDS],
    slots: &[u16; MAX_SPECIALIST_KINDS],
    w: &[i32; 7],
    hungry: bool,
) -> Option<(SpecialistId, i32)> {
    let mut best: Option<(SpecialistId, i32)> = None;
    for (i, def) in rules
        .specialists
        .iter()
        .enumerate()
        .take(MAX_SPECIALIST_KINDS)
    {
        let kind = SpecialistId(i as u16);
        let available = slots[i] > 0 || Some(kind) == rules.roles.default_specialist;
        if !available {
            continue;
        }
        let points = crate::rules::rows::SpecialistExtension::points(&def.module);
        let s = score(&table[i], i32::from(points), w, hungry);
        if best.map(|(_, bs)| s > bs).unwrap_or(true) {
            best = Some((kind, s));
        }
    }
    best
}
