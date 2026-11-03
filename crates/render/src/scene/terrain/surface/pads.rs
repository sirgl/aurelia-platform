//! Pads: level ground under the objects that stand on a tile (platform/MESHING.md §5 step 6, "terraces
//! and plateau"; platform/docs/slopes.md). A stage-4 cutout like `sites.rs`: the ground inside a pad is
//! cut and filled to one level and blends back to the natural ground over an embankment, so a
//! building, a wall ring or a mine stands on the ground itself and needs no stone plinth.
//!
//! A pad is made for a known city tile (wide: the walls and the outer slots stand on it) and
//! for a known tile with an improvement or a resource, but only where the natural ground is
//! uneven enough to matter and not so steep that it is a cliff. Neighbouring pads blend by
//! sharpened weights (a pad owns its level disc, the city's first), so a slope of such tiles
//! becomes terraces. World wonders are not given pads: the terrain does not know which ring tile
//! the object layer picks (it picks by the relief the terrain has), they keep the stone course. Contributors read the
//! world observation only: the level is the mean of the natural field over the pad disc,
//! taken once when the landscape is built.
use super::{
    biomes::smooth,
    fingerprint::{Sink, Tag},
    hydrology::Point,
    index::SegmentIndex,
};

/// A city pad is level within `CITY_INNER` of the tile centre (the walls and the outer slots stand
/// on it) and blends out over `CITY_BLEND`, wider on steeper ground (see `blend`).
pub const CITY_INNER: f32 = 1.05;
pub const CITY_BLEND: f32 = 0.55;
/// A tile pad (improvement, resource): level over the whole hex (corners reach 1.0: a field is a flat
/// hexagon).
pub const TILE_INNER: f32 = 0.95;
pub const TILE_BLEND: f32 = 0.45;
/// The embankment grows with the slope: width x (1 + STEEP_GAIN x (tan slope - STEEP_FROM)), capped.
const STEEP_GAIN: f32 = 1.5;
const STEEP_FROM: f32 = 0.35;
const BLEND_CAP: f32 = 2.2;
/// Exponent that sharpens pad weights where pads overlap.
const SHARPEN: i32 = 4;
/// Requested edge length inside a pad's support, so the embankment keeps its shape.
pub const CITY_EDGE: f32 = 0.2;
pub const TILE_EDGE: f32 = 0.22;
/// A pad is made where the natural ground varies by more than this across its level disc...
pub const CITY_RELIEF_MIN: f32 = 0.05;
pub const TILE_RELIEF_MIN: f32 = 0.10;
/// ...and its mean slope (height range over the disc width) stays below this (52 degrees).
pub const SLOPE_MAX: f32 = 1.3;
/// Share of the disc that must be dry land with a known level for a pad to be made.
pub const DRY_SHARE: f32 = 0.4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadKind {
    City,
    Tile,
}

impl PadKind {
    pub fn inner(self) -> f32 {
        match self {
            PadKind::City => CITY_INNER,
            PadKind::Tile => TILE_INNER,
        }
    }
    /// Blend width on level ground.
    pub fn blend(self) -> f32 {
        match self {
            PadKind::City => CITY_BLEND,
            PadKind::Tile => TILE_BLEND,
        }
    }
    /// Outer radius of a pad on ground of slope `tan`.
    pub fn outer_for(self, tan: f32) -> f32 {
        let widen = 1. + STEEP_GAIN * (tan - STEEP_FROM).max(0.);
        self.inner() + (self.blend() * widen).min(BLEND_CAP - self.inner())
    }
    pub fn edge(self) -> f32 {
        match self {
            PadKind::City => CITY_EDGE,
            PadKind::Tile => TILE_EDGE,
        }
    }
    pub fn relief_min(self) -> f32 {
        match self {
            PadKind::City => CITY_RELIEF_MIN,
            PadKind::Tile => TILE_RELIEF_MIN,
        }
    }
    /// Points of the disc the level is taken from: the centre and two rings.
    pub fn disc(self, centre: Point) -> Vec<Point> {
        let r = self.inner();
        let mut out = vec![centre];
        for (ring, count) in [(0.5, 6usize), (1.0, 12)] {
            for k in 0..count {
                let a = std::f32::consts::TAU * (k as f32 + 0.5 * (ring < 0.9) as u8 as f32)
                    / count as f32;
                out.push([
                    centre[0] + r * ring * a.cos(),
                    centre[1] + r * ring * a.sin(),
                ]);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pad {
    pub centre: Point,
    pub level: f32,
    pub outer: f32,
    pub kind: PadKind,
    /// The tile, for fingerprints.
    pub tile: u32,
}

#[derive(Default)]
pub struct Pads {
    list: Vec<Pad>,
    index: SegmentIndex,
}

/// What a disc of natural ground samples allows: the level and the slope (tan), or None for no
/// pad. `samples` are (natural height, is dry land with a known level) per disc point.
pub fn level_of(kind: PadKind, samples: &[(f32, bool)]) -> Option<(f32, f32)> {
    let dry: Vec<f32> = samples.iter().filter(|s| s.1).map(|s| s.0).collect();
    if dry.is_empty() || (dry.len() as f32) < DRY_SHARE * samples.len() as f32 {
        return None;
    }
    let (lo, hi) = dry
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), &y| (l.min(y), h.max(y)));
    let relief = hi - lo;
    let tan = relief / (2. * kind.inner());
    if relief < kind.relief_min() || tan > SLOPE_MAX {
        return None;
    }
    Some((dry.iter().sum::<f32>() / dry.len() as f32, tan))
}

impl Pads {
    pub fn new(list: Vec<Pad>) -> Self {
        let index = SegmentIndex::new(
            list.iter().map(|p| {
                let r = p.outer;
                [
                    [p.centre[0] - r, p.centre[1] - r],
                    [p.centre[0] + r, p.centre[1] + r],
                ]
            }),
            2.0,
        );
        Self { list, index }
    }

    /// Weight of one pad at a point: one inside, zero at the outer radius.
    fn weight(pad: &Pad, p: Point) -> f32 {
        let d = (p[0] - pad.centre[0]).hypot(p[1] - pad.centre[1]);
        let (inner, outer) = (pad.kind.inner(), pad.outer);
        1. - smooth((d - inner) / (outer - inner))
    }

    /// Whether any pad reaches `p` (within `slack` more).
    pub fn reaches(&self, p: Point, slack: f32) -> bool {
        self.index.candidates(p).iter().any(|&i| {
            let pad = &self.list[i as usize];
            (p[0] - pad.centre[0]).hypot(p[1] - pad.centre[1]) < pad.outer + slack
        })
    }

    /// Smallest requested edge length within `slack` of `p`, if a pad asks.
    pub fn edge_length(&self, p: Point, slack: f32) -> Option<f32> {
        self.index
            .candidates(p)
            .iter()
            .filter_map(|&i| {
                let pad = &self.list[i as usize];
                ((p[0] - pad.centre[0]).hypot(p[1] - pad.centre[1]) < pad.outer + slack)
                    .then_some(pad.kind.edge())
            })
            .reduce(f32::min)
    }

    /// Stage 4 at a point: the total natural height (ground and massif lift) reshaped by the
    /// pads, scaled by `allowed` (0..1, the exclusions: water, river corridor, cliff), and
    /// the share of the point the pads own (massif relief gives way by as much).
    pub fn cut(&self, p: Point, natural: f32, allowed: f32) -> (f32, f32) {
        if allowed <= 0. {
            return (natural, 0.);
        }
        // A pad owns its level disc: where one pad has full weight another may not pull on it
        // (neighbouring pads of an uneven slope sit at different levels). Weights are sharpened
        // before they average the levels, so the level is continuous and the pad with the most
        // weight dominates; the share of the natural ground that survives is one minus the
        // largest weight.
        let weights: Vec<(&Pad, f32)> = self
            .index
            .candidates(p)
            .iter()
            .map(|&i| &self.list[i as usize])
            .map(|pad| (pad, Self::weight(pad, p) * allowed))
            .filter(|(_, w)| *w > 0.)
            .collect();
        // A city pad owns the ground under its walls: the pads of the tiles around it give way
        // as it takes weight.
        let city = weights
            .iter()
            .filter(|(pad, _)| pad.kind == PadKind::City)
            .fold(0.0f32, |t, (_, w)| t.max(*w));
        let (mut sum, mut mean, mut top) = (0.0f32, 0.0f32, 0.0f32);
        for (pad, w) in weights {
            let w = if pad.kind == PadKind::City {
                w
            } else {
                w * (1. - city)
            };
            if w > 0. {
                let sharp = w.powi(SHARPEN);
                sum += sharp;
                mean += sharp * pad.level;
                top = top.max(w);
            }
        }
        if top <= 0. {
            return (natural, 0.);
        }
        (natural * (1. - top) + mean / sum * top, top)
    }

    /// Pads for region fingerprints: a pad shapes ground up to its outer radius.
    pub fn fingerprint(&self, sink: &mut Sink) {
        for pad in &self.list {
            let r = pad.outer;
            let words = [
                pad.tile,
                pad.kind as u32,
                pad.level.to_bits(),
                pad.outer.to_bits(),
            ];
            sink.item(
                Tag::Pad,
                [pad.centre[0] - r, pad.centre[1] - r],
                [pad.centre[0] + r, pad.centre[1] + r],
                &words,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(y: f32, n: usize) -> Vec<(f32, bool)> {
        vec![(y, true); n]
    }

    #[test]
    fn flat_and_cliff_ground_get_no_pad_and_a_slope_gets_the_mean() {
        assert_eq!(
            level_of(PadKind::City, &flat(0.4, 19)),
            None,
            "flat ground is left alone"
        );
        let ramp: Vec<(f32, bool)> = (0..19).map(|i| (0.3 + 0.01 * i as f32, true)).collect();
        let (level, _) = level_of(PadKind::City, &ramp).unwrap();
        assert!((level - 0.39).abs() < 1e-5);
        let cliff: Vec<(f32, bool)> = (0..19).map(|i| (0.3 + 0.2 * i as f32, true)).collect();
        assert_eq!(
            level_of(PadKind::City, &cliff),
            None,
            "a cliff stays a cliff"
        );
        let wet: Vec<(f32, bool)> = (0..19).map(|i| (0.3 + 0.01 * i as f32, i < 5)).collect();
        assert_eq!(level_of(PadKind::City, &wet), None, "mostly water: no pad");
    }

    #[test]
    fn the_pad_is_level_inside_blends_out_and_is_continuous() {
        let pads = Pads::new(vec![Pad {
            centre: [5., 5.],
            level: 0.5,
            outer: PadKind::City.outer_for(0.),
            kind: PadKind::City,
            tile: 1,
        }]);
        for k in 0..12 {
            let a = k as f32 * 0.5;
            let (y, owned) = pads.cut(
                [5. + a.cos() * 0.85, 5. + a.sin() * 0.85],
                0.9 + 0.01 * k as f32,
                1.,
            );
            assert!((y - 0.5).abs() < 1e-5 && owned == 1.);
            let (far, none) = pads.cut([5. + a.cos() * 1.6, 5. + a.sin() * 1.6], 0.9, 1.);
            assert!(far == 0.9 && none == 0.);
        }
        let mut previous = pads.cut([5. + CITY_INNER, 5.], 0.9, 1.).0;
        for i in 1..200 {
            let y = pads
                .cut(
                    [5. + CITY_INNER + i as f32 * CITY_BLEND / 200., 5.],
                    0.9,
                    1.,
                )
                .0;
            assert!((y - previous).abs() < 0.02);
            previous = y;
        }
        assert_eq!(
            pads.cut([5., 5.], 0.9, 0.).0,
            0.9,
            "an excluded point is left alone"
        );
    }

    #[test]
    fn overlapping_pads_do_not_depend_on_the_order() {
        let a = Pad {
            centre: [5., 5.],
            level: 0.5,
            outer: PadKind::City.outer_for(0.),
            kind: PadKind::City,
            tile: 1,
        };
        let b = Pad {
            centre: [6.7, 5.],
            level: 0.8,
            outer: PadKind::Tile.outer_for(0.),
            kind: PadKind::Tile,
            tile: 2,
        };
        let ab = Pads::new(vec![a, b]);
        let ba = Pads::new(vec![b, a]);
        for i in 0..40 {
            let p = [5. + i as f32 * 0.04, 5.2];
            let (y1, w1) = ab.cut(p, 0.6, 1.);
            let (y2, w2) = ba.cut(p, 0.6, 1.);
            assert!((y1 - y2).abs() < 1e-5 && (w1 - w2).abs() < 1e-5);
        }
    }
}
