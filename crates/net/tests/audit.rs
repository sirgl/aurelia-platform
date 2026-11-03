//! The anti-cheat audit on the sandbox: every command the AI plays, given by another seat, is refused.

mod common;

use common::*;
use kernel::ai::driver::AiDriver;
use kernel::ai::AiPlayers;
use sandbox_rules::ai::SandboxMind;
use sandbox_rules::Sandbox;

#[test]
fn no_command_of_the_ai_works_for_another_seat() {
    let rules = rules();
    let mut game = standard_game()(&rules, &spec(0, 4, 3));
    let mut ai =
        AiDriver::<Sandbox, SandboxMind>::for_game(&rules, rules.default_difficulty(), Vec::new());
    ai.configure_game(&mut game);
    let report = net::audit::foreign_command_audit(&mut game, &mut ai, &rules, 600);
    assert!(report.commands > 100, "a real corpus: {}", report.commands);
    assert!(
        report.accepted_as_foreign.is_empty(),
        "{:#?}",
        report.accepted_as_foreign
    );
    assert!(
        report.refused_as_own.is_empty(),
        "{:#?}",
        report.refused_as_own
    );
    eprintln!(
        "sandbox audit: {} commands, {} foreign tries, all refused",
        report.commands, report.tries
    );
}
