//! City tiles from the modular kit, and world wonders on the city ring.
//!
//! `compose` mirrors platform/tools/blender/world/city_kit.py `compose` (the contact sheets use the
//! Python twin); the kit data itself is baked from the manifest (world_models::CITY_KIT).

use crate::world::state::{CityMarker, RenderWorld};
use crate::world_models::{self, house_count, SlotKind, WorldBook, WorldModelId, CITY_KIT};

/// One piece of a city tile, game coordinates relative to the tile anchor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub model: WorldModelId,
    pub x: f32,
    pub z: f32,
    /// Degrees, +x toward +z.
    pub yaw: f32,
    /// Footprint radius; the layout keeps the piece inside the hex.
    pub radius: f32,
}

/// Slot indices of one ring in fill order, rotated by the seed.
fn rotated(outer: bool, seed: u32) -> Vec<usize> {
    let ring: Vec<usize> = CITY_KIT
        .slots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.outer == outer)
        .map(|(i, _)| i)
        .collect();
    let shift = (seed % 6) as usize * (ring.len() / 6);
    let mut out = ring[shift..].to_vec();
    out.extend_from_slice(&ring[..shift]);
    out
}

/// Free slot for a landmark: first of its ring, or the outer slot facing water.
fn pick(kind: SlotKind, free: &[usize], water_dirs: &[u8]) -> Option<usize> {
    let slots = CITY_KIT.slots;
    match kind {
        SlotKind::Inner => free.iter().copied().find(|s| !slots[*s].outer),
        SlotKind::Outer => free.iter().copied().find(|s| slots[*s].outer),
        SlotKind::Ring => None,
        SlotKind::Water => {
            let mut best: Option<(usize, f32)> = None;
            for &slot in free.iter().filter(|s| slots[**s].outer) {
                let angle = slots[slot].z.atan2(slots[slot].x).to_degrees().rem_euclid(360.0);
                let distance = water_dirs
                    .iter()
                    .map(|w| ((angle - 60.0 * *w as f32 + 180.0).rem_euclid(360.0) - 180.0).abs())
                    .fold(f32::INFINITY, f32::min);
                if best.is_none_or(|(_, d)| distance < d - 1e-6) {
                    best = Some((slot, distance));
                }
            }
            best.map(|(slot, _)| slot)
        }
    }
}

/// Pieces of one city tile. `water_dirs`: edge directions (0..6, EDGE_NEIGHBORS order)
/// whose neighbour is water; `seed`: the tile id.
pub fn compose(city: &CityMarker, book: &WorldBook, water_dirs: &[u8], seed: u32) -> Vec<Piece> {
    compose_fit(city, book, water_dirs, seed, &[])
}

/// How well a city slot suits a building on this ground: it stands as it is.
pub const SLOT_GOOD: u8 = 0;
/// Steep: used last, its plinth is tall.
pub const SLOT_POOR: u8 = 1;
/// Too steep for any building: the slot stays empty (a smaller town on a cliff tile).
pub const SLOT_NEVER: u8 = 2;
/// Ground relief across a slot up to which a building stands without a tall plinth.
const RELIEF_GOOD: f32 = 0.20;
/// Beyond this relief a slot takes nothing.
const RELIEF_NEVER: f32 = 0.35;

/// Quality of every slot of the kit for a city at (cx, cz): the relief of the ground under the
/// slot's footprint, probed at its centre and four points around it.
pub fn slot_quality(geom: &crate::scene::terrain::fields::TerrainGeometry, cx: f32, cz: f32) -> Vec<u8> {
    CITY_KIT
        .slots
        .iter()
        .map(|s| {
            let (x, z, r) = (cx + s.x, cz + s.z, s.radius);
            let points = [[x, z], [x + r, z], [x - r, z], [x, z + r], [x, z - r]];
            match super::place::relief(geom, &points) {
                Some(d) if d > RELIEF_NEVER => SLOT_NEVER,
                Some(d) if d > RELIEF_GOOD => SLOT_POOR,
                _ => SLOT_GOOD,
            }
        })
        .collect()
}

/// `compose` on uneven ground: `quality` (per kit slot, empty = all good) keeps buildings off
/// slots that are too steep and uses the poor ones last.
pub fn compose_fit(city: &CityMarker, book: &WorldBook, water_dirs: &[u8], seed: u32, quality: &[u8]) -> Vec<Piece> {
    let styles = CITY_KIT.styles;
    if styles.is_empty() {
        return Vec::new();
    }
    let era = (city.era as usize).min(styles.len() - 1);
    let center = WorldModelId(styles[era].center);
    let mut pieces = vec![Piece { model: center, x: 0.0, z: 0.0, yaw: 0.0, radius: center.model().radius }];
    // Landmarks in kit order (priority, key); one per landmark even if two buildings map to it.
    let mut chosen: Vec<usize> = city
        .buildings
        .iter()
        .filter_map(|b| book.landmark(*b))
        .filter_map(|l| CITY_KIT.landmarks.iter().position(|x| std::ptr::eq(x, l)))
        .collect();
    chosen.sort_unstable();
    chosen.dedup();
    let hidden: Vec<&str> = chosen
        .iter()
        .filter_map(|i| CITY_KIT.landmarks[*i].supersedes)
        .collect();
    let mut free = rotated(false, seed);
    free.extend(rotated(true, seed));
    let grade = |slot: usize| quality.get(slot).copied().unwrap_or(SLOT_GOOD);
    free.retain(|s| grade(*s) != SLOT_NEVER);
    free.sort_by_key(|s| grade(*s)); // stable: the fill order inside a grade is kept
    let mut slotted = 0;
    for index in chosen {
        let landmark = &CITY_KIT.landmarks[index];
        if hidden.contains(&landmark.building) {
            continue;
        }
        let model = WorldModelId(landmark.model);
        if landmark.slot == SlotKind::Ring {
            if world_models::is_wall_ring(model) {
                // The ring is drawn as parts that each take their own place on the ground.
                for part in world_models::wall_parts() {
                    pieces.push(Piece { model: part, x: 0.0, z: 0.0, yaw: 0.0, radius: part.model().radius });
                }
            } else {
                pieces.push(Piece { model, x: 0.0, z: 0.0, yaw: 0.0, radius: model.model().radius });
            }
            continue;
        }
        if slotted >= CITY_KIT.max_landmarks {
            continue;
        }
        let Some(slot) = pick(landmark.slot, &free, water_dirs) else {
            continue;
        };
        free.retain(|s| *s != slot);
        slotted += 1;
        let s = &CITY_KIT.slots[slot];
        pieces.push(Piece { model, x: s.x, z: s.z, yaw: s.yaw, radius: s.radius });
    }
    for &slot in free.iter().take(house_count(city.population.unwrap_or(0))) {
        let mut house_era = era;
        if (slot as u32 + seed).is_multiple_of(CITY_KIT.older_style_share) {
            house_era = era.saturating_sub(1);
        }
        let houses = styles[house_era].houses;
        let variant = (slot * 7 + seed as usize) % houses.len();
        let s = &CITY_KIT.slots[slot];
        let model = WorldModelId(houses[variant]);
        pieces.push(Piece { model, x: s.x, z: s.z, yaw: s.yaw, radius: s.radius });
    }
    pieces
}

/// Ring-1 then ring-2 neighbours of a tile as (q, r) offsets, deterministic order.
const RING1: [(i32, i32); 6] = crate::scene::layout::EDGE_NEIGHBORS;
const RING2: [(i32, i32); 12] = [
    (2, 0), (1, 1), (0, 2), (-1, 2), (-2, 2), (-2, 1),
    (-2, 0), (-1, -1), (0, -2), (1, -2), (2, -2), (2, -1),
];

fn offset(world: &RenderWorld, tile: u32, dq: i32, dr: i32) -> Option<u32> {
    let width = world.width as i32;
    let (q, r) = (tile as i32 % width + dq, tile as i32 / width + dr);
    let inside = q >= 0 && r >= 0 && q < width && r < world.height as i32;
    inside.then(|| (r * width + q) as u32)
}

/// Edge directions of a tile whose neighbour is water.
pub fn water_dirs(world: &RenderWorld, tile: u32) -> Vec<u8> {
    RING1
        .iter()
        .enumerate()
        .filter_map(|(dir, (dq, dr))| {
            let n = offset(world, tile, *dq, *dr)?;
            let wet = world.terrain_kinds.is_water(world.terrain.get(n as usize)?.terrain);
            wet.then_some(dir as u8)
        })
        .collect()
}

/// Footprint radius a world wonder may take from its tile centre. A city's houses have a
/// footprint radius of 0.14-0.15 and its landmarks up to 0.26: a wonder stands one step above
/// the landmarks (under 2x), so it reads as part of the city and not as a building of its own
/// that fills a whole ring tile (it was 0.80: three times a landmark).
pub const WONDER_MAX_RADIUS: f32 = 0.44;
/// Height cap of a world wonder: below the tallest city centre (center-information, 1.10), so
/// the tallest landmarks (Eiffel Tower, CN Tower, Big Ben) do not tower over the late city.
pub const WONDER_MAX_HEIGHT: f32 = 0.95;
/// A wonder on land is moved this far from its ring tile's centre toward its city, so the
/// cluster reads as one town (the tile still holds it: pull + radius < hex inradius 0.866).
pub const WONDER_PULL: f32 = 0.35;

/// Uniform scale of a world wonder model: the largest one that keeps its footprint within
/// WONDER_MAX_RADIUS and its height within WONDER_MAX_HEIGHT. Every wonder is authored to
/// fill its hex, so most are bound by the footprint and tall landmarks by the height.
pub fn wonder_scale(model: WorldModelId) -> f32 {
    let m = model.model();
    (WONDER_MAX_RADIUS / m.radius).min(WONDER_MAX_HEIGHT / m.height)
}

/// A world wonder and the tile it stands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WonderSite {
    pub model: WorldModelId,
    pub tile: u32,
    pub owner: u8,
    /// The tile of the city the wonder belongs to.
    pub city: u32,
}

/// Wonder sites of every city: each world wonder of a city (and each showpiece of the module's) takes a free known tile of its
/// ring (land, or water for harbour wonders), ring 1 first, in a per-city hashed order.
/// Cities are visited in projection order, wonders in building order; taken tiles are
/// shared, so two neighbouring cities never stack wonders.
pub fn wonder_sites(world: &RenderWorld, book: &WorldBook, relief: &dyn Fn(u32) -> f32) -> Vec<WonderSite> {
    let mut taken: std::collections::HashSet<u32> = world.cities.iter().map(|c| c.tile.0).collect();
    let mut sites = Vec::new();
    for city in &world.cities {
        let order = ring_order(world, city.tile.0);
        let showpieces = city.showpieces.iter().filter_map(|key| book.showpiece(key));
        for model in city.buildings.iter().filter_map(|b| book.wonder(*b)).chain(showpieces) {
            let water = model.model().water_site;
            let mut fits = order.iter().copied().filter(|t| !taken.contains(t) && site_fits(world, *t, water));
            // A wonder on land stands on the first ring tile whose ground is level enough for its
            // footprint; when none is, on the level-est of the first few.
            let site = if water {
                fits.next()
            } else {
                let mut best: Option<(u32, f32)> = None;
                let mut chosen = None;
                for t in fits.take(WONDER_CANDIDATES) {
                    let r = relief(t);
                    if r <= WONDER_RELIEF {
                        chosen = Some(t);
                        break;
                    }
                    if best.is_none_or(|(_, b)| r < b) {
                        best = Some((t, r));
                    }
                }
                chosen.or(best.map(|(t, _)| t))
            };
            if let Some(tile) = site {
                taken.insert(tile);
                sites.push(WonderSite { model, tile, owner: city.owner, city: city.tile.0 });
            }
        }
    }
    sites
}

/// Ground relief across a wonder's footprint up to which its tile is level enough.
const WONDER_RELIEF: f32 = 0.30;
/// Ring tiles looked at for a level one.
const WONDER_CANDIDATES: usize = 8;

/// Relief of the ground under a wonder on `tile`: centre and eight points at 0.55.
pub fn site_relief(geom: &crate::scene::terrain::fields::TerrainGeometry, centre: (f32, f32)) -> f32 {
    let mut points = vec![[centre.0, centre.1]];
    for k in 0..8 {
        let a = std::f32::consts::TAU * k as f32 / 8.0;
        points.push([centre.0 + 0.55 * a.cos(), centre.1 + 0.55 * a.sin()]);
    }
    // A tile the mesh does not cover yet is no place for a wonder: it would vanish and jump.
    super::place::relief(geom, &points).unwrap_or(f32::INFINITY)
}

fn ring_order(world: &RenderWorld, tile: u32) -> Vec<u32> {
    let rank = |t: u32| crate::scene::terrain::fields::hash_u32(t.wrapping_mul(0x9E37) ^ tile);
    let mut near: Vec<u32> = RING1.iter().filter_map(|(q, r)| offset(world, tile, *q, *r)).collect();
    let mut far: Vec<u32> = RING2.iter().filter_map(|(q, r)| offset(world, tile, *q, *r)).collect();
    near.sort_by_key(|t| rank(*t));
    far.sort_by_key(|t| rank(*t));
    near.extend(far);
    near
}

fn site_fits(world: &RenderWorld, tile: u32, water: bool) -> bool {
    let i = tile as usize;
    let Some(cell) = world.terrain.get(i) else {
        return false;
    };
    if !world.is_known(i) || world.natural_wonders.get(i).copied().flatten().is_some() {
        return false;
    }
    let wet = world.terrain_kinds.is_water(cell.terrain);
    let mountain = world.terrain_kinds.is_mountains(cell.terrain);
    if water {
        wet
    } else {
        !wet && !mountain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::ids::TileId;

    fn city(population: u32, era: u8, buildings: Vec<u16>) -> CityMarker {
        CityMarker {
            tile: TileId(0),
            owner: 0,
            name: String::new(),
            population: Some(population),
            buildings,
            era,
            showpieces: Vec::new(),
            unworked: Vec::new(),
        }
    }

    #[test]
    fn every_world_wonder_fits_its_hex_and_reads_in_one_size_class() {
        use crate::world_models::{models, WorldKind};
        let tallest_center = CITY_KIT
            .styles
            .iter()
            .map(|s| WorldModelId(s.center).model().height)
            .fold(0.0f32, f32::max);
        let band = tallest_center * 0.6..=tallest_center;
        assert!(band.contains(&WONDER_MAX_HEIGHT), "cap {WONDER_MAX_HEIGHT} outside {band:?}");
        let wonders: Vec<WorldModelId> = (0..models().len() as u16)
            .map(WorldModelId)
            .filter(|m| m.model().kind == WorldKind::Wonder)
            .collect();
        // Which wonders there are is the modules' (the classic part's 39 + the shared 8: the count is
        // asserted in classic-ui's tests); the platform alone has the shared ones.
        assert!(!wonders.is_empty(), "no wonder model");
        for id in wonders {
            let scale = wonder_scale(id);
            let mesh = id.mesh(0);
            let radius = mesh.iter().map(|v| v.pos[0].hypot(v.pos[2])).fold(0.0f32, f32::max) * scale;
            let height = mesh.iter().map(|v| v.pos[1]).fold(f32::MIN, f32::max) * scale;
            let name = id.model().id;
            assert!(radius <= WONDER_MAX_RADIUS + 1e-3, "{name}: footprint {radius:.3}");
            assert!(height <= WONDER_MAX_HEIGHT + 1e-3, "{name}: height {height:.3}");
            let at_limit = radius >= WONDER_MAX_RADIUS - 1e-3 || height >= WONDER_MAX_HEIGHT - 1e-3;
            assert!(at_limit, "{name}: {radius:.3} x {height:.3} could be larger");
        }
    }

    #[test]
    fn city_grows_with_population_and_changes_style_with_era() {
        let book = WorldBook::default();
        let small = compose(&city(1, 0, vec![]), &book, &[], 7);
        let big = compose(&city(20, 0, vec![]), &book, &[], 7);
        assert_eq!(small.len(), 1 + house_count(1));
        assert_eq!(big.len(), 1 + house_count(20));
        let modern = compose(&city(20, 7, vec![]), &book, &[], 7);
        assert_ne!(big[0].model, modern[0].model, "the center follows the era");
        assert_eq!(compose(&city(20, 7, vec![]), &book, &[], 7), modern, "deterministic");
        for piece in &big[1..] {
            assert!(piece.x.hypot(piece.z) + piece.radius <= 0.86);
        }
    }
}
