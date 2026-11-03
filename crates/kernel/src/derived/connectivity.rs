use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::ChangeJournal;
use crate::world::state::AuthWorld;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectivityState;

impl ConnectivityState {
    pub fn rebuild<M: RulesModule>(_auth: &AuthWorld<M>, _rules: &CompiledRuleset<M>) -> Self {
        Self
    }
    pub fn apply_changes<M: RulesModule>(
        &mut self,
        _changes: &ChangeJournal<M>,
        _auth: &AuthWorld<M>,
        _rules: &CompiledRuleset<M>,
    ) {
    }
}
