//! Bounded transition profiles shared by shaping and constrained triangulation.
//! Known coastline and river curves supply contours; profiles control their sections.
//! The surface uses these ordered contours without fixing interior triangle count.

#[derive(Debug, Clone, Copy)]
pub enum CoastProfile {
    Beach,
    RockLedge,
}

#[derive(Debug, Clone, Copy)]
pub struct CoastSection {
    pub height: f32,
    pub sand: f32,
    pub stone: f32,
}

/// Shared world-space geology for height sampling and matching CDT contours.
#[derive(Debug, Clone, Copy, Default)]
pub struct CoastShape {
    pub width: f32,
    pub crest: f32,
    pub bench: f32,
    /// Zero: the wall rises straight from the water. One: a low rock platform
    /// sits in front of a wall standing further inland. Changes along the coast
    /// break the brow into offset panels.
    pub setback: f32,
}
/// Piecewise planar geological panels, independent of the render triangulation.
/// Their fixed world-space creases create broad rock faces rather than vertex noise.
fn rock_panel(p: [f32; 2], scale: f32, seed: u32) -> f32 {
    let x = (p[0] * 0.86 + p[1] * 0.37) * scale;
    let z = (p[1] * 0.86 - p[0] * 0.37) * scale;
    let (ix, iz) = (x.floor(), z.floor());
    let (u, v) = (x - ix, z - iz);
    let n = super::hydrology::noise;
    let a = n(ix, iz, seed);
    let b = n(ix + 1., iz, seed);
    let c = n(ix, iz + 1., seed);
    let d = n(ix + 1., iz + 1., seed);
    if u + v <= 1. {
        a + (b - a) * u + (c - a) * v
    } else {
        d + (c - d) * (1. - u) + (b - d) * (1. - v)
    }
}
impl CoastShape {
    /// Inland offset of a contour from a coast point such that the profile
    /// sampled at the offset point lands exactly on `fraction` of its width.
    pub fn contour_offset(coast: [f32; 2], sea: [f32; 2], fraction: f32) -> f32 {
        let mut offset = fraction * Self::at(coast).width;
        for _ in 0..4 {
            let p = [coast[0] - sea[0] * offset, coast[1] - sea[1] * offset];
            offset = fraction * Self::at(p).width;
        }
        offset
    }
    pub fn at(p: [f32; 2]) -> Self {
        Self {
            width: 0.40 + 0.40 * rock_panel(p, 1.9, 2161),
            crest: 0.025 + 0.25 * rock_panel(p, 0.84, 2309),
            bench: ((rock_panel(p, 0.78, 2539) - 0.32) / 0.34).clamp(0., 1.),
            setback: ((rock_panel(p, 1.35, 2683) - 0.58) / 0.14).clamp(0., 1.),
        }
    }
}

impl CoastProfile {
    // Common ordered contour sequence makes profile blending share topology.
    pub const CONTOURS: [f32; 4] = [0.08, 0.24, 0.42, 0.66];

    #[cfg(test)]
    pub fn sample(self, distance: f32, water: f32, inland: f32) -> CoastSection {
        self.sample_shaped(
            distance,
            water,
            inland,
            CoastShape {
                width: 1.,
                crest: 0.16,
                bench: 0.48,
                setback: 0.,
            },
        )
    }

    /// Cliff height at the three inner contours as a fraction of the brow rise.
    /// A tall wall from the water, a lower wall to a narrow bench with a second
    /// wall above it, or a wide low platform before a wall set further inland.
    /// Behind each wall the top is level, so the brow is a sharp edge.
    fn wall_fractions(bench: f32, setback: f32) -> [f32; 3] {
        let from_water = [0.86, 1., 1.];
        let benched = [0.40, 0.44, 1.];
        let platform = [0.06, 0.09, 0.14];
        let mut fractions = [0.; 3];
        for i in 0..3 {
            let wall = from_water[i] * (1. - bench) + benched[i] * bench;
            fractions[i] = wall * (1. - setback) + platform[i] * setback;
        }
        fractions
    }

    /// Shelves and platforms below the brow are bare stone; the level top
    /// keeps only a thin stony rim before the grass.
    fn exposed_stone(fraction: f32) -> f32 {
        let top = ((fraction - 0.90) / 0.10).clamp(0., 1.);
        1. - 0.4 * top
    }

    pub fn sample_shaped(
        self,
        distance: f32,
        water: f32,
        inland: f32,
        shape: CoastShape,
    ) -> CoastSection {
        let distance = distance / shape.width;
        // The brow is the edge of the inland plateau, not a ridge above it: only
        // a low lip may rise over the grass, so rocks never read as mounds.
        let crown = inland.max(water + 0.45) + shape.crest * 0.25;
        let rise = crown - water;
        let wall = Self::wall_fractions(shape.bench, shape.setback);
        let xs = [0., 0.08, 0.24, 0.42, 0.66, 1.];
        let (heights, sand, stone) = match self {
            Self::Beach => (
                [
                    water,
                    water + 0.014,
                    water + (inland - water) * 0.16,
                    water + (inland - water) * 0.48,
                    water + (inland - water) * 0.86,
                    inland,
                ],
                [0.76, 1., 0.92, 0.16, 0., 0.],
                [0.; 6],
            ),
            Self::RockLedge => (
                [
                    water,
                    water + rise * wall[0],
                    water + rise * wall[1],
                    water + rise * wall[2],
                    crown,
                    inland,
                ],
                [0.08, 0., 0., 0., 0., 0.],
                [
                    0.65,
                    Self::exposed_stone(wall[0]),
                    Self::exposed_stone(wall[1]),
                    Self::exposed_stone(wall[2]),
                    0.6,
                    0.,
                ],
            ),
        };
        CoastSection {
            height: interpolate(&xs, &heights, distance),
            sand: interpolate(&xs, &sand, distance),
            stone: interpolate(&xs, &stone, distance),
        }
    }
}

pub struct RiverBankProfile;

impl RiverBankProfile {
    pub const CONTOURS: [f32; 3] = [0., 0.10, 0.36];
    pub const EXTENT: f32 = 0.46;

    pub fn width_scale(p: [f32; 2]) -> f32 {
        0.72 + 0.40 * super::hydrology::noise(p[0] * 1.13, p[1] * 1.13, 2683)
    }

    pub fn height(distance: f32, water: f32, inland: f32) -> f32 {
        let shelf = water + (inland - water).max(0.).min(0.075);
        interpolate(
            &[0., 0.10, 0.36, Self::EXTENT],
            &[water, shelf, shelf * 0.22 + inland * 0.78, inland],
            distance,
        )
    }
}

fn interpolate<const N: usize>(xs: &[f32; N], ys: &[f32; N], x: f32) -> f32 {
    if x <= xs[0] {
        return ys[0];
    }
    for i in 1..N {
        if x <= xs[i] {
            let t = (x - xs[i - 1]) / (xs[i] - xs[i - 1]);
            return ys[i - 1] * (1. - t) + ys[i] * t;
        }
    }
    ys[N - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rock_panels_are_continuous_across_their_world_space_creases() {
        for i in 0..20 {
            let p = [i as f32 * 0.43, 6.7];
            let a = CoastShape::at(p);
            let b = CoastShape::at([p[0] + 0.0001, p[1] - 0.0001]);
            assert!((a.width - b.width).abs() < 0.001);
            assert!((a.crest - b.crest).abs() < 0.001);
            assert!((0.40..=0.80).contains(&a.width));
        }
    }
    #[test]
    fn variable_geology_breaks_shelves_while_preserving_shared_contacts() {
        let mut shelves = 0;
        let mut uninterrupted_faces = 0;
        for i in 0..80 {
            let shape = CoastShape::at([i as f32 * 0.7, 4.]);
            let sample = |d| CoastProfile::RockLedge.sample_shaped(d * shape.width, 0.08, 0.4, shape);
            assert_eq!(sample(0.).height, 0.08);
            assert_eq!(sample(1.).height, 0.4);
            let lower = (sample(0.24).height - sample(0.08).height) / 0.16;
            let middle = (sample(0.42).height - sample(0.24).height) / 0.18;
            shelves += usize::from(middle < lower * 0.25);
            uninterrupted_faces += usize::from(middle > lower * 0.85);
            assert!(middle >= -1e-5, "no inverted ledge creates a surface overhang");
        }
        assert!(shelves > 5, "some exposed shelves remain");
        assert!(uninterrupted_faces > 5, "shelves must terminate instead of encircling every headland");
    }
    #[test]
    fn transitions_keep_shared_water_and_inland_contacts_across_height_range() {
        for inland in [0.10, 0.30, 0.8, 1.5, 2.5] {
            for profile in [CoastProfile::Beach, CoastProfile::RockLedge] {
                assert_eq!(profile.sample(0., 0.08, inland).height, 0.08);
                assert_eq!(profile.sample(1., 0.08, inland).height, inland);
                for x in 0..=100 {
                    let section = profile.sample(x as f32 / 100., 0.08, inland);
                    assert!(section.height.is_finite() && section.height >= 0.08);
                    assert!((0. ..=1.).contains(&section.sand));
                    assert!((0. ..=1.).contains(&section.stone));
                }
            }
            assert_eq!(RiverBankProfile::height(0., 0.08, inland), 0.08);
            assert_eq!(RiverBankProfile::height(0.46, 0.08, inland), inland);
        }
    }
}
