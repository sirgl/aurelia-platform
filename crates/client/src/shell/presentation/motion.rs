//! How a cue looks at a moment: unit looks (position, facing, gait) and the timed
//! beats (sounds, map marks, camera) it fires. Pure functions of the cue, its local
//! time and the looks its units had when it started.
use kernel::ids::{EntityId, TileId};
use render::gpu::effects::{ExplosionKind, FxEvent, ImpactKind, MuzzleKind, ProjectileKind};
use render::gpu::unit_poses::{facing_towards, Gait};
use render::unit_models::Weapon;

use super::air;
use super::cue::{self, flight_seconds, Attack, AttackStyle, CityStrike, Cue, Figure, Timing};

/// One unit's look at one moment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub position: [f32; 2],
    pub tile: TileId,
    pub facing: f32,
    pub gait: Gait,
    pub flash: f32,
    pub settle: f32,
    /// Health in percent before and after the blow the unit took, and how far the figures
    /// that left are through falling (`UnitPose::hp`); zeros: whatever the projection shows.
    pub hp: [u8; 2],
    pub change: f32,
}

impl Look {
    /// Standing at rest on a tile.
    pub fn resting(map: Map, tile: TileId) -> Self {
        Self { position: map.xz(tile), tile, facing: 0.0, gait: Gait::Stand, flash: 0.0, settle: 1.0, hp: [0, 0], change: 0.0 }
    }

    /// Gone (a fallen unit waiting for the projection to drop it).
    pub fn gone(self) -> Self {
        Self { gait: Gait::Fall { progress: 1.0 }, flash: 0.0, settle: 0.0, ..self }
    }
}

/// Map geometry the cues need: tile centres.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Map {
    pub width: u32,
}

impl Map {
    pub fn xz(self, tile: TileId) -> [f32; 2] {
        let width = self.width.max(1);
        let (x, z) = render::scene::layout::tile_to_world(tile.0 % width, tile.0 / width);
        [x, z]
    }
}

/// What a cue fires at a moment.
#[derive(Debug, Clone, PartialEq)]
pub enum Beat {
    Sound(Sound),
    Mark(Mark),
    /// A 3D effect on the map (explosion, projectile, muzzle flash): `render::gpu::effects`.
    Fx(FxEvent),
    /// Glide the camera to the tile (wonders).
    Camera(TileId),
}

/// Sounds of the game (the audio layer maps them to files).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sound {
    Select,
    Move,
    Melee,
    Archery,
    Gunpowder,
    Artillery,
    Naval,
    Air,
    Death,
    CityFounded,
    Building,
    Wonder,
    Tech,
    /// A decision of the empire's government.
    Decree,
    /// A sacred event.
    Hymn,
    Notification,
}

impl Sound {
    pub fn of_attack(style: AttackStyle) -> Self {
        match style {
            AttackStyle::Melee | AttackStyle::Mounted => Sound::Melee,
            AttackStyle::Archery => Sound::Archery,
            AttackStyle::Gunpowder => Sound::Gunpowder,
            AttackStyle::Artillery => Sound::Artillery,
            AttackStyle::Naval => Sound::Naval,
            AttackStyle::Air => Sound::Air,
        }
    }
}

/// A map mark: drawn by the HUD over the world for its lifetime.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub kind: MarkKind,
    pub at: [f32; 2],
    pub tile: TileId,
    pub seconds: f32,
}

/// A floating word, in the module's words when drawn ("+1", "Captured", "Destroyed").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Floating {
    Grew,
    Captured,
    Destroyed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarkKind {
    /// Floating damage number (red for the observer's losses is the HUD's choice).
    Damage { amount: u16, owner: u8 },
    /// Floating word over the tile.
    Word(Floating),
    /// Combat result flash (a 2D ring: melee blows).
    Flash,
    /// City founded or grown: an expanding ring.
    Puff { large: bool },
    /// Building or wonder completed.
    Sparkle { large: bool },
    /// A nuclear launch: a pulsing ring over the blast area (radius in world units).
    Warning { radius: f32 },
}

/// A beat at a cue-local time (seconds at normal speed).
#[derive(Debug, Clone, PartialEq)]
pub struct Moment {
    pub at: f32,
    pub beat: Beat,
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Shortest-arc blend between two yaws.
fn turn(from: f32, to: f32, t: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let delta = (to - from + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    from + delta * t.clamp(0.0, 1.0)
}

fn bump(t: f32, start: f32, length: f32) -> f32 {
    let u = (t - start) / length;
    if !(0.0..=1.0).contains(&u) {
        return 0.0;
    }
    (u * std::f32::consts::PI).sin()
}

/// Steps of the walk cycle per tile.
const STEPS_PER_TILE: f32 = 3.0;

/// The look of `figure` in `cue` at local time `t`; `entry` is its look when the cue
/// started. None: the cue does not draw this unit.
pub fn look(cue: &Cue, figure: EntityId, t: f32, entry: Look, map: Map) -> Option<Look> {
    match cue {
        Cue::Move { unit, path } if unit.id == figure => Some(walk(path, t, map)),
        Cue::Attack(attack) => attack_look(attack, figure, t, entry, map),
        Cue::CityStrike(strike) if strike.defender.is_some_and(|d| d.id == figure) => Some(struck_look(strike, t, entry, map)),
        // A spent missile or bomb stays out of sight (it is gone in its blast).
        Cue::Death { unit, .. } | Cue::Vanish { unit, .. } if unit.id == figure && matches!(entry.gait, Gait::Hidden) => Some(entry),
        Cue::Death { unit, .. } if unit.id == figure => {
            Some(Look { gait: Gait::Fall { progress: t / cue::DEATH_SECONDS }, settle: 0.0, flash: 0.0, ..entry })
        }
        Cue::Vanish { unit, .. } if unit.id == figure => {
            Some(Look { gait: Gait::Fall { progress: t / cue::VANISH_SECONDS }, settle: 0.0, flash: 0.0, ..entry })
        }
        Cue::Flight { unit, from, to, class, reach } if unit.id == figure => Some(air::flight_look(*class, *from, *to, *reach, t, map)),
        Cue::Nuke { unit, from, target, reach, .. } if unit.id == figure => Some(air::nuke_look(*from, *target, *reach, t, map)),
        Cue::Paradrop { unit, from, to, reach, .. } if unit.id == figure => Some(air::paradrop_look(*from, *to, *reach, t, map)),
        Cue::Airlift { unit, from, to, reach, .. } if unit.id == figure => Some(air::airlift_look(*from, *to, *reach, t, map)),
        Cue::Fortify { unit, tile } if unit.id == figure => {
            let u = ease(t / cue::FORTIFY_SECONDS);
            let rest = Look::resting(map, *tile);
            Some(Look { facing: turn(entry.facing, 0.0, u), settle: u, ..rest })
        }
        _ => None,
    }
}

/// The look a unit has before its cue starts.
pub fn first_look(cue: &Cue, figure: &Figure, map: Map) -> Option<Look> {
    let tile = match cue {
        Cue::Move { unit, path } if unit.id == figure.id => *path.first()?,
        Cue::Attack(attack) if attack.attacker.id == figure.id => attack.from,
        Cue::Attack(attack) if attack.defender.is_some_and(|d| d.id == figure.id) => attack.target,
        Cue::CityStrike(strike) if strike.defender.is_some_and(|d| d.id == figure.id) => strike.target,
        Cue::Death { tile, .. } | Cue::Vanish { tile, .. } | Cue::Fortify { tile, .. } => *tile,
        Cue::Flight { unit, from, .. } | Cue::Nuke { unit, from, .. } | Cue::Paradrop { unit, from, .. } | Cue::Airlift { unit, from, .. }
            if unit.id == figure.id =>
        {
            *from
        }
        _ => return None,
    };
    Some(Look::resting(map, tile))
}

fn walk(path: &[TileId], t: f32, map: Map) -> Look {
    let walk = cue::walk_seconds(path);
    let segments = path.len().saturating_sub(1) as f32;
    let end = *path.last().expect("a move has tiles");
    let heading = |s: f32| {
        let (a, b) = (along(path, s - 0.2, map), along(path, s + 0.2, map));
        facing_towards(b[0] - a[0], b[1] - a[1])
    };
    if t < walk {
        let s = ease(t / walk) * segments;
        let index = (s.round() as usize).min(path.len() - 1);
        let gait = Gait::Walk { phase: s * STEPS_PER_TILE };
        return Look { position: along(path, s, map), tile: path[index], facing: heading(s), gait, flash: 0.0, settle: 0.0, hp: [0, 0], change: 0.0 };
    }
    let u = ease((t - walk) / cue::SETTLE_SECONDS);
    let rest = Look::resting(map, end);
    Look { facing: turn(heading(segments), 0.0, u), settle: u, ..rest }
}

/// Point at `s` tiles along the path (clamped to its ends).
fn along(path: &[TileId], s: f32, map: Map) -> [f32; 2] {
    let last = path.len() - 1;
    let s = s.clamp(0.0, last as f32);
    let i = (s.floor() as usize).min(last.saturating_sub(1));
    let (a, b) = (map.xz(path[i]), map.xz(path[(i + 1).min(last)]));
    let f = s - i as f32;
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]
}

fn attack_look(attack: &Attack, figure: EntityId, t: f32, entry: Look, map: Map) -> Option<Look> {
    let (from, to) = (map.xz(attack.from), map.xz(attack.target));
    if attack.attacker.id == figure {
        if attack.style == AttackStyle::Air {
            return Some(air::attacker_look(attack, t, map));
        }
        let facing = facing_towards(to[0] - from[0], to[1] - from[1]);
        return Some(attacker_look(attack, t, facing, entry));
    }
    if attack.defender.is_some_and(|d| d.id == figure) {
        let facing = facing_towards(from[0] - to[0], from[1] - to[1]);
        return Some(defender_look(attack, t, facing, entry));
    }
    None
}

fn hit_time(attack: &Attack) -> f32 {
    attack.timing().hit
}

fn attacker_look(attack: &Attack, t: f32, facing: f32, entry: Look) -> Look {
    let timing = Timing::of(attack.weapon);
    let (duration, hit) = (timing.seconds, timing.hit);
    // Wind-up until the weapon fires or the blow starts, then the follow-through.
    let aim = ease(t / timing.release.max(1e-3));
    let strike = ((t - timing.release) / (duration - timing.release).max(1e-3)).clamp(0.0, 1.0);
    let strike = if t < timing.release { 0.0 } else { strike.max(1e-4) };
    let settle = ease((t - (duration - cue::SETTLE_SECONDS)) / cue::SETTLE_SECONDS);
    let facing = turn(turn(entry.facing, facing, t / 0.15), 0.0, settle);
    let flash = if attack.taken.is_some() { bump(t, hit + 0.1, 0.3) } else { 0.0 };
    // Figures the attacker lost in the exchange fall at the blow.
    let (hp, change) = lost_figures(attack.attacker_hp, t, hit);
    Look { facing, gait: Gait::Attack { aim, strike }, flash, settle, hp, change, ..entry }
}

/// Health before and after a blow that lands at `hit`, and the progress of the fall of
/// the figures it took (nothing known: the projection's).
fn lost_figures(hp: Option<[u8; 2]>, t: f32, hit: f32) -> ([u8; 2], f32) {
    match hp {
        Some(hp) => (hp, ease((t - hit) / 0.6)),
        None => ([0, 0], 0.0),
    }
}

fn defender_look(attack: &Attack, t: f32, facing: f32, entry: Look) -> Look {
    let duration = attack_seconds(attack);
    let hit = hit_time(attack);
    let settle = match attack.kill {
        true => 0.0,
        false => ease((t - (duration - cue::SETTLE_SECONDS)) / cue::SETTLE_SECONDS),
    };
    let facing = turn(turn(entry.facing, facing, t / 0.25), 0.0, settle);
    let amount = bump(t, hit, 0.35);
    let flash = bump(t, hit - 0.02, 0.3);
    let (hp, change) = lost_figures(attack.defender_hp, t, hit);
    Look { facing, gait: Gait::Recoil { amount }, flash, settle, hp, change, ..entry }
}

fn attack_seconds(attack: &Attack) -> f32 {
    attack.timing().seconds
}

/// Every beat of a cue, by local time.
pub fn moments(cue: &Cue, map: Map) -> Vec<Moment> {
    let now = |beat| Moment { at: 0.0, beat };
    let mark = |kind, tile: TileId, seconds| Beat::Mark(Mark { kind, at: map.xz(tile), tile, seconds });
    match cue {
        Cue::Move { .. } => vec![now(Beat::Sound(Sound::Move))],
        Cue::Attack(attack) => attack_moments(attack, map),
        Cue::CityStrike(strike) => city_strike_moments(strike, map),
        Cue::Death { .. } => vec![now(Beat::Sound(Sound::Death))],
        Cue::Vanish { .. } | Cue::Fortify { .. } => Vec::new(),
        Cue::CityFounded { tile, .. } => vec![
            now(Beat::Sound(Sound::CityFounded)),
            now(mark(MarkKind::Puff { large: true }, *tile, 1.2)),
        ],
        Cue::CityGrew { tile } => vec![
            now(mark(MarkKind::Puff { large: false }, *tile, 1.0)),
            now(mark(MarkKind::Word(Floating::Grew), *tile, 1.4)),
        ],
        Cue::CityCaptured { tile, .. } => vec![
            now(Beat::Sound(Sound::Notification)),
            now(mark(MarkKind::Flash, *tile, 0.5)),
            now(mark(MarkKind::Word(Floating::Captured), *tile, 1.8)),
        ],
        Cue::BuildingCompleted { tile, wonder: false } => vec![
            now(Beat::Sound(Sound::Building)),
            now(mark(MarkKind::Sparkle { large: false }, *tile, 1.2)),
        ],
        Cue::BuildingCompleted { tile, wonder: true } => vec![
            now(Beat::Camera(*tile)),
            Moment { at: 0.5, beat: Beat::Sound(Sound::Wonder) },
            Moment { at: 0.5, beat: mark(MarkKind::Sparkle { large: true }, *tile, 1.9) },
        ],
        Cue::Announcement(sound) => vec![now(Beat::Sound(*sound))],
        Cue::Flight { from, to, reach, class, .. } => air::flight_moments(*class, *from, *to, *reach, map),
        Cue::Nuke { from, target, reach, .. } => air::nuke_moments(*from, *target, *reach, map),
        Cue::Paradrop { from, to, reach, .. } => air::paradrop_moments(*from, *to, *reach, map),
        Cue::Airlift { from, to, reach, .. } => air::airlift_moments(*from, *to, *reach, map),
    }
}

/// The unit a city strike hits recoils at the hit (and settles unless it
/// dies).
fn struck_look(strike: &CityStrike, t: f32, entry: Look, map: Map) -> Look {
    let (from, to) = (map.xz(strike.city), map.xz(strike.target));
    let facing = facing_towards(from[0] - to[0], from[1] - to[1]);
    let settle = match strike.kill {
        true => 0.0,
        false => ease((t - (cue::SHOT_SECONDS - cue::SETTLE_SECONDS)) / cue::SETTLE_SECONDS),
    };
    let facing = turn(turn(entry.facing, facing, t / 0.25), 0.0, settle);
    let amount = bump(t, cue::SHOT_HIT, 0.35);
    let flash = bump(t, cue::SHOT_HIT - 0.02, 0.3);
    let (hp, change) = lost_figures(strike.defender_hp, t, cue::SHOT_HIT);
    Look { facing, gait: Gait::Recoil { amount }, flash, settle, hp, change, ..entry }
}

/// A city strike: an arrow volley leaves the city, the target flashes and
/// shows the damage (or "Destroyed").
fn city_strike_moments(strike: &CityStrike, map: Map) -> Vec<Moment> {
    let (from, to) = (map.xz(strike.city), map.xz(strike.target));
    let style = AttackStyle::Archery;
    let at_target = |kind, seconds| Beat::Mark(Mark { kind, at: to, tile: strike.target, seconds });
    let flight = cue::SHOT_HIT - cue::SHOT_RELEASE;
    let mut moments = vec![Moment { at: cue::SHOT_RELEASE, beat: Beat::Sound(Sound::of_attack(style)) }];
    // A volley from the walls: three arrows side by side.
    for (k, side) in [-0.18f32, 0.0, 0.18].into_iter().enumerate() {
        let (dx, dz) = (to[0] - from[0], to[1] - from[1]);
        let len = dx.hypot(dz).max(1e-3);
        let across = [-dz / len * side, dx / len * side];
        let fx = FxEvent::Projectile {
            from: [from[0] + across[0], from[1] + across[1]],
            to: [to[0] + across[0], to[1] + across[1]],
            kind: ProjectileKind::Arrow,
            seconds: flight,
        };
        moments.push(Moment { at: cue::SHOT_RELEASE + 0.04 * k as f32, beat: Beat::Fx(fx) });
    }
    moments.push(Moment { at: cue::SHOT_HIT, beat: Beat::Fx(FxEvent::Impact { at: to, kind: ImpactKind::Arrow }) });
    let owner = strike.defender.map_or(strike.owner, |d| d.owner);
    let result = match (strike.dealt, strike.kill) {
        (Some(amount), _) => Some(MarkKind::Damage { amount, owner }),
        (None, true) => Some(MarkKind::Word(Floating::Destroyed)),
        (None, false) => None,
    };
    if let Some(kind) = result {
        moments.push(Moment { at: cue::SHOT_HIT, beat: at_target(kind, 1.5) });
    }
    moments
}

/// The effects of one weapon's attack from `from` to `to` (XZ): the muzzle flash and the
/// flying shot at the release, the explosion or the small impact at the hit.
fn weapon_fx(weapon: Weapon, from: [f32; 2], to: [f32; 2], figures: u8, timing: Timing) -> Vec<Moment> {
    let flight = flight_seconds(weapon);
    let (release, hit) = (timing.release, timing.hit);
    let at = |at: f32, fx: FxEvent| Moment { at, beat: Beat::Fx(fx) };
    let (dx, dz) = (to[0] - from[0], to[1] - from[1]);
    let len = dx.hypot(dz).max(1e-3);
    let across = [-dz / len, dx / len];
    // Figures of a formation fire side by side, a hair apart.
    let shooters = (figures as usize).clamp(1, 3);
    let offset = |k: usize| -> [f32; 2] {
        let side = (k as f32 - (shooters as f32 - 1.0) / 2.0) * 0.22;
        [across[0] * side, across[1] * side]
    };
    let shift = |p: [f32; 2], o: [f32; 2]| [p[0] + o[0], p[1] + o[1]];
    let mut moments = Vec::new();
    match weapon {
        Weapon::Bow | Weapon::Crossbow | Weapon::Thrown | Weapon::Ballista => {
            let kind = match weapon {
                Weapon::Bow => ProjectileKind::Arrow,
                Weapon::Thrown => ProjectileKind::Stone,
                _ => ProjectileKind::Bolt,
            };
            for k in 0..shooters {
                let o = offset(k);
                let delay = 0.04 * k as f32;
                moments.push(at(release + delay, FxEvent::Projectile { from: shift(from, o), to: shift(to, o), kind, seconds: flight }));
            }
            moments.push(at(hit, FxEvent::Impact { at: to, kind: if weapon == Weapon::Thrown { ImpactKind::Blunt } else { ImpactKind::Arrow } }));
        }
        Weapon::Firearm | Weapon::Autogun => {
            let bursts = if weapon == Weapon::Autogun { 3 } else { 1 };
            for k in 0..shooters {
                let o = offset(k);
                for b in 0..bursts {
                    let delay = 0.03 * k as f32 + 0.07 * b as f32;
                    moments.push(at(release + delay, FxEvent::Muzzle { at: shift(from, o), toward: shift(to, o), kind: MuzzleKind::Rifle }));
                    moments.push(at(release + delay, FxEvent::Projectile { from: shift(from, o), to: shift(to, o), kind: ProjectileKind::Bullet, seconds: flight }));
                    moments.push(at(release + delay + flight, FxEvent::Impact { at: shift(to, o), kind: ImpactKind::Bullet }));
                }
            }
        }
        Weapon::Rocket | Weapon::Cannon | Weapon::Catapult | Weapon::Broadside | Weapon::Torpedo | Weapon::Bomb | Weapon::Missile => {
            let (muzzle, shot, blast, size) = match weapon {
                Weapon::Rocket => (Some(MuzzleKind::Rocket), ProjectileKind::Rocket, ExplosionKind::Shell, 0.5),
                Weapon::Cannon => (Some(MuzzleKind::Cannon), ProjectileKind::Shell, ExplosionKind::Shell, 0.6),
                Weapon::Catapult => (None, ProjectileKind::Stone, ExplosionKind::Rubble, 0.5),
                Weapon::Broadside => (Some(MuzzleKind::Broadside), ProjectileKind::Shell, ExplosionKind::Shell, 0.5),
                Weapon::Torpedo => (None, ProjectileKind::Shell, ExplosionKind::Shell, 0.6),
                Weapon::Bomb => (None, ProjectileKind::Bomb, ExplosionKind::Bomb, 0.8),
                _ => (None, ProjectileKind::Missile, ExplosionKind::Bomb, 0.9),
            };
            let rounds = if weapon == Weapon::Broadside { 2 } else { 1 };
            for r in 0..rounds {
                let o = offset(r % shooters);
                let delay = 0.08 * r as f32;
                if let Some(kind) = muzzle {
                    moments.push(at(release + delay, FxEvent::Muzzle { at: shift(from, o), toward: shift(to, o), kind }));
                }
                moments.push(at(release + delay, FxEvent::Projectile { from: shift(from, o), to: shift(to, o), kind: shot, seconds: flight }));
                moments.push(at(release + delay + flight, FxEvent::Explosion { at: shift(to, o), size: size * if r == 0 { 1.0 } else { 0.8 }, kind: blast }));
            }
        }
        Weapon::None | Weapon::Blade | Weapon::Club | Weapon::Polearm | Weapon::Ram => {
            let kind = match weapon {
                Weapon::Blade | Weapon::Polearm => ImpactKind::Blade,
                _ => ImpactKind::Blunt,
            };
            moments.push(at(hit, FxEvent::Impact { at: to, kind }));
            if weapon == Weapon::Ram {
                moments.push(at(hit, FxEvent::Dust { at: to, size: 0.6 }));
            }
        }
    }
    moments
}

fn attack_moments(attack: &Attack, map: Map) -> Vec<Moment> {
    let timing = attack.timing();
    let hit = timing.hit;
    let (from, to) = (map.xz(attack.from), map.xz(attack.target));
    let at_target = |kind, seconds| Beat::Mark(Mark { kind, at: to, tile: attack.target, seconds });
    let mut moments = Vec::new();
    if attack.weapon.is_ranged() {
        moments.push(Moment { at: timing.release, beat: Beat::Sound(Sound::of_attack(attack.style)) });
    } else {
        moments.push(Moment { at: hit - 0.08, beat: Beat::Sound(Sound::of_attack(attack.style)) });
        moments.push(Moment { at: hit, beat: at_target(MarkKind::Flash, 0.35) });
    }
    if attack.style == AttackStyle::Air {
        moments.extend(air::strike_fx(attack, map));
    } else {
        moments.extend(weapon_fx(attack.weapon, from, to, attack.figures, timing));
    }
    let owner = attack.target_owner;
    let result = match (attack.dealt, attack.kill) {
        (Some(amount), _) => Some(MarkKind::Damage { amount, owner }),
        (None, true) => Some(MarkKind::Word(Floating::Destroyed)),
        (None, false) => None,
    };
    if let Some(kind) = result {
        moments.push(Moment { at: hit, beat: at_target(kind, 1.5) });
    }
    if let Some(amount) = attack.taken.filter(|_| attack.attacker_seen) {
        let kind = MarkKind::Damage { amount, owner: attack.attacker.owner };
        let mark = Mark { kind, at: from, tile: attack.from, seconds: 1.5 };
        moments.push(Moment { at: hit + 0.15, beat: Beat::Mark(mark) });
    }
    moments
}
