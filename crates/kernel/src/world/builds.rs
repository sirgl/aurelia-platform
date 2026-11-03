//! Where workers may build (Classic builds): the read-only rules of
//! improvement sites, feature removal and chop production that validation,
//! the worker system (`systems::improvements`), the UI and the AI share.
//!
//! Where an improvement may stand (`improvement_site`, Classic
//! improvement placement and worker builds):
//!
//! ```text
//! the builder   workers build land improvements, work boats water ones
//!               (and are used up); great person improvements come from
//!               great people (systems::great_people)
//! the tile      land / water as the improvement says; not a city; own land
//!               (or next to it for in_adjacent_friendly, unowned land for
//!               outside_borders, anybody's for ignore_ownership, only a
//!               a host player's for only_host_land); over another
//!               improvement for requires_improvement
//! valid if      its terrain is listed, or hills (hills_makes_valid), or
//!               fresh water (fresh_water_makes_valid), or it connects the
//!               tile's resource; never hills without fresh water when it
//!               requires flatlands or fresh water
//! a feature     must be one of its features, or be removed by the build
//!               (the removal technology is known; time adds up)
//! a resource    a revealed resource it does not connect forbids it, unless
//!               buildable_on_resources
//! extras        coastal, no_two_adjacent, adjacent_luxury, the nation
//!               of a unique improvement, the technology
//! repair        the same improvement, pillaged: REPAIR_WORK_TIME
//! ```
//!
//! Feature removal: the removal
//! technology, the removal time; on completion the chop production goes to
//! the city working the tile (else the nearest own city), minus 5 per tile
//! beyond 2, and only on own land. The fall-off and the ownership
//! rule are own values, not compared with a public source (unverified).

use crate::module::RulesModule;
use crate::derived::tile_yields::{fresh_water, TechSet};
use crate::ids::{CityHandle, FeatureId, ImprovementId, PlayerId, TileId};
use crate::rules::compiled::{FeatureRemoval, ImprovementDefinition};
use crate::rules::content::ResourceClass;
use crate::rules::CompiledRuleset;
use crate::world::facts::{TileFacts, TruthFacts};
use crate::world::state::AuthWorld;

/// Work time of repairing (hundredths of worker-turns): building a
/// pillaged improvement again repairs it.
pub const REPAIR_WORK_TIME: u32 = 300;

/// Classic chop production fall-off: −5 per tile beyond 2 from the city.
pub const CHOP_FALLOFF_PER_TILE: u32 = 5;
pub const CHOP_FULL_DISTANCE: i32 = 2;

/// A build the rules allow: its work, the feature it removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildPlan {
    /// Worker turns at the base rate (estimates of the AI and the HUD).
    pub work_turns: u16,
    /// Work in hundredths of worker-turns (the build and its removal).
    pub work_time: u32,
    pub removes: Option<FeatureId>,
}

/// Who builds: the player, its nation, its technologies.
#[derive(Debug, Clone, Copy)]
pub struct Builder {
    pub player: PlayerId,
    pub nation: u8,
    pub techs: TechSet,
}

impl Builder {
    pub fn of<M: RulesModule>(auth: &AuthWorld<M>, player: PlayerId) -> Self {
        Builder {
            player,
            nation: auth.players().nation_of(player),
            techs: TechSet::of_player(auth, player),
        }
    }
}

/// Work rate of a worker: hundredths of worker-turns a turn.
pub const BASE_WORK_RATE: u32 = 100;

/// Work a worker of `owner` does in one turn (Classic worker rate):
/// the base rate × (100 + the player's `WorkerSpeedPercent`) / 100
/// (Pyramids, Citizenship), at least 1.
pub fn work_rate<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, owner: PlayerId) -> u32 {
    let speed = crate::effects::player::player_modifiers(auth, rules, owner)
        .stats
        .get(crate::rules::stats::Stat::WorkerSpeedPercent);
    let rate = i64::from(BASE_WORK_RATE) * (100 + i64::from(speed)).max(0) / 100;
    rate.clamp(1, i64::from(u32::MAX)) as u32
}

/// Turns a job of `owner` still takes at the worker's rate (the UI).
pub fn job_turns_left<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, owner: PlayerId, work: &crate::ids::UnitWork) -> u32 {
    u32::from(work.work_left).div_ceil(work_rate(auth, rules, owner)).max(1)
}

/// Worker turns of a work time (hundredths of worker-turns).
pub fn work_turns(work_time: u32) -> u16 {
    work_time.div_ceil(100).max(1).min(u16::MAX as u32) as u16
}

/// Can `player` have `improvement` on `tile` now (the truth)?
pub fn improvement_site<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    tile: TileId,
    improvement: ImprovementId,
) -> Result<BuildPlan, &'static str> {
    let facts = TruthFacts::new(auth, rules, player);
    site(&facts, rules, &Builder::of(auth, player), tile, improvement)
}

/// The site rule over any tile facts (see the module docs): the truth for
/// the game, a player's knowledge for the AI and the UI.
pub fn site<M: RulesModule>(
    facts: &impl TileFacts,
    rules: &CompiledRuleset<M>,
    builder: &Builder,
    tile: TileId,
    improvement: ImprovementId,
) -> Result<BuildPlan, &'static str> {
    let def = rules
        .improvements
        .get(improvement.0 as usize)
        .ok_or("unknown improvement")?;
    if !builder.techs.allows(def.required_tech) {
        return Err("improvement locked");
    }
    if def.nation.is_some_and(|nation| nation != builder.nation) {
        return Err("unique to another nation");
    }
    if facts.city(tile) {
        return Err("city tile");
    }
    // Whether a natural wonder allows improvements is the data's rule.
    let wonder = facts.natural_wonder(tile).and_then(|w| rules.natural_wonders.get(w.0 as usize));
    if wonder.is_some_and(|w| w.no_improvement) {
        return Err("natural wonder");
    }
    if facts.improvement(tile) == Some(improvement) {
        if facts.pillaged(tile) {
            return Ok(BuildPlan {
                work_turns: work_turns(REPAIR_WORK_TIME),
                work_time: REPAIR_WORK_TIME,
                removes: None,
            });
        }
        return Err("already built");
    }
    if def.placement.requires_improvement && facts.improvement(tile).is_none() {
        return Err("needs an improvement to replace");
    }
    if facts.is_water(tile) != def.placement.water {
        return Err(if def.placement.water {
            "needs water"
        } else {
            "water"
        });
    }
    check_ownership(facts, builder.player, tile, def)?;
    let removes = check_feature(facts, rules, tile, def, &builder.techs)?;
    let connects = check_resource(facts, rules, tile, improvement, def)?;
    check_terrain(facts, rules, tile, def, connects)?;
    check_neighbourhood(facts, rules, tile, improvement, def)?;
    let removal_time = match removes {
        Some(feature) => removal_of(rules, feature).map(|r| r.work_time).unwrap_or(0),
        None => 0,
    };
    Ok(BuildPlan {
        work_turns: work_turns(def.work_time + removal_time),
        work_time: def.work_time + removal_time,
        removes,
    })
}

fn check_ownership<M: RulesModule>(
    facts: &impl TileFacts,
    player: PlayerId,
    tile: TileId,
    def: &ImprovementDefinition<M>,
) -> Result<(), &'static str> {
    let owner = facts.owner(tile);
    if def.placement.only_host_land {
        return match owner {
            Some(host) if facts.is_host(host) => Ok(()),
            _ => Err("only on a city-state's land"),
        };
    }
    if def.placement.ignore_ownership {
        return Ok(());
    }
    if owner == Some(player) || def.placement.outside_borders && owner.is_none() {
        return Ok(());
    }
    if def.placement.in_adjacent_friendly && owner.is_none() {
        let next_to_own = facts
            .dimensions()
            .neighbors(tile)
            .into_iter()
            .any(|n| facts.owner(n) == Some(player));
        if next_to_own {
            return Ok(());
        }
    }
    Err("not your land")
}

/// The feature the build removes (None: no feature or a valid one).
fn check_feature<M: RulesModule>(
    facts: &impl TileFacts,
    rules: &CompiledRuleset<M>,
    tile: TileId,
    def: &ImprovementDefinition<M>,
    techs: &TechSet,
) -> Result<Option<FeatureId>, &'static str> {
    let Some(feature) = facts.feature(tile) else {
        if def.placement.requires_feature {
            return Err("needs a feature");
        }
        return Ok(None);
    };
    let fdef = rules
        .features
        .get(feature.0 as usize)
        .ok_or("unknown feature")?;
    if def.valid_features.contains(&feature) {
        return Ok(None);
    }
    if def.placement.requires_feature || fdef.no_improvement {
        return Err("feature does not allow it");
    }
    match fdef.removal {
        Some(removal) if techs.allows(removal.tech) => Ok(Some(feature)),
        Some(_) => Err("removing the feature is locked"),
        None => Err("feature cannot be removed"),
    }
}

/// Whether the improvement connects the tile's revealed resource; a
/// resource it does not connect forbids it unless buildable_on_resources.
fn check_resource<M: RulesModule>(
    facts: &impl TileFacts,
    rules: &CompiledRuleset<M>,
    tile: TileId,
    improvement: ImprovementId,
    def: &ImprovementDefinition<M>,
) -> Result<bool, &'static str> {
    let Some(resource) = facts.resource(tile) else {
        return Ok(false);
    };
    let connects = def.resources.contains(&resource)
        || rules
            .resources
            .get(resource.0 as usize)
            .map(|r| r.improvements.contains(&improvement))
            .unwrap_or(false);
    if connects || def.placement.buildable_on_resources {
        return Ok(connects);
    }
    Err("another resource is here")
}

fn check_terrain<M: RulesModule>(
    facts: &impl TileFacts,
    rules: &CompiledRuleset<M>,
    tile: TileId,
    def: &ImprovementDefinition<M>,
    connects: bool,
) -> Result<(), &'static str> {
    let hills = facts.terrain(tile).is_some_and(|t| rules.terrain_kinds.is_hills(t.0));
    let fresh = fresh_water(facts, rules, tile);
    if def.placement.requires_flatlands_or_fresh_water && hills && !fresh {
        return Err("hills need fresh water");
    }
    let terrain_listed = facts
        .terrain(tile)
        .and_then(|t| rules.terrain_kind_of(t))
        .map(|kind| def.valid_terrains.contains(&kind))
        .unwrap_or(false);
    let feature_listed = facts
        .feature(tile)
        .map(|f| def.valid_features.contains(&f))
        .unwrap_or(false);
    let unrestricted = def.valid_terrains.is_empty()
        && def.valid_features.is_empty()
        && def.resources.is_empty()
        && def.placement.site_flags() == Default::default();
    let valid = unrestricted
        || terrain_listed
        || feature_listed
        || connects
        || (def.placement.hills_makes_valid && hills)
        || (def.placement.fresh_water_makes_valid && fresh)
        || (def.placement.water && facts.is_water(tile) && def.resources.is_empty());
    if valid {
        Ok(())
    } else {
        Err("wrong terrain")
    }
}

fn check_neighbourhood<M: RulesModule>(
    facts: &impl TileFacts,
    rules: &CompiledRuleset<M>,
    tile: TileId,
    improvement: ImprovementId,
    def: &ImprovementDefinition<M>,
) -> Result<(), &'static str> {
    if def.placement.coastal && !facts.coastal_land(tile) {
        return Err("must be on the coast");
    }
    let neighbours = facts.dimensions().neighbors(tile);
    if def.placement.no_two_adjacent {
        let twin = neighbours
            .into_iter()
            .any(|n| facts.improvement(n) == Some(improvement));
        if twin {
            return Err("next to the same improvement");
        }
    }
    if def.placement.adjacent_luxury {
        let luxury = neighbours.into_iter().any(|n| {
            let class = facts
                .resource(n)
                .and_then(|r| rules.resources.get(r.0 as usize))
                .and_then(|r| r.class);
            class == Some(ResourceClass::Luxury)
        });
        if !luxury {
            return Err("needs a luxury next to it");
        }
    }
    Ok(())
}

pub(crate) fn removal_of<M: RulesModule>(rules: &CompiledRuleset<M>, feature: FeatureId) -> Option<FeatureRemoval> {
    rules.features.get(feature.0 as usize)?.removal
}

/// Can `player` remove the feature of `tile` now (the truth)?
pub fn removal_site<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    tile: TileId,
) -> Result<(FeatureId, FeatureRemoval), &'static str> {
    let facts = TruthFacts::new(auth, rules, player);
    removal(&facts, rules, &facts.techs, tile)
}

/// The removal rule over any tile facts: land, a removable feature, its
/// technology.
pub fn removal<M: RulesModule>(
    facts: &impl TileFacts,
    rules: &CompiledRuleset<M>,
    techs: &TechSet,
    tile: TileId,
) -> Result<(FeatureId, FeatureRemoval), &'static str> {
    if facts.is_water(tile) {
        return Err("water");
    }
    let feature = facts.feature(tile).ok_or("no feature")?;
    let removal = removal_of(rules, feature).ok_or("feature cannot be removed")?;
    if !techs.allows(removal.tech) {
        return Err("removing the feature is locked");
    }
    Ok((feature, removal))
}

/// Where chop production goes and how much (see the module docs): the
/// tile's own city, else the nearest city of `owner`; nothing off own land.
pub fn chop_production<M: RulesModule>(
    auth: &AuthWorld<M>,
    owner: PlayerId,
    tile: TileId,
    production: u32,
) -> Option<(CityHandle, u32)> {
    if auth.map().owner(tile).flatten() != Some(owner) {
        return None;
    }
    let city = match auth.map().owning_city(tile) {
        Some(city) => city,
        None => nearest_city(auth, owner, tile)?,
    };
    let dims = auth.map().dimensions;
    let core = auth.cities().get(city)?;
    let distance = dims.distance(tile, core.tile)?;
    let beyond = (distance - CHOP_FULL_DISTANCE).max(0) as u32;
    let amount = production.saturating_sub(beyond * CHOP_FALLOFF_PER_TILE);
    (amount > 0).then_some((city, amount))
}

/// The owner's nearest city (tie: the smaller slot, so the traversal is deterministic).
fn nearest_city<M: RulesModule>(auth: &AuthWorld<M>, owner: PlayerId, tile: TileId) -> Option<CityHandle> {
    let dims = auth.map().dimensions;
    auth.indexes()
        .cities_of(owner)
        .iter()
        .filter_map(|h| {
            let core = auth.cities().get(*h)?;
            let d = dims.distance(tile, core.tile)?;
            Some(((d, h.index), *h))
        })
        .min_by_key(|(k, _)| *k)
        .map(|(_, h)| h)
}
