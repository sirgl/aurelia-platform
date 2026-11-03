//! The report line of `--replay`: what a launcher prints after replaying a command journal on its
//! module's rules. The module (a launcher) decides WHICH rules a save needs; this formats the result.

use kernel::module::RulesModule;
use kernel::replay::codec::CommandCodec;
use kernel::replay::journal::{replay as replay_journal, ReplayError, Replayed};
use kernel::rules::CompiledRuleset;

/// Replays `journal` from `save` on `rules` and formats what was replayed or where it diverged.
pub fn run<M: CommandCodec>(save: &[u8], journal: &[u8], rules: &CompiledRuleset<M>) -> Result<String, String> {
    report(replay_journal(save, journal, rules))
}

/// The same for a result the caller already has.
pub fn report<M: RulesModule>(result: Result<Replayed<M>, ReplayError>) -> Result<String, String> {
    let replayed = result.map_err(|e| e.to_string())?;
    let auth = replayed.game.view().auth;
    let hash = kernel::replay::authoritative_hash(&replayed.game);
    let cut = if replayed.cut { "; the file ends inside a record" } else { "" };
    Ok(format!(
        "{} commands, {} rounds checked; round {} world {hash:016x}{cut}",
        replayed.commands,
        replayed.rounds,
        auth.turn().round
    ))
}
