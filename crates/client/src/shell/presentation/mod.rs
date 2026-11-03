//! The presentation: what the player sees and hears of the commands that ran — units
//! walking their path, attacks, deaths, cities founded and grown, wonders, research.
//!
//! It is client state only. The game module derives cues from the command journal (the
//! classic module: `classic_ui::game::cues`) after the simulation already applied the commands;
//! playing them never changes, waits for or reads the simulation, so the world hash is the same with and without it. Cues name
//! units by persistent `EntityId` and carry what they draw; `reset` drops everything on
//! a new game or a load, so nothing refers to a unit of another world.
//!
//! Scheduling: cues wait in journal order. A cue starts when no running or earlier
//! waiting cue shares one of its keys (a unit, or a tile for city events); independent
//! cues play at once (several AI units move together). Other players' cues follow the
//! speed setting: normal, fast, or skipped (only those that concern the observer are
//! shown, quickly). A unit whose cues are over is held at its last look until the
//! projection shows it there (AI turns refresh the projection at player boundaries).
pub mod air;
pub mod cue;
pub mod motion;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet, VecDeque};

use kernel::ids::{EntityId, TileId};
use render::gpu::effects::FxEvent;
use render::gpu::unit_poses::{Gait, UnitPose};
use render::world::state::RenderWorld;

use cue::{Cue, Derived, Figure, Key, Origin};
use motion::{Beat, Look, Map, Mark, Sound};

/// How the other players' actions are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Speed {
    #[default]
    Normal,
    Fast,
    /// Not animated; actions against the observer still flash by quickly.
    Skip,
}

impl Speed {
    pub const ALL: [Speed; 3] = [Speed::Normal, Speed::Fast, Speed::Skip];

    /// The value in `settings.cfg`.
    pub fn key(self) -> &'static str {
        match self {
            Speed::Normal => "normal",
            Speed::Fast => "fast",
            Speed::Skip => "skip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Other players' turns.
    pub speed: Speed,
    /// No walk cycle, lunges or camera glides; units slide and fade.
    pub reduced_motion: bool,
    /// Glide the camera to a completed wonder.
    pub camera_glides: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { speed: Speed::Normal, reduced_motion: false, camera_glides: true }
    }
}

/// Fast speed multiplier, and the one for actions shown in spite of Skip.
const FAST: f32 = 3.0;
const SKIPPED: f32 = 5.0;
/// Waiting cues beyond this are dropped oldest first (an AI round never stalls the map).
const WAITING_LIMIT: usize = 160;
/// Longest wait for the projection to show a unit where its cues left it.
const HOLD_SECONDS: f64 = 2.5;

/// What the presentation shows this frame.
#[derive(Debug, Default)]
pub struct Frame {
    pub poses: Vec<UnitPose>,
    pub marks: Vec<LiveMark>,
    pub sounds: Vec<Sound>,
    /// 3D effects that start this frame (`Renderer::play_fx`).
    pub fx: Vec<FxEvent>,
    pub camera: Option<TileId>,
    /// Something plays or waits: keep drawing frames.
    pub busy: bool,
}

/// A map mark and how far through its life it is (0..1).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveMark {
    pub mark: Mark,
    pub progress: f32,
}

#[derive(Debug)]
struct Waiting {
    cue: Cue,
    speed: f32,
}

#[derive(Debug)]
struct Running {
    cue: Cue,
    speed: f32,
    /// Local time, seconds at normal speed.
    elapsed: f32,
    /// Looks of its units when it started.
    entry: Vec<(EntityId, Look)>,
}

/// A unit whose cues are over, kept at its last look until the projection agrees.
#[derive(Debug)]
struct Hold {
    figure: Figure,
    look: Look,
    /// It fell: the projection must drop it.
    gone: bool,
    since: f64,
}

#[derive(Debug)]
struct LiveEffect {
    mark: Mark,
    born: f64,
}

#[derive(Debug, Default)]
pub struct Presentation {
    settings: Settings,
    map: Map,
    clock: f64,
    waiting: VecDeque<Waiting>,
    running: Vec<Running>,
    holds: Vec<Hold>,
    /// Last look of every unit drawn last frame (continuity between cues).
    looks: HashMap<EntityId, Look>,
    effects: Vec<LiveEffect>,
    /// Health in percent of the units the projection shows (units that do not fight
    /// show the figures of their health).
    shown_hp: HashMap<u64, u8>,
    sounds: Vec<Sound>,
    /// 3D effects started since the last frame.
    fx: Vec<FxEvent>,
    camera: Option<TileId>,
}

impl Presentation {
    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// A new world (new game, load): every cue, hold and mark of the old one goes.
    pub fn reset(&mut self, map_width: u32) {
        let settings = self.settings;
        *self = Self { settings, map: Map { width: map_width }, ..Self::default() };
    }

    /// Nothing waits, plays or is held.
    pub fn is_idle(&self) -> bool {
        self.waiting.is_empty() && self.running.is_empty() && self.holds.is_empty() && self.effects.is_empty()
    }

    /// Cues waiting to start (tests, diagnostics).
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// The cues waiting or playing, in order (tests, diagnostics).
    pub fn queued_cues(&self) -> Vec<Cue> {
        let running = self.running.iter().map(|r| r.cue.clone());
        running.chain(self.waiting.iter().map(|w| w.cue.clone())).collect()
    }

    /// Queue derived cues in journal order (speed and Skip apply here).
    pub fn push(&mut self, cues: Vec<Derived>) {
        for derived in cues {
            let speed = match (derived.origin, self.settings.speed) {
                (Origin::Own, _) | (Origin::Other, Speed::Normal) => 1.0,
                (Origin::Other, Speed::Fast) => FAST,
                (Origin::Other, Speed::Skip) if derived.concerns_observer => SKIPPED,
                (Origin::Other, Speed::Skip) => continue,
            };
            self.waiting.push_back(Waiting { cue: derived.cue, speed });
        }
        while self.waiting.len() > WAITING_LIMIT {
            self.waiting.pop_front();
        }
    }

    /// One frame: `seconds` of wall time pass; `world` is the projection on screen.
    pub fn advance(&mut self, seconds: f32, world: &RenderWorld) -> Frame {
        let seconds = seconds.clamp(0.0, 0.25);
        self.clock += f64::from(seconds);
        self.play(seconds);
        self.start_waiting();
        self.release_holds(world);
        self.shown_hp.clear();
        self.shown_hp.extend(world.units.iter().map(|u| (u.id, u.hp)));
        self.effects.retain(|e| (self.clock - e.born) < f64::from(e.mark.seconds));
        let mut frame = Frame {
            sounds: std::mem::take(&mut self.sounds),
            fx: std::mem::take(&mut self.fx),
            camera: self.camera.take(),
            ..Frame::default()
        };
        frame.poses = self.poses();
        frame.marks = self.live_marks();
        frame.busy = !self.is_idle();
        frame
    }

    /// Plays every waiting and running cue to its end at once (Skip of a whole batch).
    pub fn finish_all(&mut self) {
        self.waiting.clear();
        self.running.clear();
        self.holds.clear();
        self.effects.clear();
        self.looks.clear();
        self.sounds.clear();
        self.fx.clear();
        self.camera = None;
    }

    /// A sound of the UI (selection) mixed with the cues' sounds.
    pub fn play_sound(&mut self, sound: Sound) {
        self.sounds.push(sound);
    }

    fn play(&mut self, seconds: f32) {
        let mut finished = Vec::new();
        for (index, running) in self.running.iter_mut().enumerate() {
            let before = running.elapsed;
            running.elapsed += seconds * running.speed;
            let beats = motion::moments(&running.cue, self.map)
                .into_iter()
                .filter(|m| m.at > before && m.at <= running.elapsed)
                .map(|m| m.beat);
            fire(beats, &mut self.sounds, &mut self.effects, &mut self.fx, &mut self.camera, self.clock, self.settings);
            if running.elapsed >= running.cue.duration() {
                finished.push(index);
            }
        }
        for index in finished.into_iter().rev() {
            let done = self.running.remove(index);
            self.hold_after(done);
        }
    }

    /// The units of a finished cue stay at their last look until the projection agrees
    /// (unless another cue of theirs waits).
    fn hold_after(&mut self, done: Running) {
        let gone = matches!(done.cue, Cue::Death { .. } | Cue::Vanish { .. });
        for figure in done.cue.figures() {
            let Some(entry) = done.entry.iter().find(|(id, _)| *id == figure.id).map(|(_, look)| *look) else {
                continue;
            };
            let Some(look) = motion::look(&done.cue, figure.id, done.cue.duration(), entry, self.map) else {
                continue;
            };
            let look = if gone { look.gone() } else { look };
            self.looks.insert(figure.id, look);
            self.holds.retain(|h| h.figure.id != figure.id);
            self.holds.push(Hold { figure, look, gone, since: self.clock });
        }
    }

    /// Starts every waiting cue whose keys are free, in order.
    fn start_waiting(&mut self) {
        let mut busy: HashSet<Key> = self.running.iter().flat_map(|r| r.cue.keys()).collect();
        let mut index = 0;
        while index < self.waiting.len() {
            let keys = self.waiting[index].cue.keys();
            let free = keys.iter().all(|k| !busy.contains(k));
            busy.extend(keys);
            if !free {
                index += 1;
                continue;
            }
            let waiting = self.waiting.remove(index).expect("index in range");
            self.start(waiting);
        }
    }

    fn start(&mut self, waiting: Waiting) {
        let map = self.map;
        let mut entry = Vec::new();
        for figure in waiting.cue.figures() {
            let look = match self.looks.get(&figure.id) {
                Some(look) => *look,
                None => motion::first_look(&waiting.cue, &figure, map).unwrap_or(Look::resting(map, TileId(0))),
            };
            self.holds.retain(|h| h.figure.id != figure.id);
            entry.push((figure.id, look));
        }
        let beats = motion::moments(&waiting.cue, map).into_iter().filter(|m| m.at <= 0.0).map(|m| m.beat);
        fire(beats, &mut self.sounds, &mut self.effects, &mut self.fx, &mut self.camera, self.clock, self.settings);
        let running = Running { cue: waiting.cue, speed: waiting.speed, elapsed: 0.0, entry };
        if running.cue.duration() <= 0.0 {
            return;
        }
        self.running.push(running);
    }

    /// Held units the projection now shows as the presentation left them are released.
    fn release_holds(&mut self, world: &RenderWorld) {
        if self.holds.is_empty() {
            return;
        }
        let shown: HashMap<u64, TileId> = world.units.iter().map(|u| (u.id, u.tile)).collect();
        let clock = self.clock;
        self.holds.retain(|hold| {
            let agrees = match (hold.gone, shown.get(&hold.figure.id.0)) {
                (true, None) | (false, None) => true,
                (true, Some(_)) => false,
                (false, Some(tile)) => *tile == hold.look.tile,
            };
            !agrees && clock - hold.since < HOLD_SECONDS
        });
    }

    /// Poses of every unit the presentation draws: running cues first, then units
    /// waiting for their cue, then held ones.
    fn poses(&mut self) -> Vec<UnitPose> {
        let mut looks = Looks::default();
        self.running_looks(&mut looks);
        self.waiting_looks(&mut looks);
        for hold in &self.holds {
            looks.add(hold.figure, hold.look);
        }
        self.looks = looks.list.iter().map(|(f, l)| (f.id, *l)).collect();
        let reduced = self.settings.reduced_motion;
        let shown = &self.shown_hp;
        let mut poses: Vec<UnitPose> = looks
            .list
            .into_iter()
            .map(|(figure, mut look)| {
                if look.hp == [0, 0] {
                    let hp = shown.get(&figure.id.0).copied().unwrap_or(100);
                    look.hp = [hp, hp];
                }
                pose(figure, look, reduced)
            })
            .collect();
        // Transport planes of paradrops and airlifts are poses of their own.
        for running in &self.running {
            poses.extend(air::extra_poses(&running.cue, running.elapsed, self.map));
        }
        poses
    }

    fn running_looks(&self, looks: &mut Looks) {
        for running in &self.running {
            for figure in running.cue.figures() {
                let entry = running.entry.iter().find(|(id, _)| *id == figure.id);
                let entry = entry.map_or(Look::resting(self.map, TileId(0)), |(_, look)| *look);
                if let Some(look) = motion::look(&running.cue, figure.id, running.elapsed, entry, self.map) {
                    looks.add(figure, look);
                }
            }
        }
    }

    /// A unit waiting for its first cue stands where the cue starts (or where it was).
    fn waiting_looks(&self, looks: &mut Looks) {
        for waiting in &self.waiting {
            for figure in waiting.cue.figures() {
                if looks.has(figure.id) {
                    continue;
                }
                let last = self.looks.get(&figure.id).copied();
                if let Some(look) = last.or_else(|| motion::first_look(&waiting.cue, &figure, self.map)) {
                    looks.add(figure, look);
                }
            }
        }
    }

    fn live_marks(&self) -> Vec<LiveMark> {
        self.effects
            .iter()
            .map(|e| {
                let progress = ((self.clock - e.born) as f32 / e.mark.seconds.max(1e-3)).clamp(0.0, 1.0);
                LiveMark { mark: e.mark.clone(), progress }
            })
            .collect()
    }
}

/// Looks of this frame, one per unit (the first added wins).
#[derive(Default)]
struct Looks {
    list: Vec<(Figure, Look)>,
    ids: HashSet<EntityId>,
}

impl Looks {
    fn add(&mut self, figure: Figure, look: Look) {
        if self.ids.insert(figure.id) {
            self.list.push((figure, look));
        }
    }

    fn has(&self, id: EntityId) -> bool {
        self.ids.contains(&id)
    }
}

fn fire(
    beats: impl Iterator<Item = Beat>,
    sounds: &mut Vec<Sound>,
    effects: &mut Vec<LiveEffect>,
    fx: &mut Vec<FxEvent>,
    camera: &mut Option<TileId>,
    clock: f64,
    settings: Settings,
) {
    for beat in beats {
        match beat {
            Beat::Sound(sound) => sounds.push(sound),
            Beat::Mark(mark) => effects.push(LiveEffect { mark, born: clock }),
            Beat::Fx(event) => fx.push(event),
            Beat::Camera(tile) if settings.camera_glides && !settings.reduced_motion => *camera = Some(tile),
            Beat::Camera(_) => {}
        }
    }
}

/// The renderer's pose of a look; reduced motion keeps positions and fades but drops
/// the walk cycle, lunges and recoils.
fn pose(figure: Figure, look: Look, reduced: bool) -> UnitPose {
    let gait = match (reduced, look.gait) {
        (true, Gait::Walk { .. } | Gait::Attack { .. } | Gait::Recoil { .. }) => Gait::Stand,
        (_, gait) => gait,
    };
    UnitPose {
        id: figure.id.0,
        kind: figure.kind,
        owner: figure.owner,
        tile: look.tile.0,
        position: look.position,
        facing: look.facing,
        gait,
        flash: look.flash,
        settle: look.settle,
        hp: look.hp,
        change: look.change,
    }
}
