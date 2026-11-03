//! Sea stacks and boulders at the foot of cliffs (platform/MESHING.md §2.1, objects).
//!
//! The height field cannot express a rock standing free in the water, so the
//! cliff contributor hands these out as objects. They are placed from the wall
//! stations alone, deterministically: a hash of the station position decides,
//! never the order of the coast or the rest of the map. The GPU adapter
//! grounds them later with the final mesh query (`gpu/coast_props.rs`).
use super::hydrology::Point;
use crate::scene::terrain::fields::hash_u32;

/// One wall station at the waterline: where the foot is and which way the sea lies.
#[derive(Debug, Clone, Copy)]
pub struct Foot {
    pub position: Point,
    pub seaward: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoastPropKind {
    /// A free-standing pillar off a headland, about as tall as the cliff.
    Stack,
    /// A fallen block at the foot, mostly under water.
    Boulder,
}

/// A rock object; `radius` and `height` are world units, `height` above the sea.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoastProp {
    pub kind: CoastPropKind,
    pub position: Point,
    pub radius: f32,
    pub height: f32,
    pub angle: f32,
    pub variant: u32,
}

/// Arc length between boulders along a wall, at least.
const BOULDER_SPACING: f32 = 0.45;
/// One station in this many may hold a boulder.
const BOULDER_SHARE: u32 = 3;
/// Stations on each side whose inland rays measure how sharp a headland is.
const HEADLAND_SPAN: usize = 4;
/// A headland whose rays meet closer than this inland is sharp enough for a stack.
const HEADLAND_DEPTH: f32 = 1.6;
/// Distance between stacks of one run, at least.
const STACK_SPACING: f32 = 2.4;

pub fn place(feet: &[Foot]) -> Vec<CoastProp> {
    let mut props = boulders(feet);
    props.extend(stacks(feet));
    props
}

fn random(seed: u32, salt: u32) -> f32 {
    (hash_u32(seed ^ salt.wrapping_mul(0x9e37_79b9)) & 0xffff) as f32 / 65535.
}

fn seed(p: Point) -> u32 {
    hash_u32(p[0].to_bits() ^ p[1].to_bits().rotate_left(13) ^ 0x5EA5_7AC5)
}

fn offset(foot: &Foot, distance: f32) -> Point {
    [foot.position[0] + foot.seaward[0] * distance, foot.position[1] + foot.seaward[1] * distance]
}

fn boulders(feet: &[Foot]) -> Vec<CoastProp> {
    let mut props = vec![];
    let mut since = BOULDER_SPACING;
    for pair in feet.windows(2) {
        let [a, b] = [pair[0].position, pair[1].position];
        since += (b[0] - a[0]).hypot(b[1] - a[1]);
        let foot = &pair[1];
        let s = seed(foot.position);
        if since < BOULDER_SPACING || s % BOULDER_SHARE != 0 {
            continue;
        }
        since = 0.;
        props.push(CoastProp {
            kind: CoastPropKind::Boulder,
            position: offset(foot, 0.05 + 0.08 * random(s, 1)),
            radius: 0.08 + 0.05 * random(s, 2),
            height: 0.03 + 0.05 * random(s, 3),
            angle: random(s, 4) * std::f32::consts::TAU,
            variant: s >> 8,
        });
    }
    props
}

/// How close inland the rays of the stations `span` apart on both sides meet:
/// small at a sharp convex headland, infinite where the coast is straight or a bay.
fn headland_depth(feet: &[Foot], j: usize) -> f32 {
    if j < HEADLAND_SPAN || j + HEADLAND_SPAN >= feet.len() {
        return f32::INFINITY;
    }
    let (a, b) = (&feet[j - HEADLAND_SPAN], &feet[j + HEADLAND_SPAN]);
    let chord = [b.position[0] - a.position[0], b.position[1] - a.position[1]];
    let length = chord[0].hypot(chord[1]).max(1e-6);
    // Inland rays close in when the seaward normals spread apart along the chord.
    let spreading = ((b.seaward[0] - a.seaward[0]) * chord[0] + (b.seaward[1] - a.seaward[1]) * chord[1]) / length;
    if spreading <= 1e-6 {
        return f32::INFINITY;
    }
    length / spreading
}

fn stacks(feet: &[Foot]) -> Vec<CoastProp> {
    let depth: Vec<f32> = (0..feet.len()).map(|j| headland_depth(feet, j)).collect();
    let mut props: Vec<CoastProp> = vec![];
    for j in 0..feet.len() {
        let sharpest = (j.saturating_sub(HEADLAND_SPAN)..(j + HEADLAND_SPAN + 1).min(feet.len())).all(|k| depth[k] >= depth[j]);
        if depth[j] > HEADLAND_DEPTH || !sharpest {
            continue;
        }
        let foot = &feet[j];
        let s = seed(foot.position);
        let position = offset(foot, 0.4 + 0.14 * random(s, 5));
        let crowded = props.iter().any(|p| (p.position[0] - position[0]).hypot(p.position[1] - position[1]) < STACK_SPACING);
        if crowded || random(s, 6) > 0.7 {
            continue;
        }
        let angle = random(s, 7) * std::f32::consts::TAU;
        props.push(CoastProp {
            kind: CoastPropKind::Stack,
            position,
            radius: 0.2 + 0.08 * random(s, 8),
            height: 0.42 + 0.2 * random(s, 9),
            angle,
            variant: s >> 8,
        });
        // A fallen block beside the stack, on its seaward flank.
        let side = [-foot.seaward[1], foot.seaward[0]];
        let lean = if random(s, 10) < 0.5 { -1. } else { 1. };
        props.push(CoastProp {
            kind: CoastPropKind::Boulder,
            position: [
                position[0] + (side[0] * lean * 0.22 + foot.seaward[0] * 0.08),
                position[1] + (side[1] * lean * 0.22 + foot.seaward[1] * 0.08),
            ],
            radius: 0.09 + 0.04 * random(s, 11),
            height: 0.03 + 0.04 * random(s, 12),
            angle: angle + 1.3,
            variant: (s >> 8) + 1,
        });
    }
    props
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight wall along x, then a half-circle headland of radius 0.6.
    fn coast() -> Vec<Foot> {
        let mut feet: Vec<Foot> = (0..60).map(|i| Foot { position: [i as f32 * 0.075, 0.], seaward: [0., -1.] }).collect();
        let center = [4.5, 0.6];
        for i in 0..=25 {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 25.;
            let n = [a.cos(), a.sin()];
            feet.push(Foot { position: [center[0] + 0.6 * n[0], center[1] + 0.6 * n[1]], seaward: n });
        }
        feet
    }

    #[test]
    fn stacks_stand_off_headlands_and_boulders_are_spaced_along_the_foot() {
        let feet = coast();
        let props = place(&feet);
        assert_eq!(props, place(&feet), "deterministic");
        let stacks: Vec<_> = props.iter().filter(|p| p.kind == CoastPropKind::Stack).collect();
        assert!(!stacks.is_empty(), "the headland gets a stack");
        for s in &stacks {
            assert!(s.position[0] > 4.5, "stacks stand off the headland, not the straight wall");
            let from_center = (s.position[0] - 4.5).hypot(s.position[1] - 0.6);
            assert!(from_center > 0.6 + 0.3, "a stack stands free in the water");
        }
        let boulders: Vec<_> = props.iter().filter(|p| p.kind == CoastPropKind::Boulder).collect();
        assert!(boulders.len() >= 3);
        for b in &boulders {
            assert!(b.radius <= 0.13 && b.height <= 0.08);
        }
    }

    #[test]
    fn a_straight_wall_has_no_stack() {
        let feet: Vec<Foot> = (0..80).map(|i| Foot { position: [i as f32 * 0.075, 0.], seaward: [0., -1.] }).collect();
        assert!(place(&feet).iter().all(|p| p.kind == CoastPropKind::Boulder));
    }
}
