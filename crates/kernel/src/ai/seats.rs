//! Minds by seat (platform/docs/ai-bench.md): one game where some players decide
//! with another mind than the rest. The AI bench seats a candidate version
//! of a module's AI against its baseline this way; the driver, the rules
//! and the saves do not know about it.

use crate::ai::mind::{AiMind, TurnStart};
use crate::ai::plan::{AiSettings, Decision, PlanTools};
use crate::ai::state::AiState;
use crate::ids::{PlayerId, UnitHandle};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::world::state::GameState;

/// Players in `seats` decide with their seat's mind, every other player
/// with `others`.
#[derive(Debug)]
pub struct Seated<P> {
    others: P,
    /// Sorted by player.
    seats: Vec<(PlayerId, P)>,
}

impl<P> Seated<P> {
    pub fn new(others: P, mut seats: Vec<(PlayerId, P)>) -> Self {
        seats.sort_by_key(|(player, _)| *player);
        seats.dedup_by_key(|(player, _)| *player);
        Self { others, seats }
    }

    /// The mind that decides for `player`.
    pub fn mind(&self, player: PlayerId) -> &P {
        match self.seats.binary_search_by_key(&player, |(p, _)| *p) {
            Ok(at) => &self.seats[at].1,
            Err(_) => &self.others,
        }
    }
}

/// A seat mind's brief with the player it belongs to (the strategic
/// decisions read only the brief).
#[derive(Debug)]
pub struct SeatBrief<B> {
    pub player: PlayerId,
    pub brief: B,
}

impl<M: RulesModule, P: AiMind<M>> AiMind<M> for Seated<P> {
    type Brief = SeatBrief<P::Brief>;
    type Sight<'g> = P::Sight<'g>;
    type Near<'g> = P::Near<'g>;

    fn sense<'g>(&self, game: &'g GameState<M>, rules: &'g CompiledRuleset<M>, player: PlayerId) -> Self::Sight<'g> {
        self.mind(player).sense(game, rules, player)
    }

    fn sense_unit<'g>(
        &self,
        game: &'g GameState<M>,
        rules: &'g CompiledRuleset<M>,
        settings: &AiSettings,
        player: PlayerId,
        unit: UnitHandle,
    ) -> Option<Self::Near<'g>> {
        self.mind(player).sense_unit(game, rules, settings, player, unit)
    }

    fn note_humans(&mut self, humans: &[PlayerId]) {
        self.others.note_humans(humans);
        for (_, mind) in &mut self.seats {
            mind.note_humans(humans);
        }
    }

    fn start_turn(
        &self,
        sight: &Self::Sight<'_>,
        rules: &CompiledRuleset<M>,
        player: PlayerId,
        state: &mut AiState,
    ) -> TurnStart<M, Self::Brief> {
        let start = self.mind(player).start_turn(sight, rules, player, state);
        TurnStart {
            brief: SeatBrief { player, brief: start.brief },
            kind: start.kind,
            units: start.units,
            cities: start.cities,
            opening: start.opening,
        }
    }

    fn order_units(&self, brief: &Self::Brief, rules: &CompiledRuleset<M>, order: &mut Vec<(crate::ids::EntityId, UnitHandle)>) {
        self.mind(brief.player).order_units(&brief.brief, rules, order);
    }

    fn picks_next(&self, brief: &Self::Brief) -> bool {
        self.mind(brief.player).picks_next(&brief.brief)
    }

    fn next_unit(
        &self,
        near: &Self::Near<'_>,
        brief: &Self::Brief,
        state: &AiState,
        remaining: &[(crate::ids::EntityId, UnitHandle)],
    ) -> Option<crate::ids::EntityId> {
        self.mind(brief.player).next_unit(near, &brief.brief, state, remaining)
    }

    fn plan_strategic(&self, brief: &Self::Brief, state: &mut AiState, rules: &CompiledRuleset<M>) -> Vec<Decision<M>> {
        self.mind(brief.player).plan_strategic(&brief.brief, state, rules)
    }

    fn decide_unit(
        &self,
        near: &Self::Near<'_>,
        player: PlayerId,
        unit: UnitHandle,
        brief: &Self::Brief,
        state: &mut AiState,
        tools: &mut PlanTools<'_, M>,
    ) -> Option<Decision<M>> {
        self.mind(player).decide_unit(near, player, unit, &brief.brief, state, tools)
    }

    fn permits(&self, game: &GameState<M>, rules: &CompiledRuleset<M>, player: PlayerId, command: &M::Command) -> bool {
        self.mind(player).permits(game, rules, player, command)
    }

    fn plan_end_of_turn(
        &self,
        sight: &Self::Sight<'_>,
        player: PlayerId,
        brief: &Self::Brief,
        state: &AiState,
        tools: &mut PlanTools<'_, M>,
    ) -> Vec<Decision<M>> {
        self.mind(player).plan_end_of_turn(sight, player, &brief.brief, state, tools)
    }
}
