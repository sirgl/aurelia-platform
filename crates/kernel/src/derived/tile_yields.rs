//! Yields of one map tile by the Classic rules (classic/docs/classic/rules.md,
//! classic/docs/classic/terrain.json «yield_rules»):
//!
//! ```text
//! nature      terrain of the map code; a feature replaces it
//!             (`yield_not_additive`: forest, jungle) or adds to it; a
//!             natural wonder replaces both; a lake adds +1 food; rivers
//!             and coast add nothing in Classic
//! resource    + its yields once the owner knows the revealing technology
//! improvement + its yields, + the yields it adds to the resource it
//!             connects, + fresh water technology yields, + per adjacent
//!             mountain (terrace farm), + culture per adjacent twin (moai);
//!             nothing while pillaged
//! city centre at least the module's minimum (Classic: 2 food, 1 production)
//! clamp       every channel at least 0
//! ```
//!
//! Technology yields of improvements without a fresh water condition are
//! Tile effects of the technology and come with the city's tile effects
//! (`effects::city::TileEffects`), as do building and policy tile yields.
//! Pure functions of the world and the rules: no state, no cache.

use crate::derived::yields::YieldTerms;
use crate::module::RulesModule;
use crate::ids::{PlayerId, TechId, TileId};
use crate::rules::content::{FreshWater, Yields};
use crate::rules::CompiledRuleset;
use crate::world::facts::{TileFacts, TruthFacts};
use crate::world::state::AuthWorld;

/// Extra food of a lake tile (classic/docs/classic/terrain.json).
pub const LAKE_FOOD: i32 = 1;
/// the city centre yields at least this much food
/// and production (classic/docs/classic/terrain.json `city_tile_minimum`), the default
/// of `YieldTerms::CITY_MINIMUM`.
pub const CITY_MIN_FOOD: i32 = 2;
pub const CITY_MIN_PRODUCTION: i32 = 1;

/// The technologies a player knows, as a bitset (one lookup per question
/// instead of a scan of the learned list).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TechSet {
    bits: [u64; 4],
}

impl TechSet {
    pub fn of(techs: &[TechId]) -> Self {
        let mut set = TechSet::default();
        for tech in techs {
            let i = tech.0 as usize;
            if let Some(word) = set.bits.get_mut(i / 64) {
                *word |= 1 << (i % 64);
            }
        }
        set
    }

    pub fn of_player<M: RulesModule>(auth: &AuthWorld<M>, player: PlayerId) -> Self {
        Self::of(auth.players().techs_of(player))
    }

    pub fn has(&self, tech: TechId) -> bool {
        let i = tech.0 as usize;
        match self.bits.get(i / 64) {
            Some(word) => word & (1 << (i % 64)) != 0,
            None => false,
        }
    }

    /// No requirement, or a known technology.
    pub fn allows(&self, tech: Option<TechId>) -> bool {
        match tech {
            Some(t) => self.has(t),
            None => true,
        }
    }
}

/// a land tile on a river, next to a lake or next to
/// a fresh water feature (oasis).
pub fn fresh_water<M: RulesModule>(facts: &impl TileFacts, rules: &CompiledRuleset<M>, tile: TileId) -> bool {
    if facts.is_water(tile) {
        return false;
    }
    if facts.river(tile) {
        return true;
    }
    for near in facts.dimensions().neighbors(tile) {
        if facts.lake(near) {
            return true;
        }
        let fresh_feature = facts
            .feature(near)
            .and_then(|f| rules.features.get(f.0 as usize))
            .map(|def| def.fresh_water)
            .unwrap_or(false);
        let fresh_wonder = facts
            .natural_wonder(near)
            .and_then(|w| rules.natural_wonders.get(w.0 as usize))
            .map(|def| def.fresh_water)
            .unwrap_or(false);
        if fresh_feature || fresh_wonder {
            return true;
        }
    }
    false
}

/// Terrain and feature (and the lake bonus) of a tile.
pub fn nature_yields<M: RulesModule>(facts: &impl TileFacts, rules: &CompiledRuleset<M>, tile: TileId) -> Yields {
    let mut yields = match facts.terrain(tile).and_then(|code| rules.terrain_of(code)) {
        Some(def) => def.yields,
        None => Yields::default(),
    };
    if let Some(def) = facts
        .feature(tile)
        .and_then(|f| rules.features.get(f.0 as usize))
    {
        yields = if def.yield_not_additive {
            def.yields
        } else {
            yields.plus(def.yields)
        };
    }
    // A natural wonder's own yields replace the terrain's (natural_wonders.json).
    if let Some(def) = facts
        .natural_wonder(tile)
        .and_then(|w| rules.natural_wonders.get(w.0 as usize))
    {
        yields = def.yields;
    }
    if facts.lake(tile) {
        yields.food += LAKE_FOOD;
    }
    yields
}

/// Everything the tile itself yields to a player knowing `techs` (nature,
/// revealed resource, improvement), without city-specific parts. The facts
/// filter the resource by what the viewer sees.
pub fn plot_yields<M: RulesModule>(
    facts: &impl TileFacts,
    rules: &CompiledRuleset<M>,
    tile: TileId,
    techs: &TechSet,
) -> Yields {
    let mut yields = nature_yields(facts, rules, tile);
    let resource = facts.resource(tile);
    if let Some(def) = resource.and_then(|r| rules.resources.get(r.0 as usize)) {
        yields = yields.plus(def.yields);
    }
    // A pillaged improvement yields nothing until it is repaired.
    if let Some(imp) = facts.improvement(tile).filter(|_| !facts.pillaged(tile)) {
        yields = yields.plus(improvement_yields(facts, rules, tile, imp, resource, techs));
    }
    yields
}

/// The plot yields as if `improvement` stood on the tile (the AI and the
/// UI weigh a build with this).
pub fn plot_yields_with<M: RulesModule>(
    facts: &impl TileFacts,
    rules: &CompiledRuleset<M>,
    tile: TileId,
    techs: &TechSet,
    improvement: crate::ids::ImprovementId,
    removes_feature: bool,
) -> Yields {
    let mut yields = match facts.terrain(tile).and_then(|code| rules.terrain_of(code)) {
        Some(def) => def.yields,
        None => Yields::default(),
    };
    if !removes_feature {
        yields = nature_yields(facts, rules, tile);
    } else if facts.lake(tile) {
        yields.food += LAKE_FOOD;
    }
    let resource = facts.resource(tile);
    if let Some(def) = resource.and_then(|r| rules.resources.get(r.0 as usize)) {
        yields = yields.plus(def.yields);
    }
    yields.plus(improvement_yields(
        facts,
        rules,
        tile,
        improvement,
        resource,
        techs,
    ))
}

/// The player sees and benefits from the resource.
pub fn resource_revealed<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    resource: crate::ids::ResourceId,
    techs: &TechSet,
) -> bool {
    match rules.resources.get(resource.0 as usize) {
        Some(def) => techs.allows(def.required_tech),
        None => false,
    }
}

/// What an improvement adds on a tile (its own yields, the connected
/// resource's improved yields, fresh water technology yields, mountains).
fn improvement_yields<M: RulesModule>(
    facts: &impl TileFacts,
    rules: &CompiledRuleset<M>,
    tile: TileId,
    imp: crate::ids::ImprovementId,
    resource: Option<crate::ids::ResourceId>,
    techs: &TechSet,
) -> Yields {
    let Some(def) = rules.improvements.get(imp.0 as usize) else {
        return Yields::default();
    };
    let mut yields = def.yields;
    if let Some(res) = resource.and_then(|r| rules.resources.get(r.0 as usize)) {
        for (connecting, extra) in &res.improved_yields {
            if *connecting == imp {
                yields = yields.plus(*extra);
            }
        }
    }
    if !def.fresh_water_yields.is_empty() {
        let fresh = fresh_water(facts, rules, tile);
        for y in &def.fresh_water_yields {
            let condition = match y.fresh_water {
                FreshWater::Any => true,
                FreshWater::Required => fresh,
                FreshWater::Absent => !fresh,
            };
            if condition && techs.allows(y.tech) {
                yields.add(y.yield_kind, y.amount);
            }
        }
    }
    if !def.adjacent_mountain_yields.is_zero() {
        for near in facts.dimensions().neighbors(tile) {
            if facts.terrain(near).is_some_and(|t| rules.terrain_kinds.is_mountains(t.0)) {
                yields = yields.plus(def.adjacent_mountain_yields);
            }
        }
    }
    if def.adjacent_same_culture > 0 {
        // Moai: culture per adjacent twin.
        for near in facts.dimensions().neighbors(tile) {
            if facts.improvement(near) == Some(imp) && !facts.pillaged(near) {
                yields.culture += i32::from(def.adjacent_same_culture);
            }
        }
    }
    yields
}

/// The city centre minimum (the module's `YieldTerms::CITY_MINIMUM`), then
/// the clamp at 0.
pub(crate) fn finish_tile<M: RulesModule>(yields: Yields, city_centre: bool) -> Yields {
    let minimum = if city_centre { M::Yields::CITY_MINIMUM } else { Yields::ZERO };
    yields.at_least_each(minimum).at_least(0)
}

/// Yields of a tile as its owner sees it outside any city (UI, AI): plot
/// yields of the tile's owner (or of `viewer` on unowned land), centre
/// minimum on a city tile.
pub fn tile_yields_for<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    tile: TileId,
    viewer: PlayerId,
) -> Yields {
    let owner = auth.map().owner(tile).flatten().unwrap_or(viewer);
    let facts = TruthFacts::new(auth, rules, owner);
    let centre = auth.indexes().city_on_tile(tile).is_some();
    finish_tile::<M>(plot_yields(&facts, rules, tile, &facts.techs), centre)
}
