//! Play one AI-only sandbox game and print what happened: the end, each
//! player's empire every 10 rounds and the AI's rejected orders by reason.
//! `cargo run --release --offline -p aurelia-sandbox-rules --example sandbox_game -- [seed]`
use std::collections::BTreeMap;

use kernel::ai::driver::AiDriver;
use kernel::ai::trace::Outcome;
use kernel::ai::AiPlayers;
use kernel::ids::PlayerId;
use kernel::replay::hash::authoritative_hash;
use sandbox_rules::ai::SandboxMind;
use sandbox_rules::game::{new_game, Setup};
use sandbox_rules::victory::score;
use sandbox_rules::Sandbox;

fn main() {
    let seed = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(7_2026);
    let rules = sandbox_content::rules().expect("the sandbox pack compiles");
    let mut game = new_game(&rules, Setup::standard(seed));
    let mut ai = AiDriver::<Sandbox, SandboxMind>::for_game(&rules, rules.default_difficulty(), Vec::new());
    ai.set_trace_capacity(usize::MAX);
    ai.configure_game(&mut game);
    let started = std::time::Instant::now();
    let mut rejected: BTreeMap<String, u32> = BTreeMap::new();
    while !game.is_over() {
        ai.run(&mut game, &rules, u32::MAX);
        let auth = game.view().auth;
        let round = auth.turn().round;
        if round.is_multiple_of(10) {
            let empires: Vec<String> = (0..auth.players().count() as u8)
                .map(PlayerId)
                .map(|p| {
                    let (cities, citizens) = score(auth, p);
                    format!("P{} {cities}c/{citizens}p/{}u", p.0, auth.indexes().units_of(p).len())
                })
                .collect();
            println!("round {round:3}: {}", empires.join("  "));
        }
        for entry in ai.trace().entries() {
            if let Outcome::Rejected(error) = &entry.outcome {
                *rejected.entry(format!("{:?}: {error}", entry.reason)).or_default() += 1;
            }
        }
        ai.set_trace_capacity(usize::MAX);
    }
    let auth = game.view().auth;
    println!(
        "seed {seed}: round {}, winner {:?}, hash {:#018x}, commands {:?}, {:.2?}",
        auth.turn().round,
        game.winner(),
        authoritative_hash(&game),
        ai.command_counts(),
        started.elapsed()
    );
    for p in 0..auth.players().count() as u8 {
        let nation = &rules.nations[auth.players().nation_of(PlayerId(p)) as usize].name;
        let names: Vec<&str> = auth.indexes().cities_of(PlayerId(p)).iter().filter_map(|c| auth.cities().get(*c)).map(|c| c.name.as_str()).collect();
        println!("P{p} {nation}: {}", names.join(", "));
    }
    for (what, count) in rejected {
        println!("rejected {count:5} {what}");
    }
    println!("decisions: {:?}", ai.trace().totals());
}
