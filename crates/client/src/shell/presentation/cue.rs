//! Presentation events ("cues"): what the player should see and hear of one applied
//! command. Cues are client-side only: they name units by persistent `EntityId` and
//! carry everything they draw (kind, owner, tiles), so playing them never reads the
//! game and never touches a stale handle.
use kernel::ids::{EntityId, TileId};

use render::gpu::flight::FlightClass;
use render::unit_models::Weapon;

use super::motion::Sound;

/// A unit as the presentation draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Figure {
    pub id: EntityId,
    pub kind: u16,
    pub owner: u8,
}

/// How an attack looks and sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttackStyle {
    /// Swords, spears, clubs.
    Melee,
    /// Horses and chariots.
    Mounted,
    /// Bows and crossbows: an arrow volley.
    Archery,
    /// Muskets and rifles.
    Gunpowder,
    /// Catapults, cannons, artillery: a shell.
    Artillery,
    /// Ships: a broadside.
    Naval,
    /// Aircraft and missiles.
    Air,
}

impl AttackStyle {
    /// The attack sends a projectile across (the attacker does not lunge into the target).
    pub fn shoots(self) -> bool {
        matches!(self, Self::Archery | Self::Gunpowder | Self::Artillery | Self::Naval | Self::Air)
    }
}

/// When an attack's moments fall, in seconds at normal speed from the start of its cue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    /// The weapon fires or the blow starts: the wind-up (aim) ends.
    pub release: f32,
    /// The hit lands: damage, flash, the target's reaction.
    pub hit: f32,
    /// Length of the cue.
    pub seconds: f32,
}

/// Seconds a shot flies, by weapon (a bullet is quick, a stone from a catapult is slow).
pub fn flight_seconds(weapon: Weapon) -> f32 {
    match weapon {
        Weapon::Bow => 0.36,
        Weapon::Crossbow => 0.26,
        Weapon::Thrown => 0.34,
        Weapon::Firearm => 0.14,
        Weapon::Autogun => 0.12,
        Weapon::Rocket => 0.32,
        Weapon::Cannon => 0.22,
        Weapon::Catapult => 0.44,
        Weapon::Ballista => 0.28,
        Weapon::Broadside => 0.30,
        Weapon::Torpedo => 0.50,
        Weapon::Bomb => 0.45,
        Weapon::Missile => 0.45,
        Weapon::None | Weapon::Blade | Weapon::Club | Weapon::Polearm | Weapon::Ram => 0.0,
    }
}

impl Timing {
    pub fn of(weapon: Weapon) -> Self {
        if weapon.is_ranged() {
            Self { release: SHOT_HIT - flight_seconds(weapon), hit: SHOT_HIT, seconds: SHOT_SECONDS }
        } else {
            // The blow starts a little before it lands: the lunge reaches the target at the hit.
            Self { release: MELEE_HIT - 0.12, hit: MELEE_HIT, seconds: MELEE_SECONDS }
        }
    }
}

/// One attack: the attacker strikes the target tile; the numbers are the damage the
/// journal recorded (None: no damage recorded, e.g. a city's hit points or a kill).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attack {
    pub attacker: Figure,
    pub from: TileId,
    pub target: TileId,
    /// The defending unit (None for a city).
    pub defender: Option<Figure>,
    pub style: AttackStyle,
    /// What the attacker's model fights with: its attack animation and effects.
    pub weapon: Weapon,
    /// Figures in the attacker's formation (volleys come from each).
    pub figures: u8,
    /// Health in percent (before, after) of the defender and of the attacker when this blow
    /// took figures from them (None: no change shown).
    pub defender_hp: Option<[u8; 2]>,
    pub attacker_hp: Option<[u8; 2]>,
    /// Tiles between the attacker and the target (an air strike flies there and back).
    pub reach: u8,
    pub dealt: Option<u16>,
    pub taken: Option<u16>,
    /// The defender died in this attack (its `Death` cue follows).
    pub kill: bool,
    /// The observer sees the attacker's tile (else only the hit is shown).
    pub attacker_seen: bool,
    /// Owner of what was hit (the defending unit or the city): the colour
    /// of the damage number.
    pub target_owner: u8,
}

/// A city's ranged strike on a unit: an arrow volley from the city to the
/// target with the damage dealt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityStrike {
    pub city: TileId,
    pub owner: u8,
    pub target: TileId,
    pub defender: Option<Figure>,
    pub dealt: Option<u16>,
    /// Health in percent (before, after) of the defender.
    pub defender_hp: Option<[u8; 2]>,
    /// The defender died (its `Death` cue follows).
    pub kill: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cue {
    /// A unit walks the tiles of `path` (the first is where it starts).
    Move { unit: Figure, path: Vec<TileId> },
    Attack(Attack),
    CityStrike(CityStrike),
    /// The unit falls (killed).
    Death { unit: Figure, tile: TileId },
    /// The unit leaves the map without a fight (a settler founds a city, a great person
    /// is consumed).
    Vanish { unit: Figure, tile: TileId },
    Fortify { unit: Figure, tile: TileId },
    CityFounded { tile: TileId, owner: u8 },
    CityGrew { tile: TileId },
    CityCaptured { tile: TileId, owner: u8 },
    BuildingCompleted { tile: TileId, wonder: bool },
    /// A notice of the observer's empire that has no place on the map: its sound.
    Announcement(Sound),
    /// An aircraft or missile changes base (rebase): take-off, an arc, landing.
    Flight { unit: Figure, from: TileId, to: TileId, reach: u8, class: FlightClass },
    /// A nuclear weapon is launched at `target` and detonates there; `blast` are the tiles
    /// of the blast (what falls in it falls after the flash).
    Nuke { unit: Figure, from: TileId, target: TileId, reach: u8, blast: Vec<TileId> },
    /// Paratroopers jump from a transport plane (`plane`: the unit kind drawn as it).
    Paradrop { unit: Figure, from: TileId, to: TileId, reach: u8, plane: u16 },
    /// A land unit flies between two airports in a transport plane.
    Airlift { unit: Figure, from: TileId, to: TileId, reach: u8, plane: u16 },
}

/// Whose action a cue shows: the speed setting applies to the other players'.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Own,
    Other,
}

/// What a cue occupies while it plays: cues sharing a key play in queue order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Unit(EntityId),
    Tile(TileId),
}

/// A cue with its origin, as derived from the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    pub cue: Cue,
    pub origin: Origin,
    /// The cue concerns the observer's units or cities (shown even when the other
    /// players' actions are skipped).
    pub concerns_observer: bool,
}

/// Seconds at normal speed.
pub const STEP_SECONDS: f32 = 0.3;
pub const SETTLE_SECONDS: f32 = 0.22;
pub const MELEE_SECONDS: f32 = 0.85;
pub const MELEE_HIT: f32 = 0.42;
pub const SHOT_SECONDS: f32 = 1.0;
pub const SHOT_RELEASE: f32 = 0.22;
pub const SHOT_HIT: f32 = 0.58;
pub const DEATH_SECONDS: f32 = 0.9;
pub const VANISH_SECONDS: f32 = 0.45;
pub const FORTIFY_SECONDS: f32 = 0.4;
pub const CITY_SECONDS: f32 = 1.2;
pub const WONDER_SECONDS: f32 = 2.4;

impl Cue {
    /// Length at normal speed.
    pub fn duration(&self) -> f32 {
        match self {
            Cue::Move { path, .. } => walk_seconds(path) + SETTLE_SECONDS,
            Cue::Attack(attack) => attack.timing().seconds,
            Cue::Flight { reach, class, .. } => super::air::flight_seconds(*class, *reach),
            Cue::Nuke { reach, .. } => super::air::nuke_seconds(*reach),
            Cue::Paradrop { reach, .. } => super::air::paradrop_seconds(*reach),
            Cue::Airlift { reach, .. } => super::air::airlift_seconds(*reach),
            Cue::CityStrike(_) => SHOT_SECONDS,
            Cue::Death { .. } => DEATH_SECONDS,
            Cue::Vanish { .. } => VANISH_SECONDS,
            Cue::Fortify { .. } => FORTIFY_SECONDS,
            Cue::BuildingCompleted { wonder: true, .. } => WONDER_SECONDS,
            Cue::CityFounded { .. }
            | Cue::CityGrew { .. }
            | Cue::CityCaptured { .. }
            | Cue::BuildingCompleted { .. } => CITY_SECONDS,
            Cue::Announcement(_) => 0.0,
        }
    }

    /// Units and tiles the cue occupies.
    pub fn keys(&self) -> Vec<Key> {
        match self {
            Cue::Move { unit, .. } | Cue::Fortify { unit, .. } => vec![Key::Unit(unit.id)],
            Cue::Flight { unit, to, .. } | Cue::Paradrop { unit, to, .. } | Cue::Airlift { unit, to, .. } => {
                vec![Key::Unit(unit.id), Key::Tile(*to)]
            }
            Cue::Nuke { unit, blast, .. } => {
                let mut keys = vec![Key::Unit(unit.id)];
                keys.extend(blast.iter().map(|t| Key::Tile(*t)));
                keys
            }
            Cue::Death { unit, tile } | Cue::Vanish { unit, tile } => vec![Key::Unit(unit.id), Key::Tile(*tile)],
            Cue::Attack(attack) => {
                let mut keys = vec![Key::Unit(attack.attacker.id), Key::Tile(attack.target)];
                if let Some(defender) = attack.defender {
                    keys.push(Key::Unit(defender.id));
                }
                keys
            }
            Cue::CityStrike(strike) => {
                let mut keys = vec![Key::Tile(strike.city), Key::Tile(strike.target)];
                if let Some(defender) = strike.defender {
                    keys.push(Key::Unit(defender.id));
                }
                keys
            }
            Cue::CityFounded { tile, .. }
            | Cue::CityGrew { tile }
            | Cue::CityCaptured { tile, .. }
            | Cue::BuildingCompleted { tile, .. } => vec![Key::Tile(*tile)],
            Cue::Announcement(_) => Vec::new(),
        }
    }

    /// Units the cue draws (posed while it waits and plays).
    pub fn figures(&self) -> Vec<Figure> {
        match self {
            Cue::Move { unit, .. }
            | Cue::Fortify { unit, .. }
            | Cue::Flight { unit, .. }
            | Cue::Nuke { unit, .. }
            | Cue::Paradrop { unit, .. }
            | Cue::Airlift { unit, .. }
            | Cue::Death { unit, .. }
            | Cue::Vanish { unit, .. } => vec![*unit],
            Cue::Attack(attack) => {
                let mut figures = Vec::new();
                if attack.attacker_seen {
                    figures.push(attack.attacker);
                }
                figures.extend(attack.defender);
                figures
            }
            Cue::CityStrike(strike) => strike.defender.into_iter().collect(),
            _ => Vec::new(),
        }
    }
}

impl Attack {
    /// When the attack's moments fall: aircraft fly out and home, the rest by weapon.
    pub fn timing(&self) -> Timing {
        match self.style {
            AttackStyle::Air => super::air::attack_timing(self),
            _ => Timing::of(self.weapon),
        }
    }

    /// An attack for tests: `reach` tiles from `from` to `target`.
    #[cfg(test)]
    pub fn test(from: TileId, target: TileId, reach: u8, weapon: Weapon) -> Self {
        let figure = |id| Figure { id: EntityId(id), kind: 0, owner: 1 };
        Self {
            attacker: figure(1),
            from,
            target,
            defender: Some(figure(2)),
            style: if matches!(weapon, Weapon::Bomb | Weapon::Missile | Weapon::Autogun) { AttackStyle::Air } else { AttackStyle::Melee },
            weapon,
            figures: 1,
            reach,
            defender_hp: None,
            attacker_hp: None,
            dealt: Some(20),
            taken: None,
            kill: false,
            attacker_seen: true,
            target_owner: 2,
        }
    }
}

/// Walking time along a path at normal speed.
pub fn walk_seconds(path: &[TileId]) -> f32 {
    path.len().saturating_sub(1) as f32 * STEP_SECONDS
}
