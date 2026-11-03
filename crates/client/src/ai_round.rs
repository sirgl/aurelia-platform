//! The AI turns of one round as a self-contained job (platform/structure.md §37: "moving to a
//! worker later"): a request of bytes in, a command journal and a hash out.
//!
//! The request carries the game exactly as a save does (the kernel's codec: the world
//! and the AI players' memory), the difficulty and who is human. The job decodes a copy
//! of the game, records a command journal on it (platform/docs/saves.md §3: the format a replay
//! reads, header included), steps the AI players of the module (`AiPlayers::step`, the
//! same calls the sliced path makes) until the round's AI turns end, and answers with the
//! journal, the positions where an AI player ended its turn, the memory of the AI players
//! and the authoritative hash of its copy. The host reads the journal record by record
//! against its own game (a command names its units and cities by entity id: the handles
//! of a decoded copy are not the host's), plays each command through
//! `GameState::execute` and checks that its hash is the job's.
//!
//! Everything that crosses the seam is bytes and numbers: no handle, no command and no AI
//! object. Nothing here touches a thread, a clock or the platform: [`run_round`] is a
//! plain function and [`RoundWorker`] is the narrow seam a host plugs a thread
//! ([`ThreadWorker`], native) or a Web Worker (the browser build) into. The job is
//! stateless: everything it needs is in the request, so a worker may be recycled between
//! rounds.

use std::time::Duration;

use kernel::ai::{AiPlayers, StepOutcome};
use kernel::ids::PlayerId;
use kernel::module::RulesModule;
use kernel::persistence::ai::SavedAiStates;
use kernel::persistence::codec::{self, Field, Reader};
use kernel::persistence::decode::decode;
use kernel::persistence::encode::encode;
use kernel::replay::codec::CommandCodec;
use kernel::replay::hash::authoritative_hash;
use kernel::rules::CompiledRuleset;
use kernel::world::state::GameState;

/// One round's AI turns to play.
#[derive(Debug, Clone)]
pub struct RoundRequest {
    /// The game in the save codec (`kernel::persistence::codec`), the AI memory included.
    pub state: Vec<u8>,
    /// The handicap key the AI players play at.
    pub difficulty: String,
    /// The players who play themselves.
    pub humans: Vec<PlayerId>,
}

impl RoundRequest {
    /// The request for the game as it stands, with the AI memory `memory`.
    pub fn of<M: RulesModule>(
        game: &GameState<M>,
        rules: &CompiledRuleset<M>,
        memory: SavedAiStates,
        difficulty: &str,
        humans: Vec<PlayerId>,
    ) -> Self {
        let mut save = encode(game, rules);
        save.ai = memory;
        Self {
            state: codec::to_bytes(&save),
            difficulty: difficulty.to_owned(),
            humans,
        }
    }
}

/// What the AI players did in the round.
#[derive(Debug, Clone)]
pub struct RoundReply {
    /// The command journal of the job's game (`kernel::replay::journal`): its header and
    /// the applied commands in order, with a checkpoint where the round ended.
    pub journal: Vec<u8>,
    /// How many commands had been applied when each AI player ended its turn: the
    /// places where the host refreshes its projection.
    pub boundaries: Vec<usize>,
    /// The authoritative hash of the job's game after the last command.
    pub hash: u64,
    /// The memory of the AI players at the end of the round (`SavedAiStates` in the
    /// codec's fields): the next request's.
    pub memory: Vec<u8>,
    /// Commands applied and rejected by the AI in this job.
    pub counts: (u64, u64),
    /// The AI steps taken.
    pub steps: u32,
}

/// Why a job did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundError {
    /// The request does not read as a game of these rules.
    Request(String),
    /// The host stopped the job.
    Cancelled,
    /// The job panicked (the text is the panic message).
    Panicked(String),
    /// The worker is gone.
    Disconnected,
}

impl std::fmt::Display for RoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(why) => write!(f, "the AI job could not read its game: {why}"),
            Self::Cancelled => f.write_str("the AI job was cancelled"),
            Self::Panicked(message) => write!(f, "the AI job panicked: {message}"),
            Self::Disconnected => f.write_str("the AI worker is gone"),
        }
    }
}

impl std::error::Error for RoundError {}

/// Play the AI turns of the request's round. `cancelled` is asked between steps.
pub fn run_round<M: CommandCodec>(
    rules: &CompiledRuleset<M>,
    request: &RoundRequest,
    cancelled: &dyn Fn() -> bool,
) -> Result<RoundReply, RoundError> {
    let save = codec::from_bytes::<M>(&request.state)
        .map_err(|e| RoundError::Request(format!("{e:?}")))?;
    let mut game = decode(&save, rules).map_err(|e| RoundError::Request(format!("{e:?}")))?;
    let tiles = game.view().auth.map().tile_count();
    let mut ai = M::Ai::for_game(rules, &request.difficulty, request.humans.clone());
    ai.load(&save.ai, tiles, rules)
        .map_err(|e| RoundError::Request(format!("{e:?}")))?;
    let mut journal = game.begin_journal(rules);
    let applied_before = ai.command_counts();
    let mut boundaries = Vec::new();
    let mut steps = 0u32;
    loop {
        if cancelled() {
            return Err(RoundError::Cancelled);
        }
        steps += 1;
        let outcome = ai.step(&mut game, rules);
        match outcome {
            StepOutcome::Worked => {}
            StepOutcome::PlayerDone(_) => {
                boundaries.push((ai.command_counts().0 - applied_before.0) as usize)
            }
            StepOutcome::RoundDone => {
                boundaries.push((ai.command_counts().0 - applied_before.0) as usize);
                break;
            }
            StepOutcome::Waiting => break,
        }
    }
    journal.extend(game.take_journal());
    let counts = ai.command_counts();
    let mut memory = Vec::new();
    ai.save().put(&mut memory);
    Ok(RoundReply {
        journal,
        boundaries,
        hash: authoritative_hash(&game),
        memory,
        counts: (counts.0 - applied_before.0, counts.1 - applied_before.1),
        steps,
    })
}

/// The memory of the AI players as a reply holds it.
pub fn read_memory(bytes: &[u8]) -> Result<SavedAiStates, codec::CodecError> {
    SavedAiStates::take(&mut Reader::new(bytes, 0))
}

/// The seam between the host and wherever the AI turns run. One request at a time.
pub trait RoundWorker: std::fmt::Debug {
    /// Start a job. The previous one must have been answered or cancelled.
    fn submit(&mut self, request: RoundRequest);

    /// The job's answer, if it came within `wait` (zero: do not block). The answer is
    /// handed over once.
    fn poll(&mut self, wait: Duration) -> Option<Result<RoundReply, RoundError>>;

    /// Stop the running job; its answer, if any, is dropped.
    fn cancel(&mut self);
}

/// Called when an answer is ready (from the worker's thread, or the browser's message
/// handler): the host asks its event loop for a repaint.
pub type AiWake = std::sync::Arc<dyn Fn() + Send + Sync>;

#[cfg(not(target_arch = "wasm32"))]
pub use thread::ThreadWorker;

#[cfg(not(target_arch = "wasm32"))]
mod thread {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use kernel::replay::codec::CommandCodec;
    use kernel::rules::CompiledRuleset;

    use super::{run_round, AiWake, RoundError, RoundReply, RoundRequest, RoundWorker};

    /// A native worker: one thread that plays the rounds it is given, one at a time.
    pub struct ThreadWorker {
        requests: Option<Sender<RoundRequest>>,
        answers: Receiver<Result<RoundReply, RoundError>>,
        cancel: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
        running: bool,
    }

    impl std::fmt::Debug for ThreadWorker {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("ThreadWorker")
                .field("running", &self.running)
                .finish_non_exhaustive()
        }
    }

    impl ThreadWorker {
        /// A worker thread over `rules`; `wake` is called after every answer.
        pub fn new<M: CommandCodec>(rules: Arc<CompiledRuleset<M>>, wake: AiWake) -> Self
        where
            CompiledRuleset<M>: Send + Sync,
        {
            let (request_tx, request_rx) = channel::<RoundRequest>();
            let (answer_tx, answers) = channel();
            let cancel = Arc::new(AtomicBool::new(false));
            let flag = cancel.clone();
            let thread = std::thread::Builder::new()
                .name("ai-turns".into())
                .spawn(move || {
                    while let Ok(request) = request_rx.recv() {
                        let cancelled = || flag.load(Ordering::Relaxed);
                        let answer = catch_unwind(AssertUnwindSafe(|| {
                            run_round::<M>(&rules, &request, &cancelled)
                        }))
                        .unwrap_or_else(|payload| {
                            Err(RoundError::Panicked(panic_text(payload.as_ref())))
                        });
                        if answer_tx.send(answer).is_err() {
                            return;
                        }
                        wake();
                    }
                })
                .expect("the AI worker thread starts");
            Self {
                requests: Some(request_tx),
                answers,
                cancel,
                thread: Some(thread),
                running: false,
            }
        }
    }

    impl RoundWorker for ThreadWorker {
        fn submit(&mut self, request: RoundRequest) {
            self.cancel.store(false, Ordering::Relaxed);
            // Answers of a cancelled job that came late are not this job's.
            while self.answers.try_recv().is_ok() {}
            self.running = self
                .requests
                .as_ref()
                .is_some_and(|tx| tx.send(request).is_ok());
        }

        fn poll(&mut self, wait: Duration) -> Option<Result<RoundReply, RoundError>> {
            if !self.running {
                return Some(Err(RoundError::Disconnected));
            }
            let answer = if wait.is_zero() {
                match self.answers.try_recv() {
                    Ok(answer) => answer,
                    Err(TryRecvError::Empty) => return None,
                    Err(TryRecvError::Disconnected) => Err(RoundError::Disconnected),
                }
            } else {
                match self.answers.recv_timeout(wait) {
                    Ok(answer) => answer,
                    Err(RecvTimeoutError::Timeout) => return None,
                    Err(RecvTimeoutError::Disconnected) => Err(RoundError::Disconnected),
                }
            };
            self.running = false;
            Some(answer)
        }

        fn cancel(&mut self) {
            if self.running {
                self.cancel.store(true, Ordering::Relaxed);
                // The job stops between two AI steps; wait for it so the next job starts clean.
                let _ = self.answers.recv();
                self.running = false;
            }
        }
    }

    impl Drop for ThreadWorker {
        /// Quitting or replacing the match during the AI turns: the job stops between two
        /// AI steps and the thread ends.
        fn drop(&mut self) {
            self.cancel.store(true, Ordering::Relaxed);
            self.requests = None;
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
        payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown".to_owned())
    }
}
