//! Simulation driver (platform/structure.md §37, PLAN M6 stage C): runs the AI
//! players' turns in slices of wall time so the host stays responsive.
//!
//! The module's AI players (`RulesModule::Ai`, `kernel::ai::AiPlayers`)
//! do one bounded step at a time and never read a clock; this driver owns the wall-time budget: it
//! steps until the frame slice is spent and stops between steps, which
//! only delays the AI and never changes a decision. The render projection
//! is refreshed at presentation boundaries — when an AI player ends its
//! turn — not after every command.
//!
//! Human input during the AI turns: the host blocks gameplay commands
//! while `busy` (the camera keeps moving); `end_human_turn` starts the AI
//! turns, `tick` advances them, `round` runs them to the end at once
//! (tests, headless runs).
//!
//! Where the AI steps run is the host's choice. By default `tick` steps the AI on the
//! caller's thread in slices (the only way in the browser build without threads). A host
//! that has a worker (`offload`: a thread on native targets, `ai_round::ThreadWorker`)
//! lets the worker play the whole round on a copy of the game: the first `tick` hands the
//! game over as save bytes, later ticks poll for the answer, and the answer's commands
//! then go through `GameState::execute` on the real game in slices, checked against the
//! worker's hash. The commands, journal and hashes are those of the sliced path.

use std::time::Duration;
use web_time::Instant;

use crate::ai_round::{read_memory, RoundReply, RoundRequest, RoundWorker};

use kernel::agent::{AgentSlot, AiAgent, SeatTable};
use kernel::ai::{AiPlayers, DriverPhase, StepOutcome};
use kernel::command::CommandError;
use kernel::ids::PlayerId;
use kernel::module::RulesModule;
use kernel::persistence::ai::SavedAiStates;
use kernel::persistence::codec::{CodecError, Reader};
use kernel::persistence::DecodeError;
use kernel::replay::codec::CommandCodec;
use kernel::replay::hash::authoritative_hash;
use kernel::replay::journal::{take_record, JournalHeader, Record};
use kernel::rules::CompiledRuleset;
use kernel::world::state::{AuthWorld, GameState};

use crate::game::GameClient;
use crate::human::{human_seat, HumanSeat};

/// How many human players (P0..human_count are humans, the rest are AI).
#[derive(Debug, Clone, Copy)]
pub struct DriverConfig {
    pub human_players: u8,
}

impl DriverConfig {
    /// Humans are players 0..human_players; the first one is the local
    /// player the desktop shows.
    pub fn first_human(&self) -> PlayerId {
        PlayerId(0)
    }

    fn humans(&self) -> Vec<PlayerId> {
        (0..self.human_players).map(PlayerId).collect()
    }
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self { human_players: 1 }
    }
}

/// A slice this long asks to run to the end: a tick then waits for the worker (tests,
/// headless runs) instead of polling it between frames.
const RUN_TO_END: Duration = Duration::from_secs(1);

/// The worker's commands are played on the real game for at least this long per tick: the
/// frame no longer carries the AI's planning, so a longer slice ends the turn in fewer
/// frames (`set_apply_slice`). Measured on a late game (57 cities, 53 ms frames, shared
/// machine): the control is back after 540-600 ms with 16 ms, 800-950 ms with 3 or 8 ms
/// (the sliced path: 1540 ms); the worst tick grows from 5 to 9 ms of CPU.
const APPLY_SLICE: Duration = Duration::from_millis(16);

/// What one tick left behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickOutcome {
    /// AI turns still running (the slice ran out).
    Busy,
    /// The AI turns of the round just ended: a human acts again.
    RoundDone,
    /// Nothing to do.
    Idle,
}

/// The AI turns of a round on a worker (`SimulationDriver::offload`).
#[derive(Debug)]
struct Offload<M: RulesModule> {
    worker: Box<dyn RoundWorker>,
    /// Reads the next record of the worker's journal against the real game.
    read: ReadRecord<M>,
    /// The AI memory as the last answer left it (None: ask `ai`, which holds it).
    memory: Option<SavedAiStates>,
    stage: Stage,
    /// Commands applied and rejected by the worker's AI since the start.
    counts: (u64, u64),
    /// The job that failed, once: the driver stops and the host reports it.
    failure: Option<String>,
    failed: bool,
    /// The least wall time a tick spends playing the worker's commands on the real game.
    apply_slice: Duration,
    /// Costs of the last round.
    last: OffloadStats,
    job_started: Instant,
}

/// `take_record` of the module (the driver itself needs no `CommandCodec`).
type ReadRecord<M> = fn(&mut Reader<'_>, &AuthWorld<M>) -> Result<Record<<M as RulesModule>::Command>, CodecError>;

#[derive(Debug)]
enum Stage {
    /// No job.
    Idle,
    /// The worker plays the round.
    Running,
    /// The worker answered; its journal is played on the real game in slices.
    Applying { reply: Box<RoundReply>, offset: usize, commands: usize, boundary: usize },
}

/// Costs of the last offloaded round (diagnostics, benches).
#[derive(Debug, Clone, Copy, Default)]
pub struct OffloadStats {
    /// Encoding the game for the worker (on the calling thread).
    pub encode: Duration,
    /// Applying the answer's commands to the real game (on the calling thread, in slices).
    pub apply: Duration,
    /// The worker's answer checked against the real game's hash (on the calling thread).
    pub hash: Duration,
    /// Commands applied from the answer.
    pub commands: usize,
    /// The request's size.
    pub request_bytes: usize,
    /// From handing the game over to the answer.
    pub worker: Duration,
}

#[derive(Debug)]
pub struct SimulationDriver<M: RulesModule> {
    pub config: DriverConfig,
    /// Who plays which seat: a `HumanAgent` for each human, the AI agent for the rest.
    seats: SeatTable<M>,
    ai_slot: AgentSlot,
    /// The UI's side of each human seat.
    humans: Vec<(PlayerId, HumanSeat<M>)>,
    offload: Option<Offload<M>>,
    /// Steps and wall time of the last tick (diagnostics).
    pub last_tick_steps: u32,
    pub last_tick_time: Duration,
}

impl<M: RulesModule> SimulationDriver<M> {
    /// A driver for a game on `rules` at the default difficulty.
    pub fn new(config: DriverConfig, rules: &CompiledRuleset<M>) -> Self {
        Self::with_difficulty(config, rules, rules.default_difficulty())
    }

    /// A driver for a game at `difficulty` (a difficulty key,
    /// `core:handicap/<level>`); the AI players play it as data.
    pub fn with_difficulty(config: DriverConfig, rules: &CompiledRuleset<M>, difficulty: &str) -> Self {
        let mut seats = SeatTable::new();
        let ai_slot = seats.assign_rest(Box::new(AiAgent::<M>::new(M::Ai::for_game(rules, difficulty, config.humans()))));
        let humans = config
            .humans()
            .into_iter()
            .map(|player| {
                let (agent, seat) = human_seat::<M>();
                // The human's applied commands always wait for the client to take them
                // (it keeps them only while it records).
                seat.record_journal();
                seats.assign(&[player], Box::new(agent));
                (player, seat)
            })
            .collect();
        Self {
            config,
            seats,
            ai_slot,
            humans,
            offload: None,
            last_tick_steps: 0,
            last_tick_time: Duration::ZERO,
        }
    }

    pub fn ai(&self) -> &M::Ai {
        self.seats.agent_ref::<AiAgent<M>>(self.ai_slot).expect("the AI agent is in the table").ai()
    }

    fn ai_mut(&mut self) -> &mut M::Ai {
        self.seats.agent_mut_as::<AiAgent<M>>(self.ai_slot).expect("the AI agent is in the table").ai_mut()
    }

    /// The AI's applied commands join the client's journal (presentation).
    pub fn record_journal(&mut self) {
        self.ai_mut().record_journal();
    }

    /// AI turns are running or waiting to run: the host blocks gameplay
    /// input (the game may not be saved either).
    pub fn busy(&self, client: &GameClient<M>) -> bool {
        if let Some(offload) = &self.offload {
            if !matches!(offload.stage, Stage::Idle) {
                return true;
            }
            if offload.failed {
                return false;
            }
        }
        if self.ai().phase() != DriverPhase::Idle {
            return true;
        }
        let auth = client.game.view().auth;
        let active = auth.active_player();
        client.game.winner().is_none() && !self.is_human(active) && auth.players().is_alive(active)
    }

    /// The human ends its turn; the AI turns follow through `tick`. The order goes
    /// through the human's agent and the table of seats like any other command, and
    /// is executed before this returns (the player hears at once when the game
    /// refuses to end the turn).
    pub fn end_human_turn(&mut self, client: &mut GameClient<M>) -> Result<(), CommandError> {
        let active = client.game.view().auth.active_player();
        let Some(seat) = self.humans.iter().find(|(player, _)| *player == active).map(|(_, seat)| seat.clone()) else {
            return Ok(());
        };
        seat.end_turn();
        while seat.has_orders() {
            self.seats.step(&mut client.game, &client.rules);
        }
        client.extend_journal(seat.take_journal());
        seat.take_result().unwrap_or(Ok(()))
    }

    /// Advance the AI turns for about `slice` of wall time (at least one
    /// step). The projection follows every AI player that ends its turn.
    pub fn tick(&mut self, client: &mut GameClient<M>, slice: Duration) -> TickOutcome {
        match &self.offload {
            Some(offload) if offload.failed => TickOutcome::Idle,
            Some(offload) if !matches!(offload.stage, Stage::Idle) || self.ai().phase() == DriverPhase::Idle => {
                self.tick_offloaded(client, slice)
            }
            _ => self.tick_sliced(client, slice),
        }
    }

    fn tick_sliced(&mut self, client: &mut GameClient<M>, slice: Duration) -> TickOutcome {
        if let Some(offload) = self.offload.as_mut() {
            // The memory moves on in `ai` now.
            offload.memory = None;
        }
        let started = Instant::now();
        let mut steps = 0;
        let outcome = loop {
            if steps > 0 && started.elapsed() >= slice {
                break TickOutcome::Busy;
            }
            steps += 1;
            let outcome = self.seats.step(&mut client.game, &client.rules);
            client.extend_journal(self.ai_mut().take_journal());
            match outcome {
                StepOutcome::Worked => {}
                StepOutcome::PlayerDone(_) => client.refresh_projection(),
                StepOutcome::RoundDone => {
                    client.refresh_projection();
                    break TickOutcome::RoundDone;
                }
                StepOutcome::Waiting => break TickOutcome::Idle,
            }
        };
        self.last_tick_steps = steps;
        self.last_tick_time = started.elapsed();
        outcome
    }

    /// Let `worker` play the AI turns of the rounds from now on (see the module docs). A
    /// round already under way on this thread finishes here first.
    pub fn offload(&mut self, worker: Box<dyn RoundWorker>)
    where
        M: CommandCodec,
    {
        self.offload_with(worker, take_record::<M>);
    }

    fn offload_with(&mut self, worker: Box<dyn RoundWorker>, read: ReadRecord<M>) {
        self.offload = Some(Offload {
            worker,
            read,
            memory: None,
            stage: Stage::Idle,
            counts: (0, 0),
            failure: None,
            failed: false,
            apply_slice: APPLY_SLICE,
            last: OffloadStats::default(),
            job_started: Instant::now(),
        });
    }

    /// A worker thread plays the AI turns (native targets). `wake` is called from the
    /// thread when an answer is ready.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn offload_on_thread(&mut self, rules: std::sync::Arc<CompiledRuleset<M>>, wake: crate::ai_round::AiWake)
    where
        M: CommandCodec,
        CompiledRuleset<M>: Send + Sync,
    {
        if self.offload.is_none() {
            self.offload(Box::new(crate::ai_round::ThreadWorker::new(rules, wake)));
        }
    }

    /// The least wall time a tick plays the worker's commands for (a tick's own slice, when
    /// longer). Zero: exactly the tick's slice.
    pub fn set_apply_slice(&mut self, slice: Duration) {
        if let Some(offload) = self.offload.as_mut() {
            offload.apply_slice = slice;
        }
    }

    /// The AI turns go to a worker.
    pub fn offloaded(&self) -> bool {
        self.offload.is_some()
    }

    /// Costs of the last offloaded round.
    pub fn offload_stats(&self) -> Option<OffloadStats> {
        self.offload.as_ref().map(|o| o.last)
    }

    /// Why the AI turns stopped (a worker failure), once. The game then waits on an AI
    /// player: the host tells the player and offers the menu.
    pub fn take_failure(&mut self) -> Option<String> {
        self.offload.as_mut().and_then(|o| o.failure.take())
    }

    /// Stop a running job and take the worker out (the driver plays on the caller's thread).
    fn take_worker(&mut self) -> Option<(Box<dyn RoundWorker>, ReadRecord<M>)> {
        let mut offload = self.offload.take()?;
        offload.worker.cancel();
        Some((offload.worker, offload.read))
    }

    fn ai_to_act(&self, client: &GameClient<M>) -> bool {
        let game = &client.game;
        let auth = game.view().auth;
        let active = auth.active_player();
        !game.is_over() && !self.is_human(active) && auth.players().is_alive(active)
    }

    fn fail(&mut self, why: String) -> TickOutcome {
        eprintln!("AI turns stopped: {why}");
        if let Some(offload) = self.offload.as_mut() {
            offload.failure = Some(why);
            offload.failed = true;
            offload.stage = Stage::Idle;
        }
        TickOutcome::Idle
    }

    /// `tick` with the AI on the worker: hand the game over, poll, then play the answer.
    fn tick_offloaded(&mut self, client: &mut GameClient<M>, slice: Duration) -> TickOutcome {
        let started = Instant::now();
        let to_end = slice >= RUN_TO_END;
        loop {
            let stage = match self.offload.as_ref().map(|o| &o.stage) {
                Some(Stage::Idle) => 0,
                Some(Stage::Running) => 1,
                Some(Stage::Applying { .. }) => 2,
                None => return TickOutcome::Idle,
            };
            match stage {
                0 => {
                    if !self.ai_to_act(client) {
                        return TickOutcome::Idle;
                    }
                    self.ai().configure_game(&mut client.game);
                    let at = Instant::now();
                    let memory = match self.offload.as_mut().and_then(|o| o.memory.take()) {
                        Some(memory) => memory,
                        None => self.ai().save(),
                    };
                    let request = RoundRequest::of(
                        &client.game,
                        &client.rules,
                        memory,
                        self.ai().difficulty(),
                        self.config.humans(),
                    );
                    let offload = self.offload.as_mut().expect("offloaded");
                    offload.last = OffloadStats { encode: at.elapsed(), request_bytes: request.state.len(), ..Default::default() };
                    offload.worker.submit(request);
                    offload.stage = Stage::Running;
                    offload.job_started = Instant::now();
                    if !to_end {
                        return self.tick_done(started, TickOutcome::Busy);
                    }
                }
                1 => {
                    let offload = self.offload.as_mut().expect("offloaded");
                    let wait = if to_end { slice } else { Duration::ZERO };
                    match offload.worker.poll(wait) {
                        None => return self.tick_done(started, TickOutcome::Busy),
                        Some(Err(error)) => return self.fail(error.to_string()),
                        Some(Ok(reply)) => {
                            offload.last.worker = offload.job_started.elapsed();
                            match Self::journal_start(&reply, &client.rules) {
                                Ok(offset) => {
                                    offload.stage = Stage::Applying { reply: Box::new(reply), offset, commands: 0, boundary: 0 };
                                }
                                Err(why) => return self.fail(why),
                            }
                        }
                    }
                }
                _ => return self.apply_answer(client, slice, started),
            }
        }
    }

    /// Where the records of the worker's journal start, after its header is checked
    /// against the real game's rules.
    fn journal_start(reply: &RoundReply, rules: &CompiledRuleset<M>) -> Result<usize, String> {
        let (header, reader) = JournalHeader::read(&reply.journal).map_err(|e| format!("the worker's journal: {e}"))?;
        if header.rules != rules.hash() {
            return Err("the worker's journal was recorded with other rules".to_owned());
        }
        Ok(reply.journal.len() - reader.remaining())
    }

    /// Play the worker's journal on the real game for about `slice`; at the end check the
    /// hash.
    fn apply_answer(&mut self, client: &mut GameClient<M>, slice: Duration, started: Instant) -> TickOutcome {
        let records = self.ai().records_journal();
        let offload = self.offload.as_mut().expect("offloaded");
        let read = offload.read;
        let slice = slice.max(offload.apply_slice);
        let Stage::Applying { reply, offset, commands, boundary } = &mut offload.stage else {
            return TickOutcome::Idle;
        };
        let mut applied = 0;
        let apply_started = Instant::now();
        let mut error = None;
        loop {
            while *boundary < reply.boundaries.len() && reply.boundaries[*boundary] <= *commands {
                client.refresh_projection();
                *boundary += 1;
            }
            if *offset >= reply.journal.len() {
                break;
            }
            if applied > 0 && started.elapsed() >= slice {
                break;
            }
            // The record is read against the real game just before it is played there: the
            // units and cities it names are found by entity id.
            let mut reader = Reader::new(&reply.journal[*offset..], 0);
            let record = match read(&mut reader, client.game.view().auth) {
                Ok(record) => record,
                Err(e) => {
                    error = Some(format!("the worker's journal is damaged at byte {offset} ({e:?})"));
                    break;
                }
            };
            *offset += reader.position();
            let Record::Command(command) = record else {
                // The round's checkpoint: the final hash below covers it.
                continue;
            };
            let result = if records {
                client.submit_quiet(command)
            } else {
                let applied = client.game.execute(&client.rules, command).map(|_| ());
                client.commands += u64::from(applied.is_ok());
                applied
            };
            if let Err(e) = result {
                error = Some(format!("command {commands} of the worker's journal was refused on the real game: {e:?}"));
                break;
            }
            *commands += 1;
            applied += 1;
        }
        offload.last.apply += apply_started.elapsed();
        offload.last.commands = *commands;
        if let Some(error) = error {
            return self.fail(error);
        }
        if *offset < reply.journal.len() {
            return self.tick_done(started, TickOutcome::Busy);
        }
        let at = Instant::now();
        let hash = authoritative_hash(&client.game);
        offload.last.hash = at.elapsed();
        if hash != reply.hash {
            let why = format!("the worker's game and the real game diverged (hash {:016x} against {:016x})", reply.hash, hash);
            return self.fail(why);
        }
        let memory = match read_memory(&reply.memory) {
            Ok(memory) => memory,
            Err(e) => return self.fail(format!("the worker's AI memory does not read ({e:?})")),
        };
        let round_done = !reply.boundaries.is_empty();
        let counts = reply.counts;
        offload.memory = Some(memory);
        offload.counts = (offload.counts.0 + counts.0, offload.counts.1 + counts.1);
        offload.stage = Stage::Idle;
        let outcome = if round_done { TickOutcome::RoundDone } else { TickOutcome::Idle };
        self.tick_done(started, outcome)
    }

    fn tick_done(&mut self, started: Instant, outcome: TickOutcome) -> TickOutcome {
        self.last_tick_steps = 0;
        self.last_tick_time = started.elapsed();
        outcome
    }

    /// Advance the AI turns of a game nobody plays (a spectated match) for about `slice`
    /// of wall time, at least one step. No projection follows: the host refreshes the
    /// views it shows when a round ends. The steps are the same as `tick`'s, so the
    /// game plays out alike whatever the slice.
    pub fn tick_game(&mut self, game: &mut GameState<M>, rules: &CompiledRuleset<M>, slice: Duration) -> TickOutcome {
        let started = Instant::now();
        let mut steps = 0;
        let outcome = loop {
            if steps > 0 && started.elapsed() >= slice {
                break TickOutcome::Busy;
            }
            steps += 1;
            match self.seats.step(game, rules) {
                StepOutcome::Worked | StepOutcome::PlayerDone(_) => {}
                StepOutcome::RoundDone => break TickOutcome::RoundDone,
                StepOutcome::Waiting => break TickOutcome::Idle,
            }
        };
        self.last_tick_steps = steps;
        self.last_tick_time = started.elapsed();
        outcome
    }

    /// Run the AI turns to their end now (tests, headless runs).
    pub fn finish(&mut self, client: &mut GameClient<M>) {
        while self.tick(client, Duration::from_secs(3600)) == TickOutcome::Busy {}
    }

    /// A full round at once: the human's EndTurn, then every AI turn.
    /// Returns (AI commands applied, rejected) of this round.
    pub fn round(&mut self, client: &mut GameClient<M>) -> (usize, usize) {
        let (applied_before, rejected_before) = self.command_counts();
        let _ = self.end_human_turn(client);
        self.finish(client);
        let (applied_after, rejected_after) = self.command_counts();
        let applied = applied_after - applied_before;
        let rejected = rejected_after - rejected_before;
        (applied as usize, rejected as usize)
    }

    /// Commands the AI applied and had rejected since the start.
    pub fn command_counts(&self) -> (u64, u64) {
        let (applied, rejected) = self.ai().command_counts();
        let (worker_applied, worker_rejected) = self.offload.as_ref().map_or((0, 0), |o| o.counts);
        (applied + worker_applied, rejected + worker_rejected)
    }

    /// The AI memory for a save (only between turns: `busy` is false).
    pub fn save_ai(&self) -> SavedAiStates {
        match self.offload.as_ref().and_then(|o| o.memory.as_ref()) {
            Some(memory) => memory.clone(),
            None => self.ai().save(),
        }
    }

    /// The AI memory of a loaded game.
    pub fn load_ai(
        &mut self,
        saved: &SavedAiStates,
        client: &GameClient<M>,
    ) -> Result<(), DecodeError> {
        let tiles = client.game.view().auth.map().tile_count();
        if let Some(offload) = self.offload.as_mut() {
            offload.memory = None;
        }
        self.ai_mut().load(saved, tiles, &client.rules)
    }

    /// Forget the AI memory (a new game).
    pub fn reset(&mut self, rules: &CompiledRuleset<M>) {
        let recording = self.ai().records_journal();
        let worker = self.take_worker();
        *self = Self::new(self.config, rules);
        if recording {
            self.ai_mut().record_journal();
        }
        if let Some((worker, read)) = worker {
            self.offload_with(worker, read);
        }
    }

    /// Forget the AI memory for a new game at `difficulty`.
    pub fn reset_with_difficulty(&mut self, rules: &CompiledRuleset<M>, difficulty: &str) {
        let worker = self.take_worker();
        *self = Self::with_difficulty(self.config, rules, difficulty);
        if let Some((worker, read)) = worker {
            self.offload_with(worker, read);
        }
    }

    /// A new game takes the driver's difficulty and its AI players (rules
    /// read them from the first turn: handicap costs, growth, happiness);
    /// a game that has its settings (a save) keeps them.
    pub fn configure_game(&self, game: &mut GameState<M>) {
        self.ai().configure_game(game);
    }

    /// Content key of the game's difficulty.
    pub fn difficulty(&self) -> &str {
        self.ai().difficulty()
    }

    fn is_human(&self, player: PlayerId) -> bool {
        self.humans.iter().any(|(human, _)| *human == player)
    }
}
