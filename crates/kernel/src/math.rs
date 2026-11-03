//! Hex topology of the map: the ONE neighbour geometry for walking, combat,
//! zones of control, sight, territory and the renderer.
//!
//! Tiles are axial coordinates `(q, r)` stored row-major in a parallelogram
//! `q in 0..width, r in 0..height` (TileId = r * width + q). Every tile has up
//! to six neighbours, `HEX_DIRS`; the renderer places tiles with the same
//! axial layout (`render::scene::layout`), so "adjacent on screen" and
//! "adjacent in the rules" are the same relation.

use crate::ids::TileId;

/// Axial hex coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HexCoord {
    pub q: i32,
    pub r: i32,
}

/// The six neighbour directions (axial), in a fixed order: every iteration
/// over neighbours is deterministic.
pub const HEX_DIRS: [HexCoord; 6] = [
    HexCoord { q: 1, r: 0 },
    HexCoord { q: 1, r: -1 },
    HexCoord { q: 0, r: -1 },
    HexCoord { q: -1, r: 0 },
    HexCoord { q: -1, r: 1 },
    HexCoord { q: 0, r: 1 },
];

/// Map size; converts `HexCoord <-> TileId` without a HashMap on the hot path.
#[derive(Debug, Clone, Copy)]
pub struct MapDimensions {
    pub width: u32,
    pub height: u32,
}

/// Neighbours of one tile that lie on the map (at most six, no allocation).
#[derive(Debug, Clone, Copy)]
pub struct Neighbors {
    tiles: [TileId; 6],
    len: u8,
}

impl Neighbors {
    #[inline]
    pub fn as_slice(&self) -> &[TileId] {
        &self.tiles[..self.len as usize]
    }

    #[inline]
    pub fn contains(&self, tile: TileId) -> bool {
        self.as_slice().contains(&tile)
    }

    /// Keep only the tiles `keep` accepts (order preserved, no allocation).
    pub fn retain(&mut self, mut keep: impl FnMut(TileId) -> bool) {
        let mut len = 0;
        for i in 0..self.len as usize {
            if keep(self.tiles[i]) {
                self.tiles[len] = self.tiles[i];
                len += 1;
            }
        }
        self.len = len as u8;
    }
}

impl IntoIterator for Neighbors {
    type Item = TileId;
    type IntoIter = std::iter::Take<std::array::IntoIter<TileId, 6>>;

    fn into_iter(self) -> Self::IntoIter {
        self.tiles.into_iter().take(self.len as usize)
    }
}

impl MapDimensions {
    #[inline]
    pub fn tile_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    #[inline]
    pub fn to_tile(&self, c: HexCoord) -> Option<TileId> {
        if c.q < 0 || c.r < 0 || c.q >= self.width as i32 || c.r >= self.height as i32 {
            return None;
        }
        Some(TileId((c.r as u32) * self.width + (c.q as u32)))
    }

    pub fn to_coord(&self, id: TileId) -> Option<HexCoord> {
        let n = id.0;
        if (n as usize) >= self.tile_count() {
            return None;
        }
        Some(HexCoord {
            q: (n % self.width) as i32,
            r: (n / self.width) as i32,
        })
    }

    /// The on-map neighbours of `tile`, in `HEX_DIRS` order.
    pub fn neighbors(&self, tile: TileId) -> Neighbors {
        let mut out = Neighbors {
            tiles: [tile; 6],
            len: 0,
        };
        let Some(c) = self.to_coord(tile) else {
            return out;
        };
        for d in HEX_DIRS {
            let next = HexCoord {
                q: c.q + d.q,
                r: c.r + d.r,
            };
            if let Some(n) = self.to_tile(next) {
                out.tiles[out.len as usize] = n;
                out.len += 1;
            }
        }
        out
    }

    /// Hex distance between two tiles; None when one is off the map.
    #[inline]
    pub fn distance(&self, a: TileId, b: TileId) -> Option<i32> {
        Some(self.to_coord(a)?.distance(self.to_coord(b)?))
    }

    /// Two different tiles sharing an edge.
    #[inline]
    pub fn adjacent(&self, a: TileId, b: TileId) -> bool {
        self.distance(a, b) == Some(1)
    }
}

impl HexCoord {
    /// Distance in hexes (cube metric).
    #[inline]
    pub fn distance(self, other: HexCoord) -> i32 {
        let dq = self.q - other.q;
        let dr = self.r - other.r;
        (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
    }
}

/// All map tiles within hex distance `radius` of `center` (center included),
/// in a fixed dq/dr order. Radius 1 = the center and its six neighbours.
#[inline]
pub fn tiles_in_hex_radius(dims: &MapDimensions, center: TileId, radius: i32) -> Vec<TileId> {
    hex_disc(*dims, center, radius).collect()
}

/// The same tiles as `tiles_in_hex_radius`, in the same order, without an
/// allocation (hot paths: sight coverage).
pub fn hex_disc(dims: MapDimensions, center: TileId, radius: i32) -> impl Iterator<Item = TileId> {
    let c = dims.to_coord(center);
    let radius = if c.is_some() { radius } else { -1 };
    let c = c.unwrap_or(HexCoord { q: 0, r: 0 });
    // For each dq the dr that keep |dq|, |dr| and |dq + dr| within the radius
    // form one interval: no candidate is tested only to be thrown away (the
    // square 2R+1 by 2R+1 holds a third more cells than the disc).
    (-radius..=radius).flat_map(move |dq| {
        ((-radius).max(-dq - radius)..=radius.min(radius - dq)).filter_map(move |dr| {
            dims.to_tile(HexCoord {
                q: c.q + dq,
                r: c.r + dr,
            })
        })
    })
}

/// Fixed-point scale of the hex line (the nudge below must stay far smaller
/// than one step of 1/N between line samples).
const LINE_SCALE: i64 = 1024;
/// Nudge of the cube point (sum zero), breaks ties at exact half-way
/// points the same way on every platform.
const LINE_NUDGE: [i64; 3] = [1, 1, -2];

/// Tiles strictly between `from` and `to` on the hex line (cube lerp with
/// integer rounding), in order from `from`. Off-map samples are skipped.
#[inline]
#[cfg(test)]
pub(crate) fn hex_line_between(dims: &MapDimensions, from: TileId, to: TileId) -> Vec<TileId> {
    let mut out = Vec::new();
    hex_line_all(dims, from, to, |t| {
        out.push(t);
        true
    });
    out
}

/// Does `keep` hold for every tile strictly between `from` and `to` on the
/// hex line? Stops at the first tile that fails; no allocation (sight).
pub fn hex_line_all(
    dims: &MapDimensions,
    from: TileId,
    to: TileId,
    mut keep: impl FnMut(TileId) -> bool,
) -> bool {
    let (Some(a), Some(b)) = (dims.to_coord(from), dims.to_coord(to)) else {
        return true;
    };
    let n = a.distance(b) as i64;
    if n < 2 {
        return true;
    }
    if n == 2 {
        // Sight radius 2 is the hot case: one tile between, from a table.
        let (dq, dr) = middle_of_two_step(b.q - a.q, b.r - a.r);
        let middle = dims.to_tile(HexCoord {
            q: a.q + dq,
            r: a.r + dr,
        });
        return middle.map(&mut keep).unwrap_or(true);
    }
    let a_cube = [a.q as i64, a.r as i64, -(a.q as i64) - a.r as i64];
    let b_cube = [b.q as i64, b.r as i64, -(b.q as i64) - b.r as i64];
    let denominator = n * LINE_SCALE;
    for i in 1..n {
        let mut numerators = [0i64; 3];
        for axis in 0..3 {
            let lerp = a_cube[axis] * (n - i) + b_cube[axis] * i;
            numerators[axis] = lerp * LINE_SCALE + LINE_NUDGE[axis];
        }
        let cube = round_cube(numerators, denominator);
        let sample = dims.to_tile(HexCoord {
            q: cube[0] as i32,
            r: cube[1] as i32,
        });
        if let Some(t) = sample {
            if !keep(t) {
                return false;
            }
        }
    }
    true
}

/// The offset of the tile between a tile and one two steps away by
/// `(dq, dr)`: the hex line's single middle sample, computed once by the
/// general rule (same nudge, same rounding) and kept in a 5×5 table.
fn middle_of_two_step(dq: i32, dr: i32) -> (i32, i32) {
    static TABLE: std::sync::OnceLock<[[(i8, i8); 5]; 5]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [[(0i8, 0i8); 5]; 5];
        for q in -2i64..=2 {
            for r in -2i64..=2 {
                let b = [q, r, -q - r];
                let mut numerators = [0i64; 3];
                for axis in 0..3 {
                    numerators[axis] = b[axis] * LINE_SCALE + LINE_NUDGE[axis];
                }
                let cube = round_cube(numerators, 2 * LINE_SCALE);
                table[(q + 2) as usize][(r + 2) as usize] = (cube[0] as i8, cube[1] as i8);
            }
        }
        table
    });
    let (mq, mr) = table[(dq + 2) as usize][(dr + 2) as usize];
    (mq as i32, mr as i32)
}

/// Nearest hex of a fractional cube point `numerators / denominator`.
fn round_cube(numerators: [i64; 3], denominator: i64) -> [i64; 3] {
    let mut rounded = [0i64; 3];
    let mut error = [0i64; 3];
    for axis in 0..3 {
        rounded[axis] = (2 * numerators[axis] + denominator).div_euclid(2 * denominator);
        error[axis] = (rounded[axis] * denominator - numerators[axis]).abs();
    }
    if error[0] > error[1] && error[0] > error[2] {
        rounded[0] = -rounded[1] - rounded[2];
    } else if error[1] > error[2] {
        rounded[1] = -rounded[0] - rounded[2];
    } else {
        rounded[2] = -rounded[0] - rounded[1];
    }
    rounded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims() -> MapDimensions {
        MapDimensions {
            width: 10,
            height: 10,
        }
    }

    fn tile(q: i32, r: i32) -> TileId {
        dims().to_tile(HexCoord { q, r }).unwrap()
    }

    #[test]
    fn six_neighbours_inside_and_fewer_on_the_edge() {
        let inner = dims().neighbors(tile(5, 5));
        assert_eq!(inner.as_slice().len(), 6);
        for n in inner {
            assert_eq!(dims().distance(tile(5, 5), n), Some(1));
        }
        // The (1,1) and (-1,-1) offsets are two steps away on a hex map.
        assert!(!inner.contains(tile(6, 6)));
        assert!(!inner.contains(tile(4, 4)));
        assert_eq!(dims().neighbors(tile(0, 0)).as_slice().len(), 2);
    }

    #[test]
    fn neighbours_match_the_hex_radius_ring() {
        for q in 0..10 {
            for r in 0..10 {
                let mut ring = tiles_in_hex_radius(&dims(), tile(q, r), 1);
                ring.retain(|t| *t != tile(q, r));
                ring.sort();
                let mut near: Vec<TileId> = dims().neighbors(tile(q, r)).into_iter().collect();
                near.sort();
                assert_eq!(ring, near);
            }
        }
    }

    #[test]
    fn two_step_middles_touch_both_ends() {
        let from = tile(5, 5);
        for q in 3..=7 {
            for r in 3..=7 {
                let to = tile(q, r);
                if dims().distance(from, to) != Some(2) {
                    continue;
                }
                let between = hex_line_between(&dims(), from, to);
                assert_eq!(between.len(), 1);
                assert!(dims().adjacent(from, between[0]) && dims().adjacent(between[0], to));
                // The table agrees with the general rule (same lerp, nudge
                // and rounding as for longer lines).
                let (dq, dr) = (q - 5, r - 5);
                let numerators = [
                    i64::from(dq) * LINE_SCALE + LINE_NUDGE[0],
                    i64::from(dr) * LINE_SCALE + LINE_NUDGE[1],
                    i64::from(-dq - dr) * LINE_SCALE + LINE_NUDGE[2],
                ];
                let cube = round_cube(numerators, 2 * LINE_SCALE);
                assert_eq!(between[0], tile(5 + cube[0] as i32, 5 + cube[1] as i32));
            }
        }
    }

    /// A map that is not square: `TileId = r * width + q` must use the width,
    /// never the height.
    fn wide() -> MapDimensions {
        MapDimensions {
            width: 7,
            height: 4,
        }
    }

    fn tall() -> MapDimensions {
        MapDimensions {
            width: 4,
            height: 9,
        }
    }

    #[test]
    fn coordinates_round_trip_and_reject_what_would_wrap() {
        for dims in [wide(), tall(), dims()] {
            for n in 0..dims.tile_count() as u32 {
                let c = dims.to_coord(TileId(n)).unwrap();
                assert_eq!(dims.to_tile(c), Some(TileId(n)));
            }
            assert_eq!(dims.to_coord(TileId(dims.tile_count() as u32)), None);
            // One column past the right edge would alias the next row's first
            // tile in a flat array; it must be off the map.
            let w = dims.width as i32;
            assert_eq!(dims.to_tile(HexCoord { q: w, r: 0 }), None);
            assert_eq!(dims.to_tile(HexCoord { q: -1, r: 1 }), None);
            assert_eq!(
                dims.to_tile(HexCoord {
                    q: 0,
                    r: dims.height as i32
                }),
                None
            );
            assert_eq!(dims.to_tile(HexCoord { q: 0, r: -1 }), None);
        }
    }

    #[test]
    fn neighbour_counts_follow_the_parallelogram_corners_and_edges() {
        for dims in [wide(), tall()] {
            let (w, h) = (dims.width as i32, dims.height as i32);
            for r in 0..h {
                for q in 0..w {
                    // The six axial neighbours that lie inside 0..w x 0..h.
                    let expected = HEX_DIRS
                        .iter()
                        .filter(|d| (0..w).contains(&(q + d.q)) && (0..h).contains(&(r + d.r)))
                        .count();
                    let id = dims.to_tile(HexCoord { q, r }).unwrap();
                    assert_eq!(
                        dims.neighbors(id).as_slice().len(),
                        expected,
                        "{w}x{h} ({q},{r})"
                    );
                }
            }
            // The acute corners (0,0) and (w-1,h-1) have two neighbours, the
            // obtuse ones (w-1,0) and (0,h-1) three, the rest of the border
            // four, the inside six.
            let count = |q, r| {
                dims.neighbors(dims.to_tile(HexCoord { q, r }).unwrap())
                    .as_slice()
                    .len()
            };
            assert_eq!(
                [
                    count(0, 0),
                    count(w - 1, h - 1),
                    count(w - 1, 0),
                    count(0, h - 1)
                ],
                [2, 2, 3, 3]
            );
            assert_eq!(
                [
                    count(1, 0),
                    count(w - 2, h - 1),
                    count(0, 1),
                    count(w - 1, h - 2),
                    count(1, 1)
                ],
                [4, 4, 4, 4, 6]
            );
        }
    }

    #[test]
    fn distance_is_the_walking_distance_on_the_map() {
        // The parallelogram is convex in the hex metric, so the cube distance
        // equals the length of the shortest walk over `neighbors` (breadth
        // first, an oracle that shares no code with `distance`).
        for dims in [wide(), tall(), dims()] {
            let n = dims.tile_count();
            for start in 0..n {
                let mut walked = vec![i32::MAX; n];
                walked[start] = 0;
                let mut queue = std::collections::VecDeque::from([TileId(start as u32)]);
                while let Some(t) = queue.pop_front() {
                    for next in dims.neighbors(t) {
                        if walked[next.0 as usize] == i32::MAX {
                            walked[next.0 as usize] = walked[t.0 as usize] + 1;
                            queue.push_back(next);
                        }
                    }
                }
                for other in 0..n {
                    let d = dims
                        .distance(TileId(start as u32), TileId(other as u32))
                        .unwrap();
                    assert_eq!(
                        d, walked[other],
                        "{}x{} {start}->{other}",
                        dims.width, dims.height
                    );
                    assert_eq!(
                        dims.adjacent(TileId(start as u32), TileId(other as u32)),
                        d == 1
                    );
                }
            }
        }
    }

    #[test]
    fn discs_are_every_tile_within_the_distance_at_corners_and_edges() {
        for dims in [wide(), tall()] {
            let n = dims.tile_count() as u32;
            for center in 0..n {
                for radius in 0..=6 {
                    let want: Vec<TileId> = (0..n)
                        .map(TileId)
                        .filter(|t| dims.distance(TileId(center), *t).unwrap() <= radius)
                        .collect();
                    let mut got: Vec<TileId> = hex_disc(dims, TileId(center), radius).collect();
                    // The same set, each tile once; the order is the iteration
                    // order (q outer, r inner), not the id order.
                    got.sort();
                    assert_eq!(
                        got, want,
                        "{}x{} centre {center} radius {radius}",
                        dims.width, dims.height
                    );
                    let mut all = tiles_in_hex_radius(&dims, TileId(center), radius);
                    all.sort();
                    assert_eq!(all, want);
                }
            }
            // Off the map: no centre, no tiles.
            assert_eq!(hex_disc(dims, TileId(n), 3).count(), 0);
            assert_eq!(hex_disc(dims, TileId(0), -1).count(), 0);
        }
    }

    /// The disc as it was first written: the whole square, filtered by the
    /// cube distance. The reference for the interval version.
    fn square_disc(dims: MapDimensions, center: TileId, radius: i32) -> Vec<TileId> {
        let Some(c) = dims.to_coord(center) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for dq in -radius..=radius {
            for dr in -radius..=radius {
                if (dq.abs() + dr.abs() + (dq + dr).abs()) / 2 > radius {
                    continue;
                }
                out.extend(dims.to_tile(HexCoord {
                    q: c.q + dq,
                    r: c.r + dr,
                }));
            }
        }
        out
    }

    #[test]
    fn disc_keeps_the_order_of_the_square_it_replaced() {
        // Journals, hashes and AI tie-breaks read discs in this order.
        for dims in [wide(), tall(), dims()] {
            for center in 0..=dims.tile_count() as u32 {
                for radius in -1..=7 {
                    let got: Vec<TileId> = hex_disc(dims, TileId(center), radius).collect();
                    let want = square_disc(dims, TileId(center), radius);
                    assert_eq!(
                        got, want,
                        "{}x{} centre {center} radius {radius}",
                        dims.width, dims.height
                    );
                }
            }
        }
    }

    #[test]
    fn hex_line_is_the_same_both_ways_and_stays_connected_on_every_pair() {
        // The nudge is the same for both directions, so line(a, b) reversed
        // is line(b, a); every sample is a map tile adjacent to the previous
        // one (the parallelogram keeps lines inside it).
        for dims in [wide(), tall()] {
            let n = dims.tile_count() as u32;
            for a in 0..n {
                for b in 0..n {
                    let (from, to) = (TileId(a), TileId(b));
                    let mut path = vec![from];
                    path.extend(hex_line_between(&dims, from, to));
                    if from != to {
                        path.push(to);
                    }
                    assert_eq!(
                        path.len() as i32,
                        dims.distance(from, to).unwrap() + 1,
                        "{a}->{b}"
                    );
                    for pair in path.windows(2) {
                        assert!(dims.adjacent(pair[0], pair[1]), "{a}->{b}: {path:?}");
                    }
                    let mut back = vec![to];
                    back.extend(hex_line_between(&dims, to, from));
                    if from != to {
                        back.push(from);
                    }
                    back.reverse();
                    assert_eq!(path, back, "{a}->{b} differs from the way back");
                }
            }
        }
    }

    #[test]
    fn hex_line_steps_through_adjacent_tiles() {
        for (a, b) in [((0, 0), (6, 0)), ((1, 7), (7, 1)), ((2, 2), (5, 7))] {
            let (from, to) = (tile(a.0, a.1), tile(b.0, b.1));
            let mut path = vec![from];
            path.extend(hex_line_between(&dims(), from, to));
            path.push(to);
            assert_eq!(path.len() as i32, dims().distance(from, to).unwrap() + 1);
            for pair in path.windows(2) {
                assert!(dims().adjacent(pair[0], pair[1]), "{a:?}->{b:?}");
            }
        }
    }
}
