//! Effects: explosions, projectiles, muzzle flashes, smoke, fire and dust.
//!
//! A small particle system for the scene pass, made for an integrated GPU: a fixed pool
//! (`CAPACITY` particles, a ring: the oldest is overwritten, nothing is allocated after
//! `new`), one instanced draw, no per-frame particle update. A particle is written once
//! when it is spawned and moves as a closed-form function of the effects clock (position
//! from its velocity, drag and acceleration; size and colour from its age), evaluated in
//! the vertex shader (`platform/assets/shaders/effects.wgsl`). A delayed birth (`born` in the
//! future) gives trails and bursts without an emitter.
//!
//! Callers spawn through `Effects` (or `Effects::play` with an `FxEvent`, the form the
//! presentation emits). Positions are world coordinates (x, y up, z); the `*_xz` helpers
//! and `play` take a ground height function. `advance` moves the clock; `mode` makes the
//! system cheaper (reduced motion, low quality) or silent. Everything here is
//! deterministic: the same spawns at the same clock give the same particles.
//!
//! The CPU side is plain data (`Effects`, testable without a GPU); `EffectsGpu` owns the
//! pipeline and buffers.

use crate::gpu::context::GpuCore;
use crate::gpu::memory::{Category, GpuBuffer};

/// Particles in the pool.
pub const CAPACITY: usize = 2048;

/// What a particle looks like (the shader's `kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Shape {
    /// Soft smoke or dust blob (alpha blended).
    Puff = 0,
    /// Additive soft glow (flashes).
    Glow = 1,
    /// Additive streak along the velocity (sparks, tracers).
    Streak = 2,
    /// Ring flat on the ground (shock wave, dust ring).
    Ring = 3,
    /// Arrow or bolt along the velocity.
    Arrow = 4,
    /// Solid ball (shell, stone).
    Ball = 5,
    /// Hard-edged chip (debris).
    Chip = 6,
    /// Dark disc flat on the ground (scorch mark).
    Scorch = 7,
    /// Additive flame.
    Flame = 8,
}

/// One particle as the GPU reads it (80 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Particle {
    pub pos: [f32; 3],
    /// Clock time of birth (the particle is invisible before it).
    pub born: f32,
    pub vel: [f32; 3],
    pub life: f32,
    pub accel: [f32; 3],
    pub kind: f32,
    /// Size at birth and at the end of life (streaks and arrows: half width, length).
    pub size: [f32; 2],
    /// Velocity damping per second (0: none).
    pub drag: f32,
    /// The particle never goes below this height (debris stops on the ground).
    pub floor: f32,
    /// sRGB colour and alpha at birth and at the end of life.
    pub color0: [u8; 4],
    pub color1: [u8; 4],
    pub seed: f32,
    /// Billboard spin in radians per second.
    pub spin: f32,
}

impl Particle {
    /// Where the particle is `t` seconds after its birth (the shader's formula).
    pub fn position_at(&self, t: f32) -> [f32; 3] {
        let s = if self.drag > 1e-3 {
            (1.0 - (-self.drag * t).exp()) / self.drag
        } else {
            t
        };
        let mut p = [0.0; 3];
        for i in 0..3 {
            p[i] = self.pos[i] + self.vel[i] * s + 0.5 * self.accel[i] * t * t;
        }
        p[1] = p[1].max(self.floor);
        p
    }

    /// Clock time at which the particle is gone.
    pub fn dies(&self) -> f32 {
        self.born + self.life
    }

    #[cfg(test)]
    fn shape(&self) -> u8 {
        self.kind as u8
    }
}

/// How much the system does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EffectsMode {
    /// Everything: debris, sparks, smoke trails, scorch marks.
    #[default]
    Full,
    /// Reduced motion / low quality: a flash and a puff per event, projectiles without
    /// trails; no debris, sparks or lingering smoke.
    Reduced,
    /// Nothing is spawned (benchmarks, tests).
    Off,
}

impl EffectsMode {
    fn scale(self) -> f32 {
        match self {
            EffectsMode::Full => 1.0,
            EffectsMode::Reduced => 0.35,
            EffectsMode::Off => 0.0,
        }
    }
}

/// How big and how violent an explosion is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplosionKind {
    /// A shell or rocket: fireball, smoke, sparks, debris, a dust ring.
    Shell,
    /// A bomb or missile warhead: a bigger fireball, two rings, more smoke.
    Bomb,
    /// A blast of a siege stone or a ram breaking a gate: dust and debris, hardly any fire.
    Rubble,
    /// A nuclear blast: a white flash, a huge fireball and a column of smoke.
    Nuclear,
}

/// What flies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectileKind {
    /// An arrow on a high arc.
    Arrow,
    /// A crossbow bolt: shorter, flatter.
    Bolt,
    /// A bullet: a fast tracer on a flat line.
    Bullet,
    /// A cannon ball or shell with a smoke trail.
    Shell,
    /// A rocket: a flame and a smoke trail.
    Rocket,
    /// A thrown or catapulted stone on a high arc.
    Stone,
    /// A bomb falling from an aircraft.
    Bomb,
    /// A cruise missile: low, with a fiery trail.
    Missile,
}

/// What kind of weapon fired (muzzle flash and smoke).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuzzleKind {
    /// Musket, rifle, machine gun: a small flash and a puff.
    Rifle,
    /// Cannon, artillery, tank, anti-tank gun: a big flash, a smoke cloud, dust kicked up.
    Cannon,
    /// Ship guns: like Cannon, more smoke.
    Broadside,
    /// A rocket launcher: a back blast.
    Rocket,
}

/// What an impact is made of (small effects where a hit lands without an explosion).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactKind {
    /// An arrow or bolt hits: a dust puff and a few chips.
    Arrow,
    /// Steel on steel: sparks.
    Blade,
    /// A blow without a weapon (clubs, charge): dust.
    Blunt,
    /// A bullet: a spark and a puff.
    Bullet,
}

/// What the presentation emits; `Effects::play` turns it into particles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FxEvent {
    Muzzle {
        at: [f32; 2],
        toward: [f32; 2],
        kind: MuzzleKind,
    },
    Projectile {
        from: [f32; 2],
        to: [f32; 2],
        kind: ProjectileKind,
        seconds: f32,
    },
    Explosion {
        at: [f32; 2],
        size: f32,
        kind: ExplosionKind,
    },
    Impact {
        at: [f32; 2],
        kind: ImpactKind,
    },
    Smoke {
        at: [f32; 2],
        size: f32,
        seconds: f32,
    },
    Fire {
        at: [f32; 2],
        size: f32,
        seconds: f32,
    },
    Dust {
        at: [f32; 2],
        size: f32,
    },
    /// The air war: trails, bombs from altitude, flak, canopies, the nuclear blast
    /// (`effects_air.rs`).
    Air(AirFx),
}

/// Height above the ground where a unit's weapon is (muzzle, bow, hand).
pub const MUZZLE_HEIGHT: f32 = 0.26;

/// The effect pool: spawn here, `advance` every frame, draw with `EffectsGpu`.
#[derive(Debug, Clone)]
pub struct Effects {
    particles: Vec<Particle>,
    /// Next slot to overwrite.
    next: usize,
    /// Slots ever written (the draw covers the live prefix).
    used: usize,
    clock: f32,
    mode: EffectsMode,
    /// Counts spawns: the seed of their randomness.
    spawned: u32,
    /// Slots changed since the last upload: (first, last+1).
    dirty: Option<(usize, usize)>,
}

impl Default for Effects {
    fn default() -> Self {
        Self::new()
    }
}

/// Small deterministic generator (xorshift32).
struct Rng(std::cell::Cell<u32>);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(std::cell::Cell::new(
            seed.wrapping_mul(2_654_435_761).wrapping_add(0x9E37_79B9) | 1,
        ))
    }

    fn next(&self) -> f32 {
        let mut x = self.0.get();
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0.set(x);
        (x >> 8) as f32 / 16_777_216.0
    }

    /// In `lo..hi`.
    fn range(&self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }

    fn signed(&self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
    [r, g, b, a]
}

const SMOKE_GREY: [u8; 4] = rgba(150, 146, 140, 150);
const SMOKE_DARK: [u8; 4] = rgba(92, 86, 80, 175);
const DUST: [u8; 4] = rgba(190, 166, 124, 130);
const FLASH: [u8; 4] = rgba(255, 238, 190, 255);
const FIRE_HOT: [u8; 4] = rgba(255, 190, 70, 235);
const FIRE_COOL: [u8; 4] = rgba(200, 60, 24, 0);

fn fade(color: [u8; 4]) -> [u8; 4] {
    [color[0], color[1], color[2], 0]
}

impl Effects {
    pub fn new() -> Self {
        Self {
            particles: vec![bytemuck::Zeroable::zeroed(); CAPACITY],
            next: 0,
            used: 0,
            clock: 0.0,
            mode: EffectsMode::Full,
            spawned: 0,
            dirty: None,
        }
    }

    pub fn mode(&self) -> EffectsMode {
        self.mode
    }

    /// Reduced and Off also drop what is alive when they are set from Full.
    pub fn set_mode(&mut self, mode: EffectsMode) {
        if mode != self.mode && mode == EffectsMode::Off {
            self.clear();
        }
        self.mode = mode;
    }

    /// The effects clock, seconds.
    pub fn now(&self) -> f32 {
        self.clock
    }

    pub fn advance(&mut self, seconds: f32) {
        if seconds.is_finite() && seconds > 0.0 {
            self.clock += seconds;
        }
    }

    /// Removes every particle.
    pub fn clear(&mut self) {
        for p in &mut self.particles[..self.used] {
            p.life = 0.0;
        }
        if self.used > 0 {
            self.dirty = Some((0, self.used));
        }
    }

    /// Particles alive or waiting to be born at the current clock.
    pub fn live(&self) -> usize {
        self.particles[..self.used]
            .iter()
            .filter(|p| p.life > 0.0 && p.dies() > self.clock)
            .count()
    }

    /// Slots to draw: up to the last particle that is not yet dead.
    pub fn draw_count(&self) -> u32 {
        let last = self.particles[..self.used]
            .iter()
            .rposition(|p| p.life > 0.0 && p.dies() > self.clock);
        last.map_or(0, |i| i as u32 + 1)
    }

    pub fn particles(&self) -> &[Particle] {
        &self.particles[..self.used]
    }

    /// Slots written since the last call, for the upload.
    pub fn take_dirty(&mut self) -> Option<(usize, usize)> {
        self.dirty.take()
    }

    /// Writes one particle as it is (the ambient emitters build theirs: `ambient_fx.rs`).
    pub fn push(&mut self, p: Particle) {
        let slot = self.next;
        self.particles[slot] = p;
        self.next = (self.next + 1) % CAPACITY;
        self.used = self.used.max(slot + 1);
        self.dirty = Some(match self.dirty {
            Some((a, b)) => (a.min(slot), b.max(slot + 1)),
            None => (slot, slot + 1),
        });
    }

    fn rng(&mut self) -> Rng {
        self.spawned = self.spawned.wrapping_add(1);
        Rng::new(self.spawned)
    }

    /// One particle born `delay` seconds from now.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        rng: &Rng,
        shape: Shape,
        pos: [f32; 3],
        delay: f32,
        life: f32,
        vel: [f32; 3],
        accel: [f32; 3],
        size: [f32; 2],
        colors: ([u8; 4], [u8; 4]),
    ) {
        self.emit_with(
            rng,
            shape,
            pos,
            delay,
            life,
            vel,
            accel,
            size,
            colors,
            0.0,
            pos[1] - 1.0,
            0.0,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_with(
        &mut self,
        rng: &Rng,
        shape: Shape,
        pos: [f32; 3],
        delay: f32,
        life: f32,
        vel: [f32; 3],
        accel: [f32; 3],
        size: [f32; 2],
        colors: ([u8; 4], [u8; 4]),
        drag: f32,
        floor: f32,
        spin: f32,
    ) {
        self.push(Particle {
            pos,
            born: self.clock + delay.max(0.0),
            vel,
            life,
            accel,
            kind: shape as u8 as f32,
            size,
            drag,
            floor,
            color0: colors.0,
            color1: colors.1,
            seed: rng.next(),
            spin,
        });
    }

    /// An explosion at `at` (the ground point) of radius about `size` world units.
    pub fn spawn_explosion(&mut self, at: [f32; 3], size: f32, kind: ExplosionKind) {
        let scale = self.mode.scale();
        if scale <= 0.0 {
            return;
        }
        let rng = self.rng();
        let s = size.max(0.05);
        let full = self.mode == EffectsMode::Full;
        let (fire, smoke, rings, sparks, chips) = match kind {
            ExplosionKind::Shell => (6, 8, 1, 10, 7),
            ExplosionKind::Bomb => (10, 14, 2, 16, 12),
            ExplosionKind::Rubble => (1, 10, 1, 4, 14),
            ExplosionKind::Nuclear => (16, 24, 3, 0, 0),
        };
        let count = |n: u32| ((n as f32 * scale).round() as u32).max(1);
        // The flash.
        let flash_size = match kind {
            ExplosionKind::Nuclear => s * 3.0,
            ExplosionKind::Rubble => s * 1.1,
            _ => s * 1.7,
        };
        let flash_life = if kind == ExplosionKind::Nuclear {
            0.6
        } else {
            0.16
        };
        self.emit(
            &rng,
            Shape::Glow,
            [at[0], at[1] + s * 0.35, at[2]],
            0.0,
            flash_life,
            [0.0; 3],
            [0.0; 3],
            [flash_size, flash_size * 1.5],
            (FLASH, fade(FLASH)),
        );
        // The fireball: flames that rise out of the blast.
        for _ in 0..count(fire) {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let r = rng.range(0.0, s * 0.45);
            let pos = [at[0] + a.cos() * r, at[1] + 0.02, at[2] + a.sin() * r];
            let life = rng.range(0.35, 0.7)
                * if kind == ExplosionKind::Nuclear {
                    2.5
                } else {
                    1.0
                };
            let vel = [
                a.cos() * s * 0.4,
                rng.range(0.5, 1.3) * s,
                a.sin() * s * 0.4,
            ];
            let size = [s * rng.range(0.5, 0.8), s * rng.range(0.9, 1.3)];
            self.emit_with(
                &rng,
                Shape::Flame,
                pos,
                rng.range(0.0, 0.08),
                life,
                vel,
                [0.0, -0.6, 0.0],
                size,
                (FIRE_HOT, FIRE_COOL),
                2.0,
                pos[1],
                0.0,
            );
        }
        // Smoke after the fire, drifting up.
        for _ in 0..count(smoke) {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let r = rng.range(0.0, s * 0.5);
            let pos = [at[0] + a.cos() * r, at[1] + s * 0.2, at[2] + a.sin() * r];
            let rise = if kind == ExplosionKind::Nuclear {
                1.8
            } else {
                0.8
            };
            let vel = [
                a.cos() * s * 0.3,
                rng.range(0.4, 0.9) * s * rise,
                a.sin() * s * 0.3,
            ];
            let life = rng.range(1.1, 2.0)
                * if kind == ExplosionKind::Nuclear {
                    2.0
                } else {
                    1.0
                };
            let colors = if rng.next() < 0.5 {
                (SMOKE_DARK, fade(SMOKE_DARK))
            } else {
                (SMOKE_GREY, fade(SMOKE_GREY))
            };
            self.emit_with(
                &rng,
                Shape::Puff,
                pos,
                rng.range(0.05, 0.3),
                life,
                vel,
                [0.0; 3],
                [s * 0.4, s * rng.range(1.0, 1.5)],
                colors,
                1.2,
                pos[1] - 1.0,
                rng.signed() * 0.4,
            );
        }
        // Dust ring on the ground.
        for i in 0..rings {
            self.emit(
                &rng,
                Shape::Ring,
                [at[0], at[1], at[2]],
                0.04 * i as f32,
                0.7 + 0.1 * i as f32,
                [0.0; 3],
                [0.0; 3],
                [s * 0.4, s * (2.0 + i as f32 * 0.8)],
                (DUST, fade(DUST)),
            );
        }
        if !full {
            return;
        }
        // Sparks.
        for _ in 0..sparks {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let speed = rng.range(2.0, 4.2) * s.max(0.4);
            let up = rng.range(1.2, 3.2);
            let vel = [a.cos() * speed, up, a.sin() * speed];
            self.emit_with(
                &rng,
                Shape::Streak,
                [at[0], at[1] + 0.1, at[2]],
                0.0,
                rng.range(0.3, 0.6),
                vel,
                [0.0, -9.0, 0.0],
                [0.012, 0.16],
                (FIRE_HOT, fade(FIRE_HOT)),
                0.0,
                at[1],
                0.0,
            );
        }
        // Debris.
        for _ in 0..chips {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let speed = rng.range(0.8, 2.4) * s.max(0.4);
            let vel = [a.cos() * speed, rng.range(1.5, 3.4), a.sin() * speed];
            let shade = (rng.range(46.0, 90.0)) as u8;
            let chip = rgba(shade, shade - 6, shade - 14, 255);
            self.emit_with(
                &rng,
                Shape::Chip,
                [at[0], at[1] + 0.1, at[2]],
                0.0,
                rng.range(0.55, 0.9),
                vel,
                [0.0, -9.5, 0.0],
                [rng.range(0.018, 0.04), 0.0],
                (chip, chip),
                0.0,
                at[1],
                0.0,
            );
        }
        // The scorch mark that stays a while.
        if kind != ExplosionKind::Rubble {
            self.emit(
                &rng,
                Shape::Scorch,
                [at[0], at[1], at[2]],
                0.05,
                7.0,
                [0.0; 3],
                [0.0; 3],
                [s * 0.9, s * 1.1],
                (rgba(30, 26, 22, 80), rgba(30, 26, 22, 0)),
            );
        }
    }

    /// A projectile from `from` to `to` that arrives `seconds` after now. Returns nothing:
    /// the caller spawns the impact (`spawn_explosion`, `spawn_impact`) at the arrival.
    pub fn spawn_projectile(
        &mut self,
        from: [f32; 3],
        to: [f32; 3],
        kind: ProjectileKind,
        seconds: f32,
    ) {
        if self.mode == EffectsMode::Off {
            return;
        }
        let t = seconds.max(0.05);
        let rng = self.rng();
        let distance = (to[0] - from[0]).hypot(to[2] - from[2]);
        // Extra height of the arc's apex over the straight line, then gravity from it.
        let (shape, apex, size, colors, trail) = match kind {
            ProjectileKind::Arrow => (
                Shape::Arrow,
                0.12 + 0.28 * distance,
                [0.011, 0.24],
                (rgba(210, 190, 150, 255), rgba(210, 190, 150, 255)),
                false,
            ),
            ProjectileKind::Bolt => (
                Shape::Arrow,
                0.05 + 0.1 * distance,
                [0.012, 0.17],
                (rgba(70, 56, 44, 255), rgba(70, 56, 44, 255)),
                false,
            ),
            ProjectileKind::Bullet => (
                Shape::Streak,
                0.01,
                [0.01, 0.3],
                (rgba(255, 230, 150, 255), rgba(255, 210, 120, 255)),
                false,
            ),
            ProjectileKind::Shell => (
                Shape::Ball,
                0.08 + 0.12 * distance,
                [0.05, 0.05],
                (rgba(34, 32, 30, 255), rgba(34, 32, 30, 255)),
                true,
            ),
            ProjectileKind::Rocket => (
                Shape::Streak,
                0.06 + 0.1 * distance,
                [0.025, 0.28],
                (rgba(255, 190, 100, 255), rgba(255, 140, 60, 255)),
                true,
            ),
            ProjectileKind::Stone => (
                Shape::Ball,
                0.3 + 0.5 * distance,
                [0.075, 0.075],
                (rgba(112, 104, 94, 255), rgba(112, 104, 94, 255)),
                false,
            ),
            ProjectileKind::Bomb => (
                Shape::Ball,
                0.0,
                [0.06, 0.06],
                (rgba(30, 30, 32, 255), rgba(30, 30, 32, 255)),
                false,
            ),
            ProjectileKind::Missile => (
                Shape::Streak,
                0.1 + 0.05 * distance,
                [0.03, 0.4],
                (rgba(245, 245, 240, 255), rgba(245, 245, 240, 255)),
                true,
            ),
        };
        let gravity = 8.0 * apex / (t * t);
        let vel = [
            (to[0] - from[0]) / t,
            (to[1] - from[1]) / t + 0.5 * gravity * t,
            (to[2] - from[2]) / t,
        ];
        let accel = [0.0, -gravity, 0.0];
        self.emit(&rng, shape, from, 0.0, t, vel, accel, size, colors);
        // A faint glow on the fast ones.
        if matches!(
            kind,
            ProjectileKind::Bullet | ProjectileKind::Rocket | ProjectileKind::Missile
        ) {
            self.emit(
                &rng,
                Shape::Glow,
                from,
                0.0,
                t,
                vel,
                accel,
                [0.05, 0.05],
                (rgba(255, 200, 120, 160), rgba(255, 170, 90, 160)),
            );
        }
        if trail && self.mode == EffectsMode::Full {
            let puffs = ((t / 0.035).ceil() as usize).clamp(3, 28);
            let fiery = matches!(kind, ProjectileKind::Rocket | ProjectileKind::Missile);
            for k in 0..puffs {
                let at = t * k as f32 / puffs as f32;
                let probe = Particle {
                    pos: from,
                    born: 0.0,
                    vel,
                    life: t,
                    accel,
                    kind: 0.0,
                    size: [0.0; 2],
                    drag: 0.0,
                    floor: -10.0,
                    color0: [0; 4],
                    color1: [0; 4],
                    seed: 0.0,
                    spin: 0.0,
                };
                let p = probe.position_at(at);
                let (c0, c1) = if fiery {
                    (rgba(255, 170, 80, 200), rgba(120, 118, 114, 0))
                } else {
                    (rgba(150, 148, 144, 120), rgba(150, 148, 144, 0))
                };
                self.emit_with(
                    &rng,
                    Shape::Puff,
                    p,
                    at,
                    rng.range(0.4, 0.7),
                    [rng.signed() * 0.05, 0.12, rng.signed() * 0.05],
                    [0.0; 3],
                    [0.04, 0.13],
                    (c0, c1),
                    0.5,
                    p[1] - 1.0,
                    0.0,
                );
            }
        }
    }

    /// The flash and smoke of a weapon firing at `at` (muzzle position) toward the XZ
    /// direction `toward`.
    pub fn spawn_muzzle(&mut self, at: [f32; 3], toward: [f32; 2], kind: MuzzleKind) {
        let scale = self.mode.scale();
        if scale <= 0.0 {
            return;
        }
        let len = toward[0].hypot(toward[1]);
        let dir = if len > 1e-5 {
            [toward[0] / len, toward[1] / len]
        } else {
            [0.0, 1.0]
        };
        let rng = self.rng();
        let (flash, smoke, reach, sparks) = match kind {
            MuzzleKind::Rifle => (0.11, 3, 0.25, 3),
            MuzzleKind::Cannon => (0.24, 6, 0.55, 6),
            MuzzleKind::Broadside => (0.30, 9, 0.7, 7),
            MuzzleKind::Rocket => (0.22, 7, -0.6, 5),
        };
        let front = [at[0] + dir[0] * 0.08, at[1], at[2] + dir[1] * 0.08];
        self.emit(
            &rng,
            Shape::Glow,
            front,
            0.0,
            0.1,
            [dir[0] * 0.6, 0.0, dir[1] * 0.6],
            [0.0; 3],
            [flash, flash * 1.6],
            (FLASH, fade(FLASH)),
        );
        let count = ((smoke as f32 * scale).round() as u32).max(1);
        for _ in 0..count {
            let spread = rng.signed() * 0.25;
            let v = rng.range(0.6, 1.6) * reach * 1.6;
            let vel = [
                dir[0] * v - dir[1] * spread,
                rng.range(0.1, 0.35),
                dir[1] * v + dir[0] * spread,
            ];
            let life = rng.range(0.9, 1.6);
            self.emit_with(
                &rng,
                Shape::Puff,
                front,
                rng.range(0.0, 0.06),
                life,
                vel,
                [0.0, 0.1, 0.0],
                [flash * 0.5, flash * rng.range(1.4, 2.4)],
                (rgba(176, 172, 166, 150), rgba(176, 172, 166, 0)),
                1.6,
                front[1] - 1.0,
                rng.signed() * 0.3,
            );
        }
        if self.mode == EffectsMode::Full {
            for _ in 0..sparks {
                let a = rng.signed() * 0.5;
                let sp = rng.range(1.6, 3.2);
                let vel = [
                    (dir[0] * a.cos() - dir[1] * a.sin()) * sp,
                    rng.range(0.0, 0.8),
                    (dir[1] * a.cos() + dir[0] * a.sin()) * sp,
                ];
                self.emit_with(
                    &rng,
                    Shape::Streak,
                    front,
                    0.0,
                    rng.range(0.1, 0.22),
                    vel,
                    [0.0, -6.0, 0.0],
                    [0.008, 0.1],
                    (FIRE_HOT, fade(FIRE_HOT)),
                    0.0,
                    at[1] - 0.3,
                    0.0,
                );
            }
            // A big gun kicks dust up around it.
            if matches!(kind, MuzzleKind::Cannon | MuzzleKind::Rocket) {
                let ground = at[1] - MUZZLE_HEIGHT;
                for k in 0..3 {
                    let a = rng.range(0.0, std::f32::consts::TAU);
                    let pos = [
                        at[0] + a.cos() * 0.08,
                        ground + 0.02,
                        at[2] + a.sin() * 0.08,
                    ];
                    self.emit_with(
                        &rng,
                        Shape::Puff,
                        pos,
                        0.03 * k as f32,
                        0.7,
                        [a.cos() * 0.35, 0.12, a.sin() * 0.35],
                        [0.0; 3],
                        [0.05, 0.22],
                        (DUST, fade(DUST)),
                        1.5,
                        ground,
                        0.0,
                    );
                }
            }
        }
    }

    /// A small impact: where an arrow, a blade, a bullet or a blow lands.
    pub fn spawn_impact(&mut self, at: [f32; 3], kind: ImpactKind) {
        let scale = self.mode.scale();
        if scale <= 0.0 {
            return;
        }
        let rng = self.rng();
        let lift = [at[0], at[1] + 0.12, at[2]];
        match kind {
            ImpactKind::Blade => {
                self.emit(
                    &rng,
                    Shape::Glow,
                    lift,
                    0.0,
                    0.09,
                    [0.0; 3],
                    [0.0; 3],
                    [0.12, 0.2],
                    (FLASH, fade(FLASH)),
                );
            }
            ImpactKind::Bullet => {
                self.emit(
                    &rng,
                    Shape::Glow,
                    lift,
                    0.0,
                    0.07,
                    [0.0; 3],
                    [0.0; 3],
                    [0.07, 0.12],
                    (FLASH, fade(FLASH)),
                );
            }
            _ => {}
        }
        let dust_n = ((3.0 * scale).round() as u32).max(1);
        for _ in 0..dust_n {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let sp = rng.range(0.1, 0.4);
            self.emit_with(
                &rng,
                Shape::Puff,
                [at[0], at[1] + 0.04, at[2]],
                0.0,
                rng.range(0.4, 0.8),
                [a.cos() * sp, rng.range(0.1, 0.3), a.sin() * sp],
                [0.0; 3],
                [0.04, 0.17],
                (DUST, fade(DUST)),
                1.5,
                at[1],
                0.0,
            );
        }
        if self.mode == EffectsMode::Full {
            let n = match kind {
                ImpactKind::Blade => 7,
                ImpactKind::Arrow => 3,
                ImpactKind::Bullet => 4,
                ImpactKind::Blunt => 2,
            };
            for _ in 0..n {
                let a = rng.range(0.0, std::f32::consts::TAU);
                let sp = rng.range(0.8, 2.0);
                let vel = [a.cos() * sp, rng.range(1.0, 2.2), a.sin() * sp];
                let (shape, size, c) = match kind {
                    ImpactKind::Blade | ImpactKind::Bullet => {
                        (Shape::Streak, [0.008, 0.09], (FIRE_HOT, fade(FIRE_HOT)))
                    }
                    _ => (
                        Shape::Chip,
                        [0.016, 0.0],
                        (rgba(90, 76, 60, 255), rgba(90, 76, 60, 255)),
                    ),
                };
                self.emit_with(
                    &rng,
                    shape,
                    lift,
                    0.0,
                    rng.range(0.25, 0.5),
                    vel,
                    [0.0, -9.0, 0.0],
                    size,
                    c,
                    0.0,
                    at[1],
                    0.0,
                );
            }
        }
    }

    /// A column of smoke rising from `at` for `seconds`.
    pub fn spawn_smoke(&mut self, at: [f32; 3], size: f32, seconds: f32) {
        let scale = self.mode.scale();
        if scale <= 0.0 || seconds <= 0.0 {
            return;
        }
        let rng = self.rng();
        let rate = 5.0 * scale;
        let puffs = ((seconds * rate).ceil() as usize).clamp(1, 160);
        for k in 0..puffs {
            let delay = seconds * k as f32 / puffs as f32;
            let life = rng.range(1.8, 2.8);
            let drift = [
                0.08 + rng.signed() * 0.05,
                rng.range(0.25, 0.4) * size.max(0.2) * 3.0,
                rng.signed() * 0.05,
            ];
            self.emit_with(
                &rng,
                Shape::Puff,
                [
                    at[0] + rng.signed() * size * 0.15,
                    at[1] + 0.05,
                    at[2] + rng.signed() * size * 0.15,
                ],
                delay,
                life,
                drift,
                [0.0; 3],
                [size * 0.35, size * rng.range(0.9, 1.3)],
                (SMOKE_GREY, fade(SMOKE_GREY)),
                0.35,
                at[1],
                rng.signed() * 0.3,
            );
        }
    }

    /// Flames (with a little smoke) at `at` for `seconds`.
    pub fn spawn_fire(&mut self, at: [f32; 3], size: f32, seconds: f32) {
        let scale = self.mode.scale();
        if scale <= 0.0 || seconds <= 0.0 {
            return;
        }
        let rng = self.rng();
        let rate = 9.0 * scale;
        let flames = ((seconds * rate).ceil() as usize).clamp(1, 200);
        for k in 0..flames {
            let delay = seconds * k as f32 / flames as f32;
            let pos = [
                at[0] + rng.signed() * size * 0.3,
                at[1] + 0.03,
                at[2] + rng.signed() * size * 0.3,
            ];
            self.emit_with(
                &rng,
                Shape::Flame,
                pos,
                delay,
                rng.range(0.45, 0.8),
                [0.0, 0.12 * size, 0.0],
                [0.0; 3],
                [size * 0.5, size * 0.7],
                (FIRE_HOT, FIRE_COOL),
                0.5,
                pos[1],
                0.0,
            );
        }
        self.spawn_smoke([at[0], at[1] + size * 0.7, at[2]], size * 0.8, seconds);
    }

    /// A puff of dust (a unit stops, a gun kicks, a ram hits).
    pub fn spawn_dust(&mut self, at: [f32; 3], size: f32) {
        let scale = self.mode.scale();
        if scale <= 0.0 {
            return;
        }
        let rng = self.rng();
        let n = ((5.0 * scale).round() as u32).max(1);
        for _ in 0..n {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let sp = rng.range(0.2, 0.6) * size.max(0.2);
            self.emit_with(
                &rng,
                Shape::Puff,
                [at[0], at[1] + 0.03, at[2]],
                rng.range(0.0, 0.05),
                rng.range(0.5, 0.9),
                [a.cos() * sp, rng.range(0.1, 0.3), a.sin() * sp],
                [0.0; 3],
                [size * 0.25, size * rng.range(0.6, 0.9)],
                (DUST, fade(DUST)),
                1.6,
                at[1],
                0.0,
            );
        }
    }

    /// Plays a presentation event; `ground(x, z)` is the surface height there.
    pub fn play(&mut self, event: &FxEvent, ground: &dyn Fn(f32, f32) -> f32) {
        let on = |p: [f32; 2], lift: f32| [p[0], ground(p[0], p[1]) + lift, p[1]];
        match *event {
            FxEvent::Muzzle { at, toward, kind } => {
                let dir = [toward[0] - at[0], toward[1] - at[1]];
                self.spawn_muzzle(on(at, MUZZLE_HEIGHT), dir, kind);
            }
            FxEvent::Projectile {
                from,
                to,
                kind,
                seconds,
            } => {
                let high = if kind == ProjectileKind::Bomb {
                    1.6
                } else {
                    MUZZLE_HEIGHT
                };
                self.spawn_projectile(on(from, high), on(to, 0.12), kind, seconds);
            }
            FxEvent::Explosion { at, size, kind } => self.spawn_explosion(on(at, 0.0), size, kind),
            FxEvent::Impact { at, kind } => self.spawn_impact(on(at, 0.0), kind),
            FxEvent::Smoke { at, size, seconds } => self.spawn_smoke(on(at, 0.0), size, seconds),
            FxEvent::Fire { at, size, seconds } => self.spawn_fire(on(at, 0.0), size, seconds),
            FxEvent::Dust { at, size } => self.spawn_dust(on(at, 0.0), size),
            FxEvent::Air(ref air) => self.play_air(air, ground),
        }
    }
}

#[path = "effects_air.rs"]
mod air;
pub use air::{AirFx, BLAST_RADIUS};

/// Uniform of the effects pass: screen axes and the clock.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FxUniform {
    right: [f32; 4],
    up: [f32; 4],
    time: [f32; 4],
}

/// GPU side: uniform, bind group and the particle buffer (the pipeline is built with the
/// other scene pipelines: `pipelines::effects_pipeline`).
pub struct EffectsGpu {
    pub layout: wgpu::BindGroupLayout,
    uniform: GpuBuffer,
    bind: wgpu::BindGroup,
    particles: GpuBuffer,
    draw: u32,
}

impl EffectsGpu {
    pub fn new(core: &GpuCore) -> Self {
        let device = &core.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("effects layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform = core.buffer(
            Category::Overlays,
            &wgpu::BufferDescriptor {
                label: Some("effects uniform"),
                size: std::mem::size_of::<FxUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        );
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effects bind"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let particles = core.buffer(
            Category::Overlays,
            &wgpu::BufferDescriptor {
                label: Some("effect particles"),
                size: (CAPACITY * std::mem::size_of::<Particle>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            },
        );
        Self {
            layout,
            uniform,
            bind,
            particles,
            draw: 0,
        }
    }

    /// Uploads the new particles and the clock; returns the bytes written.
    pub fn prepare(
        &mut self,
        core: &GpuCore,
        effects: &mut Effects,
        view_proj: &glam::Mat4,
    ) -> u64 {
        let mut bytes = 0;
        if let Some((a, b)) = effects.take_dirty() {
            let slice = &effects.particles()[a..b.min(effects.particles().len())];
            let offset = (a * std::mem::size_of::<Particle>()) as u64;
            core.queue
                .write_buffer(&self.particles, offset, bytemuck::cast_slice(slice));
            bytes += std::mem::size_of_val(slice) as u64;
        }
        self.draw = effects.draw_count();
        if self.draw > 0 {
            // Row 0 / row 1 of the projection are the screen's right and up axes.
            let right = view_proj.row(0).truncate().normalize_or_zero();
            let up = view_proj.row(1).truncate().normalize_or_zero();
            let uniform = FxUniform {
                right: [right.x, right.y, right.z, 0.0],
                up: [up.x, up.y, up.z, 0.0],
                time: [effects.now(), 0.0, 0.0, 0.0],
            };
            core.queue
                .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
            bytes += std::mem::size_of::<FxUniform>() as u64;
        }
        bytes
    }

    /// Particles drawn this frame (0 when nothing is alive).
    pub fn drawn(&self) -> u32 {
        self.draw
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, pipeline: &'a wgpu::RenderPipeline) {
        if self.draw == 0 {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(1, &self.bind, &[]);
        pass.set_vertex_buffer(0, self.particles.slice(..));
        pass.draw(0..6, 0..self.draw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(_x: f32, _z: f32) -> f32 {
        0.3
    }

    #[test]
    fn a_projectile_arrives_at_its_target_when_its_time_is_up() {
        for kind in [
            ProjectileKind::Arrow,
            ProjectileKind::Bullet,
            ProjectileKind::Shell,
            ProjectileKind::Stone,
            ProjectileKind::Bomb,
        ] {
            let mut fx = Effects::new();
            let (from, to) = ([1.0, 0.6, 2.0], [3.5, 0.4, -1.0]);
            fx.spawn_projectile(from, to, kind, 0.5);
            let body = fx.particles()[0];
            let end = body.position_at(0.5);
            for i in 0..3 {
                assert!(
                    (end[i] - to[i]).abs() < 1e-4,
                    "{kind:?} axis {i}: {end:?} vs {to:?}"
                );
            }
            let mid = body.position_at(0.25);
            assert!(
                mid[1] >= (from[1] + to[1]) / 2.0 - 1e-4,
                "{kind:?} flies at least the chord's height"
            );
        }
    }

    #[test]
    fn the_same_spawns_give_the_same_particles() {
        let run = || {
            let mut fx = Effects::new();
            fx.spawn_explosion([1.0, 0.3, 1.0], 0.5, ExplosionKind::Shell);
            fx.advance(0.2);
            fx.spawn_projectile(
                [0.0, 0.5, 0.0],
                [2.0, 0.3, 0.0],
                ProjectileKind::Rocket,
                0.4,
            );
            fx.spawn_muzzle([0.0, 0.5, 0.0], [1.0, 0.0], MuzzleKind::Cannon);
            fx.spawn_smoke([2.0, 0.3, 2.0], 0.4, 3.0);
            bytemuck::cast_slice::<Particle, u8>(fx.particles()).to_vec()
        };
        assert_eq!(run(), run());
        assert!(!run().is_empty());
    }

    #[test]
    fn the_pool_is_a_ring_and_never_grows() {
        let mut fx = Effects::new();
        for _ in 0..200 {
            fx.spawn_explosion([0.0, 0.0, 0.0], 0.6, ExplosionKind::Bomb);
        }
        assert_eq!(fx.particles().len(), CAPACITY);
        assert!(fx.draw_count() as usize <= CAPACITY);
    }

    #[test]
    fn reduced_mode_is_cheaper_and_off_is_silent() {
        let count = |mode| {
            let mut fx = Effects::new();
            fx.set_mode(mode);
            fx.spawn_explosion([0.0, 0.0, 0.0], 0.6, ExplosionKind::Shell);
            fx.spawn_projectile([0.0; 3], [2.0, 0.0, 0.0], ProjectileKind::Shell, 0.4);
            fx.spawn_muzzle([0.0; 3], [1.0, 0.0], MuzzleKind::Cannon);
            fx.particles().len()
        };
        let (full, reduced, off) = (
            count(EffectsMode::Full),
            count(EffectsMode::Reduced),
            count(EffectsMode::Off),
        );
        assert!(reduced < full / 2, "{reduced} vs {full}");
        assert!(reduced > 0, "reduced still shows the event");
        assert_eq!(off, 0);
    }

    #[test]
    fn particles_expire_and_the_draw_range_shrinks() {
        let mut fx = Effects::new();
        fx.spawn_explosion([0.0, 0.0, 0.0], 0.5, ExplosionKind::Shell);
        assert!(fx.draw_count() > 0 && fx.live() > 0);
        fx.advance(20.0);
        assert_eq!(fx.live(), 0);
        assert_eq!(fx.draw_count(), 0);
    }

    #[test]
    fn delayed_particles_are_born_later_and_play_resolves_the_ground() {
        let mut fx = Effects::new();
        fx.play(
            &FxEvent::Smoke {
                at: [1.0, 1.0],
                size: 0.4,
                seconds: 2.0,
            },
            &flat,
        );
        assert!(fx.particles().iter().any(|p| p.born > fx.now() + 1.0));
        assert!(fx.particles().iter().all(|p| p.pos[1] >= 0.3));
        let mut fx = Effects::new();
        fx.play(
            &FxEvent::Projectile {
                from: [0.0, 0.0],
                to: [2.0, 0.0],
                kind: ProjectileKind::Arrow,
                seconds: 0.4,
            },
            &flat,
        );
        let arrow = fx.particles()[0];
        assert!((arrow.pos[1] - (0.3 + MUZZLE_HEIGHT)).abs() < 1e-5);
        assert_eq!(arrow.shape(), Shape::Arrow as u8);
    }

    #[test]
    fn debris_stops_on_the_ground() {
        let mut fx = Effects::new();
        fx.spawn_explosion([0.0, 0.5, 0.0], 0.6, ExplosionKind::Shell);
        for p in fx
            .particles()
            .iter()
            .filter(|p| p.shape() == Shape::Chip as u8)
        {
            for k in 0..20 {
                assert!(p.position_at(p.life * k as f32 / 19.0)[1] >= 0.5 - 1e-5);
            }
        }
    }

    #[test]
    fn dirty_range_covers_exactly_the_new_slots() {
        let mut fx = Effects::new();
        fx.spawn_dust([0.0; 3], 0.4);
        let (a, b) = fx.take_dirty().expect("dirty");
        assert_eq!(a, 0);
        assert_eq!(b, fx.particles().len());
        assert!(fx.take_dirty().is_none());
    }
}
