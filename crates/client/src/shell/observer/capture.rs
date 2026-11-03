//! Offscreen pictures of the observer screen (`AURELIA_OBSERVER=1`): a match of AI powers
//! is played to the asked rounds with no window, and the grid (or one enlarged panel)
//! is saved after each. Shared by every module's `--screenshot`.
//!
//! `AURELIA_OBSERVER_ROUND=1,40,100` the rounds to shoot (the first round by default),
//! `AURELIA_OBSERVER_PLAYER=0,3` the powers to show alone (from 1), one picture each, 0 is the
//! grid (the grid by default; `AURELIA_OBSERVER_FOCUS` is the older name of the same),
//! `AURELIA_OBSERVER_TAB=1` presses Tab that many times after a power is shown (the
//! picture is of the power it switched to), `AURELIA_OBSERVER_BENCH=600` measures that many frames from the first
//! round asked instead of shooting (frame times to the console, a frame lasts 16 ms of
//! match time), `AURELIA_OBSERVER_SPEED=0.25|0.5|1|2|4|8|16|max` the speed the bar
//! shows (×1), `AURELIA_OBSERVER_PAUSED=1` the bar shows the pause, `AURELIA_OBSERVER_REVEAL=1` the
//! whole map is open (also in a window), `AURELIA_OBSERVER_LOOK=city|unit|hover-city|hover-unit|
//! hover-stranger` what the viewer looks at in a power shown alone: its biggest city's card,
//! its first unit chosen (the outline of its sight), or the pointer held over a city, a unit
//! or a unit of another power.
use render::gpu::context::GpuCore;

use super::frame::Size;
use super::inspect::Look;
use super::shell::Stats;
use super::speed::{Clock, Speed};
use crate::shell::app::App;
use crate::shell::capture::{save_capture, wait_idle};
use crate::shell::module::ModuleUi;

/// What to shoot.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub rounds: Vec<u32>,
    /// The powers to show alone, from 1; 0 is the grid.
    pub focus: Vec<usize>,
    /// How many times Tab is pressed after a power is shown.
    pub tabs: u32,
    pub clock: Clock,
    pub size: Size,
    /// What the viewer looks at in each power shown alone.
    pub look: Option<Look>,
}

/// `AURELIA_OBSERVER_REVEAL=1`: the match starts with the whole map open.
pub fn reveal_requested(var: &impl Fn(&str) -> Option<String>) -> bool {
    var("AURELIA_OBSERVER_REVEAL").as_deref() == Some("1")
}

/// `AURELIA_OBSERVER=1`: the run is an observer's.
pub fn requested() -> bool {
    std::env::var("AURELIA_OBSERVER").as_deref() == Ok("1")
}

fn list(var: &impl Fn(&str) -> Option<String>, name: &str) -> Vec<u32> {
    var(name).map(|v| v.split(',').filter_map(|t| t.trim().parse().ok()).collect()).unwrap_or_default()
}

/// The speed a name stands for.
pub fn speed_named(name: &str) -> Option<Speed> {
    let speed = match name.trim().to_lowercase().as_str() {
        "0.25" => Speed::Quarter,
        "0.5" => Speed::Half,
        "1" => Speed::One,
        "2" => Speed::Two,
        "4" => Speed::Four,
        "8" => Speed::Eight,
        "16" => Speed::Sixteen,
        "max" => Speed::Max,
        _ => return None,
    };
    Some(speed)
}

impl Options {
    pub fn from_env(size: Size) -> Self {
        Self::from_vars(|name| std::env::var(name).ok(), size)
    }

    /// The options `var` (the environment, or a test's) asks for.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>, size: Size) -> Self {
        let mut rounds = list(&var, "AURELIA_OBSERVER_ROUND");
        if rounds.is_empty() {
            rounds.push(1);
        }
        let mut focus: Vec<usize> = list(&var, "AURELIA_OBSERVER_PLAYER").into_iter().map(|f| f as usize).collect();
        if focus.is_empty() {
            focus = list(&var, "AURELIA_OBSERVER_FOCUS").into_iter().map(|f| f as usize).collect();
        }
        if focus.is_empty() {
            focus.push(0);
        }
        let speed = var("AURELIA_OBSERVER_SPEED").and_then(|n| speed_named(&n)).unwrap_or(Speed::One);
        let clock = if var("AURELIA_OBSERVER_PAUSED").as_deref() == Some("1") { Clock::paused(speed) } else { Clock::new(speed) };
        let tabs = var("AURELIA_OBSERVER_TAB").and_then(|t| t.trim().parse().ok()).unwrap_or(0);
        let look = var("AURELIA_OBSERVER_LOOK").and_then(|n| Look::named(&n));
        Options { rounds, focus, tabs, clock, size, look }
    }
}

/// The path of the picture of round `round` and panel `focus`, one of `pictures` (a lone
/// picture keeps `path`).
pub fn picture_path(path: &str, round: u32, focus: usize, pictures: usize) -> String {
    if pictures == 1 {
        return path.to_owned();
    }
    let stem = path.trim_end_matches(".png");
    match focus {
        0 => format!("{stem}-r{round}.png"),
        panel => format!("{stem}-r{round}-p{panel}.png"),
    }
}

/// Play the observed match of `app` to each round asked and save the screen. The module
/// must have started its observed match.
pub fn run<U: ModuleUi>(app: &mut App<U>, core: &GpuCore, texture: &wgpu::Texture, target: &wgpu::TextureView, options: &Options, path: &str) {
    app.observer_clock = options.clock;
    app.observer_terrain_sync = render::gpu::terrain_gpu::TerrainSync::Immediate;
    app.apply_module_actions();
    let Some(shell) = app.observer_mut() else {
        eprintln!("the module started no observed match");
        return;
    };
    shell.clock = options.clock;
    shell.budget = usize::MAX;
    shell.pin_time(std::time::Duration::ZERO);
    if let Some(frames) = std::env::var("AURELIA_OBSERVER_BENCH").ok().and_then(|v| v.parse::<u32>().ok()) {
        return bench(app, core, target, options, frames);
    }
    let pictures = options.rounds.len() * options.focus.len();
    for &round in &options.rounds {
        app.play_observer_to(round);
        for &focus in &options.focus {
            show(app, focus, options.tabs);
            settle(app, core, target, options.size);
            if let Some(look) = options.look.filter(|_| focus > 0) {
                if app.observer_stage(look) {
                    settle(app, core, target, options.size);
                    run_frame(app, core, target, options.size, 100);
                    settle(app, core, target, options.size);
                } else {
                    eprintln!("nothing to look at for {look:?} in round {round}");
                }
            }
            save_capture(core, texture, options.size.width, options.size.height, &picture_path(path, round, focus, pictures));
        }
    }
    report(app);
}

fn show<U: ModuleUi>(app: &mut App<U>, focus: usize, tabs: u32) {
    app.observer_show(focus.checked_sub(1));
    for _ in 0..tabs {
        app.key(egui::Key::Tab);
    }
}

/// Frames until the layout settled and no panel has work left.
pub fn settle<U: ModuleUi>(app: &mut App<U>, core: &GpuCore, target: &wgpu::TextureView, size: Size) {
    let mut quiet = 0;
    for frame in 0..64u32 {
        run_frame(app, core, target, size, frame);
        wait_idle(core);
        quiet = if app.observer().is_some_and(|s| s.busy()) { 0 } else { quiet + 1 };
        if quiet >= 3 {
            break;
        }
    }
}

/// One frame of the app at `size`, `frame` frames into the run (the clock of egui moves
/// 1/60 s a frame).
fn run_frame<U: ModuleUi>(app: &mut App<U>, core: &GpuCore, target: &wgpu::TextureView, size: Size, frame: u32) {
    let screen = egui::vec2(size.width as f32, size.height as f32) / app.ctx.pixels_per_point();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
        time: Some(f64::from(frame) / 60.0),
        focused: true,
        ..Default::default()
    };
    app.frame(core, target, (size.width, size.height), raw);
}

fn report<U: ModuleUi>(app: &mut App<U>) {
    let stats = app.observer().map(|s| s.stats);
    if let (Some(stats), Some(hash)) = (stats, app.observed_hash()) {
        println!("observer: {} rounds, {} panel renders, {} view refreshes, world hash {hash:#018x}", stats.rounds, stats.renders, stats.refreshes);
    }
}

/// Frame times of `frames` frames (fewer when the match ends first) played from the first
/// round asked, printed as p50, p95 and the worst, with the panels' work. Each frame waits for the GPU, so the times are
/// what a window that cannot overlap CPU and GPU would see.
fn bench<U: ModuleUi>(app: &mut App<U>, core: &GpuCore, target: &wgpu::TextureView, options: &Options, frames: u32) {
    app.play_observer_to(options.rounds[0]);
    let size = options.size;
    if let Some(shell) = app.observer_mut() {
        shell.budget = crate::shell::observer::shell::PANELS_PER_FRAME;
        shell.pin_time(std::time::Duration::from_millis(16));
    }
    app.observer_show(options.focus[0].checked_sub(1));
    settle(app, core, target, size);
    let before = app.observer().map(|s| s.stats).unwrap_or_default();
    let mut times = Vec::new();
    while times.len() < frames as usize && !app.observed_over() {
        times.push(timed_frame(app, core, target, size, times.len() as u32));
    }
    let after = app.observer().map(|s| s.stats).unwrap_or_default();
    print_bench(&times, before, after, options);
}

/// One frame of the observer screen, waiting for the GPU; its milliseconds.
fn timed_frame<U: ModuleUi>(app: &mut App<U>, core: &GpuCore, target: &wgpu::TextureView, size: Size, frame: u32) -> f64 {
    let started = web_time::Instant::now();
    run_frame(app, core, target, size, frame);
    wait_idle(core);
    started.elapsed().as_secs_f64() * 1000.0
}

fn print_bench(times: &[f64], before: Stats, after: Stats, options: &Options) {
    if times.is_empty() {
        return println!("observer bench: no frame was played (the match had ended)");
    }
    let mut sorted = times.to_vec();
    sorted.sort_by(f64::total_cmp);
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
    println!(
        "observer bench: {} frames at {:?}, speed {:?}: p50 {:.1} ms, p95 {:.1} ms, worst {:.1} ms, mean {:.1} ms, {} frames over 33 ms; {} rounds, {} slices ({:.1} ms of AI work in all), {} panel renders ({:.0} ms drawing, {:.0} ms refreshing views)",
        times.len(),
        (options.size.width, options.size.height),
        options.clock.speed(),
        at(0.5),
        at(0.95),
        sorted[sorted.len() - 1],
        times.iter().sum::<f64>() / times.len() as f64,
        times.iter().filter(|t| **t > 33.4).count(),
        after.rounds - before.rounds,
        after.slices - before.slices,
        after.slice_ms - before.slice_ms,
        after.renders - before.renders,
        after.render_ms - before.render_ms,
        after.refresh_ms - before.refresh_ms
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: Size = Size { width: 100, height: 100 };

    fn options(vars: &[(&str, &str)]) -> Options {
        let vars: Vec<(String, String)> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Options::from_vars(|name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()), SIZE)
    }

    #[test]
    fn nothing_asked_is_the_grid_of_the_first_round() {
        let o = options(&[]);
        assert_eq!((o.rounds, o.focus, o.tabs), (vec![1], vec![0], 0));
    }

    #[test]
    fn nation_observer_player_names_the_powers_to_show_alone() {
        assert_eq!(options(&[("AURELIA_OBSERVER_PLAYER", "3")]).focus, vec![3]);
        assert_eq!(options(&[("AURELIA_OBSERVER_PLAYER", "1, 3")]).focus, vec![1, 3]);
        assert_eq!(options(&[("AURELIA_OBSERVER_PLAYER", "x")]).focus, vec![0], "a word is no power: the grid");
        assert_eq!(options(&[("AURELIA_OBSERVER_FOCUS", "2")]).focus, vec![2], "the older name still works");
        assert_eq!(options(&[("AURELIA_OBSERVER_FOCUS", "2"), ("AURELIA_OBSERVER_PLAYER", "4")]).focus, vec![4], "the new name wins");
    }

    #[test]
    fn rounds_tabs_and_the_clock_are_read() {
        let o = options(&[("AURELIA_OBSERVER_ROUND", "1,40,100"), ("AURELIA_OBSERVER_TAB", "2"), ("AURELIA_OBSERVER_SPEED", "max"), ("AURELIA_OBSERVER_PAUSED", "1")]);
        assert_eq!(o.rounds, vec![1, 40, 100]);
        assert_eq!(o.tabs, 2);
        assert_eq!(o.clock.speed(), Speed::Max);
        assert!(o.clock.is_paused());
    }

    #[test]
    fn the_open_map_and_the_look_are_read() {
        let vars = |v: &'static str| move |name: &str| (name == "AURELIA_OBSERVER_REVEAL").then(|| v.to_owned());
        assert!(reveal_requested(&vars("1")));
        assert!(!reveal_requested(&vars("0")));
        assert_eq!(options(&[("AURELIA_OBSERVER_LOOK", "hover-city")]).look, Some(Look::HoverCity));
        assert_eq!(options(&[]).look, None);
    }

    #[test]
    fn a_picture_path_names_the_round_and_the_power() {
        assert_eq!(picture_path("a.png", 40, 0, 3), "a-r40.png");
        assert_eq!(picture_path("a.png", 40, 3, 3), "a-r40-p3.png");
        assert_eq!(picture_path("a.png", 40, 3, 1), "a.png");
    }
}
