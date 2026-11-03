//! Windows of real Lands & Laws maps (seed 20260928, the slice map of 63 x 32 tiles; from
//! `AURELIA_GAME_DUMP_REGION`): the terrain codes of the render projection around a river mouth,
//! at their own map coordinates, so tile ids and every hash of the field match the game's.
//! `.` is a tile the observer does not know.
use crate::world::state::{RenderWorld, TerrainCell};
use kernel::ids::TileId;

pub const LAL_WIDTH: u32 = 63;
pub const LAL_HEIGHT: u32 = 32;

/// A window of rows (`r0` is the first row; each row starts at column `q0`) and the river
/// edges between its tiles, as a visible world.
pub fn window(q0: u32, r0: u32, rows: &[&str], edges: &[((u32, u32), (u32, u32))]) -> RenderWorld {
    let count = (LAL_WIDTH * LAL_HEIGHT) as usize;
    let mut w = RenderWorld {
        visual_surface: true,
        width: LAL_WIDTH,
        height: LAL_HEIGHT,
        visible: vec![0; count],
        explored: vec![0; count],
        terrain: (0..count as u32).map(|i| TerrainCell { tile: TileId(i), terrain: 1, owner: None, resource: None }).collect(),
        ..Default::default()
    };
    for (dr, row) in rows.iter().enumerate() {
        for (dq, code) in row.split_whitespace().enumerate() {
            let Ok(code) = code.parse::<u16>() else { continue };
            let i = ((r0 as usize + dr) * LAL_WIDTH as usize) + q0 as usize + dq;
            w.terrain[i].terrain = code;
            w.visible[i] = 1;
            w.explored[i] = 1;
        }
    }
    let id = |(q, r): (u32, u32)| r * LAL_WIDTH + q;
    for &(a, b) in edges {
        let (a, b) = (id(a), id(b));
        w.river_edges.push((a.min(b), a.max(b)));
    }
    w.river_edges.sort();
    w.river_edges.dedup();
    w
}

/// A river reaching a sea along a plain beach (mouth any:4 of the seed).
pub fn beach_mouth() -> RenderWorld {
    window(45, 10, &[
        ". . . . . . . . 1 1 1 1 1 4 4 4 4",
        ". . . . . . . 1 1 1 1 1 0 4 4 4 4",
        ". . . . . . 1 1 1 1 1 0 4 4 4 4 4",
        ". . . . . 1 1 1 0 0 0 0 4 4 4 4 4",
        ". . . . 1 1 0 0 0 2 0 4 4 4 4 4 4",
        ". . . 1 1 1 0 0 0 0 0 4 4 4 4 4 4",
        ". . 1 1 1 1 1 0 0 0 4 4 4 4 4 4 4",
        ". 1 1 2 1 1 1 0 0 0 4 4 4 4 4 4 4",
        "1 1 2 1 1 2 1 1 1 4 4 4 4 4 4 4 4",
        "0 0 0 1 1 1 1 1 1 4 4 4 4 4 4 4 .",
        "0 1 1 1 1 1 1 1 4 4 4 4 4 4 4 . .",
        "0 1 1 1 1 1 1 1 4 4 4 4 4 4 . . .",
        "0 1 1 1 1 0 0 4 4 4 4 4 4 . . . .",
        "0 1 1 1 1 0 0 4 4 4 4 4 . . . . .",
        "0 0 1 0 0 0 4 4 4 4 4 . . . . . .",
        "0 0 2 0 0 0 4 4 4 4 . . . . . . .",
        "0 0 0 0 0 4 4 4 4 . . . . . . . .",
    ], &[((49, 17), (50, 17)), ((50, 17), (49, 18)), ((49, 18), (50, 18)), ((49, 18), (49, 19)), ((50, 18), (49, 19)), ((50, 18), (50, 19)), ((51, 18), (50, 19)), ((51, 18), (51, 19)), ((52, 18), (51, 19)), ((52, 18), (52, 19)), ((53, 18), (52, 19)), ((53, 18), (53, 19)), ((48, 19), (49, 19)), ((48, 19), (48, 20)), ((46, 20), (46, 21)), ((47, 20), (48, 20)), ((47, 20), (46, 21)), ((47, 20), (47, 21)), ((48, 20), (47, 21)), ((45, 21), (46, 21)), ((45, 21), (45, 22)), ((46, 21), (47, 21)), ((47, 21), (48, 21)), ((47, 21), (46, 22)), ((48, 21), (47, 22)), ((46, 22), (47, 22)), ((47, 22), (48, 22)), ((47, 22), (46, 23)), ((48, 22), (47, 23)), ((46, 23), (47, 23)), ((47, 23), (48, 23)), ((48, 23), (47, 24)), ((48, 23), (48, 24)), ((49, 23), (48, 24)), ((48, 24), (49, 24)), ((49, 24), (48, 25)), ((48, 25), (49, 25)), ((48, 25), (48, 26)), ((47, 26), (48, 26))])
}

/// A river reaching a bay beside an outcrop cliff coast (mouth any:2).
pub fn cliff_mouth() -> RenderWorld {
    window(2, 3, &[
        ". . . . . . . . 4 4 4 4 1 1 1 1 1",
        ". . . . . . . 4 4 4 4 1 2 1 1 1 1",
        ". . . . . . 4 4 4 4 4 1 1 1 1 1 1",
        ". . . . . 4 4 4 4 4 1 1 1 1 1 1 1",
        ". . . . 4 4 4 4 4 4 1 1 1 1 1 1 1",
        ". . . 4 4 4 4 4 4 0 1 1 1 1 1 1 1",
        ". . 4 4 4 4 4 4 4 1 1 2 1 1 1 1 0",
        ". 4 4 4 4 4 4 4 1 1 1 1 1 1 1 1 0",
        "4 4 4 4 4 4 4 4 1 1 1 1 1 1 1 1 0",
        "4 4 4 4 4 4 4 1 1 1 1 1 1 1 1 0 .",
        "4 4 4 4 4 4 4 1 1 1 1 1 1 1 1 . .",
        "4 4 4 4 4 4 1 1 1 2 1 1 1 1 . . .",
        "4 4 4 4 4 4 1 1 1 1 1 1 1 . . . .",
        "4 4 4 4 4 1 1 1 1 1 1 1 . . . . .",
        "4 4 4 4 4 1 1 1 1 1 1 . . . . . .",
        "4 4 4 4 1 1 1 1 0 0 . . . . . . .",
        "4 4 4 4 0 0 0 0 0 . . . . . . . .",
    ], &[((16, 3), (17, 3)), ((17, 3), (16, 4)), ((16, 4), (17, 4)), ((17, 4), (16, 5)), ((16, 5), (17, 5)), ((17, 5), (16, 6)), ((17, 5), (17, 6)), ((18, 5), (17, 6)), ((17, 6), (18, 6)), ((18, 6), (17, 7)), ((17, 7), (18, 7)), ((18, 7), (17, 8)), ((17, 8), (18, 8)), ((17, 8), (17, 9)), ((16, 9), (17, 9)), ((17, 9), (16, 10)), ((10, 11), (9, 12)), ((10, 11), (10, 12)), ((11, 11), (10, 12)), ((10, 12), (11, 12)), ((11, 12), (10, 13)), ((7, 16), (7, 17)), ((8, 16), (7, 17)), ((7, 17), (8, 17)), ((8, 17), (7, 18))])
}

/// A short stream, source to the sea, beside cliffs (mouth any:3).
pub fn stream_mouth() -> RenderWorld {
    window(0, 8, &[
        ". . . . . . . 4 4 4 4 0 1 1 1 1",
        ". . . . . . 4 4 4 4 4 1 1 2 1 1",
        ". . . . . 4 4 4 4 4 1 1 1 1 1 1",
        ". . . . 4 4 4 4 4 4 1 1 1 1 1 1",
        ". . . 4 4 4 4 4 4 1 1 1 1 1 1 1",
        ". . 4 4 4 4 4 4 4 1 1 1 1 1 1 1",
        ". 4 4 4 4 4 4 4 1 1 1 2 1 1 1 1",
        "4 4 4 4 4 4 4 4 1 1 1 1 1 1 1 1",
        "4 4 4 4 4 4 4 1 1 1 1 1 1 1 1 1",
        "4 4 4 4 4 4 4 1 1 1 1 1 1 1 1 .",
        "4 4 4 4 4 4 1 1 1 1 0 0 1 0 . .",
        "4 4 4 4 4 4 0 0 0 0 0 0 0 . . .",
        "4 4 4 4 4 0 0 0 0 0 0 1 . . . .",
        "4 4 4 4 4 0 0 0 2 0 1 . . . . .",
        "4 4 4 4 0 0 0 1 1 1 . . . . . .",
        "4 4 4 4 0 0 1 1 1 . . . . . . .",
        "4 4 4 1 1 1 1 1 . . . . . . . .",
    ], &[((10, 11), (9, 12)), ((10, 11), (10, 12)), ((11, 11), (10, 12)), ((10, 12), (11, 12)), ((11, 12), (10, 13)), ((7, 16), (7, 17)), ((8, 16), (7, 17)), ((7, 17), (8, 17)), ((8, 17), (7, 18))])
}

/// The river of `beach_mouth` ending in a pond (a lake: a small body inside known land)
/// instead of the sea: the sea tiles turn to grass but the three-by-three pond at the mouth.
pub fn lake_mouth() -> RenderWorld {
    let mut w = beach_mouth();
    for r in 10..=26u32 {
        for q in 45..=61u32 {
            let i = (r * LAL_WIDTH + q) as usize;
            let pond = (54..=56).contains(&q) && (17..=19).contains(&r);
            if w.terrain[i].terrain == 4 && !pond {
                w.terrain[i].terrain = 1;
            }
        }
    }
    w
}

/// `beach_mouth` with the highland west of the river's last reach: hills and a mountain, so
/// the river falls steeply (a stream on a slope) before it reaches the plain.
pub fn slope_river() -> RenderWorld {
    let mut w = beach_mouth();
    for (q, r, code) in [(49, 15, 2), (50, 15, 2), (49, 16, 2), (50, 16, 2), (51, 16, 2), (48, 16, 6), (49, 17, 2), (48, 17, 6), (47, 17, 2), (47, 18, 6), (46, 18, 2), (48, 19, 2), (48, 20, 2), (47, 19, 6), (47, 20, 2)] {
        w.terrain[(r * LAL_WIDTH + q) as usize].terrain = code;
    }
    w
}

/// The plain river of `beach_mouth`: a long reach over level ground.
pub fn plain_river() -> RenderWorld {
    beach_mouth()
}

/// The rivers of `beach_mouth` that join (around tile (47, 21)).
pub fn confluence() -> RenderWorld {
    beach_mouth()
}

/// The source of the west branch of `beach_mouth`.
pub fn source_window() -> RenderWorld {
    beach_mouth()
}
