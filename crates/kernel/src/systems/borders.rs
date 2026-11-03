//! Culture of a city: its stock grows the borders (classic/docs/classic/rules.md;
//! the cost of a level is the module's, `ModuleSystems::border_threshold`).
//! Each turn the city adds its culture to its stock; when the stock covers
//! the threshold of its culture level, the threshold is spent, the level
//! grows by one and the best free tile in reach joins the city — at most
//! one level per turn, and the level grows even when no tile is free
//! (rules.md). Journal: TileChange::OwnerChanged.

use crate::effects::city::CityModifiers;
use crate::ids::{CityHandle, PlayerId, TileId};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::systems::module::ModuleSystems;
use crate::transaction::transaction::WorldTransaction;
use crate::world::city::CityWrite;

impl<'a, M: RulesModule> WorldTransaction<'a, M> {
    /// Add `culture` to the city's stock, then grow the border once if the
    /// stock covers the threshold. None: the city is gone.
    pub(crate) fn expand_borders(
        &mut self,
        owner: PlayerId,
        city: CityHandle,
        tile: TileId,
        culture: u32,
        rules: &CompiledRuleset<M>,
        mods: &CityModifiers<M>,
    ) -> Option<()> {
        if let CityWrite::TargetGone = self.world.cities.add_culture(city, culture) {
            return None;
        }
        // The level is the tiles the city owns beyond its first ring, bought
        // ones included (Unciv: CityExpansionManager.tilesClaimed).
        let claimed = crate::world::territory::tiles_claimed(self.world, city);
        let core = self.world.cities.get(city)?;
        let need = M::Systems::border_threshold(claimed, mods);
        if core.culture < need {
            return Some(());
        }
        let candidate = crate::world::territory::border_candidate(self.world, rules, owner, city);
        let dims = self.world.map.dimensions;
        let ring = match candidate {
            Some(candidate) => match (dims.to_coord(tile), dims.to_coord(candidate)) {
                (Some(cc), Some(tc)) => Some(cc.distance(tc) as u8),
                _ => None,
            },
            None => None,
        };
        // Spending the threshold, the level and the radius: one mutation.
        if let CityWrite::TargetGone = self.world.cities.raise_culture_level(city, need, ring) {
            return None;
        }
        if let (Some(candidate), Some(_)) = (candidate, ring) {
            crate::transaction::journal::claim_tile(self.world, &mut self.journal, city, candidate);
        }
        Some(())
    }
}
