//! The shell's keys, the clock's orders and the display's rhythm, over a made-up match.
use std::sync::Arc;

use render::unit_models::UnitModelBook;
use render::world::state::RenderWorld;
use render::world_models::WorldBook;

use super::*;
use crate::shell::observer::contract::{Books, CityEntry, Finale, Power, Round, Stat};
use crate::shell::observer::scene::Action;

/// A match of `rounds` rounds of `steps` slices each, with `powers` powers.
struct Fake {
    powers: Vec<Power>,
    worlds: Vec<RenderWorld>,
    round: u32,
    rounds: u32,
    steps: u32,
    left: u32,
    open: bool,
    serial: u64,
    refreshed: u32,
    /// How often each power's view was refreshed.
    refreshes: Vec<u32>,
    fallen: Vec<bool>,
    first: Option<usize>,
    revealed: bool,
    /// The cities every power has, biggest first.
    cities: Vec<CityEntry>,
}

impl Fake {
    fn new(powers: usize, rounds: u32, steps: u32) -> Self {
        Self {
            powers: (0..powers).map(|i| Power { player: i as u8, name: format!("P{i}") }).collect(),
            worlds: vec![RenderWorld::default(); powers],
            round: 1,
            rounds,
            steps,
            left: steps,
            open: false,
            serial: 0,
            refreshed: 0,
            refreshes: vec![0; powers],
            fallen: vec![false; powers],
            first: None,
            revealed: false,
            cities: Vec::new(),
        }
    }
}

impl Observed for Fake {
    fn powers(&self) -> &[Power] {
        &self.powers
    }
    fn alive(&self, power: usize) -> bool {
        !self.fallen[power]
    }
    fn standing(&self, _: usize) -> &[Stat] {
        &[]
    }
    fn round(&self) -> Round {
        Round { now: self.round, cap: Some(self.rounds) }
    }
    fn advance(&mut self, _: Duration) -> Progress {
        if self.is_over() {
            return Progress::Over;
        }
        self.open = true;
        self.left -= 1;
        if self.left > 0 {
            return Progress::Working;
        }
        self.left = self.steps;
        self.open = false;
        self.round += 1;
        self.serial += 1;
        if self.is_over() {
            Progress::Over
        } else {
            Progress::RoundDone
        }
    }
    fn mid_round(&self) -> bool {
        self.open
    }
    fn is_over(&self) -> bool {
        self.round > self.rounds
    }
    fn finale(&self) -> Option<Finale> {
        None
    }
    fn serial(&self) -> u64 {
        self.serial
    }
    fn refresh(&mut self, power: usize) -> bool {
        self.refreshed += 1;
        self.refreshes[power] += 1;
        true
    }
    fn world(&self, power: usize) -> &RenderWorld {
        &self.worlds[power]
    }
    fn books(&self) -> Books {
        Books { units: UnitModelBook::from_keys([]), world: WorldBook::default() }
    }
    fn state_hash(&self) -> u64 {
        u64::from(self.round)
    }
    fn summary(&self) -> String {
        String::new()
    }
    fn first_view(&self) -> Option<usize> {
        self.first
    }
    fn revealed(&self) -> bool {
        self.revealed
    }
    fn set_revealed(&mut self, on: bool) {
        self.revealed = on;
    }
    fn cities_of(&self, _: usize) -> Vec<CityEntry> {
        self.cities.clone()
    }
    fn city_at(&self, _: usize, tile: u32) -> Option<CityEntry> {
        self.cities.iter().find(|c| c.tile == tile).cloned()
    }
    fn city_card(&self, _: usize, tile: u32) -> Option<crate::shell::observer::card::Card> {
        self.city_at(0, tile).map(|c| crate::shell::observer::card::Card::new(c.name))
    }
}

fn shell_of(obs: &Fake, clock: Clock) -> ObserverShell {
    ObserverShell::new(obs, wgpu::TextureFormat::Rgba8UnormSrgb, Arc::new(Vec::new()), TerrainSync::Immediate, clock)
}

/// Frames of 16 ms, the views brought up to date as the app does; the rounds that ended.
/// One frame of 16 ms; the window draws every owed view at once.
fn frame_of(shell: &mut ObserverShell, obs: &mut Fake) {
    shell.pace(obs, Duration::from_millis(16));
    shell.pending_draw = false;
}

fn frames(shell: &mut ObserverShell, obs: &mut Fake, count: u32) -> u32 {
    let start = obs.round;
    for _ in 0..count {
        frame_of(shell, obs);
    }
    obs.round - start
}

#[test]
fn the_clock_waits_while_the_views_are_not_drawn() {
    let mut obs = Fake::new(4, 100, 1);
    let mut shell = shell_of(&obs, Clock::new(Speed::Sixteen));
    for _ in 0..60 {
        shell.pace(&mut obs, Duration::from_millis(16));
    }
    assert_eq!(obs.round, 1, "no frame drew the views, so the next round is not started");
}

#[test]
fn space_pauses_and_brackets_change_the_speed() {
    let mut obs = Fake::new(6, 10, 1);
    let mut shell = shell_of(&obs, Clock::default());
    assert_eq!(shell.key(egui::Key::Space, false, &mut obs), Key::Used);
    assert!(shell.clock.is_paused());
    shell.key(egui::Key::CloseBracket, false, &mut obs);
    assert_eq!(shell.clock.speed(), Speed::Two);
    shell.key(egui::Key::OpenBracket, false, &mut obs);
    shell.key(egui::Key::OpenBracket, false, &mut obs);
    assert_eq!(shell.clock.speed(), Speed::Half);
    assert_eq!(shell.key(egui::Key::A, false, &mut obs), Key::Ignored);
}

#[test]
fn digits_enlarge_a_panel_and_escape_returns_to_the_grid() {
    let mut obs = Fake::new(6, 10, 1);
    let mut shell = shell_of(&obs, Clock::default());
    assert_eq!(shell.key(egui::Key::Num3, false, &mut obs), Key::Used);
    assert_eq!(shell.focus, Some(2));
    assert_eq!(shell.key(egui::Key::Num8, false, &mut obs), Key::Ignored, "there is no eighth power");
    assert_eq!(shell.focus, Some(2));
    assert!(shell.escape(&mut obs), "Esc leaves the power shown alone");
    assert_eq!(shell.focus, None);
    assert!(!shell.escape(&mut obs), "with nothing to close Esc is the shell's");
}

#[test]
fn escape_closes_the_results_card_before_the_panel() {
    let mut obs = Fake::new(2, 1, 1);
    let mut shell = shell_of(&obs, Clock::new(Speed::Max));
    shell.focus = Some(1);
    frames(&mut shell, &mut obs, 4);
    assert!(obs.is_over() && shell.finale_open, "the card opens when the match ends");
    assert!(shell.escape(&mut obs));
    assert!(!shell.finale_open && shell.focus == Some(1));
    assert!(shell.escape(&mut obs));
    assert_eq!(shell.focus, None);
}

#[test]
fn at_one_times_a_round_takes_about_a_second() {
    let mut obs = Fake::new(4, 100, 1);
    let mut shell = shell_of(&obs, Clock::new(Speed::One));
    let rounds = frames(&mut shell, &mut obs, 190);
    assert!((2..=3).contains(&rounds), "{rounds} rounds in about three seconds");
    let mut quick = Fake::new(4, 100, 1);
    let mut shell = shell_of(&quick, Clock::new(Speed::Sixteen));
    assert!(frames(&mut shell, &mut quick, 190) > 30, "sixteen times faster");
}

#[test]
fn a_paused_match_stands_still_and_steps_one_round_at_a_time() {
    let mut obs = Fake::new(4, 100, 3);
    let mut shell = shell_of(&obs, Clock::paused(Speed::Max));
    assert_eq!(frames(&mut shell, &mut obs, 200), 0, "paused: nothing plays");
    shell.key(egui::Key::N, false, &mut obs);
    assert_eq!(frames(&mut shell, &mut obs, 200), 1, "one step, one round, even if it takes three frames");
    shell.key(egui::Key::N, false, &mut obs);
    shell.key(egui::Key::N, false, &mut obs);
    assert_eq!(frames(&mut shell, &mut obs, 200), 1, "pressing again while it waits does not pile steps up");
}

#[test]
fn the_views_wait_for_the_end_of_a_round() {
    let mut obs = Fake::new(3, 100, 4);
    let mut shell = shell_of(&obs, Clock::new(Speed::Max));
    shell.pace(&mut obs, Duration::from_millis(16));
    assert!(obs.mid_round());
    let before = obs.refreshed;
    shell.catch_up(&mut obs);
    assert_eq!(obs.refreshed, before, "no view is refreshed in the middle of a round");
}

#[test]
fn at_the_greatest_speed_the_views_follow_a_few_times_a_second() {
    let mut obs = Fake::new(3, 10_000, 1);
    let mut shell = shell_of(&obs, Clock::new(Speed::Max));
    let rounds = frames(&mut shell, &mut obs, 60);
    assert!(rounds >= 60, "a round on every frame: {rounds}");
    // 60 frames are 960 ms: about six displays of three views, not sixty.
    assert!(obs.refreshed <= 3 * 8, "{} view refreshes", obs.refreshed);
    assert!(obs.refreshed >= 3 * 4, "{} view refreshes", obs.refreshed);
}

#[test]
fn the_match_hash_does_not_depend_on_how_it_was_watched() {
    let hash_after = |clock: Clock, keys: &[(u32, egui::Key)]| {
        let mut obs = Fake::new(3, 30, 3);
        let mut shell = shell_of(&obs, clock);
        for frame in 0..200_000 {
            if obs.is_over() {
                break;
            }
            for (_, key) in keys.iter().filter(|(at, _)| *at == frame) {
                shell.key(*key, false, &mut obs);
            }
            if frame % 500 == 499 && shell.clock.is_paused() {
                shell.key(egui::Key::Space, false, &mut obs);
            }
            frame_of(&mut shell, &mut obs);
        }
        obs.state_hash()
    };
    let plain = hash_after(Clock::new(Speed::Max), &[]);
    let fiddled = hash_after(
        Clock::new(Speed::Quarter),
        &[(3, egui::Key::Space), (40, egui::Key::N), (41, egui::Key::CloseBracket), (90, egui::Key::Space), (100, egui::Key::Space), (101, egui::Key::N), (50, egui::Key::Num2), (60, egui::Key::Tab), (70, egui::Key::G), (75, egui::Key::G), (80, egui::Key::ArrowLeft)],
    );
    assert_eq!(plain, fiddled);
    assert_eq!(plain, 31);
}

fn alive_of(obs: &Fake) -> Vec<bool> {
    obs.fallen.iter().map(|f| !f).collect()
}

#[test]
fn tab_and_the_arrows_switch_powers_without_leaving_the_view_and_pass_the_fallen() {
    let mut obs = Fake::new(5, 10, 1);
    obs.fallen[2] = true;
    let mut shell = shell_of(&obs, Clock::default());
    assert_eq!(shell.key(egui::Key::Tab, false, &mut obs), Key::Ignored, "in the grid Tab is not ours");
    shell.key(egui::Key::Num2, false, &mut obs);
    assert_eq!(shell.focus, Some(1));
    shell.key(egui::Key::Tab, false, &mut obs);
    assert_eq!(shell.focus, Some(3), "the fallen third power is passed");
    shell.key(egui::Key::ArrowRight, false, &mut obs);
    assert_eq!(shell.focus, Some(4));
    shell.key(egui::Key::Tab, true, &mut obs);
    assert_eq!(shell.focus, Some(3), "Shift+Tab goes back");
    shell.key(egui::Key::ArrowLeft, false, &mut obs);
    shell.key(egui::Key::ArrowLeft, false, &mut obs);
    assert_eq!(shell.focus, Some(0));
    shell.key(egui::Key::Num3, false, &mut obs);
    assert_eq!(shell.focus, Some(2), "a digit chooses even a fallen power");
    assert_eq!(alive_of(&obs), [true, true, false, true, true]);
}

#[test]
fn the_view_of_one_power_stays_until_the_viewer_leaves_it() {
    let mut obs = Fake::new(4, 100, 1);
    let mut shell = shell_of(&obs, Clock::new(Speed::Sixteen));
    shell.apply(Action::Focus(2), &mut obs);
    frames(&mut shell, &mut obs, 200);
    assert_eq!(shell.focus, Some(2), "rounds pass and the view does not go back by itself");
    shell.apply(Action::ToggleView, &mut obs);
    assert_eq!(shell.focus, None);
    shell.apply(Action::ToggleView, &mut obs);
    assert_eq!(shell.focus, Some(2), "the button returns to the power shown last");
    shell.key(egui::Key::G, false, &mut obs);
    assert_eq!(shell.focus, None, "G does the same as the button");
}

#[test]
fn one_power_alone_refreshes_only_that_power_and_the_grid_brings_the_rest_up() {
    let mut obs = Fake::new(4, 100, 1);
    let mut shell = shell_of(&obs, Clock::new(Speed::Sixteen));
    shell.apply(Action::Focus(1), &mut obs);
    obs.refreshes.iter_mut().for_each(|n| *n = 0);
    frames(&mut shell, &mut obs, 40);
    assert!(obs.refreshes[1] >= 2, "the power shown is refreshed every round");
    assert_eq!((obs.refreshes[0], obs.refreshes[2], obs.refreshes[3]), (0, 0, 0), "the others are left alone");
    shell.apply(Action::Focus(3), &mut obs);
    assert_eq!(obs.refreshes[3], 1, "a power chosen is brought up to date at once");
    shell.escape(&mut obs);
    assert!(obs.refreshes.iter().all(|&n| n >= 1), "the grid refreshes every power again: {:?}", obs.refreshes);
}

#[test]
fn a_match_that_opens_on_one_power_starts_in_the_view_of_that_power() {
    let mut obs = Fake::new(4, 10, 1);
    obs.first = Some(2);
    let shell = shell_of(&obs, Clock::default());
    assert_eq!(shell.focus, Some(2));
    obs.first = Some(9);
    assert_eq!(shell_of(&obs, Clock::default()).focus, None, "a power the match does not have is no view");
}

#[test]
fn the_camera_follows_until_the_viewer_frees_it() {
    let mut obs = Fake::new(3, 10, 1);
    let mut shell = shell_of(&obs, Clock::default());
    assert_eq!(shell.camera_mode(), CameraMode::Follow);
    shell.apply(Action::Focus(0), &mut obs);
    shell.apply(Action::Camera(CameraMode::Free), &mut obs);
    shell.apply(Action::Camera(CameraMode::Follow), &mut obs);
    assert_eq!(shell.camera_mode(), CameraMode::Follow, "no panel is drawn yet, so nothing moves");
}

fn city(tile: u32, name: &str) -> CityEntry {
    CityEntry { tile, name: name.to_owned(), owner: "P0".to_owned(), player: 0, own: true, population: Some(5) }
}

#[test]
fn r_opens_the_whole_map_in_the_views_shown_and_closes_it_again() {
    let mut obs = Fake::new(3, 10, 1);
    let mut shell = shell_of(&obs, Clock::paused(Speed::One));
    shell.catch_up(&mut obs);
    let before = obs.refreshed;
    assert_eq!(shell.key(egui::Key::R, false, &mut obs), Key::Used);
    assert!(obs.revealed(), "the map is open");
    assert_eq!(obs.refreshed - before, 3, "every power of the grid is brought up to date at once");
    assert!(shell.owes(), "the panels are redrawn");
    shell.apply(Action::ToggleReveal, &mut obs);
    assert!(!obs.revealed(), "the button does what the key does");
}

#[test]
fn the_whole_map_waits_for_the_end_of_a_round_in_the_middle_of_one() {
    let mut obs = Fake::new(3, 100, 4);
    let mut shell = shell_of(&obs, Clock::new(Speed::Max));
    shell.pace(&mut obs, Duration::from_millis(16));
    assert!(obs.mid_round());
    let before = obs.refreshed;
    shell.key(egui::Key::R, false, &mut obs);
    assert!(obs.revealed());
    assert_eq!(obs.refreshed, before, "no view changes in the middle of a round: it follows at the end");
}

#[test]
fn the_city_card_opens_with_the_key_the_digits_and_the_arrows_and_esc_closes_it_before_the_view() {
    let mut obs = Fake::new(3, 10, 1);
    obs.cities = vec![city(10, "Ur"), city(20, "Uruk"), city(30, "Kish")];
    let mut shell = shell_of(&obs, Clock::default());
    assert_eq!(shell.key(egui::Key::C, false, &mut obs), Key::Ignored, "in the grid there is no power whose cities to show");
    shell.key(egui::Key::Num1, false, &mut obs);
    assert_eq!(shell.focus, Some(0));
    shell.key(egui::Key::C, false, &mut obs);
    assert_eq!(shell.inspect.city.as_ref().map(|c| c.tile), Some(10), "the biggest city first");
    shell.key(egui::Key::Num3, false, &mut obs);
    assert_eq!(shell.inspect.city.as_ref().map(|c| c.tile), Some(30), "with a card open a digit is a city");
    shell.key(egui::Key::ArrowRight, false, &mut obs);
    assert_eq!(shell.inspect.city.as_ref().map(|c| c.tile), Some(10), "the arrows go round the cities");
    shell.key(egui::Key::ArrowLeft, false, &mut obs);
    assert_eq!(shell.inspect.city.as_ref().map(|c| c.tile), Some(30));
    assert_eq!(shell.focus, Some(0), "the arrows did not change the power");
    assert!(shell.escape(&mut obs));
    assert!(shell.inspect.city.is_none() && shell.focus == Some(0), "Esc closes the card first");
    assert!(shell.escape(&mut obs));
    assert_eq!(shell.focus, None);
}

#[test]
fn tab_drops_the_card_and_switches_the_power_and_a_click_on_no_land_drops_the_choice() {
    let mut obs = Fake::new(3, 10, 1);
    obs.cities = vec![city(10, "Ur")];
    let mut shell = shell_of(&obs, Clock::default());
    shell.key(egui::Key::Num1, false, &mut obs);
    shell.apply(Action::ToggleCities, &mut obs);
    assert!(shell.inspect.city.is_some());
    shell.key(egui::Key::Tab, false, &mut obs);
    assert_eq!(shell.focus, Some(1));
    assert!(shell.inspect.city.is_none(), "the card belongs to the power it was opened for");
    shell.apply(Action::ToggleCities, &mut obs);
    shell.apply(Action::CardClose, &mut obs);
    assert!(shell.inspect.city.is_none(), "the button closes the card");
}
