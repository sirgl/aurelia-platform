//! PlayerView: the map as one player knows it (platform/structure.md §21–22).
//!
//! Every tile is Unknown, Remembered (what the player last saw, and when)
//! or Visible (the truth now). A resource shows only when the player knows
//! the technology that reveals it. This is the one filter between the truth
//! and the AI, the render projection and the HUD: the type keeps the world
//! private and answers only for the player's own knowledge.
//!
//! Cities are remembered as the player last saw them (`CitySighting`: name,
//! owner, population, era, buildings); a resource as the tile last showed it.
//! Neither is read from the world while its tile is out of sight.
//!
//! Units are seen, never remembered: another player's unit is known while
//! its tile is visible. The rules module's state of a unit
//! (`world::module::ModuleUnit`) follows the same rule: the owner reads
//! all of it, another player only its `Seen` part and only while it sees
//! the unit. Nothing of it enters the player's knowledge or its save.

use crate::module::RulesModule;
use crate::systems::ModuleSystems;
use crate::derived::DerivedWorld;
use crate::ids::{CityHandle, FeatureId, ImprovementId, PlayerId, ProductionItem, ResourceId, TerrainId, TileId};
use crate::knowledge::state::{CitySighting, Contact, RememberedLayers, SightedCity};
use crate::navigation::cost::{step_price, StepCost, StepFacts};
use crate::navigation::MoveProfile;
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use crate::world::module::{ModuleUnit, ModuleUnitOf, SeenUnitOf};
use crate::world::map::Route;
use crate::world::state::AuthWorld;

/// A tile as the player knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileSnapshot {
    pub terrain: TerrainId,
    pub feature: Option<FeatureId>,
    /// Only a resource the player can see (its reveal technology known).
    pub resource: Option<ResourceId>,
    pub improvement: Option<ImprovementId>,
    pub route: crate::world::map::Route,
    pub river: bool,
    /// Natural wonders never move: read from the map like terrain.
    pub natural_wonder: Option<crate::rules::content::NaturalWonderId>,
    pub owner: Option<PlayerId>,
    pub city: bool,
}

/// Geography of a known tile (static in a game).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Geography {
    pub water: bool,
    pub lake: bool,
    pub coastal_land: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KnownTile {
    Unknown,
    Remembered {
        snapshot: TileSnapshot,
        last_seen_round: u32,
    },
    Visible(TileSnapshot),
}

impl KnownTile {
    pub fn snapshot(&self) -> Option<&TileSnapshot> {
        match self {
            KnownTile::Unknown => None,
            KnownTile::Remembered { snapshot, .. } | KnownTile::Visible(snapshot) => Some(snapshot),
        }
    }

    pub fn is_visible(&self) -> bool {
        matches!(self, KnownTile::Visible(_))
    }
}

/// The key to `PlayerView::world`. A rules module implements it for a unit
/// type that only its own sensor code can name (`pub(in crate::knowledge)`),
/// and passes a value of it where it reads the authoritative world.
pub trait SensorKey<M: RulesModule> {}

/// One player's view of the world.
#[derive(Clone, Copy)]
pub struct PlayerView<'a, M: RulesModule> {
    auth: &'a AuthWorld<M>,
    derived: &'a DerivedWorld<M>,
    rules: &'a CompiledRuleset<M>,
    player: PlayerId,
    /// How the player's roads move (its technologies), for step prices.
    routes: crate::navigation::cost::RouteRules,
    /// A spectator's view with the whole map open (`PlayerView::revealed`): every tile
    /// and unit is seen now. Never set for a player of the match.
    all_seen: bool,
}

impl<M: RulesModule> std::fmt::Debug for PlayerView<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PlayerView({:?})", self.player)
    }
}

impl<'a, M: RulesModule> PlayerView<'a, M> {
    pub fn new(
        auth: &'a AuthWorld<M>,
        derived: &'a DerivedWorld<M>,
        rules: &'a CompiledRuleset<M>,
        player: PlayerId,
    ) -> Self {
        Self {
            auth,
            derived,
            rules,
            player,
            routes: crate::navigation::cost::RouteRules::of_player(auth, rules, player),
            all_seen: false,
        }
    }

    /// The same view with the whole map open, for a spectator who watches the match
    /// through `player`'s eyes and may look anywhere: every tile is visible now, every
    /// unit and city is seen. The match never reads this view (the AI and the HUD keep
    /// the filter); it only draws what the spectator asks to see.
    pub fn revealed(mut self) -> Self {
        self.all_seen = true;
        self
    }

    pub fn player(&self) -> PlayerId {
        self.player
    }

    /// A spectator's view with the whole map open (`revealed`).
    pub fn is_revealed(&self) -> bool {
        self.all_seen
    }

    /// The authoritative world behind the view, for the SENSORS of a rules
    /// module (the public queries and the observation builders). The `key`
    /// is a type only the sensor code of the module can name: the module
    /// implements `SensorKey` for a type that is private to its knowledge
    /// module, so the planners (which hold a view) cannot call this. That is
    /// how "the AI sees the world only through its observation" is held by
    /// types, not by discipline (classic/docs/ai-truth-reads.md).
    pub fn world<K: SensorKey<M>>(&self, _key: K) -> crate::world::state::GameView<'a, M> {
        self.game_view()
    }

    /// The same, for the kernel itself.
    pub(crate) fn game_view(&self) -> crate::world::state::GameView<'a, M> {
        crate::world::state::GameView {
            auth: self.auth,
            derived: self.derived,
        }
    }

    pub fn rules(&self) -> &'a CompiledRuleset<M> {
        self.rules
    }

    pub fn tile_count(&self) -> usize {
        self.auth.map().tile_count()
    }

    /// Map size and topology (public: every player knows the map size).
    pub fn dimensions(&self) -> crate::math::MapDimensions {
        self.auth.map().dimensions
    }

    /// The game's speed (public: every player knows the game's setup).
    pub fn speed(&self) -> crate::rules::speeds::SpeedValues {
        self.auth.settings().speed(self.rules)
    }

    /// What the map's terrain codes are (water, impassable): the rules'
    /// terrain table, the same for every player.
    pub fn terrain_kinds(&self) -> crate::navigation::cost::TerrainKinds {
        self.auth.map().terrain_kinds()
    }

    pub fn is_visible(&self, tile: TileId) -> bool {
        self.all_seen || is_visible(self.derived, self.player, tile)
    }

    /// The player's own era (0 = the first; its own technologies).
    pub fn own_era(&self) -> u8 {
        crate::world::player::player_era(self.auth, self.rules, self.player)
    }

    /// Does the player see this unit (`unit_seen_by`: invisible units only
    /// when revealed)?
    pub fn sees_unit(&self, core: &crate::world::unit::UnitCore) -> bool {
        self.all_seen || unit_seen_by(self.auth, self.derived, self.rules, self.player, core)
    }

    pub fn tile(&self, tile: TileId) -> KnownTile {
        if self.all_seen && tile.0 < self.auth.map().tile_count() as u32 {
            return KnownTile::Visible(self.current_snapshot(tile));
        }
        let Some(knowledge) = self.auth.knowledge().get(self.player.0 as usize) else {
            return KnownTile::Unknown;
        };
        if self.is_visible(tile) {
            return KnownTile::Visible(self.current_snapshot(tile));
        }
        match knowledge.memory(tile) {
            None => KnownTile::Unknown,
            Some(memory) => {
                // The deposit of an explored tile is read from the map, not from the
                // memory: a technology (or an event) that reveals one shows it on all
                // explored land at once. The other layers are the last observation.
                let map = self.auth.map();
                let now = map.layers(tile);
                let layers = RememberedLayers {
                    resource: now.and_then(|l| l.resource),
                    ..memory.layers()
                };
                KnownTile::Remembered {
                    snapshot: self.snapshot(now, layers),
                    last_seen_round: memory.seen_round(),
                }
            }
        }
    }

    /// The tile as it is now (the layers of the map, one lookup).
    fn current_snapshot(&self, tile: TileId) -> TileSnapshot {
        let now = self.auth.map().layers(tile);
        let layers = RememberedLayers {
            improvement: now.and_then(|l| l.improvement),
            feature: now.and_then(|l| l.feature),
            resource: now.and_then(|l| l.resource),
            owner: now.and_then(|l| l.owner),
            route: now.map_or(Route::None, |l| l.route),
            city: city_owner_on(self.auth, tile).is_some(),
        };
        self.snapshot(now, layers)
    }

    /// The cities the player knows but does not see now, each as last seen
    /// (a spectator who sees everything has none), by tile, ascending.
    pub fn remembered_cities(&self) -> impl Iterator<Item = (TileId, &'a CitySighting)> + '_ {
        let sightings = if self.all_seen {
            None
        } else {
            self.auth.knowledge().get(self.player.0 as usize)
        };
        sightings
            .into_iter()
            .flat_map(|knowledge| knowledge.sightings())
            .filter(|(tile, _)| !self.is_visible(*tile))
    }

    /// The improvement of a tile the player sees now is pillaged (tile
    /// memory keeps no pillage: a remembered tile reads intact).
    pub fn known_pillaged(&self, tile: TileId) -> bool {
        self.is_visible(tile) && self.auth.map().pillaged(tile)
    }

    /// Map size (public knowledge).
    pub fn map_dimensions(&self) -> crate::math::MapDimensions {
        self.auth.map().dimensions
    }

    /// Static geography of a tile the player knows (None: unknown tile).
    pub(crate) fn known_geography(&self, tile: TileId) -> Option<Geography> {
        self.tile(tile).snapshot()?;
        let map = self.auth.map();
        Some(Geography {
            water: map.is_water(tile),
            lake: map.lake(tile),
            coastal_land: map.coastal_land(tile),
        })
    }

    /// The tile was seen at least once (Visible or Remembered).
    pub fn is_known(&self, tile: TileId) -> bool {
        if self.all_seen {
            return true;
        }
        match self.auth.knowledge().get(self.player.0 as usize) {
            Some(knowledge) => knowledge.is_explored(tile),
            None => false,
        }
    }

    /// How the player's roads move (its technologies; public).
    pub fn routes(&self) -> crate::navigation::cost::RouteRules {
        self.routes
    }

    /// Price of the step `from → to` for a unit moving by `profile` as the
    /// player knows the two tiles, through the module's price
    /// (`ModuleSystems::step_cost`). None when a tile is unknown or the unit
    /// may not enter `to` as far as the player knows. Units and zones of
    /// control are not part of it: the mover sees those itself.
    pub fn known_step_cost_as(&self, profile: &MoveProfile<M>, from: TileId, to: TileId) -> Option<StepCost> {
        let kernel = self.known_kernel_step_cost(profile, from, to);
        M::Systems::step_cost(self.auth, self.rules, self.player, profile, from, to, kernel)
    }

    /// The kernel's price (`navigation::cost::step_price`, the rule the
    /// units move by) over the facts the player knows. A land unit that
    /// never embarks is priced as a plain land unit (other than its access
    /// to rival territory), the estimate its player's AI has always made,
    /// unless the module prices by profile (`ai_prices_by_profile`). Ships
    /// and units that take to the water are priced by their whole profile.
    fn known_kernel_step_cost(&self, profile: &MoveProfile<M>, from: TileId, to: TileId) -> Option<StepCost> {
        let facts = self.known_step_facts(from, to)?;
        let techs = self.auth.players().techs_of(self.player);
        let plain;
        let profile = if profile.domain == Domain::Land && !profile.embarks() && !M::Systems::ai_prices_by_profile() {
            let mut land = MoveProfile::land(self.routes);
            land.rival_territory = profile.rival_territory;
            plain = land;
            &plain
        } else {
            profile
        };
        step_price(self.rules, self.auth.map().terrain_kinds(), self.player, techs, profile, &facts)
    }

    /// The facts of a step from the player's snapshots of the two tiles and
    /// the public diplomacy (None: a tile is unknown). A known water tile
    /// tells its depth: coast and deep water look different.
    fn known_step_facts(&self, from: TileId, to: TileId) -> Option<StepFacts> {
        let start = *self.tile(from).snapshot()?;
        let target = *self.tile(to).snapshot()?;
        let map = self.auth.map();
        let route = |s: &TileSnapshot| s.route != crate::world::map::Route::None || s.city;
        Some(StepFacts {
            terrain: target.terrain,
            feature: target.feature,
            natural_wonder: target.natural_wonder,
            city_owner: if target.city { target.owner } else { None },
            owner: target.owner,
            may_enter: self.may_enter_known(target.owner),
            river_between: map.river_between(from, to),
            route_from: route(&start),
            route_to: route(&target),
            feature_from: start.feature,
            owner_from: start.owner,
            water_from: map.terrain_kinds().is_water(start.terrain.0),
            deep: crate::navigation::terrain::is_deep_water(map, to),
        })
    }

    /// The movement profile of one of the player's own units (its kind,
    /// promotions and the player's technologies); None for other units.
    pub fn own_unit_profile(&self, unit: crate::ids::UnitHandle) -> Option<MoveProfile<M>> {
        let core = self.auth.units().get(unit)?.core;
        (core.owner == self.player).then(|| MoveProfile::of_unit(self.auth, self.rules, core))
    }

    /// The rules module's state of one of the player's own units (None for
    /// another player's unit: see [`Self::unit_state_seen`]).
    pub fn own_unit_state(&self, unit: crate::ids::UnitHandle) -> Option<&'a ModuleUnitOf<M>> {
        let units = self.auth.units();
        let owner = units.get(unit)?.core.owner;
        if owner != self.player {
            return None;
        }
        units.module_of(unit)
    }

    /// What the player sees of a unit's module state (`ModuleUnit::seen`):
    /// of its own units always, of another player's unit only while the
    /// unit stands on a tile the player sees now (None out of sight).
    pub fn unit_state_seen(&self, unit: crate::ids::UnitHandle) -> Option<SeenUnitOf<M>> {
        let units = self.auth.units();
        let core = units.get(unit)?.core;
        if core.owner != self.player && !self.sees_unit(core) {
            return None;
        }
        units.module_of(unit).map(ModuleUnit::seen)
    }

    /// Tiles one of the player's own units reaches this turn and the move
    /// points left there (its tile included), by tile; empty for other
    /// units. The move preview of the interface: the path rules of a move
    /// order over what the player sees.
    pub fn own_reachable(&self, unit: crate::ids::UnitHandle) -> Vec<(TileId, u16)> {
        let Some(core) = self.auth.units().get(unit).map(|u| *u.core) else {
            return Vec::new();
        };
        if core.owner != self.player {
            return Vec::new();
        }
        let Some(request) = crate::navigation::PathRequest::for_unit(self.auth, self.rules, unit, core.tile) else {
            return Vec::new();
        };
        crate::navigation::reachable_this_turn(self.game_view(), self.rules, &request)
    }

    /// The player's capital (its palace city).
    pub fn own_capital(&self) -> Option<CityHandle> {
        crate::world::city::capital_of(self.auth, self.rules, self.player)
    }

    /// The player's cities its road network (or harbors) links to its
    /// capital, the capital included (the city connections of its gold).
    pub fn own_connected_cities(&self) -> std::sync::Arc<Vec<CityHandle>> {
        let Some(capital) = self.own_capital() else {
            return std::sync::Arc::new(Vec::new());
        };
        crate::derived::connections::connected_cities(self.auth, self.rules, self.player, capital)
    }

    /// The movement profile of a new unit of `kind` of the player.
    pub(crate) fn own_kind_profile(&self, kind: crate::ids::UnitKindId) -> MoveProfile<M> {
        MoveProfile::of_kind(self.auth, self.rules, self.player, kind)
    }

    /// Borders as the player knows them: own and unowned land always,
    /// foreign land by the module's border rule (`ModuleSystems::may_enter`,
    /// over public diplomacy).
    fn may_enter_known(&self, owner: Option<PlayerId>) -> bool {
        let Some(host) = owner else {
            return true;
        };
        host == self.player || M::Systems::may_enter(self.auth, self.player, host)
    }

    /// The building rules (`world::building_rules`) for one of the
    /// player's own cities (its own state; Err for another player's city).
    pub fn own_building_site(&self, city: CityHandle, kind: crate::ids::BuildingKindId) -> Result<(), &'static str> {
        let core = self.auth.cities().get(city).ok_or("unknown city")?;
        if core.owner != self.player {
            return Err("not your city");
        }
        crate::world::building_rules::building_site(self.auth, self.rules, city, kind)
    }

    /// Production cost of `item` in one of the player's own cities: the
    /// price the city's production pays (cost modifiers included). None for
    /// another player's city or an unknown item.
    pub fn own_production_cost(&self, city: CityHandle, item: ProductionItem) -> Option<u32> {
        let core = self.auth.cities().get(city)?;
        if core.owner != self.player {
            return None;
        }
        if let ProductionItem::Unit(kind) = item {
            let settler = self.rules.units.get(kind.0 as usize).is_some_and(|d| d.founder);
            let trains_founders = self.auth.players().kind_of(self.player).module().is_none_or(|kind| {
                crate::world::player::ModulePlayerKind::trains_founders(kind)
            });
            if settler && !trains_founders {
                return None;
            }
            if crate::effects::player::unit_class_forbidden(self.auth, self.rules, self.player, kind) {
                return None;
            }
        }
        let modifiers = crate::effects::city::city_modifiers(self.auth, self.rules, core);
        modifiers.production_cost(self.rules, item)
    }

    /// Gold price to hurry the head of one of the player's own cities
    /// (`effects::city::head_hurry_price`). None for another player's city
    /// or an empty queue.
    pub fn own_hurry_price(&self, city: CityHandle) -> Option<u32> {
        let core = self.auth.cities().get(city)?;
        if core.owner != self.player {
            return None;
        }
        crate::effects::city::head_hurry_price(self.auth, self.rules, core)
    }

    /// The tiles one of the player's own cities may buy now and their gold
    /// prices, by tile (`world::territory::tile_purchase_offers`, the city
    /// screen's overlay); empty for another player's city.
    pub fn own_tile_offers(&self, city: CityHandle) -> Vec<(TileId, u32)> {
        match self.auth.cities().get(city) {
            Some(core) if core.owner == self.player => crate::world::territory::tile_purchase_offers(self.auth, self.rules, city)
                .into_iter()
                .map(|(tile, offer)| (tile, offer.price))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The additive production percent one of the player's own cities would
    /// have toward `item` (`effects::city::item_production_percent`); 0 for
    /// another player's city.
    pub fn own_item_production_percent(&self, city: CityHandle, item: ProductionItem) -> i32 {
        match self.auth.cities().get(city) {
            Some(core) if core.owner == self.player => {
                crate::effects::city::item_production_percent(self.auth, self.rules, core, item)
            }
            _ => 0,
        }
    }

    pub fn contact(&self, other: PlayerId) -> Contact {
        match self.auth.knowledge().get(self.player.0 as usize) {
            Some(knowledge) => knowledge.contact(other),
            None => Contact::default(),
        }
    }

    /// Static layers come from the map (they never change in a game), the
    /// changing ones from the given layers; the resource (which `tile` reads
    /// from the map for an explored tile) is filtered by the player's technologies.
    fn snapshot(&self, now: Option<crate::world::map::TileLayers>, layers: RememberedLayers) -> TileSnapshot {
        TileSnapshot {
            terrain: now.map_or(TerrainId(0), |l| l.terrain),
            feature: layers.feature,
            resource: layers.resource.filter(|r| self.all_seen || self.sees_resource(*r)),
            improvement: layers.improvement,
            route: layers.route,
            river: now.is_some_and(|l| l.river),
            natural_wonder: now.and_then(|l| l.natural_wonder),
            owner: layers.owner,
            city: layers.city,
        }
    }

    fn sees_resource(&self, resource: ResourceId) -> bool {
        let Some(def) = self.rules.resources.get(resource.0 as usize) else {
            return false;
        };
        match def.required_tech {
            Some(tech) => self.auth.players().techs_of(self.player).contains(&tech),
            None => true,
        }
    }
}

pub fn is_visible<M: RulesModule>(derived: &DerivedWorld<M>, player: PlayerId, tile: TileId) -> bool {
    let vis = &derived.visibility;
    let index = player.0 as usize * vis.tiles + tile.0 as usize;
    vis.counts.get(index).is_some_and(|c| *c > 0)
}

/// Does `viewer` see the unit `core`? Its own units always; another
/// player's unit when its tile is visible, and for an invisible unit
/// (`UnitAbility::Invisible`) only when it is revealed: next to one of the
/// viewer's units or cities, or within the sight of one of its units that
/// see invisible units (`UnitAbility::SeeInvisible`).
pub fn unit_seen_by<M: RulesModule>(
    auth: &AuthWorld<M>,
    derived: &DerivedWorld<M>,
    rules: &CompiledRuleset<M>,
    viewer: PlayerId,
    core: &crate::world::unit::UnitCore,
) -> bool {
    if core.owner == viewer {
        return true;
    }
    if !is_visible(derived, viewer, core.tile) {
        return false;
    }
    if !crate::effects::unit::own_ability(rules, core, crate::rules::effects::UnitAbility::Invisible) {
        return true;
    }
    invisible_revealed(auth, derived, rules, viewer, core.tile)
}

/// An invisible unit on `tile` is revealed to `viewer` (see `unit_seen_by`).
fn invisible_revealed<M: RulesModule>(
    auth: &AuthWorld<M>,
    derived: &DerivedWorld<M>,
    rules: &CompiledRuleset<M>,
    viewer: PlayerId,
    tile: TileId,
) -> bool {
    let dims = auth.map().dimensions;
    let near = |at: TileId| dims.distance(at, tile).is_some_and(|d| d <= 1);
    let city_near = auth
        .indexes()
        .cities_of(viewer)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .any(|c| near(c.tile));
    if city_near {
        return true;
    }
    auth.indexes().units_of(viewer).iter().any(|h| {
        let Some(view) = auth.units().get(*h) else {
            return false;
        };
        let core = view.core;
        let distance = dims.distance(core.tile, tile).unwrap_or(i32::MAX);
        if distance <= 1 {
            return true;
        }
        let sees = crate::effects::unit::own_ability(rules, core, crate::rules::effects::UnitAbility::SeeInvisible);
        sees && distance <= derived.visibility.sight_of(*h)
            && crate::derived::visibility::los_clear(auth.map(), core.tile, tile)
    })
}

/// The changing layers of a tile now (what sync remembers).
pub(crate) fn current_layers<M: RulesModule>(auth: &AuthWorld<M>, tile: TileId) -> RememberedLayers {
    let now = auth.map().layers(tile);
    RememberedLayers {
        improvement: now.and_then(|l| l.improvement),
        feature: now.and_then(|l| l.feature),
        resource: now.and_then(|l| l.resource),
        owner: now.and_then(|l| l.owner),
        route: now.map_or(Route::None, |l| l.route),
        city: city_owner_on(auth, tile).is_some(),
    }
}

/// The city standing on `tile` as the player who sees it now sees it (what
/// sync remembers); None when no city stands there.
pub(crate) fn current_sighting<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    tile: TileId,
) -> Option<CitySighting> {
    let city = auth.map().owning_city(tile)?;
    let core = auth.cities().get(city).filter(|core| core.tile == tile)?;
    let mut buildings: Vec<u16> = core.buildings.iter().map(|b| b.0).collect();
    buildings.sort_unstable();
    Some(CitySighting {
        owner: core.owner,
        name: core.name.clone(),
        seen: Some(SightedCity {
            population: core.population,
            era: auth.city_era(core.owner, rules),
            buildings,
        }),
    })
}

/// The city on `tile` by its name and owner alone: a sighting whose numbers are unknown.
#[cfg(any(test, feature = "harness"))]
pub(crate) fn current_name_and_owner<M: RulesModule>(auth: &AuthWorld<M>, tile: TileId) -> Option<CitySighting> {
    let city = auth.map().owning_city(tile)?;
    let core = auth.cities().get(city).filter(|core| core.tile == tile)?;
    Some(CitySighting { owner: core.owner, name: core.name.clone(), seen: None })
}

/// Owner of the city standing on `tile`, if any. A city's tile belongs to
/// the city itself: the owning-city layer answers without a hash lookup.
pub(crate) fn city_owner_on<M: RulesModule>(auth: &AuthWorld<M>, tile: TileId) -> Option<PlayerId> {
    // Almost every tile has no city: the index answers that with one bit.
    auth.indexes().city_on_tile(tile)?;
    let city = auth.map().owning_city(tile)?;
    let core = auth.cities().get(city)?;
    (core.tile == tile).then_some(core.owner)
}
