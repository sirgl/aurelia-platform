//! Stage 1 of the field (platform/MESHING.md §2.2): biomes, and the hill form of stage 2.
//!
//! Every known cell has a look (`Look`): its terrain, the base terrain whose
//! colors it wears (a hill or a mountain foot wears the climate around it,
//! since the map stores hills as their own code), and its feature. The base
//! of a point is a weighted mix of the known cells within `REACH`: heights
//! from their terrain, colors from their materials. Hidden cells never
//! contribute.
//!
//! Colors and heights blend differently. Heights blend over the whole reach,
//! so the ground has no step at a hex edge. Materials blend over a narrower,
//! domain-warped reach: a desert meets the plains along an irregular line
//! with a short soft band, never along a hex outline and never as a two-hex
//! gradient. Each material is evaluated once per point (with its own mottling
//! noise) and mixed by its share of the weights.
use super::hydrology::{noise, Point};
use crate::{
    scene::{fog::fog_at, layout::tile_to_world, style::CompiledTerrainStyle},
    world::state::{FeatureKind, RenderWorld},
};

/// Cells farther than this from a point do not shape it.
const REACH: f32 = 2.35;
/// Cells farther than this from the warped point do not color it.
const COLOR_REACH: f32 = 1.75;
/// Domain warp of the color mix, world units.
const WARP: f32 = 0.55;

const PLAINS: u16 = 0;
const GRASS: u16 = 1;
const HILLS: u16 = 2;
const DESERT: u16 = 3;
const SEA: u16 = 4;
const SNOW: u16 = 5;
const MOUNTAIN: u16 = 6;
const TUNDRA: u16 = 7;

/// The ground a coast, river or cliff then reshapes.
pub struct Ground {
    pub height: f32,
    /// Share of `height` that is hill relief, not the cell's base height.
    pub hill: f32,
    /// Height from land cells only: the plateau a cliff cuts, not lowered by the sea.
    pub land_height: f32,
    pub color: [f32; 3],
    /// Share of the fallout material in the colour mix (0..1).
    pub fallout: f32,
    pub terrain: u16,
    /// The tint of the water that crosses this ground, as a position on the water palette
    /// (0 clear mountain water, 0.3 lowland, 0.62 turbid or marshy, 1 icy): the mix of the
    /// cells around, so it never steps at a hex edge.
    pub tint: f32,
}

/// The place of a cell's water on the palette (`Ground::tint`).
fn tint_of(look: Look) -> f32 {
    match (look.feature, look.terrain) {
        (FeatureKind::Marsh | FeatureKind::FloodPlains, _) => 0.66,
        (FeatureKind::Ice, _) | (_, SNOW | TUNDRA) => 1.0,
        (_, HILLS | MOUNTAIN) => 0.0,
        (_, DESERT) => 0.62,
        _ => 0.30,
    }
}

/// What a known cell looks like.
#[derive(Debug, Clone, Copy, Default)]
pub struct Look {
    pub known: bool,
    pub terrain: u16,
    /// The terrain whose colors the cell wears.
    pub base: u16,
    pub feature: FeatureKind,
}

/// Looks of every cell, derived once per world observation.
pub struct Biomes {
    width: u32,
    height: u32,
    looks: Vec<Look>,
}

pub fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

impl Biomes {
    pub fn new(world: &RenderWorld) -> Self {
        let mut looks: Vec<Look> = world
            .terrain
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let known = fog_at(world, cell.tile) >= 0.4;
                let feature = match world.feature.get(i) {
                    Some(&kind) => kind,
                    None if world.forest.get(i).is_some_and(|&f| f != 0) => FeatureKind::Forest,
                    None => FeatureKind::None,
                };
                Look { known, terrain: cell.terrain, base: cell.terrain, feature }
            })
            .collect();
        let bases: Vec<u16> = (0..looks.len()).map(|i| climate_of(world, &looks, i)).collect();
        for (look, base) in looks.iter_mut().zip(bases) {
            look.base = base;
        }
        Self { width: world.width, height: world.height, looks }
    }

    /// A cell's look in one word, for region fingerprints.
    pub fn look_key(&self, tile: usize) -> u64 {
        let look = self.looks.get(tile).copied().unwrap_or_default();
        u64::from(look.known) | u64::from(look.terrain) << 8 | u64::from(look.base) << 24 | (look.feature as u64) << 40
    }

    #[cfg(test)]
    pub fn look(&self, tile: usize) -> Look {
        self.looks.get(tile).copied().unwrap_or_default()
    }

    /// Known cells within `reach` of a point: tile, look, centre, distance.
    fn near(&self, x: f32, z: f32, reach: f32) -> impl Iterator<Item = (u32, Look, Point, f32)> + '_ {
        let r0 = (z / 1.5).round() as i32;
        let q0 = (x / 1.7320508 - z / 3.).round() as i32;
        let (w, h) = (self.width as i32, self.height as i32);
        (r0 - 2..=r0 + 2).flat_map(move |r| (q0 - 2..=q0 + 2).map(move |q| (q, r))).filter_map(move |(q, r)| {
            if q < 0 || r < 0 || q >= w || r >= h {
                return None;
            }
            let tile = (r * w + q) as u32;
            let look = self.looks[tile as usize];
            if !look.known {
                return None;
            }
            let (cx, cz) = tile_to_world(q as u32, r as u32);
            let d = (x - cx).hypot(z - cz);
            (d < reach).then_some((tile, look, [cx, cz], d))
        })
    }

    /// Stages 1 and 2 (hills) at a point. Where no known cell is near, a neutral grass plain.
    pub fn ground(&self, style: &CompiledTerrainStyle, x: f32, z: f32) -> Ground {
        let mut total = 0.;
        let mut height = 0.;
        let mut land_total = 0.;
        let mut land_height = 0.;
        let mut strongest = 0.;
        let mut terrain = GRASS;
        let mut hill = 0.0f32;
        let mut tint = 0.;
        for (tile, look, center, d) in self.near(x, z, REACH) {
            let weight = (1. - d / REACH).powi(3);
            tint += tint_of(look) * weight;
            let h = cell_height(style, look);
            height += h * weight;
            if look.terrain != SEA {
                land_height += h * weight;
                land_total += weight;
            }
            total += weight;
            if weight > strongest {
                strongest = weight;
                terrain = look.terrain;
            }
            if look.terrain == HILLS {
                hill = hill.max(hill_relief(tile, x - center[0], z - center[1]));
            }
        }
        if total < 1e-6 {
            return Ground { height: 0.3, hill: 0., land_height: 0.3, color: grass([x, z]), fallout: 0., terrain: GRASS, tint: 0.3 };
        }
        let height = height / total;
        let land_height = if land_total < 1e-6 { height } else { land_height / land_total };
        let folds = (noise(x * 0.48, z * 0.48, 883) - 0.5) * 0.23 + (noise(x * 1.08, z * 1.08, 461) - 0.5) * 0.075;
        Ground {
            height: height + hill + folds,
            hill,
            land_height: land_height + hill + folds,
            color: self.color(x, z),
            fallout: self.fallout_share(x, z),
            terrain,
            tint: tint / total,
        }
    }

    /// The share of the fallout material in the mix at a point (the weights of `color`).
    fn fallout_share(&self, x: f32, z: f32) -> f32 {
        let warp = [(noise(x * 0.35, z * 0.35, 1201) - 0.5) * 2. * WARP, (noise(x * 0.35, z * 0.35, 1307) - 0.5) * 2. * WARP];
        let (wx, wz) = (x + warp[0], z + warp[1]);
        let (mut fallout, mut total) = (0f32, 0f32);
        for (_, look, _, d) in self.near(wx, wz, COLOR_REACH) {
            let weight = (1. - d / COLOR_REACH).powi(4);
            if material(look) == FALLOUT {
                fallout += weight;
            }
            total += weight;
        }
        if total < 1e-6 { 0. } else { fallout / total }
    }

    /// The material mix at a point.
    fn color(&self, x: f32, z: f32) -> [f32; 3] {
        let warp = [(noise(x * 0.35, z * 0.35, 1201) - 0.5) * 2. * WARP, (noise(x * 0.35, z * 0.35, 1307) - 0.5) * 2. * WARP];
        let (wx, wz) = (x + warp[0], z + warp[1]);
        let mut shares = [0f32; MATERIALS];
        let mut total = 0.;
        let mut oasis: Option<Point> = None;
        for (_, look, center, d) in self.near(wx, wz, COLOR_REACH) {
            let weight = (1. - d / COLOR_REACH).powi(4);
            shares[material(look) as usize] += weight;
            total += weight;
            if look.feature == FeatureKind::Oasis {
                oasis = Some(center);
            }
        }
        if total < 1e-6 {
            // Only cells of the wider height reach: the nearest known look.
            let nearest = self.near(x, z, REACH).min_by(|a, b| a.3.total_cmp(&b.3));
            return match nearest {
                Some((_, look, _, _)) => paint(material(look), [x, z]),
                None => grass([x, z]),
            };
        }
        let mut color = [0.; 3];
        for (m, share) in shares.iter().enumerate() {
            if *share <= 0. {
                continue;
            }
            let c = paint(m as u8, [x, z]);
            for k in 0..3 {
                color[k] += c[k] * share / total;
            }
        }
        match oasis {
            Some(center) => oasis_ring(color, [x, z], center),
            None => color,
        }
    }
}

/// Lakes: enclosed water bodies of at most this many cells; larger ones are sea. Unciv:
/// ModConstants.kt (`maxLakeSize` 10), equal (the rules' `LAKE_MAX_AREA_SIZE`).
pub const LAKE_MAX_CELLS: usize = 10;

impl Biomes {
    /// Lake cells: known water whose whole body is known, enclosed by known
    /// land away from the map edge, and no larger than `LAKE_MAX_CELLS`. A
    /// body the observer has not seen whole stays sea until it is seen.
    pub fn lakes(&self) -> Vec<bool> {
        let (w, h) = (self.width as i32, self.height as i32);
        let mut lake = vec![false; self.looks.len()];
        let mut seen = vec![false; self.looks.len()];
        for start in 0..self.looks.len() {
            let look = self.looks[start];
            if seen[start] || !look.known || look.terrain != SEA {
                continue;
            }
            let mut body = vec![start];
            let mut pending = vec![start];
            let mut enclosed = true;
            seen[start] = true;
            while let Some(i) = pending.pop() {
                let (q, r) = ((i as i32) % w, (i as i32) / w);
                for (dq, dr) in crate::scene::layout::EDGE_NEIGHBORS {
                    let (nq, nr) = (q + dq, r + dr);
                    if nq < 0 || nr < 0 || nq >= w || nr >= h {
                        enclosed = false;
                        continue;
                    }
                    let n = (nr * w + nq) as usize;
                    let other = self.looks[n];
                    if !other.known {
                        enclosed = false;
                    } else if other.terrain == SEA && !seen[n] {
                        seen[n] = true;
                        body.push(n);
                        pending.push(n);
                    }
                }
            }
            if enclosed && body.len() <= LAKE_MAX_CELLS {
                for i in body {
                    lake[i] = true;
                }
            }
        }
        lake
    }
}

/// The climate a hill or mountain wears: the most common base of the known
/// flat land around it (ring 1 counts double), grassland when none is near.
fn climate_of(world: &RenderWorld, looks: &[Look], i: usize) -> u16 {
    let look = looks[i];
    if !matches!(look.terrain, HILLS | MOUNTAIN) {
        return look.terrain;
    }
    let (w, h) = (world.width as i32, world.height as i32);
    let (q, r) = ((i as u32 % world.width) as i32, (i as u32 / world.width) as i32);
    let mut votes = [0u32; 8];
    for dr in -2i32..=2 {
        for dq in -2i32..=2 {
            let ring = dq.abs().max(dr.abs()).max((dq + dr).abs());
            let (nq, nr) = (q + dq, r + dr);
            if ring == 0 || ring > 2 || nq < 0 || nr < 0 || nq >= w || nr >= h {
                continue;
            }
            let other = looks[(nr * w + nq) as usize];
            if other.known && matches!(other.terrain, PLAINS | GRASS | DESERT | SNOW | TUNDRA) {
                votes[other.terrain as usize] += if ring == 1 { 2 } else { 1 };
            }
        }
    }
    let best = (0..votes.len()).max_by_key(|&t| (votes[t], std::cmp::Reverse(t))).unwrap_or(GRASS as usize);
    if votes[best] == 0 {
        GRASS
    } else {
        best as u16
    }
}

fn cell_height(style: &CompiledTerrainStyle, look: Look) -> f32 {
    match (look.terrain, look.feature) {
        // An atoll's reef lifts the sea floor close under the surface.
        (SEA, FeatureKind::Atoll) => -0.04,
        (SEA, _) => -0.34,
        (HILLS, _) => 0.98,
        (MOUNTAIN, _) => 0.88,
        // Wetlands and river flats lie low and level.
        (_, FeatureKind::Marsh) => 0.20,
        (_, FeatureKind::FloodPlains) => 0.21,
        (t, _) => style.height(t).unwrap_or(0.3),
    }
}

/// Materials of the color mix.
const GRASSLAND: u8 = 0;
const STEPPE: u8 = 1;
const SAND: u8 = 2;
const LICHEN: u8 = 3;
const FIRN: u8 = 4;
const FOREST_FLOOR: u8 = 5;
const JUNGLE_FLOOR: u8 = 6;
const MARSH: u8 = 7;
const SILT: u8 = 8;
const FALLOUT: u8 = 9;
const SEABED: u8 = 10;
const REEF: u8 = 11;
const MATERIALS: usize = 12;

fn material(look: Look) -> u8 {
    match look.feature {
        FeatureKind::Forest if look.base != SNOW => return FOREST_FLOOR,
        FeatureKind::Jungle => return JUNGLE_FLOOR,
        FeatureKind::Marsh => return MARSH,
        FeatureKind::FloodPlains => return SILT,
        FeatureKind::Fallout => return FALLOUT,
        FeatureKind::Atoll => return REEF,
        _ => {}
    }
    match look.base {
        SEA => SEABED,
        PLAINS => STEPPE,
        DESERT => SAND,
        TUNDRA => LICHEN,
        SNOW => FIRN,
        _ => GRASSLAND,
    }
}

fn blend(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|k| a[k] * (1. - t) + b[k] * t)
}

/// One material at a point, with its own broad mottling. Palette values are
/// display-space albedo, read at strategic zoom: each biome keeps one clear hue.
fn paint(material: u8, p: Point) -> [f32; 3] {
    let n = |f: f32, seed: u32| noise(p[0] * f, p[1] * f, seed);
    match material {
        GRASSLAND => grass(p),
        STEPPE => {
            // Khaki plains: drier than grassland, with greener hollows and darker scrub.
            let green = smooth((n(0.55, 2203) - 0.35) / 0.35) * 0.6;
            let scrub = smooth((n(1.7, 2207) - 0.62) / 0.2) * 0.30;
            blend(blend([0.66, 0.66, 0.35], [0.47, 0.63, 0.29], green), [0.48, 0.50, 0.29], scrub)
        }
        SAND => {
            // Dune bands across the wind and pale salt flats.
            let dune = ((p[0] * 0.9 + p[1] * 0.35 + n(0.4, 2213) * 2.2).sin() * 0.5 + 0.5) * 0.10;
            let flat = smooth((n(0.6, 2217) - 0.6) / 0.25) * 0.4;
            blend([0.90 - dune, 0.77 - dune * 0.9, 0.52 - dune * 0.6], [0.93, 0.86, 0.68], flat)
        }
        LICHEN => {
            // Olive moss, rust lichen and grey stones.
            let rust = smooth((n(0.9, 2221) - 0.55) / 0.25) * 0.55;
            let stone = smooth((n(2.1, 2223) - 0.68) / 0.15) * 0.5;
            blend(blend([0.52, 0.57, 0.44], [0.62, 0.53, 0.41], rust), [0.63, 0.63, 0.61], stone)
        }
        FIRN => {
            let shade = n(0.7, 2227) * 0.05;
            [0.90 - shade, 0.93 - shade * 0.6, 0.97]
        }
        FOREST_FLOOR => {
            let litter = smooth((n(1.3, 2231) - 0.5) / 0.3) * 0.3;
            blend([0.30, 0.47, 0.24], [0.42, 0.44, 0.24], litter)
        }
        JUNGLE_FLOOR => blend([0.16, 0.44, 0.18], [0.22, 0.52, 0.16], n(1.1, 2237) * 0.25),
        MARSH => {
            // Reeds, mud and standing water in broad puddles.
            let puddle = smooth((n(1.25, 2239) - 0.58) / 0.08);
            let reeds = smooth((n(2.6, 2243) - 0.45) / 0.3);
            blend(blend([0.36, 0.40, 0.25], [0.47, 0.54, 0.27], reeds), [0.26, 0.40, 0.42], puddle * 0.9)
        }
        SILT => {
            // Fertile silt in long strips along the river.
            let strip = ((p[0] * 1.6 + p[1] * 0.7 + n(0.5, 2251) * 3.0).sin() * 0.5 + 0.5) * 0.5;
            blend([0.62, 0.62, 0.36], [0.48, 0.58, 0.29], strip)
        }
        FALLOUT => {
            let scorch = smooth((n(1.4, 2267) - 0.55) / 0.2);
            let glow = smooth((n(3.1, 2269) - 0.75) / 0.1) * 0.6;
            blend(blend([0.55, 0.57, 0.33], [0.27, 0.27, 0.21], scorch * 0.8), [0.78, 0.76, 0.28], glow)
        }
        REEF => [0.88, 0.84, 0.64],
        _ => [0.66, 0.64, 0.47],
    }
}

/// Warm and cool grass patches and dry ochre: the meadow of grassland.
fn grass(p: Point) -> [f32; 3] {
    let (x, z) = (p[0], p[1]);
    let warp = (noise(x * 0.16, z * 0.16, 17) - 0.5) * 1.9;
    let moisture = noise(x * 0.38 + warp, z * 0.38 - warp * 0.6, 331);
    let meadow = noise(x * 0.82 + warp * 0.3, z * 0.82, 587);
    let dry = smooth((noise(x * 0.61 - warp, z * 0.61, 971) - 0.56) / 0.26);
    let lush = smooth((moisture - 0.24) / 0.53);
    let cool = [0.32, 0.57, 0.255];
    let warm = [0.53, 0.70, 0.28];
    let ochre = [0.66, 0.65, 0.34];
    std::array::from_fn(|k| {
        let green = warm[k] * (1. - lush) + cool[k] * lush;
        let patch = green * (1. - dry * 0.5) + ochre[k] * dry * 0.5;
        patch * (0.94 + meadow * 0.12)
    })
}

/// An oasis: a turquoise pool in the middle of its cell, a ring of palms'
/// green around it, then the desert.
fn oasis_ring(color: [f32; 3], p: Point, center: Point) -> [f32; 3] {
    let d = (p[0] - center[0]).hypot(p[1] - center[1]) * (0.9 + 0.2 * noise(p[0] * 2., p[1] * 2., 2281));
    let pool = 1. - smooth((d - 0.30) / 0.08);
    let green = (1. - smooth((d - 0.62) / 0.25)) * (1. - pool);
    blend(blend(color, [0.30, 0.56, 0.22], green), [0.18, 0.60, 0.62], pool)
}

/// A readable hill shoulder inside a cell, with a smooth foot across its edge.
/// Orientation is stable; neighbouring hills retain separate crowns and saddles.
fn hill_relief(tile: u32, dx: f32, dz: f32) -> f32 {
    let seed = crate::scene::terrain::fields::hash_u32(tile ^ 0x4317);
    let angle = (seed & 255) as f32 / 255. * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    let u = dx * cos + dz * sin;
    let v = -dx * sin + dz * cos;
    // A short oblique ridge with a gentler back and a steeper exposed face.
    // Broad planar shoulders replace the rotationally symmetric dome.
    let across = v - 0.16 * u + 0.04;
    let flank = if across > 0. { 0.76 } else { 1.02 };
    let end = if u > 0. { 1.22 } else { 1.05 };
    let along = 1. - u.abs() / end;
    let shoulder = along.min(1. - across.abs() / flank).max(0.);
    let foot = smooth(shoulder / 0.18);
    0.46 * shoulder.min(0.90) * foot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::style::{compile, default_style};
    use crate::world::state::TerrainCell;
    use kernel::ids::TileId;

    /// A strip of plains, grassland, desert, tundra and snow, a hill in the
    /// desert, a marsh in the grassland.
    fn world() -> RenderWorld {
        let (width, height) = (25u32, 6u32);
        let count = (width * height) as usize;
        let band = |q: u32| [PLAINS, GRASS, DESERT, TUNDRA, SNOW][(q / 5) as usize];
        let mut w = RenderWorld {
            width,
            height,
            visible: vec![1; count],
            explored: vec![1; count],
            terrain: (0..width * height)
                .map(|i| TerrainCell { tile: TileId(i), terrain: band(i % width), owner: None, resource: None })
                .collect(),
            feature: vec![FeatureKind::None; count],
            forest: vec![0; count],
            ..Default::default()
        };
        w.terrain[(3 * width + 12) as usize].terrain = HILLS;
        w.feature[(3 * width + 7) as usize] = FeatureKind::Marsh;
        w
    }

    #[test]
    fn small_enclosed_known_water_is_a_lake() {
        let mut w = world();
        let lake = (2 * w.width + 8) as usize;
        let bay = (5 * w.width + 2) as usize;
        w.terrain[lake].terrain = SEA;
        w.terrain[bay].terrain = SEA;
        let lakes = Biomes::new(&w).lakes();
        assert!(lakes[lake], "a pond inside known land");
        assert!(!lakes[bay], "water at the map edge is not a lake");
        w.visible[lake + 1] = 0;
        w.explored[lake + 1] = 0;
        assert!(!Biomes::new(&w).lakes()[lake], "a body not seen whole stays sea");
    }

    #[test]
    fn hills_wear_the_climate_around_them() {
        let w = world();
        let biomes = Biomes::new(&w);
        assert_eq!(biomes.look((3 * w.width + 12) as usize).base, DESERT);
    }

    #[test]
    fn biomes_keep_their_hue_and_change_without_a_hex_step() {
        let w = world();
        let biomes = Biomes::new(&w);
        let style = compile(default_style()).unwrap();
        let at = |q: u32| {
            let (x, z) = tile_to_world(q, 2);
            biomes.ground(&style, x, z).color
        };
        let (plains, grass, desert, tundra, snow) = (at(2), at(6), at(13), at(17), at(22));
        assert!(desert[0] > grass[0] + 0.2 && desert[2] > grass[2] + 0.1, "sand is warm and pale: {desert:?}");
        assert!(plains[0] > grass[0] + 0.05, "plains are golden next to grassland: {plains:?} {grass:?}");
        assert!(snow.iter().all(|&c| c > 0.85), "snow is white: {snow:?}");
        assert!(tundra[1] < snow[1] - 0.2 && tundra[0] < desert[0], "tundra is dull olive: {tundra:?}");
        // Along a row, colors change continuously (no step at the hex edges).
        let (x0, z) = tile_to_world(0, 2);
        let mut previous = biomes.ground(&style, x0, z).color;
        for i in 1..4000 {
            let c = biomes.ground(&style, x0 + i as f32 * 0.01, z).color;
            for k in 0..3 {
                assert!((c[k] - previous[k]).abs() < 0.04, "color step at x={}", x0 + i as f32 * 0.01);
            }
            previous = c;
        }
    }

    #[test]
    fn marsh_lies_low_and_shows_water() {
        let w = world();
        let biomes = Biomes::new(&w);
        let style = compile(default_style()).unwrap();
        let (x, z) = tile_to_world(7, 3);
        let mut dry = w.clone();
        dry.feature.fill(FeatureKind::None);
        let meadow = Biomes::new(&dry).ground(&style, x, z).height;
        assert!(biomes.ground(&style, x, z).height < meadow - 0.04, "the marsh is lower than the meadow it replaced");
        let blue = (0..200).any(|i| {
            let c = biomes.ground(&style, x - 0.5 + (i % 20) as f32 * 0.05, z - 0.5 + (i / 20) as f32 * 0.1).color;
            c[2] > c[0] + 0.08
        });
        assert!(blue, "puddles of standing water");
    }
}
