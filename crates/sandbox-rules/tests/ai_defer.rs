//! A mind that defers (`Reason::Deferred`, `driver::DEFERS_PER_UNIT`): the
//! driver moves the unit to the end of the turn's queue once a turn, and a
//! request beyond that ends the unit's turn.

use std::cell::RefCell;
use std::collections::BTreeMap;

use kernel::ai::driver::{AiDriver, DEFERS_PER_UNIT};
use kernel::ai::mind::{AiMind, TurnStart};
use kernel::ai::plan::{AiSettings, Decision, PlanTools};
use kernel::ai::state::AiState;
use kernel::ai::trace::{Actor, Reason};
use kernel::ids::{EntityId, PlayerId, UnitHandle};
use kernel::replay::hash::authoritative_hash;
use sandbox_rules::ai::{Brief, SandboxMind};
use sandbox_rules::game::{new_game, Setup};
use sandbox_rules::{Game, Rules, Sandbox};

const SEED: u64 = 7_2026;
const ROUNDS: u32 = 60;

/// The sandbox mind that asks `asks` deferrals of every unit at each turn's
/// start of its decisions, and logs the units in the order they decide.
#[derive(Debug, Default)]
struct Deferring {
    asks: u32,
    /// (player, round) → per unit the deferrals asked this turn.
    asked: RefCell<BTreeMap<(PlayerId, u32), BTreeMap<EntityId, u32>>>,
    /// (player, round, unit, deferred) per decision.
    log: RefCell<Vec<(PlayerId, u32, EntityId, bool)>>,
}

impl AiMind<Sandbox> for Deferring {
    type Brief = Brief;
    type Sight<'g> = &'g Game;
    type Near<'g> = &'g Game;

    fn sense<'g>(&self, game: &'g Game, rules: &'g Rules, player: PlayerId) -> &'g Game {
        SandboxMind.sense(game, rules, player)
    }

    fn sense_unit<'g>(&self, game: &'g Game, rules: &'g Rules, settings: &AiSettings, player: PlayerId, unit: UnitHandle) -> Option<&'g Game> {
        SandboxMind.sense_unit(game, rules, settings, player, unit)
    }

    fn start_turn(&self, game: &&Game, rules: &Rules, player: PlayerId, state: &mut AiState) -> TurnStart<Sandbox, Brief> {
        SandboxMind.start_turn(game, rules, player, state)
    }

    fn plan_strategic(&self, brief: &Brief, state: &mut AiState, rules: &Rules) -> Vec<Decision<Sandbox>> {
        SandboxMind.plan_strategic(brief, state, rules)
    }

    fn decide_unit(&self, near: &&Game, player: PlayerId, unit: UnitHandle, brief: &Brief, state: &mut AiState, tools: &mut PlanTools<'_, Sandbox>) -> Option<Decision<Sandbox>> {
        let game: &Game = near;
        let id = game.view().auth.units().get(unit)?.core.id;
        let mut asked = self.asked.borrow_mut();
        let count = asked.entry((player, brief.round)).or_default().entry(id).or_default();
        if *count < self.asks {
            *count += 1;
            self.log.borrow_mut().push((player, brief.round, id, true));
            return Some(Decision::refuse(Actor::Unit(id), Reason::Deferred));
        }
        self.log.borrow_mut().push((player, brief.round, id, false));
        SandboxMind.decide_unit(near, player, unit, brief, state, tools)
    }

    fn plan_end_of_turn(&self, game: &&Game, player: PlayerId, brief: &Brief, state: &AiState, tools: &mut PlanTools<'_, Sandbox>) -> Vec<Decision<Sandbox>> {
        SandboxMind.plan_end_of_turn(game, player, brief, state, tools)
    }
}

fn rules() -> Rules {
    sandbox_content::rules().expect("the sandbox pack compiles")
}

fn play<P: AiMind<Sandbox>>(rules: &Rules, mind: P) -> (Game, AiDriver<Sandbox, P>) {
    let mut game = new_game(rules, Setup::standard(SEED));
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut ai = AiDriver::with_mind(settings, Vec::new(), mind);
    ai.configure_game(&mut game);
    while !game.is_over() && game.view().auth.turn().round < ROUNDS {
        ai.run(&mut game, rules, u32::MAX);
    }
    (game, ai)
}

/// Every unit deferred once at its first decision: the first pass sends no
/// command, the second decides the units in the driver's order on the same
/// world, so the game is the plain mind's game.
#[test]
fn deferring_every_unit_once_plays_the_same_game() {
    let rules = rules();
    let (plain, _) = play(&rules, SandboxMind);
    let (deferred, mut ai) = play(&rules, Deferring { asks: DEFERS_PER_UNIT, ..Deferring::default() });
    assert_eq!(authoritative_hash(&deferred), authoritative_hash(&plain), "the same game");
    let log = ai.mind_mut().log.borrow();
    assert!(log.iter().filter(|e| e.3).count() > 100, "units deferred");
    // Per turn: every unit's deferral comes before any unit decides, then the units in the same order.
    let mut turns: BTreeMap<(PlayerId, u32), Vec<(EntityId, bool)>> = BTreeMap::new();
    for &(p, r, id, deferred) in log.iter() {
        turns.entry((p, r)).or_default().push((id, deferred));
    }
    let mut checked = 0;
    for entries in turns.values() {
        let first: Vec<EntityId> = entries.iter().filter(|e| e.1).map(|e| e.0).collect();
        let pass: usize = entries.iter().position(|e| !e.1).unwrap_or(entries.len());
        assert_eq!(pass, first.len(), "the deferrals come first: {entries:?}");
        // The second pass: each unit's decisions in a row, the units in the first pass's order.
        let mut seen: Vec<EntityId> = entries[pass..].iter().map(|e| e.0).collect();
        seen.dedup();
        assert_eq!(seen, first, "the second pass keeps the order");
        checked += usize::from(first.len() >= 2);
    }
    assert!(checked > 10, "{checked} turns of two units or more");
}

/// A mind that keeps asking: the driver grants `DEFERS_PER_UNIT` deferrals a
/// turn per unit and ends the unit's turn at the next request; every turn
/// ends.
#[test]
fn a_deferral_beyond_the_limit_ends_the_units_turn() {
    let rules = rules();
    let (game, mut ai) = play(&rules, Deferring { asks: u32::MAX, ..Deferring::default() });
    assert!(game.view().auth.turn().round >= ROUNDS || game.is_over(), "the game goes on");
    let log = ai.mind_mut().log.borrow();
    let mut asked: BTreeMap<(PlayerId, u32, EntityId), u32> = BTreeMap::new();
    for &(p, r, id, deferred) in log.iter() {
        assert!(deferred, "this mind never decides");
        *asked.entry((p, r, id)).or_default() += 1;
    }
    assert!(!asked.is_empty());
    assert!(asked.values().all(|&n| n == DEFERS_PER_UNIT + 1), "each unit asked once more than granted: {:?}", asked.values().max());
}
