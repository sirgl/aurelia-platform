//! The kernel's test module: a rules module with no words, no tables, no
//! world state of its own, a state of each unit ([`Marks`]) and three
//! commands: pass, the kernel's orders and a stand-in fight. The kernel's
//! tests run the generic code on it, which shows that the kernel lives
//! without Classic (platform/PLATFORM.md stage 5; `sandbox-rules` of stage 7 is the
//! playable proof).

use crate::ai::{AiPlayers, DriverPhase, StepOutcome};
use crate::derived::yields::YieldTerms;
use crate::derived::DerivedWorld;
use crate::effects::city::CityModifiers;
use crate::empty::{NoVocabulary, NoWord};
use crate::command::{validate_kernel, CommandError, KernelCommand};
use crate::ids::{CityHandle, PlayerId, TileId, UnitHandle, UnitKindId};
use crate::module::{Rejected, RulesModule};
use crate::navigation::cost::StepCost;
use crate::navigation::{MoveProfile, PathScratch};
use crate::persistence::ai::SavedAiStates;
use crate::persistence::decode::DecodeError;
use crate::rules::compile::{RulesError, SourceKeys};
use crate::rules::content::{ContentTable, Yields};
use crate::rules::effects::{EffectSet, YieldKind};
use crate::rules::fingerprint::Fingerprint;
use crate::rules::key_index::{KeyIndex, TableKeys};
use crate::rules::source::SourceRules;
use crate::rules::tables::ModuleTables;
use crate::rules::CompiledRuleset;
use crate::systems::turn::{CityStepInput, CityTurn, EndTurnInput};
use crate::systems::{CityGrowth, KernelEvent, ModuleSystems};
use crate::transaction::journal::{AppliedCommand, ChangeJournal, UnitChange};
use crate::transaction::WorldTransaction;
use crate::turn::resolution::{KernelCityStep, KernelStep, Step, TurnSchedule};
use crate::turn::TurnPhase;
use crate::validation::world::ValidationError;
use crate::world::city::CityCore;
use crate::world::city_economy::Process;
use crate::world::module::{HashPoint, ModuleId, ModuleUnit, ModuleWorld, NewModuleWorld};
use crate::world::state::{AuthWorld, GameState, GameView};
use crate::world::unit::UnitCore;

/// The test module.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TestModule;

/// The test module's (empty) tables, world and yields.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Nothing;

/// The test module's commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Order {
    /// End the turn: changes nothing.
    Pass,
    Kernel(KernelCommand),
    /// The unit takes `damage` hit points in a fight and dies at none left;
    /// a survivor earns a fight (`Marks::fights`).
    Fight { unit: UnitHandle, damage: u16 },
}

/// The test module's state of a unit: a badge every player who sees the
/// unit sees, and the fights it survived, which only its owner knows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    pub badge: u16,
    pub fights: u16,
}

impl Marks {
    /// The most fights a saved unit may have survived.
    pub const MAX_FIGHTS: u16 = 1_000;
}

impl ModuleUnit<TestModule> for Marks {
    type Saved = (u16, u16);
    type Seen = u16;
    const SAVED_SINCE: u32 = 1;

    fn encode(&self) -> (u16, u16) {
        (self.badge, self.fights)
    }
    fn decode(&(badge, fights): &(u16, u16), _: &CompiledRuleset<TestModule>) -> Result<Self, &'static str> {
        if fights > Self::MAX_FIGHTS {
            return Err("fights");
        }
        Ok(Marks { badge, fights })
    }
    fn hash(&self, mix: &mut dyn FnMut(u64)) {
        if *self != Marks::default() {
            mix(0x6d61_726b ^ (u64::from(self.badge) << 16) ^ u64::from(self.fights));
        }
    }
    fn seen(&self) -> u16 {
        self.badge
    }
}

/// The unit takes `damage` in a fight: it dies when no hit points are left,
/// a survivor counts the fight.
fn fight(world: &mut AuthWorld<TestModule>, unit: UnitHandle, damage: u16) -> Result<ChangeJournal<TestModule>, CommandError> {
    let mut tx = WorldTransaction::new(world);
    let core = tx.world.units.get_mut_core(unit).ok_or(CommandError::UnknownUnit)?;
    if damage >= core.hp {
        tx.destroy_unit(unit)?;
        return Ok(tx.journal);
    }
    core.hp -= damage;
    tx.journal.unit_changes.push(UnitChange::Damaged { unit, damage });
    if let Some(marks) = tx.world.units.module_mut(unit) {
        marks.fights += 1;
    }
    Ok(tx.journal)
}

const KERNEL_TURN: &[(TurnPhase, Step<KernelStep, NoWord>)] = &[
    (TurnPhase::Start, Step::Kernel(KernelStep::HealUnits)),
    (TurnPhase::Start, Step::Kernel(KernelStep::RestoreMoves)),
    (TurnPhase::Start, Step::Kernel(KernelStep::CapitalConnections)),
    (TurnPhase::Start, Step::Kernel(KernelStep::TurnStartEffects)),
    (TurnPhase::Resolution, Step::Kernel(KernelStep::Cities)),
    (TurnPhase::Resolution, Step::Kernel(KernelStep::Gold)),
    (TurnPhase::Resolution, Step::Kernel(KernelStep::Science)),
    (TurnPhase::Resolution, Step::Kernel(KernelStep::Research)),
    (TurnPhase::Resolution, Step::Kernel(KernelStep::WorkerJobs)),
    (TurnPhase::End, Step::Kernel(KernelStep::TimedEffects)),
];

const KERNEL_CITY: &[Step<KernelCityStep, NoWord>] = &[
    Step::Kernel(KernelCityStep::Growth),
    Step::Kernel(KernelCityStep::Production),
    Step::Kernel(KernelCityStep::Borders),
];

impl RulesModule for TestModule {
    type World = Nothing;
    type Vocabulary = NoVocabulary;
    type Tables = Nothing;
    type Yields = Nothing;
    type Command = Order;
    type Systems = Nothing;
    type Ai = NoAi;
    type Step = NoWord;
    type CityStep = NoWord;

    const SCHEDULE: TurnSchedule<NoWord, NoWord> = TurnSchedule { end_turn: KERNEL_TURN, city: KERNEL_CITY };

    fn execute(
        world: &mut AuthWorld<Self>,
        derived: &DerivedWorld<Self>,
        rules: &CompiledRuleset<Self>,
        command: Order,
        scratch: &mut PathScratch,
    ) -> Result<ChangeJournal<Self>, Rejected<Self>> {
        let rejected = |error| Rejected { error, partial: ChangeJournal::default() };
        match command {
            Order::Pass => Ok(ChangeJournal::default()),
            Order::Kernel(order) => {
                let resolved = validate_kernel(GameView { auth: &*world, derived }, rules, order, scratch).map_err(rejected)?;
                let mut tx = WorldTransaction::new(world);
                match tx.apply_kernel(&resolved, rules, scratch, derived) {
                    Ok(()) => Ok(tx.journal),
                    Err(error) => Err(Rejected { error, partial: tx.journal }),
                }
            }
            Order::Fight { unit, damage } => fight(world, unit, damage).map_err(rejected),
        }
    }

    fn victor(_world: &AuthWorld<Self>) -> Option<PlayerId> {
        None
    }

    fn is_over(_world: &AuthWorld<Self>) -> bool {
        false
    }

    fn end_turn(_player: PlayerId) -> Order {
        Order::Pass
    }
}

// The test module's commands in a command journal: `Pass` is the tag alone.
crate::command_field!(Order {
    0 => Pass {},
    1 => Kernel(order),
    2 => Fight { unit, damage },
});

impl crate::replay::codec::CommandCodec for TestModule {
    const COMMAND_CODEC_VERSION: u32 = 1;
}

/// The test module's AI players: none, every player is human.
#[derive(Debug)]
pub struct NoAi;

impl AiPlayers<TestModule> for NoAi {
    fn for_game(_: &CompiledRuleset<TestModule>, _: &str, _: Vec<PlayerId>) -> Self {
        NoAi
    }
    fn phase(&self) -> DriverPhase {
        DriverPhase::Idle
    }
    fn is_human(&self, _: PlayerId) -> bool {
        true
    }
    fn set_humans(&mut self, _: Vec<PlayerId>) {}
    fn step(&mut self, _: &mut GameState<TestModule>, _: &CompiledRuleset<TestModule>) -> StepOutcome {
        StepOutcome::Waiting
    }
    fn advance(&mut self, _: &GameState<TestModule>, _: &CompiledRuleset<TestModule>) -> crate::ai::Advance<<TestModule as RulesModule>::Command> {
        crate::ai::Advance::Waiting
    }
    fn executed(&mut self, _: Result<crate::transaction::journal::CommandResult<TestModule>, crate::command::CommandError>) -> StepOutcome {
        StepOutcome::Worked
    }
    fn turns_passed(&mut self) {}
    fn record_journal(&mut self) {}
    fn records_journal(&self) -> bool {
        false
    }
    fn take_journal(&mut self) -> Vec<AppliedCommand<TestModule>> {
        Vec::new()
    }
    fn command_counts(&self) -> (u64, u64) {
        (0, 0)
    }
    fn save(&self) -> SavedAiStates {
        SavedAiStates::default()
    }
    fn load(&mut self, _: &SavedAiStates, _: usize, _: &CompiledRuleset<TestModule>) -> Result<(), DecodeError> {
        Ok(())
    }
    fn configure_game(&self, _: &mut GameState<TestModule>) {}
    fn difficulty(&self) -> &str {
        ""
    }
}

impl ModuleTables<TestModule> for Nothing {
    type Source = ();
    type Rows = crate::rules::rows::NoRows;
    type SourceRoles = ();
    type Roles = ();
    fn merge_source(_: &mut (), _: ()) {}
    fn source_keys(_: &()) -> Vec<(ContentTable, TableKeys)> {
        Vec::new()
    }
    fn merge_roles(_: &mut (), _: ()) {}
    fn compile_roles(_: &(), _: &KeyIndex, _: &CompiledRuleset<TestModule>) -> Result<(), RulesError> {
        Ok(())
    }
    fn compile(
        _: &SourceRules<TestModule>,
        _: &SourceKeys,
        _: &KeyIndex,
        _: &CompiledRuleset<TestModule>,
    ) -> Result<Self, RulesError> {
        Ok(Nothing)
    }
    fn check(_: &CompiledRuleset<TestModule>) -> Result<(), RulesError> {
        Ok(())
    }
    fn attach_effects(
        _: &SourceRules<TestModule>,
        _: &SourceKeys,
        _: &KeyIndex,
        _: &mut CompiledRuleset<TestModule>,
    ) -> Result<(), RulesError> {
        Ok(())
    }
    fn effect_set(&self, source: NoWord) -> Option<&EffectSet<TestModule>> {
        match source {}
    }
    fn effect_sets(&self) -> Vec<(NoWord, &EffectSet<TestModule>)> {
        Vec::new()
    }
    fn hash_after(&self, _: &str, _: &mut Fingerprint) {}
}

impl crate::world::module::ModulePlayer<TestModule> for Nothing {
    type Saved = crate::world::module::KernelPlayerRecord;
    fn encode(&self, kernel: Self::Saved) -> Self::Saved {
        kernel
    }
    fn kernel_part(saved: &Self::Saved) -> Self::Saved {
        *saved
    }
    fn decode(_: &Self::Saved, _: usize, _: &AuthWorld<TestModule>) -> Result<Self, String> {
        Ok(Nothing)
    }
    fn hash(saved: &Self::Saved, mix: &mut dyn FnMut(u64)) {
        saved.hash(mix)
    }
}

impl ModuleWorld<TestModule> for Nothing {
    type Player = Nothing;
    type PlayerKind = crate::world::player::NoKind;
    type City = ();
    type Unit = Marks;
    const ID: ModuleId = ModuleId { name: "test", version: 1 };
    type Saved = ();
    type Change = NoWord;

    fn new(_: NewModuleWorld<'_, TestModule>) -> Self {
        Nothing
    }
    fn hash(&self, _: HashPoint, _: &AuthWorld<TestModule>, _: &mut dyn FnMut(u64)) {}
    fn validate(&self, _: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>) -> Result<(), ValidationError> {
        Ok(())
    }
    fn unit_removed(&mut self, _: crate::ids::EntityId) {}
    fn encode(&self, _: &AuthWorld<TestModule>) {}
    fn decode(_: &(), _: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>) -> Result<Self, DecodeError> {
        Ok(Nothing)
    }
}

impl ModuleSystems<TestModule> for Nothing {
    fn run_step(_: &mut WorldTransaction<'_, TestModule>, step: NoWord, _: &EndTurnInput<'_, TestModule>) {
        match step {}
    }
    fn run_city_step(_: &mut WorldTransaction<'_, TestModule>, step: NoWord, _: &CityStepInput<'_, TestModule>) {
        match step {}
    }
    fn end_round(_: &mut WorldTransaction<'_, TestModule>, _: &CompiledRuleset<TestModule>) {}
    fn on_event(_: &mut WorldTransaction<'_, TestModule>, _: &CompiledRuleset<TestModule>, _: KernelEvent) {}
    fn heal_amount(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: &UnitCore) -> i32 {
        0
    }
    fn city_growth(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: &CityTurn<TestModule>) -> CityGrowth {
        CityGrowth { difference_times100: 0, kept_percent: 0, threshold: u32::MAX }
    }
    fn food_production(_: &CityTurn<TestModule>) -> i32 {
        0
    }
    fn border_threshold(_: u32, _: &CityModifiers<TestModule>) -> u32 {
        u32::MAX
    }
    fn run_process(_: &mut WorldTransaction<'_, TestModule>, _: PlayerId, _: CityHandle, _: Process, _: u32) -> (i32, i32) {
        (0, 0)
    }
    fn queue_item_blocked(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: PlayerId, _: crate::ids::ProductionItem) -> bool {
        false
    }
    fn gold_balance(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: PlayerId, cities_gold: i32) -> i64 {
        i64::from(cities_gold)
    }
    fn treasury_deficit(_: &mut WorldTransaction<'_, TestModule>, _: &CompiledRuleset<TestModule>, _: PlayerId, _: i64) {}
    fn science(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: PlayerId, cities_science: i32) -> i32 {
        cities_science
    }
    fn attacks_left(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: &UnitCore) -> u8 {
        0
    }
    fn city_max_hit_points(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: &CityCore) -> u16 {
        1
    }
    fn founding_problem(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: PlayerId, _: TileId) -> Option<&'static str> {
        None
    }
    fn fortify_problem(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: &UnitCore) -> Option<&'static str> {
        None
    }
    fn production_problem(
        _: &AuthWorld<TestModule>,
        _: &CompiledRuleset<TestModule>,
        _: PlayerId,
        _: &CityCore,
        _: crate::ids::ProductionItem,
    ) -> Result<(), crate::command::CommandError> {
        Ok(())
    }
    fn unit_for_sale(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: PlayerId, _: UnitKindId) -> bool {
        true
    }
    fn hurry_problem(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: PlayerId, _: &CityCore) -> Result<(), crate::command::CommandError> {
        Ok(())
    }
    fn stays_alive(_: &AuthWorld<TestModule>, _: PlayerId) -> bool {
        false
    }
    fn had_city(_: &AuthWorld<TestModule>, _: PlayerId) -> bool {
        false
    }
    /// Every border is open: the kernel asks the module, not diplomacy.
    fn may_enter(_: &AuthWorld<TestModule>, _: PlayerId, _: PlayerId) -> bool {
        true
    }
    /// A step the kernel forbids takes all moves: the kernel's price goes
    /// through the module's.
    fn step_cost(
        _: &AuthWorld<TestModule>,
        _: &CompiledRuleset<TestModule>,
        _: PlayerId,
        _: &MoveProfile<TestModule>,
        _: TileId,
        _: TileId,
        kernel: Option<StepCost>,
    ) -> Option<StepCost> {
        kernel.or(Some(StepCost::AllMoves))
    }
}

impl YieldTerms<TestModule> for Nothing {
    type Empire = ();
    /// Not Classic's: the kernel takes the module's minimum.
    const CITY_MINIMUM: Yields = Yields { food: 3, ..Yields::ZERO };
    fn empire(_: &AuthWorld<TestModule>, _: PlayerId) {}
    fn tile_base(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: &CityCore, _: TileId) -> Yields {
        Yields::default()
    }
    fn tile_final(_: &(), _: &mut Yields) {}
    fn city_base(_: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: &CityCore, _: &CityModifiers<TestModule>) -> Yields {
        Yields::default()
    }
    fn channel_percent(_: &(), _: YieldKind<TestModule>) -> i32 {
        0
    }
    fn city_after(_: &AuthWorld<TestModule>, _: CityHandle) -> Yields {
        Yields::default()
    }
    fn stale_players(_: &ChangeJournal<TestModule>, _: &AuthWorld<TestModule>, _: &CompiledRuleset<TestModule>, _: &mut Vec<PlayerId>) {}
}

#[cfg(test)]
mod cancel_order;

#[cfg(test)]
mod seat_table;

#[cfg(test)]
mod unit_state;

#[cfg(test)]
mod window;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{TerrainId, TileId};
    use crate::persistence::decode::decode;
    use crate::persistence::encode::encode;
    use crate::replay::hash::authoritative_hash;
    use crate::rules::compile::compile_rules;
    use crate::world::state::GameConfig;

    fn rules() -> CompiledRuleset<TestModule> {
        compile_rules(SourceRules::default()).expect("empty rules compile")
    }

    fn game(rules: &CompiledRuleset<TestModule>) -> GameState<TestModule> {
        GameState::new(rules, GameConfig { players: 2, width: 12, height: 10, rng_seed: 7 })
    }

    #[test]
    fn a_game_without_a_classic_word_starts_and_validates() {
        let rules = rules();
        let game = game(&rules);
        game.validate(&rules).expect("valid world");
        assert_eq!(game.view().auth.map().tile_count(), 120);
        assert_eq!(game.winner(), None, "two living players, no victory");
        assert_eq!(TestModule::outcome(game.view().auth), None, "the default outcome is the victor's");
        assert!(!game.is_over());
    }

    #[test]
    fn the_kernel_asks_the_module_for_borders_step_prices_and_the_centre_minimum() {
        let rules = rules();
        let mut game = game(&rules);
        let (p0, p1) = (PlayerId(0), PlayerId(1));
        let centre = TileId(50);
        game.debug_found_city(&rules, p1, centre);
        let auth = game.view().auth;
        assert_eq!(auth.map().owner(centre).flatten(), Some(p1));
        assert!(!auth.diplomacy().at_war(p0, p1) && !auth.diplomacy().grants_entry(p1, p0), "closed by the Classic module's rule");
        assert!(crate::world::territory::may_enter(auth, p0, centre), "open by the module's");
        let near = auth.map().dimensions.neighbors(centre).into_iter().next().expect("a neighbour");
        let price = crate::navigation::cost::step_cost(auth, &rules, p0, near, centre);
        assert_eq!(price, Some(StepCost::AllMoves), "no terrain: the kernel forbids, the module allows");
        let centre_yields = crate::derived::tile_yields::tile_yields_for(auth, &rules, centre, p1);
        assert_eq!((centre_yields.food, centre_yields.production), (3, 0), "the module's minimum");
    }

    #[test]
    fn a_command_goes_through_the_module_and_keeps_the_hash() {
        let rules = rules();
        let mut game = game(&rules);
        let before = authoritative_hash(&game);
        let result = game.execute(&rules, Order::Pass).expect("the test module accepts its command");
        assert!(result.changes().is_empty());
        assert_eq!(authoritative_hash(&game), before);
    }

    #[test]
    fn a_save_loads_back_to_the_same_hash() {
        let rules = rules();
        let mut game = game(&rules);
        game.debug_set_terrain_batch(&rules, &[(TileId(3), TerrainId(1)), (TileId(4), TerrainId(1))]);
        let loaded = decode(&encode(&game, &rules), &rules).expect("own save loads");
        assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
        loaded.validate(&rules).expect("loaded world is valid");
    }
}
