//! Cost of a land step (§33 platform/project.md, §18 platform/structure.md), (classic/docs/classic/rules.md, Standard speed).
//!
//! Layers: static constraints (mountains are impassable for land units,
//! water needs embarkation: shallow water with one technology, deep water
//! with another), slow-changing terrain, features, hills, rivers and roads
//! (here), fast dynamic units and zones of control (the path search).
//!
//! One pure function prices a step over its facts (`step_price`): the rules
//! give it the true facts (`step_cost_for`), the AI the facts its player
//! knows (`PlayerView::known_step_cost_as`), so both follow every rule.
//!
//! Move points: MOVE_DENOMINATOR (60) per movement point; a unit may enter a
//! tile while it has any points left (the partial-move rule, §7.3).

use crate::module::RulesModule;
use crate::ids::{PlayerId, TerrainId, TileId};
use crate::navigation::profile::{Embark, MoveProfile};
use crate::rules::content::Domain;
use crate::rules::roles::{Ground, TerrainPalette};
use crate::rules::effects::{Condition, UnitAbility};
use crate::rules::CompiledRuleset;
use crate::systems::ModuleSystems;
use crate::world::state::AuthWorld;
use crate::world::unit::MOVE_DENOMINATOR;

/// What each map terrain code is, read from the rules' terrain table
/// (`TerrainDefinition::map_code`, `water`, `impassable`) and the grounds
/// its terrains play (`rules::roles`). Rules without a terrain table (test
/// fixtures) get `LEGACY`: the codes of `TerrainPalette::LEGACY`, water and
/// impassable mountains. Bit `c` of each mask describes code `c`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainKinds {
    known: u64,
    water: u64,
    impassable: u64,
    hills: u64,
    mountains: u64,
    /// Features that raise their tile against sight (sight level > 0:
    /// forest, jungle); bit `f` describes `FeatureId(f)`.
    sight_features: u64,
    palette: TerrainPalette,
}

impl Default for TerrainKinds {
    fn default() -> Self {
        Self::LEGACY
    }
}

impl TerrainKinds {
    pub const LEGACY: Self = {
        let p = TerrainPalette::LEGACY;
        let mountains = 1 << p.code(Ground::Mountains);
        Self {
            known: (1 << Ground::ALL.len()) - 1,
            water: 1 << p.code(Ground::Water),
            impassable: mountains,
            hills: 1 << p.code(Ground::Hills),
            mountains,
            sight_features: 0,
            palette: p,
        }
    };

    pub fn from_rules<M: RulesModule>(rules: &CompiledRuleset<M>) -> Self {
        let sight_features = rules
            .features
            .iter()
            .enumerate()
            .take(64)
            .filter(|(_, f)| f.see_through > 0)
            .fold(0u64, |mask, (i, _)| mask | 1 << i);
        let palette = rules.roles.palette(rules);
        let mut kinds = Self { known: 0, water: 0, impassable: 0, hills: 0, mountains: 0, sight_features, palette };
        for def in &rules.terrains {
            let Some(code) = def.map_code.filter(|c| *c < 64) else {
                continue;
            };
            kinds.known |= 1 << code;
            kinds.water |= u64::from(def.water) << code;
            kinds.impassable |= u64::from(def.impassable) << code;
        }
        if kinds.known == 0 {
            return Self { sight_features, ..Self::LEGACY };
        }
        kinds.hills = 1 << palette.code(Ground::Hills);
        kinds.mountains = 1 << palette.code(Ground::Mountains);
        kinds
    }

    /// Does the feature raise its tile against sight (forest, jungle)?
    pub(crate) fn feature_hides(self, feature: crate::ids::FeatureId) -> bool {
        feature.0 < 64 && self.sight_features >> feature.0 & 1 == 1
    }

    /// Is `code` a terrain of the rules (a save may hold only those)?
    pub fn knows(self, code: u16) -> bool {
        code < 64 && self.known >> code & 1 == 1
    }

    pub fn is_water(self, code: u16) -> bool {
        code < 64 && self.water >> code & 1 == 1
    }

    pub fn is_hills(self, code: u16) -> bool {
        code < 64 && self.hills >> code & 1 == 1
    }

    pub fn is_mountains(self, code: u16) -> bool {
        code < 64 && self.mountains >> code & 1 == 1
    }

    /// The map code of every ground (what the generator writes).
    pub fn palette(self) -> TerrainPalette {
        self.palette
    }

    /// Whole movement points for a land unit to enter a terrain, hills
    /// extra included; None = water or impassable (or not a terrain).
    pub fn enter_cost(self, terrain: TerrainId) -> Option<u32> {
        let code = terrain.0;
        if !self.knows(code) || self.is_water(code) || self.impassable >> code & 1 == 1 {
            return None;
        }
        Some(if self.is_hills(code) { 1 + u32::from(HILLS_EXTRA_MOVEMENT) } else { 1 })
    }
}

/// Extra movement points to enter hills (not a hill city): hills cost 2. Unciv:
/// `Terrains.json` (Hill `movementCost` 2), `MovementCost.kt` (a city centre
/// costs 1); equal.
pub const HILLS_EXTRA_MOVEMENT: u16 = 1;

/// Whole movement points to enter a mountain for a unit that crosses them
/// (the terrain row of the catalog holds the plot's graphical 1; the plot
/// itself is the slowest ground).
pub const MOUNTAIN_MOVEMENT: u32 = 3;

/// Road step in move points (30/60 = half a point); the owner's technologies
/// change it (`RouteRules`: a third after the road-speed technology, 20/60).
/// Unciv: `logic/map/tile/RoadStatus.kt` (Road movement 0.5, improved 1/3),
/// `MovementCost.kt` (a river crossing takes the whole turn unless roads
/// connect across rivers); equal. Railroads have no movement rule of their
/// own yet: they move as roads (Unciv's railroad is faster; not adopted).
pub const ROAD_MOVE_POINTS: u32 = 30;

/// How a player's roads move (rules.md): Engineering's
/// bridges (`bridge_building`: a road step across a river is a road step)
/// and the road's move points after Machinery (`road_move_change`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteRules {
    pub bridges: bool,
    /// Move points of a step along a road (at least 1).
    pub road_points: u32,
}

impl Default for RouteRules {
    fn default() -> Self {
        RouteRules::BASE
    }
}

impl RouteRules {
    /// Before any technology.
    pub const BASE: RouteRules = RouteRules {
        bridges: false,
        road_points: ROAD_MOVE_POINTS,
    };

    /// The rules of `player`'s known technologies.
    pub fn of_player<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, player: PlayerId) -> Self {
        let mut bridges = false;
        let mut change = 0i32;
        for tech in auth.players().techs_of(player) {
            let Some(def) = rules.technologies.get(tech.0 as usize) else {
                continue;
            };
            bridges |= def.bridge_building;
            change += i32::from(def.road_move_change);
        }
        let points = (ROAD_MOVE_POINTS as i32 + change).max(1) as u32;
        RouteRules {
            bridges,
            road_points: points,
        }
    }
}

/// The cheapest possible step anywhere on `map` for a mover by `profile`:
/// the admissible A* estimate per hex. A road step needs two route tiles
/// side by side (the mover's own roads on its land count: `RoadInFriendlyLand`);
/// two cities are never adjacent (MIN_CITY_RANGE), so without a road every
/// step costs a whole point and every partial move is a multiple of one.
pub(crate) fn min_step_points<M: RulesModule>(map: &crate::world::map::MapState, profile: &MoveProfile<M>) -> u32 {
    if map.has_roads() || profile.has_road_rules() {
        profile.routes.road_points.min(u32::from(MOVE_DENOMINATOR))
    } else {
        u32::from(MOVE_DENOMINATOR)
    }
}

/// Price of one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepCost {
    /// Move points (MOVE_DENOMINATOR per movement point).
    Points(u32),
    /// The step takes all moves left (river crossing without a bridge,
    /// zone of control — rules.md).
    AllMoves,
}

/// Terrain part of entering a tile (passability by terrain only, from the
/// map's terrain table); None = off the map or impassable terrain.
pub fn enter_cost_on_map(map: &crate::world::map::MapState, tile: TileId) -> Option<u32> {
    map.terrain(tile).and_then(|t| map.terrain_kinds().enter_cost(t))
}

/// Cost of the step `from → to` for a plain land unit of `player` (the
/// default profile), before units and zones of control.
pub fn step_cost<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    from: TileId,
    to: TileId,
) -> Option<StepCost> {
    let profile = MoveProfile::land(RouteRules::of_player(auth, rules, player));
    step_cost_for(auth, rules, player, &profile, from, to)
}

/// Cost of the step `from → to` for a unit of `player` moving by
/// `profile`, before units and zones of control: the kernel's price
/// through the module's (`ModuleSystems::step_cost`). None: the unit may
/// not enter `to`.
pub fn step_cost_for<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    profile: &MoveProfile<M>,
    from: TileId,
    to: TileId,
) -> Option<StepCost> {
    let kernel = kernel_step_cost(auth, rules, player, profile, from, to);
    M::Systems::step_cost(auth, rules, player, profile, from, to, kernel)
}

/// The kernel's price of a step (classic/docs/classic/rules.md):
/// `step_price` over the true facts of the step.
fn kernel_step_cost<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    profile: &MoveProfile<M>,
    from: TileId,
    to: TileId,
) -> Option<StepCost> {
    if profile.immobile {
        return None;
    }
    let facts = step_facts(auth, player, from, to)?;
    let techs = auth.players().techs_of(player);
    step_price(rules, auth.map().terrain_kinds(), player, techs, profile, &facts)
}

/// The true facts of the step `from → to` for `player`.
fn step_facts<M: RulesModule>(auth: &AuthWorld<M>, player: PlayerId, from: TileId, to: TileId) -> Option<StepFacts> {
    let map = auth.map();
    Some(StepFacts {
        terrain: map.terrain(to)?,
        feature: map.feature(to).flatten(),
        natural_wonder: map.natural_wonder(to),
        city_owner: crate::knowledge::view::city_owner_on(auth, to),
        owner: map.owner(to).flatten(),
        may_enter: crate::world::territory::may_enter(auth, player, to),
        river_between: map.river_between(from, to),
        route_from: has_route(auth, from),
        route_to: has_route(auth, to),
        feature_from: map.feature(from).flatten(),
        owner_from: map.owner(from).flatten(),
        water_from: map.terrain(from).is_some_and(|t| map.terrain_kinds().is_water(t.0)),
        deep: crate::navigation::terrain::is_deep_water(map, to),
    })
}

/// What the price of a step `from → to` depends on: the truth for the
/// rules (`step_cost_for`), a player's knowledge for its AI
/// (`PlayerView::known_step_cost_as`). Both price it with `step_price`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepFacts {
    /// Terrain of the target tile.
    pub terrain: TerrainId,
    /// Feature of the target tile.
    pub feature: Option<crate::ids::FeatureId>,
    /// Natural wonder of the target tile.
    pub natural_wonder: Option<crate::rules::content::NaturalWonderId>,
    /// Owner of a city standing on the target tile.
    pub city_owner: Option<PlayerId>,
    /// Owner of the target tile.
    pub owner: Option<PlayerId>,
    /// The borders of the target tile let the player in (`may_enter`).
    pub may_enter: bool,
    /// A river runs along the edge between the two tiles.
    pub river_between: bool,
    /// A road or a city on the start tile.
    pub route_from: bool,
    /// A road or a city on the target tile.
    pub route_to: bool,
    /// Feature of the start tile.
    pub feature_from: Option<crate::ids::FeatureId>,
    /// Owner of the start tile.
    pub owner_from: Option<PlayerId>,
    /// The start tile is water (an embarked unit steps ashore).
    pub water_from: bool,
    /// The target tile is deep water (water with no land next to it).
    pub deep: bool,
}

/// The price of one step for a unit of `player` moving by `profile` that
/// knows `techs`, over the facts of the step (§7.2, §7.4–7.5): borders, a
/// foreign city, the unit's impassable terrain rules, then the step of its
/// domain. None: the unit may not enter the target tile.
pub fn step_price<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    kinds: TerrainKinds,
    player: PlayerId,
    techs: &[crate::ids::TechId],
    profile: &MoveProfile<M>,
    facts: &StepFacts,
) -> Option<StepCost> {
    if profile.immobile {
        return None;
    }
    let rival = profile.rival_territory && facts.owner.is_some();
    if !facts.may_enter && !rival {
        return None;
    }
    if facts.city_owner.is_some_and(|owner| owner != player) {
        return None;
    }
    if terrain_forbids(rules, kinds, profile, facts, techs) {
        return None;
    }
    match profile.domain {
        Domain::Sea => sea_step(kinds, facts),
        Domain::Air => None,
        Domain::Land => land_step(rules, kinds, player, profile, facts),
    }
}

/// A river crossing takes all moves (unless `river` is false: units that
/// ignore terrain), except along a road with bridges; a road on both tiles
/// (`road`) caps the price at the road's points.
fn river_and_roads(facts: &StepFacts, road: bool, regular: u32, river: bool, routes: RouteRules) -> StepCost {
    let bridged = road && routes.bridges;
    if river && facts.river_between && !bridged {
        return StepCost::AllMoves;
    }
    if road {
        return StepCost::Points(regular.min(routes.road_points));
    }
    StepCost::Points(regular)
}

/// May a unit moving by `profile` stand on `tile` at all (terrain only,
/// before borders and units)? Move orders check their destination with it.
pub fn destination_ok<M: RulesModule>(auth: &AuthWorld<M>, profile: &MoveProfile<M>, tile: TileId) -> bool {
    let map = auth.map();
    let Some(terrain) = map.terrain(tile) else {
        return false;
    };
    let water = map.terrain_kinds().is_water(terrain.0);
    match profile.domain {
        Domain::Air => false,
        Domain::Sea => water || crate::knowledge::view::city_owner_on(auth, tile).is_some(),
        Domain::Land if water => profile.hover || profile.embark != Embark::No,
        Domain::Land => profile.land_enter_cost(map.terrain_kinds(), terrain).is_some(),
    }
}

/// A ship sails on water and enters coastal cities (foreign ones are
/// refused before); one point per tile.
fn sea_step(kinds: TerrainKinds, facts: &StepFacts) -> Option<StepCost> {
    if kinds.is_water(facts.terrain.0) || facts.city_owner.is_some() {
        return Some(StepCost::Points(u32::from(MOVE_DENOMINATOR)));
    }
    None
}

fn land_step<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    kinds: TerrainKinds,
    player: PlayerId,
    profile: &MoveProfile<M>,
    facts: &StepFacts,
) -> Option<StepCost> {
    let water_to = kinds.is_water(facts.terrain.0);
    if water_to && !profile.hover {
        return embarked_step(profile, facts);
    }
    if facts.water_from && !water_to && !profile.hover {
        return landing_step(rules, kinds, profile, facts);
    }
    if profile.flat_cost {
        return Some(StepCost::Points(u32::from(MOVE_DENOMINATOR)));
    }
    let terrain_cost = profile.land_enter_cost(kinds, facts.terrain)?;
    let ignore_terrain = profile.ignore_terrain_cost
        || (profile.hills_ignore_terrain_cost && kinds.is_hills(facts.terrain.0));
    let regular = if ignore_terrain {
        u32::from(MOVE_DENOMINATOR)
    } else {
        let base = regular_points_for(rules, facts, terrain_cost, profile.cross_impassable)?;
        halved_if_double(rules, kinds, profile, facts, base)
    };
    if profile.rough_ends_turn && is_rough(rules, kinds, facts) {
        return Some(StepCost::AllMoves);
    }
    let regular = regular + border_obstacle_points(profile, facts, water_to);
    let road = on_road(profile, player, facts);
    Some(river_and_roads(facts, road, regular, !ignore_terrain, profile.routes))
}

/// Both tiles of the step carry a road: a built one, a city, or the
/// mover's own roads on its land (`RoadInFriendlyLand`).
fn on_road<M: RulesModule>(profile: &MoveProfile<M>, player: PlayerId, facts: &StepFacts) -> bool {
    (facts.route_from || profile.own_land_road(player, facts.feature_from, facts.owner_from))
        && (facts.route_to || profile.own_land_road(player, facts.feature, facts.owner))
}

/// An embarked unit steps ashore onto a tile it could walk onto (no
/// mountain, ice or impassable feature): landing ends the move (§7.4),
/// except for units that land for one point.
fn landing_step<M: RulesModule>(rules: &CompiledRuleset<M>, kinds: TerrainKinds, profile: &MoveProfile<M>, facts: &StepFacts) -> Option<StepCost> {
    let terrain_cost = profile.land_enter_cost(kinds, facts.terrain)?;
    regular_points_for(rules, facts, terrain_cost, profile.cross_impassable)?;
    if profile.disembark_flat {
        return Some(StepCost::Points(u32::from(MOVE_DENOMINATOR)));
    }
    Some(StepCost::AllMoves)
}

/// The Great Wall (`BorderObstacle`): one more point to enter a land tile
/// of an enemy that guards its borders (added to
/// the terrain price only, a road on both tiles still caps the step).
fn border_obstacle_points<M: RulesModule>(profile: &MoveProfile<M>, facts: &StepFacts, water: bool) -> u32 {
    if water || profile.border_obstacles.is_empty() {
        return 0;
    }
    match facts.owner {
        Some(owner) if profile.border_obstacles.contains(owner) => u32::from(MOVE_DENOMINATOR),
        _ => 0,
    }
}

/// A land unit on or into water: embarkation needs Optics, deep water
/// Astronomy; embarking ends the move, moving embarked costs a point.
fn embarked_step<M: RulesModule>(profile: &MoveProfile<M>, facts: &StepFacts) -> Option<StepCost> {
    let allowed = match profile.embark {
        Embark::No => false,
        Embark::Shallow => !facts.deep,
        Embark::All => true,
    };
    if !allowed {
        return None;
    }
    if facts.water_from {
        Some(StepCost::Points(u32::from(MOVE_DENOMINATOR)))
    } else {
        Some(StepCost::AllMoves)
    }
}

/// Hills, mountains, a rough feature or a rough natural wonder (the facts'
/// version of `navigation::terrain::is_rough`).
fn is_rough<M: RulesModule>(rules: &CompiledRuleset<M>, kinds: TerrainKinds, facts: &StepFacts) -> bool {
    let code = facts.terrain.0;
    if kinds.is_hills(code) || kinds.is_mountains(code) {
        return true;
    }
    if let Some(wonder) = facts.natural_wonder.and_then(|w| rules.natural_wonders.get(w.0 as usize)) {
        return wonder.rough;
    }
    facts
        .feature
        .and_then(|f| rules.features.get(f.0 as usize))
        .is_some_and(|def| def.rough)
}

/// Hills with HillsDoubleMove, or a DoubleMove rule of the entered tile:
/// half the cost (§7.2).
fn halved_if_double<M: RulesModule>(rules: &CompiledRuleset<M>, kinds: TerrainKinds, profile: &MoveProfile<M>, facts: &StepFacts, points: u32) -> u32 {
    if !profile.hills_double_move && profile.terrain_rules().next().is_none() {
        return points;
    }
    let hills = kinds.is_hills(facts.terrain.0);
    let double_rule = profile
        .terrain_rules()
        .any(|r| r.ability == UnitAbility::DoubleMove && rule_matches(rules, kinds, r.condition, facts, &[]));
    if (hills && profile.hills_double_move) || double_rule {
        return (points / 2).max(1);
    }
    points
}

/// An Impassable rule of the unit matches the target tile.
fn terrain_forbids<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    kinds: TerrainKinds,
    profile: &MoveProfile<M>,
    facts: &StepFacts,
    techs: &[crate::ids::TechId],
) -> bool {
    profile
        .terrain_rules()
        .any(|r| r.ability == UnitAbility::Impassable && rule_matches(rules, kinds, r.condition, facts, techs))
}

/// Does a movement rule's condition hold on the target tile? (The feature
/// replaces the terrain for terrain conditions, as in combat.)
fn rule_matches<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    kinds: TerrainKinds,
    condition: Condition<M>,
    facts: &StepFacts,
    techs: &[crate::ids::TechId],
) -> bool {
    match condition {
        Condition::TileFeature(f) => facts.feature == Some(f),
        Condition::TileTerrain(t) => terrain_kind_of(rules, kinds, facts) == Some(t),
        Condition::TileTerrainUntil(t, tech) => !techs.contains(&tech) && terrain_kind_of(rules, kinds, facts) == Some(t),
        _ => false,
    }
}

/// The terrain kind of the target tile: the deep or shallow water role for
/// water (by depth), else the terrain whose `map_code` is the tile's code
/// (the facts' version of `navigation::terrain::terrain_kind`).
fn terrain_kind_of<M: RulesModule>(rules: &CompiledRuleset<M>, kinds: TerrainKinds, facts: &StepFacts) -> Option<crate::rules::content::TerrainKindId> {
    if !kinds.is_water(facts.terrain.0) {
        return rules.terrain_kind_of(facts.terrain);
    }
    if facts.deep {
        rules.roles.deep_water
    } else {
        rules.roles.ground(Ground::Water)
    }
}

/// Like `regular_points`, but impassable terrain and features are entered
/// at their (or one) cost by units that cross them.
fn regular_points_for<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    facts: &StepFacts,
    terrain_cost: u32,
    cross_impassable: bool,
) -> Option<u32> {
    match regular_points(rules, facts, terrain_cost) {
        Some(points) => Some(points),
        None if cross_impassable => Some(u32::from(MOVE_DENOMINATOR) * 2),
        None => None,
    }
}

/// Feature movement replaces the terrain's; hills add their extra point
/// unless a city stands on them. In move points.
fn regular_points<M: RulesModule>(rules: &CompiledRuleset<M>, facts: &StepFacts, terrain_cost: u32) -> Option<u32> {
    let hills = rules.terrain_kinds.is_hills(facts.terrain.0);
    let city = facts.city_owner.is_some();
    let base = match facts.feature {
        Some(feature) => {
            let movement = feature_movement(rules, feature)?;
            movement
                + if hills {
                    u32::from(HILLS_EXTRA_MOVEMENT)
                } else {
                    0
                }
        }
        None => terrain_cost,
    };
    let base = if hills && city {
        base - u32::from(HILLS_EXTRA_MOVEMENT)
    } else {
        base
    };
    Some(base * u32::from(MOVE_DENOMINATOR))
}

/// Whole movement points to enter a feature, from the ruleset (Classic
/// `Impassable`); None = impassable. A feature without
/// a movement cost in its data (the frozen v1 content) costs one point.
fn feature_movement<M: RulesModule>(rules: &CompiledRuleset<M>, feature: crate::ids::FeatureId) -> Option<u32> {
    let Some(def) = rules.features.get(feature.0 as usize) else {
        return Some(1);
    };
    if def.impassable {
        return None;
    }
    Some(u32::from(def.movement_cost.max(1)))
}

/// A road, or a city (cities count as road tiles in Classic).
fn has_route<M: RulesModule>(auth: &AuthWorld<M>, tile: TileId) -> bool {
    auth.map().road(tile) || crate::knowledge::view::city_owner_on(auth, tile).is_some()
}
