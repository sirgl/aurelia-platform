//! Stage 3 of the field (platform/MESHING.md §2.2): hydrology transforms.
//!
//! The sea floor, the beach profile of the coast and the river bed with its
//! banks reshape the ground of stages 1–2. Profiles come from
//! `transitions.rs`; the coast and river geometry from `hydrology.rs`.
use super::{
    biomes::{smooth, Ground},
    hydrology::{noise, Point, River},
    transitions::{CoastProfile, CoastSection, CoastShape, RiverBankProfile},
};

/// Sea level; every water contact sits exactly here or on a river level.
pub const SEA_LEVEL: f32 = 0.08;
/// Coast profiles end within one width (at most 0.80). A raised cliff plateau
/// then slopes gently back to the sea-weighted base height by this distance.
pub const COAST_EXTENT: f32 = 3.0;

/// Height and materials of a point while the stages are applied.
pub struct Shape {
    pub y: f32,
    pub albedo: [f32; 3],
    pub stone: f32,
    /// Distance to the nearest water, sea or river.
    pub shore: f32,
}

/// The sea floor at a signed coast distance (negative at sea).
pub fn sea_floor(shore: f32) -> f32 {
    SEA_LEVEL - 0.02 - (-shore * 0.32).min(0.85)
}

/// The beach profile of the coast at `shore` inland of the waterline.
pub fn beach(shore: f32, ground: &Ground, shape: CoastShape) -> CoastSection {
    CoastProfile::Beach.sample_shaped(shore, SEA_LEVEL, ground.height.max(0.10), shape)
}

/// The vertex color under the coast: sand on beaches, and grass that dries
/// toward the stones of a cliff rim. Stone itself is not a vertex tint: the
/// walls are colored per face and flagstones are drawn by the terrain shader
/// from the stone weight (`SurfaceVertex::coastal_rock`).
pub fn coast_albedo(grass: [f32; 3], section: CoastSection) -> [f32; 3] {
    // Fresh moss and short turf around the slabs of a rocky rim.
    let dry_grass = [0.50, 0.68, 0.28];
    std::array::from_fn(|k| {
        let turf = grass[k] * (1. - 0.45 * section.stone) + dry_grass[k] * 0.45 * section.stone;
        turf * (1. - section.sand) + [0.89, 0.82, 0.65][k] * section.sand
    })
}

/// Beyond the river bank profile a cutout comes back over this distance.
const CORRIDOR_FADE: f32 = 0.35;

/// The farthest a river's bank profile reaches from its axis: the widest channel and bank.
pub const MAX_REACH: f32 = 1.0;

/// One outside a river's corridor (bed and bank profile), zero inside it.
/// Stage 3 owns the channel: a cutout weighted by this never dams a river.
pub fn outside_corridor((r, d, t): (&River, f32, f32)) -> f32 {
    let reach = r.width_at(t) + RiverBankProfile::EXTENT * r.bank_scale(t);
    smooth((d - reach) / CORRIDOR_FADE)
}

/// The water level of a river at its nearest point.
pub fn river_level((r, _, t): (&River, f32, f32)) -> f32 {
    r.ya * (1. - t) + r.yb * t
}

impl Shape {
    /// A river bed and its banks reshape the ground near the river.
    pub fn carve_river(&mut self, (r, d, t): (&River, f32, f32)) {
        let level = r.ya * (1. - t) + r.yb * t;
        let width = r.width_at(t);
        if d < width {
            self.stone = 0.;
            self.y = self.y.min(level - r.depth_at(t) * (1. - (d / width).powi(2)));
            self.shore = self.shore.min(d - width);
            return;
        }
        let scale = r.bank_scale(t);
        if d < width + RiverBankProfile::EXTENT * scale {
            self.y = RiverBankProfile::height((d - width) / scale, level, self.y);
        }
    }
}

/// What the water decides about its banks: the palette position of the ground it crosses,
/// how big the river is (0 a brook, 1 the widest river) and how much the point is a spring.
#[derive(Debug, Clone, Copy)]
pub struct BankLook {
    pub tint: f32,
    pub size: f32,
    pub spring: f32,
}

impl Default for BankLook {
    fn default() -> Self {
        Self { tint: 0.3, size: 1., spring: 0. }
    }
}

/// Places on the palettes of the water and its banks (`Ground::tint`).
const TINTS: [f32; 4] = [0., 0.3, 0.62, 1.];

/// A palette value at `tint`: the stops are bare stone, silt and soil of a lowland, mud of a
/// marsh or floodplain, and the frost of a cold country.
fn palette(stops: &[[f32; 3]; 4], tint: f32) -> [f32; 3] {
    let tint = tint.clamp(0., 1.);
    let i = (1..4).find(|&i| tint <= TINTS[i]).unwrap_or(3);
    let t = (tint - TINTS[i - 1]) / (TINTS[i] - TINTS[i - 1]);
    std::array::from_fn(|k| stops[i - 1][k] * (1. - t) + stops[i][k] * t)
}

/// The tint of a river's water at a point: the ground's, clearer where the river falls
/// fast, slightly milkier where a big river is slow.
pub fn water_tint(ground: f32, river: &River, t: f32) -> f32 {
    let rapids = rapids_of(river);
    let tint = ground + 0.10 * river.size_at(t) * (1. - rapids);
    tint * (1. - 0.8 * rapids)
}

/// How much of a fall is a rapid (white water, clear): 0 on level ground, 1 on a steep reach.
pub fn rapids_of(river: &River) -> f32 {
    smooth((river.fall() - 0.03) / 0.07)
}

/// Existing surface vertices carry the bank material; no extra ribbon or pass.
pub fn river_bank_color(grass: [f32; 3], p: Point, bank_distance: f32, rise: f32, sea_distance: f32, look: BankLook) -> [f32; 3] {
    let marsh = look.spring * (1. - smooth(bank_distance.max(0.) / 0.55));
    if (bank_distance > 0.29 || sea_distance <= 0.35) && marsh <= 0. {
        return grass;
    }
    if sea_distance <= 0.35 {
        return grass;
    }
    let sediment = noise(p[0] * 0.94, p[1] * 0.94, 1201);
    let grain = noise(p[0] * 2.6, p[1] * 2.6, 1493);
    let steep = smooth((rise - 0.14) / 0.48);
    let deposit = smooth((sediment - 0.24) / 0.43) * (1. - steep * 0.58);
    // A brook keeps a narrower band of its own.
    let scale = 0.6 + 0.4 * look.size;
    let width = (0.065 + deposit * 0.19) * scale;
    let wet = 1. - smooth((bank_distance + 0.002) / 0.027);
    let dry = (1. - smooth((bank_distance - width * 0.36) / (width * 0.64))) * (0.16 + 0.84 * deposit);
    let coverage = (wet.max(dry) * smooth((sea_distance - 0.35) / 0.65)).max(marsh * 0.8);
    if coverage <= 0. {
        return grass;
    }
    let silt = palette(&[[0.66, 0.65, 0.60], [0.80, 0.72, 0.53], [0.46, 0.42, 0.28], [0.88, 0.93, 0.96]], look.tint);
    let earth = palette(&[[0.50, 0.50, 0.46], [0.59, 0.51, 0.37], [0.36, 0.32, 0.20], [0.70, 0.80, 0.86]], look.tint);
    let wet_soil = palette(&[[0.36, 0.37, 0.35], [0.52, 0.44, 0.31], [0.24, 0.25, 0.15], [0.52, 0.64, 0.74]], look.tint);
    let mineral = steep * 0.75 + (1. - deposit) * 0.20;
    let tint = 0.96 + 0.08 * grain;
    let bank: [f32; 3] = std::array::from_fn(|k| {
        let dry = silt[k] * (1. - mineral) + earth[k] * mineral;
        (dry * (1. - wet) + wet_soil[k] * wet) * tint
    });
    // A spring is a wet green patch of reeds and mud around the first water.
    let reeds = [0.30, 0.47, 0.20];
    let patch = marsh * (0.55 + 0.45 * smooth((noise(p[0] * 3.4, p[1] * 3.4, 1699) - 0.3) / 0.4));
    std::array::from_fn(|k| {
        let band = grass[k] * (1. - coverage) + bank[k] * coverage;
        band * (1. - patch * 0.75) + (reeds[k] * (1. - 0.25 * wet) + wet_soil[k] * 0.25 * wet) * patch * 0.75
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dry_river_deposits_are_broken_patches_not_a_continuous_sand_ribbon() {
        let grass = [0.34, 0.66, 0.26];
        let mut exposed = 0;
        let mut grassy = 0;
        for x in 0..80 {
            let c = river_bank_color(grass, [x as f32 * 0.25, 4.], 0.12, 0.05, 3., BankLook::default());
            if c[0] > grass[0] + 0.10 {
                exposed += 1;
            }
            if c[0] < grass[0] + 0.015 {
                grassy += 1;
            }
        }
        assert!(exposed > 8 && grassy > 8, "both sediment bars and turf interruptions must remain");
    }
    #[test]
    fn river_material_preserves_sea_and_distant_grass() {
        let grass = [0.34, 0.66, 0.26];
        for sea in [-2., 0., 0.35] {
            assert_eq!(river_bank_color(grass, [4., 7.], 0.01, 0.1, sea, BankLook::default()), grass);
        }
        assert_eq!(river_bank_color(grass, [4., 7.], 0.41, 0.1, 3., BankLook::default()), grass);
    }
    #[test]
    fn river_contact_is_wet_earth_and_shoulder_is_lighter_silt() {
        let grass = [0.34, 0.66, 0.26];
        let wet = river_bank_color(grass, [4., 7.], 0., 0.05, 3., BankLook::default());
        let dry = river_bank_color(grass, [4., 7.], 0.055, 0.05, 3., BankLook::default());
        let steep = river_bank_color(grass, [4., 7.], 0.055, 0.4, 3., BankLook::default());
        assert!(wet[0] > wet[1] && wet[1] > wet[2], "wet soil must read as earth");
        assert!(dry[0] > wet[0] && dry[1] > wet[1], "dry silt is lighter than contact");
        assert!(steep[0] < dry[0], "steep banks expose earth instead of bright sand");
    }
}
