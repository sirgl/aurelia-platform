//! Effects of the air war (a child module of `effects`, it uses the pool's private parts):
//! contrails and exhaust, bombs falling from an aircraft, flak, parachute canopies and the
//! nuclear blast with its stem, cap, shock ring and scorched ground.
//!
//! Every effect is a handful of particles born at once with delayed births (the pool needs
//! no emitters); `EffectsMode::Reduced` keeps the event (a flash, a puff) without the show.

use super::*;
use crate::gpu::flight::Trail;

/// Hex-radius of a nuclear blast in world units (2 tiles of 1.73).
pub const BLAST_RADIUS: f32 = 2.0 * 1.732_050_8;

/// The effects of the air war, as the presentation names them (XZ plus an altitude above
/// the ground at that point).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AirFx {
    /// One puff of a flier's trail.
    Trail { at: [f32; 2], altitude: f32, kind: Trail },
    /// A bomb released at `altitude` over `from` that lands on `to` after `seconds`.
    Drop { from: [f32; 2], altitude: f32, to: [f32; 2], seconds: f32 },
    /// A flak burst.
    Flak { at: [f32; 2], altitude: f32 },
    /// A parachute canopy that sinks from `altitude` to the ground in `seconds` (the
    /// soldier it carries is a pose).
    Canopy { at: [f32; 2], altitude: f32, seconds: f32 },
    /// A nuclear blast: flash, fireball, shock ring, stem, cap, a scorched circle.
    Blast { at: [f32; 2], radius: f32 },
    /// A missile or a nuclear weapon leaves its base: ignition flash, flame and a ground
    /// cloud of smoke and dust. `heavy` is a ballistic launch (a bigger cloud).
    Launch { at: [f32; 2], heavy: bool },
}

const WHITE_TRAIL: [u8; 4] = rgba(244, 246, 250, 120);
const EXHAUST: [u8; 4] = rgba(70, 68, 66, 120);
const BLACK_SMOKE: [u8; 4] = rgba(26, 24, 24, 220);
const CANOPY: [u8; 4] = rgba(236, 232, 214, 240);
const CAP_HOT: [u8; 4] = rgba(255, 150, 60, 230);
const CAP_COOL: [u8; 4] = rgba(110, 96, 90, 0);
const BLAST_WHITE: [u8; 4] = rgba(255, 250, 230, 255);

impl Effects {
    pub fn play_air(&mut self, event: &AirFx, ground: &dyn Fn(f32, f32) -> f32) {
        let on = |p: [f32; 2], lift: f32| [p[0], ground(p[0], p[1]) + lift, p[1]];
        match *event {
            AirFx::Trail { at, altitude, kind } => self.spawn_trail(on(at, altitude), kind),
            AirFx::Drop { from, altitude, to, seconds } => {
                self.spawn_projectile(on(from, altitude), on(to, 0.12), ProjectileKind::Bomb, seconds)
            }
            AirFx::Flak { at, altitude } => self.spawn_flak(on(at, altitude)),
            AirFx::Canopy { at, altitude, seconds } => self.spawn_canopy(on(at, 0.0), altitude, seconds),
            AirFx::Blast { at, radius } => self.spawn_nuclear_blast(on(at, 0.0), radius),
            AirFx::Launch { at, heavy } => self.spawn_launch(on(at, 0.0), heavy),
        }
    }

    /// A puff left behind by a flier at `at` (world position).
    pub fn spawn_trail(&mut self, at: [f32; 3], kind: Trail) {
        if self.mode == EffectsMode::Off || kind == Trail::None {
            return;
        }
        // Quality Low and reduced motion: no contrails or exhaust, the rest keeps its flame.
        if self.mode == EffectsMode::Reduced && matches!(kind, Trail::Contrail | Trail::Exhaust) {
            return;
        }
        let rng = self.rng();
        match kind {
            Trail::None => {}
            Trail::Contrail => self.emit_with(
                &rng, Shape::Puff, at, 0.0, 1.1, [0.0; 3], [0.0; 3], [0.07, 0.17], (WHITE_TRAIL, fade(WHITE_TRAIL)), 0.0, -10.0, 0.0,
            ),
            Trail::Exhaust => self.emit_with(
                &rng, Shape::Puff, at, 0.0, 0.7, [0.0, 0.05, 0.0], [0.0; 3], [0.05, 0.12], (EXHAUST, fade(EXHAUST)), 0.0, -10.0, 0.0,
            ),
            Trail::Rocket => {
                self.emit_with(
                    &rng, Shape::Flame, at, 0.0, 0.22, [0.0; 3], [0.0; 3], [0.07, 0.04], (FIRE_HOT, FIRE_COOL), 0.0, -10.0, 0.0,
                );
                if self.mode == EffectsMode::Full {
                    self.emit_with(
                        &rng, Shape::Puff, at, 0.0, 1.0, [rng.signed() * 0.05, 0.1, rng.signed() * 0.05], [0.0; 3], [0.08, 0.2],
                        (SMOKE_GREY, fade(SMOKE_GREY)), 0.4, -10.0, 0.0,
                    );
                }
            }
            Trail::Damage => {
                self.emit_with(
                    &rng, Shape::Puff, at, 0.0, 1.4, [rng.signed() * 0.05, 0.1, rng.signed() * 0.05], [0.0; 3], [0.09, 0.24],
                    (BLACK_SMOKE, fade(BLACK_SMOKE)), 0.3, -10.0, 0.0,
                );
                self.emit_with(
                    &rng, Shape::Flame, at, 0.0, 0.3, [0.0; 3], [0.0; 3], [0.07, 0.05], (FIRE_HOT, FIRE_COOL), 0.0, -10.0, 0.0,
                );
            }
        }
    }

    /// An anti-aircraft burst: a black cloud with a flash.
    pub fn spawn_flak(&mut self, at: [f32; 3]) {
        if self.mode == EffectsMode::Off {
            return;
        }
        let rng = self.rng();
        self.emit_with(&rng, Shape::Glow, at, 0.0, 0.12, [0.0; 3], [0.0; 3], [0.35, 0.5], (FLASH, fade(FLASH)), 0.0, -10.0, 0.0);
        let puffs = if self.mode == EffectsMode::Full { 3 } else { 1 };
        for _ in 0..puffs {
            let o = [rng.signed() * 0.12, rng.signed() * 0.08, rng.signed() * 0.12];
            self.emit_with(
                &rng, Shape::Puff, [at[0] + o[0], at[1] + o[1], at[2] + o[2]], 0.04, 1.2, [0.0, 0.05, 0.0], [0.0; 3], [0.16, 0.34],
                (BLACK_SMOKE, fade(BLACK_SMOKE)), 0.8, -10.0, rng.signed() * 0.3,
            );
        }
    }

    /// A parachute canopy: a pale dome above a soldier who sinks from `altitude` to the
    /// ground in `seconds` (the canopy rides 0.3 above the soldier's head; no mesh).
    pub fn spawn_canopy(&mut self, ground: [f32; 3], altitude: f32, seconds: f32) {
        if self.mode == EffectsMode::Off {
            return;
        }
        let rng = self.rng();
        let seconds = seconds.max(0.2);
        let pos = [ground[0], ground[1] + altitude + 0.34, ground[2]];
        let vel = [0.0, -altitude / seconds, 0.0];
        self.emit_with(&rng, Shape::Puff, pos, 0.0, seconds, vel, [0.0; 3], [0.34, 0.34], (CANOPY, CANOPY), 0.0, ground[1] + 0.3, 0.0);
        // The shroud lines: two thin streaks to the soldier.
        if self.mode == EffectsMode::Full {
            for side in [-1.0f32, 1.0] {
                let p = [pos[0] + side * 0.09, pos[1] - 0.18, pos[2]];
                self.emit_with(
                    &rng, Shape::Streak, p, 0.0, seconds, vel, [0.0; 3], [0.006, 0.14], (rgba(220, 220, 210, 200), rgba(220, 220, 210, 200)),
                    0.0, ground[1] + 0.1, 0.0,
                );
            }
        }
    }

    /// The ignition of a missile at the ground point `at`: a flash, the first flame and a
    /// cloud of smoke and dust that rolls out along the ground.
    pub fn spawn_launch(&mut self, at: [f32; 3], heavy: bool) {
        let scale = self.mode.scale();
        if scale <= 0.0 {
            return;
        }
        let rng = self.rng();
        let k = if heavy { 1.6 } else { 1.0 };
        let flash = [at[0], at[1] + 0.12, at[2]];
        self.emit_with(&rng, Shape::Glow, flash, 0.0, 0.2, [0.0; 3], [0.0; 3], [0.35 * k, 0.6 * k], (FLASH, fade(FLASH)), 0.0, -10.0, 0.0);
        self.emit_with(&rng, Shape::Flame, flash, 0.0, 0.45, [0.0, 0.5, 0.0], [0.0; 3], [0.16 * k, 0.05], (FIRE_HOT, FIRE_COOL), 0.0, -10.0, 0.0);
        let puffs = ((if heavy { 9.0 } else { 5.0 }) * scale).round().max(2.0) as u32;
        for i in 0..puffs {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let out = rng.range(0.2, 0.5) * k;
            self.emit_with(
                &rng, Shape::Puff, [at[0], at[1] + 0.06, at[2]], 0.03 * i as f32, rng.range(1.2, 1.9), [a.cos() * out, rng.range(0.08, 0.2), a.sin() * out],
                [0.0; 3], [0.12 * k, 0.34 * k], (SMOKE_GREY, fade(SMOKE_GREY)), 1.2, at[1] + 0.03, rng.signed() * 0.3,
            );
        }
        if self.mode == EffectsMode::Full {
            self.emit(&rng, Shape::Ring, [at[0], at[1], at[2]], 0.0, 0.7, [0.0; 3], [0.0; 3], [0.1 * k, 0.9 * k], (rgba(210, 190, 150, 150), rgba(210, 190, 150, 0)));
        }
    }

    /// The nuclear blast at the ground point `at`: radius of the shock ring `radius`.
    pub fn spawn_nuclear_blast(&mut self, at: [f32; 3], radius: f32) {
        let scale = self.mode.scale();
        if scale <= 0.0 {
            return;
        }
        let rng = self.rng();
        let full = self.mode == EffectsMode::Full;
        let count = |n: u32| ((n as f32 * scale).round() as u32).max(1);
        let r = radius.max(0.5);
        // The flash: a white glow over the blast for a few frames, then a long orange one
        // (scene pass only: the interface keeps its exposure).
        self.emit_with(&rng, Shape::Glow, [at[0], at[1] + 0.5, at[2]], 0.0, 0.18, [0.0; 3], [0.0; 3], [r * 1.3, r * 1.9], (BLAST_WHITE, fade(BLAST_WHITE)), 0.0, -10.0, 0.0);
        self.emit_with(&rng, Shape::Glow, [at[0], at[1] + 0.6, at[2]], 0.05, 0.9, [0.0; 3], [0.0; 3], [r * 0.7, r * 1.0], (CAP_HOT, fade(CAP_HOT)), 0.0, -10.0, 0.0);
        // The fireball and the stem: flames and dark smoke rising in a column.
        for i in 0..count(14) {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let d = rng.range(0.0, 0.18 * r);
            let pos = [at[0] + a.cos() * d, at[1] + 0.05, at[2] + a.sin() * d];
            let rise = rng.range(1.6, 2.6);
            self.emit_with(
                &rng, Shape::Flame, pos, 0.04 * i as f32, rng.range(0.9, 1.5), [a.cos() * 0.25, rise, a.sin() * 0.25], [0.0, -0.5, 0.0],
                [r * 0.17, r * 0.24], (FIRE_HOT, FIRE_COOL), 1.0, pos[1], 0.0,
            );
        }
        for i in 0..count(14) {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let d = rng.range(0.0, 0.1 * r);
            let pos = [at[0] + a.cos() * d, at[1] + 0.05, at[2] + a.sin() * d];
            self.emit_with(
                &rng, Shape::Puff, pos, 0.07 * i as f32, rng.range(2.2, 3.0), [a.cos() * 0.12, rng.range(1.6, 2.4), a.sin() * 0.12], [0.0; 3],
                [r * 0.14, r * 0.34], (SMOKE_DARK, fade(SMOKE_DARK)), 0.9, pos[1], rng.signed() * 0.3,
            );
        }
        // The cap: a ring of hot puffs that spread out at the top of the stem and cool.
        for _ in 0..count(16) {
            let a = rng.range(0.0, std::f32::consts::TAU);
            let top = at[1] + r * 0.62;
            let pos = [at[0], top, at[2]];
            let out = rng.range(0.5, 1.0);
            self.emit_with(
                &rng, Shape::Puff, pos, rng.range(0.55, 0.9), rng.range(1.8, 2.6), [a.cos() * out, rng.range(0.05, 0.4), a.sin() * out], [0.0; 3],
                [r * 0.2, r * 0.46], (CAP_HOT, CAP_COOL), 1.1, top - 1.0, rng.signed() * 0.4,
            );
        }
        // The shock rings on the ground, out to the blast radius.
        for i in 0..(if full { 3 } else { 1 }) {
            self.emit(
                &rng, Shape::Ring, [at[0], at[1], at[2]], 0.12 * i as f32, 1.1 + 0.1 * i as f32, [0.0; 3], [0.0; 3],
                [r * 0.12, r * (0.78 + 0.11 * i as f32)], (rgba(235, 220, 190, 170), rgba(235, 220, 190, 0)),
            );
        }
        // Fallout: a low, wide, greenish haze that lingers for ten seconds after the blast
        // (few, big, faint puffs: the fill cost is what counts on an integrated GPU).
        if full {
            for i in 0..5 {
                let a = rng.range(0.0, std::f32::consts::TAU);
                let d = rng.range(0.15, 0.55) * r;
                let pos = [at[0] + a.cos() * d, at[1] + 0.22, at[2] + a.sin() * d];
                let haze = rgba(118, 128, 92, 60);
                self.emit_with(
                    &rng, Shape::Puff, pos, 0.9 + 0.15 * i as f32, rng.range(9.0, 11.0), [a.cos() * 0.03, 0.02, a.sin() * 0.03], [0.0; 3],
                    [r * 0.55, r * 0.7], (haze, fade(haze)), 0.2, pos[1], rng.signed() * 0.05,
                );
            }
        }
        // The scorched circle stays a while (the fallout takes the tiles over).
        if full {
            self.emit(
                &rng, Shape::Scorch, [at[0], at[1], at[2]], 0.15, 14.0, [0.0; 3], [0.0; 3], [r * 0.85, r * 1.0],
                (rgba(14, 12, 10, 150), rgba(14, 12, 10, 0)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blast_is_a_bounded_burst_and_cheaper_when_reduced() {
        let mut full = Effects::new();
        full.spawn_nuclear_blast([0.0, 0.0, 0.0], BLAST_RADIUS);
        let mut reduced = Effects::new();
        reduced.set_mode(EffectsMode::Reduced);
        reduced.spawn_nuclear_blast([0.0, 0.0, 0.0], BLAST_RADIUS);
        assert!(full.live() > reduced.live() && reduced.live() >= 3);
        assert!(full.live() < 90, "a blast is a few dozen particles, not a flood: {}", full.live());
        let mut off = Effects::new();
        off.set_mode(EffectsMode::Off);
        off.spawn_nuclear_blast([0.0; 3], BLAST_RADIUS);
        assert_eq!(off.live(), 0);
    }

    #[test]
    fn a_launch_is_a_small_bounded_burst_and_a_blast_leaves_a_lingering_haze() {
        let mut fx = Effects::new();
        fx.spawn_launch([0.0; 3], true);
        assert!(fx.live() >= 5 && fx.live() < 20, "{}", fx.live());
        let mut reduced = Effects::new();
        reduced.set_mode(EffectsMode::Reduced);
        reduced.spawn_launch([0.0; 3], false);
        assert!(reduced.live() >= 3 && reduced.live() < fx.live());
        let mut blast = Effects::new();
        blast.spawn_nuclear_blast([0.0; 3], BLAST_RADIUS);
        let longest = blast.particles().iter().map(|p| p.life).fold(0.0f32, f32::max);
        assert!(longest > 8.0, "the fallout haze outlives the fireball: {longest}");
    }

    #[test]
    fn the_canopy_sinks_to_the_soldiers_head_in_the_time_given() {
        let mut fx = Effects::new();
        fx.spawn_canopy([1.0, 0.0, 2.0], 0.8, 1.6);
        let canopy = fx.particles().iter().find(|p| p.shape_is_puff()).expect("a canopy");
        let start = canopy.position_at(0.0);
        let end = canopy.position_at(1.6);
        assert!((start[1] - 1.14).abs() < 1e-4);
        assert!((end[1] - 0.34).abs() < 1e-3, "lands 0.34 above the ground: {end:?}");
    }

    #[test]
    fn trails_obey_the_modes() {
        let mut fx = Effects::new();
        fx.spawn_trail([0.0, 1.0, 0.0], Trail::Contrail);
        assert_eq!(fx.live(), 1);
        fx.set_mode(EffectsMode::Reduced);
        fx.spawn_trail([0.0, 1.0, 0.0], Trail::Contrail);
        assert_eq!(fx.live(), 1, "no contrails when reduced");
        fx.set_mode(EffectsMode::Off);
        fx.spawn_trail([0.0, 1.0, 0.0], Trail::Rocket);
        assert_eq!(fx.live(), 0, "switching the effects off clears the pool and nothing new is spawned");
    }
}

impl Particle {
    #[cfg(test)]
    fn shape_is_puff(&self) -> bool {
        self.kind as u8 == Shape::Puff as u8
    }
}
