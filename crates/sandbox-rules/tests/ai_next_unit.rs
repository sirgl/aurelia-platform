//! "Who is next" (`AiMind::picks_next`, `AiMind::next_unit`): a mind that
//! picks its turn's next unit is asked before each unit decision, its answer
//! decides next, it may defer a unit `REORDERS_PER_UNIT` times a turn, and a
//! mind that keeps the queue's head plays the plain mind's game.

use std::cell::RefCell;
use std::collections::BTreeMap;

use kernel::ai::driver::{AiDriver, REORDERS_PER_UNIT};
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

/// How the test mind answers "who is next".
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// None: the queue's head.
    #[default]
    Head,
    /// The last of the remaining units on the map.
    Last,
}

/// The sandbox mind that picks the next unit by `answer`, defers each unit
/// `defers` times a turn, and logs its answers and decisions.
#[derive(Debug, Default)]
struct Picking {
    answer: Answer,
    defers: u32,
    /// (player, round) → per unit the deferrals asked this turn.
    asked: RefCell<BTreeMap<(PlayerId, u32), BTreeMap<EntityId, u32>>>,
    /// The "who is next" calls and the decisions, in order.
    log: RefCell<Vec<Event>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    Asked { round: u32, remaining: usize, answer: Option<EntityId> },
    Decided { player: PlayerId, round: u32, unit: EntityId, deferred: bool },
}

impl AiMind<Sandbox> for Picking {
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

    fn picks_next(&self, _brief: &Brief) -> bool {
        true
    }

    fn next_unit(&self, near: &&Game, brief: &Brief, _state: &AiState, remaining: &[(EntityId, UnitHandle)]) -> Option<EntityId> {
        let answer = match self.answer {
            Answer::Head => None,
            // The last unit still on the map (a settler that founded a city is gone).
            Answer::Last => {
                let game: &Game = near;
                remaining.iter().rev().find(|r| game.view().auth.units().get(r.1).is_some()).map(|r| r.0)
            }
        };
        self.log.borrow_mut().push(Event::Asked { round: brief.round, remaining: remaining.len(), answer });
        answer
    }

    fn plan_strategic(&self, brief: &Brief, state: &mut AiState, rules: &Rules) -> Vec<Decision<Sandbox>> {
        SandboxMind.plan_strategic(brief, state, rules)
    }

    fn decide_unit(&self, near: &&Game, player: PlayerId, unit: UnitHandle, brief: &Brief, state: &mut AiState, tools: &mut PlanTools<'_, Sandbox>) -> Option<Decision<Sandbox>> {
        let game: &Game = near;
        let id = game.view().auth.units().get(unit)?.core.id;
        let mut asked = self.asked.borrow_mut();
        let count = asked.entry((player, brief.round)).or_default().entry(id).or_default();
        if *count < self.defers {
            *count += 1;
            self.log.borrow_mut().push(Event::Decided { player, round: brief.round, unit: id, deferred: true });
            return Some(Decision::refuse(Actor::Unit(id), Reason::Deferred));
        }
        self.log.borrow_mut().push(Event::Decided { player, round: brief.round, unit: id, deferred: false });
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

/// A mind asked every time that always keeps the head plays the plain
/// mind's game: the call itself changes nothing.
#[test]
fn keeping_the_head_plays_the_same_game() {
    let rules = rules();
    let (plain, _) = play(&rules, SandboxMind);
    let (picked, mut ai) = play(&rules, Picking::default());
    assert_eq!(authoritative_hash(&picked), authoritative_hash(&plain), "the same game");
    let log = ai.mind_mut().log.borrow();
    let asked = log.iter().filter(|e| matches!(e, Event::Asked { .. })).count();
    assert!(asked > 100, "asked {asked} times");
    assert!(log.iter().all(|e| !matches!(e, Event::Asked { remaining, .. } if *remaining < 2)), "asked only with two units or more left");
}

/// The answer decides next: every decision that follows an answer is the
/// answered unit's.
#[test]
fn the_answered_unit_decides_next() {
    let rules = rules();
    let (_, mut ai) = play(&rules, Picking { answer: Answer::Last, ..Picking::default() });
    let log = ai.mind_mut().log.borrow();
    let mut followed = 0;
    for pair in log.windows(2) {
        if let [Event::Asked { round, answer: Some(unit), .. }, Event::Decided { round: r, unit: u, .. }] = pair {
            assert_eq!((round, unit), (r, u), "the answer decides next");
            followed += 1;
        }
    }
    assert!(followed > 100, "{followed} answers followed");
}

/// A picking mind may defer a unit `REORDERS_PER_UNIT` times a turn; the
/// request after that ends the unit's turn, and the game goes on.
#[test]
fn a_picking_mind_defers_a_unit_at_most_the_reorder_limit() {
    let rules = rules();
    let (game, mut ai) = play(&rules, Picking { defers: u32::MAX, ..Picking::default() });
    assert!(game.view().auth.turn().round >= ROUNDS || game.is_over(), "the game goes on");
    let log = ai.mind_mut().log.borrow();
    let mut asked: BTreeMap<(PlayerId, u32, EntityId), u32> = BTreeMap::new();
    for e in log.iter() {
        if let Event::Decided { player, round, unit, deferred } = *e {
            assert!(deferred, "this mind never decides");
            *asked.entry((player, round, unit)).or_default() += 1;
        }
    }
    assert!(!asked.is_empty());
    assert!(asked.values().all(|&n| n == REORDERS_PER_UNIT + 1), "each unit asked once more than granted: {:?}", asked.values().max());
}
