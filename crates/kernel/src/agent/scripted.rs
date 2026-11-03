//! ScriptedAgent: a seat played from a list or a function. Replaces the
//! hand-rolled loops of tests, benches and perturbation runs.

use super::{AgentContext, AgentEvent, AgentPoll, PlayerAgent};
use crate::module::RulesModule;

type Script<M> = Box<dyn FnMut(&AgentContext<'_, M>) -> AgentPoll<<M as RulesModule>::Command>>;

/// Answers each poll from a script. When the script ends the agent answers
/// `Pending`: the seat waits for ever, which a test notices as a stuck game.
pub struct ScriptedAgent<M: RulesModule> {
    script: Script<M>,
    applied: u64,
    rejected: u64,
}

impl<M: RulesModule> ScriptedAgent<M> {
    /// The answers in order, one per poll.
    pub fn new<I>(answers: I) -> Self
    where
        I: IntoIterator<Item = AgentPoll<M::Command>>,
        I::IntoIter: 'static,
    {
        let mut answers = answers.into_iter();
        Self::from_fn(move |_| answers.next().unwrap_or(AgentPoll::Pending))
    }

    /// The commands in order, then `Pending`.
    pub fn commands<I>(commands: I) -> Self
    where
        I: IntoIterator<Item = M::Command>,
        I::IntoIter: 'static,
        M::Command: 'static,
    {
        Self::new(commands.into_iter().map(AgentPoll::Command))
    }

    /// Answers computed from what the seat sees.
    pub fn from_fn(f: impl FnMut(&AgentContext<'_, M>) -> AgentPoll<M::Command> + 'static) -> Self {
        Self {
            script: Box::new(f),
            applied: 0,
            rejected: 0,
        }
    }

    /// Commands the game applied and refused since the start.
    pub fn counts(&self) -> (u64, u64) {
        (self.applied, self.rejected)
    }
}

impl<M: RulesModule> PlayerAgent<M> for ScriptedAgent<M> {
    fn poll(&mut self, cx: &AgentContext<'_, M>) -> AgentPoll<M::Command> {
        (self.script)(cx)
    }

    fn observe(&mut self, event: AgentEvent<M>) {
        if let AgentEvent::Executed { result, .. } = event {
            if result.is_ok() {
                self.applied += 1;
            } else {
                self.rejected += 1;
            }
        }
    }
}
