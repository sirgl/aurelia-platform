//! Validation of the kernel's commands: read-only, without side effects,
//! deterministic (§11). The rules module adds its own rules at fixed
//! points (`ModuleSystems::founding_problem`, `production_problem`, ...).
//!
//! Scratch for path search is passed from outside (owned by GameState, reused):
//! the output of validate depends only on (state, rules, command), observably pure.

use crate::command::{CommandError, KernelCommand, Refusal, ResolvedKernel};
use crate::derived::tile_yields::TechSet;
use crate::effects::player::{building_unlocked, unit_unlocked};
use crate::ids::{CityHandle, ImprovementId, PlayerId, ProductionItem, UnitHandle};
use crate::module::RulesModule;
use crate::navigation::{find_path_cost, PathRequest, PathScratch};
use crate::rules::content::{Domain, ImprovementKind};
use crate::rules::CompiledRuleset;
use crate::systems::improvements::{idle_builder, Builder};
use crate::systems::module::ModuleSystems;
use crate::world::builds::{improvement_site, removal_site};
use crate::world::state::{AuthWorld, GameView};

fn violation(reason: impl Into<String>) -> CommandError {
    CommandError::RuleViolation(reason.into())
}

/// Validate a kernel command: only the active player acts (sequential
/// turns, `turn::state`), then the command's own rules.
pub fn validate_kernel<M: RulesModule>(
    state: GameView<'_, M>,
    rules: &CompiledRuleset<M>,
    command: KernelCommand,
    scratch: &mut PathScratch,
) -> Result<ResolvedKernel, CommandError> {
    if let Some(actor) = command.acting_player(state.auth) {
        if actor != state.auth.active_player() {
            return Err(violation("not your turn"));
        }
    }
    let auth = state.auth;
    // In a simultaneous window the refusal of a move is an answer to a player who may be probing:
    // whether a tile can be entered says what is on it. A seat orders moves to tiles it has
    // explored (the interface offers no others).
    if auth.turn().is_acting() {
        if let KernelCommand::MoveUnit { destination, .. } | KernelCommand::MarchTo { destination, .. } = command {
            let explored = auth.knowledge().get(auth.active_player().0 as usize).is_some_and(|k| k.is_explored(destination));
            if !explored {
                return Err(violation("unexplored destination"));
            }
        }
    }
    match command {
        KernelCommand::MoveUnit { unit, destination } => validate_move(state, rules, unit, destination, scratch),
        KernelCommand::MarchTo { unit, destination } => validate_march(auth, rules, unit, destination),
        KernelCommand::CancelOrder { unit } => {
            let view = auth.units().get(unit).ok_or(CommandError::UnknownUnit)?;
            if !view.core.has_order() {
                return Err(violation("no standing order to cancel"));
            }
            Ok(ResolvedKernel::CancelOrder { unit })
        }
        KernelCommand::FoundCity { settler } => validate_found(auth, rules, settler),
        KernelCommand::Fortify { unit } => {
            let core = *auth.units().get(unit).ok_or(CommandError::UnknownUnit)?.core;
            if core.stance.is_fortified() {
                return Err(CommandError::Refused(Refusal::AlreadyFortified));
            }
            match M::Systems::fortify_problem(auth, rules, &core) {
                Some(reason) => Err(violation(reason)),
                None => Ok(ResolvedKernel::Fortify { unit }),
            }
        }
        KernelCommand::BuildImprovement { unit, improvement } => validate_build(auth, rules, unit, improvement),
        KernelCommand::BuildRoad { unit } => validate_road(auth, rules, unit),
        KernelCommand::ChopForest { unit } => validate_removal(auth, rules, unit),
        KernelCommand::ChooseTechnology { player, technology } => {
            validate_technology(auth, rules, player, technology)?;
            Ok(ResolvedKernel::ChooseTechnology { player, technology })
        }
        KernelCommand::PlanResearch { player, target } => {
            if !auth.players().is_alive(player) {
                return Err(CommandError::UnknownPlayer);
            }
            let queue = crate::systems::research::plan_research(rules, auth.players().techs_of(player), target)
                .map_err(|e| violation(e.to_string()))?;
            Ok(ResolvedKernel::PlanResearch { player, target, queue })
        }
        KernelCommand::EnqueueProduction { player, city, item } => {
            validate_enqueue(auth, rules, player, city, item)?;
            Ok(ResolvedKernel::EnqueueProduction { player, city, item })
        }
        KernelCommand::ClearProduction { player, city } => {
            own_city(auth, player, city)?;
            Ok(ResolvedKernel::ClearProduction { player, city })
        }
        KernelCommand::HurryProduction { player, city } => {
            validate_hurry(auth, rules, player, city)?;
            Ok(ResolvedKernel::HurryProduction { player, city })
        }
        KernelCommand::PurchaseTile { player, city, tile } => {
            own_city(auth, player, city)?;
            let offer = crate::world::territory::tile_purchase_offer(auth, rules, city, tile)
                .ok_or(CommandError::Refused(Refusal::TileNotForSale))?;
            let treasury = auth.players().treasury_of(player);
            if treasury < offer.price {
                return Err(CommandError::Refused(Refusal::NotEnoughGold { price: offer.price, treasury }));
            }
            Ok(ResolvedKernel::PurchaseTile { player, city, tile })
        }
        KernelCommand::ToggleWorkedTile { player, city, tile } => {
            crate::systems::worked_tiles::check_toggle(auth, player, city, tile)?;
            Ok(ResolvedKernel::ToggleWorkedTile { player, city, tile })
        }
        KernelCommand::LockWorkedTile { player, city, tile } => {
            crate::systems::worked_tiles::check_lock(auth, player, city, tile)?;
            Ok(ResolvedKernel::LockWorkedTile { player, city, tile })
        }
        KernelCommand::SetCityFocus { player, city, focus } => {
            crate::systems::worked_tiles::check_focus(auth, player, city, focus)?;
            Ok(ResolvedKernel::SetCityFocus { player, city, focus })
        }
        KernelCommand::SetSpecialists { player, city, specialist, count } => {
            crate::systems::worked_tiles::check_specialists(auth, rules, player, city, specialist, count)?;
            Ok(ResolvedKernel::SetSpecialists { player, city, specialist, count })
        }
        KernelCommand::SetAvoidGrowth { player, city, avoid } => {
            crate::systems::worked_tiles::check_avoid_growth(auth, player, city, avoid)?;
            Ok(ResolvedKernel::SetAvoidGrowth { player, city, avoid })
        }
        KernelCommand::ChooseCityName { player, city, name } => {
            crate::systems::city_naming::check_city_name(auth, rules, player, city, name)?;
            Ok(ResolvedKernel::ChooseCityName { player, city, name })
        }
        KernelCommand::EndTurn { player } => {
            if !auth.players().is_alive(player) {
                return Err(CommandError::UnknownPlayer);
            }
            // A seat of a simultaneous window never ends a turn itself: the
            // turns end in the round's resolution, in player order.
            if auth.turn().is_acting() {
                return Err(violation("a turn ends in the round's resolution"));
            }
            Ok(ResolvedKernel::EndTurn { player })
        }
    }
}

/// An own city of `player`.
fn own_city<'w, M: RulesModule>(
    auth: &'w AuthWorld<M>,
    player: PlayerId,
    city: CityHandle,
) -> Result<&'w crate::world::city::CityCore, CommandError> {
    let core = auth.cities().get(city).ok_or(CommandError::UnknownCity)?;
    if core.owner != player {
        return Err(violation("not your city"));
    }
    Ok(core)
}

fn validate_move<M: RulesModule>(
    state: GameView<'_, M>,
    rules: &CompiledRuleset<M>,
    unit: UnitHandle,
    destination: crate::ids::TileId,
    scratch: &mut PathScratch,
) -> Result<ResolvedKernel, CommandError> {
    let core = *state.auth.units().get(unit).ok_or(CommandError::UnknownUnit)?.core;
    if destination.0 as usize >= state.auth.map().tile_count() {
        return Err(CommandError::BadDestination);
    }
    // Target bounds as an explicit reason; the path bypasses them (A* closes
    // every step by the same rule unit_may_enter).
    let profile = crate::navigation::MoveProfile::of_unit(state.auth, rules, &core);
    if !crate::world::territory::unit_may_enter(state.auth, &profile, core.owner, destination) {
        return Err(CommandError::Refused(Refusal::ClosedBorders));
    }
    if destination == core.tile {
        return Err(violation("already there"));
    }
    // Turn-aware path cost: the whole way must fit this turn's moves
    // (partial-move rule). Impassable vs exhausted, explicitly.
    let request = PathRequest::for_unit(state.auth, rules, unit, destination).ok_or(CommandError::UnknownUnit)?;
    let cost = find_path_cost(state, rules, &request, scratch).ok_or(CommandError::Impassable)?;
    if !cost.this_turn() {
        return Err(CommandError::Refused(Refusal::NoMovesLeft));
    }
    crate::world::stacking::can_stop_at(state.auth, rules, core.owner, core.kind, Some(unit), destination)
        .map_err(violation)?;
    Ok(ResolvedKernel::MoveUnit { unit, to: destination })
}

/// Cheap validate WITHOUT A*: existence, bounds, passability of the target and
/// "not already standing there". The route and the step are in commit (one A*).
fn validate_march<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    unit: UnitHandle,
    destination: crate::ids::TileId,
) -> Result<ResolvedKernel, CommandError> {
    let view = auth.units().get(unit).ok_or(CommandError::UnknownUnit)?;
    if view.core.tile == destination {
        return Err(violation("already there"));
    }
    if destination.0 as usize >= auth.map().tile_count() {
        return Err(CommandError::BadDestination);
    }
    let profile = crate::navigation::MoveProfile::of_unit(auth, rules, view.core);
    if !crate::navigation::cost::destination_ok(auth, &profile, destination) {
        return Err(CommandError::Impassable);
    }
    // Target bounds (the march steps of A* bypass them, commit checks each).
    if !crate::world::territory::unit_may_enter(auth, &profile, view.core.owner, destination) {
        return Err(CommandError::Refused(Refusal::ClosedBorders));
    }
    if view.core.moves_left == 0 {
        return Err(CommandError::Refused(Refusal::NoMovesLeft));
    }
    Ok(ResolvedKernel::March { unit, to: destination })
}

/// Only a founder kind (settler) founds a city; the kind is looked up in content:
/// an unknown kind is not a settler.
fn validate_found<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    settler: UnitHandle,
) -> Result<ResolvedKernel, CommandError> {
    let view = auth.units().get(settler).ok_or(CommandError::UnknownUnit)?;
    let is_founder = rules.units.get(view.core.kind.0 as usize).map(|u| u.founder).unwrap_or(false);
    if !is_founder && !M::Systems::founds_cities(rules, view.core.kind) {
        return Err(violation("not a settler"));
    }
    if let Some(reason) = M::Systems::unit_founding_problem(auth, rules, view.core.owner, view.core.kind, view.core.tile) {
        return Err(violation(reason));
    }
    if let Some(reason) = M::Systems::founding_problem(auth, rules, view.core.owner, view.core.tile) {
        return Err(violation(reason));
    }
    crate::world::territory::city_site_problem(auth, view.core.owner, view.core.tile)
        .map_err(|problem| CommandError::Refused(Refusal::Site(problem)))?;
    Ok(ResolvedKernel::FoundCity { settler, at: view.core.tile, owner: view.core.owner })
}

fn validate_technology<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    technology: crate::ids::TechId,
) -> Result<(), CommandError> {
    if !auth.players().is_alive(player) {
        return Err(CommandError::UnknownPlayer);
    }
    let def = rules.technologies.get(technology.0 as usize).ok_or(violation("unknown technology"))?;
    let known = auth.players().techs_of(player);
    if known.contains(&technology) {
        return Err(violation("already researched"));
    }
    // Tree prerequisites: all requires are already learned (strict v1:
    // chains are ordered in sequence, not ahead).
    if def.requires.iter().any(|req| !known.contains(req)) {
        return Err(violation("prerequisite missing"));
    }
    let queue = auth.players().research_queue_of(player);
    if queue.contains(&technology) {
        return Err(violation("already queued"));
    }
    if queue.len() >= crate::world::player::RESEARCH_QUEUE_MAX {
        return Err(violation("research queue full"));
    }
    Ok(())
}

/// The kernel's rules of an order, then the module's
/// (`ModuleSystems::production_problem`), then the queue's length.
fn validate_enqueue<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    city: CityHandle,
    item: ProductionItem,
) -> Result<(), CommandError> {
    let core = own_city(auth, player, city)?;
    let nation = auth.players().nation_of(player);
    match rules.production_gate(item, nation) {
        Some(crate::rules::compiled::ProductionBlock::Unbuildable) => return Err(CommandError::Refused(Refusal::NotProducible)),
        Some(crate::rules::compiled::ProductionBlock::OtherNation) => {
            return Err(CommandError::Refused(Refusal::OtherNation))
        }
        None => {}
    }
    match item {
        ProductionItem::Unit(kind) => {
            let def = rules.units.get(kind.0 as usize).ok_or(violation("unknown unit kind"))?;
            if rules.replaced_for(nation, kind) {
                return Err(CommandError::Refused(Refusal::ReplacedByUniqueUnit));
            }
            if let Some(req) = def.required_tech {
                let known = auth.players().techs_of(player).contains(&req);
                if !known && !unit_unlocked(auth, rules, player, kind) {
                    return Err(CommandError::Refused(Refusal::NeedsTechnology(req)));
                }
            }
            if crate::effects::player::unit_class_forbidden(auth, rules, player, kind) {
                return Err(violation("forbidden to this nation"));
            }
        }
        ProductionItem::Project(_) => {}
        ProductionItem::Building(kind) => {
            let def = rules.buildings.get(kind.0 as usize).ok_or(violation("unknown building"))?;
            if core.buildings.contains(&kind) {
                return Err(CommandError::Refused(Refusal::AlreadyBuilt));
            }
            if core.queue.contains(&item) {
                return Err(CommandError::Refused(Refusal::AlreadyQueued));
            }
            // A world or national wonder is built in one city of a player at
            // a time: queued in another own city, it is refused here.
            let once = def.wonder || def.kind == crate::rules::content::BuildingKind::NationalWonder;
            if once && auth.cities().iter().any(|(h, c)| h != city && c.owner == player && c.queue.contains(&item)) {
                return Err(CommandError::Refused(Refusal::QueuedElsewhere));
            }
            if let Some(req) = def.required_tech {
                let known = auth.players().techs_of(player).contains(&req);
                if !known && !building_unlocked(auth, rules, player, kind) {
                    return Err(CommandError::Refused(Refusal::NeedsTechnology(req)));
                }
            }
            crate::world::building_rules::building_site(auth, rules, city, kind).map_err(violation)?;
            // Wonder: global uniqueness; as soon as any
            // city has built it, the order is rejected for everyone.
            if def.wonder && auth.cities().iter().any(|(_, c)| c.buildings.contains(&kind)) {
                return Err(CommandError::Refused(Refusal::WonderBuilt));
            }
        }
    }
    M::Systems::production_problem(auth, rules, player, core, item)?;
    if core.queue.len() >= crate::world::city::MAX_QUEUE {
        return Err(violation("queue full"));
    }
    Ok(())
}

fn validate_hurry<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    city: CityHandle,
) -> Result<(), CommandError> {
    let core = own_city(auth, player, city)?;
    if core.queue.is_empty() {
        return Err(CommandError::Refused(Refusal::NothingQueued));
    }
    M::Systems::hurry_problem(auth, rules, player, core)?;
    // Same price as the commit (one query).
    let price = crate::effects::city::head_hurry_price(auth, rules, core).ok_or(violation("cannot be purchased"))?;
    let treasury = auth.players().treasury_of(player);
    if treasury < price {
        return Err(CommandError::Refused(Refusal::NotEnoughGold { price, treasury }));
    }
    Ok(())
}

/// Validate `BuildImprovement`: a worker builds regular and unique land
/// improvements, a work boat water ones, a combat unit its own builds.
fn validate_build<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    unit: UnitHandle,
    improvement: ImprovementId,
) -> Result<ResolvedKernel, CommandError> {
    let (owner, tile, builder) = idle_builder(auth, rules, unit)?;
    let def = rules.improvements.get(improvement.0 as usize).ok_or_else(|| violation("unknown improvement"))?;
    let by_builder = match (builder, def.kind) {
        (Builder::Combat { improvements, .. }, _) => improvements.contains(&improvement),
        // An improvement that prompts its builder for a choice is built by
        // the module's own command, never a worker.
        (Builder::Worker(_), ImprovementKind::Regular | ImprovementKind::Unique) if def.placement.prompt_when_complete => false,
        (Builder::Worker(domain), ImprovementKind::Regular | ImprovementKind::Unique) => {
            if def.placement.water {
                domain == Domain::Sea
            } else {
                domain == Domain::Land && !def.unbuildable
            }
        }
        _ => false,
    };
    if !by_builder {
        return Err(violation("not built by this unit"));
    }
    let plan = improvement_site(auth, rules, owner, tile, improvement).map_err(violation)?;
    Ok(ResolvedKernel::BuildImprovement { unit, improvement, tile, work_time: plan.work_time })
}

/// Validate `BuildRoad`: a land worker or a road-building combat unit, the
/// road technology, land, no road.
fn validate_road<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    unit: UnitHandle,
) -> Result<ResolvedKernel, CommandError> {
    let (owner, tile, builder) = idle_builder(auth, rules, unit)?;
    let roads = match builder {
        Builder::Worker(domain) => domain == Domain::Land,
        Builder::Combat { roads, .. } => roads,
    };
    if !roads {
        return Err(violation("not built by this unit"));
    }
    if !TechSet::of_player(auth, owner).allows(rules.roles.road_tech) {
        return Err(violation("roads are locked"));
    }
    if auth.map().is_water(tile) || crate::navigation::cost::enter_cost_on_map(auth.map(), tile).is_none() {
        return Err(violation("water"));
    }
    if auth.map().road(tile) {
        return Err(violation("already road"));
    }
    Ok(ResolvedKernel::BuildRoad { unit, tile })
}

/// Validate `ChopForest` (remove any removable feature): land workers only.
fn validate_removal<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    unit: UnitHandle,
) -> Result<ResolvedKernel, CommandError> {
    let (owner, tile, builder) = idle_builder(auth, rules, unit)?;
    if !matches!(builder, Builder::Worker(Domain::Land)) {
        return Err(violation("not built by this unit"));
    }
    let (feature, removal) = removal_site(auth, rules, owner, tile).map_err(violation)?;
    Ok(ResolvedKernel::ChopForest { unit, tile, feature, work_time: removal.work_time })
}
