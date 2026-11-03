//! What happened to each power, read from the world between rounds: the cities founded,
//! taken and lost, the wars begun and ended, the technologies learned, the fall. The
//! facts of every round are compared with the facts of the one before, so the
//! simulation is not touched: this only reads the world.
use std::collections::{BTreeMap, BTreeSet};

use kernel::ids::PlayerId;
use kernel::module::RulesModule;
use kernel::rules::CompiledRuleset;
use kernel::world::state::GameState;

use super::contract::{Event, Happened, Power};

/// The events kept for each power.
pub const KEPT: usize = 5;

/// A city of the world: who holds it and what it is called.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Held {
    owner: u8,
    name: String,
}

/// The facts of one round's end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Facts {
    cities: BTreeMap<u64, Held>,
    /// Technologies known, per power.
    known: Vec<BTreeSet<u16>>,
    /// The pairs of players at war, the lower number first.
    wars: BTreeSet<(u8, u8)>,
    alive: Vec<bool>,
}

/// The events of every power, round by round.
#[derive(Debug, Clone, Default)]
pub struct Chronicle {
    last: Facts,
    events: Vec<Vec<Happened>>,
}

impl Chronicle {
    pub fn new<M: RulesModule>(game: &GameState<M>, powers: &[Power]) -> Self {
        Self { last: facts(game, powers), events: vec![Vec::new(); powers.len()] }
    }

    pub fn of(&self, power: usize) -> &[Happened] {
        self.events.get(power).map_or(&[], Vec::as_slice)
    }

    /// Read the world after the round `round` and note what changed since the last time.
    pub fn note<M: RulesModule>(&mut self, game: &GameState<M>, rules: &CompiledRuleset<M>, powers: &[Power], round: u32) {
        let now = facts(game, powers);
        let tech = |id: u16| rules.technologies.get(usize::from(id)).map(|t| if t.name.is_empty() { t.key.clone() } else { t.name.clone() }).unwrap_or_default();
        let mut found = Vec::new();
        city_events(&self.last, &now, powers, &mut found);
        war_events(&self.last, &now, powers, &mut found);
        learned_events(&self.last, &now, &tech, &mut found);
        fall_events(&self.last, &now, &mut found);
        for (power, event) in found {
            self.push(power, round, event);
        }
        self.last = now;
    }

    fn push(&mut self, power: usize, round: u32, event: Event) {
        let Some(list) = self.events.get_mut(power) else { return };
        list.push(Happened { round, event });
        let extra = list.len().saturating_sub(KEPT);
        list.drain(..extra);
    }
}

fn facts<M: RulesModule>(game: &GameState<M>, powers: &[Power]) -> Facts {
    let auth = game.view().auth;
    let players = auth.players();
    let cities = auth.cities().iter().map(|(_, c)| (c.id.0, Held { owner: c.owner.0, name: c.name.clone() })).collect();
    let known = powers.iter().map(|p| players.techs_of(PlayerId(p.player)).iter().map(|t| t.0).collect()).collect();
    let numbers: Vec<u8> = powers.iter().map(|p| p.player).collect();
    let wars = numbers
        .iter()
        .flat_map(|&a| numbers.iter().filter(move |&&b| a < b).map(move |&b| (a, b)))
        .filter(|&(a, b)| auth.diplomacy().at_war(PlayerId(a), PlayerId(b)))
        .collect();
    let alive = powers.iter().map(|p| players.is_alive(PlayerId(p.player))).collect();
    Facts { cities, known, wars, alive }
}

type Found = Vec<(usize, Event)>;

fn index(powers: &[Power], player: u8) -> Option<usize> {
    powers.iter().position(|p| p.player == player)
}

fn name(powers: &[Power], player: u8) -> Option<String> {
    index(powers, player).map(|i| powers[i].name.clone())
}

fn city_events(before: &Facts, now: &Facts, powers: &[Power], found: &mut Found) {
    for (id, city) in &now.cities {
        let Some(owner) = index(powers, city.owner) else { continue };
        match before.cities.get(id) {
            None => found.push((owner, Event::Founded(city.name.clone()))),
            Some(old) if old.owner != city.owner => {
                found.push((owner, Event::Captured { city: city.name.clone(), from: name(powers, old.owner) }));
                if let Some(loser) = index(powers, old.owner) {
                    found.push((loser, Event::Lost { city: city.name.clone(), to: name(powers, city.owner) }));
                }
            }
            Some(_) => {}
        }
    }
    for old in before.cities.iter().filter(|(id, _)| !now.cities.contains_key(id)).map(|(_, old)| old) {
        if let Some(loser) = index(powers, old.owner) {
            found.push((loser, Event::Lost { city: old.name.clone(), to: None }));
        }
    }
}

fn war_events(before: &Facts, now: &Facts, powers: &[Power], found: &mut Found) {
    let both = |pair: &(u8, u8), make: &dyn Fn(String) -> Event, found: &mut Found| {
        for (me, other) in [(pair.0, pair.1), (pair.1, pair.0)] {
            if let (Some(i), Some(name)) = (index(powers, me), name(powers, other)) {
                found.push((i, make(name)));
            }
        }
    };
    now.wars.difference(&before.wars).for_each(|pair| both(pair, &Event::War, found));
    before.wars.difference(&now.wars).for_each(|pair| both(pair, &Event::Peace, found));
}

fn learned_events(before: &Facts, now: &Facts, tech: &dyn Fn(u16) -> String, found: &mut Found) {
    for (power, known) in now.known.iter().enumerate() {
        let old = before.known.get(power);
        for id in known.iter().filter(|id| old.is_none_or(|o| !o.contains(id))) {
            found.push((power, Event::Learned(tech(*id))));
        }
    }
}

fn fall_events(before: &Facts, now: &Facts, found: &mut Found) {
    for (power, alive) in now.alive.iter().enumerate() {
        if !alive && before.alive.get(power) == Some(&true) {
            found.push((power, Event::Fell));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn powers() -> Vec<Power> {
        vec![Power { player: 0, name: "A".into() }, Power { player: 1, name: "B".into() }]
    }

    fn facts_of(cities: &[(u64, u8, &str)], wars: &[(u8, u8)], alive: [bool; 2]) -> Facts {
        Facts {
            cities: cities.iter().map(|&(id, owner, name)| (id, Held { owner, name: name.into() })).collect(),
            known: vec![BTreeSet::new(); 2],
            wars: wars.iter().copied().collect(),
            alive: alive.to_vec(),
        }
    }

    #[test]
    fn a_new_city_a_capture_a_war_and_a_fall_are_told_to_the_right_power() {
        let before = facts_of(&[(1, 0, "Ur"), (2, 1, "Kish")], &[], [true, true]);
        let now = facts_of(&[(1, 0, "Ur"), (2, 0, "Kish"), (3, 1, "Lagash")], &[(0, 1)], [true, false]);
        let mut found = Vec::new();
        city_events(&before, &now, &powers(), &mut found);
        war_events(&before, &now, &powers(), &mut found);
        fall_events(&before, &now, &mut found);
        assert!(found.contains(&(1, Event::Founded("Lagash".into()))));
        assert!(found.contains(&(0, Event::Captured { city: "Kish".into(), from: Some("B".into()) })));
        assert!(found.contains(&(1, Event::Lost { city: "Kish".into(), to: Some("A".into()) })));
        assert!(found.contains(&(0, Event::War("B".into()))) && found.contains(&(1, Event::War("A".into()))));
        assert!(found.contains(&(1, Event::Fell)));
        assert_eq!(found.iter().filter(|(p, e)| *p == 0 && matches!(e, Event::Founded(_))).count(), 0);
    }

    #[test]
    fn peace_and_a_razed_city_are_noticed() {
        let before = facts_of(&[(1, 0, "Ur")], &[(0, 1)], [true, true]);
        let now = facts_of(&[], &[], [true, true]);
        let mut found = Vec::new();
        city_events(&before, &now, &powers(), &mut found);
        war_events(&before, &now, &powers(), &mut found);
        assert!(found.contains(&(0, Event::Lost { city: "Ur".into(), to: None })));
        assert!(found.contains(&(0, Event::Peace("B".into()))));
    }

    #[test]
    fn only_the_newest_events_are_kept() {
        let mut chronicle = Chronicle { last: Facts::default(), events: vec![Vec::new(); 2] };
        for round in 1..=9 {
            chronicle.push(0, round, Event::Fell);
        }
        assert_eq!(chronicle.of(0).len(), KEPT);
        assert_eq!(chronicle.of(0).last().map(|h| h.round), Some(9));
        assert!(chronicle.of(7).is_empty());
    }
}
