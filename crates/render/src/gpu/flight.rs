//! Aircraft in flight: the per-class parameters (a table, no model names), the shear that
//! banks and pitches a model, and the flat shadow it throws on the ground below.
//!
//! A flying unit is posed (`unit_poses::Gait::Flight`): lifted by its altitude, yawed along
//! its heading, tilted by a ground shear (`HexInstance::slope`, the same attribute that
//! lays plane models onto slopes: no new shader). The presentation (client) computes the
//! path and the angles from these parameters; the renderer only draws.

use crate::gpu::buffers::HexInstance;
use crate::unit_models::{Rig, UnitModel};

/// What kind of flier a unit is: picks its row of `PARAMS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlightClass {
    /// Fighters and bombers: take off, cruise, land.
    Plane,
    /// A helicopter: low, slow, nose down.
    Helicopter,
    /// A cruise missile: leaves its base at once, low and fast, a fiery trail.
    Cruise,
    /// A nuclear weapon: climbs steeply, arcs high, falls steeply.
    Ballistic,
    /// The transport plane of an airlift or a paradrop (drawn with a bomber's model).
    Transport,
}

/// How the exhaust of a flier is drawn (the presentation spawns it with the effects pool).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trail {
    None,
    /// A thin white contrail behind a jet at altitude.
    Contrail,
    /// Dark exhaust smoke (propeller engines, helicopters).
    Exhaust,
    /// Flame and smoke of a rocket.
    Rocket,
    /// A hit aircraft: black smoke and flames.
    Damage,
}

/// The numbers of one class.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlightParams {
    /// Cruise altitude above the ground, world units (a tile is 1.73 wide).
    pub altitude: f32,
    /// Seconds per tile at cruise (normal speed).
    pub tile_seconds: f32,
    /// Seconds of the take-off climb and of the landing descent.
    pub takeoff: f32,
    pub landing: f32,
    /// Sideways bulge of the path, as a fraction of its length (rebase flights are arcs).
    pub arc: f32,
    /// How fast the aircraft banks into a turn (radians per radian/second of turn rate).
    pub bank: f32,
    /// Nose-up angle at the steepest climb (radians) per unit of climb slope.
    pub pitch: f32,
    pub trail: Trail,
}

pub const PARAMS: [(FlightClass, FlightParams); 5] = [
    (
        FlightClass::Plane,
        FlightParams { altitude: 0.62, tile_seconds: 0.19, takeoff: 0.55, landing: 0.60, arc: 0.10, bank: 0.55, pitch: 0.9, trail: Trail::Contrail },
    ),
    (
        FlightClass::Helicopter,
        FlightParams { altitude: 0.34, tile_seconds: 0.30, takeoff: 0.40, landing: 0.40, arc: 0.04, bank: 0.35, pitch: 0.5, trail: Trail::Exhaust },
    ),
    (
        FlightClass::Cruise,
        FlightParams { altitude: 0.30, tile_seconds: 0.12, takeoff: 0.22, landing: 0.10, arc: 0.03, bank: 0.30, pitch: 0.8, trail: Trail::Rocket },
    ),
    (
        FlightClass::Ballistic,
        FlightParams { altitude: 1.90, tile_seconds: 0.12, takeoff: 0.55, landing: 0.30, arc: 0.0, bank: 0.0, pitch: 1.2, trail: Trail::Rocket },
    ),
    (
        FlightClass::Transport,
        FlightParams { altitude: 0.78, tile_seconds: 0.20, takeoff: 0.50, landing: 0.50, arc: 0.0, bank: 0.35, pitch: 0.7, trail: Trail::Exhaust },
    ),
];

pub fn params(class: FlightClass) -> FlightParams {
    PARAMS.iter().find(|(c, _)| *c == class).map(|(_, p)| *p).unwrap_or(PARAMS[0].1)
}

/// How much of the presentation's pitch and bank a model takes (by its rig).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile {
    pub pitch_gain: f32,
    pub bank_gain: f32,
}

pub fn profile(model: &UnitModel) -> Profile {
    match model.rig {
        Rig::Plane => Profile { pitch_gain: 1.0, bank_gain: 1.0 },
        // A missile points along its path but does not roll into turns.
        Rig::Missile => Profile { pitch_gain: 1.0, bank_gain: 0.2 },
        _ => Profile { pitch_gain: 0.6, bank_gain: 0.6 },
    }
}

/// The ground shear `(dy/dx, dy/dz)` that pitches (`pitch` > 0: nose up) and banks
/// (`bank` > 0: the right wing down) a model turned by `yaw`.
pub fn shear(yaw: f32, pitch: f32, bank: f32) -> [f32; 2] {
    let (s, c) = yaw.sin_cos();
    let forward = [-s, c];
    let right = [c, s];
    // The shear is not a rotation: it is exact for small angles, which flight keeps to.
    let (p, b) = (pitch.clamp(-0.9, 0.9), bank.clamp(-0.7, 0.7));
    [p * forward[0] - b * right[0], p * forward[1] - b * right[1]]
}

/// A flat dark copy of a flying figure on the ground below it: its shadow. Smaller and
/// fainter the higher it flies.
pub fn ground_shadow(figure: &HexInstance, ground: f32, altitude: f32) -> HexInstance {
    let mut shadow = *figure;
    shadow.offset[1] = ground + 0.008;
    shadow.color = [0.02, 0.03, 0.03];
    shadow.scale_height = [figure.scale_height[0] * (1.0 - 0.06 * altitude.min(2.0)), 0.012];
    shadow.fog = figure.fog * (0.9 / (1.0 + 0.35 * altitude));
    shadow.slope = [0.0, 0.0];
    shadow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_class_has_a_row_and_flies_above_the_ground() {
        for class in [FlightClass::Plane, FlightClass::Helicopter, FlightClass::Cruise, FlightClass::Ballistic, FlightClass::Transport] {
            let p = params(class);
            assert!(p.altitude > 0.2 && p.tile_seconds > 0.0 && p.takeoff > 0.0, "{class:?}");
        }
        assert!(params(FlightClass::Ballistic).altitude > params(FlightClass::Plane).altitude);
        assert!(params(FlightClass::Cruise).altitude < params(FlightClass::Plane).altitude);
    }

    #[test]
    fn pitch_lifts_the_nose_and_bank_drops_one_wing() {
        // Facing +z (yaw 0): the nose is at +z, the wings at +-x.
        let up = shear(0.0, 0.3, 0.0);
        assert!(up[1] > 0.0 && up[0].abs() < 1e-6, "nose up lifts +z");
        let banked = shear(0.0, 0.0, 0.3);
        assert!(banked[0] < 0.0 && banked[1].abs() < 1e-6, "right wing (+x) down");
        // Turned a quarter: the nose points along -x.
        let turned = shear(std::f32::consts::FRAC_PI_2, 0.3, 0.0);
        assert!(turned[0] < -0.2 && turned[1].abs() < 1e-5);
    }

    #[test]
    fn the_shadow_is_flat_dark_and_on_the_ground() {
        let figure = HexInstance::upright([3.0, 1.2, 4.0], [0.9, 0.2, 0.2], [1.0, 1.0], 1.0);
        let shadow = ground_shadow(&figure, 0.4, 0.8);
        assert!((shadow.offset[1] - 0.408).abs() < 1e-6);
        assert!(shadow.scale_height[1] < 0.05 && shadow.color[0] < 0.1);
        assert!(shadow.fog < figure.fog);
    }
}
