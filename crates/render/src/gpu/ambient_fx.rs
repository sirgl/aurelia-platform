//! Ambient particle emitters of the map and the cues of pillaging and repair, on the effects
//! pool (`effects.rs`, platform/docs/animation.md).
//!
//! The pool has no per-frame update: a particle is written once and moves in closed form. An
//! emitter therefore works in *cycles*: when a cycle of a site comes within `LOOKAHEAD` seconds,
//! all its particles are written at once with their absolute birth times. Cycle starts, particle
//! times and randomness are pure functions of the tile, the cycle number and the effects clock, so
//! the same clock gives the same picture (and a clock that jumps, as in the gallery, only
//! re-spawns the cycles around it). Sites differ in phase and period by tile: no two move in
//! lockstep.
//!
//! Costs: only sites in the view frustum, at most `MAX_SITES` of them (the nearest to the view
//! centre), and only while the map is zoomed in enough for particles to read
//! (`MIN_PX_PER_UNIT`). Per-site budgets are a few dozen particles per cycle (see each
//! `cycle_*`). The shell turns the whole thing off with `set_enabled(false)` on Quality::Low
//! and with reduced motion: nothing then moves by itself.

use std::collections::HashMap;

use crate::gpu::effects::{Effects, Particle};
use crate::gpu::world_anim;
use crate::world_models::WorldModelId;

/// How far ahead (seconds) a cycle is written.
const LOOKAHEAD: f32 = 2.5;
/// Sites that emit at once.
pub const MAX_SITES: usize = 8;
/// Below this zoom (screen pixels per hex unit) ambient particles are skipped.
pub const MIN_PX_PER_UNIT: f32 = 12.0;
/// Most particles one site may write in one cycle.
pub const CYCLE_CAP: usize = 64;

/// What a site emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Emitter {
    /// A column of steam and a spray of drops in an eruption cycle, a faint vapour in the pause.
    Geyser,
    /// Dark smoke from a crater, a few sparks.
    Volcano,
    /// Glints rising from the water.
    Fountain,
    /// A thin smoke column from a fire.
    Hearth,
    /// Smoke and embers over a pillaged work.
    Ruin,
}

/// One place that emits: the world point of the source and the clock of its cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site {
    pub tile: u32,
    pub emitter: Emitter,
    pub pos: [f32; 3],
    /// Cycle length in seconds and the fraction of a cycle already elapsed at clock 0.
    pub period: f32,
    pub phase: f32,
}

impl Site {
    fn start(&self, cycle: i64) -> f32 {
        (cycle as f32 - self.phase) * self.period
    }
    fn cycle_at(&self, clock: f32) -> i64 {
        (clock / self.period + self.phase).floor() as i64
    }
}

/// A change of a tile's improvement the player watched happen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cue {
    /// A unit pillaged the work: flames, dust and sparks over it.
    Pillaged { pos: [f32; 3] },
    /// The work was repaired: a rising glitter.
    Repaired { pos: [f32; 3] },
}

#[derive(Debug, Default)]
pub struct Ambient {
    enabled: bool,
    sites: Vec<Site>,
    /// Last cycle written per (tile, emitter).
    written: HashMap<(u32, Emitter), i64>,
    last_clock: f32,
    cues: Vec<Cue>,
    /// Particles written by the last `update`.
    pub spawned: usize,
}

/// Small deterministic generator keyed by (tile, cycle, salt).
struct Rand(u32);

impl Rand {
    fn new(tile: u32, cycle: i64, salt: u32) -> Self {
        let mut h = tile.wrapping_mul(0x9E37_79B1) ^ (cycle as u32).wrapping_mul(0x85EB_CA6B) ^ salt.wrapping_mul(0xC2B2_AE35);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2C1B_3C6D);
        Self(h | 1)
    }
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / 16_777_216.0
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
    fn signed(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

const PUFF: f32 = 0.0;
const GLOW: f32 = 1.0;
const STREAK: f32 = 2.0;
const BALL: f32 = 5.0;

fn particle(kind: f32, pos: [f32; 3], born: f32, life: f32, vel: [f32; 3], accel: [f32; 3], size: [f32; 2], colors: ([u8; 4], [u8; 4]), drag: f32, floor: f32, rng: &mut Rand) -> Particle {
    Particle {
        pos,
        born,
        vel,
        life,
        accel,
        kind,
        size,
        drag,
        floor,
        color0: colors.0,
        color1: colors.1,
        seed: rng.next(),
        spin: rng.signed() * 0.6,
    }
}

fn faded(c: [u8; 4]) -> [u8; 4] {
    [c[0], c[1], c[2], 0]
}

impl Ambient {
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.written.clear();
        }
    }

    /// Forget what was written (the pool was emptied or the scene was replaced).
    pub fn restart(&mut self) {
        self.written.clear();
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn sites(&self) -> &[Site] {
        &self.sites
    }

    /// The emitters of the map as of the last build of the props; a work that became pillaged or
    /// repaired since (on a tile that was in sight both times) queues its cue.
    pub fn set_sites(&mut self, sites: Vec<Site>, cues: Vec<Cue>) {
        self.cues.extend(cues);
        self.written.retain(|key, _| sites.iter().any(|s| (s.tile, s.emitter) == *key));
        self.sites = sites;
    }

    /// Writes what is due at the effects clock. `visible` tells whether a world point is in the
    /// view frustum; `centre` is the view centre (xz) for choosing the nearest sites; `px_per_unit`
    /// the zoom.
    pub fn update(&mut self, effects: &mut Effects, visible: &dyn Fn([f32; 3]) -> bool, centre: [f32; 2], px_per_unit: f32) {
        self.spawned = 0;
        for cue in std::mem::take(&mut self.cues) {
            play_cue(effects, cue);
        }
        let now = effects.now();
        // A clock that went back (a gallery frame) or jumped: write the cycles around it again.
        if now < self.last_clock - 1e-3 || now > self.last_clock + 4.0 {
            self.written.clear();
        }
        self.last_clock = now;
        if !self.enabled || px_per_unit < MIN_PX_PER_UNIT || effects.mode() == crate::gpu::effects::EffectsMode::Off {
            return;
        }
        let mut near: Vec<(f32, Site)> = self
            .sites
            .iter()
            .filter(|s| visible(s.pos))
            .map(|s| ((s.pos[0] - centre[0]).hypot(s.pos[2] - centre[1]), *s))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, site) in near.into_iter().take(MAX_SITES) {
            let last = self.written.get(&(site.tile, site.emitter)).copied();
            // The cycle running now, the one that began before it (still in flight), and the next
            // ones that start within the lookahead.
            let first = last.map_or(site.cycle_at(now) - 1, |c| c + 1);
            let upto = site.cycle_at(now + LOOKAHEAD);
            for cycle in first..=upto {
                let before = effects.live();
                write_cycle(effects, &site, cycle);
                self.spawned += effects.live().saturating_sub(before);
                self.written.insert((site.tile, site.emitter), cycle);
            }
        }
    }
}

fn write_cycle(effects: &mut Effects, site: &Site, cycle: i64) {
    let start = site.start(cycle);
    let mut rng = Rand::new(site.tile, cycle, site.emitter as u32 + 1);
    let mut out: Vec<Particle> = Vec::with_capacity(CYCLE_CAP);
    match site.emitter {
        Emitter::Geyser => cycle_geyser(site, start, &mut rng, &mut out),
        Emitter::Volcano => cycle_volcano(site, start, &mut rng, &mut out),
        Emitter::Fountain => cycle_fountain(site, start, &mut rng, &mut out),
        Emitter::Hearth => cycle_hearth(site, start, &mut rng, &mut out),
        Emitter::Ruin => cycle_ruin(site, start, &mut rng, &mut out),
    }
    out.truncate(CYCLE_CAP);
    for p in out {
        effects.push(p);
    }
}

const STEAM: [u8; 4] = [236, 240, 246, 175];
const ASH: [u8; 4] = [74, 70, 68, 185];
const SMOKE: [u8; 4] = [150, 148, 146, 110];
const DROP: [u8; 4] = [205, 228, 255, 235];
const GLINT: [u8; 4] = [190, 255, 246, 255];
const EMBER: [u8; 4] = [255, 160, 60, 255];

/// Eruption: the column rises (u 0.05..0.30), holds (to 0.55), falls (to 0.85) and rests; the
/// timeline of `world_anim` / hex.wgsl, so the model's stub column and the particles agree.
fn cycle_geyser(site: &Site, start: f32, rng: &mut Rand, out: &mut Vec<Particle>) {
    let p = site.period;
    let envelope = |u: f32| {
        let up = ((u - 0.05) / 0.25).clamp(0.0, 1.0);
        let down = 1.0 - ((u - 0.55) / 0.30).clamp(0.0, 1.0);
        up * up * (3.0 - 2.0 * up) * down * down * (3.0 - 2.0 * down)
    };
    let [x, y, z] = site.pos;
    // The column: puffs thrown up while the jet is strong.
    for i in 0..40 {
        let u = 0.06 + 0.76 * (i as f32 + rng.next()) / 40.0;
        let e = envelope(u);
        if e < 0.08 {
            continue;
        }
        let a = rng.range(0.0, std::f32::consts::TAU);
        let lateral = rng.range(0.0, 0.07);
        out.push(particle(
            PUFF,
            [x + a.cos() * lateral, y, z + a.sin() * lateral],
            start + u * p,
            rng.range(1.7, 2.3),
            [0.05 + rng.signed() * 0.05, 0.5 + 1.4 * e, rng.signed() * 0.05],
            [0.06, 0.15, 0.0],
            [0.11, rng.range(0.42, 0.55)],
            (STEAM, faded(STEAM)),
            0.5,
            y,
            rng,
        ));
    }
    // The spray: drops on ballistic arcs while the jet is high.
    for _ in 0..16 {
        let u = rng.range(0.12, 0.62);
        let e = envelope(u);
        if e < 0.2 {
            continue;
        }
        let vy = rng.range(1.2, 2.6) * (0.5 + 0.5 * e);
        let g = 4.2;
        let a = rng.range(0.0, std::f32::consts::TAU);
        let s = rng.range(0.1, 0.45);
        out.push(particle(
            BALL,
            [x, y + 0.05, z],
            start + u * p,
            2.0 * vy / g + 0.05,
            [a.cos() * s, vy, a.sin() * s],
            [0.0, -g, 0.0],
            [0.024, 0.02],
            (DROP, [DROP[0], DROP[1], DROP[2], 120]),
            0.0,
            y,
            rng,
        ));
    }
    // The pause: a faint vapour over the vent.
    for _ in 0..5 {
        let u = rng.range(0.86, 1.05);
        out.push(particle(
            PUFF,
            [x + rng.signed() * 0.04, y, z + rng.signed() * 0.04],
            start + u * p,
            rng.range(1.4, 2.0),
            [0.03, 0.14, 0.0],
            [0.0; 3],
            [0.04, 0.13],
            ([STEAM[0], STEAM[1], STEAM[2], 80], faded(STEAM)),
            0.4,
            y,
            rng,
        ));
    }
}

/// A crater: ash smoke rising and leaning in the wind, now and then a spark.
fn cycle_volcano(site: &Site, start: f32, rng: &mut Rand, out: &mut Vec<Particle>) {
    let p = site.period;
    let [x, y, z] = site.pos;
    for i in 0..16 {
        let u = (i as f32 + rng.next()) / 16.0;
        out.push(particle(
            PUFF,
            [x + rng.signed() * 0.05, y, z + rng.signed() * 0.05],
            start + u * p,
            rng.range(2.8, 3.6),
            [0.10 + rng.signed() * 0.03, rng.range(0.45, 0.65), rng.signed() * 0.04],
            [0.04, 0.0, 0.0],
            [0.14, rng.range(0.55, 0.75)],
            (ASH, faded(ASH)),
            0.35,
            y,
            rng,
        ));
    }
    for _ in 0..4 {
        let u = rng.range(0.0, 1.0);
        let a = rng.range(0.0, std::f32::consts::TAU);
        let s = rng.range(0.1, 0.4);
        out.push(particle(
            STREAK,
            [x, y, z],
            start + u * p,
            rng.range(0.9, 1.4),
            [a.cos() * s, rng.range(0.7, 1.3), a.sin() * s],
            [0.0, -1.6, 0.0],
            [0.010, 0.08],
            (EMBER, [255, 70, 20, 0]),
            0.0,
            y - 0.4,
            rng,
        ));
    }
}

/// Glints rising off the water.
fn cycle_fountain(site: &Site, start: f32, rng: &mut Rand, out: &mut Vec<Particle>) {
    let p = site.period;
    let [x, y, z] = site.pos;
    for _ in 0..16 {
        let u = rng.range(0.0, 1.0);
        let a = rng.range(0.0, std::f32::consts::TAU);
        let r = rng.range(0.0, 0.38).sqrt() * 0.62;
        out.push(particle(
            GLOW,
            [x + a.cos() * r, y, z + a.sin() * r],
            start + u * p,
            rng.range(1.0, 1.7),
            [0.0, rng.range(0.10, 0.26), 0.0],
            [0.0; 3],
            [0.05, 0.0],
            (GLINT, faded(GLINT)),
            0.0,
            y,
            rng,
        ));
    }
}

/// A thin column from a fire.
fn cycle_hearth(site: &Site, start: f32, rng: &mut Rand, out: &mut Vec<Particle>) {
    let p = site.period;
    let [x, y, z] = site.pos;
    for i in 0..5 {
        let u = (i as f32 + rng.next()) / 5.0;
        out.push(particle(
            PUFF,
            [x + rng.signed() * 0.02, y, z + rng.signed() * 0.02],
            start + u * p,
            rng.range(2.0, 2.8),
            [0.05, rng.range(0.20, 0.30), rng.signed() * 0.02],
            [0.02, 0.0, 0.0],
            [0.03, rng.range(0.10, 0.16)],
            (SMOKE, faded(SMOKE)),
            0.3,
            y,
            rng,
        ));
    }
}

/// A pillaged work: dark smoke and a few embers.
fn cycle_ruin(site: &Site, start: f32, rng: &mut Rand, out: &mut Vec<Particle>) {
    let p = site.period;
    let [x, y, z] = site.pos;
    for i in 0..5 {
        let u = (i as f32 + rng.next()) / 5.0;
        out.push(particle(
            PUFF,
            [x + rng.signed() * 0.12, y, z + rng.signed() * 0.12],
            start + u * p,
            rng.range(2.2, 3.0),
            [0.07, rng.range(0.22, 0.34), rng.signed() * 0.03],
            [0.02, 0.0, 0.0],
            [0.05, rng.range(0.16, 0.24)],
            ([60, 58, 56, 150], [60, 58, 56, 0]),
            0.3,
            y,
            rng,
        ));
    }
    for _ in 0..3 {
        let u = rng.range(0.0, 1.0);
        out.push(particle(
            GLOW,
            [x + rng.signed() * 0.15, y, z + rng.signed() * 0.15],
            start + u * p,
            rng.range(0.8, 1.3),
            [0.0, rng.range(0.10, 0.22), 0.0],
            [0.0; 3],
            [0.022, 0.0],
            (EMBER, [255, 60, 20, 0]),
            0.0,
            y,
            rng,
        ));
    }
}

/// The one-off effects of a pillage and a repair: through the pool's own calls, so the effects
/// mode (reduced motion keeps a flash and a puff) applies.
fn play_cue(effects: &mut Effects, cue: Cue) {
    match cue {
        Cue::Pillaged { pos } => {
            effects.spawn_fire(pos, 0.22, 1.6);
            effects.spawn_dust(pos, 0.35);
            let clock = effects.now();
            let mut rng = Rand::new(pos[0].to_bits() ^ pos[2].to_bits().rotate_left(7), (clock * 50.0) as i64, 91);
            let scale = if effects.mode() == crate::gpu::effects::EffectsMode::Reduced { 3 } else { 9 };
            for _ in 0..scale {
                let a = rng.range(0.0, std::f32::consts::TAU);
                let s = rng.range(0.15, 0.5);
                effects.push(particle(
                    STREAK,
                    [pos[0], pos[1] + 0.1, pos[2]],
                    clock + rng.range(0.0, 0.15),
                    rng.range(0.7, 1.2),
                    [a.cos() * s, rng.range(0.8, 1.6), a.sin() * s],
                    [0.0, -2.2, 0.0],
                    [0.010, 0.08],
                    (EMBER, [255, 70, 20, 0]),
                    0.0,
                    pos[1],
                    &mut rng,
                ));
            }
        }
        Cue::Repaired { pos } => {
            let clock = effects.now();
            let mut rng = Rand::new(pos[0].to_bits() ^ pos[2].to_bits().rotate_left(11), (clock * 50.0) as i64, 17);
            let n = if effects.mode() == crate::gpu::effects::EffectsMode::Reduced { 4 } else { 16 };
            for _ in 0..n {
                let a = rng.range(0.0, std::f32::consts::TAU);
                let r = rng.range(0.0, 0.3);
                effects.push(particle(
                    GLOW,
                    [pos[0] + a.cos() * r, pos[1] + 0.03, pos[2] + a.sin() * r],
                    clock + rng.range(0.0, 0.5),
                    rng.range(0.9, 1.5),
                    [0.0, rng.range(0.25, 0.5), 0.0],
                    [0.0; 3],
                    [0.04, 0.0],
                    ([255, 232, 140, 255], [255, 232, 140, 0]),
                    0.0,
                    pos[1],
                    &mut rng,
                ));
            }
            effects.spawn_dust(pos, 0.18);
        }
    }
}

/// The emitter of a prop model and the source point in model space (None: it emits nothing).
pub fn emitter_of(model: WorldModelId) -> Option<(Emitter, [f32; 3])> {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Vec<Option<(Emitter, [f32; 3])>>> = OnceLock::new();
    TABLE.get_or_init(|| (0..crate::world_models::models().len()).map(|i| scan(WorldModelId(i as u16))).collect())[model.0 as usize]
}

fn scan(model: WorldModelId) -> Option<(Emitter, [f32; 3])> {
    use crate::world_models::WorldKind;
    let m = model.model();
    if !matches!(m.kind, WorldKind::NaturalWonder | WorldKind::Improvement) {
        return None;
    }
    match (world_anim::kind_of(model), world_anim::named_emitter(model)) {
        (Some(world_anim::Kind::Geyser), _) => {
            let (low, _) = world_anim::steam_extent(model)?;
            Some((Emitter::Geyser, [0.0, low, 0.0]))
        }
        (_, Some("fountain")) => Some((Emitter::Fountain, [0.0, 0.30, 0.0])),
        _ => {
            // By material: a model with lava has a crater, one with fire a hearth.
            let hot = |colour: [f32; 3]| -> Option<[f32; 3]> {
                let hits: Vec<[f32; 3]> = model
                    .mesh(0)
                    .iter()
                    .filter(|v| world_anim::is_color(v.color, colour))
                    .map(|v| v.pos)
                    .collect();
                let top = hits.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
                let high: Vec<&[f32; 3]> = hits.iter().filter(|p| p[1] > top - 0.08).collect();
                (!high.is_empty()).then(|| {
                    let n = high.len() as f32;
                    [high.iter().map(|p| p[0]).sum::<f32>() / n, top + 0.02, high.iter().map(|p| p[2]).sum::<f32>() / n]
                })
            };
            if let Some(at) = hot(world_anim::LAVA) {
                return Some((Emitter::Volcano, at));
            }
            hot(world_anim::FIRE).map(|at| (Emitter::Hearth, at))
        }
    }
}

/// Cycle length and phase of a site: the geyser follows its model's own clock (the shader
/// reads the same `anim`), the others have a period by kind and a phase by tile.
pub fn clock_of(emitter: Emitter, anim: [f32; 4], tile: u32) -> (f32, f32) {
    let h = crate::scene::terrain::fields::hash_u32(tile.wrapping_mul(0x2545_F491) ^ 0x6A09);
    let phase = (h & 0xFFFF) as f32 / 65536.0;
    let jitter = 0.9 + 0.2 * ((h >> 16) & 0xFF) as f32 / 255.0;
    match emitter {
        Emitter::Geyser if anim[3] > 0.0 => (std::f32::consts::TAU / anim[3], anim[1] / std::f32::consts::TAU),
        Emitter::Geyser => (12.0 * jitter, phase),
        Emitter::Volcano => (6.0 * jitter, phase),
        Emitter::Fountain => (3.0 * jitter, phase),
        Emitter::Hearth => (4.0 * jitter, phase),
        Emitter::Ruin => (5.0 * jitter, phase),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(emitter: Emitter, tile: u32) -> Site {
        let (period, phase) = clock_of(emitter, [0.0; 4], tile);
        Site { tile, emitter, pos: [1.0, 0.4, 2.0], period, phase }
    }

    fn run(emitter: Emitter, tile: u32, clock: f32) -> Vec<Particle> {
        let mut effects = Effects::new();
        effects.advance(clock);
        let mut ambient = Ambient::default();
        ambient.set_enabled(true);
        ambient.set_sites(vec![site(emitter, tile)], Vec::new());
        ambient.update(&mut effects, &|_| true, [1.0, 2.0], 100.0);
        effects.particles().to_vec()
    }

    #[test]
    fn same_clock_same_particles() {
        for emitter in [Emitter::Geyser, Emitter::Volcano, Emitter::Fountain, Emitter::Hearth, Emitter::Ruin] {
            assert_eq!(run(emitter, 40, 17.3), run(emitter, 40, 17.3), "{emitter:?}");
        }
    }

    #[test]
    fn tiles_do_not_move_in_lockstep() {
        let a: Vec<f32> = run(Emitter::Geyser, 3, 20.0).iter().map(|p| p.born).collect();
        let b: Vec<f32> = run(Emitter::Geyser, 4, 20.0).iter().map(|p| p.born).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn a_site_stays_within_its_budget() {
        for emitter in [Emitter::Geyser, Emitter::Volcano, Emitter::Fountain, Emitter::Hearth, Emitter::Ruin] {
            for clock in [0.0, 3.3, 9.9, 31.0] {
                let n = run(emitter, 9, clock).len();
                assert!(n <= CYCLE_CAP * 5, "{emitter:?} at {clock}: {n} particles");
            }
        }
    }

    #[test]
    fn disabled_or_zoomed_out_writes_nothing() {
        let mut effects = Effects::new();
        let mut ambient = Ambient::default();
        ambient.set_sites(vec![site(Emitter::Geyser, 1)], Vec::new());
        ambient.update(&mut effects, &|_| true, [0.0, 0.0], 100.0);
        assert!(effects.particles().is_empty(), "off by default");
        ambient.set_enabled(true);
        ambient.update(&mut effects, &|_| true, [0.0, 0.0], MIN_PX_PER_UNIT - 1.0);
        assert!(effects.particles().is_empty(), "too far out");
    }

    #[test]
    fn steady_frames_write_each_cycle_once() {
        let mut effects = Effects::new();
        let mut ambient = Ambient::default();
        ambient.set_enabled(true);
        ambient.set_sites(vec![site(Emitter::Fountain, 5)], Vec::new());
        ambient.update(&mut effects, &|_| true, [0.0, 0.0], 100.0);
        let first = effects.particles().len();
        for _ in 0..10 {
            effects.advance(0.016);
            ambient.update(&mut effects, &|_| true, [0.0, 0.0], 100.0);
        }
        assert_eq!(effects.particles().len(), first, "no cycle is due yet");
    }

    #[test]
    fn the_geyser_is_found_in_the_model() {
        let model = (0..crate::world_models::models().len() as u16).map(WorldModelId).find(|m| m.model().id == "natural-wonder/old-faithful").unwrap();
        assert_eq!(emitter_of(model).map(|e| e.0), Some(Emitter::Geyser));
    }
}
