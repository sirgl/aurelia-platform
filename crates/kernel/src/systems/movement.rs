//! Movement: a unit walks its shortest path step by step while it has move
//! points (partial-move rule, classic/docs/classic/rules.md). MoveUnit walks
//! a path validated to fit this turn; MarchTo walks as far as the moves go.
//! Journal: UnitChange::Moved per step (sight and knowledge follow the path).

use crate::command::{CommandError, Refusal};
use crate::derived::DerivedWorld;
use crate::ids::{PlayerId, TileId, UnitHandle};
use crate::module::RulesModule;
use crate::navigation::astar::Mover;
use crate::navigation::cost::StepCost;
use crate::navigation::{find_path, PathRequest, PathScratch};
use crate::rules::CompiledRuleset;
use crate::systems::module::{KernelEvent, ModuleSystems};
use crate::transaction::journal::UnitChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::state::GameView;

/// How a walk ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkOutcome {
    Arrived,
    /// No move points left: the rest of the way is for later turns.
    OutOfMoves,
    /// The next step is impossible now (a unit in the way, a zone of control
    /// that ends the move on an occupied tile, a border that closed).
    Blocked,
    /// An enemy military unit came into the unit's sight (only walks that
    /// watch for danger: move orders).
    Danger,
}

/// Does a walk stop when an enemy comes into sight?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Watch {
    /// MoveUnit: the player chose this step knowingly.
    Ignore,
    /// MarchTo and its continuation: an enemy in sight ends the order.
    Danger,
}

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// Walk `unit` towards `to` along the shortest path. Each step is checked
    /// against the live world: any unit of another player on the next tile
    /// (seen or not) stops the walk, and the unit stops only where the
    /// stacking rule allows. Err: unknown unit or no path at all.
    pub fn walk(
        &mut self,
        unit: UnitHandle,
        to: TileId,
        rules: &CompiledRuleset<M>,
        scratch: &mut PathScratch,
        derived: &DerivedWorld<M>,
        watch: Watch,
    ) -> Result<WalkOutcome, CommandError> {
        let path = {
            let view = GameView {
                auth: &*self.world,
                derived,
            };
            let request = PathRequest::for_unit(view.auth, rules, unit, to).ok_or(CommandError::UnknownUnit)?;
            find_path(view, rules, request, scratch)
                .ok_or(CommandError::RuleViolation("unreachable".into()))?
        };
        for pair in path.tiles.windows(2) {
            if let Some(stop) = self.walk_step(unit, pair[0], pair[1], to, rules, derived)? {
                return Ok(stop);
            }
            if watch == Watch::Danger && pair[1] != to && self.enemy_in_sight(unit, rules, derived) {
                return Ok(WalkOutcome::Danger);
            }
        }
        Ok(WalkOutcome::Arrived)
    }

    /// MarchTo: walk as far as the moves go; the order stays while the way
    /// is long and ends on arrival, a blocked way or an enemy in sight.
    pub fn march(
        &mut self,
        unit: UnitHandle,
        to: TileId,
        rules: &CompiledRuleset<M>,
        scratch: &mut PathScratch,
        derived: &DerivedWorld<M>,
    ) -> Result<WalkOutcome, CommandError> {
        let outcome = self.walk(unit, to, rules, scratch, derived, Watch::Danger)?;
        let goal = match outcome {
            WalkOutcome::OutOfMoves => Some(to),
            WalkOutcome::Arrived | WalkOutcome::Blocked | WalkOutcome::Danger => None,
        };
        self.set_goal(unit, goal);
        Ok(outcome)
    }

    /// Start of a player's turn: units with a move order walk on (Classic
    /// moves them at the start of the turn). An enemy already in sight, a
    /// blocked way or a lost path cancel the order.
    pub(crate) fn continue_move_orders(
        &mut self,
        player: PlayerId,
        rules: &CompiledRuleset<M>,
        scratch: &mut PathScratch,
        derived: &DerivedWorld<M>,
    ) {
        let units = self.world.indexes.units_of(player).to_vec();
        for unit in units {
            let Some(goal) = self.world.units.get(unit).and_then(|u| u.core.goal) else {
                continue;
            };
            if self.enemy_in_sight(unit, rules, derived) {
                self.set_goal(unit, None);
                continue;
            }
            if self.march(unit, goal, rules, scratch, derived).is_err() {
                self.set_goal(unit, None);
            }
        }
    }

    /// CancelOrder: the unit's march and build end where it stands. Its
    /// position, stance and moves are untouched; nothing is spent.
    pub fn cancel_order(&mut self, unit: UnitHandle) -> Result<(), CommandError> {
        let core = self.world.units.get_mut_core(unit).ok_or(CommandError::UnknownUnit)?;
        core.goal = None;
        core.work = None;
        Ok(())
    }

    /// Set or cancel the unit's move order.
    pub(crate) fn set_goal(&mut self, unit: UnitHandle, goal: Option<TileId>) {
        if let Some(core) = self.world.units.get_mut_core(unit) {
            core.goal = goal;
        }
    }

    /// Does the unit itself see an enemy military unit (at war, within its
    /// sight radius and line of sight)? Read from the live world: the unit
    /// sees what stands around it now.
    fn enemy_in_sight(&self, unit: UnitHandle, rules: &CompiledRuleset<M>, derived: &DerivedWorld<M>) -> bool {
        use crate::derived::visibility::{covered_tiles, SIGHT_RADIUS};
        let Some(core) = self.world.units.get(unit).map(|u| *u.core) else {
            return false;
        };
        let map = &self.world.map;
        // The unit's own sight profile (its promotions and owner sources as
        // the derived state has them) standing where it stands now.
        let sight = derived
            .visibility
            .cover_of(unit)
            .map_or(SIGHT_RADIUS, |cover| cover.sight.at(map, core.tile));
        let mut around = covered_tiles(map, core.tile, sight);
        around.any(|tile| self.enemy_military_on(core.owner, tile, rules))
    }

    fn enemy_military_on(&self, owner: PlayerId, tile: TileId, rules: &CompiledRuleset<M>) -> bool {
        self.world.indexes.units_on(tile).iter().any(|other| {
            self.world.units.get(*other).is_some_and(|o| {
                self.world.diplomacy.at_war(owner, o.core.owner) && !rules.is_civilian(o.core.kind)
            })
        })
    }

    /// One step `from → next`; Some(outcome) when the walk stops before it.
    fn walk_step(
        &mut self,
        unit: UnitHandle,
        from: TileId,
        next: TileId,
        destination: TileId,
        rules: &CompiledRuleset<M>,
        derived: &DerivedWorld<M>,
    ) -> Result<Option<WalkOutcome>, CommandError> {
        let core = *self.world.units.get(unit).ok_or(CommandError::UnknownUnit)?.core;
        if core.moves_left == 0 {
            return Ok(Some(WalkOutcome::OutOfMoves));
        }
        if self.foreign_unit_on(core.owner, next) {
            return Ok(Some(WalkOutcome::Blocked));
        }
        let profile = crate::navigation::MoveProfile::of_unit(self.world, rules, &core);
        let Some(cost) = self.live_step_cost(core.owner, &profile, from, next, rules, derived) else {
            return Ok(Some(WalkOutcome::Blocked));
        };
        let left_after = match cost {
            StepCost::Points(points) => u32::from(core.moves_left).saturating_sub(points) as u16,
            StepCost::AllMoves => 0,
        };
        let stops_here = next == destination || left_after == 0;
        let stop = crate::world::stacking::can_stop_at(self.world, rules, core.owner, core.kind, Some(unit), next);
        if stops_here && stop.is_err() {
            return Ok(Some(WalkOutcome::Blocked));
        }
        self.move_unit(unit, from, next, core.moves_left - left_after, &profile, rules)?;
        Ok(None)
    }

    /// The step price for the mover's view (units it sees, zones of
    /// control); None when the step is impossible.
    fn live_step_cost(
        &self,
        owner: PlayerId,
        profile: &crate::navigation::MoveProfile<M>,
        from: TileId,
        next: TileId,
        rules: &CompiledRuleset<M>,
        derived: &DerivedWorld<M>,
    ) -> Option<StepCost> {
        let view = GameView {
            auth: &*self.world,
            derived,
        };
        let mover = Mover::new(view, rules, owner, *profile);
        let control = mover.control_around(from);
        mover.edge(from, next, &control)
    }

    /// Does a unit of another player stand on `tile` (seen or not)?
    fn foreign_unit_on(&self, owner: PlayerId, tile: TileId) -> bool {
        let units = &self.world.units;
        self.world
            .indexes
            .units_on(tile)
            .iter()
            .any(|h| units.get(*h).is_some_and(|u| u.core.owner != owner))
    }

    /// Move one step, spending `cost` move points (at most the points left).
    /// The checks come before any change: an Err leaves the world as it was.
    pub fn move_unit(
        &mut self,
        unit: UnitHandle,
        from: TileId,
        to: TileId,
        cost: u16,
        profile: &crate::navigation::MoveProfile<M>,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), CommandError> {
        let owner = self.unit_owner(unit)?;
        self.check_border_entry(owner, profile, to)?;
        let core = self
            .world
            .units
            .get_mut_core(unit)
            .ok_or(CommandError::UnknownUnit)?;
        if core.moves_left == 0 || cost > core.moves_left {
            return Err(CommandError::Refused(Refusal::NoMovesLeft));
        }
        core.tile = to;
        core.moves_left -= cost;
        // Moving ends any stance (fortification, siege set-up, interception).
        core.stance = crate::world::unit::Stance::Ready;
        // A march abandons construction (as in Nation: changing the tile = a new order).
        core.work = None;
        self.world.indexes.move_unit(unit, from, to);
        self.journal
            .unit_changes
            .push(UnitChange::Moved { unit, from, to });
        self.refresh_allowance_across(unit, from, to, rules);
        let event = KernelEvent::UnitEntered { player: owner, unit, tile: to };
        M::Systems::on_event(self, rules, event);
        Ok(())
    }

    /// A land unit that stepped between land and water (embarking,
    /// landing) switches to the move allowance of its new element
    /// (classic/docs/classic/rules.md); the points left never grow.
    pub fn refresh_allowance_across(&mut self, unit: UnitHandle, from: TileId, to: TileId, rules: &CompiledRuleset<M>) {
        let map = &self.world.map;
        if crate::navigation::terrain::is_water(map, from) == crate::navigation::terrain::is_water(map, to) {
            return;
        }
        let land = self.world.units.get(unit).and_then(|u| rules.units.get(u.core.kind.0 as usize));
        if land.is_some_and(|d| d.domain == crate::rules::content::Domain::Land) {
            self.refresh_allowance(unit, rules);
        }
    }

    /// The unit's moves per turn where it stands now (embarked or not); the
    /// points left never exceed them.
    pub fn refresh_allowance(&mut self, unit: UnitHandle, rules: &CompiledRuleset<M>) {
        let Some(core) = self.world.units.get(unit).map(|u| *u.core) else {
            return;
        };
        let mods = crate::effects::unit::unit_modifiers(self.world, rules, &core);
        let max = mods.moves_on(rules, &self.world.map, core.kind, core.tile);
        if let Some(core) = self.world.units.get_mut_core(unit) {
            core.moves_max = max;
            core.moves_left = core.moves_left.min(max);
        }
    }

    /// Borders: foreign territory in peacetime only with a grant.
    /// One point for all movements (each step of MoveUnit and MarchTo).
    fn check_border_entry(
        &self,
        owner: PlayerId,
        profile: &crate::navigation::MoveProfile<M>,
        to: TileId,
    ) -> Result<(), CommandError> {
        if crate::world::territory::unit_may_enter(self.world, profile, owner, to) {
            return Ok(());
        }
        Err(CommandError::Refused(Refusal::ClosedBorders))
    }

    pub fn unit_owner(&self, unit: UnitHandle) -> Result<PlayerId, CommandError> {
        match self.world.units.get(unit) {
            Some(view) => Ok(view.core.owner),
            None => Err(CommandError::UnknownUnit),
        }
    }
}
