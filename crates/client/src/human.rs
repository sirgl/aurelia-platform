//! HumanAgent: the human seat as a `kernel::agent::PlayerAgent` (classic/docs/player-agent.md, P3).
//!
//! The UI holds a [`HumanSeat`], the handle that queues what the player orders;
//! the table of seats holds the [`HumanAgent`] that answers `poll` from that
//! queue (`Pending` while it is empty: the player is thinking). What the game
//! made of an order comes back through the same handle. Selection, overlays and
//! the render projection stay in the client; the agent knows only commands.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use kernel::agent::{AgentContext, AgentEvent, AgentPoll, PlayerAgent};
use kernel::command::CommandError;
use kernel::module::RulesModule;
use kernel::transaction::journal::AppliedCommand;

enum Order<C> {
    Command(C),
    EndTurn,
}

struct Shared<M: RulesModule> {
    orders: VecDeque<Order<M::Command>>,
    /// What the game made of the orders executed so far, oldest first.
    results: VecDeque<Result<(), CommandError>>,
    /// Applied commands with their journals, while the client records them.
    journal: Option<Vec<AppliedCommand<M>>>,
}

fn lock<M: RulesModule>(shared: &Mutex<Shared<M>>) -> MutexGuard<'_, Shared<M>> {
    // A poisoned queue is still a queue: the orders in it are plain data.
    shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The UI's side of a human seat.
pub struct HumanSeat<M: RulesModule> {
    shared: Arc<Mutex<Shared<M>>>,
}

impl<M: RulesModule> Clone for HumanSeat<M> {
    fn clone(&self) -> Self {
        Self { shared: self.shared.clone() }
    }
}

/// The table's side: answers `poll` from the queue.
pub struct HumanAgent<M: RulesModule> {
    shared: Arc<Mutex<Shared<M>>>,
}

/// A human agent for the table and the handle for the UI.
pub fn human_seat<M: RulesModule>() -> (HumanAgent<M>, HumanSeat<M>) {
    let shared = Arc::new(Mutex::new(Shared { orders: VecDeque::new(), results: VecDeque::new(), journal: None }));
    (HumanAgent { shared: shared.clone() }, HumanSeat { shared })
}

impl<M: RulesModule> HumanSeat<M> {
    /// Queue a command for the game.
    pub fn submit(&self, command: M::Command) {
        lock(&self.shared).orders.push_back(Order::Command(command));
    }

    /// Queue the end of the player's turn.
    pub fn end_turn(&self) {
        lock(&self.shared).orders.push_back(Order::EndTurn);
    }

    /// Orders the game has not executed yet.
    pub fn has_orders(&self) -> bool {
        !lock(&self.shared).orders.is_empty()
    }

    /// The result of the oldest executed order nobody has read.
    pub fn take_result(&self) -> Option<Result<(), CommandError>> {
        lock(&self.shared).results.pop_front()
    }

    /// Keep every applied command with its journal until `take_journal`.
    pub fn record_journal(&self) {
        lock(&self.shared).journal.get_or_insert_with(Vec::new);
    }

    /// The commands applied since the last call, in order.
    pub fn take_journal(&self) -> Vec<AppliedCommand<M>> {
        lock(&self.shared).journal.as_mut().map(std::mem::take).unwrap_or_default()
    }
}

impl<M: RulesModule> std::fmt::Debug for HumanSeat<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HumanSeat")
    }
}

impl<M: RulesModule> PlayerAgent<M> for HumanAgent<M> {
    fn is_interactive(&self) -> bool {
        true
    }

    fn poll(&mut self, _cx: &AgentContext<'_, M>) -> AgentPoll<M::Command> {
        match lock(&self.shared).orders.pop_front() {
            Some(Order::Command(command)) => AgentPoll::Command(command),
            Some(Order::EndTurn) => AgentPoll::EndTurn,
            None => AgentPoll::Pending,
        }
    }

    fn observe(&mut self, event: AgentEvent<M>) {
        let AgentEvent::Executed { command, result } = event else {
            return;
        };
        let mut shared = lock(&self.shared);
        match result {
            Ok(result) => {
                if let Some(journal) = shared.journal.as_mut() {
                    journal.push(AppliedCommand { command, changes: result.into_journal() });
                }
                shared.results.push_back(Ok(()));
            }
            Err(error) => shared.results.push_back(Err(error)),
        }
    }
}
