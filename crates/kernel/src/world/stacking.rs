//! One unit per tile and class (classic/docs/classic/
//! rules.md): a tile holds at most one unit of each class
//! (land military, land civilian, naval military, naval civilian, embarked)
//! of one player, and no unit of another player. Aircraft are not limited
//! here (a city holds up to its air capacity). The limit applies where a
//! unit ends its move; passing through own units is free.

use crate::module::RulesModule;
use crate::ids::{PlayerId, TileId, UnitHandle, UnitKindId};
use crate::rules::content::Domain;
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;

/// The class a unit counts in for the one-unit-per-tile limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackClass {
    Military,
    Civilian,
    NavalMilitary,
    NavalCivilian,
    /// Every land unit on water is one class (Classic: embarked units).
    Embarked,
    /// Not limited by the rule.
    Air,
}

/// The class of a unit of `kind` standing on `tile`.
pub(crate) fn stack_class_on<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, kind: UnitKindId, tile: TileId) -> StackClass {
    let domain = rules.units.get(kind.0 as usize).map(|d| d.domain).unwrap_or_default();
    let civilian = rules.is_civilian(kind);
    match domain {
        Domain::Air => StackClass::Air,
        Domain::Sea if civilian => StackClass::NavalCivilian,
        Domain::Sea => StackClass::NavalMilitary,
        Domain::Land if crate::navigation::terrain::is_water(auth.map(), tile) => StackClass::Embarked,
        Domain::Land if civilian => StackClass::Civilian,
        Domain::Land => StackClass::Military,
    }
}

/// The land class of a unit kind (military or civilian), as on land.
pub fn stack_class<M: RulesModule>(rules: &CompiledRuleset<M>, kind: UnitKindId) -> StackClass {
    if rules.is_civilian(kind) {
        StackClass::Civilian
    } else {
        StackClass::Military
    }
}

/// May a unit (`owner`, `kind`; `moving` is the unit itself when it is
/// already on the map) end its move on `tile`? Err carries the reason.
pub fn can_stop_at<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    kind: UnitKindId,
    moving: Option<UnitHandle>,
    tile: TileId,
) -> Result<(), &'static str> {
    let class = stack_class_on(auth, rules, kind, tile);
    for &other in auth.indexes().units_on(tile) {
        if Some(other) == moving {
            continue;
        }
        let Some(view) = auth.units().get(other) else {
            continue;
        };
        if view.core.owner != owner {
            return Err("tile occupied");
        }
        if class != StackClass::Air && stack_class_on(auth, rules, view.core.kind, tile) == class {
            return Err("one unit of a kind per tile");
        }
    }
    Ok(())
}
