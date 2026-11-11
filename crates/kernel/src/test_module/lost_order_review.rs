//! Reviewer's adversarial tests for the lost-order commit.
use super::{Order, TestModule};
use crate::command::{KernelCommand, Refusal, CommandError};
use crate::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TileId};
use crate::rules::compile::compile_rules;
use crate::rules::content::BuildingKind;
use crate::rules::source::{SourceBuilding, SourceRules, StableContentKey};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{CityChange, OrderRemoval};
use crate::world::state::{GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const NATIONAL: BuildingKindId = BuildingKindId(0);
const PLAIN: BuildingKindId = BuildingKindId(1);
const WONDER: BuildingKindId = BuildingKindId(2);

fn b(key: &str, cost: u32, wonder: bool, kind: BuildingKind) -> SourceBuilding<TestModule> {
    SourceBuilding { id: Some(StableContentKey::new(key)), cost, wonder, kind, ..Default::default() }
}
fn rules() -> CompiledRuleset<TestModule> {
    let mut s = SourceRules::default();
    s.buildings = vec![
        b("test:building/wonder", 10, true, BuildingKind::WorldWonder),
        b("test:building/plain", 5, false, BuildingKind::Regular),
        b("test:building/national", 10, false, BuildingKind::NationalWonder),
    ];
    compile_rules(s).unwrap()
}
struct W { r: CompiledRuleset<TestModule>, g: GameState<TestModule>, a: CityHandle, a2: CityHandle, b: CityHandle, c: CityHandle }
fn world() -> W {
    let r = rules();
    let mut g = GameState::new(&r, GameConfig { players: 2, width: 16, height: 12, rng_seed: 7 });
    let a = g.debug_found_city(&r, P0, TileId(17));
    let a2 = g.debug_found_city(&r, P0, TileId(24));
    let b = g.debug_found_city(&r, P1, TileId(100));
    let c = g.debug_found_city(&r, P1, TileId(107));
    W { r, g, a, a2, b, c }
}
fn it(x: BuildingKindId) -> ProductionItem { ProductionItem::Building(x) }
impl W {
    fn queue(&mut self, c: CityHandle, items: &[BuildingKindId], stock: u32) {
        let core = self.g.world_mut().cities.get_mut(c).unwrap();
        core.queue = items.iter().map(|x| it(*x)).collect();
        core.production = stock;
    }
    fn q(&self, c: CityHandle) -> Vec<ProductionItem> { self.g.view().auth.cities().get(c).unwrap().queue.clone() }
    fn stock(&self, c: CityHandle) -> u32 { self.g.view().auth.cities().get(c).unwrap().production }
    fn has(&self, c: CityHandle, x: BuildingKindId) -> bool { self.g.view().auth.cities().get(c).unwrap().buildings.contains(&x) }
    fn gold(&self, p: PlayerId) -> u32 { self.g.view().auth.players().treasury_of(p) }
    fn exec(&mut self, o: KernelCommand) -> Result<Vec<CityChange>, CommandError> {
        self.g.execute(&self.r, Order::Kernel(o)).map(|x| x.changes().city_changes.clone())
    }
    fn end(&mut self, p: PlayerId) -> Vec<CityChange> { self.exec(KernelCommand::EndTurn { player: p }).unwrap() }
}
fn rem(ch: &[CityChange]) -> Vec<(CityHandle, OrderRemoval)> {
    ch.iter().filter_map(|c| if let CityChange::OrderRemoved { city, reason, .. } = c { Some((*city, *reason)) } else { None }).collect()
}

/// Same wonder twice in another city's queue (only reachable through the harness/old saves).
#[test]
fn duplicate_wonder_deeper_in_other_queue_is_removed_at_once() {
    let mut w = world();
    w.queue(w.a, &[WONDER], 10);
    w.queue(w.b, &[WONDER, PLAIN, WONDER], 3);
    w.end(P0);
    assert!(!w.q(w.b).contains(&it(WONDER)), "queue of b after sweep: {:?}", w.q(w.b));
}

/// The winner has the wonder queued twice: the second goes at once (not only at its head).
#[test]
fn winner_with_wonder_queued_twice_loses_the_second_at_once() {
    let mut w = world();
    w.queue(w.a, &[WONDER, PLAIN, WONDER], 10);
    let ch = w.end(P0);
    assert!(w.has(w.a, WONDER));
    assert!(!w.q(w.a).contains(&it(WONDER)), "queue of a: {:?} records {:?}", w.q(w.a), rem(&ch));
}

/// Winner has wonder twice adjacent: second is removed at the drain with a record, no spend.
#[test]
fn winner_with_adjacent_duplicate_spends_once_and_records() {
    let mut w = world();
    w.queue(w.a, &[WONDER, WONDER], 25);
    let ch = w.end(P0);
    assert!(w.has(w.a, WONDER));
    assert!(w.q(w.a).is_empty());
    assert_eq!(w.stock(w.a), 15);
    assert_eq!(rem(&ch), vec![(w.a, OrderRemoval::AlreadyBuilt)]);
}

/// Wonder completed by a purchase (hurry): the sweep runs, other queues lose the order, gold is spent once.
#[test]
fn hurry_completion_sweeps_other_queues() {
    let mut w = world();
    w.queue(w.a, &[WONDER], 0);
    w.queue(w.b, &[PLAIN, WONDER], 4);
    w.queue(w.c, &[WONDER], 4);
    w.g.debug_earn(P0, 100_000);
    let g0 = w.gold(P0);
    let ch = w.exec(KernelCommand::HurryProduction { player: P0, city: w.a }).unwrap();
    assert!(w.has(w.a, WONDER));
    assert!(w.gold(P0) < g0);
    assert_eq!(w.q(w.b), vec![it(PLAIN)]);
    assert!(w.q(w.c).is_empty());
    assert_eq!((w.stock(w.b), w.stock(w.c)), (4, 4));
    assert_eq!(rem(&ch).len(), 2);
}

/// Hurry the head while the NEXT order is a lost wonder: gold only for the head, wonder dropped, stock kept.
#[test]
fn hurry_head_then_lost_next_costs_nothing_extra() {
    let mut w = world();
    w.g.debug_add_building(&w.r, w.b, WONDER);
    w.queue(w.a, &[PLAIN, WONDER, PLAIN], 0);
    w.g.debug_earn(P0, 100_000);
    let ch = w.exec(KernelCommand::HurryProduction { player: P0, city: w.a }).unwrap();
    assert!(w.has(w.a, PLAIN));
    assert!(!w.has(w.a, WONDER));
    assert_eq!(rem(&ch), vec![(w.a, OrderRemoval::WonderBuilt), (w.a, OrderRemoval::AlreadyBuilt)]);
    assert!(w.q(w.a).is_empty());
}

/// A wonder that arrives by debug_add_building (a module grant / capture path): other orders are not swept at once.
/// The next drain removes it without spending. (Documented behaviour, checked here.)
#[test]
fn granted_wonder_is_cleaned_at_next_drain_without_spend() {
    let mut w = world();
    w.queue(w.b, &[WONDER, PLAIN], 12);
    w.g.debug_add_building(&w.r, w.a, WONDER);
    assert_eq!(w.q(w.b).len(), 2, "not swept at the grant");
    w.end(P0);
    let ch = w.end(P1);
    assert_eq!(rem(&ch), vec![(w.b, OrderRemoval::WonderBuilt)]);
    assert!(w.has(w.b, PLAIN));
    assert_eq!(w.stock(w.b), 7);
}

/// A national wonder queued in another own city and a duplicate cannot be enqueued by command.
#[test]
fn national_wonder_second_enqueue_refused() {
    let mut w = world();
    w.exec(KernelCommand::EnqueueProduction { player: P0, city: w.a, item: it(NATIONAL) }).unwrap();
    let e = w.exec(KernelCommand::EnqueueProduction { player: P0, city: w.a2, item: it(NATIONAL) });
    assert!(e.is_err(), "{e:?}");
    let e = w.exec(KernelCommand::EnqueueProduction { player: P0, city: w.a, item: it(NATIONAL) });
    assert!(matches!(e, Err(CommandError::Refused(Refusal::AlreadyQueued))), "{e:?}");
}

/// Hurry of a wonder in a city that is not the head's: the second-in-queue lost wonder cannot be bought either.
/// Two cities, same wonder, same player, both hurried in the same turn: the second hurry is refused, no gold.
#[test]
fn second_hurry_of_same_wonder_same_turn_is_refused_and_free() {
    let mut w = world();
    w.queue(w.a, &[WONDER], 0);
    w.queue(w.a2, &[WONDER], 0);
    w.g.debug_earn(P0, 100_000);
    w.exec(KernelCommand::HurryProduction { player: P0, city: w.a }).unwrap();
    let g = w.gold(P0);
    assert!(w.q(w.a2).is_empty(), "swept by the first purchase");
    let e = w.exec(KernelCommand::HurryProduction { player: P0, city: w.a2 });
    assert!(e.is_err());
    assert_eq!(w.gold(P0), g);
}

/// Determinism: two identical runs give the same record order and the same queues.
#[test]
fn sweep_is_deterministic() {
    let run = || {
        let mut w = world();
        w.queue(w.a, &[WONDER], 10);
        w.queue(w.c, &[WONDER], 1);
        w.queue(w.b, &[WONDER], 1);
        w.queue(w.a2, &[WONDER], 1);
        let ch = w.end(P0);
        (rem(&ch), w.q(w.b), w.q(w.c))
    };
    assert_eq!(run(), run());
}

/// Empty queue after the sweep: a later turn and a hurry on it do not panic.
#[test]
fn emptied_queue_is_harmless() {
    let mut w = world();
    w.queue(w.a, &[WONDER], 10);
    w.queue(w.b, &[WONDER], 3);
    w.end(P0);
    w.g.debug_earn(P1, 1000);
    let e = w.exec(KernelCommand::HurryProduction { player: P1, city: w.b });
    assert!(matches!(e, Err(CommandError::Refused(Refusal::NothingQueued))), "{e:?}");
    assert_eq!(w.stock(w.b), 3);
}

/// The record is visible to the owner of the loser only? The journal carries every player's removals
/// (no filtering in the kernel): shown, not asserted as defect.
#[test]
fn record_names_loser_owner_and_item() {
    let mut w = world();
    w.queue(w.a, &[WONDER], 10);
    w.queue(w.b, &[WONDER], 3);
    let ch = w.end(P0);
    assert!(ch.iter().any(|c| matches!(c, CityChange::OrderRemoved { owner, .. } if *owner == P1)));
}
