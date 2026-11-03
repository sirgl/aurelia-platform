//! Fog of war as data: a per-tile factor (visible / explored / murk).
//! A pure function of RenderWorld: a shared place for meshers (terrain,
//! water, props): the same numbers everywhere, no duplication.

use crate::world::state::RenderWorld;

/// The world as CPU geometry builders must see it: every known tile counts as visible.
///
/// Geometry, props, borders and shadow casters depend on knowledge, never on what is
/// visible right now; the current visibility reaches the GPU as the fog field and is
/// applied in shaders. With visible := known, each builder's visibility threshold
/// (0.4, 0.55, 0.95) reduces to a knowledge threshold, and baked vertex fog is an
/// upper bound the shaders take the minimum with.
pub fn knowledge_view(world: &RenderWorld) -> RenderWorld {
    let mut view = world.clone();
    for (i, visible) in view.visible.iter_mut().enumerate() {
        *visible = u8::from(*visible == 1 || world.explored.get(i) == Some(&1));
    }
    view
}

/// `knowledge_view` kept in step with a changing world without copying it whole: each
/// sync copies only the parts of the world whose layer revision moved (a unit move
/// copies the unit list). The renderer owns one; it equals `knowledge_view(world)`
/// after every `sync`.
#[derive(Debug, Default)]
pub struct KnowledgeView {
    view: RenderWorld,
    synced: Option<crate::world::state::LayerRevisions>,
}

impl KnowledgeView {
    pub fn sync(&mut self, world: &RenderWorld) -> &RenderWorld {
        let shape = |w: &RenderWorld| (w.width, w.height, w.observer, w.visual_surface, w.terrain.len());
        let Some(old) = self.synced.filter(|_| shape(&self.view) == shape(world)) else {
            self.view = knowledge_view(world);
            self.synced = Some(world.layers);
            return &self.view;
        };
        let now = world.layers;
        let view = &mut self.view;
        if (old.terrain, old.materials, old.features, old.borders) != (now.terrain, now.materials, now.features, now.borders) {
            // Per-tile data of several layers shares the cells: copy every tile vector.
            view.terrain.clone_from(&world.terrain);
            view.improvements.clone_from(&world.improvements);
            view.pillaged.clone_from(&world.pillaged);
            view.road.clone_from(&world.road);
            view.natural_wonders.clone_from(&world.natural_wonders);
            view.natural_wonder_keys.clone_from(&world.natural_wonder_keys);
            view.river.clone_from(&world.river);
            view.forest.clone_from(&world.forest);
            view.feature.clone_from(&world.feature);
            view.river_edges.clone_from(&world.river_edges);
            view.unlocked_resources.clone_from(&world.unlocked_resources);
        }
        if old.knowledge != now.knowledge || old.terrain != now.terrain {
            view.explored.clone_from(&world.explored);
            view.visible.clone_from(&world.visible);
            for (i, visible) in view.visible.iter_mut().enumerate() {
                *visible = u8::from(*visible == 1 || world.explored.get(i) == Some(&1));
            }
        }
        if old.units != now.units {
            view.units.clone_from(&world.units);
        }
        if old.cities != now.cities {
            view.cities.clone_from(&world.cities);
        }
        view.layers = now;
        self.synced = Some(now);
        &self.view
    }
}

/// Fog factor per tile: visible / explored / murk.
pub fn fog_at(world: &RenderWorld, tile: kernel::ids::TileId) -> f32 {
    let i = tile.0 as usize;
    if world.visible.get(i).copied().unwrap_or(0) > 0 {
        1.0
    } else if world.explored.get(i).copied().unwrap_or(0) > 0 {
        0.45
    } else {
        0.18
    }
}

// World-space wash variation is independent of terrain and knowledge contents.
fn wash_noise(x: f32, z: f32) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let hash = |a: i32, b: i32| {
        let seed = (a as u32).wrapping_mul(0x9e3779b9) ^ (b as u32).wrapping_mul(0x85ebca6b);
        crate::scene::terrain::fields::hash_u32(seed) as f32 / u32::MAX as f32
    };
    let smooth = |t: f32| t * t * (3. - 2. * t);
    let tx = smooth(x - x.floor());
    let tz = smooth(z - z.floor());
    let near = hash(ix, iz) * (1. - tx) + hash(ix + 1, iz) * tx;
    let far = hash(ix, iz + 1) * (1. - tx) + hash(ix + 1, iz + 1) * tx;
    near * (1. - tz) + far * tz
}

/// Conservative, position-based visibility. Fade on the known side of a
/// boundary; this function never reads terrain or other hidden content.
pub fn fog_at_position(world: &RenderWorld, x: f32, z: f32) -> f32 {
    let rf = z / 1.5;
    let qf = x / 1.732_050_8 - rf * 0.5;
    let q0 = qf.round() as i32;
    let r0 = rf.round() as i32;
    let mut fog = 1.0f32;
    // Vary the width and reach of the wash, not just its final color. The
    // minimum radius still contains every point of an unknown radius-one hex;
    // the fade is narrow so it stays near the shared edge and a visible
    // neighbour (centre 1.73 away) is not washed out — a wide fade used to
    // hide the whole outer ring of a unit's sight.
    let wash = wash_noise(x * 0.39 + 13.7, z * 0.39 - 8.2) * 0.73
        + wash_noise(x * 1.13 - 3.9, z * 1.13 + 25.1) * 0.27;
    let radius = 1.01 + wash * 0.12;
    let width = 0.30 + wash_noise(x * 0.18 + 7., z * 0.18 + 41.) * 0.12;
    for r in r0 - 2..=r0 + 2 {
        for q in q0 - 2..=q0 + 2 {
            let known = if q >= 0 && r >= 0 && q < world.width as i32 && r < world.height as i32 {
                fog_at(
                    world,
                    kernel::ids::TileId(r as u32 * world.width + q as u32),
                )
            } else {
                0.18
            };
            if known >= 1.0 {
                continue;
            }
            let cx = 1.732_050_8 * (q as f32 + r as f32 * 0.5);
            let cz = 1.5 * r as f32;
            let distance = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt();
            let t = ((distance - radius) / width).clamp(0.0, 1.0);
            let blend = t * t * (3.0 - 2.0 * t);
            fog = fog.min(known + (1.0 - known) * blend);
        }
    }
    fog
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Differential: a view synced layer by layer equals a fresh copy after every change.
    #[test]
    fn synced_view_equals_a_fresh_knowledge_view() {
        use crate::world::state::{CityMarker, Layer, TerrainCell, UnitMarker};
        use kernel::ids::TileId;
        let n = 12 * 9;
        let mut world = RenderWorld {
            width: 12,
            height: 9,
            terrain: (0..n).map(|i| TerrainCell { tile: TileId(i as u32), terrain: 1, owner: None, resource: None }).collect(),
            visible: vec![0; n],
            explored: vec![0; n],
            improvements: vec![None; n],
            road: vec![0; n],
            river: vec![0; n],
            forest: vec![0; n],
            ..Default::default()
        };
        let mut synced = KnowledgeView::default();
        let mut seed = 3u32;
        for step in 0..200 {
            seed = crate::scene::terrain::fields::hash_u32(seed);
            let i = (seed >> 4) as usize % n;
            let layer = match seed % 7 {
                0 => { world.terrain[i].terrain = (seed % 6) as u16; Layer::Terrain }
                1 => { world.improvements[i] = Some((seed % 3) as u16); Layer::Materials }
                2 => { world.forest[i] ^= 1; Layer::Features }
                3 => { world.units.push(UnitMarker { id: 0, embarked: false, fortified: false, tile: TileId(i as u32), owner: 0, kind: 0, hp: 100 }); Layer::Units }
                4 => { world.cities.push(CityMarker { tile: TileId(i as u32), owner: 1, name: "c".into(), population: Some(1), buildings: vec![], era: 0, showpieces: vec![], unworked: vec![] }); Layer::Cities }
                5 => { world.terrain[i].owner = Some(2); Layer::Borders }
                _ => { world.visible[i] ^= 1; world.explored[i] = 1; Layer::Knowledge }
            };
            world.touch(layer);
            let fresh = knowledge_view(&world);
            assert_eq!(format!("{:?}", synced.sync(&world)), format!("{fresh:?}"), "step {step}");
        }
    }

    #[test]
    fn irregular_wash_never_reveals_any_point_of_an_unknown_hex() {
        let mut world = RenderWorld {
            width: 9, height: 9,
            visible: vec![1; 81], explored: vec![1; 81],
            ..Default::default()
        };
        for (q, r) in [(2, 3), (4, 4), (6, 5)] {
            let index = (r * world.width + q) as usize;
            world.visible[index] = 0;
            world.explored[index] = 0;
            let (cx, cz) = crate::scene::layout::tile_to_world(q, r);
            // Testing the entire circumscribed disk is stricter than the hex.
            for radius in [0., 0.25, 0.5, 0.75, 1.] {
                for i in 0..64 {
                    let a = i as f32 * std::f32::consts::TAU / 64.;
                    let fog = fog_at_position(&world, cx + a.cos() * radius, cz + a.sin() * radius);
                    assert!((fog - 0.18).abs() < 1e-5, "hidden point disclosed: {q},{r} radius {radius} angle {a}: {fog}");
                }
            }
        }
    }
}
