//! Stage 4 of the field (platform/MESHING.md §2.2, decision D3): cutouts.
//!
//! A cutout (cliff, terrace, quarry, city plateau) proposes a section of
//! height and materials with a weight that is one inside its support and
//! falls to zero at its edge. Overlapping cutouts combine by a partition of
//! unity: the weights are normalized where they sum past one, and the input
//! keeps the remaining share. The result does not depend on the order in
//! which cutouts are listed, and is continuous wherever the weights are.
use super::transitions::CoastSection;

#[derive(Debug, Clone, Copy)]
pub struct Cut {
    pub weight: f32,
    pub section: CoastSection,
}

pub fn partition(input: CoastSection, cuts: &[Cut]) -> CoastSection {
    let total: f32 = cuts.iter().map(|c| c.weight.max(0.)).sum();
    if total <= 0. {
        return input;
    }
    let scale = if total > 1. { 1. / total } else { 1. };
    let keep = 1. - total * scale;
    let mut out = CoastSection { height: 0., sand: 0., stone: 0. };
    for cut in cuts {
        let w = cut.weight.max(0.) * scale;
        out.height += cut.section.height * w;
        out.sand += cut.section.sand * w;
        out.stone += cut.section.stone * w;
    }
    CoastSection {
        height: input.height * keep + out.height,
        sand: input.sand * keep + out.sand,
        stone: input.stone * keep + out.stone,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(height: f32, stone: f32) -> CoastSection {
        CoastSection { height, sand: 0.2, stone }
    }

    #[test]
    fn overlapping_cutouts_commute_and_weights_partition_unity() {
        let input = section(0.3, 0.);
        let a = Cut { weight: 0.7, section: section(0.8, 1.) };
        let b = Cut { weight: 0.6, section: section(0.5, 0.4) };
        let ab = partition(input, &[a, b]);
        let ba = partition(input, &[b, a]);
        assert!((ab.height - ba.height).abs() < 1e-6 && (ab.stone - ba.stone).abs() < 1e-6);
        // Weights past one are normalized: the input share vanishes.
        assert!((ab.height - (0.8 * 0.7 + 0.5 * 0.6) / 1.3).abs() < 1e-6);
        // One partial cutout keeps the rest of the input.
        let half = partition(input, &[Cut { weight: 0.5, section: section(0.8, 1.) }]);
        assert!((half.height - 0.55).abs() < 1e-6 && (half.stone - 0.5).abs() < 1e-6);
        assert_eq!(partition(input, &[]).height, input.height);
    }

    #[test]
    fn partition_is_continuous_in_the_weights() {
        let input = section(0.3, 0.);
        let cut = |w: f32| partition(input, &[Cut { weight: w, section: section(0.9, 1.) }, Cut { weight: w * 0.5, section: section(0.1, 0.) }]);
        let mut previous = cut(0.).height;
        for i in 1..=200 {
            let h = cut(i as f32 / 100.).height;
            assert!((h - previous).abs() < 0.02, "no jump at weight {}", i as f32 / 100.);
            previous = h;
        }
    }
}
