//! Stage 2 of the field (platform/MESHING.md §2.2): mountain massifs.
//!
//! A massif is an `Add` of geological lift from the known mountain seeds
//! (`mountains.rs`), read from the world observation only, never from the
//! mesh. Its lift rides on top of the level that hydrology and cutouts shape:
//! a cliff keeps its profile height and the massif rises above it, rather
//! than the coast cutting a wall as tall as the mountain. Two rules bound it:
//! a river ravine attenuates the lift (stage 3 owns the channel), and the
//! protection stage drops it at wet contacts, which stay exactly at the
//! water level (`SurfaceMesh` stage 5).
use super::hydrology::{Point, River};
use super::MountainVertex;
use crate::{
    scene::{
        fog::fog_at,
        terrain::{
            features::is_mountain_tile,
            mountains::{mountain_sample, mountain_seeds_where, rock_color, MountainSeed},
        },
    },
    world::state::RenderWorld,
};
use std::collections::BTreeMap;

/// Seeds are binned by this cell size; a seed is listed in every bin its radius touches.
const BIN: f32 = 4.;

#[derive(Default)]
pub struct Massifs {
    bins: BTreeMap<(i32, i32), Vec<MountainSeed>>,
}

fn bin(x: f32) -> i32 {
    (x / BIN).floor() as i32
}

impl Massifs {
    pub fn new(world: &RenderWorld) -> Self {
        // Hidden neighbours must not affect the rank of a known summit.
        // A natural wonder's model is its own mountain: no massif under it.
        let wonder = |tile: kernel::ids::TileId| world.natural_wonders.get(tile.0 as usize).copied().flatten().is_some();
        let known_mountain = |tile| is_mountain_tile(world, tile) && fog_at(world, tile) >= 0.4 && !wonder(tile);
        let mut bins = BTreeMap::<_, Vec<_>>::new();
        for seed in mountain_seeds_where(world, known_mountain) {
            let radius = seed.radius * seed.aspect.max(1.);
            for z in bin(seed.center[1] - radius)..=bin(seed.center[1] + radius) {
                for x in bin(seed.center[0] - radius)..=bin(seed.center[0] + radius) {
                    bins.entry((x, z)).or_default().push(seed);
                }
            }
        }
        Self { bins }
    }

    /// Each bin and the seeds a point in it reads, for region fingerprints.
    pub fn fingerprint(&self, sink: &mut super::fingerprint::Sink) {
        for (&(x, z), seeds) in &self.bins {
            let mut words = vec![x as u32, z as u32];
            for s in seeds {
                words.extend([s.center[0], s.center[1], s.radius, s.height, s.rotation, s.aspect].map(f32::to_bits));
            }
            let low = [x as f32 * BIN, z as f32 * BIN];
            sink.item(super::fingerprint::Tag::Massif, low, [low[0] + BIN, low[1] + BIN], &words);
        }
    }

    /// Lift and snow at a known point; zero lift where no massif reaches.
    /// `river` is the nearest river within the hydrology's query radius.
    pub fn lift(&self, world: &RenderWorld, p: Point, fog: f32, river: Option<(&River, f32, f32)>) -> MountainVertex {
        let none = MountainVertex::default();
        if self.bins.is_empty() || fog < 0.4 {
            return none;
        }
        let nearby = self.bins.get(&(bin(p[0]), bin(p[1]))).map(Vec::as_slice).unwrap_or(&[]);
        let sample = mountain_sample(world, nearby, [p[0], 0., p[1]]);
        if sample.lift <= 0. {
            return none;
        }
        let factor = ravine(river, sample.lift);
        MountainVertex { lift: sample.lift * factor, snow: sample.snow * factor }
    }
}

/// Carved river contacts remain authoritative; higher relief has a wider ravine.
fn ravine(river: Option<(&River, f32, f32)>, lift: f32) -> f32 {
    let Some((r, d, t)) = river else {
        return 1.;
    };
    let distance = d - r.width_at(t);
    let shoulder = (0.36 + lift * 0.55).min(1.40);
    let t = ((distance - 0.10) / shoulder).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Rock albedo on the shared vertices of the soft grass/stone foot. Snow is a
/// face material chosen by the GPU adapter, never a white vertex gradient.
pub fn paint(albedo: [f32; 3], p: Point, lift: f32) -> [f32; 3] {
    if lift <= 0. {
        return albedo;
    }
    let stone = rock_color(p);
    let blend = ((lift - 0.025) / 0.15).clamp(0., 1.);
    std::array::from_fn(|k| albedo[k] * (1. - blend) + stone[k] * blend)
}
