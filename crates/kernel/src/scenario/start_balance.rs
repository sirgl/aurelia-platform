//! Balanced starts: every nation starts on land that can feed and build, with the
//! early strategic resources in reach, and the starts of one game are close in value.
//!
//! The policy is one struct, `StartPolicy`; its numbers were taken from the
//! distribution of the starts before this module existed (classic/docs/start-balance.md)
//! and the rules supply everything else: yields, resource classes and techs,
//! terrain codes. Nothing here names a resource, a terrain or a nation.
//!
//! | Step | Where | What |
//! |---|---|---|
//! | value | `around` | the value of a start: the real tile yields (terrain, feature, the bonus resources known from turn 0) of the start tile and the three rings round it, each yield channel and each ring by a weight of the policy |
//! | choose | `pick_starts_with` | a start whose food and production within two tiles reach the minimum, and whose value is no lower than `floor_percent` of the target, is preferred to any other; the target is the `target_percentile` of the values of all candidates and a candidate scores the more the nearer its value is to it, from below and above, so the starts of a game come out alike and none takes the jackpot; among equals the farthest from the chosen starts wins |
//! | strategic | `balance_starts` | each early strategic resource (reveal technology in an era up to `strategic_up_to_era`) lies within three tiles of every start, not under the start tile, in a fixed amount; the rules decide which resources these are; where no free tile has the right terrain, one is turned to it (its jungle or forest cleared) |
//! | even | `balance_starts` | a start under the minimum food or production, or under `even_percent` of the game's median value, gets at most `edit_budget` edits: a bonus resource on a free tile, or a poor flat tile turned into hills or grass |
//!
//! Integer arithmetic over the map and the seed only (a hash of the tile breaks ties), no random stream.

use super::landscape::tile_hash;
use crate::derived::tile_yields::{finish_tile, plot_yields};
use crate::ids::{ResourceId, TerrainId, TileId};
use crate::math::{HexCoord, MapDimensions};
use crate::module::RulesModule;
use crate::rules::content::{ResourceClass, Yields};
use crate::rules::roles::{Climate, Ground};
use crate::rules::CompiledRuleset;
use crate::world::builder::WorldBuilder;
use crate::world::facts::TruthFacts;
use crate::world::state::AuthWorld;
use std::collections::{BTreeMap, BTreeSet};

/// What each yield channel is worth in the value of a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YieldWeights {
    pub food: i32,
    pub production: i32,
    pub gold: i32,
    pub science: i32,
    pub culture: i32,
    pub module: i32,
}

/// The numbers of the policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartPolicy {
    /// Weights of the yield channels; production weighs more than food because land that can
    /// build is scarcer than land that can feed (coast and ocean feed).
    pub weights: YieldWeights,
    /// Weights of the start tile and of the rings 1, 2 and 3 round it.
    pub ring_weights: [i32; 4],
    /// The value every start aims at: this percentile of the values of all candidates. A
    /// candidate is the better the closer its value is to the target, from below and from above,
    /// so that the starts of a game come out close to each other and none takes the jackpot.
    pub target_percentile: u8,
    /// Candidates below this percent of the target are taken only when no other is left.
    pub floor_percent: u8,
    /// What the nearness to the target is worth in the score of a candidate, next to the nearness
    /// to the sea and the nation's wishes (a candidate at the target gets all of it, one at the
    /// floor or beyond the mirror of the floor none).
    pub target_points: i32,
    /// Least food and least production of the tiles within two tiles of the start (the start
    /// tile left out). The census (`start_census` example) measures the same sums.
    pub min_food: i32,
    pub min_production: i32,
    /// A start under this percent of the median value of the game's starts is topped up.
    pub even_percent: u8,
    /// Edits (bonus resources, terrain turns) a start may get at most.
    pub edit_budget: u8,
    /// A strategic resource is guaranteed when the era of its reveal technology is no later
    /// than this index (an era index of the rules, 0 is the first).
    pub strategic_up_to_era: u8,
    /// The order of the distances at which a guaranteed deposit is put.
    pub strategic_rings: [i32; 3],
    /// A farther start is no better than this distance, when starts are told apart.
    pub distance_clamp: i32,
}

impl StartPolicy {
    pub const CLASSIC: StartPolicy = StartPolicy {
        weights: YieldWeights { food: 2, production: 3, gold: 1, science: 1, culture: 1, module: 1 },
        ring_weights: [4, 3, 2, 1],
        target_percentile: 75,
        floor_percent: 80,
        target_points: 40,
        min_food: 18,
        min_production: 5,
        even_percent: 80,
        edit_budget: 4,
        strategic_up_to_era: 1,
        strategic_rings: [2, 3, 1],
        distance_clamp: 16,
    };
}

/// What a new game does to even its starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartBalance {
    /// Fair starts: the choice by value and the top-up (food, production, evenness).
    pub fair: bool,
    /// The guaranteed strategic resources ("Strategic balance" of the setup).
    pub strategic: bool,
    pub policy: StartPolicy,
}

impl StartBalance {
    /// The starts as they were before this module (tests that pin an old map).
    pub const OFF: StartBalance = StartBalance { fair: false, strategic: false, policy: StartPolicy::CLASSIC };
    pub const ON: StartBalance = StartBalance { fair: true, strategic: true, policy: StartPolicy::CLASSIC };
}

impl Default for StartBalance {
    fn default() -> Self {
        Self::ON
    }
}

/// What a balance changed (tests, the census, a log line).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BalanceReport {
    /// Strategic deposits put near a start.
    pub strategic_added: u32,
    /// Starts that lack a guaranteed strategic resource nothing could be put for.
    pub strategic_missing: u32,
    /// Bonus deposits put to top a start up.
    pub bonus_added: u32,
    /// Tiles turned to another terrain: to top a start up, or to make room for a guaranteed deposit.
    pub terrain_turned: u32,
    /// Features (jungle, forest) cleared to make room for a guaranteed deposit where nothing else was free.
    pub features_cleared: u32,
}

fn weighted(y: Yields, w: &YieldWeights) -> i64 {
    i64::from(y.food) * i64::from(w.food)
        + i64::from(y.production) * i64::from(w.production)
        + i64::from(y.gold) * i64::from(w.gold)
        + i64::from(y.science) * i64::from(w.science)
        + i64::from(y.culture) * i64::from(w.culture)
        + i64::from(y.module) * i64::from(w.module)
}

/// What lies round a start tile.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Around {
    /// The weighted value of the start tile and the three rings.
    pub value: i64,
    /// Food and production of rings 1 and 2.
    pub food2: i32,
    pub prod2: i32,
}

/// The hex ring of `offset` (axial) from the centre.
fn ring_of(dq: i32, dr: i32) -> usize {
    dq.abs().max(dr.abs()).max((dq + dr).abs()) as usize
}

/// The tiles within `radius` of `centre` with their ring.
pub(crate) fn disc(dims: MapDimensions, centre: TileId, radius: i32) -> Vec<(TileId, usize)> {
    let mut out = Vec::new();
    let Some(c) = dims.to_coord(centre) else {
        return out;
    };
    for dq in -radius..=radius {
        for dr in (-radius).max(-dq - radius)..=radius.min(radius - dq) {
            if let Some(t) = dims.to_tile(HexCoord { q: c.q + dq, r: c.r + dr }) {
                out.push((t, ring_of(dq, dr)));
            }
        }
    }
    out
}

/// The value of a start tile; `yield_at(tile, is_centre)` gives a tile's yields.
pub(crate) fn around(dims: MapDimensions, centre: TileId, policy: &StartPolicy, yield_at: &dyn Fn(TileId, bool) -> Yields) -> Around {
    let mut out = Around::default();
    for (tile, ring) in disc(dims, centre, 3) {
        let y = yield_at(tile, ring == 0);
        out.value += weighted(y, &policy.weights) * i64::from(policy.ring_weights[ring]);
        if (1..=2).contains(&ring) {
            out.food2 += y.food;
            out.prod2 += y.production;
        }
    }
    out
}

/// The values of the candidates of a pick and the target and floor taken from them.
pub(crate) struct FairPick {
    pub around: Vec<Around>,
    pub target: i64,
    pub floor: i64,
}

impl FairPick {
    /// Points of a candidate for its nearness to the target: `points` at the target, none as far
    /// from it as the floor is (and beyond).
    pub fn points(&self, value: i64, points: i32) -> i32 {
        let reach = (self.target - self.floor).max(1);
        let gap = (value - self.target).abs().min(reach);
        (i64::from(points) * (reach - gap) / reach) as i32
    }
}

/// The values of every candidate tile (`eligible`) as seen by a player without technologies.
/// None when the rules give nothing to value (no terrain yields): the pick then keeps its old way.
pub(crate) fn fair_pick<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    eligible: &[bool],
    policy: &StartPolicy,
) -> Option<FairPick> {
    let dims = auth.map().dimensions;
    let facts = TruthFacts::new(auth, rules, crate::ids::PlayerId(0));
    let yields: Vec<Yields> = (0..dims.tile_count())
        .map(|i| plot_yields(&facts, rules, TileId(i as u32), &facts.techs))
        .collect();
    let yield_at = |t: TileId, centre: bool| {
        let y = yields[t.0 as usize];
        if centre { finish_tile::<M>(y, true) } else { y }
    };
    let mut around_all = vec![Around::default(); yields.len()];
    let mut values: Vec<i64> = Vec::new();
    for (i, slot) in around_all.iter_mut().enumerate() {
        if eligible[i] {
            *slot = around(dims, TileId(i as u32), policy, &yield_at);
            values.push(slot.value);
        }
    }
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let target = values[(values.len() - 1) * usize::from(policy.target_percentile) / 100];
    if target <= 0 {
        return None;
    }
    Some(FairPick { around: around_all, target, floor: target * i64::from(policy.floor_percent) / 100 })
}

/// The worlds edits made so far (not yet in the builder), over the map.
struct Plan<'a, M: RulesModule> {
    auth: &'a AuthWorld<M>,
    rules: &'a CompiledRuleset<M>,
    starts: &'a [TileId],
    seed: u64,
    terrain: BTreeMap<TileId, u16>,
    deposit: BTreeMap<TileId, (ResourceId, u8)>,
    cleared: BTreeSet<TileId>,
}

const SALT: u64 = 0x57A2_7BA1_A9CE_0001;

impl<'a, M: RulesModule> Plan<'a, M> {
    fn dims(&self) -> MapDimensions {
        self.auth.map().dimensions
    }

    fn code(&self, t: TileId) -> Option<u16> {
        self.terrain.get(&t).copied().or_else(|| self.auth.map().terrain(t).map(|c| c.0))
    }

    fn ground(&self, t: TileId) -> Option<Ground> {
        self.code(t).and_then(|c| self.rules.terrain_kinds.palette().ground(c))
    }

    fn resource(&self, t: TileId) -> Option<ResourceId> {
        self.deposit.get(&t).map(|(r, _)| *r).or_else(|| self.auth.map().resource(t).flatten())
    }

    fn feature_free(&self, t: TileId) -> bool {
        self.cleared.contains(&t) || self.auth.map().feature(t).flatten().is_none()
    }

    /// Land a unit can enter, with no natural wonder, and not a start tile.
    fn usable(&self, t: TileId) -> bool {
        let map = self.auth.map();
        !map.is_water(t)
            && crate::navigation::cost::enter_cost_on_map(map, t).is_some()
            && map.natural_wonder(t).is_none()
            && !self.starts.contains(&t)
    }

    /// Yields of a tile as `facts`' player sees them, with the plan's edits.
    fn yields(&self, facts: &TruthFacts<'_, M>, t: TileId, centre: bool) -> Yields {
        let mut y = match self.terrain.get(&t) {
            Some(&code) => self.rules.terrain_of(TerrainId(code)).map_or(Yields::ZERO, |d| d.yields),
            None => plot_yields(facts, self.rules, t, &facts.techs),
        };
        if let Some((r, _)) = self.deposit.get(&t) {
            if let Some(def) = self.rules.resources.get(r.0 as usize) {
                if facts.techs.allows(def.required_tech) {
                    y = y.plus(def.yields);
                }
            }
        }
        if centre { finish_tile::<M>(y, true) } else { y }
    }

    fn measure(&self, facts: &TruthFacts<'_, M>, start: TileId, policy: &StartPolicy) -> Around {
        around(self.dims(), start, policy, &|t, centre| self.yields(facts, t, centre))
    }

    fn hash(&self, t: TileId, salt: u64) -> u64 {
        tile_hash(t.0, SALT ^ salt, self.seed)
    }

    /// A ground that may hold a guaranteed deposit when the tile is turned: land, not frozen,
    /// not a mountain.
    fn habitable(&self, code: u16) -> bool {
        matches!(
            self.rules.terrain_kinds.palette().ground(code),
            Some(Ground::Hills | Ground::Flat(Climate::Humid | Climate::Temperate | Climate::Arid | Climate::Cold))
        )
    }
}

/// Resources a start is promised: strategic ones whose reveal technology belongs to an
/// early era. A resource without a technology counts as early.
fn guaranteed_kinds<M: RulesModule>(rules: &CompiledRuleset<M>, policy: &StartPolicy) -> Vec<usize> {
    (0..rules.resources.len())
        .filter(|&k| {
            let def = &rules.resources[k];
            def.class == Some(ResourceClass::Strategic)
                && def.in_standard_game
                && def.required_tech.is_none_or(|tech| {
                    rules
                        .technologies
                        .get(tech.0 as usize)
                        .and_then(|t| t.content_era)
                        .and_then(|era| rules.eras.get(era.0 as usize))
                        .is_some_and(|era| era.index <= policy.strategic_up_to_era)
                })
        })
        .collect()
}

/// Bonus resources a start may be topped up with: placed by the generator on its own
/// (density above 0), known from the first turn, with yields.
fn topping_kinds<M: RulesModule>(rules: &CompiledRuleset<M>) -> Vec<usize> {
    (0..rules.resources.len())
        .filter(|&k| {
            let def = &rules.resources[k];
            def.class == Some(ResourceClass::Bonus)
                && def.required_tech.is_none()
                && def.in_standard_game
                && def.density > 0
                && !def.terrains.is_empty()
                && !def.yields.is_zero()
        })
        .collect()
}

/// The amount of a guaranteed deposit: the smallest quantity the rules know for the resource.
fn guaranteed_amount<M: RulesModule>(rules: &CompiledRuleset<M>, kind: usize) -> u8 {
    rules.resources[kind].quantities.iter().copied().min().unwrap_or(1).max(1)
}

/// Make the starts of a new game fair: put the guaranteed strategic resources near each of them,
/// then top up the starts that fall short. `starts` are the start tiles in player order; the
/// player `i` is the owner of `starts[i]` (its technologies decide which bonus resources it sees).
/// Call it after the starting units are placed and before anything else claims the free tiles.
pub fn balance_starts<M: RulesModule>(game: &mut WorldBuilder<'_, M>, starts: &[TileId], seed: u64, balance: &StartBalance) -> BalanceReport {
    let mut report = BalanceReport::default();
    if starts.is_empty() || !(balance.fair || balance.strategic) {
        return report;
    }
    let rules = game.rules();
    let policy = &balance.policy;
    let (terrain, deposits, cleared) = {
        let auth = game.state().view().auth;
        let mut plan = Plan { auth, rules, starts, seed, terrain: BTreeMap::new(), deposit: BTreeMap::new(), cleared: BTreeSet::new() };
        if balance.strategic {
            for &kind in &guaranteed_kinds(rules, policy) {
                for &start in starts {
                    if !has_within(&plan, start, kind, 3) {
                        if put_strategic(&mut plan, start, kind, policy, &mut report) {
                            report.strategic_added += 1;
                        } else {
                            report.strategic_missing += 1;
                        }
                    }
                }
            }
        }
        if balance.fair {
            top_up(&mut plan, policy, &mut report);
        }
        (plan.terrain, plan.deposit, plan.cleared)
    };
    if !cleared.is_empty() {
        game.clear_features(&cleared.into_iter().collect::<Vec<_>>());
    }
    if !terrain.is_empty() {
        let turned: Vec<(TileId, TerrainId)> = terrain.into_iter().map(|(t, c)| (t, TerrainId(c))).collect();
        game.set_terrain(&turned);
    }
    if !deposits.is_empty() {
        let placed: Vec<(TileId, ResourceId, u8)> = deposits.into_iter().map(|(t, (r, n))| (t, r, n)).collect();
        game.place_deposits(&placed);
    }
    report
}

/// Does a deposit of `kind` lie within `radius` of `start`, not on the start tile?
fn has_within<M: RulesModule>(plan: &Plan<'_, M>, start: TileId, kind: usize, radius: i32) -> bool {
    disc(plan.dims(), start, radius)
        .into_iter()
        .any(|(t, ring)| ring > 0 && plan.resource(t).is_some_and(|r| r.0 as usize == kind))
}

/// Put a deposit of `kind` near `start`. The nearest free tile of the preferred distance whose
/// terrain the resource lives on; when there is none, a free plain tile turned to a terrain it lives on.
fn put_strategic<M: RulesModule>(
    plan: &mut Plan<'_, M>,
    start: TileId,
    kind: usize,
    policy: &StartPolicy,
    report: &mut BalanceReport,
) -> bool {
    let def = &plan.rules.resources[kind];
    let ring_order = |ring: usize| policy.strategic_rings.iter().position(|&r| r as usize == ring).unwrap_or(usize::MAX);
    let free: Vec<(TileId, usize)> = disc(plan.dims(), start, 3)
        .into_iter()
        .filter(|&(t, ring)| ring > 0 && plan.usable(t) && plan.resource(t).is_none())
        .collect();
    let lives = |code: u16| def.terrains.is_empty() || def.terrains.contains(&code);
    // Rank: a tile without a feature, then the preferred distance, then the tile's hash.
    let best = free
        .iter()
        .filter(|&&(t, _)| plan.code(t).is_some_and(lives))
        .min_by_key(|&&(t, ring)| (!plan.feature_free(t), ring_order(ring), plan.hash(t, kind as u64)))
        .map(|&(t, _)| (t, plan.code(t).unwrap_or(0), false));
    // Nothing of the right terrain is free: turn a tile (clearing its jungle or forest when it has one;
    // a tile without a feature first, then the preferred distance).
    let chosen = best.or_else(|| {
        let turn_to = def.terrains.iter().copied().find(|&c| plan.habitable(c))?;
        free.iter()
            .filter(|&&(t, _)| plan.ground(t).is_some_and(|g| !matches!(g, Ground::Mountains | Ground::Water)))
            .min_by_key(|&&(t, ring)| (!plan.feature_free(t), ring_order(ring), plan.hash(t, kind as u64)))
            .map(|&(t, _)| (t, turn_to, true))
    });
    let Some((tile, code, turn)) = chosen else {
        return false;
    };
    if turn {
        if !plan.feature_free(tile) {
            plan.cleared.insert(tile);
            report.features_cleared += 1;
        }
        plan.terrain.insert(tile, code);
        report.terrain_turned += 1;
    }
    plan.deposit.insert(tile, (ResourceId(kind as u16), guaranteed_amount(plan.rules, kind)));
    true
}

/// Top up the starts under the minimum food or production or under the even bound.
fn top_up<M: RulesModule>(plan: &mut Plan<'_, M>, policy: &StartPolicy, report: &mut BalanceReport) {
    let rules = plan.rules;
    let starts = plan.starts;
    let facts: Vec<TruthFacts<'_, M>> =
        (0..starts.len()).map(|p| TruthFacts::new(plan.auth, rules, crate::ids::PlayerId(p as u8))).collect();
    let mut values: Vec<i64> = (0..starts.len()).map(|p| plan.measure(&facts[p], starts[p], policy).value).collect();
    values.sort_unstable();
    let median = values[(values.len() - 1) / 2];
    let target = median * i64::from(policy.even_percent) / 100;
    let bonus = topping_kinds(rules);
    for (p, &start) in starts.iter().enumerate() {
        for _ in 0..policy.edit_budget {
            let now = plan.measure(&facts[p], start, policy);
            let need_prod = now.prod2 < policy.min_production;
            let need_food = now.food2 < policy.min_food;
            let need_value = now.value < target;
            if !(need_prod || need_food || need_value) {
                break;
            }
            let Some(edit) = best_edit(plan, &facts[p], start, &bonus, policy, (need_prod, need_food)) else {
                break;
            };
            match edit {
                Edit::Bonus(tile, kind) => {
                    let amount = crate::scenario::deposit_amount(rules, ResourceId(kind as u16), tile);
                    plan.deposit.insert(tile, (ResourceId(kind as u16), amount));
                    report.bonus_added += 1;
                }
                Edit::Turn(tile, code) => {
                    plan.terrain.insert(tile, code);
                    report.terrain_turned += 1;
                }
            }
        }
    }
}

enum Edit {
    Bonus(TileId, usize),
    Turn(TileId, u16),
}

/// The edit that helps a start most: the production first when it is short, then the food, then
/// the value; a bonus deposit before a turned tile, a near tile before a far one.
fn best_edit<M: RulesModule>(
    plan: &Plan<'_, M>,
    facts: &TruthFacts<'_, M>,
    start: TileId,
    bonus: &[usize],
    policy: &StartPolicy,
    (need_prod, need_food): (bool, bool),
) -> Option<Edit> {
    let rules = plan.rules;
    let palette = rules.terrain_kinds.palette();
    let turn_targets = [Ground::Hills, Ground::Flat(Climate::Humid), Ground::Flat(Climate::Temperate)];
    let mut best: Option<((i64, u8, i32, u64), Edit)> = None;
    let mut consider = |delta: Yields, rank_tail: (u8, i32, u64), edit: Edit| {
        let gain = weighted(delta, &policy.weights);
        if gain <= 0 || (need_prod && delta.production <= 0) || (!need_prod && need_food && delta.food <= 0) {
            return;
        }
        let boost = if need_prod {
            i64::from(delta.production)
        } else if need_food {
            i64::from(delta.food)
        } else {
            0
        };
        // Larger is better: the boost, the gain, a bonus before a turn, nearer, then the hash.
        let key = (boost * 1000 + gain, rank_tail.0, -rank_tail.1, rank_tail.2);
        if best.as_ref().is_none_or(|(k, _)| key > *k) {
            best = Some((key, edit));
        }
    };
    for (tile, ring) in disc(plan.dims(), start, 2) {
        if ring == 0 || !plan.usable(tile) || plan.resource(tile).is_some() || !plan.feature_free(tile) {
            continue;
        }
        let Some(code) = plan.code(tile) else { continue };
        let hash = plan.hash(tile, 0xB0);
        for &kind in bonus {
            let def = &rules.resources[kind];
            if def.terrains.contains(&code) {
                consider(def.yields, (1, ring as i32, hash), Edit::Bonus(tile, kind));
            }
        }
        let here = rules.terrain_of(TerrainId(code)).map_or(Yields::ZERO, |d| d.yields);
        if !matches!(plan.ground(tile), Some(Ground::Hills | Ground::Flat(_))) || plan.auth.map().lake(tile) {
            continue;
        }
        for ground in turn_targets {
            let to = palette.code(ground);
            if to == code {
                continue;
            }
            if let Some(def) = rules.terrain_of(TerrainId(to)) {
                let delta = Yields {
                    food: def.yields.food - here.food,
                    production: def.yields.production - here.production,
                    gold: def.yields.gold - here.gold,
                    science: def.yields.science - here.science,
                    culture: def.yields.culture - here.culture,
                    module: def.yields.module - here.module,
                };
                consider(delta, (0, ring as i32, hash), Edit::Turn(tile, to));
            }
        }
    }
    let _ = facts;
    best.map(|(_, e)| e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims() -> MapDimensions {
        MapDimensions { width: 20, height: 20 }
    }

    #[test]
    fn a_disc_holds_one_tile_then_six_per_ring() {
        let centre = TileId(10 * 20 + 10);
        let disc = disc(dims(), centre, 3);
        let ring = |n: usize| disc.iter().filter(|(_, r)| *r == n).count();
        assert_eq!((ring(0), ring(1), ring(2), ring(3)), (1, 6, 12, 18));
        assert_eq!(disc.len(), 37);
        assert!(disc.iter().all(|(t, r)| dims().distance(centre, *t) == Some(*r as i32)), "the ring is the hex distance");
    }

    #[test]
    fn a_disc_at_the_edge_is_cut() {
        assert!(disc(dims(), TileId(0), 3).len() < 37);
    }

    #[test]
    fn the_value_weighs_channels_and_rings() {
        let w = StartPolicy::CLASSIC.weights;
        let y = Yields { food: 2, production: 1, gold: 1, ..Yields::ZERO };
        assert_eq!(weighted(y, &w), 2 * 2 + 3 + 1);
        let policy = StartPolicy::CLASSIC;
        // One food everywhere: 4 + 6*3 + 12*2 + 18*1 = 64 rings-weighted, 2 weight per food.
        let value = around(dims(), TileId(10 * 20 + 10), &policy, &|_, _| Yields { food: 1, ..Yields::ZERO });
        assert_eq!(value.value, 2 * (4 + 18 + 24 + 18));
        assert_eq!((value.food2, value.prod2), (18, 0));
    }

    #[test]
    fn nearness_to_the_target_is_worth_most_at_the_target() {
        let pick = FairPick { around: Vec::new(), target: 100, floor: 80 };
        assert_eq!(pick.points(100, 40), 40);
        assert_eq!(pick.points(90, 40), 20);
        assert_eq!(pick.points(110, 40), 20, "from above as from below");
        assert_eq!(pick.points(80, 40), 0);
        assert_eq!(pick.points(500, 40), 0);
        assert_eq!(pick.points(0, 40), 0);
    }

    #[test]
    fn the_policy_is_sane() {
        let p = StartPolicy::CLASSIC;
        assert!(p.floor_percent < 100 && p.target_percentile <= 100);
        assert!(p.ring_weights.windows(2).all(|w| w[0] >= w[1]), "nearer tiles weigh no less");
        assert_eq!(StartBalance::default(), StartBalance::ON);
        assert!(!StartBalance::OFF.fair && !StartBalance::OFF.strategic);
    }
}
