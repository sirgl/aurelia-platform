//! The terrain and water fragment shaders skip the part of their colour the knowledge fog
//! hides (`platform/assets/shaders/cartography.wgsl`, `VISIBLE_FOG_MIN` and `REMEMBERED_FOG_MAX`).
//! That is exact only while `map_finish` really mixes in no remembered colour at the top
//! of the fog range and no live colour at the bottom: this is its arithmetic, ported, over
//! the whole range of the noise it takes.

const CARTOGRAPHY: &str = include_str!("../../../assets/shaders/cartography.wgsl");
const TERRAIN: &str = include_str!("../../../assets/shaders/terrain.wgsl");
const WATER: &str = include_str!("../../../assets/shaders/water.wgsl");
const FEATURE: &str = include_str!("../../../assets/shaders/feature.wgsl");
const MODELS: &str = include_str!("../../../assets/shaders/hex.wgsl");

fn constant(name: &str) -> f32 {
    let line = CARTOGRAPHY
        .lines()
        .find(|l| l.starts_with(&format!("const {name}: f32 =")))
        .unwrap_or_else(|| panic!("cartography.wgsl declares {name}"));
    line.split('=').nth(1).unwrap().trim().trim_end_matches(';').parse().unwrap()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `map_edge`: `wash` is `map_wash` (a weighted sum of three value noises, 0..=1) and
/// `feather` the 0..=0.08 noise term.
fn edge(fog: f32, wash: f32, feather: f32) -> f32 {
    let progress = ((fog - 0.52) / 0.48).clamp(0.0, 1.0);
    progress - (wash * 0.76 + feather) * (1.0 - progress)
}

fn reveal(fog: f32, wash: f32, feather: f32) -> f32 {
    smoothstep(0.015, 0.61, edge(fog, wash, feather))
}

/// The weight of the deposits `map_finish` paints over the mix (`edge_band`).
fn edge_band(fog: f32) -> f32 {
    smoothstep(0.48, 0.57, fog) * (1.0 - smoothstep(0.94, 1.0, fog))
}

fn noise_corners() -> impl Iterator<Item = (f32, f32)> {
    let washes = [0.0, 0.1, 0.5, 0.9, 1.0];
    let feathers = [0.0, 0.04, 0.08];
    washes.into_iter().flat_map(move |w| feathers.into_iter().map(move |f| (w, f)))
}

#[test]
fn remembered_fog_shows_none_of_the_live_colour() {
    let top = constant("REMEMBERED_FOG_MAX");
    for step in 0..=100 {
        let fog = top * step as f32 / 100.0;
        for (wash, feather) in noise_corners() {
            assert_eq!(reveal(fog, wash, feather), 0.0, "fog {fog}, wash {wash}, feather {feather}");
        }
    }
}

#[test]
fn visible_fog_shows_none_of_the_remembered_colour() {
    let bottom = constant("VISIBLE_FOG_MIN");
    for fog in [bottom, (bottom + 1.0) * 0.5, 1.0, 1.0001] {
        for (wash, feather) in noise_corners() {
            assert!(reveal(fog, wash, feather) >= 1.0 - 1e-6, "fog {fog}, wash {wash}, feather {feather}");
        }
        // The deposits (at most about a third of the pigment) fade out with the band.
        assert!(edge_band(fog) * 0.34 < 1e-4, "fog {fog}: edge band {}", edge_band(fog));
    }
}

/// `map_known`.
fn known(fog: f32) -> f32 {
    smoothstep(0.20, 0.45, fog)
}

#[test]
fn unknown_fog_is_the_bare_parchment() {
    // Up to BARE_FOG_MAX nothing remembered, no reveal and no edge band: the terrain shader
    // returns the parchment alone.
    let top = constant("BARE_FOG_MAX");
    assert_eq!(known(top), 0.0);
    for step in 0..=100 {
        let fog = top * step as f32 / 100.0;
        assert_eq!(known(fog), 0.0, "fog {fog}");
        assert_eq!(edge_band(fog), 0.0, "fog {fog}");
        for (wash, feather) in noise_corners() {
            assert_eq!(reveal(fog, wash, feather), 0.0, "fog {fog}, wash {wash}, feather {feather}");
        }
    }
}

#[test]
fn the_edge_band_starts_at_its_constant() {
    let bottom = constant("EDGE_FOG_MIN");
    for step in 0..=100 {
        let fog = bottom * step as f32 / 100.0;
        assert_eq!(edge_band(fog), 0.0, "fog {fog}");
    }
    assert!(constant("EDGE_FOG_MIN") <= constant("REMEMBERED_FOG_MAX"));
    assert!(constant("BARE_FOG_MAX") < constant("EDGE_FOG_MIN"));
}

#[test]
fn the_shaders_branch_on_those_constants() {
    for (name, source) in [("terrain", TERRAIN), ("water", WATER)] {
        assert!(source.contains("fog > REMEMBERED_FOG_MAX"), "{name}: the lit colour is skipped for remembered fog");
        assert!(source.contains("fog >= VISIBLE_FOG_MIN") || source.contains("fog < VISIBLE_FOG_MIN"), "{name}: the remembered colour is skipped for visible fog");
    }
    // The terrain shader also stops early for the bare parchment and before the edge band.
    assert!(TERRAIN.contains("fog > BARE_FOG_MAX"));
    assert!(TERRAIN.contains("fog > EDGE_FOG_MIN"));
    // Objects light their pixel anyway; only the remembered pigment is skipped when visible.
    for (name, source) in [("feature", FEATURE), ("models", MODELS)] {
        assert!(source.contains("fog >= VISIBLE_FOG_MIN"), "{name}: the remembered colour is skipped for visible fog");
    }
}
