//! How one unit moves (classic/docs/classic/rules.md): its domain,
//! embarkation and the movement abilities of its promotions. A request
//! for a path carries the mover's profile; the default profile is a plain
//! land unit (the pre-M4 rules).

use crate::module::RulesModule;
use crate::effects::unit::{unit_modifiers, TerrainRule, UnitModifiers};
use crate::ids::{FeatureId, PlayerId, TerrainId, UnitKindId};
use crate::navigation::cost::{TerrainKinds, MOUNTAIN_MOVEMENT};
use crate::rules::content::Domain;
use crate::rules::effects::{Condition, UnitAbility};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;
use crate::world::unit::UnitCore;

/// Where a land unit may go on water.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Embark {
    /// Never enters water.
    #[default]
    No,
    /// Shallow water only (Optics).
    Shallow,
    /// Any water (Astronomy, or an all-water embarkation promotion).
    All,
}

/// A set of players (bit = `PlayerId`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayerSet([u64; 4]);

impl PlayerSet {
    pub fn insert(&mut self, player: PlayerId) {
        let bit = usize::from(player.0);
        self.0[bit / 64] |= 1 << (bit % 64);
    }

    pub fn contains(&self, player: PlayerId) -> bool {
        let bit = usize::from(player.0);
        self.0[bit / 64] >> (bit % 64) & 1 == 1
    }

    pub fn is_empty(&self) -> bool {
        self.0 == [0; 4]
    }
}

/// At most this many terrain rules per unit (double move, impassable).
pub const MAX_TERRAIN_RULES: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveProfile<M: RulesModule> {
    pub domain: Domain,
    pub embark: Embark,
    /// Moves only by rebasing (aircraft).
    pub immobile: bool,
    pub ignore_zone_of_control: bool,
    pub ignore_terrain_cost: bool,
    pub flat_cost: bool,
    pub rough_ends_turn: bool,
    pub hills_double_move: bool,
    /// Entering hills costs one point whatever they hold.
    pub hills_ignore_terrain_cost: bool,
    /// Enters impassable terrain (mountains, ice).
    pub cross_impassable: bool,
    /// Enters mountains, and nothing else that is impassable.
    pub cross_mountains: bool,
    /// Moves over water like land (hovercraft, helicopters).
    pub hover: bool,
    /// Enters foreign land without open borders.
    pub rival_territory: bool,
    /// Stepping ashore costs one move point, not all of them.
    pub disembark_flat: bool,
    /// Players at war with the mover whose territory costs one more point
    /// per tile (a working Great Wall; land units only).
    pub border_obstacles: PlayerSet,
    /// How the owner's roads move (bridges, road move points).
    pub routes: crate::navigation::cost::RouteRules,
    terrain_rules: [Option<TerrainRule<M>>; MAX_TERRAIN_RULES],
}

impl<M: RulesModule> MoveProfile<M> {
    /// A plain land unit without abilities.
    pub const LAND: MoveProfile<M> = MoveProfile {
        domain: Domain::Land,
        embark: Embark::No,
        immobile: false,
        ignore_zone_of_control: false,
        ignore_terrain_cost: false,
        flat_cost: false,
        rough_ends_turn: false,
        hills_double_move: false,
        hills_ignore_terrain_cost: false,
        cross_impassable: false,
        cross_mountains: false,
        hover: false,
        rival_territory: false,
        disembark_flat: false,
        border_obstacles: PlayerSet([0; 4]),
        routes: crate::navigation::cost::RouteRules::BASE,
        terrain_rules: [None; MAX_TERRAIN_RULES],
    };

    /// A plain land unit of a player whose roads move by `routes`.
    pub fn land(routes: crate::navigation::cost::RouteRules) -> Self {
        MoveProfile {
            routes,
            ..MoveProfile::LAND
        }
    }

    /// The profile of a unit now (its kind, promotions and the owner's
    /// technologies).
    pub fn of_unit(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, unit: &UnitCore) -> Self {
        let mods = unit_modifiers(auth, rules, unit);
        Self::with_modifiers(auth, rules, unit, &mods)
    }

    /// The profile of a unit whose modifiers are already collected.
    pub fn with_modifiers(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, unit: &UnitCore, mods: &UnitModifiers<M>) -> Self {
        let def = rules.units.get(unit.kind.0 as usize);
        let domain = def.map(|d| d.domain).unwrap_or_default();
        let mut profile = MoveProfile {
            domain,
            embark: Embark::No,
            immobile: def.is_some_and(|d| d.combat.immobile) || domain == Domain::Air,
            ignore_zone_of_control: mods.has(UnitAbility::IgnoreZoneOfControl),
            ignore_terrain_cost: mods.has(UnitAbility::IgnoreTerrainCost),
            flat_cost: mods.has(UnitAbility::FlatMovementCost),
            rough_ends_turn: mods.has(UnitAbility::RoughTerrainEndsTurn),
            hills_double_move: mods.has(UnitAbility::HillsDoubleMove),
            hills_ignore_terrain_cost: mods.has(UnitAbility::HillsIgnoreTerrainCost),
            cross_impassable: mods.has(UnitAbility::MoveImpassable)
                || mods.has(UnitAbility::Hovering),
            cross_mountains: mods.has(UnitAbility::CrossMountains),
            hover: mods.has(UnitAbility::Hovering),
            rival_territory: mods.has(UnitAbility::RivalTerritory),
            disembark_flat: mods.has(UnitAbility::EmbarkedToLandFlatCost),
            border_obstacles: PlayerSet::default(),
            routes: crate::navigation::cost::RouteRules::of_player(auth, rules, unit.owner),
            terrain_rules: [None; MAX_TERRAIN_RULES],
        };
        if domain == Domain::Land {
            profile.embark = embark_of(auth, rules, unit.owner, mods);
            profile.border_obstacles = border_obstacles(auth, rules, unit.owner);
        }
        for (slot, rule) in profile
            .terrain_rules
            .iter_mut()
            .zip(mods.terrain_rules.iter())
        {
            *slot = Some(*rule);
        }
        profile
    }

    /// The profile of a new unit of `kind` (path previews before training).
    pub fn of_kind(
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
        owner: PlayerId,
        kind: UnitKindId,
    ) -> Self {
        let core = UnitCore {
            id: crate::ids::EntityId(u64::MAX),
            tile: crate::ids::TileId(0),
            owner,
            kind,
            hp: crate::world::unit::UNIT_MAX_HP,
            moves_left: 0,
            moves_max: 0,
            stance: crate::world::unit::Stance::Ready,
            work: None,
            goal: None,
            promotions: crate::effects::unit::free_promotions(rules, kind),
            experience: 0,
            level: 1,
            attacks_made: 0,
            interceptions_made: 0,
        };
        Self::of_unit(auth, rules, &core)
    }

    /// A land unit that may take to the water (and does not hover): its
    /// moves per turn differ on land and embarked.
    pub fn embarks(&self) -> bool {
        self.domain == Domain::Land && !self.hover && self.embark != Embark::No
    }

    pub(crate) fn terrain_rules(&self) -> impl Iterator<Item = &TerrainRule<M>> {
        self.terrain_rules.iter().flatten()
    }

    /// Whole movement points for a land unit to enter `terrain`, the
    /// impassable terrain it crosses included; None: it may not enter.
    pub(crate) fn land_enter_cost(&self, kinds: TerrainKinds, terrain: TerrainId) -> Option<u32> {
        kinds.enter_cost(terrain).or_else(|| {
            if self.cross_impassable {
                Some(1)
            } else if self.cross_mountains && kinds.is_mountains(terrain.0) {
                Some(MOUNTAIN_MOVEMENT)
            } else {
                None
            }
        })
    }

    /// The unit has roads of its own on some land (`RoadInFriendlyLand`).
    pub(crate) fn has_road_rules(&self) -> bool {
        self.terrain_rules().any(|r| r.ability == UnitAbility::RoadInFriendlyLand)
    }

    /// A tile with `feature` and `owner` is a road for this unit of
    /// `player`: a feature its `RoadInFriendlyLand` rule names, on its own
    /// territory.
    pub(crate) fn own_land_road(&self, player: PlayerId, feature: Option<FeatureId>, owner: Option<PlayerId>) -> bool {
        owner == Some(player)
            && feature.is_some_and(|f| {
                self.terrain_rules()
                    .any(|r| r.ability == UnitAbility::RoadInFriendlyLand && r.condition == Condition::TileFeature(f))
            })
    }
}

/// The features that are roads for `player` on its own land (`RoadInFriendlyLand` granted to
/// all its units by its nation: the forests and jungles of the Iroquois). Units walk
/// them as roads (`MoveProfile::own_land_road`); city connections and trade routes read the
/// same list, so the three agree.
pub fn own_land_road_features<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, player: PlayerId) -> Vec<FeatureId> {
    crate::effects::unit::owner_unit_modifiers(auth, rules, player)
        .terrain_rules
        .iter()
        .filter_map(|rule| match (rule.ability, rule.condition) {
            (UnitAbility::RoadInFriendlyLand, Condition::TileFeature(feature)) => Some(feature),
            _ => None,
        })
        .collect()
}

/// Embarkation of a land unit of `owner`: a technology that allows it
/// (Optics) or an embarkation promotion; all water with Astronomy or an
/// all-water promotion.
fn embark_of<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    mods: &crate::effects::unit::UnitModifiers<M>,
) -> Embark {
    let techs = auth.players().techs_of(owner);
    let known = |flag: fn(&crate::rules::compiled::TechnologyDefinition<M>) -> bool| {
        techs
            .iter()
            .any(|t| rules.technologies.get(t.0 as usize).is_some_and(flag))
    };
    let embarks = mods.has(UnitAbility::Embarkation) || known(|t| t.allows_embarking);
    if !embarks {
        return Embark::No;
    }
    if mods.has(UnitAbility::EmbarkedAllWater) || known(|t| t.embarked_all_water) {
        Embark::All
    } else {
        Embark::Shallow
    }
}

/// The enemies of `mover` (at war) whose territory a building with
/// `BorderObstacle` guards: its owner has not learned the building's
/// obsolete technology (Great Wall until Dynamite). Only the enemies'
/// cities are looked at, only when the ruleset has such a building.
fn border_obstacles<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, mover: PlayerId) -> PlayerSet {
    let mut set = PlayerSet::default();
    let walls = &rules.effect_index.border_obstacle_buildings;
    if walls.is_empty() {
        return set;
    }
    for player in 0..auth.players().count() {
        let enemy = PlayerId(player as u8);
        if enemy != mover && auth.diplomacy().at_war(mover, enemy) && guards_borders(auth, rules, enemy) {
            set.insert(enemy);
        }
    }
    set
}

/// Does a city of `owner` hold a working border obstacle?
fn guards_borders<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, owner: PlayerId) -> bool {
    let techs = auth.players().techs_of(owner);
    for city in auth.indexes().cities_of(owner) {
        let Some(core) = auth.cities().get(*city) else {
            continue;
        };
        for wall in &rules.effect_index.border_obstacle_buildings {
            let obsolete = rules.buildings[wall.0 as usize].obsolete_tech.is_some_and(|t| techs.contains(&t));
            if !obsolete && core.buildings.contains(wall) {
                return true;
            }
        }
    }
    false
}
