//! Who sees what + what is explored (§16 platform/structure.md, §32 platform/project.md).
//! Not responsible for the AI's long-term memory: that is `knowledge`.
//!
//! Sight (rules.md):
//! - a unit sees its kind's sight (`Sight`, default `SIGHT_RADIUS`) plus
//!   its promotions and owner sources, +1 standing on
//!   hills; a land unit on water is embarked and sees `EMBARKED_SIGHT` plus
//!   its embarked sight (`EmbarkedSight`) instead;
//! - a city sees `CITY_SIGHT_RADIUS`;
//! - every owned tile shows its neighbours to the owner;
//! - the ray to a tile closes on a tile strictly between that stands higher
//!   than the observer: hills 1, a forest or jungle (a feature that raises its tile)
//!   +1, mountains 3; an observer on hills stands at 1, others at 0. Water
//!   lies low and never closes the view.
//!
//! Rebuild and delta use ONE function `covered_tiles` — oracle equivalence
//! by construction, diff tests prove it. The state keeps, per unit slot,
//! the sight profile and the disc the unit's coverage was added with, so the
//! delta always removes exactly what it added. A profile is computed again
//! when the unit is promoted or when its owner's unit sources change (a
//! technology, policy, wonder or timed effect: the owner part of the unit
//! modifiers is compared after the player's turn, an adopted policy, a
//! timed effect or a city change). Every coverage change of a journal is
//! listed in `events` for the knowledge sync. Later: natural wonders
//! (sight level 2), shared vision.

use crate::effects::unit::{owner_unit_modifiers, with_own_sources, UnitModifiers};
use crate::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use crate::math::{hex_disc, hex_line_all};
use crate::metrics::WorkCounters;
use crate::module::RulesModule;
use crate::rules::effects::{UnitAbility, UnitStat};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{ChangeJournal, CityChange, PlayerChange, TileChange, UnitChange};
use crate::rules::content::Domain;
use crate::world::map::MapState;
use crate::world::module::ModuleChange;
use crate::world::state::AuthWorld;
use crate::world::unit::UnitCore;
use crate::tile_set::TileSet;

/// Sight of a unit kind without a sight value (every unit sees 2).
/// Unciv: MapUnit.getVisibilityRange (2), equal.
pub const SIGHT_RADIUS: i32 = 2;
/// Sight of a city (one ring further than a unit).
pub const CITY_SIGHT_RADIUS: i32 = 3;
/// Every owned tile shows its neighbours to the owner. Own value, not compared
/// with a public source (unverified).
pub const PLOT_SIGHT_RADIUS: i32 = 1;
/// A unit on hills sees one ring further.
pub const HILLS_SIGHT_BONUS: i32 = 1;
/// Sight of an embarked unit before its extra embarked sight. Own value, not
/// compared with a public source (unverified); 1 matches the "+1 sight when
/// embarked" of the nation that has it with its +2 promotion.
pub const EMBARKED_SIGHT: i32 = 1;
/// The farthest a unit sees, hills included (bounds the work per observer
/// and the knowledge sync).
pub const MAX_SIGHT: i32 = 7;
/// Tiles in a sight disc: 1 + 3R(R+1) = 19 at R=2.
pub const SIGHT_AREA: u64 = (1 + 3 * SIGHT_RADIUS * (SIGHT_RADIUS + 1)) as u64;

/// What a unit sees, independent of where it stands: the radius on land,
/// the radius while embarked, and whether it embarks at all (a land unit
/// that does not hover). Computed from the unit's kind, promotions and the
/// sources of its owner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnitSight {
    land: u8,
    embarked: u8,
    embarks: bool,
}

impl UnitSight {
    /// The sight of a unit of `kind` with the modifiers of all its sources.
    pub fn of<M: RulesModule>(rules: &CompiledRuleset<M>, mods: &UnitModifiers<M>, kind: UnitKindId) -> Self {
        let def = rules.units.get(kind.0 as usize);
        let base = def.map_or(0, |d| i32::from(d.sight));
        let base = if base == 0 { SIGHT_RADIUS } else { base };
        let land = (base + mods.stat(UnitStat::Sight)).clamp(1, MAX_SIGHT);
        let embarked = (EMBARKED_SIGHT + mods.stat(UnitStat::EmbarkedSight)).clamp(1, MAX_SIGHT);
        Self {
            land: land as u8,
            embarked: embarked as u8,
            embarks: def.is_some_and(|d| d.domain == Domain::Land) && !mods.has(UnitAbility::Hovering),
        }
    }

    /// The radius standing on `tile`: embarked on water, else the land
    /// radius (+1 on hills).
    pub fn at(self, map: &MapState, tile: TileId) -> i32 {
        if self.embarks && crate::navigation::terrain::is_water(map, tile) {
            return i32::from(self.embarked);
        }
        let hills = map.terrain(tile).is_some_and(|t| map.terrain_kinds().is_hills(t.0));
        (i32::from(self.land) + if hills { HILLS_SIGHT_BONUS } else { 0 }).min(MAX_SIGHT)
    }
}

/// Where a unit's coverage is and with which radius (radius 0: no unit in
/// the slot).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitCover {
    pub sight: UnitSight,
    pub at: TileId,
    pub radius: u8,
}

impl Default for UnitCover {
    fn default() -> Self {
        Self { sight: UnitSight::default(), at: TileId(0), radius: 0 }
    }
}

/// One coverage change of a journal: an observer of `player` started or
/// stopped covering the disc of `radius` around `at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SightEvent {
    pub player: PlayerId,
    pub at: TileId,
    pub radius: i32,
    pub change: SightChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SightChange {
    /// The observer arrived (created, moved in, founded): it notices the
    /// other players around.
    Arrived,
    /// The observer covers the disc again with a new radius (promotion,
    /// owner sources, a city that changed hands): no arrival.
    Widened,
    Left,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VisibilityState<M: RulesModule> {
    /// visibility_count[player][tile] — flatten row-major (player outer).
    pub counts: Vec<u16>,
    /// Bit `player * tiles + tile`: the tile was covered at least once.
    pub seen: TileSet,
    pub players: usize,
    pub tiles: usize,
    /// The coverage of every unit by slot (`UnitHandle::index`).
    pub unit_cover: Vec<UnitCover>,
    /// The owner part of the unit modifiers per player the profiles were
    /// computed with (a cache: the oracle does not compare it).
    owner_parts: Vec<UnitModifiers<M>>,
    /// Coverage changes of the last applied journal (knowledge sync).
    pub events: Vec<SightEvent>,
}

/// Coverage counter decrement: loud in debug (§62 item 6: a mismatch of
/// add/sub must fail in tests, not quietly accumulate), saturating in release.
fn dec_count(counts: &mut [u16], idx: usize) {
    if let Some(x) = counts.get_mut(idx) {
        #[cfg(debug_assertions)]
        {
            *x = x
                .checked_sub(1)
                .expect("visibility underflow: sub without matching add");
        }
        #[cfg(not(debug_assertions))]
        {
            *x = x.saturating_sub(1);
        }
    }
}

/// Coverage counter increment (u16 overflow is practically unreachable:
/// thousands of observers on one tile; checked in debug for symmetry).
fn inc_count(counts: &mut [u16], idx: usize) {
    if let Some(x) = counts.get_mut(idx) {
        #[cfg(debug_assertions)]
        {
            *x = x
                .checked_add(1)
                .expect("visibility overflow: more than 65535 observers");
        }
        #[cfg(not(debug_assertions))]
        {
            *x = x.saturating_add(1);
        }
    }
}

/// Add an observer's coverage (player p, position at, radius).
/// The ONLY place that knows "what it means to light up": rebuild and the delta
/// must go through it (§62: a shared rule, not two copies).
/// Returns the number of visited tiles (for counters).
fn add_coverage(
    counts: &mut [u16],
    seen: &mut TileSet,
    tiles: usize,
    map: &MapState,
    p: usize,
    at: TileId,
    radius: i32,
) -> u64 {
    let mut visited = 0u64;
    for t in covered_tiles(map, at, radius) {
        let idx = p * tiles + (t.0 as usize);
        inc_count(counts, idx);
        seen.insert(TileId(idx as u32));
        visited += 1;
    }
    visited
}

/// Remove an observer's coverage (symmetric to add_coverage).
fn sub_coverage(
    counts: &mut [u16],
    tiles: usize,
    map: &MapState,
    p: usize,
    at: TileId,
    radius: i32,
) -> u64 {
    let mut visited = 0u64;
    for t in covered_tiles(map, at, radius) {
        let idx = p * tiles + (t.0 as usize);
        dec_count(counts, idx);
        visited += 1;
    }
    visited
}

/// The tiles an observer at `at` with `radius` covers: the hex disc,
/// minus tiles behind a higher tile. The one rule of sight: coverage
/// counts, rebuild and knowledge sync all go through it.
pub fn covered_tiles(map: &MapState, at: TileId, radius: i32) -> impl Iterator<Item = TileId> + '_ {
    hex_disc(map.dimensions, at, radius).filter(move |t| los_clear(map, at, *t))
}

/// How high a tile stands against sight (above flat land): hills 1, mountains 3, +1 for a forest or jungle.
pub(crate) fn sight_level(map: &MapState, t: TileId) -> u8 {
    let Some(code) = map.terrain(t) else {
        return u8::MAX;
    };
    let kinds = map.terrain_kinds();
    let base = if kinds.is_hills(code.0) {
        1
    } else if kinds.is_mountains(code.0) {
        3
    } else {
        0
    };
    let hides = map.feature(t).flatten().is_some_and(|f| kinds.feature_hides(f));
    base + u8::from(hides)
}

/// How high an observer on `at` stands: 1 on
/// hills, else 0.
pub(crate) fn see_from_level(map: &MapState, at: TileId) -> u8 {
    u8::from(map.terrain(at).is_some_and(|t| map.terrain_kinds().is_hills(t.0)))
}

/// Is the ray observer→target clear? Integer hex line (no floats, §23).
/// The ends are not blockers: the observer sees its own tile and the
/// higher tile itself; a tile strictly between that stands higher than the
/// observer closes the view. Used by rebuild, delta, combat and movement —
/// one function, one result.
pub fn los_clear(map: &MapState, from: TileId, to: TileId) -> bool {
    if from == to {
        return true;
    }
    let eye = see_from_level(map, from);
    hex_line_all(&map.dimensions, from, to, |t| sight_level(map, t) <= eye)
}

impl<M: RulesModule> VisibilityState<M> {
    pub fn rebuild(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> Self {
        Self::rebuild_counted(auth, rules).0
    }

    /// Full recompute + work counters (for bench/CI bounds).
    pub fn rebuild_counted(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> (Self, WorkCounters) {
        let players = auth.players().count();
        let map = auth.map();
        let tiles = map.tile_count();
        let owner_parts: Vec<UnitModifiers<M>> = (0..players)
            .map(|p| owner_unit_modifiers(auth, rules, PlayerId(p as u8)))
            .collect();
        let mut vis = Self {
            counts: vec![0u16; players * tiles],
            seen: TileSet::with_tile_count(players * tiles),
            players,
            tiles,
            unit_cover: Vec::new(),
            owner_parts,
            events: Vec::new(),
        };
        let mut entities = 0u64;
        let mut tiles_visited = 0u64;
        // Units of the same owner, kind and promotions see the same: the
        // profile is computed once for them.
        let mut known: crate::fx::FxHashMap<(u8, UnitKindId, crate::world::unit::PromotionSet), UnitSight> =
            crate::fx::FxHashMap::default();
        // Units, cities and own territory light up by one rule.
        for (handle, core) in auth.units().iter_handles() {
            let p = core.owner.0 as usize;
            if p >= players {
                continue;
            }
            let sight = *known
                .entry((core.owner.0, core.kind, core.promotions))
                .or_insert_with(|| vis.sight_of_unit(auth, rules, core));
            let radius = sight.at(map, core.tile);
            vis.set_cover(handle, UnitCover { sight, at: core.tile, radius: radius as u8 });
            entities += 1;
            tiles_visited += add_coverage(&mut vis.counts, &mut vis.seen, tiles, map, p, core.tile, radius);
        }
        let cities = auth
            .cities()
            .iter()
            .map(|(_, c)| (c.owner.0 as usize, c.tile, CITY_SIGHT_RADIUS));
        let territory = (0..tiles as u32).filter_map(|t| {
            let owner = map.owner(TileId(t)).flatten()?;
            Some((owner.0 as usize, TileId(t), PLOT_SIGHT_RADIUS))
        });
        for (p, at, radius) in cities.chain(territory) {
            if p >= players {
                continue;
            }
            entities += 1;
            tiles_visited += add_coverage(&mut vis.counts, &mut vis.seen, tiles, map, p, at, radius);
        }
        // The counter keeps its definition (a byte per observer and tile for `seen`), so the pinned work
        // counters stay comparable across the bit-packed layout.
        let bytes = (vis.counts.len() * 2 + players * tiles + vis.unit_cover.len() * std::mem::size_of::<UnitCover>()) as u64;
        (
            vis,
            WorkCounters {
                entities_visited: entities,
                tiles_visited,
                bytes_allocated: bytes,
            },
        )
    }

    /// The sight profile of `core` with its owner's cached sources.
    fn sight_of_unit(&self, auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, core: &UnitCore) -> UnitSight {
        let part = self.owner_parts.get(core.owner.0 as usize).cloned().unwrap_or_default();
        let mods = with_own_sources(auth, rules, part, core);
        UnitSight::of(rules, &mods, core.kind)
    }

    /// Where a unit's coverage is now (None: the unit is not covered).
    pub fn cover_of(&self, unit: UnitHandle) -> Option<UnitCover> {
        self.unit_cover.get(unit.index as usize).copied().filter(|c| c.radius > 0)
    }

    /// The radius a unit sees with now (0: no such unit).
    pub fn sight_of(&self, unit: UnitHandle) -> i32 {
        self.cover_of(unit).map_or(0, |c| i32::from(c.radius))
    }

    fn set_cover(&mut self, unit: UnitHandle, cover: UnitCover) {
        let index = unit.index as usize;
        if self.unit_cover.len() <= index {
            self.unit_cover.resize(index + 1, UnitCover::default());
        }
        self.unit_cover[index] = cover;
    }

    pub fn apply_changes(
        &mut self,
        changes: &ChangeJournal<M>,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
    ) {
        let _ = Self::apply_changes_counted(self, changes, auth, rules);
    }

    /// Coverage increment (O(area) per event; cities and territory use the same
    /// rule). Sight depends on observers, terrain, features and the
    /// sources of unit sight: a terrain or feature change (LOS geometry)
    /// falls back to a full rebuild (the coverage events are still listed);
    /// other tile changes (roads, improvements) do no work. Returns the
    /// counters.
    pub fn apply_changes_counted(
        vis: &mut VisibilityState<M>,
        changes: &ChangeJournal<M>,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
    ) -> WorkCounters {
        vis.events.clear();
        let geometry_changed = changes.tile_changes.iter().any(|change| {
            matches!(
                change,
                TileChange::TerrainSet { .. } | TileChange::FeatureSet { .. } | TileChange::FeatureChopped { .. }
            )
        });
        let owners_changed = changes
            .tile_changes
            .iter()
            .any(|change| matches!(change, TileChange::OwnerChanged { .. }));
        let sources = owners_to_refresh(changes, auth);
        if changes.unit_changes.is_empty() && changes.city_changes.is_empty() && !owners_changed && sources.is_empty() {
            if geometry_changed {
                return vis.rebuild_keeping_events(auth, rules);
            }
            return WorkCounters::default();
        }
        let mut delta = Delta {
            auth,
            rules,
            counting: !geometry_changed,
            counters: WorkCounters::default(),
            need_rebuild: false,
        };
        for change in &changes.unit_changes {
            delta.unit(vis, change);
        }
        for change in &changes.city_changes {
            delta.city(vis, change);
        }
        for player in sources {
            delta.refresh_owner(vis, player);
        }
        // Territory: each owner change of a tile is a sub/add pair of radius 1
        // (the knowledge sync handles it itself).
        if delta.counting {
            for tc in &changes.tile_changes {
                if let TileChange::OwnerChanged { tile, from, to } = *tc {
                    if let Some(p) = from {
                        delta.sub_tiles(vis, p, tile, PLOT_SIGHT_RADIUS);
                    }
                    if let Some(p) = to {
                        delta.add_tiles(vis, p, tile, PLOT_SIGHT_RADIUS);
                    }
                }
            }
        }
        let mut counters = delta.counters;
        if geometry_changed || delta.need_rebuild {
            counters.add(vis.rebuild_keeping_events(auth, rules));
        }
        counters
    }

    /// Replace everything by a rebuild, keeping this journal's events.
    fn rebuild_keeping_events(&mut self, auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> WorkCounters {
        let events = std::mem::take(&mut self.events);
        let (fresh, counters) = Self::rebuild_counted(auth, rules);
        *self = fresh;
        self.events = events;
        counters
    }
}

/// Players whose unit sources may have changed in a journal: the player
/// whose turn ended (research, golden ages), whose timed effects changed,
/// whom the module names (an adopted policy), the owners of cities that
/// gained a building, were founded, changed hands or were removed.
fn owners_to_refresh<M: RulesModule>(changes: &ChangeJournal<M>, auth: &AuthWorld<M>) -> Vec<PlayerId> {
    let mut players = Vec::new();
    for change in &changes.player_changes {
        match *change {
            PlayerChange::TurnEnded { player }
            | PlayerChange::TechnologyChosen { player }
            | PlayerChange::EffectsChanged { player } => players.push(player),
            _ => {}
        }
    }
    players.extend(changes.module_changes.iter().filter_map(|change| change.changes_unit_sources()));
    for change in &changes.city_changes {
        match *change {
            CityChange::BuildingAdded { city, .. } | CityChange::Founded(city) => {
                if let Some(core) = auth.cities().get(city) {
                    players.push(core.owner);
                }
            }
            CityChange::OwnerChanged { from, to, .. } => {
                players.push(from);
                players.push(to);
            }
            CityChange::Removed { owner, .. } => players.push(owner),
            _ => {}
        }
    }
    players.sort();
    players.dedup();
    players
}

/// One journal applied to the coverage. With `counting` false (a terrain
/// change: the counts are rebuilt afterwards) only profiles and events move.
struct Delta<'a, M: RulesModule> {
    auth: &'a AuthWorld<M>,
    rules: &'a CompiledRuleset<M>,
    counting: bool,
    counters: WorkCounters,
    need_rebuild: bool,
}

impl<M: RulesModule> Delta<'_, M> {
    fn add_tiles(&mut self, vis: &mut VisibilityState<M>, player: PlayerId, at: TileId, radius: i32) {
        if player.0 as usize >= vis.players {
            return;
        }
        let tiles = vis.tiles;
        self.counters.tiles_visited +=
            add_coverage(&mut vis.counts, &mut vis.seen, tiles, self.auth.map(), player.0 as usize, at, radius);
        self.counters.entities_visited += 1;
    }

    fn sub_tiles(&mut self, vis: &mut VisibilityState<M>, player: PlayerId, at: TileId, radius: i32) {
        if player.0 as usize >= vis.players {
            return;
        }
        let tiles = vis.tiles;
        self.counters.tiles_visited += sub_coverage(&mut vis.counts, tiles, self.auth.map(), player.0 as usize, at, radius);
        self.counters.entities_visited += 1;
    }

    fn add(&mut self, vis: &mut VisibilityState<M>, player: PlayerId, at: TileId, radius: i32, change: SightChange) {
        if self.counting {
            self.add_tiles(vis, player, at, radius);
        }
        vis.events.push(SightEvent { player, at, radius, change });
    }

    fn sub(&mut self, vis: &mut VisibilityState<M>, player: PlayerId, at: TileId, radius: i32) {
        if self.counting {
            self.sub_tiles(vis, player, at, radius);
        }
        vis.events.push(SightEvent { player, at, radius, change: SightChange::Left });
    }

    fn unit(&mut self, vis: &mut VisibilityState<M>, change: &UnitChange) {
        match *change {
            UnitChange::Created(unit) => match self.auth.units().get(unit) {
                Some(view) => {
                    let core = *view.core;
                    let sight = vis.sight_of_unit(self.auth, self.rules, &core);
                    let radius = sight.at(self.auth.map(), core.tile);
                    vis.set_cover(unit, UnitCover { sight, at: core.tile, radius: radius as u8 });
                    self.add(vis, core.owner, core.tile, radius, SightChange::Arrived);
                }
                // Created and gone in one transaction (a unit built and
                // disbanded for a deficit in one turn): its Destroyed entry
                // would take sight never added.
                None => self.need_rebuild = true,
            },
            UnitChange::Moved { unit, to, .. } => {
                let (Some(cover), Some(owner)) = (vis.cover_of(unit), self.auth.units().get(unit).map(|u| u.core.owner)) else {
                    self.need_rebuild = true;
                    return;
                };
                let radius = cover.sight.at(self.auth.map(), to);
                self.sub(vis, owner, cover.at, i32::from(cover.radius));
                vis.set_cover(unit, UnitCover { sight: cover.sight, at: to, radius: radius as u8 });
                self.add(vis, owner, to, radius, SightChange::Arrived);
            }
            UnitChange::Destroyed { unit, owner, .. } => {
                let Some(cover) = vis.cover_of(unit) else {
                    self.need_rebuild = true;
                    return;
                };
                self.sub(vis, owner, cover.at, i32::from(cover.radius));
                vis.set_cover(unit, UnitCover::default());
            }
            UnitChange::Promoted { unit, .. } => self.resight(vis, unit),
            // HP and stances do not change sight: no work.
            UnitChange::Damaged { .. } | UnitChange::StanceChanged { .. } => {}
        }
    }

    /// Compute one unit's sight again where its coverage is; move the
    /// coverage when the radius changed.
    fn resight(&mut self, vis: &mut VisibilityState<M>, unit: UnitHandle) {
        let (Some(cover), Some(core)) = (vis.cover_of(unit), self.auth.units().get(unit).map(|u| *u.core)) else {
            return;
        };
        let sight = vis.sight_of_unit(self.auth, self.rules, &core);
        let radius = sight.at(self.auth.map(), cover.at);
        if radius != i32::from(cover.radius) {
            self.sub(vis, core.owner, cover.at, i32::from(cover.radius));
            self.add(vis, core.owner, cover.at, radius, SightChange::Widened);
        }
        vis.set_cover(unit, UnitCover { sight, at: cover.at, radius: radius as u8 });
    }

    /// The player's unit sources may have changed: when the owner part of
    /// the modifiers did, every unit of the player is seen again.
    fn refresh_owner(&mut self, vis: &mut VisibilityState<M>, player: PlayerId) {
        let fresh = owner_unit_modifiers(self.auth, self.rules, player);
        let Some(slot) = vis.owner_parts.get_mut(player.0 as usize) else {
            return;
        };
        if *slot == fresh {
            return;
        }
        *slot = fresh;
        for unit in self.auth.indexes().units_of(player).to_vec() {
            self.resight(vis, unit);
        }
    }

    /// Cities: the same rule (CITY_SIGHT_RADIUS); OwnerChanged moves the
    /// coverage between players with one sub/add pair.
    fn city(&mut self, vis: &mut VisibilityState<M>, change: &CityChange) {
        match *change {
            CityChange::Founded(city) => {
                if let Some(core) = self.auth.cities().get(city) {
                    let (owner, tile) = (core.owner, core.tile);
                    self.add(vis, owner, tile, CITY_SIGHT_RADIUS, SightChange::Arrived);
                }
            }
            CityChange::OwnerChanged { city, from, to } => {
                if let Some(tile) = self.auth.cities().get(city).map(|c| c.tile) {
                    self.sub(vis, from, tile, CITY_SIGHT_RADIUS);
                    self.add(vis, to, tile, CITY_SIGHT_RADIUS, SightChange::Widened);
                }
            }
            CityChange::Removed { owner, tile, .. } => self.sub(vis, owner, tile, CITY_SIGHT_RADIUS),
            // Buildings, citizens, status, the queue and the name do not change sight.
            CityChange::BuildingAdded { .. }
            | CityChange::PopulationChanged { .. }
            | CityChange::WorkedChanged { .. }
            | CityChange::StatusChanged { .. }
            | CityChange::QueueChanged { .. }
            | CityChange::Renamed { .. } => {}
        }
    }
}
