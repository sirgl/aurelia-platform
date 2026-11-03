//! Citizen orders of a city (Classic city screen): take a tile off work or
//! give it back, lock a tile so the city works it first, set the focus of
//! the automatic assignment, assign specialists by hand. The assignment
//! itself is derived (`derived::citizens`). Journal: CityChange::WorkedChanged.

use crate::command::CommandError;
use crate::derived::citizens::specialist_slots;
use crate::world::city::CITY_WORK_RADIUS;
use crate::ids::{CityHandle, PlayerId, TileId};
use crate::module::RulesModule;
use crate::rules::content::SpecialistId;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::CityChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::citizens::{CityFocus, MAX_FORCED_SPECIALISTS};
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;

fn violation(reason: &'static str) -> CommandError {
    CommandError::RuleViolation(reason.into())
}

/// An own city of `player`.
fn own_city<M: RulesModule>(
    auth: &AuthWorld<M>,
    player: PlayerId,
    city: CityHandle,
) -> Result<&CityCore, CommandError> {
    let core = auth.cities().get(city).ok_or(CommandError::UnknownCity)?;
    if core.owner != player {
        return Err(violation("not your city"));
    }
    Ok(core)
}

/// A tile of this city it may work: not the centre, in 3 rings, its own.
fn check_work_tile<M: RulesModule>(
    auth: &AuthWorld<M>,
    core: &CityCore,
    city: CityHandle,
    tile: TileId,
) -> Result<(), CommandError> {
    if tile == core.tile {
        return Err(violation("center always worked"));
    }
    let distance = auth.map().dimensions.distance(core.tile, tile);
    if distance.map(|d| d > CITY_WORK_RADIUS).unwrap_or(true) {
        return Err(violation("tile outside work zone"));
    }
    if auth.map().owning_city(tile) != Some(city) {
        return Err(violation("not this city's tile"));
    }
    Ok(())
}

pub(crate) fn check_toggle<M: RulesModule>(
    auth: &AuthWorld<M>,
    player: PlayerId,
    city: CityHandle,
    tile: TileId,
) -> Result<(), CommandError> {
    let core = own_city(auth, player, city)?;
    check_work_tile(auth, core, city, tile)?;
    Ok(())
}

pub(crate) fn check_lock<M: RulesModule>(
    auth: &AuthWorld<M>,
    player: PlayerId,
    city: CityHandle,
    tile: TileId,
) -> Result<(), CommandError> {
    let core = own_city(auth, player, city)?;
    check_work_tile(auth, core, city, tile)?;
    Ok(())
}

pub(crate) fn check_focus<M: RulesModule>(
    auth: &AuthWorld<M>,
    player: PlayerId,
    city: CityHandle,
    focus: CityFocus,
) -> Result<(), CommandError> {
    let core = own_city(auth, player, city)?;
    if core.citizens.focus == focus {
        return Err(violation("focus already set"));
    }
    Ok(())
}

pub(crate) fn check_avoid_growth<M: RulesModule>(
    auth: &AuthWorld<M>,
    player: PlayerId,
    city: CityHandle,
    avoid: bool,
) -> Result<(), CommandError> {
    let core = own_city(auth, player, city)?;
    if core.citizens.avoid_growth == avoid {
        return Err(violation("growth already set"));
    }
    Ok(())
}

/// Hand-assigned specialists: a kind with slots in the city, at most its
/// slots (the default specialist has no slots and is never forced).
pub(crate) fn check_specialists<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    city: CityHandle,
    specialist: SpecialistId,
    count: u8,
) -> Result<(), CommandError> {
    let core = own_city(auth, player, city)?;
    let slots = specialist_slots(rules, core);
    let free = slots.get(specialist.0 as usize).copied().unwrap_or(0);
    if u16::from(count) > free || count > MAX_FORCED_SPECIALISTS {
        return Err(violation("not enough specialist slots"));
    }
    if core.citizens.forced(specialist) == count {
        return Err(violation("specialists already set"));
    }
    Ok(())
}

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// Take a tile off work (it also loses its lock) or give it back.
    pub(crate) fn toggle_worked_tile(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        tile: TileId,
    ) -> Result<(), CommandError> {
        check_toggle(self.world, player, city, tile)?;
        let core = self
            .world
            .cities
            .get_mut(city)
            .ok_or(CommandError::UnknownCity)?;
        core.citizens.unlock(tile);
        self.world.cities.toggle_worked(city, tile);
        self.journal
            .city_changes
            .push(CityChange::WorkedChanged { city });
        Ok(())
    }

    /// Lock a tile (it comes back to work if it was off) or unlock it.
    pub(crate) fn lock_worked_tile(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        tile: TileId,
    ) -> Result<(), CommandError> {
        check_lock(self.world, player, city, tile)?;
        let core = self
            .world
            .cities
            .get_mut(city)
            .ok_or(CommandError::UnknownCity)?;
        if core.citizens.toggle_lock(tile) {
            core.unworked.retain(|t| *t != tile);
        }
        self.journal
            .city_changes
            .push(CityChange::WorkedChanged { city });
        Ok(())
    }

    pub(crate) fn set_city_focus(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        focus: CityFocus,
    ) -> Result<(), CommandError> {
        check_focus(self.world, player, city, focus)?;
        let core = self
            .world
            .cities
            .get_mut(city)
            .ok_or(CommandError::UnknownCity)?;
        core.citizens.focus = focus;
        self.journal
            .city_changes
            .push(CityChange::WorkedChanged { city });
        Ok(())
    }

    pub(crate) fn set_avoid_growth(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        avoid: bool,
    ) -> Result<(), CommandError> {
        check_avoid_growth(self.world, player, city, avoid)?;
        let core = self
            .world
            .cities
            .get_mut(city)
            .ok_or(CommandError::UnknownCity)?;
        core.citizens.avoid_growth = avoid;
        self.journal
            .city_changes
            .push(CityChange::WorkedChanged { city });
        Ok(())
    }

    pub(crate) fn set_specialists(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        specialist: SpecialistId,
        count: u8,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), CommandError> {
        check_specialists(self.world, rules, player, city, specialist, count)?;
        let core = self
            .world
            .cities
            .get_mut(city)
            .ok_or(CommandError::UnknownCity)?;
        core.citizens.set_forced(specialist, count);
        self.journal
            .city_changes
            .push(CityChange::WorkedChanged { city });
        Ok(())
    }
}
