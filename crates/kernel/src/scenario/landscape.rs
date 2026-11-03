//! The land of a generated map, Classic-style: plot types, climate, rivers,
//! features and natural wonders. It follows the Classic map scripts in
//! spirit (`MapGenerator.lua`, `TerrainGenerator.lua`, `FeatureGenerator.lua`,
//! `NaturalWonders.lua`), not line by line:
//!
//! | Step | Classic | Here |
//! |---|---|---|
//! | water | fractal sea level | lowest `water_ratio` % of an elevation field, exact quota |
//! | mountains, hills | ridge fractal, ~6 % and ~14 % of land | ridges of a value-noise field, exact quotas |
//! | terrain | latitude bands with temperature and rainfall noise | the same: snow and tundra at the poles, a dry subtropical belt of desert, plains and grassland by rainfall |
//! | rivers | from hills toward the sea along plot edges | a walk along hex edges that prefers the lower side, from hills and mountains |
//! | features | ice at the poles, jungle near the equator, forest, marsh, oasis, flood plains on desert rivers, atolls | the same rules, coherent patches |
//! | natural wonders | the placement rules of the catalog | the typed rules of the ruleset (`rules::natural_wonders`) |
//!
//! Hills are a plot type on a base terrain in Classic; the map stores one code
//! per tile, so a hill tile is `Ground::Hills` and its base terrain shows
//! only in which features it may carry (`Land::base`). The land is laid in
//! grounds (`rules::roles`); the rules' palette turns them into map codes
//! (`terrain_batch`).
//!
//! Every value is integer arithmetic over hashed value noise and the seed:
//! a map reproduces bit for bit on every platform. Only rivers draw from the
//! map RNG stream.
use crate::ids::{FeatureId, TerrainId, TileId};
use crate::rules::content::NaturalWonderId;
use crate::math::MapDimensions;
use crate::rules::roles::{Climate, FeatureRole, Ground, TerrainPalette};
use crate::rules::CompiledRuleset;
pub use super::map_script::MapScript;

pub const WATER: Ground = Ground::Water;
pub const HILLS: Ground = Ground::Hills;
pub const MOUNTAIN: Ground = Ground::Mountains;
pub const PLAINS: Ground = Ground::Flat(Climate::Temperate);
pub const GRASS: Ground = Ground::Flat(Climate::Humid);
pub const DESERT: Ground = Ground::Flat(Climate::Arid);
pub const TUNDRA: Ground = Ground::Flat(Climate::Cold);
pub const SNOW: Ground = Ground::Flat(Climate::Frozen);

/// Shares of the land, in percent, by exact quota.
const MOUNTAIN_SHARE: u64 = 6;
const HILL_SHARE: u64 = 14;

/// How much of each kind of land a generated map holds (opt-in: the
/// default is the classic map, bit for bit). A bench or a test asks for a
/// richer or a barer land; the setup screen keeps the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LandOptions {
    /// Mountains, percent of the land (exact quota; classic 6).
    pub mountain_percent: u8,
    /// Hills, percent of the land (exact quota; classic 14).
    pub hill_percent: u8,
    /// Forest, percent of the classic share of a patch (100: classic).
    pub forest_percent: u16,
    /// Rivers, percent of the classic count (2–80 by the map's size; 100:
    /// classic).
    pub river_percent: u16,
    /// Land resources, percent of each resource's density (100: classic;
    /// a tile's chance is at most 100 %).
    pub resource_percent: u16,
}

impl LandOptions {
    /// The classic map: every share as it always was.
    pub const CLASSIC: LandOptions = LandOptions {
        mountain_percent: MOUNTAIN_SHARE as u8,
        hill_percent: HILL_SHARE as u8,
        forest_percent: 100,
        river_percent: 100,
        resource_percent: 100,
    };

    /// A land made for a tall science empire (the science audit's ideal
    /// case, classic/docs/ai-science-audit.md): twice the mountains, more
    /// hills, half again the forest, two and a half times the rivers,
    /// twice the land resources.
    pub const RICH: LandOptions = LandOptions {
        mountain_percent: 12,
        hill_percent: 22,
        forest_percent: 150,
        river_percent: 250,
        resource_percent: 200,
    };
}

impl Default for LandOptions {
    fn default() -> Self {
        Self::CLASSIC
    }
}

/// Edge neighbours in the order of the render's EDGE_NEIGHBORS: E, SE, SW, W, NW, NE.
const EDGE: [(i64, i64); 6] = [(1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1), (1, -1)];

/// Smoothed value noise on a lattice of `cell` tiles, 0..2^20. Integer
/// only, so reproducible on every platform.
pub fn field(q: u64, r: u64, cell: u64, salt: u64, seed: u64) -> u32 {
    let gx = q / cell;
    let gy = r / cell;
    let tx = (q % cell) * 1024 / cell;
    let ty = (r % cell) * 1024 / cell;
    let sx = tx * tx * (3072 - 2 * tx) / 1_048_576;
    let sy = ty * ty * (3072 - 2 * ty) / 1_048_576;
    let lat = |dx: u64, dy: u64| -> u64 {
        let x = (gx + dx).wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(salt);
        let y = (gy + dy).wrapping_mul(0xBF58476D1CE4E5B9).wrapping_add(seed);
        let mut z = (x ^ y.rotate_left(17)).wrapping_add(0x9E3779B97F4A7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        (z ^ (z >> 31)) % 1_048_576
    };
    let (a, b, c, d) = (lat(0, 0), lat(1, 0), lat(0, 1), lat(1, 1));
    ((a * (1024 - sx) * (1024 - sy) + b * sx * (1024 - sy) + c * (1024 - sx) * sy + d * sx * sy) / 1_048_576) as u32
}

/// A per-tile hash in 0..2^32, for rare features and tie-breaks.
pub(super) fn tile_hash(tile: u32, salt: u64, seed: u64) -> u64 {
    let mut z = (tile as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ salt.rotate_left(29) ^ seed;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    (z ^ (z >> 31)) & 0xFFFF_FFFF
}

/// The land of one map: grounds, the flat ground under hills, and the
/// latitude and rainfall the features read.
pub struct Land {
    pub width: u32,
    pub height: u32,
    pub terrain: Vec<Ground>,
    /// The flat ground a hill stands on (its climate); the ground itself elsewhere.
    pub base: Vec<Ground>,
    /// 0 at the equator, 1024 at the poles.
    latitude: Vec<u32>,
    /// 0 dry .. 1023 wet.
    rainfall: Vec<u32>,
    /// The map script's elevation: water takes its lowest share, rivers
    /// run down it.
    elevation: Vec<u32>,
    seed: u64,
    /// The shares asked for (forest and rivers read them after the plots).
    options: LandOptions,
}

impl Land {
    pub fn tiles(&self) -> usize {
        self.terrain.len()
    }

    fn dims(&self) -> MapDimensions {
        MapDimensions { width: self.width, height: self.height }
    }

    fn coord(&self, i: usize) -> (u64, u64) {
        (i as u64 % self.width as u64, i as u64 / self.width as u64)
    }

    /// Edge neighbours inside the map.
    pub fn neighbours(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        let (q, r) = self.coord(i);
        EDGE.iter().filter_map(move |&(dq, dr)| {
            let (nq, nr) = (q as i64 + dq, r as i64 + dr);
            let inside = nq >= 0 && nr >= 0 && nq < self.width as i64 && nr < self.height as i64;
            inside.then(|| (nr as u32 * self.width + nq as u32) as usize)
        })
    }

    pub fn is_water(&self, i: usize) -> bool {
        self.terrain[i] == WATER
    }

    /// 0 at the equator, 1024 at the poles.
    pub fn latitude(&self, i: usize) -> u32 {
        self.latitude[i]
    }
}

/// Plot types and climate of a map of `width`×`height` with `water_ratio`
/// percent water (the Fractal script).
pub fn land(width: u32, height: u32, water_ratio: u8, seed: u64) -> Land {
    land_shaped(width, height, water_ratio, seed, MapScript::Fractal)
}

/// Plot types and climate of a map whose land the map script shapes.
pub fn land_shaped(width: u32, height: u32, water_ratio: u8, seed: u64, script: MapScript) -> Land {
    land_shaped_with(width, height, water_ratio, seed, script, LandOptions::CLASSIC)
}

/// `land_shaped` with the shares of `options` (`LandOptions::CLASSIC` is
/// `land_shaped`).
pub fn land_shaped_with(width: u32, height: u32, water_ratio: u8, seed: u64, script: MapScript, options: LandOptions) -> Land {
    let tiles = width as u64 * height as u64;
    let w = width as u64;
    let mut terrain = vec![PLAINS; tiles as usize];
    // Water: the lowest share of the elevation field, an exact quota.
    let elevation = script.elevation(width, height, seed);
    let mut order: Vec<u32> = (0..tiles as u32).collect();
    order.sort_by_key(|&i| (elevation[i as usize], i));
    let water = (tiles * water_ratio as u64 / 100) as usize;
    for &i in &order[..water] {
        terrain[i as usize] = WATER;
    }
    // Mountains on ridges, hills on their flanks and on lesser ridges.
    let mut land: Vec<u32> = order[water..].to_vec();
    land.sort_by_key(|&i| (std::cmp::Reverse(relief(i as u64 % w, i as u64 / w, seed)), i));
    let mountains = land.len() * options.mountain_percent.min(100) as usize / 100;
    let hills = (land.len() * options.hill_percent.min(100) as usize / 100).min(land.len() - mountains);
    let latitude: Vec<u32> = (0..tiles).map(|i| latitude(i / w, height as u64)).collect();
    let rainfall: Vec<u32> = (0..tiles).map(|i| rainfall(i % w, i / w, seed)).collect();
    let base: Vec<Ground> = (0..tiles as usize)
        .map(|i| climate(latitude[i], rainfall[i], temperature_noise(i as u64 % w, i as u64 / w, seed)))
        .collect();
    for (k, &i) in land.iter().enumerate() {
        let i = i as usize;
        terrain[i] = if k < mountains {
            MOUNTAIN
        } else if k < mountains + hills {
            HILLS
        } else {
            base[i]
        };
    }
    let base = (0..tiles as usize).map(|i| if terrain[i] == WATER { WATER } else { base[i] }).collect();
    Land { width, height, terrain, base, latitude, rainfall, elevation, seed, options }
}

/// Ridge lines of one field, broken by a broad mask, plus small knolls.
fn relief(q: u64, r: u64, seed: u64) -> u64 {
    let f = field(q, r, 8, 0x6D0E, seed) as i64;
    let ridge = (1_048_576 - (2 * f - 1_048_576).abs()) as u64;
    let mask = field(q, r, 13, 0x3A5C, seed) as u64 + 524_288;
    let knolls = field(q, r, 3, 0x41F1, seed) as u64;
    (ridge * mask >> 20) + knolls / 3
}

/// 0 at the equator, 1024 at the poles.
fn latitude(r: u64, height: u64) -> u32 {
    let doubled = (2 * r + 1).abs_diff(height);
    (doubled * 1024 / height.max(1)) as u32
}

fn rainfall(q: u64, r: u64, seed: u64) -> u32 {
    let broad = field(q, r, 9, 0x3A1D, seed) as u64;
    let fine = field(q, r, 4, 0x3A1E, seed) as u64;
    ((broad * 3 + fine) / 4 >> 10) as u32
}

/// ±170 on the latitude: ragged climate band edges.
fn temperature_noise(q: u64, r: u64, seed: u64) -> i64 {
    (field(q, r, 7, 0x7E3, seed) as i64 - 524_288) / 3084
}

/// The flat ground of a climate. Snow and tundra by temperature, a dry
/// subtropical belt of desert, plains where it is drier, grassland where wet.
fn climate(latitude: u32, rainfall: u32, noise: i64) -> Ground {
    let cold = latitude as i64 + noise;
    if cold > 955 {
        return SNOW;
    }
    if cold > 840 {
        return TUNDRA;
    }
    let belt = (1024 - (latitude as i64 - 300).abs() * 4).max(0);
    if cold < 680 && (rainfall as i64) < 255 + belt * 210 / 1024 {
        return DESERT;
    }
    if rainfall < 540 {
        PLAINS
    } else {
        GRASS
    }
}

/// River edges: walks along hex edges from hills and mountains toward the
/// lower side, ending in water. `next` draws from the map RNG.
pub fn rivers(land: &Land, mut next: impl FnMut(u64) -> u64) -> Vec<(TileId, TileId)> {
    let (w, h) = (land.width as i64, land.height as i64);
    let in_map = |q: i64, r: i64| q >= 0 && r >= 0 && q < w && r < h;
    let at = |q: i64, r: i64| (r * w + q) as usize;
    let is_water = |q: i64, r: i64| !in_map(q, r) || land.is_water(at(q, r));
    let elevation = |q: i64, r: i64| -> u64 {
        if is_water(q, r) {
            return 0;
        }
        let t = land.terrain[at(q, r)];
        let lift = match t {
            MOUNTAIN => 3_000_000,
            HILLS => 1_500_000,
            _ => 0,
        };
        lift + land.elevation[at(q, r)] as u64
    };
    // A vertex is corner k of tile T; it touches T, T + E[k-1] and T + E[k].
    let vertex_height = |q: i64, r: i64, k: usize| {
        let (a, b) = (EDGE[(k + 5) % 6], EDGE[k]);
        elevation(q, r) + elevation(q + a.0, r + a.1) + elevation(q + b.0, r + b.1)
    };
    let sources: Vec<usize> = (0..land.tiles())
        .filter(|&i| matches!(land.terrain[i], HILLS | MOUNTAIN) && land.neighbours(i).all(|n| !land.is_water(n)))
        .collect();
    let classic = (land.tiles() / 350).clamp(2, 80);
    let count = if land.options.river_percent == 100 { classic } else { classic * usize::from(land.options.river_percent) / 100 };
    let mut edges = vec![];
    for _ in 0..count {
        if sources.is_empty() {
            break;
        }
        let source = sources[next(sources.len() as u64) as usize];
        let (mut q, mut r, mut k) = (source as i64 % w, source as i64 / w, next(6) as usize);
        let mut arrival: Option<((i64, i64), usize)> = None;
        let length = 8 + next(17);
        let mut path = vec![];
        for _ in 0..length {
            let (km1, kp1) = ((k + 5) % 6, (k + 1) % 6);
            let (lq, lr) = (q + EDGE[km1].0, r + EDGE[km1].1);
            let (rq, rr) = (q + EDGE[k].0, r + EDGE[k].1);
            // e1 = (T, k-1) and e2 = (T, k) run between land tiles; e3 = (L, k+1)
            // between L and R, a mouth when exactly one of them is water.
            let e1 = in_map(lq, lr) && !is_water(lq, lr) && !is_water(q, r);
            let e2 = in_map(rq, rr) && !is_water(rq, rr) && !is_water(q, r);
            let e3_in = in_map(lq, lr) && in_map(rq, rr);
            let e3 = e3_in && !(is_water(lq, lr) && is_water(rq, rr));
            let mouth = e3 && is_water(lq, lr) != is_water(rq, rr);
            let id = |q: i64, r: i64| (r * w + q) as u32;
            let mut options: Vec<((u32, u32), (i64, i64, usize), ((i64, i64), usize), bool)> = vec![];
            if e1 {
                let (a, b) = (id(q, r), id(lq, lr));
                options.push(((a.min(b), a.max(b)), (q, r, km1), ((q, r), km1), false));
            }
            if e2 {
                let (a, b) = (id(q, r), id(rq, rr));
                options.push(((a.min(b), a.max(b)), (q, r, kp1), ((q, r), k), false));
            }
            if e3 {
                let (a, b) = (id(lq, lr), id(rq, rr));
                options.push(((a.min(b), a.max(b)), (lq, lr, kp1), ((lq, lr), kp1), mouth));
            }
            if let Some(arrived) = arrival {
                options.retain(|o| o.2 != arrived);
            }
            options.retain(|o| !path.contains(&o.0));
            if options.is_empty() {
                break;
            }
            // Mostly downhill: the lowest next vertex two times in three.
            options.sort_by_key(|o| (u64::from(!o.3), vertex_height(o.1 .0, o.1 .1, o.1 .2), o.0));
            let pick = if next(3) == 0 { next(options.len() as u64) as usize } else { 0 };
            let (pair, (nq, nr, nk), arrived, is_mouth) = options.swap_remove(pick);
            path.push(pair);
            if is_mouth {
                break;
            }
            (q, r, k) = (nq, nr, nk);
            arrival = Some(arrived);
        }
        edges.extend(path.into_iter().map(|(a, b)| (TileId(a), TileId(b))));
    }
    edges.sort_by_key(|&(a, b)| (a.0, b.0));
    edges.dedup();
    edges
}

/// Features of the generated land (Classic FeatureGenerator): ice on polar
/// water, atolls in warm shallow seas, jungle near the equator, forest,
/// marsh on wet grassland, flood plains on desert rivers, oases in the open
/// desert. Fallout only comes from nuclear weapons.
pub fn features<M: crate::module::RulesModule>(land: &Land, river: &[bool], rules: &CompiledRuleset<M>) -> Vec<Option<FeatureId>> {
    let id = |role| rules.roles.feature(role);
    let kind = Kinds {
        forest: id(FeatureRole::Forest),
        jungle: id(FeatureRole::Jungle),
        marsh: id(FeatureRole::Marsh),
        oasis: id(FeatureRole::Oasis),
        flood_plains: id(FeatureRole::FloodPlains),
        ice: id(FeatureRole::Ice),
        atoll: id(FeatureRole::Atoll),
    };
    let mut out: Vec<Option<FeatureId>> = vec![None; land.tiles()];
    for i in 0..land.tiles() {
        let feature = feature_at(land, river, &kind, &out, i);
        out[i] = feature.filter(|f| fits_site(land, river, rules, *f, i));
    }
    out
}

/// The feature's own site rules (`FeatureDefinition::fits_site`): the
/// generator's climate picks a feature, the rules have the last word.
fn fits_site<M: crate::module::RulesModule>(land: &Land, river: &[bool], rules: &CompiledRuleset<M>, feature: FeatureId, i: usize) -> bool {
    let terrain = land.terrain[i];
    let flat = terrain != HILLS && terrain != MOUNTAIN;
    match rules.features.get(feature.0 as usize) {
        Some(def) => def.fits_site(flat, river[i]),
        None => true,
    }
}

struct Kinds {
    forest: Option<FeatureId>,
    jungle: Option<FeatureId>,
    marsh: Option<FeatureId>,
    oasis: Option<FeatureId>,
    flood_plains: Option<FeatureId>,
    ice: Option<FeatureId>,
    atoll: Option<FeatureId>,
}

fn feature_at(land: &Land, river: &[bool], kind: &Kinds, placed: &[Option<FeatureId>], i: usize) -> Option<FeatureId> {
    let (q, r) = land.coord(i);
    let seed = land.seed;
    let (lat, wet) = (land.latitude[i], land.rainfall[i]);
    let patch = |cell: u64, salt: u64| field(q, r, cell, salt, seed) >> 10;
    if land.is_water(i) {
        let polar = lat as i64 + (patch(3, 0x1CE) as i64 - 512) / 6;
        if polar > 900 {
            return kind.ice;
        }
        let land_near = land.neighbours(i).filter(|&n| !land.is_water(n)).count();
        let warm = lat < 560;
        if warm && land_near == 0 && tile_hash(i as u32, 0xA7011, seed) % 45 == 0 {
            let shallow = land.neighbours(i).any(|n| land.neighbours(n).any(|m| !land.is_water(m)));
            if shallow {
                return kind.atoll;
            }
        }
        return None;
    }
    let terrain = land.terrain[i];
    let base = land.base[i];
    let flat = terrain != HILLS && terrain != MOUNTAIN;
    if terrain == MOUNTAIN || base == SNOW {
        return None;
    }
    if base == DESERT {
        if !flat {
            return None;
        }
        if river[i] {
            return kind.flood_plains;
        }
        let open = land.neighbours(i).all(|n| !land.is_water(n) && placed[n] != kind.oasis);
        if open && tile_hash(i as u32, 0x0A515, seed) % 24 == 0 {
            return kind.oasis;
        }
        return None;
    }
    if base == GRASS && lat < 330 && wet > 560 && patch(4, 0x1A9) < 640 {
        return kind.jungle;
    }
    if base == GRASS && flat && wet > 740 && patch(3, 0x3A55) < 330 {
        return kind.marsh;
    }
    let share = if base == TUNDRA { 470 } else { 380 };
    let share = share * u32::from(land.options.forest_percent) / 100;
    if patch(5, 0xF04E) < share {
        return kind.forest;
    }
    None
}

/// Natural wonders a map of this many tiles holds (Classic worlds: duel 2 to huge 7).
pub fn natural_wonder_count(tiles: usize) -> usize {
    match tiles {
        0..=1_200 => 2,
        1_201..=2_500 => 3,
        2_501..=4_500 => 4,
        4_501..=8_000 => 5,
        8_001..=16_000 => 6,
        _ => 7,
    }
}

/// No two natural wonders closer than this.
const WONDER_SPACING: i32 = 7;

/// Natural wonder sites by the placement rules of the ruleset
/// (`rules::natural_wonders`), and the core tile changes they ask for (a
/// mountain, flat land of a terrain, water around, no feature).
/// Deterministic: wonders and tiles are ranked by a hash of the seed.
pub fn natural_wonders<M: crate::module::RulesModule>(land: &mut Land, features: &mut [Option<FeatureId>], river: &[bool], rules: &CompiledRuleset<M>) -> Vec<(TileId, NaturalWonderId)> {
    let mut wonders: Vec<(u64, usize)> = (0..rules.natural_wonders.len())
        .map(|n| (tile_hash(n as u32, 0x5EED_0F_A11, land.seed), n))
        .collect();
    wonders.sort();
    let biggest = biggest_landmass(land);
    let dims = land.dims();
    let wanted = natural_wonder_count(land.tiles());
    let mut sites: Vec<(TileId, NaturalWonderId)> = vec![];
    for (_, n) in wonders {
        if sites.len() >= wanted {
            break;
        }
        let site = Site { rules, placement: &rules.natural_wonders[n].placement };
        let far = |i: usize| sites.iter().all(|(t, _)| dims.distance(TileId(i as u32), *t).is_none_or(|d| d >= WONDER_SPACING));
        let best = (0..land.tiles())
            .filter(|&i| !river[i] && far(i) && !(site.placement.avoid_biggest_landmass && biggest[i]))
            .filter(|&i| site.allows(land, features, i))
            .min_by_key(|&i| (tile_hash(i as u32, 0xC0DE + n as u64, land.seed), i));
        let Some(core) = best else { continue };
        site.change(land, features, core);
        sites.push((TileId(core as u32), NaturalWonderId(n as u16)));
    }
    sites.sort_by_key(|s| s.0 .0);
    sites
}

/// Land tiles of the largest landmass (connected land; ties go to the
/// landmass found first in tile order).
fn biggest_landmass(land: &Land) -> Vec<bool> {
    let mut mass = vec![usize::MAX; land.tiles()];
    let mut sizes: Vec<usize> = Vec::new();
    for start in 0..land.tiles() {
        if land.is_water(start) || mass[start] != usize::MAX {
            continue;
        }
        let id = sizes.len();
        let mut stack = vec![start];
        mass[start] = id;
        let mut size = 0;
        while let Some(i) = stack.pop() {
            size += 1;
            for n in land.neighbours(i) {
                if !land.is_water(n) && mass[n] == usize::MAX {
                    mass[n] = id;
                    stack.push(n);
                }
            }
        }
        sizes.push(size);
    }
    let largest = sizes.iter().enumerate().max_by_key(|(id, size)| (**size, usize::MAX - id)).map(|(id, _)| id);
    mass.iter().map(|m| Some(*m) == largest).collect()
}

/// One natural wonder's placement rules over the generated land.
struct Site<'a, M: crate::module::RulesModule> {
    rules: &'a CompiledRuleset<M>,
    placement: &'a crate::rules::natural_wonders::NaturalWonderPlacement,
}

impl<M: crate::module::RulesModule> Site<'_, M> {
    /// The ground a terrain kind plays (None: the map has no code for it).
    fn ground(&self, kind: crate::rules::content::TerrainKindId) -> Option<Ground> {
        self.rules.roles.ground_of(kind)
    }

    fn allows(&self, land: &Land, features: &[Option<FeatureId>], i: usize) -> bool {
        use crate::rules::natural_wonders::PlacementMethod;
        let (q, r) = land.coord(i);
        let edge = q < 2 || r < 2 || q + 2 >= land.width as u64 || r + 2 >= land.height as u64;
        if edge {
            return false;
        }
        let water_around = land.neighbours(i).filter(|&n| land.is_water(n)).count();
        let land_around = land.neighbours(i).count() - water_around;
        let terrain = land.terrain[i];
        match self.placement.method {
            // A reef: open water near a coast.
            PlacementMethod::Reef => {
                terrain == WATER && water_around == 6 && land.neighbours(i).any(|n| land.neighbours(n).any(|m| !land.is_water(m)))
            }
            // A rock at the tip of a peninsula.
            PlacementMethod::PeninsulaTip => {
                terrain != WATER && terrain != MOUNTAIN && water_around >= 4 && land_around >= 1
            }
            PlacementMethod::Water => terrain == WATER && self.neighbours_fit(land, features, i),
            PlacementMethod::Land => {
                let core_fits = terrain != WATER && terrain != MOUNTAIN && land.base[i] != SNOW;
                let coast_fits = !self.placement.avoid_ocean_coast || water_around == 0;
                core_fits && coast_fits && self.core_terrain_fits(land, i) && self.neighbours_fit(land, features, i)
            }
        }
    }

    /// A core restricted to some base terrains must be flat land of one of them.
    fn core_terrain_fits(&self, land: &Land, i: usize) -> bool {
        if self.placement.core_terrains.is_empty() {
            return true;
        }
        let listed = self.placement.core_terrains.iter().any(|k| self.ground(*k) == Some(land.base[i]));
        listed && land.terrain[i] != HILLS
    }

    fn neighbours_fit(&self, land: &Land, features: &[Option<FeatureId>], i: usize) -> bool {
        use crate::rules::natural_wonders::AdjacentBound;
        self.placement.adjacent.iter().all(|rule| {
            let count = land.neighbours(i).filter(|&n| self.counts_as(land, features, n, rule.kind)).count() as u8;
            match rule.bound {
                AdjacentBound::AtLeast(n) => count >= n,
                AdjacentBound::AtMost(n) => count <= n,
            }
        })
    }

    fn counts_as(
        &self,
        land: &Land,
        features: &[Option<FeatureId>],
        n: usize,
        kind: crate::rules::natural_wonders::AdjacentKind<crate::rules::content::TerrainKindId, FeatureId>,
    ) -> bool {
        use crate::rules::natural_wonders::AdjacentKind;
        let terrain = land.terrain[n];
        match kind {
            AdjacentKind::Hills => terrain == HILLS,
            AdjacentKind::Mountain => terrain == MOUNTAIN,
            AdjacentKind::HillsOrMountain => terrain == HILLS || terrain == MOUNTAIN,
            AdjacentKind::Land => terrain != WATER,
            AdjacentKind::Water => terrain == WATER,
            AdjacentKind::Terrain(t) => self.ground(t) == Some(land.base[n]),
            AdjacentKind::Feature(f) => features[n] == Some(f),
        }
    }

    /// The core tile becomes what the wonder stands on.
    fn change(&self, land: &mut Land, features: &mut [Option<FeatureId>], i: usize) {
        use crate::rules::natural_wonders::{CoreShape, PlacementMethod};
        features[i] = None;
        if matches!(self.placement.method, PlacementMethod::Reef | PlacementMethod::PeninsulaTip) {
            return;
        }
        if let Some(ground) = self.placement.core_terrain.and_then(|k| self.ground(k)) {
            land.base[i] = ground;
        }
        match self.placement.core_shape {
            CoreShape::Mountain => land.terrain[i] = MOUNTAIN,
            CoreShape::Flat => land.terrain[i] = land.base[i],
            CoreShape::Keep if land.terrain[i] != HILLS => land.terrain[i] = land.base[i],
            CoreShape::Keep => {}
        }
        if self.placement.surround_with_water {
            let around: Vec<usize> = land.neighbours(i).collect();
            for n in around {
                land.terrain[n] = WATER;
                land.base[n] = WATER;
                features[n] = None;
            }
        }
    }
}

/// The terrain batch of a land in the rules' map codes, for
/// `debug_set_terrain_batch`.
pub(crate) fn terrain_batch(land: &Land, palette: TerrainPalette) -> Vec<(TileId, TerrainId)> {
    land.terrain.iter().enumerate().map(|(i, &t)| (TileId(i as u32), TerrainId(palette.code(t)))).collect()
}

/// Land tiles touched by a river edge (Classic: a river runs along the tile).
pub fn river_tiles(land: &Land, edges: &[(TileId, TileId)]) -> Vec<bool> {
    let mut out = vec![false; land.tiles()];
    for &(a, b) in edges {
        for t in [a, b] {
            if !land.is_water(t.0 as usize) {
                out[t.0 as usize] = true;
            }
        }
    }
    out
}
