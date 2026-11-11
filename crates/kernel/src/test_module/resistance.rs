//! A city in resistance (`CityCore::resistance_turns > 0`) gives nothing: no yield, no step of its
//! turn (no growth, no starvation, nothing built, its stock does not move), and no order that
//! builds or buys. When resistance ends everything returns. The kernel knows no yield by name.

use super::{Order, TestModule};
use crate::command::{CommandError, KernelCommand, Refusal};
use crate::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TileId};
use crate::rules::compile::compile_rules;
use crate::rules::content::BuildingKind;
use crate::rules::source::{SourceBuilding, SourceRules, StableContentKey};
use crate::rules::CompiledRuleset;
use crate::world::state::{GameConfig, GameState};

const P0: PlayerId = PlayerId(0);
const PLAIN: BuildingKindId = BuildingKindId(0);
const PLAIN_COST: u32 = 5;

fn rules() -> CompiledRuleset<TestModule> {
    let mut source = SourceRules::default();
    source.buildings = vec![SourceBuilding {
        id: Some(StableContentKey::new("test:building/plain")),
        cost: PLAIN_COST,
        kind: BuildingKind::Regular,
        ..Default::default()
    }];
    compile_rules(source).expect("the rules compile")
}

struct World {
    rules: CompiledRuleset<TestModule>,
    game: GameState<TestModule>,
    city: CityHandle,
}

fn world(resistance: u16, stock: u32) -> World {
    let rules = rules();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 16, height: 12, rng_seed: 7 });
    let city = game.debug_found_city(&rules, P0, TileId(17));
    game.debug_found_city(&rules, PlayerId(1), TileId(100));
    let core = game.world_mut().cities.get_mut(city).expect("a live city");
    core.queue = vec![ProductionItem::Building(PLAIN)];
    core.production = stock;
    core.resistance_turns = resistance;
    World { rules, game, city }
}

impl World {
    fn end_turn(&mut self) {
        self.game.execute(&self.rules, Order::Kernel(KernelCommand::EndTurn { player: P0 })).expect("the turn ends");
        self.game.execute(&self.rules, Order::Kernel(KernelCommand::EndTurn { player: PlayerId(1) })).expect("the rival's turn ends");
    }
    fn core(&self) -> crate::world::city::CityCore {
        self.game.view().auth.cities().get(self.city).expect("a live city").clone()
    }
    fn order(&mut self, command: KernelCommand) -> Result<(), CommandError> {
        self.game.execute(&self.rules, Order::Kernel(command)).map(|_| ())
    }
}

#[test]
fn a_city_in_resistance_builds_nothing_and_its_stock_stays() {
    let mut w = world(2, 7);
    w.end_turn();
    let core = w.core();
    assert!(core.buildings.is_empty(), "nothing is built");
    assert_eq!(core.production, 7, "the stock does not move");
    assert_eq!(core.queue, vec![ProductionItem::Building(PLAIN)], "the queue stays");
}

#[test]
fn when_resistance_ends_the_city_builds_again() {
    let mut w = world(1, 7);
    w.end_turn();
    assert!(w.core().buildings.is_empty());
    w.game.world_mut().cities.get_mut(w.city).unwrap().resistance_turns = 0;
    w.end_turn();
    let core = w.core();
    assert_eq!(core.buildings, vec![PLAIN], "built as before");
    assert_eq!(core.production, 7 - PLAIN_COST);
}

#[test]
fn a_city_in_resistance_yields_nothing() {
    let mut w = world(3, 0);
    let rates = |w: &World| crate::derived::yields::city_breakdown(w.game.view().auth, &w.rules, &w.core()).rates;
    assert_eq!(rates(&w), crate::rules::content::Yields::default());
    w.game.world_mut().cities.get_mut(w.city).unwrap().resistance_turns = 0;
    assert!(rates(&w).food > 0, "the same city gives food once resistance is over");
}

#[test]
fn nothing_can_be_queued_bought_or_purchased_in_a_city_in_resistance() {
    let mut w = world(3, 2);
    w.game.debug_earn(P0, 10_000);
    let gold = w.game.view().auth.players().treasury_of(P0);
    let refused = Err(CommandError::Refused(Refusal::CityInResistance { turns: 3 }));
    assert_eq!(w.order(KernelCommand::HurryProduction { player: P0, city: w.city }), refused);
    assert_eq!(
        w.order(KernelCommand::EnqueueProduction { player: P0, city: w.city, item: ProductionItem::Building(BuildingKindId(0)) }),
        refused
    );
    assert_eq!(w.order(KernelCommand::PurchaseTile { player: P0, city: w.city, tile: TileId(18) }), refused);
    assert_eq!(w.game.view().auth.players().treasury_of(P0), gold, "no gold is charged");
    assert_eq!(w.core().production, 2);
}

#[test]
fn a_city_without_resistance_is_bought_and_queued_as_before() {
    let mut w = world(0, 2);
    w.game.debug_earn(P0, 10_000);
    w.order(KernelCommand::HurryProduction { player: P0, city: w.city }).expect("bought");
    assert_eq!(w.core().buildings, vec![PLAIN]);
}
