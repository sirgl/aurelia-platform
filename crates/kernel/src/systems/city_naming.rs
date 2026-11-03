//! A player gives one of its cities a name from the list of its nation.
//! The founding picks the first free name itself (`world::city::next_city_name`); this is the
//! player's choice afterwards. Journal: CityChange::Renamed (a name changes no yield).

use crate::command::{CommandError, Refusal};
use crate::ids::{CityHandle, PlayerId};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::CityChange;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::free_city_names;
use crate::world::state::AuthWorld;

/// The free pool name `choice` points to, when `player` owns `city`.
pub(crate) fn check_city_name<'r, M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &'r CompiledRuleset<M>,
    player: PlayerId,
    city: CityHandle,
    choice: u16,
) -> Result<&'r str, CommandError> {
    let core = auth.cities().get(city).ok_or(CommandError::UnknownCity)?;
    if core.owner != player {
        return Err(CommandError::RuleViolation("not your city".into()));
    }
    free_city_names(auth, rules, player)
        .into_iter()
        .find_map(|(at, name)| (at == choice).then_some(name))
        .ok_or(CommandError::Refused(Refusal::NameTaken))
}

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// The commit safety net mirrors validate: the name is looked up again.
    pub(crate) fn choose_city_name(
        &mut self,
        player: PlayerId,
        city: CityHandle,
        choice: u16,
        rules: &CompiledRuleset<M>,
    ) -> Result<(), CommandError> {
        let name = check_city_name(self.world, rules, player, city, choice)?.to_owned();
        self.world.cities.get_mut(city).ok_or(CommandError::UnknownCity)?.name = name;
        self.journal.city_changes.push(CityChange::Renamed { city });
        Ok(())
    }
}
