//! An order the rules can no longer complete leaves the queue by itself, with a
//! record, and costs nothing: a wonder another city finished, a duplicate, a
//! national wonder of the same player. The city's production stock goes on to the
//! next order. Purchases and orders of a wonder that stands are refused.

use super::{Order, TestModule};
use crate::command::{CommandError, KernelCommand, Refusal};
use crate::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TileId};
use crate::rules::compile::compile_rules;
use crate::rules::content::BuildingKind;
use crate::rules::source::{KeyCount, SourceBuilding, SourceResource, SourceRules, StableContentKey};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{CityChange, OrderRemoval};
use crate::world::state::{GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
// The compiler orders buildings by key.
const NATIONAL: BuildingKindId = BuildingKindId(0);
const PLAIN: BuildingKindId = BuildingKindId(1);
const WONDER: BuildingKindId = BuildingKindId(2);
/// Consumes a resource the test empire never has: the test module vetoes it.
const VETOED: BuildingKindId = BuildingKindId(3);
/// Like `VETOED`, but needs two of the resource: the test module explains it in `hurry_problem`.
const VETOED_WITH_REASON: BuildingKindId = BuildingKindId(4);
/// Needs `PLAIN` in its city.
const NEEDS_PLAIN: BuildingKindId = BuildingKindId(5);
const WONDER_COST: u32 = 10;
const PLAIN_COST: u32 = 5;

fn building(key: &str, cost: u32, wonder: bool, kind: BuildingKind) -> SourceBuilding<TestModule> {
    SourceBuilding { id: Some(StableContentKey::new(key)), cost, wonder, kind, ..Default::default() }
}

fn rules() -> CompiledRuleset<TestModule> {
    let mut source = SourceRules::default();
    source.buildings = vec![
        building("test:building/wonder", WONDER_COST, true, BuildingKind::WorldWonder),
        building("test:building/plain", PLAIN_COST, false, BuildingKind::Regular),
        building("test:building/national", WONDER_COST, false, BuildingKind::NationalWonder),
        SourceBuilding {
            consumes_resources: vec![KeyCount { key: StableContentKey::new("test:resource/ore"), count: 1 }],
            ..building("test:building/zz-vetoed", PLAIN_COST, false, BuildingKind::Regular)
        },
        SourceBuilding {
            consumes_resources: vec![KeyCount { key: StableContentKey::new("test:resource/ore"), count: 2 }],
            ..building("test:building/zz-vetoed-with-reason", PLAIN_COST, false, BuildingKind::Regular)
        },
        SourceBuilding {
            requires_buildings: vec![StableContentKey::new("test:building/plain")],
            ..building("test:building/zzz-needs-plain", PLAIN_COST, false, BuildingKind::Regular)
        },
    ];
    source.resources = vec![SourceResource { id: Some(StableContentKey::new("test:resource/ore")), ..Default::default() }];
    compile_rules(source).expect("the rules compile")
}

struct World {
    rules: CompiledRuleset<TestModule>,
    game: GameState<TestModule>,
    /// P0's cities.
    a: CityHandle,
    a2: CityHandle,
    /// P1's cities.
    b: CityHandle,
    c: CityHandle,
}

fn world() -> World {
    let rules = rules();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 16, height: 12, rng_seed: 7 });
    let a = game.debug_found_city(&rules, P0, TileId(17));
    let a2 = game.debug_found_city(&rules, P0, TileId(24));
    let b = game.debug_found_city(&rules, P1, TileId(100));
    let c = game.debug_found_city(&rules, P1, TileId(107));
    World { rules, game, a, a2, b, c }
}

impl World {
    fn queue(&mut self, city: CityHandle, items: &[BuildingKindId], stock: u32) {
        let core = self.game.world_mut().cities.get_mut(city).expect("a live city");
        core.queue = items.iter().map(|b| ProductionItem::Building(*b)).collect();
        core.production = stock;
    }
    fn stock(&self, city: CityHandle) -> u32 {
        self.game.view().auth.cities().get(city).expect("a live city").production
    }
    fn queued(&self, city: CityHandle) -> Vec<ProductionItem> {
        self.game.view().auth.cities().get(city).expect("a live city").queue.clone()
    }
    fn has(&self, city: CityHandle, b: BuildingKindId) -> bool {
        self.game.view().auth.cities().get(city).expect("a live city").buildings.contains(&b)
    }
    fn treasury(&self, p: PlayerId) -> u32 {
        self.game.view().auth.players().treasury_of(p)
    }
    fn end_turn(&mut self, p: PlayerId) -> Vec<CityChange> {
        let result = self.game.execute(&self.rules, Order::Kernel(KernelCommand::EndTurn { player: p })).expect("the turn ends");
        result.changes().city_changes.clone()
    }
}

fn removals(changes: &[CityChange]) -> Vec<(CityHandle, PlayerId, ProductionItem, OrderRemoval)> {
    changes
        .iter()
        .filter_map(|c| match c {
            CityChange::OrderRemoved { city, owner, item, reason } => Some((*city, *owner, *item, *reason)),
            _ => None,
        })
        .collect()
}

fn item(b: BuildingKindId) -> ProductionItem {
    ProductionItem::Building(b)
}

/// W1, W5, W2: the wonder finished in one city takes the order out of the queues of
/// other players' cities at any position; the stock stays and builds the next order.
#[test]
fn a_finished_wonder_takes_its_order_out_of_every_other_queue() {
    let mut w = world();
    w.queue(w.a, &[WONDER], WONDER_COST);
    w.queue(w.b, &[WONDER, PLAIN], 7);
    w.queue(w.c, &[PLAIN, WONDER], 3);

    let changes = w.end_turn(P0);
    assert!(w.has(w.a, WONDER), "the wonder stands in the winner");
    assert_eq!(w.stock(w.a), 0, "the winner paid its cost");
    assert_eq!(w.queued(w.b), vec![item(PLAIN)], "removed from the head");
    assert_eq!(w.queued(w.c), vec![item(PLAIN)], "removed from the second place");
    assert_eq!((w.stock(w.b), w.stock(w.c)), (7, 3), "the removal leaves the stock alone");
    assert_eq!(
        removals(&changes),
        vec![
            (w.b, P1, item(WONDER), OrderRemoval::WonderBuilt),
            (w.c, P1, item(WONDER), OrderRemoval::WonderBuilt),
        ],
        "one record each: city, owner, item, reason",
    );
    assert!(changes.iter().any(|c| matches!(c, CityChange::QueueChanged { city } if *city == w.b)), "a new head is journaled");

    // W2: the stock of the loser goes into the next order.
    w.end_turn(P1);
    assert!(w.has(w.b, PLAIN), "7 of stock built the plain building (cost 5)");
    assert_eq!(w.stock(w.b), 7 - PLAIN_COST);
    assert!(!w.has(w.b, WONDER));
    assert_eq!(w.stock(w.c), 3, "not enough for the next order yet: nothing was spent");
    w.game.validate(&w.rules).expect("a valid world");
}

/// W3: two cities of one player reach the same wonder in the same round (the second order
/// bypassed validation): the second spends nothing and loses the order.
#[test]
fn two_cities_reaching_one_wonder_in_a_round_spend_once() {
    let mut w = world();
    w.queue(w.a, &[WONDER, PLAIN], WONDER_COST);
    w.queue(w.a2, &[WONDER], WONDER_COST + 2);
    let changes = w.end_turn(P0);
    let built = [w.a, w.a2].into_iter().filter(|c| w.has(*c, WONDER)).count();
    assert_eq!(built, 1, "one wonder in the world");
    assert!(w.has(w.a, WONDER), "the first city in slot order wins");
    assert_eq!(w.stock(w.a2), WONDER_COST + 2, "the loser spent nothing");
    assert!(w.queued(w.a2).is_empty());
    assert_eq!(removals(&changes), vec![(w.a2, P0, item(WONDER), OrderRemoval::WonderBuilt)]);
}

/// W3: an order that is stale in the queue (a save from before the rule, a world edited
/// by the harness) is dropped at the drain without spending production.
#[test]
fn a_stale_order_for_a_standing_wonder_costs_nothing() {
    let mut w = world();
    w.game.debug_add_building(&w.rules, w.b, WONDER);
    w.queue(w.a, &[WONDER, PLAIN], WONDER_COST + PLAIN_COST);
    let changes = w.end_turn(P0);
    assert!(!w.has(w.a, WONDER), "no second wonder");
    assert!(w.has(w.a, PLAIN), "the stock went on to the next order");
    assert_eq!(w.stock(w.a), WONDER_COST, "only the plain building was paid for");
    assert_eq!(removals(&changes), vec![(w.a, P0, item(WONDER), OrderRemoval::WonderBuilt)]);
}

/// W3, class: a duplicate of a building the city has is removed, not paid for.
#[test]
fn a_duplicate_order_is_removed_without_spending() {
    let mut w = world();
    w.game.debug_add_building(&w.rules, w.a, PLAIN);
    w.queue(w.a, &[PLAIN], PLAIN_COST);
    let changes = w.end_turn(P0);
    assert_eq!(w.stock(w.a), PLAIN_COST);
    assert!(w.queued(w.a).is_empty());
    assert_eq!(removals(&changes), vec![(w.a, P0, item(PLAIN), OrderRemoval::AlreadyBuilt)]);
}

/// W6, class: a national wonder is one per player; finishing it removes the order from the
/// player's other cities (a conquered city can carry one) and leaves other players alone.
#[test]
fn a_finished_national_wonder_leaves_the_orders_of_other_players_alone() {
    let mut w = world();
    w.queue(w.a, &[NATIONAL], WONDER_COST);
    w.queue(w.a2, &[NATIONAL], 4);
    w.queue(w.b, &[NATIONAL], 4);
    let changes = w.end_turn(P0);
    assert!(w.has(w.a, NATIONAL));
    assert!(w.queued(w.a2).is_empty(), "the same player's other city lost the order");
    assert_eq!(w.queued(w.b), vec![item(NATIONAL)], "another player's national wonder is its own");
    assert_eq!(w.stock(w.a2), 4);
    assert_eq!(removals(&changes), vec![(w.a2, P0, item(NATIONAL), OrderRemoval::NationalWonderBuilt)]);
}

/// W4: a wonder that stands anywhere is refused when queued, and a purchase of a stale
/// head is refused before any gold is spent.
#[test]
fn a_standing_wonder_can_be_neither_queued_nor_bought() {
    let mut w = world();
    w.game.debug_add_building(&w.rules, w.a, WONDER);
    w.end_turn(P0);
    let queue = Order::Kernel(KernelCommand::EnqueueProduction { player: P1, city: w.b, item: item(WONDER) });
    assert!(matches!(w.game.execute(&w.rules, queue), Err(CommandError::Refused(Refusal::WonderBuilt))));

    w.queue(w.b, &[WONDER], 0);
    w.game.debug_earn(P1, 10_000);
    let gold = w.treasury(P1);
    let hurry = Order::Kernel(KernelCommand::HurryProduction { player: P1, city: w.b });
    assert!(matches!(w.game.execute(&w.rules, hurry), Err(CommandError::Refused(Refusal::WonderBuilt))));
    assert_eq!(w.treasury(P1), gold, "no gold for nothing");
    assert_eq!(w.stock(w.b), 0);
    assert_eq!(w.queued(w.b), vec![item(WONDER)], "a refused command changes nothing");
}

/// Control for the test above: the same purchase of a plain building is taken.
#[test]
fn a_plain_building_is_bought_with_gold() {
    let mut w = world();
    w.end_turn(P0);
    w.queue(w.b, &[PLAIN], 0);
    w.game.debug_earn(P1, 10_000);
    let gold = w.treasury(P1);
    let hurry = Order::Kernel(KernelCommand::HurryProduction { player: P1, city: w.b });
    w.game.execute(&w.rules, hurry).expect("bought");
    assert!(w.has(w.b, PLAIN));
    assert!(w.treasury(P1) < gold);
}

/// W6, `Blocked`: the module's veto pops the order with a record at the drain; the stock is kept
/// and builds the next order.
#[test]
fn a_vetoed_order_is_popped_with_a_record_and_the_stock_stays() {
    let mut w = world();
    w.queue(w.a, &[VETOED, PLAIN], 7);
    let changes = w.end_turn(P0);
    assert_eq!(removals(&changes), vec![(w.a, P0, item(VETOED), OrderRemoval::Blocked)]);
    assert!(!w.has(w.a, VETOED));
    assert!(w.has(w.a, PLAIN), "the stock went to the next order");
    assert_eq!(w.stock(w.a), 7 - PLAIN_COST);
}

/// A vetoed head cannot be hurried: refused with the treasury, stock and queue unchanged.
#[test]
fn a_vetoed_head_cannot_be_bought() {
    let mut w = world();
    w.queue(w.a, &[VETOED], 2);
    w.game.debug_earn(P0, 10_000);
    let gold = w.treasury(P0);
    let hurry = Order::Kernel(KernelCommand::HurryProduction { player: P0, city: w.a });
    assert!(matches!(w.game.execute(&w.rules, hurry), Err(CommandError::Refused(Refusal::NotProducible))));
    assert_eq!(w.treasury(P0), gold);
    assert_eq!(w.stock(w.a), 2);
    assert_eq!(w.queued(w.a), vec![item(VETOED)]);
}

/// The module's own reason for refusing a purchase comes before the generic refusal of a lost
/// order, so the player learns why; gold is not charged either way.
#[test]
fn a_vetoed_head_with_a_module_reason_is_refused_with_that_reason() {
    let mut w = world();
    w.queue(w.a, &[VETOED_WITH_REASON], 2);
    w.game.debug_earn(P0, 10_000);
    let gold = w.treasury(P0);
    let hurry = Order::Kernel(KernelCommand::HurryProduction { player: P0, city: w.a });
    match w.game.execute(&w.rules, hurry) {
        Err(CommandError::RuleViolation(why)) => assert_eq!(why, "needs more resources"),
        other => panic!("the module's reason was expected, got {other:?}"),
    }
    assert_eq!(w.treasury(P0), gold);
    assert_eq!(w.stock(w.a), 2);
    assert_eq!(w.queued(w.a), vec![item(VETOED_WITH_REASON)]);
}

/// Another arrival of a wonder (the harness here; a module's grant) does not sweep at that
/// moment: the kernel has no single entry for a building entering a city. The fallback holds:
/// hurry is refused, nothing is spent, and the drain removes the order with a record.
#[test]
fn a_granted_wonder_is_refused_for_hurry_and_cleaned_at_the_drain() {
    let mut w = world();
    w.queue(w.a, &[WONDER, PLAIN], 12);
    w.game.debug_add_building(&w.rules, w.b, WONDER);
    assert_eq!(w.queued(w.a).len(), 2, "no sweep at the grant");
    w.game.debug_earn(P0, 10_000);
    let gold = w.treasury(P0);
    let hurry = Order::Kernel(KernelCommand::HurryProduction { player: P0, city: w.a });
    assert!(matches!(w.game.execute(&w.rules, hurry), Err(CommandError::Refused(Refusal::WonderBuilt))));
    assert_eq!((w.treasury(P0), w.stock(w.a)), (gold, 12));
    let changes = w.end_turn(P0);
    assert_eq!(removals(&changes), vec![(w.a, P0, item(WONDER), OrderRemoval::WonderBuilt)]);
    assert_eq!(w.stock(w.a), 12 - PLAIN_COST);
}

/// A building the order needs is gone (sold, lost): the purchase is refused with the reason and
/// nothing is spent; the drain takes the order out with a record, and the stock goes on.
#[test]
fn an_order_whose_required_building_is_gone_is_refused_for_hurry_and_cleaned_at_the_drain() {
    let mut w = world();
    w.game.debug_add_building(&w.rules, w.a, PLAIN);
    w.queue(w.a, &[NEEDS_PLAIN, PLAIN], 12);
    w.game.debug_remove_building(w.a, PLAIN);
    w.game.debug_earn(P0, 10_000);
    let gold = w.treasury(P0);
    let hurry = Order::Kernel(KernelCommand::HurryProduction { player: P0, city: w.a });
    assert!(matches!(w.game.execute(&w.rules, hurry), Err(CommandError::Refused(Refusal::NeedsBuilding))));
    assert_eq!((w.treasury(P0), w.stock(w.a)), (gold, 12));
    let changes = w.end_turn(P0);
    assert_eq!(removals(&changes), vec![(w.a, P0, item(NEEDS_PLAIN), OrderRemoval::NeedsBuilding)]);
    assert!(!w.has(w.a, NEEDS_PLAIN));
}

/// The sweep takes the order out at any position, once per order, and only the orders that
/// lost a requirement; the stock is untouched.
#[test]
fn the_sweep_takes_only_the_unbuildable_orders_out_of_any_position() {
    let mut w = world();
    w.game.debug_add_building(&w.rules, w.a, PLAIN);
    w.queue(w.a, &[WONDER, NEEDS_PLAIN], 3);
    w.game.debug_remove_building(w.a, PLAIN);
    let owner = P0;
    let rules = &w.rules;
    let mut tx = crate::transaction::transaction::WorldTransaction::new(w.game.world_mut());
    tx.sweep_unbuildable_orders(owner, rules);
    let removed = removals(&tx.journal.city_changes);
    assert_eq!(removed, vec![(w.a, P0, item(NEEDS_PLAIN), OrderRemoval::NeedsBuilding)]);
    assert_eq!(w.queued(w.a), vec![item(WONDER)]);
    assert_eq!(w.stock(w.a), 3);
}
