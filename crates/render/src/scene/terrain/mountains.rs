//! Shared geological mountain field: seeds and profile, read by the surface
//! massif contributor (surface/massifs.rs) and the legacy mountain shells.
use crate::world::state::RenderWorld;
use kernel::ids::TileId;

/// Shared terrain vertices form one continuous rock surface, rather than a cone
/// for each hex. Sparse summits overlap broad shoulders to form natural saddles.
#[derive(Clone, Copy)]
pub(crate) struct MountainSeed {
    pub center: [f32; 2],
    pub radius: f32,
    pub height: f32,
    pub rotation: f32,
    pub aspect: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct MountainSample {
    pub pos: [f32; 3],
    pub lift: f32,
    pub snow: f32,
    pub fog: f32,
}

pub(crate) fn mountain_seeds(world: &RenderWorld) -> Vec<MountainSeed> {
    use crate::scene::terrain::features::is_mountain_tile;
    mountain_seeds_where(world, |tile| is_mountain_tile(world, tile))
}

/// Seeds of the mountain cells `is_mountain` names; their summit rank reads
/// the same predicate.
pub(crate) fn mountain_seeds_where(world: &RenderWorld, is_mountain: impl Fn(TileId) -> bool) -> Vec<MountainSeed> {
    use crate::scene::terrain::features::is_summit;
    use crate::scene::terrain::fields::hash_u32;
    world
        .terrain
        .iter()
        .filter(|c| is_mountain(c.tile))
        .map(|c| {
            let center = super::features::mountain_center(world.width, c.tile);
            let random = (hash_u32(c.tile.0 ^ 0x390A) % 1000) as f32 / 1000.0;
            let summit = is_summit(world, c.tile, &is_mountain);
            MountainSeed {
                center,
                radius: if summit {
                    2.1 + random * 0.55
                } else {
                    1.55 + random * 0.4
                },
                height: if summit {
                    1.9 + random * 0.8
                } else {
                    0.35 + random * 0.55
                },
                rotation: random * std::f32::consts::TAU,
                aspect: 0.72 + random * 0.35,
            }
        })
        .collect()
}

/// Asymmetric ridge with planar flanks and sloping ends, inside the seed's radius.
/// Bending its crest changes the silhouette without adding fine vertex noise.
pub(crate) fn mountain_profile(u: f32, v: f32, variation: f32) -> f32 {
    let along = u / 0.90;
    let across = (v - (0.06 + 0.08 * variation) * u - 0.05 * u.abs()) / 0.64;
    let flank = (across * (1.10 + 0.10 * variation)).max(-across * (0.94 + 0.08 * variation));
    let crest = 1. - 0.26 * along.abs() - flank;
    let end = 1.
        - (along * (1. + 0.05 * variation)).max(-along * (1.06 - 0.04 * variation))
        - 0.50 * across.abs();
    let envelope = crest.min(end).max(0.);
    // Two unequal erosion gullies split the flanks into buttresses. Their
    // piecewise-linear cuts are broad geological planes, not per-vertex noise.
    // Fade the cuts at the summit and foot so adjacent massifs stay connected.
    let exposure = (1. - envelope).min(envelope * 1.7).max(0.);
    let cut_a = (1. - (u - 0.32 - v * 0.48).abs() / 0.15).max(0.);
    let cut_b = (1. - (u + 0.38 + v * 0.32).abs() / 0.20).max(0.);
    let slope = (v.abs() / 0.24).min(1.);
    (envelope - exposure * slope * (cut_a * 0.43 + cut_b * 0.29)).max(0.)
}

pub(crate) fn mountain_sample(
    world: &RenderWorld,
    seeds: &[MountainSeed],
    point: [f32; 3],
) -> MountainSample {
    let [x, ground, z] = point;
    let (mut highest, mut total, mut snow) = (0.0f32, 0.0f32, 0.0f32);
    let fog = crate::scene::picking::world_to_tile(x, z, (world.width, world.height))
        .map(|tile| crate::scene::fog::fog_at(world, tile))
        .unwrap_or(0.18);
    for seed in seeds {
        let (sin, cos) = seed.rotation.sin_cos();
        let dx = x - seed.center[0];
        let dz = z - seed.center[1];
        let u = (dx * cos + dz * sin) / seed.radius;
        let v = (-dx * sin + dz * cos) / (seed.radius * seed.aspect);
        let variation = seed.rotation / std::f32::consts::TAU;
        let amount = mountain_profile(u, v, variation);
        let height = seed.height * amount;
        total += height;
        if height > highest {
            highest = height;
            snow = if seed.height > 1.8 && amount > 0.67 {
                1.0
            } else {
                0.0
            };
        }
    }
    let lift = highest + (total - highest) * 0.16;
    MountainSample {
        pos: [x, ground + lift, z],
        lift,
        snow,
        fog,
    }
}

pub(crate) fn rock_color(p: [f32; 2]) -> [f32; 3] {
    let mineral = ((p[0] * 0.37 + p[1] * 0.23).sin() * 0.5 + 0.5) * 0.07;
    [0.48 + mineral, 0.51 + mineral * 0.65, 0.54 + mineral * 0.28]
}
