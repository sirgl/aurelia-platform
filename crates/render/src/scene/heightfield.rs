//! Heightfield: canonical shared corners of the whole map.
//!
//! The corner key is quantised coordinates: x/(sqrt3/2) and z/0.5 are always integer
//! (hex corners: cos in {+-sqrt3/2, 0}, sin in {+-0.5, +-1}), float error
//! is removed by round(). One key is one vertex for the whole map: seams between
//! chunks are impossible by construction (centres have z-key == 0 mod 3,
//! corners 1 or 2 mod 3, the sets do not intersect).
//!
//! Corner height = the average of the referencing hexes' heights; colour = the average of colours
//! (soft biome transitions). Summation in row-major order is
//! deterministic. Rivers later: carve = subtracting depth from corners
//! along the path + a ribbon layer; the topology does not change.

use crate::scene::layout::EDGE_NEIGHBORS;
use crate::scene::style::CompiledTerrainStyle;
use crate::scene::terrain::{corner_lattice, key_from_world};
use crate::world::state::RenderWorld;
use std::collections::HashMap;

/// Canonical corner key (doubled lattice units; constructors are
/// only the exact `corner_lattice` and the lookup shim `corner_key`).
pub type CornerKey = (i32, i32);

pub fn corner_key(x: f32, z: f32) -> CornerKey {
    key_from_world(x, z)
}

#[derive(Debug, Clone, Default)]
pub struct Heightfield {
    corners: HashMap<CornerKey, CornerData>,
}

/// Corner data: the averaged height + the terrain distribution for splat.
/// counts hold up to 4 records (at most 3 hexes touch a corner + a reserve), sorted
/// (count desc, id asc) at finalisation, so deterministic.
#[derive(Debug, Clone)]
pub struct CornerData {
    pub height: f32,
    pub counts: [(u16, u8); 4],
    pub n: u32,
    /// Water votes from carve (0..=2): for the sandy strip on the shore.
    pub water: u8,
}

impl Heightfield {
    pub fn get(&self, key: CornerKey) -> Option<CornerData> {
        self.corners.get(&key).cloned()
    }

    pub fn corner_at(&self, x: f32, z: f32) -> Option<CornerData> {
        self.get(corner_key(x, z))
    }

    pub fn corner_count(&self) -> usize {
        self.corners.len()
    }
}

/// Build the heightfield of the whole map from terrain styles.
pub fn build_heightfield(world: &RenderWorld, style: &CompiledTerrainStyle) -> Heightfield {
    // Accumulators: sum of heights + a terrain histogram (traversal order
    // row-major, so deterministic; finalisation by sorting).
    struct Acc {
        height_sum: f64,
        n: u32,
        hist: HashMap<u16, u8>,
    }
    // Shore carve (water meshing, as in Nation): corners of river beds and lake rims
    // are lowered + get water votes in splat. The same corner math
    // as in the loop below (corner_key): the keys match bit for bit.
    // Beds go by edges (min,max id); lake rims are water/land edges.
    const RIVER_DROP: f32 = 0.30;
    const SHORE_DROP: f32 = 0.16;
    const WATER_VOTES: u8 = 2;
    let mut carve: HashMap<CornerKey, (f32, u8)> = HashMap::new();
    let mut mark_edge = |qa: i32, ra: i32, i: usize, drop: f32| {
        // Exact: edge corners from the integer lattice, without trigonometry.
        for k in [
            corner_lattice(qa, ra, i),
            corner_lattice(qa, ra, (i + 1) % 6),
        ] {
            let e = carve.entry(k).or_insert((0.0, 0));
            if drop > e.0 {
                e.0 = drop;
            }
            e.1 = e.1.max(WATER_VOTES);
        }
    };
    let w = world.width as i32;
    let h = world.height as i32;
    for (a, b) in &world.river_edges {
        let (aq, ar) = (
            (*a % world.width.max(1)) as i32,
            (*a / world.width.max(1)) as i32,
        );
        let (bq, br) = (
            (*b % world.width.max(1)) as i32,
            (*b / world.width.max(1)) as i32,
        );
        if let Some(i) = EDGE_NEIGHBORS
            .iter()
            .position(|(dq, dr)| aq + dq == bq && ar + dr == br)
        {
            mark_edge(aq, ar, i, RIVER_DROP);
        }
    }
    // Lake rims: water->land in all 6 directions.
    let is_water = |q: i32, r: i32| -> bool {
        if q < 0 || r < 0 || q >= w || r >= h {
            return false;
        }
        world.is_water((r as u32 * world.width + q as u32) as usize)
    };
    for cell in &world.terrain {
        let (q, r) = (
            (cell.tile.0 % world.width.max(1)) as i32,
            (cell.tile.0 / world.width.max(1)) as i32,
        );
        if !world.terrain_kinds.is_water(cell.terrain) {
            continue;
        }
        for (i, (dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
            if !is_water(q + dq, r + dr) {
                mark_edge(q, r, i, SHORE_DROP);
            }
        }
    }
    let mut acc: HashMap<CornerKey, Acc> = HashMap::new();
    for cell in &world.terrain {
        let q = cell.tile.0 % world.width.max(1);
        let r = cell.tile.0 / world.width.max(1);
        let h = style.height(cell.terrain).unwrap_or(0.3) as f64;
        for i in 0..6 {
            // Exact corner key (the same as in carve above: matching bit for bit
            // by construction, not through float rounding).
            let key = corner_lattice(q as i32, r as i32, i);
            let e = acc.entry(key).or_insert(Acc {
                height_sum: 0.0,
                n: 0,
                hist: HashMap::new(),
            });
            e.height_sum += h;
            e.n += 1;
            *e.hist.entry(cell.terrain).or_insert(0) += 1;
            // Water votes on shores (splat pulls towards water at a bed/rim).
            if let Some((_, votes)) = carve.get(&key) {
                if *votes > 0 {
                    let v = e.hist.entry(world.water_code()).or_insert(0);
                    *v = (*v).saturating_add(*votes);
                }
            }
        }
    }
    let corners = acc
        .into_iter()
        .map(|(k, e)| {
            let n = e.n.max(1);
            let mut counts: Vec<(u16, u8)> = e.hist.into_iter().collect();
            counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let mut arr = [(0u16, 0u8); 4];
            for (i, c) in counts.into_iter().take(4).enumerate() {
                arr[i] = c;
            }
            // Shore lowering (bed/rim): the water lies in a hollow.
            let (drop, water) = carve.get(&k).map(|(d, v)| (*d, *v)).unwrap_or((0.0, 0));
            let mut height = (e.height_sum / n as f64) as f32 - drop;
            // A bowl at the water: the underwater ground is not above the surface minus a gap
            // (the shared waterline constants from style; the water mesh reads the same ones).
            if water > 0 {
                use crate::scene::style::{WATER_GROUND_CLEARANCE, WATER_SURFACE_LIFT};
                let water_surface =
                    style.height(world.water_code()).unwrap_or(0.05) + WATER_SURFACE_LIFT;
                let bowl = water_surface - WATER_GROUND_CLEARANCE;
                if height > bowl {
                    height = bowl;
                }
            }
            (
                k,
                CornerData {
                    height,
                    counts: arr,
                    n,
                    water,
                },
            )
        })
        .collect();
    Heightfield { corners }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::layout::tile_to_world;
    use crate::scene::style::{compile, default_style};
    use crate::world::state::TerrainCell;
    use kernel::ids::TileId;

    fn test_world(w: u32, h: u32, terrain: u16) -> RenderWorld {
        let mut world = RenderWorld {
            width: w,
            height: h,
            ..Default::default()
        };
        for r in 0..h {
            for q in 0..w {
                world.terrain.push(TerrainCell {
                    tile: TileId(r * w + q),
                    terrain,
                    owner: None,
                    resource: None,
                });
            }
        }
        world
    }

    #[test]
    fn corner_keys_are_stable_and_shared() {
        // Corner +30 degrees of hex (2,2) == corner 150 degrees of hex (3,2): a shared grid corner.
        let (x0, z0) = tile_to_world(2, 2);
        let (x1, z1) = tile_to_world(3, 2);
        let a = corner_key(
            x0 + (30f32).to_radians().cos(),
            z0 + (30f32).to_radians().sin(),
        );
        let b = corner_key(
            x1 + (150f32).to_radians().cos(),
            z1 + (150f32).to_radians().sin(),
        );
        assert_eq!(a, b);
        // And is it also corner 270 degrees of hex (2,1)? We check distinguishing: another key nearby.
        let c = corner_key(
            x0 + (90f32).to_radians().cos(),
            z0 + (90f32).to_radians().sin(),
        );
        assert_ne!(a, c);
    }

    #[test]
    fn uniform_map_gives_uniform_corners() {
        let style = compile(default_style()).unwrap();
        let world = test_world(6, 6, 1);
        let hf = build_heightfield(&world, &style);
        let expect_h = style.height(1).unwrap();
        for data in hf.corners.values() {
            assert!((data.height - expect_h).abs() < 1e-5, "h={}", data.height);
            // One terrain per corner: counter [(1, k)], the rest zeros.
            assert_eq!(data.counts[0].0, 1);
            assert!(data.counts[0].1 > 0);
            assert!(data.counts[1..].iter().all(|c| c.1 == 0));
        }
    }

    #[test]
    fn border_corner_blends_biomes() {
        // A corner between plains and hills: the height strictly between, both terrains in counts.
        let style = compile(default_style()).unwrap();
        let mut world = test_world(4, 2, 0);
        for cell in world.terrain.iter_mut().filter(|c| c.tile.0 % 4 >= 2) {
            cell.terrain = 2;
        }
        let hf = build_heightfield(&world, &style);
        // Corner of hex (1,0) on the boundary: the east corners touch hills.
        let (wx, wz) = tile_to_world(1, 0);
        let mut blended = false;
        for i in 0..6 {
            let a = (60.0 * i as f32 - 30.0).to_radians();
            if let Some(data) = hf.corner_at(wx + a.cos(), wz + a.sin()) {
                let (hp, hh) = (style.height(0).unwrap(), style.height(2).unwrap());
                let terrains: Vec<u16> = data
                    .counts
                    .iter()
                    .filter(|c| c.1 > 0)
                    .map(|c| c.0)
                    .collect();
                if data.height > hp + 1e-6
                    && data.height < hh - 1e-6
                    && terrains.contains(&0)
                    && terrains.contains(&2)
                {
                    blended = true;
                }
            }
        }
        assert!(blended, "border corner must blend heights and terrains");
    }

    #[test]
    fn river_carve_lowers_banks_and_adds_water() {
        // A bed along edge (20,21) on 8x8: the edge ends are lower than neighbouring corners,
        // water in splat. Without beds, exactly flat (carve regression).
        let style = compile(default_style()).unwrap();
        let mut world = test_world(8, 8, 0);
        world.river_edges = vec![(20, 21)];
        let hf = build_heightfield(&world, &style);
        let flat = style.height(0).unwrap();
        // Corners of tile 20 = (q=4, r=2); edge (20,21) is direction 0.
        let (wx, _) = tile_to_world(4, 2);
        let (_, wz) = tile_to_world(4, 2);
        let mut hs: Vec<f32> = (0..6)
            .map(|i| {
                let a = (60.0 * i as f32 - 30.0).to_radians();
                hf.get(crate::scene::heightfield::corner_key(
                    wx + a.cos(),
                    wz + a.sin(),
                ))
                .map(|d| d.height)
                .unwrap_or(f32::NAN)
            })
            .collect();
        hs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(hs[0] < flat - 0.1, "carved corner below flat: {:?}", hs);
        assert!(
            (hs[5] - flat).abs() < 1e-4,
            "untouched corner stays flat: {:?}",
            hs
        );
        // Water in the splat of at least one corner.
        let watery = hf.corners.values().any(|d| {
            d.counts
                .iter()
                .any(|(t, c)| *t == world.water_code() && *c > 0)
        });
        assert!(watery, "river must inject water into splat");
        // Without edges there is no carve at all.
        let mut plain = test_world(8, 8, 0);
        plain.river_edges = vec![];
        let hf2 = build_heightfield(&plain, &style);
        assert!(hf2.corners.values().all(|d| (d.height - flat).abs() < 1e-4));
    }

    #[test]
    fn lake_shore_carves_banks() {
        // A lake (water in the centre): land corners at the rim are lower, water in splat.
        // The rim = water/land edges, visible to render_world.river? No:
        // rims are computed inside build_heightfield from terrains.
        let style = compile(default_style()).unwrap();
        let mut world = test_world(8, 8, 0);
        // A 2x2 pond in the centre (tiles 27,28,35,36 -> water id 4).
        for t in [27u32, 28, 35, 36] {
            world.terrain[t as usize].terrain = 4;
        }
        let hf = build_heightfield(&world, &style);
        let flat = style.height(0).unwrap();
        // A corner between water and land: below flat (shore).
        // Tile (3,3)=27 is water, neighbour (3,2) is land: their shared edge.
        let (wx, _) = tile_to_world(3, 3);
        let (_, wz) = tile_to_world(3, 3);
        let mut shore_low = false;
        for i in 0..6 {
            let a = (60.0 * i as f32 - 30.0).to_radians();
            if let Some(d) = hf.get(crate::scene::heightfield::corner_key(
                wx + a.cos(),
                wz + a.sin(),
            )) {
                if d.height < flat - 0.05 {
                    shore_low = true;
                }
            }
        }
        assert!(shore_low, "shore corners must dip");
    }
}
