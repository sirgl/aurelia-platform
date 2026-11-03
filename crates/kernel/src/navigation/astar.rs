//! A* over the hex map (§33 platform/project.md): six hex directions, hex-distance
//! heuristic, dense arrays, reusable scratch. Query service (§18): does not
//! mutate the world, caches nothing globally.
//!
//! Turn-aware cost (pathfinder, classic/docs/classic/rules.md): a unit may
//! enter a tile while it has any move points left, so a path's cost is not a
//! plain sum. The search runs on one scalar per tile,
//! `g = turns × scale + (scale − points left)` with `scale` the larger of
//! the unit's allowances on land and embarked: a step spends
//! `min(step cost, points left)`, a turn boundary refills the allowance of
//! the tile the unit stands on (a land unit on water has its embarked
//! moves, §7.1), and the points left after a step never exceed the
//! allowance of the entered tile. Embarking and landing end the move, so a
//! turn never mixes the two allowances (a one-point landing is capped by
//! the land allowance). Each step is monotone in its predecessor, so
//! Dijkstra/A* stay exact.
//!
//! Units and zones of control come from the mover's own view: only enemy
//! units and cities the player sees block tiles or slow steps (no hidden
//! information in a path, §28).

use crate::module::RulesModule;
use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::math::{HexCoord, MapDimensions, Neighbors};
use crate::navigation::cost::{min_step_points, step_cost_for, StepCost};
use crate::navigation::profile::MoveProfile;
use crate::rules::content::Domain;
use crate::navigation::scratch::PathScratch;
use crate::rules::CompiledRuleset;
use crate::world::state::{AuthWorld, GameView};

/// Move points of the moving unit: per turn in its own element, per turn
/// while embarked (a land unit on water), and left this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveBudget {
    pub max: u16,
    pub embarked: u16,
    pub left: u16,
}

impl MoveBudget {
    /// The same allowance on land and water (units that never embark).
    pub const fn flat(max: u16, left: u16) -> Self {
        Self { max, embarked: max, left }
    }

    /// The scalar's turn length: the larger allowance.
    fn scale(&self) -> u32 {
        u32::from(self.max.max(self.embarked))
    }
}

#[derive(Debug, Clone)]
pub struct PathRequest<M: RulesModule> {
    pub from: TileId,
    pub to: TileId,
    /// Whose unit is moving: bounds, enemies and zones of control are for it.
    pub player: PlayerId,
    pub budget: MoveBudget,
    /// How the unit moves (domain, embarkation, abilities).
    pub profile: MoveProfile<M>,
}

impl<M: RulesModule> PathRequest<M> {
    /// A request for one unit of the world (its tile, owner, moves and
    /// movement profile).
    pub fn for_unit(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, unit: UnitHandle, to: TileId) -> Option<Self> {
        let core = auth.units().get(unit)?.core;
        let mods = crate::effects::unit::unit_modifiers(auth, rules, core);
        let profile = MoveProfile::with_modifiers(auth, rules, core, &mods);
        Some(Self {
            from: core.tile,
            to,
            player: core.owner,
            budget: MoveBudget::of_unit(auth.map(), rules, core, &mods, &profile),
            profile,
        })
    }
}

impl MoveBudget {
    /// The budget of a unit now: its stored allowance where it stands, the
    /// other element's from its modifiers when it may cross (an embarking
    /// land unit), else the same on both.
    fn of_unit<M: RulesModule>(
        map: &crate::world::map::MapState,
        rules: &CompiledRuleset<M>,
        core: &crate::world::unit::UnitCore,
        mods: &crate::effects::unit::UnitModifiers<M>,
        profile: &MoveProfile<M>,
    ) -> Self {
        if !profile.embarks() {
            return Self::flat(core.moves_max, core.moves_left);
        }
        let (max, embarked) = if mods.embarked_on(rules, map, core.kind, core.tile) {
            (mods.movement(rules, core.kind), core.moves_max)
        } else {
            (core.moves_max, mods.embarked_movement())
        };
        Self { max, embarked, left: core.moves_left }
    }
}

/// Turns spent before the last step (0: reached this turn) and the points
/// left after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathCost {
    pub turns: u32,
    pub moves_left: u16,
}

impl PathCost {
    pub fn this_turn(&self) -> bool {
        self.turns == 0
    }

    pub fn from_scalar(g: u32, max: u32) -> Self {
        if g <= max {
            return Self {
                turns: 0,
                moves_left: (max - g) as u16,
            };
        }
        let turns = (g - 1) / max;
        Self {
            turns,
            moves_left: ((turns + 1) * max - g) as u16,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    /// Tiles from `from` to `to` inclusive.
    pub tiles: Vec<TileId>,
    pub cost: PathCost,
}

/// Allowances of one search: the scalar's turn length and the moves per
/// turn on land and while embarked.
#[derive(Debug, Clone, Copy)]
pub struct Allowance {
    scale: u32,
    land: u32,
    embarked: u32,
    /// The mover is a land unit that may embark (hovering units do not).
    embarks: bool,
}

impl Allowance {
    pub fn of<M: RulesModule>(budget: &MoveBudget, profile: &MoveProfile<M>) -> Self {
        Self {
            scale: budget.scale(),
            land: u32::from(budget.max),
            embarked: u32::from(budget.embarked),
            embarks: profile.embarks(),
        }
    }

    /// Moves per turn on `tile`: the embarked allowance for a land unit on
    /// water.
    pub fn full_on(&self, map: &crate::world::map::MapState, tile: TileId) -> u32 {
        if self.embarks && crate::navigation::terrain::is_water(map, tile) {
            self.embarked
        } else {
            self.land
        }
    }

    /// The scalar of the start: the points left now, at most the allowance
    /// of the start tile.
    pub fn start(&self, left: u16, full_here: u32) -> u32 {
        self.scale - u32::from(left).min(full_here)
    }

    /// (turn, points left) of a scalar on a tile whose allowance is
    /// `full_here`: `turn × scale` is the end of the previous turn with
    /// nothing left, i.e. a fresh turn here.
    fn decode(&self, g: u32, full_here: u32) -> (u32, u32) {
        let turn = g / self.scale;
        let used = g % self.scale;
        if used == 0 && g > 0 {
            return (turn, full_here);
        }
        (turn, self.scale - used)
    }

    /// One step on the scalar: spend `min(cost, points left)` (a fresh turn
    /// when nothing is left), keep at most the allowance of the entered
    /// tile (see the module header).
    pub fn advance(&self, g: u32, cost: StepCost, full_from: u32, full_to: u32) -> u32 {
        let (turn, left) = self.decode(g, full_from);
        let step = match cost {
            StepCost::Points(points) => points.min(left),
            StepCost::AllMoves => left,
        };
        let left_after = (left - step).min(full_to);
        turn * self.scale + (self.scale - left_after)
    }
}

/// The moving player's view of the dynamic layer: which enemy units and
/// cities it sees, and the zone-of-control rule (rules.md).
pub struct Mover<'a, M: RulesModule> {
    view: GameView<'a, M>,
    rules: &'a CompiledRuleset<M>,
    player: PlayerId,
    profile: MoveProfile<M>,
    enemies: Vec<PlayerId>,
}

impl<'a, M: RulesModule> Mover<'a, M> {
    pub fn new(view: GameView<'a, M>, rules: &'a CompiledRuleset<M>, player: PlayerId, profile: MoveProfile<M>) -> Self {
        let players = view.auth.players().count();
        let enemies = (0..players)
            .map(|p| PlayerId(p as u8))
            .filter(|p| *p != player && view.auth.diplomacy().at_war(player, *p))
            .collect();
        Self {
            view,
            rules,
            player,
            profile,
            enemies,
        }
    }

    fn visible(&self, tile: TileId) -> bool {
        let vis = &self.view.derived.visibility;
        let index = self.player.0 as usize * vis.tiles + tile.0 as usize;
        vis.counts.get(index).is_some_and(|c| *c > 0)
    }

    /// A visible enemy unit stands here: the tile can only be attacked.
    /// (The occupancy layer answers at once; an invisible unit the player
    /// has not revealed does not count.)
    fn enemy_unit_seen(&self, tile: TileId) -> bool {
        let occupancy = &self.view.derived.movement;
        let enemy = self.enemies.iter().any(|e| occupancy.units_of(*e, tile) > 0);
        enemy && self.visible(tile) && self.seen_enemy_on(tile, |_| true)
    }

    /// An enemy unit on `tile` that the moving player sees and `wanted`
    /// accepts.
    fn seen_enemy_on(&self, tile: TileId, wanted: impl Fn(&crate::world::unit::UnitCore) -> bool) -> bool {
        let auth = self.view.auth;
        auth.indexes().units_on(tile).iter().any(|h| {
            let Some(view) = auth.units().get(*h) else {
                return false;
            };
            let core = view.core;
            self.enemies.contains(&core.owner)
                && wanted(core)
                && crate::knowledge::view::unit_seen_by(auth, self.view.derived, self.rules, self.player, core)
        })
    }

    /// A visible enemy military unit or enemy city here exerts a zone of
    /// control on its neighbours (§7.6): ships control land and sea; land
    /// units control land only, and embarked units control nothing.
    fn controls(&self, tile: TileId) -> bool {
        if !self.visible(tile) {
            return false;
        }
        let occupancy = &self.view.derived.movement;
        let rules = self.rules;
        let naval = || {
            let present = self.enemies.iter().any(|e| occupancy.naval_of(*e, tile) > 0);
            present
                && self.seen_enemy_on(tile, |c| {
                    !rules.is_civilian(c.kind) && rules.units.get(c.kind.0 as usize).is_some_and(|d| d.domain == Domain::Sea)
                })
        };
        // Land units on water are embarked: they control nothing.
        let land = || {
            self.enemies.iter().any(|e| occupancy.military_of(*e, tile) > 0)
                && !crate::navigation::terrain::is_water(self.view.auth.map(), tile)
        };
        let military = match self.profile.domain {
            Domain::Sea => naval(),
            Domain::Land => land() || naval(),
            Domain::Air => false,
        };
        military
            || crate::knowledge::view::city_owner_on(self.view.auth, tile)
                .is_some_and(|owner| self.enemies.contains(&owner))
    }

    /// Tiles around `from` that exert a zone of control (at most six).
    pub(crate) fn control_around(&self, from: TileId) -> Neighbors {
        let mut around = self.view.auth.map().dimensions.neighbors(from);
        if self.enemies.is_empty() || self.profile.ignore_zone_of_control {
            around.retain(|_| false);
            return around;
        }
        around.retain(|t| self.controls(t));
        around
    }

    /// Price of `from → to` for this player, units and ZOC included; None
    /// when the unit cannot enter. `control` is `control_around(from)`.
    pub fn edge(&self, from: TileId, to: TileId, control: &Neighbors) -> Option<StepCost> {
        if self.enemy_unit_seen(to) {
            return None;
        }
        let cost = step_cost_for(self.view.auth, self.rules, self.player, &self.profile, from, to)?;
        let dims = self.view.auth.map().dimensions;
        let slowed = control.as_slice().iter().any(|source| dims.adjacent(*source, to));
        if slowed {
            return Some(StepCost::AllMoves);
        }
        Some(cost)
    }
}

/// Admissible estimate: hex distance × the cheapest possible step.
fn heuristic(dims: &MapDimensions, tile: u32, goal: HexCoord, min_step: u32) -> u32 {
    let at = HexCoord {
        q: (tile % dims.width) as i32,
        r: (tile / dims.width) as i32,
    };
    at.distance(goal) as u32 * min_step
}

/// Can any neighbour of the goal step onto it for this mover? The price of
/// a step (terrain, borders, foreign cities, the unit's own limits, the
/// module's hook) is asked exactly as the search asks it; units and zones
/// of control only ever add refusals or an all-moves price, so they are not
/// asked. When no neighbour can, the search can never reach the goal and
/// would only explore the whole reachable area to say so (a land unit
/// pointed at the sea or a mountain, which is what a hover preview does).
fn can_be_entered<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, req: &PathRequest<M>) -> bool {
    auth.map()
        .dimensions
        .neighbors(req.to)
        .into_iter()
        .any(|from| step_cost_for(auth, rules, req.player, &req.profile, from, req.to).is_some())
}

/// Search core: fills scratch, returns the scalar g of the target.
fn search<M: RulesModule>(
    world: GameView<'_, M>,
    rules: &CompiledRuleset<M>,
    req: &PathRequest<M>,
    scratch: &mut PathScratch,
) -> Option<u32> {
    search_core(world, rules, req, scratch, true)
}

/// The search as it was before the early exit: the reference of the
/// differential test (`kernel_tests/astar.rs`), not for gameplay code.
#[doc(hidden)]
pub fn find_path_without_early_exit<M: RulesModule>(
    world: GameView<'_, M>,
    rules: &CompiledRuleset<M>,
    request: PathRequest<M>,
    scratch: &mut PathScratch,
) -> Option<Path> {
    let g = search_core(world, rules, &request, scratch, false)?;
    Some(path_from_parents(&request, scratch, g))
}

fn search_core<M: RulesModule>(
    world: GameView<'_, M>,
    rules: &CompiledRuleset<M>,
    req: &PathRequest<M>,
    scratch: &mut PathScratch,
    early_exit: bool,
) -> Option<u32> {
    let map = world.auth.map();
    let n = map.tile_count();
    let dims = map.dimensions;
    let (f, goal_index) = (req.from.0 as usize, req.to.0 as usize);
    if f >= n || goal_index >= n || req.budget.max == 0 {
        return None;
    }
    let allowance = Allowance::of(&req.budget, &req.profile);
    let start = allowance.start(req.budget.left, allowance.full_on(map, req.from));
    if f == goal_index {
        return Some(start);
    }
    let goal = dims.to_coord(req.to)?;
    if early_exit && !can_be_entered(world.auth, rules, req) {
        scratch.expanded_last = 0;
        return None;
    }
    let mover = Mover::new(world, rules, req.player, req.profile);
    scratch.ensure(n);
    scratch.cost.fill(u32::MAX);
    scratch.parent.fill(u32::MAX);
    scratch.cost[f] = start;
    let min_step = min_step_points(map, &req.profile);
    let estimate = start + heuristic(&dims, req.from.0, goal, min_step);
    scratch.open.push(std::cmp::Reverse((estimate, start, req.from.0)));
    let mut expanded = 0u64;
    while let Some(std::cmp::Reverse((_, g, cur))) = scratch.open.pop() {
        if g != scratch.cost[cur as usize] {
            continue; // stale entry
        }
        expanded += 1;
        if cur == req.to.0 {
            scratch.expanded_last = expanded;
            return Some(g);
        }
        let control = mover.control_around(TileId(cur));
        let full_here = allowance.full_on(map, TileId(cur));
        for next in dims.neighbors(TileId(cur)) {
            let Some(cost) = mover.edge(TileId(cur), next, &control) else {
                continue;
            };
            let ng = allowance.advance(g, cost, full_here, allowance.full_on(map, next));
            let ni = next.0 as usize;
            if ng < scratch.cost[ni] {
                scratch.cost[ni] = ng;
                scratch.parent[ni] = cur;
                let f_score = ng.saturating_add(heuristic(&dims, next.0, goal, min_step));
                scratch.open.push(std::cmp::Reverse((f_score, ng, next.0)));
            }
        }
    }
    scratch.expanded_last = expanded;
    None
}

/// Tiles the request's unit reaches this turn and the move points it has
/// left there (its start included), by tile: the move preview of the
/// interface, under the same rules as a move order (borders, visible
/// enemies, zones of control). `req.to` is ignored.
pub fn reachable_this_turn<M: RulesModule>(
    world: GameView<'_, M>,
    rules: &CompiledRuleset<M>,
    req: &PathRequest<M>,
) -> Vec<(TileId, u16)> {
    let map = world.auth.map();
    let (n, dims) = (map.tile_count(), map.dimensions);
    if req.from.0 as usize >= n || req.budget.max == 0 {
        return Vec::new();
    }
    let allowance = Allowance::of(&req.budget, &req.profile);
    let scale = req.budget.scale();
    let mover = Mover::new(world, rules, req.player, req.profile);
    let start = allowance.start(req.budget.left, allowance.full_on(map, req.from));
    // A few dozen tiles at most: a list, not map-sized arrays.
    let mut reached: Vec<(TileId, u32)> = vec![(req.from, start)];
    let mut open = std::collections::BinaryHeap::new();
    open.push(std::cmp::Reverse((start, req.from.0)));
    while let Some(std::cmp::Reverse((g, cur))) = open.pop() {
        if g >= scale || reached.iter().any(|(t, best)| t.0 == cur && *best < g) {
            continue;
        }
        let control = mover.control_around(TileId(cur));
        let full_here = allowance.full_on(map, TileId(cur));
        for next in dims.neighbors(TileId(cur)) {
            let Some(cost) = mover.edge(TileId(cur), next, &control) else {
                continue;
            };
            let ng = allowance.advance(g, cost, full_here, allowance.full_on(map, next));
            match reached.iter_mut().find(|(t, _)| *t == next) {
                Some((_, best)) if *best <= ng => continue,
                Some((_, best)) => *best = ng,
                None => reached.push((next, ng)),
            }
            open.push(std::cmp::Reverse((ng, next.0)));
        }
    }
    reached.sort_by_key(|(t, _)| *t);
    reached.into_iter().map(|(t, g)| (t, (scale - g.min(scale)) as u16)).collect()
}

/// Cost only, no path allocation, for validate (the hot path of commands).
pub fn find_path_cost<M: RulesModule>(
    world: GameView<'_, M>,
    rules: &CompiledRuleset<M>,
    req: &PathRequest<M>,
    scratch: &mut PathScratch,
) -> Option<PathCost> {
    let _span = prof::span("search/nav_path_cost");
    let g = search(world, rules, req, scratch)?;
    Some(PathCost::from_scalar(g, req.budget.scale()))
}

/// Full path with tiles, for movement orders, AI and preview.
pub fn find_path<M: RulesModule>(
    world: GameView<'_, M>,
    rules: &CompiledRuleset<M>,
    request: PathRequest<M>,
    scratch: &mut PathScratch,
) -> Option<Path> {
    let _span = prof::span("search/nav_find_path");
    let g = search(world, rules, &request, scratch)?;
    Some(path_from_parents(&request, scratch, g))
}

fn path_from_parents<M: RulesModule>(request: &PathRequest<M>, scratch: &PathScratch, g: u32) -> Path {
    let mut tiles = vec![request.to];
    let mut cur = request.to.0;
    while cur != request.from.0 {
        cur = scratch.parent[cur as usize];
        debug_assert_ne!(cur, u32::MAX, "parent chain broken");
        tiles.push(TileId(cur));
    }
    tiles.reverse();
    let cost = PathCost::from_scalar(g, request.budget.scale());
    Path { tiles, cost }
}
