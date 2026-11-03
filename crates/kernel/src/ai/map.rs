//! The map as the AI knows it (R-07 AI §2, stage D): paths only over known
//! tiles, step prices from `PlayerView::known_step_cost_as` (the core's own
//! step rule applied to the player's knowledge and the mover's profile:
//! embarking, sailing). Unknown tiles are never
//! entered by a plan — they are the frontier. Every search has a
//! deterministic node budget, and the scratch is reset by generation stamps,
//! not by clearing the whole map (R-07 P3).

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::ids::{TileId, UnitHandle, UnitKindId};
use crate::knowledge::view::PlayerView;
use crate::module::RulesModule;
use crate::navigation::cost::StepCost;
use crate::navigation::MoveProfile;
use crate::world::unit::MOVE_DENOMINATOR;

/// The known map for one mover: the player's view and how the mover moves
/// (land units embark once the player knows how; ships sail).
#[derive(Clone, Copy)]
pub struct KnownMap<'v, 'a, M: RulesModule> {
    pub view: &'v PlayerView<'a, M>,
    pub profile: MoveProfile<M>,
}

impl<'v, 'a, M: RulesModule> KnownMap<'v, 'a, M> {
    /// A plain land unit of the player (no embarking).
    pub fn land(view: &'v PlayerView<'a, M>) -> Self {
        Self {
            view,
            profile: MoveProfile::land(view.routes()),
        }
    }

    /// One of the player's own units as it moves now.
    pub fn of_unit(view: &'v PlayerView<'a, M>, unit: UnitHandle) -> Self {
        let profile = view.own_unit_profile(unit).unwrap_or(MoveProfile::land(view.routes()));
        Self { view, profile }
    }

    /// A new unit of `kind` of the player (settler sites before training).
    pub fn of_kind(view: &'v PlayerView<'a, M>, kind: UnitKindId) -> Self {
        Self {
            view,
            profile: view.own_kind_profile(kind),
        }
    }
}

/// Price of a step that takes all moves left (river crossing): planned as
/// two movement points.
const ALL_MOVES_POINTS: u32 = 2 * MOVE_DENOMINATOR as u32;

/// Reusable search buffers (not saved: scratch, R-07 §3).
#[derive(Debug, Default)]
pub struct MapScratch {
    stamp: Vec<u32>,
    generation: u32,
    cost: Vec<u32>,
    parent: Vec<u32>,
    heap: BinaryHeap<Reverse<(u32, u32)>>,
}

impl MapScratch {
    fn begin(&mut self, tiles: usize) {
        if self.stamp.len() != tiles {
            self.stamp = vec![0; tiles];
            self.cost = vec![0; tiles];
            self.parent = vec![0; tiles];
            self.generation = 0;
        }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.stamp.fill(0);
            self.generation = 1;
        }
        self.heap.clear();
    }

    /// A new search from `from` (price 0).
    fn start(&mut self, tiles: usize, from: TileId) {
        self.begin(tiles);
        self.reach(from, 0, from);
        self.heap.push(Reverse((0, from.0)));
    }

    fn cost_of(&self, tile: TileId) -> u32 {
        let i = tile.0 as usize;
        if self.stamp[i] == self.generation {
            self.cost[i]
        } else {
            u32::MAX
        }
    }

    fn reach(&mut self, tile: TileId, cost: u32, parent: TileId) {
        let i = tile.0 as usize;
        self.stamp[i] = self.generation;
        self.cost[i] = cost;
        self.parent[i] = parent.0;
    }

    fn path_to(&self, from: TileId, to: TileId) -> Vec<TileId> {
        let mut tiles = vec![to];
        let mut at = to;
        while at != from {
            at = TileId(self.parent[at.0 as usize]);
            tiles.push(at);
        }
        tiles.reverse();
        tiles
    }
}

/// Deterministic work limit of one search (part of the AI decision budget).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchLimit {
    pub max_nodes: u32,
}

/// Work counters of the searches (deterministic).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchStats {
    pub searches: u64,
    pub nodes: u64,
    pub over_budget: u64,
    pub no_path: u64,
}

/// Tiles a plan must not enter: foreign units the player sees (a unit is
/// attacked or walked around, never walked into). Sorted.
#[derive(Debug, Clone, Default)]
pub struct Blockers {
    tiles: Vec<TileId>,
}

impl Blockers {
    pub fn new(mut tiles: Vec<TileId>) -> Self {
        tiles.sort();
        tiles.dedup();
        Self { tiles }
    }

    pub fn contains(&self, tile: TileId) -> bool {
        self.tiles.binary_search(&tile).is_ok()
    }
}

/// Result of a path search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathOutcome {
    /// Tiles from the start to the goal, both included, and the price in
    /// move points.
    Found { tiles: Vec<TileId>, points: u32 },
    /// The known map has no way there.
    NoPath,
    /// The node budget ran out first.
    OverBudget,
}

/// Move points of one known step; None when the step is impossible as far
/// as the player knows.
pub fn step_points<M: RulesModule>(map: &KnownMap<'_, '_, M>, from: TileId, to: TileId) -> Option<u32> {
    match map.view.known_step_cost_as(&map.profile, from, to)? {
        StepCost::Points(points) => Some(points),
        StepCost::AllMoves => Some(ALL_MOVES_POINTS),
    }
}

/// Whole turns a unit with `moves_max` points needs for `points`.
pub fn turns_for(points: u32, moves_max: u16) -> u32 {
    let per_turn = u32::from(moves_max.max(1));
    points.div_ceil(per_turn)
}

/// A* over known tiles. The goal may be entered even when blocked (the
/// caller decides what to do there).
pub fn find_path<M: RulesModule>(
    map: &KnownMap<'_, '_, M>,
    from: TileId,
    to: TileId,
    blockers: &Blockers,
    scratch: &mut MapScratch,
    limit: SearchLimit,
    stats: &mut SearchStats,
) -> PathOutcome {
    let _span = prof::span("search/ai_known_map_astar");
    stats.searches += 1;
    let dims = view_dimensions(map.view);
    scratch.start(map.view.tile_count(), from);
    let mut expanded = 0u32;
    while let Some(Reverse((_, tile))) = scratch.heap.pop() {
        let tile = TileId(tile);
        if tile == to {
            let tiles = scratch.path_to(from, to);
            let points = scratch.cost_of(to);
            return PathOutcome::Found { tiles, points };
        }
        expanded += 1;
        stats.nodes += 1;
        if expanded > limit.max_nodes {
            stats.over_budget += 1;
            return PathOutcome::OverBudget;
        }
        relax_neighbors(map, dims, tile, Some(to), blockers, scratch);
    }
    stats.no_path += 1;
    PathOutcome::NoPath
}

/// Push the neighbours of `tile` whose known step is cheaper than before.
/// With a goal the queue key is cost + distance estimate (A*), without one
/// it is the cost (Dijkstra).
fn relax_neighbors<M: RulesModule>(
    map: &KnownMap<'_, '_, M>,
    dims: crate::math::MapDimensions,
    tile: TileId,
    goal: Option<TileId>,
    blockers: &Blockers,
    scratch: &mut MapScratch,
) {
    let base = scratch.cost_of(tile);
    for next in dims.neighbors(tile) {
        if blockers.contains(next) && Some(next) != goal {
            continue;
        }
        // A step never costs less than nothing: a tile already reached at
        // `base` or cheaper cannot improve, so its (expensive) step price is
        // not asked for. Same result as pricing every step.
        if scratch.cost_of(next) <= base {
            continue;
        }
        let Some(points) = step_points(map, tile, next) else {
            continue;
        };
        let cost = base.saturating_add(points);
        if cost >= scratch.cost_of(next) {
            continue;
        }
        scratch.reach(next, cost, tile);
        let estimate = match goal {
            Some(goal) => {
                dims.distance(next, goal).unwrap_or(0) as u32 * u32::from(MOVE_DENOMINATOR)
            }
            None => 0,
        };
        scratch.heap.push(Reverse((cost + estimate, next.0)));
    }
}

/// What a visitor of `explore` wants next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visit {
    Continue,
    Stop,
}

/// Dijkstra over known tiles from `from` in price order; `visit` sees each
/// reached tile once (the start included) with its price in move points.
/// Ends when the visitor stops, the known area is exhausted or the node
/// budget runs out.
pub fn explore<M: RulesModule>(
    map: &KnownMap<'_, '_, M>,
    from: TileId,
    blockers: &Blockers,
    scratch: &mut MapScratch,
    limit: SearchLimit,
    stats: &mut SearchStats,
    visit: &mut dyn FnMut(TileId, u32) -> Visit,
) {
    let _span = prof::span("search/ai_known_map_explore");
    stats.searches += 1;
    let dims = view_dimensions(map.view);
    scratch.start(map.view.tile_count(), from);
    let mut expanded = 0u32;
    while let Some(Reverse((cost, tile))) = scratch.heap.pop() {
        let tile = TileId(tile);
        if cost > scratch.cost_of(tile) {
            continue;
        }
        if visit(tile, cost) == Visit::Stop {
            return;
        }
        expanded += 1;
        stats.nodes += 1;
        if expanded > limit.max_nodes {
            stats.over_budget += 1;
            return;
        }
        relax_neighbors(map, dims, tile, None, blockers, scratch);
    }
}

/// The path from the last `explore` start to a tile it reached.
pub fn explored_path(scratch: &MapScratch, from: TileId, to: TileId) -> Option<Vec<TileId>> {
    if scratch.cost_of(to) == u32::MAX {
        return None;
    }
    Some(scratch.path_to(from, to))
}

/// Unknown tiles within `radius` of `tile`: what a unit standing there
/// would reveal.
pub fn unknown_around<M: RulesModule>(view: &PlayerView<'_, M>, tile: TileId, radius: i32) -> u32 {
    let dims = view_dimensions(view);
    let mut unknown = 0;
    for near in crate::math::hex_disc(dims, tile, radius) {
        if !view.is_known(near) {
            unknown += 1;
        }
    }
    unknown
}

/// A known tile next to an unknown one.
pub fn is_frontier<M: RulesModule>(view: &PlayerView<'_, M>, tile: TileId) -> bool {
    let dims = view_dimensions(view);
    dims.neighbors(tile).into_iter().any(|n| !view.is_known(n))
}

fn view_dimensions<M: RulesModule>(view: &PlayerView<'_, M>) -> crate::math::MapDimensions {
    view.dimensions()
}
