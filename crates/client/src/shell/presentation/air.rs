//! The air war on the map: rebase flights, bombing and strafing runs, missiles, nuclear
//! launches and blasts, paradrops and airlifts.
//!
//! Every function here is pure: a cue, its local time and the map give a unit's look (a
//! flight pose: altitude, heading, pitch, bank) and the timed beats (trail puffs, a bomb
//! released over the target, a blast). The per-class numbers (cruise altitude, speed,
//! take-off and landing time, how far a path bulges, how a model banks) are a table in
//! `render::gpu::flight`, not code. A flight is a *course* (`Course`): the path over the
//! ground by arc length and a speed profile in time, so heading, bank and pitch come from
//! the path itself and no frame ever teleports.

use kernel::ids::TileId;
use render::gpu::effects::{AirFx, ExplosionKind, FxEvent, ImpactKind, MuzzleKind, ProjectileKind, BLAST_RADIUS};
use render::gpu::flight::{params, FlightClass, FlightParams, Trail};
use render::gpu::unit_poses::{facing_towards, Gait, UnitPose};
use render::unit_models::Weapon;

use super::cue::{Attack, Cue, Timing, SETTLE_SECONDS};
use super::motion::{Beat, Look, Map, Mark, MarkKind, Moment, Sound};

/// A tile is 1.732 world units wide.
const TILE: f32 = 1.732_050_8;
/// A bomb falls this long from the aircraft to the ground.
pub const BOMB_FALL: f32 = 0.45;
/// Seconds a paratrooper sinks under the canopy.
pub const DESCENT: f32 = 1.3;
/// Where a bombing run turns round: this far past the target, on a turn of this radius.
const OVERSHOOT: f32 = 0.7;
const TURN_RADIUS: f32 = 1.1;
/// Seconds between trail puffs.
const TRAIL_STEP: f32 = 0.09;
/// Longest flight (in tiles) the camera does not follow.
pub const FOLLOW_REACH: u8 = 9;

/// Persistent ids of transport planes are the soldier's id turned inside out: a synthetic
/// unit that no resting figure collapses for.
pub fn plane_id(unit: u64) -> u64 {
    u64::MAX - unit
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Route {
    /// Base to base along an arc, take-off and landing included (rebase, airlift).
    Transfer,
    /// Out to the target, round a turn and back to the base (bombing run, paradrop plane).
    Sortie,
    /// Base to the target and no further (missiles, nuclear weapons).
    OneWay,
}

/// The path and the clock of one flight.
#[derive(Debug, Clone, Copy)]
struct Course {
    route: Route,
    class: FlightClass,
    p: FlightParams,
    from: [f32; 2],
    dir: [f32; 2],
    perp: [f32; 2],
    chord: f32,
    /// Sideways bulge of a transfer (world units, signed).
    bulge: f32,
    /// Path length.
    len: f32,
    /// Total seconds, and seconds of acceleration and braking.
    total: f32,
    accel: f32,
    brake: f32,
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn bump(t: f32, centre: f32, half: f32) -> f32 {
    let u = ((t - centre) / half).abs();
    if u >= 1.0 {
        0.0
    } else {
        0.5 + 0.5 * (u * std::f32::consts::PI).cos()
    }
}

impl Course {
    fn new(route: Route, class: FlightClass, from: [f32; 2], to: [f32; 2], reach: u8) -> Self {
        let p = params(class);
        let (dx, dz) = (to[0] - from[0], to[1] - from[1]);
        let chord = dx.hypot(dz).max(0.05);
        let dir = [dx / chord, dz / chord];
        let perp = [-dir[1], dir[0]];
        // The side of the bulge: stable for a pair of places.
        let side = if ((from[0] * 7.0 + to[1] * 3.0) as i32) % 2 == 0 { 1.0 } else { -1.0 };
        let len = match route {
            Route::Transfer | Route::OneWay => chord,
            Route::Sortie => 2.0 * (chord + OVERSHOOT) + std::f32::consts::PI * TURN_RADIUS,
        };
        // The clock follows the hex distance (the cue knows it before it has a map), the
        // path follows the real positions: the speed takes up the difference.
        let nominal = reach.max(1) as f32 * TILE;
        let nominal_len = match route {
            Route::Transfer | Route::OneWay => nominal,
            Route::Sortie => 2.0 * (nominal + OVERSHOOT) + std::f32::consts::PI * TURN_RADIUS,
        };
        let cruise = nominal_len / TILE * p.tile_seconds;
        let (accel, brake, tail) = match (route, class) {
            (Route::OneWay, FlightClass::Ballistic) => (p.takeoff, p.landing, p.landing),
            (Route::OneWay, _) => (p.takeoff, 0.10, 0.10),
            _ => (p.takeoff * 0.8, p.landing * 0.8, p.landing),
        };
        let total = p.takeoff + cruise + tail;
        Self {
            route,
            class,
            p,
            from,
            dir,
            perp,
            chord,
            bulge: if route == Route::Transfer { p.arc * chord * side } else { 0.0 },
            len,
            total,
            accel: accel.min(total * 0.45),
            brake: brake.min(total * 0.45),
        }
    }

    /// Arc length flown at time `t` (a trapezoid speed profile: accelerate, cruise, brake).
    fn arc(&self, t: f32) -> f32 {
        let (ta, tb, total) = (self.accel, self.brake, self.total);
        let t = t.clamp(0.0, total);
        if self.class == FlightClass::Ballistic {
            return self.len * smooth(t / total);
        }
        let v = self.len / (total - 0.5 * ta - 0.5 * tb).max(1e-3);
        let s = if t < ta {
            v * t * t / (2.0 * ta.max(1e-3))
        } else if t <= total - tb {
            v * (0.5 * ta + (t - ta))
        } else {
            let r = total - t;
            self.len - v * r * r / (2.0 * tb.max(1e-3))
        };
        s.clamp(0.0, self.len)
    }

    /// Time at which the aircraft is `s` along its path.
    fn time_at(&self, s: f32) -> f32 {
        let (mut lo, mut hi) = (0.0, self.total);
        for _ in 0..28 {
            let mid = 0.5 * (lo + hi);
            if self.arc(mid) < s {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }

    /// When the aircraft is over the target (sortie: after `chord`; one way: the arrival).
    fn hit(&self) -> f32 {
        match self.route {
            Route::Sortie => self.time_at(self.chord),
            Route::OneWay => self.total - self.brake.min(0.1),
            Route::Transfer => self.total,
        }
    }

    /// Position over the ground at arc length `s`.
    fn pos(&self, s: f32) -> [f32; 2] {
        let at = |along: f32, side: f32| {
            [self.from[0] + self.dir[0] * along + self.perp[0] * side, self.from[1] + self.dir[1] * along + self.perp[1] * side]
        };
        match self.route {
            Route::Transfer => {
                let u = (s / self.len).clamp(0.0, 1.0);
                at(s, self.bulge * (std::f32::consts::PI * u).sin())
            }
            Route::OneWay => at(s, 0.0),
            Route::Sortie => {
                let out = self.chord + OVERSHOOT;
                let turn = std::f32::consts::PI * TURN_RADIUS;
                if s <= out {
                    at(s, 0.0)
                } else if s <= out + turn {
                    let theta = (s - out) / TURN_RADIUS;
                    // Centre of the turn is a radius to the side of the end of the outward leg.
                    let c = at(out, TURN_RADIUS);
                    let (sin, cos) = theta.sin_cos();
                    [
                        c[0] - self.perp[0] * TURN_RADIUS * cos + self.dir[0] * TURN_RADIUS * sin,
                        c[1] - self.perp[1] * TURN_RADIUS * cos + self.dir[1] * TURN_RADIUS * sin,
                    ]
                } else {
                    let back = s - out - turn;
                    let along = out - back;
                    let blend = 1.0 - smooth(back / out.max(0.1));
                    at(along, 2.0 * TURN_RADIUS * blend)
                }
            }
        }
    }

    /// Height above the ground at time `t`.
    fn altitude(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, self.total);
        let alt = self.p.altitude;
        match (self.route, self.class) {
            (Route::OneWay, FlightClass::Ballistic) => {
                let u = (t / self.total).clamp(0.0, 1.0);
                alt * (std::f32::consts::PI * u).sin().max(0.0).powf(0.7)
            }
            (Route::OneWay, _) => {
                let climb = smooth(t / self.p.takeoff.max(1e-3));
                let dive = 1.0 - 0.92 * smooth((t - (self.total - 0.45)) / 0.4);
                alt * climb * dive
            }
            (Route::Sortie, _) => {
                let up = smooth(t / self.p.takeoff.max(1e-3));
                let down = 1.0 - smooth((t - (self.total - self.p.landing)) / self.p.landing.max(1e-3));
                // A little lower over the target: the bombing run.
                alt * up * down * (1.0 - 0.22 * bump(t, self.hit(), 0.5))
            }
            (Route::Transfer, _) => {
                let up = smooth(t / self.p.takeoff.max(1e-3));
                let down = 1.0 - smooth((t - (self.total - self.p.landing)) / self.p.landing.max(1e-3));
                alt * up * down
            }
        }
    }

    /// Heading, pitch and bank at time `t`.
    fn attitude(&self, t: f32) -> (f32, f32, f32) {
        let s = self.arc(t);
        let facing_at = |s: f32| {
            let (a, b) = (self.pos((s - 0.12).max(0.0)), self.pos((s + 0.12).min(self.len)));
            facing_towards(b[0] - a[0], b[1] - a[1])
        };
        // At the very ends the path has no look-back or look-ahead: use the nearer sample.
        let s = s.clamp(0.0, self.len);
        let facing = facing_at(s);
        // Bank follows the curvature of the path over a window of one world unit.
        let (h0, h1) = (facing_at((s - 0.5).max(0.0)), facing_at((s + 0.5).min(self.len)));
        let mut turn = h1 - h0;
        while turn > std::f32::consts::PI {
            turn -= std::f32::consts::TAU;
        }
        while turn < -std::f32::consts::PI {
            turn += std::f32::consts::TAU;
        }
        let window = ((s + 0.5).min(self.len) - (s - 0.5).max(0.0)).max(0.2);
        let bank = (-(turn / window) * self.p.bank).clamp(-0.6, 0.6);
        // Pitch from the climb: height gained over ground covered.
        let dt = 0.06;
        let ground = (self.arc(t + dt) - self.arc((t - dt).max(0.0))).max(1e-3);
        let climb = self.altitude(t + dt) - self.altitude((t - dt).max(0.0));
        let pitch = (climb.atan2(ground) * self.p.pitch).clamp(-0.9, 0.9);
        (facing, pitch, bank)
    }

    /// The flier's look at `t`: `landing` blends the last moments into a parked aircraft.
    fn look(&self, t: f32, map: Map, landing: bool) -> Look {
        let s = self.arc(t);
        let pos = self.pos(s);
        let (facing, pitch, bank) = self.attitude(t);
        let altitude = self.altitude(t);
        let tile = nearest_tile(map, pos);
        let settle = if landing { smooth((t - (self.total - SETTLE_SECONDS)) / SETTLE_SECONDS) } else { 0.0 };
        Look {
            position: pos,
            tile,
            facing: blend_yaw(facing, 0.0, settle),
            gait: Gait::Flight { altitude, pitch: pitch * (1.0 - settle), bank: bank * (1.0 - settle) },
            flash: 0.0,
            settle,
            hp: [0, 0],
            change: 0.0,
        }
    }
}

fn blend_yaw(from: f32, to: f32, t: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let delta = (to - from + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    from + delta * t.clamp(0.0, 1.0)
}

/// The tile whose centre is nearest to a world position (for the fog under a flier).
pub fn nearest_tile(map: Map, xz: [f32; 2]) -> TileId {
    let width = map.width.max(1) as i32;
    let r = (xz[1] / 1.5).round() as i32;
    let q = (xz[0] / TILE - r as f32 * 0.5).round() as i32;
    let r = r.max(0);
    let q = q.clamp(0, width - 1);
    TileId((r as u32) * width as u32 + q as u32)
}

fn xz(map: Map, tile: TileId) -> [f32; 2] {
    map.xz(tile)
}

// ---------------------------------------------------------------- air strikes

/// The class of the aircraft of an attack: missiles fly low and are spent.
fn strike_class(attack: &Attack) -> FlightClass {
    match attack.weapon {
        Weapon::Missile => FlightClass::Cruise,
        _ => FlightClass::Plane,
    }
}

fn strike_course(attack: &Attack, map: Map) -> Course {
    let (from, to) = (xz(map, attack.from), xz(map, attack.target));
    let class = strike_class(attack);
    let route = if class == FlightClass::Cruise { Route::OneWay } else { Route::Sortie };
    Course::new(route, class, from, to, attack.reach)
}

fn tiles_to_xz(reach: u8) -> ([f32; 2], [f32; 2]) {
    // The duration depends only on the distance: a straight line of `reach` tiles.
    ([0.0, 0.0], [TILE * reach.max(1) as f32, 0.0])
}

/// Timing of an air attack: the weapon is released a bomb-fall before the aircraft is
/// over the target (the hit); the cue lasts until the aircraft is home (a missile: a
/// moment after its blast).
pub fn attack_timing(attack: &Attack) -> Timing {
    let (from, to) = tiles_to_xz(attack.reach);
    let class = strike_class(attack);
    let route = if class == FlightClass::Cruise { Route::OneWay } else { Route::Sortie };
    let course = Course::new(route, class, from, to, attack.reach);
    let hit = course.hit();
    match route {
        Route::OneWay => Timing { release: (hit - 0.4).max(0.0), hit, seconds: hit + 0.55 },
        _ => Timing { release: (hit - BOMB_FALL).max(0.0), hit, seconds: course.total },
    }
}

/// The attacker's look in an air strike.
pub fn attacker_look(attack: &Attack, t: f32, map: Map) -> Look {
    let course = strike_course(attack, map);
    let timing = attack_timing(attack);
    if course.route == Route::OneWay && t >= timing.hit {
        // The missile is spent in the blast.
        let end = course.look(course.total, map, false);
        return Look { gait: Gait::Hidden, ..end };
    }
    course.look(t, map, course.route != Route::OneWay)
}

/// Effects of an air strike, by local time: the weapon from the aircraft, the blast,
/// flak and smoke when it was hit, the trail.
pub fn strike_fx(attack: &Attack, map: Map) -> Vec<Moment> {
    let course = strike_course(attack, map);
    let timing = attack_timing(attack);
    let to = xz(map, attack.target);
    let at = |at: f32, fx: FxEvent| Moment { at, beat: Beat::Fx(fx) };
    let place = |t: f32| (course.pos(course.arc(t)), course.altitude(t));
    let mut out = Vec::new();
    match attack.weapon {
        Weapon::Bomb => {
            let (from, altitude) = place(timing.release);
            out.push(at(timing.release, FxEvent::Air(AirFx::Drop { from, altitude, to, seconds: timing.hit - timing.release })));
            out.push(at(timing.hit, FxEvent::Explosion { at: to, size: 0.8, kind: ExplosionKind::Bomb }));
        }
        Weapon::Missile | Weapon::Rocket => {
            // Ignition at the base, the blast, and a fire that burns on at the target (a city
            // or a unit that took the hit).
            out.push(at(0.0, FxEvent::Air(AirFx::Launch { at: xz(map, attack.from), heavy: false })));
            out.push(at(timing.hit, FxEvent::Explosion { at: to, size: 0.75, kind: ExplosionKind::Bomb }));
            out.push(at(timing.hit + 0.1, FxEvent::Fire { at: to, size: 0.5, seconds: 2.5 }));
        }
        _ => {
            // Strafing: three bursts of tracer fire from the aircraft over the target.
            for k in 0..3 {
                let t = (timing.hit - 0.30 + 0.08 * k as f32).max(0.0);
                let (from, _) = place(t);
                out.push(at(t, FxEvent::Muzzle { at: from, toward: to, kind: MuzzleKind::Rifle }));
                out.push(at(t, FxEvent::Projectile { from, to, kind: ProjectileKind::Bullet, seconds: 0.12 }));
                out.push(at(t + 0.12, FxEvent::Impact { at: to, kind: ImpactKind::Bullet }));
            }
        }
    }
    // Hit on the way (interception, anti-aircraft fire): flak round the aircraft, then a
    // smoking trail home.
    let hurt = attack.taken.is_some();
    if hurt {
        for k in 0..3 {
            let t = (timing.hit - 0.35 + 0.1 * k as f32).max(0.0);
            let (pos, altitude) = place(t);
            out.push(at(t, FxEvent::Air(AirFx::Flak { at: [pos[0] + 0.15 * (k as f32 - 1.0), pos[1] + 0.1], altitude: altitude + 0.1 })));
        }
    }
    let kind = if hurt { Trail::Damage } else { course.p.trail };
    let (first, last) = match course.route {
        Route::OneWay => (0.12, timing.hit),
        _ => (0.2, course.total - 0.3),
    };
    out.extend(trail_puffs(&course, kind, first, last, hurt, timing.hit));
    out
}

/// Trail puffs along a course between two times; a hurt flier smokes only after the hit.
fn trail_puffs(course: &Course, kind: Trail, first: f32, last: f32, only_after: bool, after: f32) -> Vec<Moment> {
    let mut out = Vec::new();
    if kind == Trail::None {
        return out;
    }
    let mut t = first;
    while t < last {
        if !only_after || t >= after - 0.35 {
            let pos = course.pos(course.arc(t));
            let altitude = course.altitude(t);
            // Nothing is left behind while the aircraft is still on the ground.
            if altitude > 0.12 {
                out.push(Moment { at: t, beat: Beat::Fx(FxEvent::Air(AirFx::Trail { at: pos, altitude: altitude * 0.95, kind })) });
            }
        }
        t += TRAIL_STEP;
    }
    out
}

// ---------------------------------------------------------------- rebase flights

fn flight_course(class: FlightClass, from: [f32; 2], to: [f32; 2], reach: u8) -> Course {
    Course::new(Route::Transfer, class, from, to, reach)
}

pub fn flight_seconds(class: FlightClass, reach: u8) -> f32 {
    let (from, to) = tiles_to_xz(reach);
    flight_course(class, from, to, reach).total
}

pub fn flight_look(class: FlightClass, from: TileId, to: TileId, reach: u8, t: f32, map: Map) -> Look {
    flight_course(class, xz(map, from), xz(map, to), reach).look(t, map, true)
}

pub fn flight_moments(class: FlightClass, from: TileId, to: TileId, reach: u8, map: Map) -> Vec<Moment> {
    let course = flight_course(class, xz(map, from), xz(map, to), reach);
    let mut out = vec![Moment { at: 0.0, beat: Beat::Sound(Sound::Air) }];
    out.extend(trail_puffs(&course, course.p.trail, 0.2, course.total - 0.3, false, 0.0));
    if reach >= FOLLOW_REACH {
        out.push(Moment { at: course.total * 0.35, beat: Beat::Camera(to) });
    }
    out
}

// ---------------------------------------------------------------- nuclear launches

fn nuke_course(from: [f32; 2], to: [f32; 2], reach: u8) -> Course {
    Course::new(Route::OneWay, FlightClass::Ballistic, from, to, reach)
}

/// The launch lasts until the blast, plus a moment for the flash to be seen.
pub fn nuke_seconds(reach: u8) -> f32 {
    let (from, to) = tiles_to_xz(reach);
    nuke_course(from, to, reach).total + 0.35
}

pub fn nuke_look(from: TileId, target: TileId, reach: u8, t: f32, map: Map) -> Look {
    let course = nuke_course(xz(map, from), xz(map, target), reach);
    let end = course.total;
    if t >= course.hit() {
        return Look { gait: Gait::Hidden, ..course.look(end, map, false) };
    }
    course.look(t, map, false)
}

pub fn nuke_moments(from: TileId, target: TileId, reach: u8, map: Map) -> Vec<Moment> {
    let course = nuke_course(xz(map, from), xz(map, target), reach);
    let to = xz(map, target);
    let hit = course.hit();
    let blast_tiles = 2.0;
    let mut out = vec![
        Moment { at: 0.0, beat: Beat::Sound(Sound::Air) },
        // Ignition at the silo: flash, flame, a rolling cloud.
        Moment { at: 0.0, beat: Beat::Fx(FxEvent::Air(AirFx::Launch { at: xz(map, from), heavy: true })) },
        // The warning: a pulsing ring on the blast area from the launch to the impact.
        Moment {
            at: 0.0,
            beat: Beat::Mark(Mark { kind: MarkKind::Warning { radius: blast_tiles * TILE }, at: to, tile: target, seconds: hit }),
        },
        Moment { at: (hit - 0.9).max(0.0), beat: Beat::Camera(target) },
        Moment { at: hit, beat: Beat::Sound(Sound::Death) },
        Moment { at: hit, beat: Beat::Fx(FxEvent::Air(AirFx::Blast { at: to, radius: BLAST_RADIUS })) },
        Moment { at: hit, beat: Beat::Mark(Mark { kind: MarkKind::Flash, at: to, tile: target, seconds: 0.5 }) },
    ];
    out.extend(trail_puffs(&course, Trail::Rocket, 0.05, hit - 0.02, false, 0.0));
    out
}

// ---------------------------------------------------------------- paradrop and airlift

fn transport_course(from: [f32; 2], to: [f32; 2], paradrop: bool, reach: u8) -> Course {
    let route = if paradrop { Route::Sortie } else { Route::Transfer };
    Course::new(route, FlightClass::Transport, from, to, reach)
}

pub fn paradrop_seconds(reach: u8) -> f32 {
    let (from, to) = tiles_to_xz(reach);
    let course = transport_course(from, to, true, reach);
    course.total.max(course.hit() + DESCENT + 0.45)
}

pub fn airlift_seconds(reach: u8) -> f32 {
    let (from, to) = tiles_to_xz(reach);
    transport_course(from, to, false, reach).total + 0.25
}

/// The soldier: inside the plane until it is over the drop zone, then sinking under a
/// canopy, then standing (a dust puff).
pub fn paradrop_look(from: TileId, to: TileId, reach: u8, t: f32, map: Map) -> Look {
    let course = transport_course(xz(map, from), xz(map, to), true, reach);
    let drop = course.hit();
    let landing = xz(map, to);
    if t < drop {
        return Look { gait: Gait::Hidden, ..Look::resting(map, from) };
    }
    let u = ((t - drop) / DESCENT).clamp(0.0, 1.0);
    let start = course.altitude(drop).max(0.3);
    if u >= 1.0 {
        let settle = smooth((t - drop - DESCENT) / SETTLE_SECONDS);
        return Look { settle, ..Look::resting(map, to) };
    }
    let sway = (t * 5.0).sin();
    Look {
        position: landing,
        tile: to,
        facing: 0.3 * (t * 2.0).sin(),
        gait: Gait::Flight { altitude: start * (1.0 - u), pitch: 0.0, bank: 0.12 * sway },
        flash: 0.0,
        settle: 0.0,
        hp: [0, 0],
        change: 0.0,
    }
}

pub fn paradrop_moments(from: TileId, to: TileId, reach: u8, map: Map) -> Vec<Moment> {
    let course = transport_course(xz(map, from), xz(map, to), true, reach);
    let drop = course.hit();
    let landing = xz(map, to);
    let start = course.altitude(drop).max(0.3);
    let mut out = vec![
        Moment { at: 0.0, beat: Beat::Sound(Sound::Air) },
        Moment { at: drop, beat: Beat::Fx(FxEvent::Air(AirFx::Canopy { at: landing, altitude: start, seconds: DESCENT })) },
        Moment { at: drop + DESCENT, beat: Beat::Fx(FxEvent::Dust { at: landing, size: 0.35 }) },
        Moment { at: drop + DESCENT, beat: Beat::Sound(Sound::Move) },
    ];
    out.extend(trail_puffs(&course, course.p.trail, 0.2, course.total - 0.3, false, 0.0));
    out
}

pub fn airlift_look(from: TileId, to: TileId, reach: u8, t: f32, map: Map) -> Look {
    let course = transport_course(xz(map, from), xz(map, to), false, reach);
    if t < course.total {
        return Look { gait: Gait::Hidden, ..Look::resting(map, from) };
    }
    Look { settle: smooth((t - course.total) / 0.2), ..Look::resting(map, to) }
}

pub fn airlift_moments(from: TileId, to: TileId, reach: u8, map: Map) -> Vec<Moment> {
    let course = transport_course(xz(map, from), xz(map, to), false, reach);
    let mut out = vec![
        Moment { at: 0.0, beat: Beat::Sound(Sound::Air) },
        Moment { at: 0.05, beat: Beat::Fx(FxEvent::Dust { at: xz(map, from), size: 0.4 }) },
        Moment { at: course.total, beat: Beat::Fx(FxEvent::Dust { at: xz(map, to), size: 0.4 }) },
    ];
    out.extend(trail_puffs(&course, course.p.trail, 0.2, course.total - 0.3, false, 0.0));
    out
}

/// The transport plane of a paradrop or an airlift as a pose of its own (a synthetic id,
/// drawn with the model of the `plane` unit kind).
pub fn extra_poses(cue: &Cue, t: f32, map: Map) -> Vec<UnitPose> {
    let (unit, plane, from, to, reach, paradrop) = match cue {
        Cue::Paradrop { unit, plane, from, to, reach } => (unit, *plane, *from, *to, *reach, true),
        Cue::Airlift { unit, plane, from, to, reach } => (unit, *plane, *from, *to, *reach, false),
        _ => return Vec::new(),
    };
    let course = transport_course(xz(map, from), xz(map, to), paradrop, reach);
    if t >= course.total {
        return Vec::new();
    }
    let look = course.look(t, map, false);
    vec![UnitPose {
        id: plane_id(unit.id.0),
        kind: plane,
        owner: unit.owner,
        tile: look.tile.0,
        position: look.position,
        facing: look.facing,
        gait: look.gait,
        flash: 0.0,
        settle: 0.0,
        hp: [0, 0],
        change: 0.0,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: Map = Map { width: 32 };

    fn pos(look: &Look) -> [f32; 2] {
        look.position
    }

    fn altitude(look: &Look) -> f32 {
        match look.gait {
            Gait::Flight { altitude, .. } => altitude,
            _ => 0.0,
        }
    }

    #[test]
    fn a_rebase_takes_off_cruises_and_lands_without_a_jump() {
        let (from, to) = (TileId(2 * 32 + 3), TileId(5 * 32 + 14));
        let total = flight_seconds(FlightClass::Plane, 14);
        assert!(total > 2.5 && total < 6.0, "{total}");
        let mut last = flight_look(FlightClass::Plane, from, to, 14, 0.0, MAP);
        assert!(altitude(&last) < 0.01, "starts on the ground");
        let mut peak = 0.0f32;
        let mut t = 0.0;
        while t < total {
            t += 1.0 / 60.0;
            let look = flight_look(FlightClass::Plane, from, to, 14, t, MAP);
            let step = (pos(&look)[0] - pos(&last)[0]).hypot(pos(&look)[1] - pos(&last)[1]);
            assert!(step < 0.35, "no jump between frames: {step} at {t}");
            peak = peak.max(altitude(&look));
            last = look;
        }
        assert!(peak > 0.5, "cruises high: {peak}");
        let end = flight_look(FlightClass::Plane, from, to, 14, total, MAP);
        assert!(altitude(&end) < 0.01 && end.settle > 0.99);
        let home = MAP.xz(to);
        assert!((pos(&end)[0] - home[0]).hypot(pos(&end)[1] - home[1]) < 0.01, "lands on the destination");
    }

    #[test]
    fn a_bombing_run_is_over_the_target_at_the_hit_and_home_at_the_end() {
        let attack = Attack::test(TileId(3 * 32 + 3), TileId(3 * 32 + 8), 5, Weapon::Bomb);
        let timing = attack_timing(&attack);
        assert!(timing.release < timing.hit && timing.hit < timing.seconds);
        let over = attacker_look(&attack, timing.hit, MAP);
        let target = MAP.xz(attack.target);
        assert!((pos(&over)[0] - target[0]).hypot(pos(&over)[1] - target[1]) < 0.1, "over the target");
        assert!(altitude(&over) > 0.3, "flying, not standing");
        let end = attacker_look(&attack, timing.seconds, MAP);
        let base = MAP.xz(attack.from);
        assert!((pos(&end)[0] - base[0]).hypot(pos(&end)[1] - base[1]) < 0.05, "back at the base");
    }

    #[test]
    fn it_banks_in_the_turn_and_not_on_the_straight() {
        let attack = Attack::test(TileId(3 * 32 + 3), TileId(3 * 32 + 8), 5, Weapon::Bomb);
        let timing = attack_timing(&attack);
        let bank_at = |t: f32| match attacker_look(&attack, t, MAP).gait {
            Gait::Flight { bank, .. } => bank,
            other => panic!("{other:?}"),
        };
        let straight = bank_at(timing.hit - 0.4).abs();
        let turning = (timing.hit..timing.seconds).step_by_f(0.03).into_iter().map(|t| bank_at(t).abs()).fold(0.0, f32::max);
        assert!(straight < 0.08, "level on the way out: {straight}");
        assert!(turning > 0.3, "banked in the turn: {turning}");
    }

    #[test]
    fn a_missile_is_spent_in_its_blast_and_a_nuke_is_a_high_arc() {
        let missile = Attack::test(TileId(3 * 32 + 3), TileId(3 * 32 + 9), 6, Weapon::Missile);
        let timing = attack_timing(&missile);
        assert!(matches!(attacker_look(&missile, timing.hit + 0.1, MAP).gait, Gait::Hidden));
        assert!(altitude(&attacker_look(&missile, timing.hit * 0.5, MAP)) > 0.15);
        let (from, target) = (TileId(3 * 32 + 3), TileId(10 * 32 + 12));
        let mid = nuke_look(from, target, 12, (nuke_seconds(12) - 0.35) * 0.5, MAP);
        assert!(altitude(&mid) > 1.5, "ballistic apex: {}", altitude(&mid));
        assert!(matches!(nuke_look(from, target, 12, nuke_seconds(12), MAP).gait, Gait::Hidden));
        let hit = nuke_moments(from, target, 12, MAP);
        let blasts = hit.iter().filter(|m| matches!(m.beat, Beat::Fx(FxEvent::Air(AirFx::Blast { .. })))).count();
        assert_eq!(blasts, 1);
        let launches = |moments: &[Moment]| moments.iter().filter(|m| m.at == 0.0 && matches!(m.beat, Beat::Fx(FxEvent::Air(AirFx::Launch { .. })))).count();
        assert_eq!(launches(&hit), 1, "a nuclear launch ignites at the silo");
        let missile_fx = strike_fx(&missile, MAP);
        assert_eq!(launches(&missile_fx), 1, "a missile ignites at its base");
        assert!(missile_fx.iter().any(|m| matches!(m.beat, Beat::Fx(FxEvent::Fire { .. }))), "and burns on at the target");
    }

    #[test]
    fn the_paratrooper_rides_then_sinks_then_stands() {
        let (from, to) = (TileId(3 * 32 + 3), TileId(3 * 32 + 9));
        let total = paradrop_seconds(6);
        assert!(matches!(paradrop_look(from, to, 6, 0.2, MAP).gait, Gait::Hidden));
        let drop = transport_course(MAP.xz(from), MAP.xz(to), true, 6).hit();
        let falling = paradrop_look(from, to, 6, drop + DESCENT * 0.5, MAP);
        assert!(altitude(&falling) > 0.1 && falling.tile == to);
        let down = paradrop_look(from, to, 6, total, MAP);
        assert!(matches!(down.gait, Gait::Stand) && down.tile == to);
    }

    trait StepF {
        fn step_by_f(self, step: f32) -> Vec<f32>;
    }
    impl StepF for std::ops::Range<f32> {
        fn step_by_f(self, step: f32) -> Vec<f32> {
            let mut out = Vec::new();
            let mut t = self.start;
            while t < self.end {
                out.push(t);
                t += step;
            }
            out
        }
    }
}
