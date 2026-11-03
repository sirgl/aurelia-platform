//! The anti-cheat audit of a rules module: the commands the AI really plays, submitted by every
//! other seat of an open window, must all be refused. The AI's commands are the module's own
//! corpus (every kind a player has to be able to give, with real arguments), so a command that
//! some module forgot to tie to its player shows here instead of in a match.

use kernel::ai::{AiPlayers, StepOutcome};
use kernel::ids::PlayerId;
use kernel::replay::codec::CommandCodec;
use kernel::rules::CompiledRuleset;
use kernel::world::state::GameState;

#[derive(Debug, Clone, Default)]
pub struct AuditReport {
    /// AI commands replayed as other seats.
    pub commands: usize,
    /// Tries made (a command is tried as each other seat).
    pub tries: usize,
    /// Commands some other seat got executed: the holes.
    pub accepted_as_foreign: Vec<String>,
    /// Commands the actor's own seat got refused when replayed in a window (should be none:
    /// the window must let a seat do what its turn lets it do).
    pub refused_as_own: Vec<String>,
    /// Moves to unexplored tiles the AI orders and a window refuses on purpose (a refusal there
    /// is a probe of the map; see `kernel::command::validate`).
    pub unexplored: usize,
}

/// Play `steps` steps of the AI on `game` and replay each command it applies as every other
/// living player of an open window (and as its own seat, to see that the window lets it through).
pub fn foreign_command_audit<M: CommandCodec>(
    game: &mut GameState<M>,
    ai: &mut M::Ai,
    rules: &CompiledRuleset<M>,
    steps: u32,
) -> AuditReport {
    let mut report = AuditReport::default();
    ai.record_journal();
    let seats = game.view().auth.players().count();
    for _ in 0..steps {
        let before = game.clone();
        let actor = game.view().auth.active_player();
        let outcome = ai.step(game, rules);
        let applied = ai.take_journal();
        if let Some(first) = applied.first() {
            report.commands += 1;
            let mask = (1u64 << seats.min(63)) - 1;
            let label = format!("{:?}", first.command);
            for q in 0..seats as u8 {
                let seat = PlayerId(q);
                if !before.view().auth.players().is_alive(seat) {
                    continue;
                }
                let mut probe = before.clone();
                probe.open_window(mask);
                let result = probe.execute_as(seat, rules, first.command.clone());
                if seat == actor {
                    // Ending a turn is the one thing a window refuses its own seat.
                    match &result {
                        Err(e) if format!("{e:?}").contains("unexplored destination") => {
                            report.unexplored += 1
                        }
                        Err(e) if !label.contains("EndTurn") => {
                            report.refused_as_own.push(format!("{label} -> {e:?}"))
                        }
                        _ => {}
                    }
                } else {
                    report.tries += 1;
                    if result.is_ok() {
                        report
                            .accepted_as_foreign
                            .push(format!("seat {q} did {label}"));
                    }
                }
            }
        }
        if outcome == StepOutcome::Waiting {
            break;
        }
    }
    report
}
