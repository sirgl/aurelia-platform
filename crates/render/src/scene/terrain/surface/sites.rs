//! Natural wonder sites: a stage-4 cutout (platform/MESHING.md §2.2) that prepares the
//! ground for the wonder's model, which the world-art props draw on the tile
//! (`gpu/world_objects`). The surface does not model the wonder; it gives it
//! a place to stand, the `SiteShape` the wonder's model asks for in the model
//! manifest (`site_shape`): a level pad at the cell's own height, a bowl with
//! a low rim, or the sea floor lifted close under the surface around the tile.
//! A wonder without a model stands on a pad.
//!
//! Massif relief does not rise inside a site (`massifs.rs` also leaves the
//! wonder's tile out of the mountain seeds): a wonder mountain is its model.
//! `SurfaceMesh::wonder_sites` lists each known site with the ground level
//! its model stands on.
use super::{
    banks::SEA_LEVEL,
    biomes::{smooth, Biomes},
    hydrology::Point,
};
use crate::{scene::{fog::fog_at, layout::tile_to_world, style::CompiledTerrainStyle}, world::state::RenderWorld};
pub use crate::world_models::SiteShape;
use kernel::ids::TileId;

/// The pad is level within this distance of the tile centre and gone at the outer one.
const INNER: f32 = 0.62;
const OUTER: f32 = 0.98;
/// The reef lifts the sea floor this far from the tile centre.
const REEF_REACH: f32 = 1.6;
/// Requested edge length inside a site, so a rim or a pad edge keeps its shape.
pub const SITE_EDGE: f32 = 0.2;

/// A known natural wonder site and the ground level its model stands on.
#[derive(Debug, Clone, PartialEq)]
pub struct WonderSite {
    pub tile: u32,
    /// Content key of the wonder (empty when the world carries no keys).
    pub key: String,
    pub shape: SiteShape,
    /// World XZ of the tile centre and the level of the pad there.
    pub position: [f32; 3],
    /// Radius of the level part of the pad.
    pub radius: f32,
}

#[derive(Default)]
pub struct Sites {
    pub list: Vec<WonderSite>,
}

/// The ground a wonder's model asks for (`site_shape` in its manifest entry); a pad when the key
/// has no model.
fn shape_of(key: &str) -> SiteShape {
    crate::world_models::resolve(key).map_or(SiteShape::Pad, |model| model.model().site_shape)
}

impl Sites {
    pub fn new(world: &RenderWorld, biomes: &Biomes, style: &CompiledTerrainStyle) -> Self {
        Self::with_shapes(world, biomes, style, &shape_of)
    }

    /// The sites with the shapes `shape` gives to the wonders' keys (the tests of the geometry
    /// give them by hand, so they need no module's models).
    fn with_shapes(world: &RenderWorld, biomes: &Biomes, style: &CompiledTerrainStyle, shape: &dyn Fn(&str) -> SiteShape) -> Self {
        let mut list = vec![];
        for (i, wonder) in world.natural_wonders.iter().enumerate() {
            let Some(id) = wonder else { continue };
            let tile = TileId(i as u32);
            if fog_at(world, tile) < 0.4 {
                continue;
            }
            let key = world.natural_wonder_keys.get(*id as usize).cloned().unwrap_or_default();
            let (x, z) = tile_to_world(tile.0 % world.width, tile.0 / world.width);
            let shape = shape(&key);
            let ground = biomes.ground(style, x, z);
            let level = match shape {
                SiteShape::Reef => SEA_LEVEL,
                _ => ground.land_height.max(SEA_LEVEL + 0.12),
            };
            list.push(WonderSite { tile: tile.0, key, shape, position: [x, level, z], radius: INNER });
        }
        Self { list }
    }

    /// Sites for region fingerprints: a site shapes ground up to `REEF_REACH` away.
    pub fn fingerprint(&self, sink: &mut super::fingerprint::Sink) {
        for site in &self.list {
            let [x, y, z] = site.position;
            let words = [site.tile, site.shape as u32, x.to_bits(), y.to_bits(), z.to_bits(), site.radius.to_bits()];
            let reach = REEF_REACH.max(OUTER);
            sink.item(super::fingerprint::Tag::Site, [x - reach, z - reach], [x + reach, z + reach], &words);
        }
    }

    /// Distance to the nearest site and the site; few sites, a plain scan.
    fn nearest(&self, p: Point) -> Option<(f32, &WonderSite)> {
        self.list
            .iter()
            .map(|s| ((p[0] - s.position[0]).hypot(p[1] - s.position[2]), s))
            .min_by(|a, b| a.0.total_cmp(&b.0))
    }

    /// Smallest requested edge length within `slack` of `p`, if a site asks.
    pub fn edge_length(&self, p: Point, slack: f32) -> Option<f32> {
        let (d, site) = self.nearest(p)?;
        let reach = if site.shape == SiteShape::Reef { REEF_REACH } else { OUTER };
        (d < reach + slack).then_some(SITE_EDGE)
    }

    /// Stage 4 at a point: the ground height `y` reshaped by a site, and how
    /// much of the point the site owns (massif relief gives way by as much).
    #[cfg(test)]
    pub fn cut(&self, p: Point, y: f32, wet: bool) -> (f32, f32) {
        self.cut_shifted(p, y, wet, &|_| 0.)
    }

    /// `cut` with the level of the site lifted by `shift(site centre)` (the developer relief of the
    /// gallery probes moves the whole ground; the site must move with it).
    pub fn cut_shifted(&self, p: Point, y: f32, wet: bool, shift: &dyn Fn(Point) -> f32) -> (f32, f32) {
        let Some((d, site)) = self.nearest(p) else {
            return (y, 0.);
        };
        let level = site.position[1] + shift([site.position[0], site.position[2]]);
        match site.shape {
            SiteShape::Reef => {
                if !wet || d >= REEF_REACH {
                    return (y, 0.);
                }
                let w = 1. - smooth((d - 0.5) / (REEF_REACH - 0.5));
                let floor = SEA_LEVEL - 0.05;
                (y + (y.max(floor) - y) * w, 0.)
            }
            SiteShape::Pad | SiteShape::Crater => {
                if d >= OUTER {
                    return (y, 0.);
                }
                let w = 1. - smooth((d - INNER) / (OUTER - INNER));
                let mut target = level;
                if site.shape == SiteShape::Crater {
                    let bowl = (1. - (d / 0.48).powi(2)).max(0.) * 0.24;
                    let rim = (1. - ((d - 0.56) / 0.12).abs()).max(0.) * 0.07;
                    target += rim - bowl;
                }
                (y + (target - y) * w, w)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::style::{compile, default_style};
    use crate::world::state::TerrainCell;

    fn world(key: &str) -> RenderWorld {
        let (width, height) = (9u32, 7u32);
        let n = (width * height) as usize;
        let mut w = RenderWorld {
            width,
            height,
            visible: vec![1; n],
            explored: vec![1; n],
            terrain: (0..n as u32).map(|i| TerrainCell { tile: TileId(i), terrain: 1, owner: None, resource: None }).collect(),
            natural_wonders: vec![None; n],
            natural_wonder_keys: vec![key.to_string()],
            ..Default::default()
        };
        w.natural_wonders[(3 * width + 4) as usize] = Some(0);
        w
    }

    #[test]
    fn a_pad_is_level_at_the_site_and_leaves_the_ground_outside() {
        let w = world("a-pad");
        let style = compile(default_style()).unwrap();
        let sites = Sites::with_shapes(&w, &Biomes::new(&w), &style, &|_| SiteShape::Pad);
        let site = &sites.list[0];
        assert_eq!(site.shape, SiteShape::Pad);
        let [x, level, z] = site.position;
        for k in 0..12 {
            let a = k as f32 * 0.5;
            let (y, owned) = sites.cut([x + a.cos() * 0.5, z + a.sin() * 0.5], 0.9 + k as f32 * 0.01, false);
            assert!((y - level).abs() < 1e-5 && owned == 1.);
            let (far, none) = sites.cut([x + a.cos() * 1.2, z + a.sin() * 1.2], 0.9, false);
            assert!(far == 0.9 && none == 0.);
        }
        // Continuous across the pad edge.
        let mut previous = sites.cut([x + INNER, z], 0.5, false).0;
        for i in 1..100 {
            let y = sites.cut([x + INNER + i as f32 * (OUTER - INNER) / 100., z], 0.5, false).0;
            assert!((y - previous).abs() < 0.02);
            previous = y;
        }
    }

    #[test]
    fn the_crater_is_a_bowl_and_the_reef_lifts_the_sea_floor() {
        let style = compile(default_style()).unwrap();
        let shapes = |key: &str| if key == "a-crater" { SiteShape::Crater } else { SiteShape::Reef };
        let w = world("a-crater");
        let sites = Sites::with_shapes(&w, &Biomes::new(&w), &style, &shapes);
        let [x, level, z] = sites.list[0].position;
        assert!(sites.cut([x, z], level, false).0 < level - 0.2, "a bowl in the middle");
        assert!(sites.cut([x + 0.56, z], level, false).0 > level + 0.05, "a rim around it");
        let w = world("a-reef");
        let sites = Sites::with_shapes(&w, &Biomes::new(&w), &style, &shapes);
        let [x, _, z] = sites.list[0].position;
        assert!(sites.cut([x + 0.8, z], -0.6, true).0 > -0.2, "shallow water over the reef");
        assert_eq!(sites.cut([x + 0.8, z], 0.4, false).0, 0.4, "dry ground stays");
    }

    #[test]
    fn a_wonder_without_a_model_stands_on_a_pad() {
        assert_eq!(shape_of("a-module:no-such-wonder"), SiteShape::Pad);
    }
}
