//! Test worlds for the surface. `island` is a small island with a river, without cities,
//! units and props: everything the surface reads, so its tests see a coast, cliffs,
//! mountains and the river mouth through a rocky coast.
use crate::{
    scene::{layout::tile_to_world, terrain::fields::hash_u32},
    world::state::{RenderWorld, TerrainCell},
};
use kernel::ids::TileId;

pub fn island() -> RenderWorld {
    const WIDTH: u32 = 32;
    const HEIGHT: u32 = 28;
    let noise = |q: u32, r: u32| hash_u32(q.wrapping_mul(1777) ^ r.wrapping_mul(7981) ^ 7127);
    let count = (WIDTH * HEIGHT) as usize;
    let mut w = RenderWorld {
        visual_surface: true,
        width: WIDTH,
        height: HEIGHT,
        visible: vec![0; count],
        explored: vec![0; count],
        ..Default::default()
    };
    let (cx, cz) = tile_to_world(12, 12);
    for r in 0..HEIGHT {
        for q in 0..WIDTH {
            let (x, z) = tile_to_world(q, r);
            let (dx, dz) = (x - cx, z - cz);
            let n = noise(q, r);
            let coast = 7.3 + 1.3 * (dz * 0.46).sin() + 0.7 * (dz * 0.95).cos();
            let water = dx > coast || dx < -18.0 + 1.5 * (dz * 0.6).sin() || dz.abs() > 18.0;
            let peak = ((dx + 5.5) / 3.2).powi(2) + ((dz + 8.0) / 3.1).powi(2) < 1.0
                || ((dx + 11.0) / 3.0).powi(2) + ((dz - 0.5) / 4.6).powi(2) < 1.0
                || ((dx + 8.0) / 2.5).powi(2) + ((dz - 12.5) / 2.7).powi(2) < 1.0;
            let terrain = if water {
                4
            } else if peak {
                6
            } else if n % 13 == 0 {
                2
            } else if n % 3 == 0 {
                0
            } else {
                1
            };
            w.terrain.push(TerrainCell { tile: TileId(r * WIDTH + q), terrain, owner: None, resource: None });
            let i = (r * WIDTH + q) as usize;
            let explored = (dx / 15.5).powi(2) + (dz / 15.5).powi(2) < 1.0;
            let visible = (dx / 12.8).powi(2) + (dz / 12.3).powi(2) < 1.0;
            w.visible[i] = u8::from(visible);
            w.explored[i] = u8::from(explored || visible);
        }
    }
    let flat = [(15, 12), (14, 12), (15, 11), (14, 13), (13, 14), (14, 14), (12, 15), (11, 15), (12, 16), (13, 16), (10, 12), (11, 12), (10, 13), (11, 13), (14, 11), (13, 9), (12, 14)];
    for (q, r) in flat {
        w.terrain[(r * WIDTH + q) as usize].terrain = 1;
    }
    let river = [
        ((11, 6), (12, 6)), ((11, 6), (11, 7)), ((10, 7), (11, 7)), ((11, 7), (10, 8)), ((10, 8), (11, 8)),
        ((11, 8), (10, 9)), ((10, 9), (11, 9)), ((11, 9), (10, 10)), ((10, 10), (11, 10)), ((11, 10), (10, 11)),
        ((10, 11), (11, 11)), ((11, 11), (10, 12)), ((10, 12), (11, 12)), ((11, 12), (10, 13)), ((11, 12), (11, 13)),
        ((12, 12), (11, 13)), ((11, 13), (12, 13)), ((11, 13), (11, 14)), ((10, 14), (11, 14)), ((10, 14), (10, 15)),
        ((9, 15), (10, 15)), ((10, 15), (9, 16)), ((9, 16), (10, 16)), ((10, 16), (9, 17)), ((9, 17), (10, 17)),
        ((10, 17), (9, 18)), ((9, 18), (10, 18)), ((10, 18), (9, 19)), ((10, 18), (10, 19)), ((11, 18), (10, 19)),
        ((11, 18), (11, 19)), ((12, 18), (11, 19)), ((12, 18), (12, 19)),
    ];
    for (index, ((aq, ar), (bq, br))) in river.into_iter().enumerate() {
        let (a, b) = (ar * WIDTH + aq, br * WIDTH + bq);
        w.river_edges.push((a.min(b), a.max(b)));
        for t in [a, b] {
            if index > 1 && w.terrain[t as usize].terrain != 4 {
                w.terrain[t as usize].terrain = 1;
            }
        }
    }
    w.terrain[(9 * WIDTH + 18) as usize].terrain = 2;
    // The renderer sees only the observation: hidden cells are plain grass.
    for (i, cell) in w.terrain.iter_mut().enumerate() {
        if w.explored[i] == 0 {
            cell.terrain = 1;
        }
    }
    w.river_edges.sort();
    w.river_edges.dedup();
    w
}
