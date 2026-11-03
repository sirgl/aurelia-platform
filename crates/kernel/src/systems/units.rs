//! Units between their moves: healing at the end of the owner's turn (by
//! the module's amount, `ModuleSystems::heal_amount`) and fortification.
//! Journal: UnitChange::StanceChanged.

use crate::command::{CommandError, Refusal};
use crate::ids::{PlayerId, UnitHandle};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::systems::module::ModuleSystems;
use crate::transaction::journal::UnitChange;
use crate::transaction::WorldTransaction;
use crate::world::unit::{Stance, MAX_FORTIFY_TURNS, UNIT_MAX_HP};

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// Heal the player's units (one list taken at the start of the end
    /// turn) by the module's healing.
    pub(crate) fn heal_units(&mut self, handles: &[UnitHandle], rules: &CompiledRuleset<M>) {
        for h in handles {
            let Some(view) = self.world.units.get(*h) else {
                continue;
            };
            let core = *view.core;
            let dose = M::Systems::heal_amount(self.world, rules, &core);
            let Some(c) = self.world.units.get_mut_core(*h) else {
                continue;
            };
            c.hp = (i32::from(c.hp) + dose).clamp(1, i32::from(UNIT_MAX_HP)) as u16;
        }
    }

    /// The player's turn begins: fortified units gain a turn of
    /// fortification (1 after the order, 2 from the next
    /// turn on).
    pub(crate) fn advance_fortification(&mut self, player: PlayerId) {
        let units = self.world.indexes.units_of(player).to_vec();
        for unit in units {
            let Some(c) = self.world.units.get_mut_core(unit) else {
                continue;
            };
            if let Stance::Fortified { turns } = c.stance {
                c.stance = Stance::Fortified {
                    turns: (turns + 1).min(MAX_FORTIFY_TURNS),
                };
            }
        }
    }

    /// Fortify: the unit takes the stance; its move order ends.
    pub fn fortify(&mut self, unit: UnitHandle) -> Result<(), CommandError> {
        let core = self
            .world
            .units
            .get_mut_core(unit)
            .ok_or(CommandError::UnknownUnit)?;
        if core.stance.is_fortified() {
            return Err(CommandError::Refused(Refusal::AlreadyFortified));
        }
        core.stance = Stance::Fortified { turns: 1 };
        core.goal = None;
        self.journal
            .unit_changes
            .push(UnitChange::StanceChanged { unit });
        Ok(())
    }
}
