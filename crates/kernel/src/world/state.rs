//! GameState: AuthWorld + DerivedWorld. The main write API is execute() (§41).

use crate::module::RulesModule;
use crate::command::CommandError;
use crate::derived::DerivedWorld;
use crate::ids::{PlayerId, TerrainId};
use crate::knowledge::view::PlayerView;
use crate::knowledge::PlayerKnowledge;
use crate::math::MapDimensions;
use crate::replay::codec::CommandCodec;
use crate::replay::journal::{JournalHeader, Recording};
use crate::rng::RngState;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{ChangeJournal, CommandResult};
use crate::turn::TurnState;
use crate::validation::world::validate_world;
use crate::world::city::CityStore;
use crate::world::diplomacy::DiplomacyState;
use crate::world::indexes::WorldIndexes;
use crate::world::map::MapState;
use crate::world::module::{ModuleWorld, NewModuleWorld};
use crate::world::player::PlayerStore;
use crate::world::unit::UnitStore;

#[derive(Debug, Clone, Default)]
pub struct GameConfig {
    pub players: u8,
    pub width: u32,
    pub height: u32,
    /// Seed of all simulation RNG streams (§24). Deterministic, in the save.
    pub rng_seed: u64,
}

impl GameConfig {
}

/// Authoritative truth. The fields are public for the rules module's
/// systems, which mutate them inside the module's transaction; a
/// `&mut AuthWorld` is reachable only through the module's `execute`, the
/// `WorldBuilder` (a world before its first turn) and, behind the `harness`
/// feature, `GameState::world_mut` — that is the boundary.
#[derive(Debug, Clone)]
pub struct AuthWorld<M: RulesModule> {
    pub map: MapState,
    pub players: PlayerStore<M>,
    pub cities: CityStore<M>,
    pub units: UnitStore<M>,
    pub diplomacy: DiplomacyState,
    pub turn: TurnState,
    pub indexes: WorldIndexes,
    pub knowledge: Vec<PlayerKnowledge>,
    /// Timed effects (Duration::Turns) — truth: persisted and hashed.
    pub effects: crate::world::effects::ActiveEffectStore<M>,
    /// RNG is part of the authoritative truth: persisted, included in the hash (§24).
    pub rng: RngState,
    /// Next persistent id for a unit or a city (persisted and hashed).
    pub next_entity: u64,
    /// Difficulty and AI players (saved and hashed with the economy).
    pub settings: crate::world::game_settings::GameSettings,
    /// The rules module's world state (`world::module`): every mechanic
    /// the kernel does not know.
    pub module: M::World,
    /// Results of pure effect functions, valid until the world changes
    /// (`effects::memo`). Not saved, not hashed, empty in a clone.
    pub(crate) memo: crate::effects::memo::EffectMemo<M>,
}

#[derive(Debug, Clone)]
pub struct GameState<M: RulesModule> {
    pub(in crate::world) auth: AuthWorld<M>,
    pub(in crate::world) derived: DerivedWorld<M>,
    /// Reusable scratch for path search in validate.
    /// Single-owner: belongs to GameState, never handed out (§19).
    /// Reuse keeps validate observably pure (the output depends only on the inputs).
    path_scratch: crate::navigation::PathScratch,
    /// The command journal being recorded (`begin_journal`); off by default.
    recording: Recording<M>,
}

/// A copy of a game made for a "what if" question (`GameState::trial`). It reads as the
/// game it was copied from and, unlike a game, may be written to outside commands.
#[derive(Debug, Clone)]
pub struct Trial<M: RulesModule>(GameState<M>);

impl<M: RulesModule> std::ops::Deref for Trial<M> {
    type Target = GameState<M>;
    fn deref(&self) -> &GameState<M> {
        &self.0
    }
}

impl<M: RulesModule> Trial<M> {
    /// The copy as a plain game (a preview's world handed to code that reads games).
    pub fn into_inner(self) -> GameState<M> {
        self.0
    }

    /// The copy's authoritative world, to be changed for the question.
    pub fn world_mut(&mut self) -> &mut AuthWorld<M> {
        self.0.auth.memo.close();
        &mut self.0.auth
    }
}

/// Restricted read view (§40). Do not hand `&GameState` to everyone.
#[derive(Debug, Clone, Copy)]
pub struct GameView<'a, M: RulesModule> {
    pub auth: &'a AuthWorld<M>,
    pub derived: &'a DerivedWorld<M>,
}

impl<M: RulesModule> AuthWorld<M> {
    pub fn map(&self) -> &MapState {
        &self.map
    }
    pub fn units(&self) -> &UnitStore<M> {
        &self.units
    }
    pub fn cities(&self) -> &CityStore<M> {
        &self.cities
    }
    pub fn players(&self) -> &PlayerStore<M> {
        &self.players
    }
    pub fn turn(&self) -> &TurnState {
        &self.turn
    }
    pub fn diplomacy(&self) -> &DiplomacyState {
        &self.diplomacy
    }
    pub fn knowledge(&self) -> &[PlayerKnowledge] {
        &self.knowledge
    }
    pub fn rng(&self) -> &RngState {
        &self.rng
    }
    pub fn indexes(&self) -> &WorldIndexes {
        &self.indexes
    }
    pub fn active_effects(&self) -> &crate::world::effects::ActiveEffectStore<M> {
        &self.effects
    }
    pub fn settings(&self) -> &crate::world::game_settings::GameSettings {
        &self.settings
    }

    /// The memo of effect results (`effects::memo`).
    pub fn memo(&self) -> &crate::effects::memo::EffectMemo<M> {
        &self.memo
    }

    /// The era `owner`'s cities are drawn in (an index into the city kit's styles; 0 the
    /// oldest): the owner's current era, as the Classic city art takes it, never the era of
    /// the newest building. The module's answer (`RulesModule::city_look_era`), else the
    /// latest content era of the owner's technologies.
    pub fn city_era(&self, owner: PlayerId, rules: &crate::rules::CompiledRuleset<M>) -> u8 {
        M::city_look_era(self, rules, owner).unwrap_or_else(|| self.players().nation_era_of(owner, rules))
    }

    /// The rules module's world state.
    pub fn module(&self) -> &M::World {
        &self.module
    }

    /// Whose turn it is. Normally the stored active player; a player with
    /// nothing at all (a harness world populated in any order, or a player
    /// eliminated outside its own turn) never holds the round: the turn is
    /// then the next alive player's. An active player eliminated during its
    /// own turn passes the turn in the command commit.
    pub fn active_player(&self) -> PlayerId {
        if let Some(seat) = self.turn.acting {
            return seat;
        }
        let stored = self.turn.active_player;
        if self.players.is_alive(stored) {
            return stored;
        }
        let next = crate::turn::state::TurnState::next_after(stored, self.players.count(), |p| {
            self.players.is_alive(p)
        });
        match next {
            crate::turn::state::NextTurn::SameRound(p) | crate::turn::state::NextTurn::NewRound(p) => p,
            crate::turn::state::NextTurn::Nobody => stored,
        }
    }

    pub fn next_entity(&self) -> crate::ids::EntityId {
        crate::ids::EntityId(self.next_entity)
    }

    /// A fresh persistent id (units and cities share one counter).
    pub(crate) fn allocate_entity(&mut self) -> crate::ids::EntityId {
        let id = crate::ids::EntityId(self.next_entity);
        self.next_entity += 1;
        id
    }
}

impl<M: RulesModule> GameState<M> {
    pub fn new(rules: &CompiledRuleset<M>, config: GameConfig) -> Self {
        // A module whose turn schedule breaks the kernel's invariants fails
        // its build here.
        const { M::SCHEDULE.validate() };
        let players = config.players.max(1);
        let dims = MapDimensions {
            width: config.width.max(1),
            height: config.height.max(1),
        };
        let tiles = dims.tile_count();
        let mut map = MapState::new(dims, TerrainId(0));
        map.set_terrain_kinds(rules.terrain_kinds);
        let auth = AuthWorld {
            map,
            players: PlayerStore::new(players),
            cities: CityStore::new(),
            units: UnitStore::new(),
            diplomacy: DiplomacyState::new(players as usize),
            turn: TurnState::new(),
            indexes: WorldIndexes::new(),
            knowledge: (0..players)
                .map(|_| PlayerKnowledge::new(tiles, players as usize))
                .collect(),
            effects: Default::default(),
            rng: RngState::new(config.rng_seed),
            next_entity: 0,
            settings: Default::default(),
            module: M::World::new(NewModuleWorld {
                players: players as usize,
                seed: config.rng_seed,
                rules,
            }),
            memo: Default::default(),
        };
        let derived = DerivedWorld::rebuild(&auth, rules);
        Self {
            auth,
            derived,
            path_scratch: crate::navigation::PathScratch::default(),
            recording: Recording::off(),
        }
    }

    pub fn view(&self) -> GameView<'_, M> {
        self.auth.memo.open();
        GameView {
            auth: &self.auth,
            derived: &self.derived,
        }
    }

    pub fn derived(&self) -> &DerivedWorld<M> {
        self.auth.memo.open();
        &self.derived
    }

    /// How effect results are memoised (`effects::memo`); the default is `AURELIA_MEMO` or on.
    pub fn set_memo_mode(&mut self, mode: crate::effects::memo::MemoMode) {
        self.auth.memo.set_mode(mode);
    }

    /// The one external write boundary (§41): the rules module validates
    /// and applies the command; derived state and knowledge follow its
    /// journal.
    pub fn execute(
        &mut self,
        rules: &CompiledRuleset<M>,
        command: M::Command,
    ) -> Result<CommandResult<M>, CommandError> {
        self.auth.memo.close();
        let _span = prof::span("cmd/execute");
        let record = self.recording.command(&command, &self.auth);
        self.apply(rules, command, record)
    }

    /// Open a simultaneous window: the players in `seats` (bit `n`: player
    /// `n`) may order their own units and cities now with `execute_as`, all
    /// at once, until `close_window`. The turn itself does not move: turns
    /// still end one by one through `execute`, in player order. The window
    /// is recorded in the command journal.
    pub fn open_window(&mut self, seats: u64) {
        self.auth.turn.window = seats;
        self.recording.window(seats);
    }

    /// Close the window: only the active player acts again.
    pub fn close_window(&mut self) {
        self.open_window(0);
    }

    /// `execute` for a seat of the open window. The command runs as if it
    /// were `seat`'s turn: every module's «it is your turn» check compares
    /// the command's player with the seat (`AuthWorld::active_player`), so a
    /// command that names another player's unit or city is refused as it is
    /// out of turn today. Ending a turn is refused: it belongs to the
    /// round's resolution.
    pub fn execute_as(
        &mut self,
        seat: PlayerId,
        rules: &CompiledRuleset<M>,
        command: M::Command,
    ) -> Result<CommandResult<M>, CommandError> {
        let open = seat.0 < 64 && self.auth.turn.window & (1u64 << seat.0) != 0;
        if !open {
            return Err(CommandError::RuleViolation("no open window for this player".into()));
        }
        if !self.auth.players.is_alive(seat) {
            return Err(CommandError::UnknownPlayer);
        }
        let record = self.recording.command_as(seat, &command, &self.auth);
        self.auth.turn.acting = Some(seat);
        let result = self.apply(rules, command, record);
        self.auth.turn.acting = None;
        result
    }

    fn apply(
        &mut self,
        rules: &CompiledRuleset<M>,
        command: M::Command,
        record: Option<usize>,
    ) -> Result<CommandResult<M>, CommandError> {
        let round = self.auth.turn.round;
        let executed = {
            let _span = prof::span("cmd/module_execute");
            M::execute(&mut self.auth, &self.derived, rules, command, &mut self.path_scratch)
        };
        match executed {
            Ok(journal) => {
                self.commit_journal(rules, &journal);
                if self.recording.is_on() && self.auth.turn.round != round {
                    let _span = prof::span("cmd/hash_checkpoint");
                    let hash = crate::replay::authoritative_hash(self);
                    self.recording.checkpoint(round, hash);
                }
                Ok(CommandResult::new(journal))
            }
            Err(rejected) => {
                self.recording.discard(record);
                // A rejected command must leave the world as it was. One that
                // changed it anyway is a defect of its system: loud in debug
                // builds, and derived state and knowledge still follow the
                // change, so the world never goes out of step with itself.
                debug_assert!(rejected.partial.is_empty(), "rejected command changed the world: {:?}", rejected.partial);
                if !rejected.partial.is_empty() {
                    self.commit_journal(rules, &rejected.partial);
                }
                Err(rejected.error)
            }
        }
    }

    /// Start a command journal from the world as it is now (just saved):
    /// returns its header; from here every applied command and every round's
    /// hash is recorded until the next `begin_journal` (platform/docs/saves.md §3).
    pub fn begin_journal(&mut self, rules: &CompiledRuleset<M>) -> Vec<u8>
    where
        M: CommandCodec,
    {
        self.recording = Recording::begin();
        JournalHeader::of(self, rules).to_bytes()
    }

    /// The journal's records since the last call (none when not recording).
    pub fn take_journal(&mut self) -> Vec<u8> {
        self.recording.take()
    }

    /// The one post-commit point: derived state applies the journal, then
    /// knowledge syncs. Every mutation (`execute`, the module's harness)
    /// goes through it, so they never go out of step.
    pub fn commit_journal(&mut self, rules: &CompiledRuleset<M>, journal: &ChangeJournal<M>) {
        {
            let _span = prof::span("cmd/derived_apply");
            // The command (or the harness) is over: nobody changes the world
            // while derived state follows it, so effect results are valid.
            self.auth.memo.open();
            self.derived.apply_changes(journal, &self.auth, rules);
            // Knowledge sync writes to the world.
            self.auth.memo.close();
        }
        let _span = prof::span("cmd/knowledge_sync");
        crate::knowledge::sync::sync(&mut self.auth, &self.derived, rules, journal);
    }

    /// Harness only: the authoritative world to mutate outside a command. Every
    /// change must reach `commit_journal` with its journal. A game is built
    /// with `WorldBuilder`, never with this.
    #[cfg(any(test, feature = "harness"))]
    pub fn world_mut(&mut self) -> &mut AuthWorld<M> {
        self.auth.memo.close();
        &mut self.auth
    }

    /// A copy of this game for a question of the form "what if" (a forecast, a
    /// preview): the copy may be changed freely outside commands and is never
    /// played back into the game. Its derived state is NOT updated by such
    /// changes; the question must read authoritative state or recompute.
    pub fn trial(&self) -> Trial<M> {
        Trial(self.clone())
    }

    /// The rules module's own world, for the one module state a player
    /// changes without a command today (Classic's "one more turn" after a
    /// victory). The list of such uses is in platform/docs/world-builder.md; each is
    /// remaining work to become a command.
    pub fn module_mut(&mut self) -> &mut M::World {
        self.auth.memo.close();
        &mut self.auth.module
    }

    /// The map and the other players as `player` knows them (§21).
    pub fn player_view<'a>(&'a self, rules: &'a CompiledRuleset<M>, player: PlayerId) -> PlayerView<'a, M> {
        self.auth.memo.open();
        PlayerView::new(&self.auth, &self.derived, rules, player)
    }

    // ---- Writes outside `execute`. There are two callers and no others: the
    // `WorldBuilder` (a world before its first turn) and the test harness
    // (`world/harness.rs`, behind `cfg(any(test, feature = "harness"))`).

    pub(in crate::world) fn close_memo(&mut self) {
        self.auth.memo.close();
    }

    pub(in crate::world) fn auth_for_builder(&mut self) -> &mut AuthWorld<M> {
        &mut self.auth
    }

    /// The single helper of every write outside `execute`: closes the effect
    /// memo, runs the write (which records what it changed in the journal it
    /// is given) and returns the journal for the caller to commit or, when no
    /// derived state or knowledge reads what was written, to be empty.
    pub(crate) fn write_outside_commands<R>(
        &mut self,
        f: impl FnOnce(&mut AuthWorld<M>, &mut ChangeJournal<M>) -> R,
    ) -> (R, ChangeJournal<M>) {
        self.auth.memo.close();
        let mut journal = ChangeJournal::default();
        let result = f(&mut self.auth, &mut journal);
        (result, journal)
    }

    /// `write_outside_commands`, then the journal reaches `commit_journal`
    /// (derived state, then knowledge) if it holds anything.
    pub(crate) fn construct<R>(
        &mut self,
        rules: &CompiledRuleset<M>,
        f: impl FnOnce(&mut AuthWorld<M>, &mut ChangeJournal<M>) -> R,
    ) -> R {
        let (result, journal) = self.write_outside_commands(f);
        if !journal.is_empty() {
            self.commit_journal(rules, &journal);
        }
        result
    }

    /// Like `construct`, for a write that brings its own journal (a transaction).
    pub(crate) fn construct_with_journal<R>(
        &mut self,
        rules: &CompiledRuleset<M>,
        f: impl FnOnce(&mut AuthWorld<M>) -> (R, ChangeJournal<M>),
    ) -> R {
        self.auth.memo.close();
        let (result, journal) = f(&mut self.auth);
        self.commit_journal(rules, &journal);
        result
    }

    /// Game settings: set once when a game is created, before its first turn
    /// (the builder), or by the AI driver for a game nobody configured.
    #[cfg(any(test, feature = "harness"))]
    pub fn configure_players(&mut self, difficulty: &str, ai: &[PlayerId]) {
        self.configure_players_inner(difficulty, ai);
    }

    #[cfg(not(any(test, feature = "harness")))]
    pub(crate) fn configure_players(&mut self, difficulty: &str, ai: &[PlayerId]) {
        self.configure_players_inner(difficulty, ai);
    }

    /// The game's speed (a speed key; Standard keeps the setting empty):
    /// tests and tools that made a game without the new-game path.
    #[cfg(any(test, feature = "harness"))]
    pub fn set_speed(&mut self, rules: &CompiledRuleset<M>, speed: &str) {
        self.write_outside_commands(|auth, _| crate::world::construction::set_speed(auth, rules, speed));
    }

    fn configure_players_inner(&mut self, difficulty: &str, ai: &[PlayerId]) {
        self.write_outside_commands(|auth, _| crate::world::construction::configure_players(auth, difficulty, ai));
    }

    pub fn validate(&self, rules: &CompiledRuleset<M>) -> Result<(), String> {
        validate_world(self, rules).map_err(|e| format!("{:?}", e))?;
        self.derived.assert_matches_rebuild(&self.auth, rules)
    }

    /// The game is over: someone won and nobody asked for one more turn.
    /// The game is over: the module says so (Classic: someone won and nobody
    /// asked for one more turn).
    pub fn is_over(&self) -> bool {
        M::is_over(&self.auth)
    }

    /// The winner: the module's victor, else the only living major player.
    /// None while the game goes on (or when everybody is dead).
    pub fn winner(&self) -> Option<PlayerId> {
        if let Some(victor) = M::victor(&self.auth) {
            return Some(victor);
        }
        let mut living = None;
        for i in 0..self.auth.players().count() {
            let p = PlayerId(i as u8);
            // City-states and barbarians do not win.
            if self.auth.players().is_alive(p) && self.auth.players().is_major(p) {
                if living.is_some() {
                    return None;
                }
                living = Some(p);
            }
        }
        living
    }

    /// Restore from a verified save schema: derived is rebuilt by the oracle path.
    /// The invariant derived==rebuild holds by construction.
    pub(crate) fn restore(auth: AuthWorld<M>, rules: &CompiledRuleset<M>) -> Self {
        let derived = DerivedWorld::rebuild(&auth, rules);
        Self {
            auth,
            derived,
            path_scratch: crate::navigation::PathScratch::default(),
            recording: Recording::off(),
        }
    }
}
