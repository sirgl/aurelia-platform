//! What a player's AI sees of units and cities (§22 platform/structure.md; R-07 AI
//! stage B contract): its own units in full, other players' units and
//! cities only where it sees them now. A rules module builds its
//! observation from these parts and adds its own (Classic:
//! `classic_rules::knowledge::observation`); the combat facts come from the
//! module (`ModuleSystems::fortify_problem`, `attacks_left`,
//! `city_max_hit_points`).

use crate::ids::{CityHandle, EntityId, PlayerId, TileId, UnitHandle, UnitKindId, UnitWork};
use crate::knowledge::view::PlayerView;
use crate::metrics::WorkCounters;
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::systems::module::ModuleSystems;
use crate::world::city::CityCore;
use crate::world::state::AuthWorld;
use crate::world::unit::UnitCore;

/// A unit of another player, as seen now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedUnit {
    pub handle: UnitHandle,
    pub tile: TileId,
    pub owner: PlayerId,
    pub kind: UnitKindId,
    pub hp: u16,
}

/// A city of another player, as seen now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedCity {
    pub handle: CityHandle,
    pub tile: TileId,
    pub owner: PlayerId,
    pub population: u32,
    /// Health bar over the city (Classic shows it to everyone who sees it).
    pub hp: u16,
}

/// One of the player's own units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnUnit {
    pub handle: UnitHandle,
    /// Persistent identity (AI memory survives save/load by it).
    pub id: EntityId,
    pub tile: TileId,
    pub kind: UnitKindId,
    pub hp: u16,
    pub moves_left: u16,
    pub moves_max: u16,
    pub stance: crate::world::unit::Stance,
    /// The stance is `Fortified` (the planners' shorthand).
    pub fortified: bool,
    /// A `Fortify` order would be accepted (combat land unit with a
    /// defensive bonus, on land, not fortified yet).
    pub can_fortify: bool,
    /// The unit has moves and attacks left this turn.
    pub can_attack: bool,
    pub experience: u32,
    pub level: u8,
    pub attacks_made: u8,
    pub promotions: crate::world::unit::PromotionSet,
    pub work: Option<UnitWork>,
    pub goal: Option<TileId>,
}

pub fn units<M: RulesModule>(
    auth: &AuthWorld<M>,
    view: &PlayerView<'_, M>,
    c: &mut WorkCounters,
) -> (Vec<OwnUnit>, Vec<ObservedUnit>) {
    let player = view.player();
    let mut own = Vec::new();
    let mut foreign = Vec::new();
    for (handle, core) in auth.units().iter_handles() {
        c.entities_visited += 1;
        if core.owner == player {
            own.push(own_unit(auth, view.rules(), handle, core));
        } else if view.sees_unit(core) {
            foreign.push(observed_unit(handle, core));
        }
    }
    own.sort_by_key(|u| u.id);
    (own, foreign)
}

pub fn own_unit<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, handle: UnitHandle, core: &UnitCore) -> OwnUnit {
    OwnUnit {
        handle,
        id: core.id,
        tile: core.tile,
        kind: core.kind,
        hp: core.hp,
        moves_left: core.moves_left,
        moves_max: core.moves_max,
        stance: core.stance,
        fortified: matches!(core.stance, crate::world::unit::Stance::Fortified { .. }),
        can_fortify: M::Systems::fortify_problem(auth, rules, core).is_none(),
        can_attack: M::Systems::attacks_left(auth, rules, core) > 0,
        experience: core.experience,
        level: core.level,
        attacks_made: core.attacks_made,
        promotions: core.promotions,
        work: core.work,
        goal: core.goal,
    }
}

pub fn observed_unit(handle: UnitHandle, core: &UnitCore) -> ObservedUnit {
    ObservedUnit {
        handle,
        tile: core.tile,
        owner: core.owner,
        kind: core.kind,
        hp: core.hp,
    }
}

pub fn observed_city<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, handle: CityHandle, core: &CityCore) -> ObservedCity {
    ObservedCity {
        handle,
        tile: core.tile,
        owner: core.owner,
        population: core.population,
        hp: M::Systems::city_max_hit_points(auth, rules, core).saturating_sub(core.damage),
    }
}
