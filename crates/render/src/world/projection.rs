//! Game read model + ChangeJournal -> RenderWorld (§32).
//!
//! The projection is the observer's knowledge, never the truth (§28): every tile comes from
//! the core's PlayerView — Unknown tiles carry a neutral placeholder and no layers,
//! Remembered tiles what the observer last saw, Visible tiles the present — and units and
//! cities the player sees: the same filter (`PlayerView`) the AI observes through.
//!
//! Three entry points share the per-tile and per-entity rules below:
//! - [`rebuild_render_world`] is the oracle: a full rebuild from the game state.
//! - [`apply_changes`] is the fast path: it reads only what the journal and the observer's
//!   knowledge delta can have changed, and bumps exactly the layers whose content changed.
//! - [`refresh`] rebuilds and then bumps the layers that differ (after quiet command batches
//!   and world replacement, where no single journal is at hand).
//!
//! The differential test proves apply == rebuild after random command sequences (§62).

use kernel::module::RulesModule;
use crate::world::state::{
    CityMarker, FeatureKind, Layer, LayerRevisions, RenderWorld, TerrainCell, TileYields, UnitMarker,
    ALL_LAYERS,
};
use kernel::derived::tile_yields::{plot_yields, TechSet};
use kernel::ids::{PlayerId, TileId};
use kernel::knowledge::{KnownTile, PlayerView};
use kernel::rules::content::Yields;
use kernel::rules::effects::YieldKind;
use kernel::rules::CompiledRuleset;
use kernel::transaction::journal::{ChangeJournal, TileChange};
use kernel::world::state::{AuthWorld, GameView};
use kernel::world::unit::UnitCore;
use kernel::effects::unit::unit_modifiers;

/// Terrain shown for tiles the observer has never seen: neutral paper ground.
pub const UNKNOWN_TERRAIN: u16 = 1;

/// Everything the projection keeps per tile, already masked by the observer's knowledge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TileView {
    cell: TerrainCell,
    improvement: Option<u16>,
    /// The improvement is pillaged and the observer sees it now.
    pillaged: bool,
    road: u8,
    river: u8,
    forest: u8,
    feature: FeatureKind,
    natural_wonder: Option<u16>,
    yields: TileYields,
}

/// A tile's yields in `TileYields` order: the kernel's channels, then the rules
/// module's (0 when it has none), clamped to the projection's range.
pub fn tile_yields<M: RulesModule>(yields: Yields) -> TileYields {
    let mut channels = YieldKind::<M>::all().filter(|kind| *kind != YieldKind::Output);
    std::array::from_fn(|_| channels.next().map_or(0, |kind| yields.of(kind).clamp(0, i8::MAX as i32) as i8))
}

impl TileView {
    /// The tile as the observer knows it (resource only when its technology is known).
    fn of<M: RulesModule>(view: &PlayerView<'_, M>, tile: TileId, rules: &CompiledRuleset<M>, techs: &TechSet) -> Self {
        let Some(s) = view.tile(tile).snapshot().copied() else {
            return Self::unknown(tile);
        };
        let y = plot_yields(view, rules, tile, techs).at_least(0);
        let feature = s.feature.map_or(FeatureKind::None, |f| FeatureKind::of(rules, f));
        Self {
            cell: TerrainCell {
                tile,
                terrain: s.terrain.0,
                owner: s.owner.map(|p| p.0),
                resource: s.resource.map(|r| r.0),
            },
            improvement: s.improvement.map(|i| i.0),
            pillaged: s.improvement.is_some() && view.known_pillaged(tile),
            road: s.route.code(),
            river: s.river as u8,
            forest: feature.has_trees() as u8,
            feature,
            natural_wonder: s.natural_wonder.map(|w| w.0),
            yields: tile_yields::<M>(y),
        }
    }

    fn unknown(tile: TileId) -> Self {
        Self {
            cell: TerrainCell {
                tile,
                terrain: UNKNOWN_TERRAIN,
                owner: None,
                resource: None,
            },
            improvement: None,
            pillaged: false,
            road: 0,
            river: 0,
            forest: 0,
            feature: FeatureKind::None,
            natural_wonder: None,
            yields: [0; 6],
        }
    }

    fn read(world: &RenderWorld, i: usize) -> Self {
        Self {
            cell: world.terrain[i],
            improvement: world.improvements[i],
            pillaged: world.pillaged.get(i).copied().unwrap_or(false),
            road: world.road[i],
            river: world.river[i],
            forest: world.forest[i],
            feature: world.feature.get(i).copied().unwrap_or_default(),
            // Hand-built worlds may leave the newer per-tile vectors empty.
            natural_wonder: world.natural_wonders.get(i).copied().flatten(),
            yields: world.yields.get(i).copied().unwrap_or_default(),
        }
    }

    fn write(self, world: &mut RenderWorld, i: usize) {
        world.terrain[i] = self.cell;
        world.improvements[i] = self.improvement;
        if let Some(slot) = world.pillaged.get_mut(i) {
            *slot = self.pillaged;
        }
        world.road[i] = self.road;
        world.river[i] = self.river;
        world.forest[i] = self.forest;
        if let Some(slot) = world.feature.get_mut(i) {
            *slot = self.feature;
        }
        if let Some(slot) = world.natural_wonders.get_mut(i) {
            *slot = self.natural_wonder;
        }
        if let Some(slot) = world.yields.get_mut(i) {
            *slot = self.yields;
        }
    }

    /// Layers whose content differs between two views of the same tile.
    fn changed_layers(self, other: Self, out: &mut LayerSet) {
        // Ground features (marsh, ice, flood plains, ...) and natural wonder
        // sites shape and paint the surface.
        let ground = self.feature != other.feature || self.natural_wonder != other.natural_wonder;
        if self.cell.terrain != other.cell.terrain || self.river != other.river || ground {
            out.insert(Layer::Terrain);
        }
        if self.cell.owner != other.cell.owner {
            out.insert(Layer::Borders);
        }
        if self.cell.resource != other.cell.resource
            || self.yields != other.yields
            || self.forest != other.forest
            || self.feature != other.feature
            || self.natural_wonder != other.natural_wonder
        {
            out.insert(Layer::Features);
        }
        if self.improvement != other.improvement || self.pillaged != other.pillaged || self.road != other.road {
            out.insert(Layer::Materials);
        }
    }
}

/// Small set of layers (bit per layer).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayerSet(pub u8);

impl LayerSet {
    fn bit(layer: Layer) -> u8 {
        1 << ALL_LAYERS.iter().position(|l| *l == layer).unwrap_or(0)
    }

    pub fn insert(&mut self, layer: Layer) {
        self.0 |= Self::bit(layer);
    }

    pub fn contains(&self, layer: Layer) -> bool {
        self.0 & Self::bit(layer) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub fn layers(&self) -> Vec<Layer> {
        ALL_LAYERS
            .into_iter()
            .filter(|layer| self.contains(*layer))
            .collect()
    }
}

/// Visible mask of the observer: counts[observer][tile] > 0.
fn visible_mask<M: RulesModule>(game: GameView<'_, M>, observer: PlayerId) -> Vec<u8> {
    let tiles = game.auth.map().tile_count();
    let start = observer.0 as usize * tiles;
    let counts = &game.derived.visibility.counts;
    (0..tiles)
        .map(|i| u8::from(counts.get(start + i).copied().unwrap_or(0) > 0))
        .collect()
}

fn explored_mask<M: RulesModule>(game: GameView<'_, M>, observer: PlayerId) -> Vec<u8> {
    let knowledge = game.auth.knowledge();
    let tiles = game.auth.map().tile_count();
    match knowledge.get(observer.0 as usize) {
        Some(k) => k.explored_flags(),
        None => vec![0; tiles],
    }
}

/// Units the observer sees (own and visible foreign) and the cities it knows: own and
/// visible ones with their live data, remembered ones (a city on an explored tile out of
/// sight) as the observer last saw them (`city_markers`).
fn entities<M: RulesModule>(
    game: GameView<'_, M>,
    observer: PlayerId,
    rules: &CompiledRuleset<M>,
    revealed: bool,
) -> (Vec<UnitMarker>, Vec<CityMarker>) {
    let view = player_view(game, observer, rules, revealed);
    let mut own = Vec::new();
    let mut foreign = Vec::new();
    for (_, core) in game.auth.units().iter_handles() {
        let own_unit = core.owner == observer;
        if !own_unit && !view.sees_unit(core) {
            continue;
        }
        let marker = UnitMarker {
            id: core.id.0,
            fortified: core.stance.is_fortified(),
            embarked: is_embarked(game.auth, rules, core),
            tile: core.tile,
            owner: core.owner.0,
            kind: core.kind.0,
            hp: core.hp.clamp(1, 100) as u8,
        };
        if own_unit {
            own.push(marker);
        } else {
            foreign.push(marker);
        }
    }
    own.sort_by_key(|u| u.id);
    let units = own.into_iter().chain(foreign).collect();
    (units, city_markers(game.auth, &view, rules))
}

/// Is a unit the observer sees afloat: a land unit on a water tile that does not hover
/// (`UnitModifiers::embarked_on`, the rule the movement and the combat go by)? A unit
/// the observer sees stands on a tile it sees, so this reads nothing hidden.
fn is_embarked<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, core: &UnitCore) -> bool {
    kernel::navigation::terrain::is_water(auth.map(), core.tile)
        && unit_modifiers(auth, rules, core).embarked_on(rules, auth.map(), core.kind, core.tile)
}

/// The player's view of the world: its own knowledge, or the whole map open for a
/// spectator (`PlayerView::revealed`).
fn player_view<'a, M: RulesModule>(
    game: GameView<'a, M>,
    observer: PlayerId,
    rules: &'a CompiledRuleset<M>,
    revealed: bool,
) -> PlayerView<'a, M> {
    let view = PlayerView::new(game.auth, game.derived, rules, observer);
    if revealed { view.revealed() } else { view }
}

/// Visible cities carry their live data (buildings, the owner's era: the city kit's style);
/// remembered ones what the observer last saw of them (`CitySighting`: owner, name,
/// population, era, buildings), never the city list of the world now: a city that grew,
/// changed hands or vanished out of sight looks as it did until the observer sees it again.
fn city_markers<M: RulesModule>(auth: &AuthWorld<M>, view: &PlayerView<'_, M>, rules: &CompiledRuleset<M>) -> Vec<CityMarker> {
    let mut markers = Vec::new();
    for (handle, core) in auth.cities().iter() {
        if let KnownTile::Visible(_) = view.tile(core.tile) {
            markers.push(CityMarker {
                tile: core.tile,
                owner: core.owner.0,
                name: core.name.clone(),
                population: Some(core.population),
                buildings: sorted_buildings(core),
                era: auth.city_era(core.owner, rules),
                showpieces: M::city_showpieces(auth, rules, handle),
                unworked: own_unworked(view, core),
            });
        }
    }
    markers.extend(view.remembered_cities().map(|(tile, sighting)| CityMarker {
        tile,
        owner: sighting.owner.0,
        name: sighting.name.clone(),
        population: sighting.seen.as_ref().map(|seen| seen.population),
        buildings: sighting.seen.as_ref().map(|seen| seen.buildings.clone()).unwrap_or_default(),
        era: sighting.seen.as_ref().map_or(0, |seen| seen.era),
        showpieces: Vec::new(),
        unworked: Vec::new(),
    }));
    markers
}

fn sorted_buildings(core: &kernel::world::city::CityCore) -> Vec<u16> {
    let mut buildings: Vec<u16> = core.buildings.iter().map(|b| b.0).collect();
    buildings.sort_unstable();
    buildings
}

fn own_unworked<M: RulesModule>(view: &PlayerView<'_, M>, core: &kernel::world::city::CityCore) -> Vec<u32> {
    if core.owner != view.player() {
        return Vec::new();
    }
    core.unworked.iter().map(|t| t.0).collect()
}

fn river_edges<M: RulesModule>(auth: &AuthWorld<M>, explored: &[u8]) -> Vec<(u32, u32)> {
    auth.map()
        .river_edges()
        .iter()
        .copied()
        .filter(|(a, b)| explored.get(*a as usize) == Some(&1) && explored.get(*b as usize) == Some(&1))
        .collect()
}

fn unlocked_resources<M: RulesModule>(auth: &AuthWorld<M>, observer: PlayerId, rules: &CompiledRuleset<M>) -> Vec<u16> {
    let techs = auth.players().techs_of(observer);
    rules
        .resources
        .iter()
        .enumerate()
        .filter(|(_, def)| def.required_tech.map(|q| techs.contains(&q)).unwrap_or(true))
        .map(|(i, _)| i as u16)
        .collect()
}

/// The oracle: the observer's full projection from the game state.
pub fn rebuild_render_world<M: RulesModule>(
    game: GameView<'_, M>,
    observer: PlayerId,
    rules: &CompiledRuleset<M>,
) -> RenderWorld {
    rebuild_with(game, observer, rules, false)
}

/// The projection of a spectator who watches through `observer`'s colours and
/// technologies with the whole map open: every tile visible and explored, every unit
/// and city shown as it is now. Only for a viewer that is not a player of the match.
pub fn rebuild_revealed_world<M: RulesModule>(
    game: GameView<'_, M>,
    observer: PlayerId,
    rules: &CompiledRuleset<M>,
) -> RenderWorld {
    rebuild_with(game, observer, rules, true)
}

fn rebuild_with<M: RulesModule>(
    game: GameView<'_, M>,
    observer: PlayerId,
    rules: &CompiledRuleset<M>,
    revealed: bool,
) -> RenderWorld {
    let auth = game.auth;
    let n = auth.map().tile_count();
    let (visible, explored) = if revealed {
        (vec![1; n], vec![1; n])
    } else {
        (visible_mask(game, observer), explored_mask(game, observer))
    };
    let mut world = RenderWorld {
        visual_surface: false,
        width: auth.map().dimensions.width,
        height: auth.map().dimensions.height,
        terrain: Vec::with_capacity(n),
        terrain_kinds: auth.map().terrain_kinds(),
        improvements: Vec::with_capacity(n),
        pillaged: Vec::with_capacity(n),
        road: Vec::with_capacity(n),
        river: Vec::with_capacity(n),
        forest: Vec::with_capacity(n),
        feature: Vec::with_capacity(n),
        natural_wonders: Vec::with_capacity(n),
        yields: Vec::with_capacity(n),
        observer: observer.0,
        layers: LayerRevisions::fresh(),
        ..Default::default()
    };
    let player_view = player_view(game, observer, rules, revealed);
    let techs = TechSet::of(auth.players().techs_of(observer));
    for i in 0..n {
        let view = TileView::of(&player_view, TileId(i as u32), rules, &techs);
        world.terrain.push(view.cell);
        world.improvements.push(view.improvement);
        world.pillaged.push(view.pillaged);
        world.road.push(view.road);
        world.river.push(view.river);
        world.forest.push(view.forest);
        world.feature.push(view.feature);
        world.natural_wonders.push(view.natural_wonder);
        world.yields.push(view.yields);
    }
    let (units, cities) = entities(game, observer, rules, revealed);
    world.units = units;
    world.cities = cities;
    world.river_edges = river_edges(auth, &explored);
    world.unlocked_resources = unlocked_resources(auth, observer, rules);
    world.natural_wonder_keys = rules.natural_wonders.iter().map(|w| w.key.clone()).collect();
    world.visible = visible;
    world.explored = explored;
    world
}

/// Fast path: applies one committed command's journal. Bumps exactly the changed layers.
pub fn apply_changes<M: RulesModule>(
    render_world: &mut RenderWorld,
    game: GameView<'_, M>,
    changes: &ChangeJournal<M>,
    rules: &CompiledRuleset<M>,
) -> LayerSet {
    let mut changed = LayerSet::default();
    if changes.is_empty() {
        return changed;
    }
    let observer = PlayerId(render_world.observer);
    let view = PlayerView::new(game.auth, game.derived, rules, observer);
    let unlocked = unlocked_resources(game.auth, observer, rules);
    let resources_changed = unlocked != render_world.unlocked_resources;
    let (newly_known, newly_visible) = apply_knowledge(render_world, game, observer, &mut changed);
    let mut tiles: Vec<usize> = newly_known.clone();
    tiles.extend(newly_visible);
    tiles.extend(changes.tile_changes.iter().map(|c| changed_tile(c).0 as usize));
    let learned = changes.player_changes.iter().any(|c| {
        matches!(c, kernel::transaction::journal::PlayerChange::TechnologyChosen { player } if *player == observer)
    });
    if resources_changed || learned {
        // A technology reveals resources and improves improvements on every known tile.
        tiles = (0..render_world.terrain.len()).collect();
    }
    let techs = TechSet::of(game.auth.players().techs_of(observer));
    apply_tiles(render_world, &view, &tiles, rules, &techs, &mut changed);
    let rivers_touched = changes
        .tile_changes
        .iter()
        .any(|c| matches!(c, TileChange::RiverSet { .. }));
    if rivers_touched || !newly_known.is_empty() {
        let edges = river_edges(game.auth, &render_world.explored);
        if edges != render_world.river_edges {
            render_world.river_edges = edges;
            changed.insert(Layer::Terrain);
        }
    }
    let entities_touched = !changes.unit_changes.is_empty()
        || !changes.city_changes.is_empty()
        || changed.contains(Layer::Knowledge);
    // Player and module changes (policies, religion, trade) may change what
    // the observer's banners show.
    if entities_touched || !changes.player_changes.is_empty() || !changes.module_changes.is_empty() {
        // A new technology can change the owner's era: the city style follows.
        apply_entities(render_world, game, observer, rules, &mut changed);
    }
    if resources_changed {
        render_world.unlocked_resources = unlocked;
        changed.insert(Layer::Features);
    }
    for layer in changed.layers() {
        render_world.layers.bump(layer);
    }
    changed
}

fn changed_tile(change: &TileChange) -> TileId {
    match *change {
        TileChange::OwnerChanged { tile, .. }
        | TileChange::ImprovementBuilt { tile }
        | TileChange::ResourceSet { tile }
        | TileChange::RoadBuilt { tile }
        | TileChange::RiverSet { tile }
        | TileChange::FeatureSet { tile }
        | TileChange::FeatureChopped { tile }
        | TileChange::NaturalWonderSet { tile }
        | TileChange::TerrainSet { tile } => tile,
    }
}

/// Updates the visible and explored masks; returns the tiles that became known and the
/// known tiles that came back into sight (a remembered tile shows the present again).
/// O(tiles) byte comparison: far below one frame, and independent of what moved.
fn apply_knowledge<M: RulesModule>(
    world: &mut RenderWorld,
    game: GameView<'_, M>,
    observer: PlayerId,
    changed: &mut LayerSet,
) -> (Vec<usize>, Vec<usize>) {
    let visible = visible_mask(game, observer);
    let explored = explored_mask(game, observer);
    let mut newly_known = Vec::new();
    let mut newly_visible = Vec::new();
    for i in 0..visible.len() {
        let was = world.is_known(i);
        let now = explored[i] == 1 || visible[i] == 1;
        if now && !was {
            newly_known.push(i);
        } else if was && visible[i] == 1 && world.visible.get(i) != Some(&1) {
            newly_visible.push(i);
        }
    }
    if explored != world.explored {
        // Geometry depends on the explored set: a first reveal is a terrain change.
        changed.insert(Layer::Terrain);
        changed.insert(Layer::Knowledge);
        world.explored = explored;
    }
    if visible != world.visible {
        changed.insert(Layer::Knowledge);
        world.visible = visible;
    }
    (newly_known, newly_visible)
}

fn apply_tiles<M: RulesModule>(
    world: &mut RenderWorld,
    view: &PlayerView<'_, M>,
    tiles: &[usize],
    rules: &CompiledRuleset<M>,
    techs: &TechSet,
    changed: &mut LayerSet,
) {
    for &i in tiles {
        if i >= world.terrain.len() {
            continue;
        }
        let fresh = TileView::of(view, TileId(i as u32), rules, techs);
        let old = TileView::read(world, i);
        if fresh != old {
            fresh.changed_layers(old, changed);
            fresh.write(world, i);
        }
    }
}

fn apply_entities<M: RulesModule>(
    world: &mut RenderWorld,
    game: GameView<'_, M>,
    observer: PlayerId,
    rules: &CompiledRuleset<M>,
    changed: &mut LayerSet,
) {
    let (units, cities) = entities(game, observer, rules, false);
    if units != world.units {
        world.units = units;
        changed.insert(Layer::Units);
    }
    if cities != world.cities {
        world.cities = cities;
        changed.insert(Layer::Cities);
    }
}

/// Rebuilds the projection and bumps the layers whose content differs. Keeps the surface
/// mode chosen by the consumer. Used after quiet command batches and world replacement.
pub fn refresh<M: RulesModule>(
    render_world: &mut RenderWorld,
    game: GameView<'_, M>,
    observer: PlayerId,
    rules: &CompiledRuleset<M>,
) -> LayerSet {
    refresh_with(render_world, game, observer, rules, false)
}

/// `refresh` of a spectator's projection: with `revealed` the whole map is open
/// (`rebuild_revealed_world`), without it the observer's own knowledge.
pub fn refresh_with<M: RulesModule>(
    render_world: &mut RenderWorld,
    game: GameView<'_, M>,
    observer: PlayerId,
    rules: &CompiledRuleset<M>,
    revealed: bool,
) -> LayerSet {
    let mut fresh = rebuild_with(game, observer, rules, revealed);
    fresh.visual_surface = render_world.visual_surface;
    let changed = changed_layers(render_world, &fresh);
    fresh.layers = render_world.layers;
    for layer in changed.layers() {
        fresh.layers.bump(layer);
    }
    *render_world = fresh;
    changed
}

/// Layers whose content differs between two projections (revisions are ignored).
pub fn changed_layers(a: &RenderWorld, b: &RenderWorld) -> LayerSet {
    let mut changed = LayerSet::default();
    let shape = (a.width, a.height, a.observer, a.visual_surface, a.terrain.len(), a.terrain_kinds)
        != (b.width, b.height, b.observer, b.visual_surface, b.terrain.len(), b.terrain_kinds);
    if shape {
        for layer in ALL_LAYERS {
            changed.insert(layer);
        }
        return changed;
    }
    let terrain = a.terrain.iter().zip(&b.terrain).any(|(x, y)| (x.tile, x.terrain) != (y.tile, y.terrain))
        || a.river != b.river
        || a.river_edges != b.river_edges
        || a.explored != b.explored
        || a.feature != b.feature
        || a.natural_wonders != b.natural_wonders;
    if terrain {
        changed.insert(Layer::Terrain);
    }
    if a.improvements != b.improvements || a.pillaged != b.pillaged || a.road != b.road {
        changed.insert(Layer::Materials);
    }
    let resources = a.terrain.iter().zip(&b.terrain).any(|(x, y)| x.resource != y.resource);
    if resources
        || a.forest != b.forest
        || a.feature != b.feature
        || a.natural_wonders != b.natural_wonders
        || a.unlocked_resources != b.unlocked_resources
        || a.yields != b.yields
    {
        changed.insert(Layer::Features);
    }
    if a.units != b.units {
        changed.insert(Layer::Units);
    }
    if a.cities != b.cities {
        changed.insert(Layer::Cities);
    }
    if a.terrain.iter().zip(&b.terrain).any(|(x, y)| x.owner != y.owner) {
        changed.insert(Layer::Borders);
    }
    if a.visible != b.visible || a.explored != b.explored {
        changed.insert(Layer::Knowledge);
    }
    changed
}

/// Replaces the geography of tiles outside the world's knowledge (hand-built showcase
/// worlds; game projections are masked at construction). Hidden units, cities and river
/// beds are dropped by the same rules as [`rebuild_render_world`].
pub fn mask_unknown(world: &mut RenderWorld) {
    for i in 0..world.terrain.len() {
        if !world.is_known(i) {
            TileView::unknown(world.terrain[i].tile).write(world, i);
        }
    }
    let visible = world.visible.clone();
    let explored = world.explored.clone();
    world.units.retain(|u| visible.get(u.tile.0 as usize) == Some(&1));
    world.cities.retain(|c| explored.get(c.tile.0 as usize) == Some(&1));
    world.river_edges.retain(|(a, b)| {
        explored.get(*a as usize) == Some(&1) && explored.get(*b as usize) == Some(&1)
    });
}
