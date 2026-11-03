use crate::module::RulesModule;
use crate::persistence::schema::SaveGameV1;

/// initial snapshot + command log + periodic hashes (§25 replay).
#[derive(Debug, Clone)]
pub struct ReplayEntry<M: RulesModule> {
    pub command: M::Command,
    pub hash_after: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Replay<M: RulesModule> {
    pub initial: SaveGameV1<M>,
    pub entries: Vec<ReplayEntry<M>>,
}
