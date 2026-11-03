//! Motion of world models (presentation only): a registry of animation classes by model id, the
//! per-instance parameters the vertex shader reads (`HexInstance::motion`), and the per-vertex
//! weights the registry derives from each mesh once at upload.
//!
//! Everything moves in the vertex shader (hex.wgsl) from the camera uniform's clock
//! (`time_.x`), which the shell freezes at 0 on `Quality::Low` and with reduced motion: a frozen
//! frame is the rest pose, and the same time always gives the same frame. Nothing here touches
//! the simulation, its hashes or the journal.
//!
//! Instance parameters (`HexInstance::motion`):
//! - `x`: the kind (low 4 bits), flags (16 = pillaged, 32 = palette colours glow and glitter);
//! - `y`: phase in radians, hashed from the tile: instances of one model never move in lockstep;
//! - `z`: amplitude (hex units; its meaning depends on the kind);
//! - `w`: angular speed (radians per second).
//!
//! Besides the classes, the shader animates by *material*: a vertex whose baked colour is a
//! palette colour that glows or glitters (fire, lava, uranium, gems, water, foam) does so on
//! every model that carries it, so a campfire, a lighthouse flame and a volcano all flicker
//! without a table entry.

use crate::gpu::buffers::UnitVertex;
use crate::scene::terrain::fields::hash_u32;
use crate::world_models::{self, WorldModelId};

/// What a class does (the values are the shader's `kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Plants, crops and trees: a wind sway growing with the square of the height.
    Sway = 1,
    /// The same sway for the green parts of a model only (trees on a mountain, a garden).
    Foliage = 5,
    /// Animals: each grazes on its own beat (a slow lowering of the head, a long graze, a quick lift, then a few
    /// steps along a short arc), the legs step while it walks, the bodies breathe (see `herd_codes`).
    Graze = 2,
    /// Things afloat: a slow heave and roll.
    Bob = 3,
    /// A geyser: the steam column rises, holds, collapses and pauses in a cycle.
    Geyser = 4,
    /// A crater: the model's smoke above the crater is replaced by particles while animated.
    Plume = 6,
}

/// Animation parameters of a class.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Class {
    pub kind: Kind,
    /// Sway: displacement at `REF` height; Graze: head dip; Bob: heave; Geyser: vent height.
    pub amp: f32,
    /// Radians per second (Geyser: the cycle's angular speed).
    pub speed: f32,
}

const fn class(kind: Kind, amp: f32, speed: f32) -> Class {
    Class { kind, amp, speed }
}

/// Height at which a sway reaches its full amplitude (hex units).
const SWAY_REF: f32 = 0.35;

/// The classes of the animated models: the `world_anim.table` of every part present
/// (`<part>/assets/models/world_anim.table`, joined by build.rs, platform first), by manifest id
/// (an entry ending in `*` takes every id with that prefix; first match wins). Data, not code:
/// the tables name models, the code does not.
const TABLE_TEXT: &str = include_str!(concat!(env!("OUT_DIR"), "/world_anim.table"));

/// A table row: the key, the class and the emitter it names (a fountain's glints).
type Row = (String, Class, Option<String>);

/// The table rows that match no baked model (a typo, or a model of a part that is absent):
/// the tests of the part that owns the models assert none of its own is left.
pub fn rows_without_a_model() -> Vec<String> {
    table()
        .iter()
        .filter(|(key, _, _)| {
            !world_models::models().iter().any(|m| match key.strip_suffix('*') {
                Some(prefix) => m.id.starts_with(prefix),
                None => m.id == key.as_str(),
            })
        })
        .map(|(key, _, _)| key.clone())
        .collect()
}

fn table() -> &'static [Row] {
    static TABLE: std::sync::OnceLock<Vec<Row>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        TABLE_TEXT
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .map(|line| {
                let f: Vec<&str> = line.split_whitespace().collect();
                let kind = match f[1] {
                    "sway" => Kind::Sway,
                    "foliage" => Kind::Foliage,
                    "graze" => Kind::Graze,
                    "bob" => Kind::Bob,
                    "geyser" => Kind::Geyser,
                    "plume" => Kind::Plume,
                    other => panic!("world_anim.table: unknown kind {other}"),
                };
                let num = |i: usize| f[i].parse::<f32>().expect("world_anim.table: a number");
                (f[0].to_string(), class(kind, num(2), num(3)), f.get(4).map(|e| e.to_string()))
            })
            .collect()
    })
}

fn row_of(model: WorldModelId) -> Option<&'static Row> {
    let id = model.model().id;
    table().iter().find(|(key, _, _)| match key.strip_suffix('*') {
        Some(prefix) => id.starts_with(prefix),
        None => key == id,
    })
}

/// The emitter a model's table row names, if any (`fountain`).
pub fn named_emitter(model: WorldModelId) -> Option<&'static str> {
    row_of(model).and_then(|(_, _, e)| e.as_deref())
}

/// The kind of a model's class, if it moves (no mesh work).
pub fn kind_of(model: WorldModelId) -> Option<Kind> {
    row_of(model).map(|r| r.1.kind)
}

/// The class of a model, if it moves.
pub fn class_of(model: WorldModelId) -> Option<Class> {
    let mut class = row_of(model)?.1;
    if class.kind == Kind::Geyser {
        class.amp = steam_extent(model).map_or(0.0, |(low, _)| low);
    }
    if class.kind == Kind::Plume {
        class.amp = crate::gpu::ambient_fx::emitter_of(model).map_or(0.0, |(_, at)| at[1]);
    }
    Some(class)
}

/// The palette colour of steam (the geyser plume): see the manifest palette.
pub const STEAM: [f32; 3] = [0.93, 0.95, 0.97];

/// Palette colours the emitters look for (manifest palette).
pub const FIRE: [f32; 3] = [0.95, 0.55, 0.15];
pub const LAVA: [f32; 3] = [0.96, 0.36, 0.11];

pub fn is_color(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.006)
}

/// Lowest and highest steam vertex of a model's LOD0.
pub fn steam_extent(model: WorldModelId) -> Option<(f32, f32)> {
    let mut range: Option<(f32, f32)> = None;
    for v in model.mesh(0).iter().filter(|v| is_color(v.color, STEAM)) {
        range = Some(range.map_or((v.pos[1], v.pos[1]), |(lo, hi)| (lo.min(v.pos[1]), hi.max(v.pos[1]))));
    }
    range
}

const PILLAGED: f32 = 16.0;
/// Set on every world object: its palette colours glow and glitter (units do not carry it).
const MATERIAL_MOTION: f32 = 32.0;

/// The `anim` of an instance of `model` on `tile` (`salt` tells the objects of one tile apart);
/// `pillaged` shows the ruined state.
pub fn instance_anim(model: WorldModelId, tile: u32, salt: u32, pillaged: bool) -> [f32; 4] {
    let class = class_of(model);
    let h = hash_u32(tile.wrapping_mul(0x9E37_79B1) ^ salt.wrapping_mul(0x85EB_CA6B) ^ 0x51ED);
    let phase = (h & 0xFFFF) as f32 / 65536.0 * std::f32::consts::TAU;
    let flags = MATERIAL_MOTION + if pillaged { PILLAGED } else { 0.0 };
    match class {
        Some(c) => {
            // Sway amplitude is stored as the factor of the squared height (see hex.wgsl).
            let amp = if matches!(c.kind, Kind::Sway | Kind::Foliage) { c.amp / (SWAY_REF * SWAY_REF) } else { c.amp };
            // Speeds differ a little from instance to instance.
            let speed = c.speed * (0.85 + 0.3 * ((h >> 16) & 0xFF) as f32 / 255.0);
            [c.kind as u32 as f32 + flags, phase, amp, speed]
        }
        None => [flags, phase, 0.0, 0.0],
    }
}

/// What moves on a model, for the coverage report: its class, its emitter and the palette materials
/// the shader makes glow or glitter.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Description {
    pub class: Option<&'static str>,
    pub emitter: Option<&'static str>,
    pub materials: Vec<&'static str>,
}

pub fn describe(model: WorldModelId) -> Description {
    const MATERIALS: &[(&str, [f32; 3])] = &[
        ("fire", FIRE),
        ("lava", LAVA),
        ("uranium", [0.49, 0.81, 0.31]),
        ("gem_blue", [0.21, 0.43, 0.87]),
        ("gem_red", [0.81, 0.15, 0.27]),
        ("gold", [0.90, 0.72, 0.30]),
        ("neon", [0.40, 0.95, 0.85]),
        ("water", [0.25, 0.51, 0.63]),
        ("turquoise", [0.31, 0.76, 0.73]),
        ("foam", [0.87, 0.93, 0.95]),
    ];
    let mesh = model.mesh(0);
    Description {
        class: class_of(model).map(|c| match c.kind {
            Kind::Sway => "sway",
            Kind::Foliage => "foliage sway",
            Kind::Graze => "graze",
            Kind::Bob => "bob",
            Kind::Geyser => "geyser",
            Kind::Plume => "plume",
        }),
        emitter: crate::gpu::ambient_fx::emitter_of(model).map(|(e, _)| match e {
            crate::gpu::ambient_fx::Emitter::Geyser => "geyser column and spray",
            crate::gpu::ambient_fx::Emitter::Volcano => "crater smoke and sparks",
            crate::gpu::ambient_fx::Emitter::Fountain => "rising glints",
            crate::gpu::ambient_fx::Emitter::Hearth => "fire smoke",
            crate::gpu::ambient_fx::Emitter::Ruin => "ruin smoke",
        }),
        materials: MATERIALS
            .iter()
            .filter(|(_, c)| mesh.iter().any(|v| is_color(v.color, *c)))
            .map(|(n, _)| *n)
            .collect(),
    }
}

/// The class of a model by manifest id, for the coverage report.
pub fn animated_ids() -> Vec<&'static str> {
    world_models::models()
        .iter()
        .enumerate()
        .filter(|(i, _)| class_of(WorldModelId(*i as u16)).is_some())
        .map(|(_, m)| m.id)
        .collect()
}

/// Every world vertex with its animation code folded into `team`: `team + 2 * code`, where `code` 0 is a part
/// that stays and 1.. a part of an animal (see [`herd_codes`]). hex.wgsl splits it again; only models of the
/// `Graze` kind carry any.
pub fn encoded_vertices() -> Vec<UnitVertex> {
    let mut all = world_models::vertices().to_vec();
    for (index, model) in world_models::models().iter().enumerate() {
        let id = WorldModelId(index as u16);
        if class_of(id).is_none_or(|c| c.kind != Kind::Graze) {
            continue;
        }
        for lod in 0..model.lods.len() {
            let (start, count) = model.lods[lod];
            let range = start as usize..(start + count) as usize;
            let codes = herd_codes(&all[range.clone()]);
            for (v, code) in all[range].iter_mut().zip(codes) {
                v.team += 2.0 * code as f32;
            }
        }
    }
    all
}

/// What a vertex of an animal is, as the shader decodes it (`code - 1`, bits from the low end):
///
/// | bits | field |
/// |---|---|
/// | 0..2 | class: 0 body (and what rides on it: patches, a hump, a tail), 1 head (neck, head, horns, ears, mane), 2 and 3 legs of the two diagonal pairs |
/// | 2..5 | weight, 0..7 of 7: a head's distance from the shoulder, a leg's depth from the hip to the foot, a body's height |
/// | 5 | the head is baked already at the grass (the motion then lifts it instead of lowering it) |
/// | 6..10 | heading of the animal in 16 steps (the way the head points, in the model's x-z plane) |
/// | 10..16, 16..22 | the animal's centre in x and z, 64 steps over -0.64..0.64 (the pivot of its wander) |
///
/// Twenty-two bits, so `2 * code` stays an exact f32 integer. Every animal of a model gets its own heading and
/// centre, so the shader also draws its randomness from them: no table of animals, no export data beyond the
/// vertices.
pub const CODE_BITS: u32 = 22;

struct Piece {
    tris: usize,
    lo: [f32; 3],
    hi: [f32; 3],
}

/// Triangle parts of a mesh (a triangle list): welded by position, one root per triangle and the
/// bounds of each part.
fn parts_of(mesh: &[UnitVertex]) -> (Vec<usize>, std::collections::BTreeMap<usize, Piece>) {
    let triangles = mesh.len() / 3;
    let key = |p: [f32; 3]| [(p[0] * 500.0).round() as i32, (p[1] * 500.0).round() as i32, (p[2] * 500.0).round() as i32];
    let mut parent: Vec<usize> = (0..triangles).collect();
    fn find(parent: &mut [usize], mut a: usize) -> usize {
        while parent[a] != a {
            parent[a] = parent[parent[a]];
            a = parent[a];
        }
        a
    }
    let mut first: std::collections::HashMap<[i32; 3], usize> = std::collections::HashMap::new();
    for t in 0..triangles {
        for k in 0..3 {
            let at = key(mesh[3 * t + k].pos);
            match first.get(&at) {
                Some(&other) => {
                    let (a, b) = (find(&mut parent, t), find(&mut parent, other));
                    parent[a] = b;
                }
                None => {
                    first.insert(at, t);
                }
            }
        }
    }
    let roots: Vec<usize> = (0..triangles).map(|t| find(&mut parent, t)).collect();
    let mut parts: std::collections::BTreeMap<usize, Piece> = std::collections::BTreeMap::new();
    for t in 0..triangles {
        let part = parts.entry(roots[t]).or_insert(Piece { tris: 0, lo: [f32::MAX; 3], hi: [f32::MIN; 3] });
        part.tris += 1;
        for k in 0..3 {
            let p = mesh[3 * t + k].pos;
            for a in 0..3 {
                part.lo[a] = part.lo[a].min(p[a]);
                part.hi[a] = part.hi[a].max(p[a]);
            }
        }
    }
    (roots, parts)
}

/// One animal found in a mesh: its body part, centre, heading and the extent of the body along it.
struct Animal {
    root: usize,
    centre: [f32; 2],
    heading: [f32; 2],
    length: f32,
    belly: f32,
    height: f32,
    /// Largest distance from the shoulder to a head vertex, and the lowest head vertex.
    reach: f32,
    head_low: f32,
}

impl Animal {
    fn shoulder(&self) -> [f32; 3] {
        let s = 0.30 * self.length;
        [self.centre[0] + self.heading[0] * s, self.belly + 0.8 * self.height, self.centre[1] + self.heading[1] * s]
    }
}

fn centre_of(p: &Piece) -> [f32; 2] {
    [(p.lo[0] + p.hi[0]) / 2.0, (p.lo[2] + p.hi[2]) / 2.0]
}

/// The code of every vertex of a herd mesh (triangle list): see [`CODE_BITS`]. Animals are the largest parts
/// of the mesh that do not sit inside a larger one (a hump is not a second bison); each other part belongs to the
/// nearest body and is a leg (hangs below the belly beside the body), a head (ahead of the shoulder: neck, head,
/// horns, ears, mane) or rides on the body (patches, hump, tail). The heading is the body's long axis, signed
/// toward the mass of its small parts above the belly. Parts far from every body stay still (code 0).
pub fn herd_codes(mesh: &[UnitVertex]) -> Vec<u32> {
    let (roots, parts) = parts_of(mesh);
    let biggest = parts.values().map(|p| p.tris).max().unwrap_or(0);
    // Bodies: big parts, largest first, not inside an accepted one.
    let mut candidates: Vec<(&usize, &Piece)> = parts.iter().filter(|(_, p)| p.tris * 10 >= biggest * 7).collect();
    candidates.sort_by(|a, b| b.1.tris.cmp(&a.1.tris).then(a.0.cmp(b.0)));
    let mut bodies: Vec<usize> = Vec::new();
    for (root, p) in candidates {
        let c = centre_of(p);
        let inside = bodies.iter().any(|b| {
            let q = &parts[b];
            c[0] >= q.lo[0] - 0.02 && c[0] <= q.hi[0] + 0.02 && c[1] >= q.lo[2] - 0.02 && c[1] <= q.hi[2] + 0.02
                && (p.lo[1] + p.hi[1]) / 2.0 >= q.lo[1]
                && (p.lo[1] + p.hi[1]) / 2.0 <= q.hi[1] + 0.05
        });
        if !inside {
            bodies.push(*root);
        }
    }
    let nearest = |c: [f32; 2], bodies: &[Animal]| -> Option<usize> {
        bodies
            .iter()
            .enumerate()
            .map(|(n, a)| (n, (a.centre[0] - c[0]).hypot(a.centre[1] - c[1])))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, d)| *d < 0.45)
            .map(|(n, _)| n)
    };
    // Body axes from the principal axis of the body's vertices.
    let mut animals: Vec<Animal> = Vec::new();
    for &root in &bodies {
        let p = &parts[&root];
        let c = centre_of(p);
        let (mut sxx, mut szz, mut sxz, mut n) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for t in 0..roots.len() {
            if roots[t] != root {
                continue;
            }
            for k in 0..3 {
                let (dx, dz) = (mesh[3 * t + k].pos[0] - c[0], mesh[3 * t + k].pos[2] - c[1]);
                sxx += dx * dx;
                szz += dz * dz;
                sxz += dx * dz;
                n += 1.0;
            }
        }
        let _ = n;
        let angle = 0.5 * (2.0 * sxz).atan2(sxx - szz);
        animals.push(Animal {
            root,
            centre: c,
            heading: [angle.cos(), angle.sin()],
            length: 0.0,
            belly: p.lo[1],
            height: (p.hi[1] - p.lo[1]).max(1e-3),
            reach: 1e-3,
            head_low: f32::MAX,
        });
    }
    // Sign: toward the small parts that stand above the belly (a head outweighs a tail).
    let mut lean = vec![0.0f32; animals.len()];
    for (root, p) in &parts {
        if bodies.contains(root) {
            continue;
        }
        let c = centre_of(p);
        if let Some(n) = nearest(c, &animals) {
            let a = &animals[n];
            if p.lo[1] > a.belly + 0.3 * a.height {
                lean[n] += p.tris as f32 * ((c[0] - a.centre[0]) * a.heading[0] + (c[1] - a.centre[1]) * a.heading[1]);
            }
        }
    }
    for (a, l) in animals.iter_mut().zip(&lean) {
        if *l < 0.0 {
            a.heading = [-a.heading[0], -a.heading[1]];
        }
    }
    // Body length along the heading.
    for a in animals.iter_mut() {
        let p = &parts[&a.root];
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for t in 0..roots.len() {
            if roots[t] != a.root {
                continue;
            }
            for k in 0..3 {
                let q = mesh[3 * t + k].pos;
                let s = (q[0] - a.centre[0]) * a.heading[0] + (q[2] - a.centre[1]) * a.heading[1];
                lo = lo.min(s);
                hi = hi.max(s);
            }
        }
        a.length = (hi - lo).max(0.05);
        let _ = p;
    }
    // Class of every part.
    #[derive(Clone, Copy, PartialEq)]
    enum Role {
        Body,
        Head,
        Leg(u32),
        Still,
    }
    let mut role: std::collections::HashMap<usize, (Role, usize)> = std::collections::HashMap::new();
    for (root, p) in &parts {
        let c = centre_of(p);
        let Some(n) = nearest(c, &animals) else {
            role.insert(*root, (Role::Still, 0));
            continue;
        };
        let a = &animals[n];
        if *root == a.root {
            role.insert(*root, (Role::Body, n));
            continue;
        }
        let rel = [c[0] - a.centre[0], c[1] - a.centre[1]];
        let along = rel[0] * a.heading[0] + rel[1] * a.heading[1];
        let side = rel[1] * a.heading[0] - rel[0] * a.heading[1];
        let r = if p.lo[1] < a.belly - 0.005 && along.abs() < 0.42 * a.length {
            Role::Leg(u32::from((along > 0.0) != (side > 0.0)))
        } else if along > 0.25 * a.length {
            Role::Head
        } else {
            Role::Body
        };
        role.insert(*root, (r, n));
    }
    // The head's reach from the shoulder, the lowest head vertex.
    for t in 0..roots.len() {
        if let Some((Role::Head, n)) = role.get(&roots[t]).copied() {
            let sh = animals[n].shoulder();
            for k in 0..3 {
                let q = mesh[3 * t + k].pos;
                let d = ((q[0] - sh[0]).powi(2) + (q[1] - sh[1]).powi(2) + (q[2] - sh[2]).powi(2)).sqrt();
                let a = &mut animals[n];
                a.reach = a.reach.max(d);
                a.head_low = a.head_low.min(q[1]);
            }
        }
    }
    let level = |x: f32| ((x.clamp(0.0, 1.0) * 7.0).round() as u32).min(7);
    let quant = |x: f32| (((x + 0.64) / 1.28 * 63.0).round().clamp(0.0, 63.0)) as u32;
    (0..mesh.len() / 3)
        .flat_map(|t| {
            let root = roots[t];
            let part = &parts[&root];
            let code = match role.get(&root).copied() {
                None | Some((Role::Still, _)) => [0u32; 3],
                Some((r, n)) => {
                    let a = &animals[n];
                    let heading = (a.heading[1].atan2(a.heading[0]) / std::f32::consts::TAU * 16.0).round().rem_euclid(16.0) as u32;
                    let down = u32::from(a.head_low < a.belly);
                    let base = (down << 5) | (heading << 6) | (quant(a.centre[0]) << 10) | (quant(a.centre[1]) << 16);
                    let mut out = [0u32; 3];
                    for k in 0..3 {
                        let q = mesh[3 * t + k].pos;
                        let (class, weight) = match r {
                            Role::Head => {
                                let sh = a.shoulder();
                                let d = ((q[0] - sh[0]).powi(2) + (q[1] - sh[1]).powi(2) + (q[2] - sh[2]).powi(2)).sqrt();
                                (1, d / a.reach)
                            }
                            Role::Leg(pair) => (2 + pair, (part.hi[1] - q[1]) / (part.hi[1] - part.lo[1]).max(1e-3)),
                            _ => (0, (q[1] - a.belly) / a.height),
                        };
                        out[k] = 1 + (class | (level(weight) << 2) | base);
                    }
                    out
                }
            };
            code
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The model with this manifest id, if the part that owns it is present in this build.
    fn find(id: &str) -> Option<WorldModelId> {
        (0..world_models::models().len() as u16).map(WorldModelId).find(|m| m.model().id == id)
    }

    fn model(id: &str) -> WorldModelId {
        find(id).unwrap_or_else(|| panic!("{id} is not in the manifest"))
    }

    #[test]
    fn every_table_row_names_a_model() {
        // The tables are the parts' own (`<part>/assets/models/world_anim.table`): a part that is
        // absent brings neither its models nor its rows.
        assert_eq!(rows_without_a_model(), Vec::<String>::new());
    }

    #[test]
    fn same_tile_same_motion_and_neighbours_differ() {
        let herd = model("resource/horses");
        assert_eq!(instance_anim(herd, 7, 1, false), instance_anim(herd, 7, 1, false));
        let phases: std::collections::HashSet<u32> = (0..64).map(|t| instance_anim(herd, t, 1, false)[1].to_bits()).collect();
        assert!(phases.len() > 56, "instances share phases: {}", phases.len());
    }

    #[test]
    fn a_still_model_has_no_kind_until_pillaged() {
        let still = (0..world_models::models().len() as u16)
            .map(WorldModelId)
            .find(|m| kind_of(*m).is_none())
            .expect("a model without an animation class");
        assert_eq!(instance_anim(still, 3, 1, false)[0], MATERIAL_MOTION);
        assert_eq!(instance_anim(still, 3, 1, true)[0], MATERIAL_MOTION + PILLAGED);
    }

    #[test]
    fn herds_are_taken_apart_per_animal() {
        // The shared horses are always there; the other herds belong to the classic part.
        let herds: Vec<_> = [("resource/sheep", [4, 3]), ("resource/horses", [3, 2]), ("resource/deer", [3, 2]), ("resource/cattle", [3, 2]), ("resource/bison", [3, 2])]
            .into_iter()
            .filter_map(|(id, animals)| find(id).map(|m| (id, animals, m)))
            .collect();
        assert!(herds.iter().any(|(id, _, _)| *id == "resource/horses"));
        for (id, animals, m) in herds {
            for lod in 0..2usize {
                let mesh = m.mesh(lod);
                let codes = herd_codes(mesh);
                assert_eq!(codes.len(), mesh.len());
                // One (heading, centre) pair per animal.
                let found: std::collections::BTreeSet<u32> =
                    codes.iter().filter(|c| **c > 0).map(|c| (*c - 1) >> 6).collect();
                assert_eq!(found.len(), animals[lod], "{id} lod {lod}: animals found");
                let class_share = |class: u32| {
                    codes.iter().filter(|c| **c > 0 && (*c - 1) & 3 == class).count() as f32 / codes.len() as f32
                };
                let head = class_share(1);
                let legs = class_share(2) + class_share(3);
                assert!(head > 0.04 && head < 0.5, "{id} lod {lod}: {head:.2} of the vertices are heads");
                assert!(lod == 1 && legs > 0.0 || lod == 0 && legs > 0.1, "{id} lod {lod}: {legs:.2} of the vertices are legs");
                assert!(codes.iter().all(|c| *c < (1 << CODE_BITS) + 1), "{id}: a code overflows");
            }
        }
    }

    #[test]
    fn geyser_plume_is_found() {
        let (low, high) = steam_extent(model("natural-wonder/old-faithful")).expect("steam");
        assert!(high > 1.0 && low >= 0.0 && low < high, "{low} .. {high}");
    }
}
