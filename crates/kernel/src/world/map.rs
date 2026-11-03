//! Base geometry and authoritative tile data (§7).
//!
//! TileId is a cheap dense lookup key. Map is not responsible for
//! visibility/UI/AI-threat/rendering.

use crate::ids::{CityHandle, FeatureId, ImprovementId, PlayerId, ResourceId, TerrainId, TileId};
use crate::math::MapDimensions;
use crate::rules::content::NaturalWonderId;
use crate::tile_set::TileSet;

/// Route on a tile (Classic: road, railroad). The save stores the code (0 none, 1 road,
/// 2 railroad), so maps from before railroads read unchanged. Railroads have no build order
/// or movement rule yet (M4): the map keeps them so saves, projection and art agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Route {
    #[default]
    None,
    Road,
    Railroad,
}

impl Route {
    #[inline]
    pub fn code(self) -> u8 {
        match self {
            Route::None => 0,
            Route::Road => 1,
            Route::Railroad => 2,
        }
    }

    #[inline]
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Route::None),
            1 => Some(Route::Road),
            2 => Some(Route::Railroad),
            _ => None,
        }
    }
}

/// "No value" of a 16-bit id layer (the ids are table indices, far below it).
const NONE16: u16 = u16::MAX;
/// "No owner" (a match has fewer than 255 players).
const NONE8: u8 = u8::MAX;
/// `TileCell::flags`: a river touches the (land) tile.
const FLAG_RIVER: u8 = 1;
/// `TileCell::flags`: the improvement is pillaged.
const FLAG_PILLAGED: u8 = 2;

/// Every per-tile layer a view of the tile reads, in one 16-byte record (a tile is one cache
/// line access, not nine arrays). `Option<Id>` layers are packed with `NONE16` as "none".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct TileCell {
    terrain: u16,
    feature: u16,
    resource: u16,
    improvement: u16,
    wonder: u16,
    owner: u8,
    route: u8,
    /// Copies the resource gives (0 without one).
    amount: u8,
    flags: u8,
    _pad: u16,
}

impl TileCell {
    fn new(terrain: TerrainId) -> Self {
        Self {
            terrain: terrain.0,
            feature: NONE16,
            resource: NONE16,
            improvement: NONE16,
            wonder: NONE16,
            owner: NONE8,
            route: 0,
            amount: 0,
            flags: 0,
            _pad: 0,
        }
    }
}

#[inline]
fn some16(v: u16) -> Option<u16> {
    (v != NONE16).then_some(v)
}

#[inline]
fn pack16(v: Option<u16>) -> u16 {
    v.unwrap_or(NONE16)
}

/// The layers of one tile a player's view reads, taken with one lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileLayers {
    pub terrain: TerrainId,
    pub feature: Option<FeatureId>,
    pub resource: Option<ResourceId>,
    pub improvement: Option<ImprovementId>,
    pub route: Route,
    pub river: bool,
    pub natural_wonder: Option<NaturalWonderId>,
    pub owner: Option<PlayerId>,
}

#[derive(Debug, Clone)]
pub struct MapState {
    pub dimensions: MapDimensions,
    /// Terrain, feature, resource (and its copies: a strategic deposit holds several, every
    /// other resource one), improvement, natural wonder, owner, route, river and pillage
    /// flags of each tile. The improvement of a tile is pillaged:
    /// it yields, connects and defends nothing until a worker repairs it; only true on a tile
    /// with an improvement.
    cells: Vec<TileCell>,
    /// The city a tile belongs to (Classic: every owned plot has one owning
    /// city). Some only on owned tiles, always a city of the tile's owner.
    owning_city: Vec<Option<CityHandle>>,
    /// Number of route tiles (the path search's estimate needs to know
    /// whether a half-point road step exists anywhere).
    road_tiles: u32,
    /// Tiles with a natural wonder, ascending (a cache of the wonder layer:
    /// happiness looks at the few wonder tiles, not the whole map).
    wonder_tiles: Vec<TileId>,
    /// River beds: canonical edges (min,max) of tile ids, for meshing
    /// the shores in the render (rivers run ALONG EDGES, as in Nation). The tile flag
    /// (`FLAG_RIVER`) is a gameplay cache (ford/farms), synchronised here too.
    river_edges: Vec<(u32, u32)>,
    /// Connected water body of each water tile (index into
    /// `water_area_sizes`; `NO_AREA` on land). A cache of the terrain,
    /// rebuilt whenever terrain is set (like the river flags).
    water_area: Vec<u32>,
    water_area_sizes: Vec<u32>,
    /// Connected landmass of each land tile (Classic area; `NO_AREA` on
    /// water). Rebuilt with the water bodies.
    land_area: Vec<u32>,
    /// What each terrain code is (water, impassable), from the rules.
    terrain_kinds: crate::navigation::cost::TerrainKinds,
}

/// A water body of at most this many tiles is a lake (fresh water, +1 food).
/// Unciv: ModConstants.kt (`maxLakeSize` 10, `MapGenerator.kt` compares with
/// `<=`), equal.
pub const LAKE_MAX_AREA_SIZE: u32 = 10;
/// A coastal city touches a water body of
/// at least this many tiles (harbors, coastal buildings, sea trade).
pub const MIN_WATER_SIZE_FOR_OCEAN: u32 = 10;
/// Water area of a land tile.
pub const NO_AREA: u32 = u32::MAX;

impl MapState {
    pub fn new(dimensions: MapDimensions, default_terrain: TerrainId) -> Self {
        let n = dimensions.tile_count();
        Self {
            dimensions,
            cells: vec![TileCell::new(default_terrain); n],
            owning_city: vec![None; n],
            road_tiles: 0,
            wonder_tiles: Vec::new(),
            river_edges: Vec::new(),
            water_area: vec![NO_AREA; n],
            water_area_sizes: Vec::new(),
            land_area: vec![NO_AREA; n],
            terrain_kinds: crate::navigation::cost::TerrainKinds::LEGACY,
        }
        .with_geography()
    }

    fn with_geography(mut self) -> Self {
        self.rebuild_water_areas();
        self
    }

    /// The terrain table of the rules the map is played with.
    #[inline]
    pub fn terrain_kinds(&self) -> crate::navigation::cost::TerrainKinds {
        self.terrain_kinds
    }

    /// River flags are cached for land tiles only, so they follow the table.
    pub(crate) fn set_terrain_kinds(&mut self, kinds: crate::navigation::cost::TerrainKinds) {
        self.terrain_kinds = kinds;
        self.rebuild_river_flags();
        self.rebuild_water_areas();
    }

    #[inline]
    fn idx(&self, t: TileId) -> usize {
        t.0 as usize
    }

    #[inline]
    fn cell(&self, t: TileId) -> Option<&TileCell> {
        self.cells.get(t.0 as usize)
    }

    #[inline]
    fn cell_mut(&mut self, t: TileId) -> Option<&mut TileCell> {
        self.cells.get_mut(t.0 as usize)
    }

    /// Restore from the save schema (lengths are checked in decode before the call).
    /// Many arguments (one per layer) on purpose: the positional restore
    /// is called from exactly one place, a builder here is noise, not help.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_parts(
        dimensions: MapDimensions,
        terrain: Vec<TerrainId>,
        feature: Vec<Option<FeatureId>>,
        resource: Vec<Option<ResourceId>>,
        resource_amount: Vec<u8>,
        owner: Vec<Option<PlayerId>>,
        owning_city: Vec<Option<CityHandle>>,
        improvement: Vec<Option<ImprovementId>>,
        pillaged: TileSet,
        route: Vec<Route>,
        natural_wonder: Vec<Option<NaturalWonderId>>,
        river_edges: Vec<(u32, u32)>,
    ) -> Self {
        debug_assert_eq!(terrain.len(), dimensions.tile_count());
        debug_assert_eq!(route.len(), dimensions.tile_count());
        debug_assert_eq!(natural_wonder.len(), dimensions.tile_count());
        let road_tiles = route.iter().filter(|r| **r != Route::None).count() as u32;
        let cells: Vec<TileCell> = terrain
            .iter()
            .enumerate()
            .map(|(i, terrain)| TileCell {
                terrain: terrain.0,
                feature: pack16(feature.get(i).copied().flatten().map(|f| f.0)),
                resource: pack16(resource.get(i).copied().flatten().map(|r| r.0)),
                improvement: pack16(improvement.get(i).copied().flatten().map(|m| m.0)),
                wonder: pack16(natural_wonder.get(i).copied().flatten().map(|w| w.0)),
                owner: owner.get(i).copied().flatten().map_or(NONE8, |p| p.0),
                route: route.get(i).copied().unwrap_or_default().code(),
                amount: resource_amount.get(i).copied().unwrap_or(0),
                flags: if pillaged.contains(TileId(i as u32)) { FLAG_PILLAGED } else { 0 },
                _pad: 0,
            })
            .collect();
        let mut map = Self {
            dimensions,
            cells,
            owning_city,
            road_tiles,
            wonder_tiles: natural_wonder
                .iter()
                .enumerate()
                .filter(|(_, w)| w.is_some())
                .map(|(i, _)| TileId(i as u32))
                .collect(),
            river_edges,
            water_area: Vec::new(),
            water_area_sizes: Vec::new(),
            land_area: Vec::new(),
            terrain_kinds: crate::navigation::cost::TerrainKinds::LEGACY,
        };
        map.rebuild_river_flags();
        map.rebuild_water_areas();
        map
    }

    #[inline]
    pub fn tile_count(&self) -> usize {
        self.dimensions.tile_count()
    }

    /// Every layer a player's view reads, with one lookup (None outside the map).
    #[inline]
    pub fn layers(&self, t: TileId) -> Option<TileLayers> {
        let c = self.cell(t)?;
        Some(TileLayers {
            terrain: TerrainId(c.terrain),
            feature: some16(c.feature).map(FeatureId),
            resource: some16(c.resource).map(ResourceId),
            improvement: some16(c.improvement).map(ImprovementId),
            route: Route::from_code(c.route).unwrap_or_default(),
            river: c.flags & FLAG_RIVER != 0,
            natural_wonder: some16(c.wonder).map(NaturalWonderId),
            owner: (c.owner != NONE8).then_some(PlayerId(c.owner)),
        })
    }

    #[inline]
    pub fn terrain(&self, t: TileId) -> Option<TerrainId> {
        self.cell(t).map(|c| TerrainId(c.terrain))
    }

    #[inline]
    pub fn feature(&self, t: TileId) -> Option<Option<FeatureId>> {
        self.cell(t).map(|c| some16(c.feature).map(FeatureId))
    }

    #[inline]
    pub fn resource(&self, t: TileId) -> Option<Option<ResourceId>> {
        self.cell(t).map(|c| some16(c.resource).map(ResourceId))
    }

    /// Copies the tile's resource gives (0 without a resource).
    #[inline]
    pub fn resource_amount(&self, t: TileId) -> u8 {
        self.cell(t).map_or(0, |c| c.amount)
    }

    #[inline]
    pub fn improvement(&self, t: TileId) -> Option<Option<ImprovementId>> {
        self.cell(t).map(|c| some16(c.improvement).map(ImprovementId))
    }

    /// The tile's improvement is pillaged.
    #[inline]
    pub fn pillaged(&self, t: TileId) -> bool {
        self.cell(t).is_some_and(|c| c.flags & FLAG_PILLAGED != 0)
    }

    /// The improvement standing and not pillaged (what yields, connects
    /// and defends).
    #[inline]
    pub fn working_improvement(&self, t: TileId) -> Option<ImprovementId> {
        let c = self.cell(t)?;
        if c.flags & FLAG_PILLAGED != 0 {
            return None;
        }
        some16(c.improvement).map(ImprovementId)
    }

    #[inline]
    pub fn owner(&self, t: TileId) -> Option<Option<PlayerId>> {
        self.cell(t).map(|c| (c.owner != NONE8).then_some(PlayerId(c.owner)))
    }

    /// The city the tile belongs to (None on unowned tiles).
    #[inline]
    pub fn owning_city(&self, t: TileId) -> Option<CityHandle> {
        self.owning_city.get(self.idx(t)).copied().flatten()
    }

    /// Owner and owning city change together (one invariant: an owned tile
    /// belongs to one city of its owner).
    #[inline]
    pub fn set_owner(&mut self, t: TileId, owner: Option<PlayerId>, city: Option<CityHandle>) {
        let idx = t.0 as usize;
        if let (Some(cell), Some(city_slot)) = (self.cells.get_mut(idx), self.owning_city.get_mut(idx)) {
            cell.owner = owner.map_or(NONE8, |p| p.0);
            *city_slot = city;
        }
    }

    /// Place an improvement (build completion; overwriting is an honest replacement,
    /// the journal records the fact; razing by a separate order later).
    pub fn set_improvement(&mut self, t: TileId, imp: Option<ImprovementId>) {
        if let Some(cell) = self.cell_mut(t) {
            cell.improvement = pack16(imp.map(|i| i.0));
            // A new (or no) improvement is intact.
            cell.flags &= !FLAG_PILLAGED;
        }
    }

    /// Pillage the tile's improvement (no-op without one).
    #[inline]
    pub fn pillage_improvement(&mut self, t: TileId) {
        if let Some(cell) = self.cell_mut(t) {
            if cell.improvement != NONE16 {
                cell.flags |= FLAG_PILLAGED;
            } else {
                cell.flags &= !FLAG_PILLAGED;
            }
        }
    }

    /// Place/remove a feature (forest: the generator places, chopping removes;
    /// the journal records the fact: FeatureChopped).
    #[inline]
    pub fn set_feature(&mut self, t: TileId, f: Option<FeatureId>) {
        if let Some(cell) = self.cell_mut(t) {
            cell.feature = pack16(f.map(|f| f.0));
        }
    }

    /// Any route (road or railroad): the movement rule of roads applies.
    #[inline]
    pub fn road(&self, t: TileId) -> bool {
        self.route(t) != Route::None
    }

    #[inline]
    pub fn route(&self, t: TileId) -> Route {
        self.cell(t).map_or(Route::None, |c| Route::from_code(c.route).unwrap_or_default())
    }

    /// Build a road (work completion; journal: RoadBuilt).
    pub fn set_route(&mut self, t: TileId, route: Route) {
        if let Some(cell) = self.cells.get_mut(t.0 as usize) {
            match (cell.route != 0, route != Route::None) {
                (false, true) => self.road_tiles += 1,
                (true, false) => self.road_tiles -= 1,
                _ => {}
            }
            cell.route = route.code();
        }
    }

    /// Is there any road on the map?
    #[inline]
    pub(crate) fn has_roads(&self) -> bool {
        self.road_tiles > 0
    }

    #[inline]
    pub fn natural_wonder(&self, t: TileId) -> Option<NaturalWonderId> {
        self.cell(t).and_then(|c| some16(c.wonder)).map(NaturalWonderId)
    }

    pub(crate) fn set_natural_wonder(&mut self, t: TileId, wonder: Option<NaturalWonderId>) {
        let Some(cell) = self.cells.get_mut(t.0 as usize) else {
            return;
        };
        cell.wonder = pack16(wonder.map(|w| w.0));
        match (self.wonder_tiles.binary_search(&t), wonder) {
            (Err(at), Some(_)) => self.wonder_tiles.insert(at, t),
            (Ok(at), None) => {
                self.wonder_tiles.remove(at);
            }
            _ => {}
        }
    }

    /// Tiles with a natural wonder, ascending.
    #[inline]
    pub fn natural_wonder_tiles(&self) -> &[TileId] {
        &self.wonder_tiles
    }

    /// Does a river touch the tile?
    #[inline]
    pub fn river(&self, t: TileId) -> bool {
        self.cell(t).is_some_and(|c| c.flags & FLAG_RIVER != 0)
    }

    /// Place a river (generator and debug; in the game rivers are eternal, flooding later).
    #[cfg(any(test, feature = "harness"))]
    pub(crate) fn set_river(&mut self, t: TileId, river: bool) {
        if let Some(cell) = self.cell_mut(t) {
            if river {
                cell.flags |= FLAG_RIVER;
            } else {
                cell.flags &= !FLAG_RIVER;
            }
        }
    }

    /// Does a river run along the edge between two adjacent tiles (a river
    /// crossing)? Binary search on the canonical
    /// edge list.
    #[inline]
    pub fn river_between(&self, a: TileId, b: TileId) -> bool {
        let edge = if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) };
        self.river_edges.binary_search(&edge).is_ok()
    }

    /// River beds: canonical edges (for meshing). The getter is a slice of the truth.
    #[inline]
    pub fn river_edges(&self) -> &[(u32, u32)] {
        &self.river_edges
    }

    /// Set the beds wholesale: canonicalisation (min,max + sort + dedup) and
    /// a rebuild of the tile cache (the land ends of each edge are river).
    /// The water ends of mouth edges do not get the flag ("rivers never on
    /// water"; the edges themselves are stored, carve and meshing read them).
    /// The only write path for beds (generator, debug, load).
    pub(crate) fn set_river_edges(&mut self, edges: &[(TileId, TileId)]) {
        let mut canon: Vec<(u32, u32)> = edges
            .iter()
            .map(|(a, b)| if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) })
            .collect();
        canon.sort();
        canon.dedup();
        self.river_edges = canon;
        self.rebuild_river_flags();
    }

    /// Tile cache from edges: the land ends of each edge are river.
    /// One rule for the generator, debug and load (a save restores exactly
    /// the flags the live map had).
    fn rebuild_river_flags(&mut self) {
        for cell in self.cells.iter_mut() {
            cell.flags &= !FLAG_RIVER;
        }
        for (a, b) in &self.river_edges {
            for t in [*a, *b] {
                let Some(cell) = self.cells.get_mut(t as usize) else { continue };
                if !self.terrain_kinds.is_water(cell.terrain) {
                    cell.flags |= FLAG_RIVER;
                }
            }
        }
    }

    /// Place/remove a resource (generator and debug; the game changes them only by
    /// depletion, later; for now resources are eternal).
    #[inline]
    pub fn set_resource(&mut self, t: TileId, res: Option<ResourceId>) {
        self.set_resource_amount(t, res, 1);
    }

    /// A resource with its copies (a strategic deposit); `amount` is kept
    /// at least 1 on a resource and 0 without one.
    #[inline]
    pub fn set_resource_amount(&mut self, t: TileId, res: Option<ResourceId>, amount: u8) {
        if let Some(cell) = self.cell_mut(t) {
            cell.resource = pack16(res.map(|r| r.0));
            cell.amount = if res.is_some() { amount.max(1) } else { 0 };
        }
    }

    /// Harness-only (generator terrain before settling; derived is still empty).
    /// The caller rebuilds the geography after a batch (`rebuild_water_areas`).
    pub(crate) fn set_terrain(&mut self, t: TileId, v: TerrainId) {
        if let Some(cell) = self.cell_mut(t) {
            cell.terrain = v.0;
        }
    }

    /// A water tile (the generator's one water code).
    /// A water tile (by the rules' terrain table).
    #[inline]
    pub fn is_water(&self, t: TileId) -> bool {
        match self.terrain(t) {
            Some(code) => self.terrain_kinds.is_water(code.0),
            None => false,
        }
    }

    /// Landmass of a land tile (None on water): tiles of one area are
    /// connected over land.
    #[inline]
    pub fn land_area(&self, t: TileId) -> Option<u32> {
        let area = *self.land_area.get(self.idx(t))?;
        (area != NO_AREA).then_some(area)
    }

    /// Water body of a water tile (None on land).
    #[inline]
    pub fn water_area(&self, t: TileId) -> Option<u32> {
        let area = *self.water_area.get(self.idx(t))?;
        (area != NO_AREA).then_some(area)
    }

    /// Number of tiles of a water body.
    #[inline]
    pub fn water_area_size(&self, area: u32) -> u32 {
        self.water_area_sizes.get(area as usize).copied().unwrap_or(0)
    }

    /// A lake tile: water in a body of at most LAKE_MAX_AREA_SIZE tiles.
    #[inline]
    pub fn lake(&self, t: TileId) -> bool {
        match self.water_area(t) {
            Some(area) => self.water_area_size(area) <= LAKE_MAX_AREA_SIZE,
            None => false,
        }
    }

    /// Land next to a sea (a water body of at least MIN_WATER_SIZE_FOR_OCEAN
    /// tiles).
    #[inline]
    pub fn coastal_land(&self, t: TileId) -> bool {
        if self.is_water(t) {
            return false;
        }
        self.dimensions.neighbors(t).into_iter().any(|n| match self.water_area(n) {
            Some(area) => self.water_area_size(area) >= MIN_WATER_SIZE_FOR_OCEAN,
            None => false,
        })
    }

    /// Flood-fill water bodies over hex neighbours in tile order
    /// (deterministic ids). One rule for new maps, terrain batches and load.
    pub(crate) fn rebuild_water_areas(&mut self) {
        let (water_area, water_area_sizes) = self.flood_areas(true);
        let (land_area, _) = self.flood_areas(false);
        self.water_area = water_area;
        self.water_area_sizes = water_area_sizes;
        self.land_area = land_area;
    }

    /// Connected areas of water tiles (`water`) or land tiles: the area of
    /// each tile (`NO_AREA` for the other kind) and the size of each area.
    fn flood_areas(&self, water: bool) -> (Vec<u32>, Vec<u32>) {
        let n = self.tile_count();
        let mut areas = vec![NO_AREA; n];
        let mut sizes = Vec::new();
        let mut stack = Vec::new();
        for start in 0..n {
            let tile = TileId(start as u32);
            if areas[start] != NO_AREA || self.is_water(tile) != water {
                continue;
            }
            let area = sizes.len() as u32;
            areas[start] = area;
            stack.push(tile);
            let mut size = 0u32;
            while let Some(t) = stack.pop() {
                size += 1;
                for next in self.dimensions.neighbors(t) {
                    let i = next.0 as usize;
                    if areas[i] == NO_AREA && self.is_water(next) == water {
                        areas[i] = area;
                        stack.push(next);
                    }
                }
            }
            sizes.push(size);
        }
        (areas, sizes)
    }
}
