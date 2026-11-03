//! Seating of world models on the final surface (platform/MESHING.md 2.6: objects are placed after
//! the mesh by the same queries as picking). Pure functions of the terrain geometry, run once
//! when an object is created or its terrain chunk changes; nothing here runs per frame.
//!
//! A model touches the ground at its *contacts* (the base outline the bake found in the GLB,
//! turned by yaw and scale), not at a circle around its anchor:
//!
//! - `Terraced` (buildings): the high side of the footprint is buried a little (`SINK`, the
//!   foundation course); a low stone plinth fills the gap below the low side, in two tiers when
//!   tall, never deeper than `PLINTH_MAX` (beyond that the high side is buried instead).
//! - `Bare` (ground cover, resources, improvements, natural wonders: wide compositions that a
//!   plinth would turn into a stone disc): the ground under the heart of the model, but never
//!   higher than a share of its radius above the lowest contact; no plinth.
//! - `Shore` (quay, lighthouse): the land contacts set the height, the deck stays above the
//!   water, piles reach the sea floor under the part that stands over water.
//! - Plane models (fields, wall parts) are sheared onto the least-squares plane through their
//!   contacts, the slope limited, the offset centred between the highest and lowest residual.
//! - Water models float at the water surface.

use crate::gpu::buffers::HexInstance;
use crate::scene::terrain::evaluator::sample_surface;
use crate::scene::terrain::fields::TerrainGeometry;
use crate::world_models::{self, Ground, Seat, WorldModel, WorldModelId};

/// The tallest plinth of the seating in force (the new one allows stepped courses).
pub fn plinth_max() -> f32 {
    if crate::debug_view::slope_mode_new() {
        PLINTH_MAX_NEW
    } else {
        PLINTH_MAX
    }
}

/// The ground may stand this far above a rigid model's base on its high side (the buried
/// foundation course of the authored mesh sinks 0.02 below it).
pub const SINK: f32 = 0.03;
/// A plinth reaches this far below the lowest ground under it.
const EMBED: f32 = 0.04;
/// A gap below the low side up to this size is hidden by the authored base: no plinth.
const GAP_TOL: f32 = 0.06;
/// The deepest plinth; a steeper site buries the high side more instead.
pub const PLINTH_MAX: f32 = 0.30;
/// A plinth taller than this is built in two tiers: a wide low one and a narrower upper one.
const TWO_TIERS: f32 = 0.14;
const LOWER_SHARE: f32 = 0.55;
const LOWER_WIDEN: f32 = 1.10;
/// The plinth top reaches this far into the model base (no seam of light).
const TOP_OVERLAP: f32 = 0.006;
/// Plinth radius = footprint radius times this plus the margin.
const PLINTH_FLARE: f32 = 1.03;
/// The new seating: tighter gap tolerance, a plinth no wider than the footprint (lower tier 4 % more), lower.
const GAP_TOL_NEW: f32 = 0.035;
const PLINTH_FLARE_NEW: f32 = 1.0;
const LOWER_WIDEN_NEW: f32 = 1.04;
const PLINTH_MAX_NEW: f32 = 0.42;
/// Height of one stone course of the new seating, and the most courses.
const TIER_NEW: f32 = 0.15;
const MAX_TIERS_NEW: usize = 3;
/// A world wonder is a monument: it may stand on a podium of up to five courses.
const WONDER_PLINTH_MAX: f32 = 0.70;
const MAX_TIERS_WONDER: usize = 5;
const PLINTH_MARGIN: f32 = 0.006;
/// Points on the plinth rim probed for the lowest ground.
const RIM: usize = 8;
/// A bare model stands at most this share of its footprint radius above the lowest ground under it.
pub const BARE_REACH: f32 = 0.08;
/// The steepest slope (dy per unit) a sheared model follows.
pub const SLOPE_MAX: f32 = 0.6;
/// A plane model is lowered this much from the centre of its residuals.
const PLANE_BIAS: f32 = 0.005;
/// The lowest contact of a sheared model is at most this far below its base.
pub const PLANE_HANG: f32 = 0.08;
/// The new seating of sheared models: the ground may stand this far above the base on the high side.
const SHEAR_SINK: f32 = 0.02;
/// The steepest slope a bare composition is sheared onto, and how far its base may hang above the lowest contact.
pub const BARE_SLOPE_MAX: f32 = 0.6;
pub const BARE_HANG: f32 = 0.14;
/// Deck of a quay stands this far above the water surface.
const DECK: f32 = 0.025;
const PILE_RADIUS: f32 = 0.014;
const MAX_PILES: usize = 4;

/// Where and how a model stands: anchor (x, z), yaw (radians), owner color, fog.
#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub color: [f32; 3],
    pub fog: f32,
}

/// A seated model: its instance and the support (plinth tiers, piles) standing under it.
#[derive(Debug, Clone)]
pub struct Seated {
    pub main: HexInstance,
    pub support: Vec<(WorldModelId, HexInstance)>,
}

/// Ground height at (x, z) on the final surface; None outside the mesh.
fn ground(geom: &TerrainGeometry, x: f32, z: f32) -> Option<f32> {
    sample_surface(geom, x, z).map(|s| s.y)
}

/// Water surface at (x, z); None where there is no water above the ground (water triangles also
/// run under the shore and under hills).
fn water_level(geom: &TerrainGeometry, x: f32, z: f32) -> Option<f32> {
    let level = geom.surface.as_ref().and_then(|s| s.sample_water(x, z)).map(|s| s.y)?;
    match ground(geom, x, z) {
        Some(g) if g >= level => None,
        _ => Some(level),
    }
}

/// Water surface at (x, z); the ground when there is no water there.
fn water(geom: &TerrainGeometry, x: f32, z: f32) -> Option<f32> {
    water_level(geom, x, z).or_else(|| ground(geom, x, z))
}

/// Ground height range (lowest, highest) under a set of points; None when none is on the mesh.
pub fn relief_range(geom: &TerrainGeometry, points: &[[f32; 2]]) -> Option<(f32, f32)> {
    let mut range: Option<(f32, f32)> = None;
    for p in points {
        if let Some(y) = ground(geom, p[0], p[1]) {
            range = Some(range.map_or((y, y), |(lo, hi)| (lo.min(y), hi.max(y))));
        }
    }
    range
}

/// Height difference of the ground under a set of points.
pub fn relief(geom: &TerrainGeometry, points: &[[f32; 2]]) -> Option<f32> {
    relief_range(geom, points).map(|(lo, hi)| hi - lo)
}

/// A point and the ground under it.
type Probe = ([f32; 2], f32);

/// The model's contacts in world x/z with the ground under each (those on the mesh).
fn probes(geom: &TerrainGeometry, m: &WorldModel, inst: &HexInstance) -> Vec<Probe> {
    m.contacts
        .iter()
        .filter_map(|c| {
            let p = inst.transform([c[0], 0.0, c[1]]);
            ground(geom, p[0], p[2]).map(|y| ([p[0], p[2]], y))
        })
        .collect()
}

fn extent(probes: &[Probe]) -> (f32, f32) {
    probes.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)))
}

/// Least-squares plane y = a + b dx + c dz through the probes around (x, z).
fn fit_plane(probes: &[Probe], x: f32, z: f32) -> (f32, [f32; 2]) {
    let n = probes.len() as f32;
    let (mut sx, mut sz, mut sxx, mut sxz, mut szz) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let (mut sy, mut sxy, mut szy) = (0.0f32, 0.0f32, 0.0f32);
    for ([px, pz], y) in probes {
        let (dx, dz) = (px - x, pz - z);
        sx += dx;
        sz += dz;
        sxx += dx * dx;
        sxz += dx * dz;
        szz += dz * dz;
        sy += y;
        sxy += dx * y;
        szy += dz * y;
    }
    // Cramer on [n sx sz; sx sxx sxz; sz sxz szz] (a b c) = (sy sxy szy).
    let det = n * (sxx * szz - sxz * sxz) - sx * (sx * szz - sxz * sz) + sz * (sx * sxz - sxx * sz);
    if n < 3.0 || det.abs() < 1e-7 {
        return (sy / n.max(1.0), [0.0, 0.0]);
    }
    let a = (sy * (sxx * szz - sxz * sxz) - sx * (sxy * szz - sxz * szy) + sz * (sxy * sxz - sxx * szy)) / det;
    let b = (n * (sxy * szz - sxz * szy) - sy * (sx * szz - sxz * sz) + sz * (sx * szy - sxy * sz)) / det;
    let c = (n * (sxx * szy - sxy * sxz) - sx * (sx * szy - sxy * sz) + sy * (sx * sxz - sxx * sz)) / det;
    (a, [b, c])
}

/// The steepest slope a wall part follows through its end contacts.
pub const WALL_SLOPE_MAX: f32 = 2.0;
/// The anchored wall base sinks this far below the ground at its ends.
const WALL_SINK: f32 = 0.012;

/// The plane of a wall part: through the ground at the two contacts farthest apart (the ends of
/// the part), tilted across by the least squares of the other contacts; its height at (x, z) and
/// the gradient. None when the contacts do not span a line.
fn anchored_plane(probes: &[Probe], x: f32, z: f32) -> Option<(f32, [f32; 2])> {
    let (mut a, mut b, mut far) = (0, 0, 0.0f32);
    for (i, p) in probes.iter().enumerate() {
        for (j, q) in probes.iter().enumerate().skip(i + 1) {
            let d = (p.0[0] - q.0[0]).hypot(p.0[1] - q.0[1]);
            if d > far {
                (a, b, far) = (i, j, d);
            }
        }
    }
    if far < 0.05 {
        return None;
    }
    let (pa, pb) = (probes[a], probes[b]);
    let u = [(pb.0[0] - pa.0[0]) / far, (pb.0[1] - pa.0[1]) / far];
    let v = [-u[1], u[0]];
    let along = (pb.1 - pa.1) / far;
    // Across: residual of every contact against the end-to-end line, per distance across.
    let (mut num, mut den) = (0.0f32, 0.0f32);
    for ([px, pz], y) in probes {
        let (dx, dz) = (px - pa.0[0], pz - pa.0[1]);
        let r = y - pa.1 - along * (u[0] * dx + u[1] * dz);
        let t = v[0] * dx + v[1] * dz;
        num += r * t;
        den += t * t;
    }
    let across = if den > 1e-6 { num / den } else { 0.0 };
    let limit = |g: f32| g.clamp(-WALL_SLOPE_MAX, WALL_SLOPE_MAX);
    let (gu, gv) = (limit(along), limit(across));
    let slope = [gu * u[0] + gv * v[0], gu * u[1] + gv * v[1]];
    let (dx, dz) = (x - pa.0[0], z - pa.0[1]);
    let at_anchor = pa.1 + slope[0] * dx + slope[1] * dz;
    Some((at_anchor - WALL_SINK, slope))
}

/// The instance of `model` at `at` with its support. `scale` is (footprint, height). None when
/// the surface has no sample under any contact (outside the built mesh).
pub fn seat(geom: &TerrainGeometry, model: WorldModelId, at: Placement, scale: [f32; 2]) -> Option<Seated> {
    let m = model.model();
    let mut inst = HexInstance::upright([at.x, 0.0, at.z], at.color, scale, at.fog);
    inst.yaw = at.yaw;
    let mut support = Vec::new();
    match m.ground {
        Ground::Water => inst.offset[1] = water(geom, at.x, at.z)?,
        Ground::Plane => {
            let probes = probes(geom, m, &inst);
            if probes.is_empty() {
                return None;
            }
            // A wall part follows the ground through its end contacts: the parts of a ring share
            // their end points, so the seams close by construction (platform/docs/slopes.md).
            if crate::debug_view::slope_mode_new() && world_models::wall_parts().any(|p| p == model) {
                if let Some((offset, slope)) = anchored_plane(&probes, at.x, at.z) {
                    inst.offset[1] = offset;
                    inst.slope = slope;
                    return Some(Seated { main: inst, support });
                }
            }
            let (_, slope) = fit_plane(&probes, at.x, at.z);
            let steep = slope[0].hypot(slope[1]);
            let slope = if steep > SLOPE_MAX { [slope[0] * SLOPE_MAX / steep, slope[1] * SLOPE_MAX / steep] } else { slope };
            // Residuals of the (clamped) plane through the anchor: centre them.
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for ([px, pz], y) in &probes {
                let r = y - (slope[0] * (px - at.x) + slope[1] * (pz - at.z));
                lo = lo.min(r);
                hi = hi.max(r);
            }
            if crate::debug_view::slope_mode_new() {
                // The high side is buried at most SHEAR_SINK, the low side hangs at most PLANE_HANG.
                inst.offset[1] = (hi - SHEAR_SINK).min(lo + PLANE_HANG);
            } else {
                // Centred, but the low side never hangs more than PLANE_HANG (fields have no footing).
                inst.offset[1] = ((lo + hi) / 2.0 - PLANE_BIAS).min(lo + PLANE_HANG);
            }
            inst.slope = slope;
        }
        Ground::Upright => {
            let probes = probes(geom, m, &inst);
            if probes.is_empty() {
                return None;
            }
            match m.seat {
                Seat::Bare if crate::debug_view::slope_mode_new() => bare_sheared(&probes, &mut inst, m.low),
                Seat::Bare => {
                    // The ground under the heart of the model, never higher than BARE_REACH of its radius above
                    // the lowest contact (beyond that the low side would hang in the air) and never
                    // lower than it (the model would sink for nothing).
                    let (lo, _) = extent(&probes);
                    let centre = inst.transform([m.foot[0], 0.0, m.foot[1]]);
                    let heart = ground(geom, centre[0], centre[2]).unwrap_or(lo);
                    let reach = BARE_REACH * m.foot[2] * inst.scale_height[0];
                    inst.offset[1] = heart.clamp(lo, lo + reach);
                }
                Seat::Terraced => terrace(geom, m, &mut inst, &probes, f32::NEG_INFINITY, &mut support),
                Seat::Shore => shore(geom, m, &mut inst, &probes, &mut support),
            }
        }
    }
    Some(Seated { main: inst, support })
}

/// A bare composition (improvement, resource, natural wonder) on uneven ground: sheared onto the
/// plane through its contacts (verticals stay vertical, the base follows the ground), the high side
/// buried at most `SHEAR_SINK`, the low side hanging at most `BARE_HANG` below the base plus the
/// authored foundation (`low`) that hides the gap.
fn bare_sheared(probes: &[Probe], inst: &mut HexInstance, low: f32) {
    let (x, z) = (inst.offset[0], inst.offset[2]);
    let (_, slope) = fit_plane(probes, x, z);
    let steep = slope[0].hypot(slope[1]);
    let slope = if steep > BARE_SLOPE_MAX { [slope[0] * BARE_SLOPE_MAX / steep, slope[1] * BARE_SLOPE_MAX / steep] } else { slope };
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for ([px, pz], y) in probes {
        let r = y - (slope[0] * (px - x) + slope[1] * (pz - z));
        lo = lo.min(r);
        hi = hi.max(r);
    }
    let hidden = -low.min(0.0) * inst.scale_height[1];
    inst.offset[1] = (hi - SHEAR_SINK).min(lo + BARE_HANG + hidden);
    inst.slope = slope;
}

/// A plinth instance: footprint radius `radius` around (x, z), from `bottom` up `height`.
fn plinth_instance(main: &HexInstance, x: f32, z: f32, radius: f32, bottom: f32, height: f32) -> (WorldModelId, HexInstance) {
    let mut p = HexInstance::upright([x, bottom, z], main.color, [radius, height], main.fog);
    p.yaw = 0.0;
    (world_models::plinth(), p)
}

/// The base of a model whose contacts stand on `probes`: bury the high side by SINK and fill
/// the gap below the low side with a plinth.
fn terrace(
    geom: &TerrainGeometry,
    m: &WorldModel,
    inst: &mut HexInstance,
    probes: &[Probe],
    floor: f32,
    support: &mut Vec<(WorldModelId, HexInstance)>,
) {
    let (mut lo, hi) = extent(probes);
    let low = m.low.min(0.0) * inst.scale_height[1];
    // The new seating (platform/docs/slopes.md): the pads of the terrain take most of the slope, what is
    // left is a tight, low stone course that stays inside the footprint.
    let new = crate::debug_view::slope_mode_new();
    let wonder = new && m.kind == world_models::WorldKind::Wonder;
    let (gap_tol, flare, widen, plinth_max) = if wonder {
        (GAP_TOL_NEW, PLINTH_FLARE_NEW, LOWER_WIDEN_NEW, WONDER_PLINTH_MAX)
    } else if new {
        (GAP_TOL_NEW, PLINTH_FLARE_NEW, LOWER_WIDEN_NEW, PLINTH_MAX_NEW)
    } else {
        (GAP_TOL, PLINTH_FLARE, LOWER_WIDEN, PLINTH_MAX)
    };
    let mut base = (hi - SINK).max(floor);
    if base + low - lo <= gap_tol {
        inst.offset[1] = base;
        return;
    }
    // The plinth is a little wider than the footprint: its rim may stand lower than any contact.
    let centre = inst.transform([m.foot[0], 0.0, m.foot[1]]);
    let radius = m.foot[2] * inst.scale_height[0] * flare + PLINTH_MARGIN;
    // On a quay (`floor` is a deck above the water) the rim over the sea says nothing: the land
    // contacts alone decide, the piles reach the bottom.
    if floor == f32::NEG_INFINITY {
        // Three rings up to the lower tier of a two-tier plinth, the widest part.
        for ring in [0.5, 1.0, widen] {
            for k in 0..RIM {
                let a = std::f32::consts::TAU * k as f32 / RIM as f32;
                let reach = radius * ring;
                if let Some(y) = ground(geom, centre[0] + reach * a.cos(), centre[2] + reach * a.sin()) {
                    lo = lo.min(y);
                }
            }
        }
    }
    let mut height = base + low - lo + EMBED;
    if height > plinth_max {
        base = (base - (height - plinth_max)).max(floor);
        height = plinth_max;
    }
    inst.offset[1] = base;
    let top = base + low + TOP_OVERLAP;
    let bottom = top - height - TOP_OVERLAP;
    if new {
        // Stone courses of at most TIER_NEW each, the lowest the widest (4 % per step), all inside
        // the footprint of the model: a tall base reads as a stepped retaining wall.
        let tiers = ((height / TIER_NEW).ceil() as usize).clamp(1, if wonder { MAX_TIERS_WONDER } else { MAX_TIERS_NEW });
        for k in 0..tiers {
            let from = bottom + height * k as f32 / tiers as f32;
            let to = if k + 1 == tiers { top } else { bottom + height * (k + 1) as f32 / tiers as f32 };
            let r = radius * (1.0 + (widen - 1.0) * (tiers - 1 - k) as f32);
            support.push(plinth_instance(inst, centre[0], centre[2], r, from, to - from));
        }
    } else if height > TWO_TIERS {
        let step = bottom + height * LOWER_SHARE;
        support.push(plinth_instance(inst, centre[0], centre[2], radius * widen, bottom, step - bottom));
        support.push(plinth_instance(inst, centre[0], centre[2], radius, step, top - step));
    } else {
        support.push(plinth_instance(inst, centre[0], centre[2], radius, bottom, top - bottom));
    }
}

/// A quay: the land contacts decide the height, the deck stays above the water, piles stand
/// under the contacts over water down to the sea floor.
fn shore(geom: &TerrainGeometry, m: &WorldModel, inst: &mut HexInstance, probes: &[Probe], support: &mut Vec<(WorldModelId, HexInstance)>) {
    let Some(surface) = water_level(geom, inst.offset[0], inst.offset[2]) else {
        terrace(geom, m, inst, probes, f32::NEG_INFINITY, support);
        return;
    };
    let land: Vec<Probe> = probes.iter().filter(|p| p.1 >= surface - 0.01).copied().collect();
    let low = m.low.min(0.0) * inst.scale_height[1];
    if land.is_empty() {
        inst.offset[1] = surface + DECK;
    } else {
        terrace(geom, m, inst, &land, surface + DECK, support);
    }
    add_piles(inst, probes, surface, low, support);
}

fn add_piles(inst: &HexInstance, probes: &[Probe], surface: f32, low: f32, support: &mut Vec<(WorldModelId, HexInstance)>) {
    let deck = inst.offset[1] + low + TOP_OVERLAP;
    // Piles stand under the contacts over water; the land contacts are carried by the plinth.
    let mut wet: Vec<&Probe> = probes.iter().filter(|p| p.1 < surface - 0.01 && p.1 < deck - 0.03).collect();
    wet.sort_by(|a, b| a.1.total_cmp(&b.1));
    for ([x, z], y) in wet.into_iter().take(MAX_PILES) {
        let bottom = y - EMBED;
        let mut pile = HexInstance::upright([*x, bottom, *z], inst.color, [PILE_RADIUS, deck - bottom], inst.fog);
        pile.yaw = 0.0;
        support.push((world_models::pile(), pile));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_fit_recovers_a_tilted_plane_through_scattered_points() {
        let pts = [[3.0, 2.0], [3.4, 2.1], [2.7, 1.8], [3.1, 2.5], [2.6, 2.3], [3.5, 1.6]];
        let samples: Vec<Probe> = pts.iter().map(|p| (*p, 0.4 + 0.2 * (p[0] - 3.0) - 0.1 * (p[1] - 2.0))).collect();
        let (a, [b, c]) = fit_plane(&samples, 3.0, 2.0);
        assert!((a - 0.4).abs() < 1e-3 && (b - 0.2).abs() < 1e-3 && (c + 0.1).abs() < 1e-3, "{a} {b} {c}");
    }

    #[test]
    fn collinear_points_give_a_flat_plane() {
        let samples: Vec<Probe> = (0..5).map(|i| ([i as f32, 0.0], 0.3)).collect();
        let (a, slope) = fit_plane(&samples, 2.0, 0.0);
        assert!((a - 0.3).abs() < 1e-6 && slope == [0.0, 0.0]);
    }
}
