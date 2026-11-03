//! World objects: the "world" sections of the manifests of every part present
//! (`<part>/assets/models/manifest.json`, see parts.rs) + `<part>/assets/models/world/<kind>/*.glb`
//! -> baked vertices and the content-key index.
//!
//! Validates every model against its budget class (LOD triangles, footprint radius,
//! height, grounding), the key map against classic/docs/classic (every improvement, resource, natural
//! wonder, world and national wonder has a model, or an explicit icon-only / no-map entry)
//! and the city kit (pieces, slots, worst-case city tile triangles). The merge is checked for
//! the platform alone, for the platform with each module and for everything, as units.rs does.
//! Writes OUT_DIR/world_models.bin (UnitVertex array, models sorted by id) and
//! OUT_DIR/world_models.rs.

use crate::glb::{self, extent, Mesh, Problems, LOD_NODES};
use crate::parts::Part;
use crate::support;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const KINDS: [&str; 5] = ["improvement", "resource", "natural-wonder", "wonder", "city"];
const GROUNDS: [&str; 3] = ["upright", "plane", "water"];
const SITES: [&str; 2] = ["land", "water"];
const SITE_SHAPES: [&str; 3] = ["pad", "crater", "reef"];
const SLOT_KINDS: [&str; 4] = ["inner", "outer", "water", "ring"];
const SHAPE_EPS: f32 = 3e-3;
/// Reduced LOD bounds may exceed LOD0 bounds by LOD_SLACK + LOD_SLACK_SIZE * size.
const LOD_SLACK: f32 = 0.05;
const LOD_SLACK_SIZE: f32 = 0.1;

#[derive(Deserialize)]
struct Root {
    #[serde(default)]
    key_namespace: String,
    #[serde(default)]
    palette: BTreeMap<String, [f32; 3]>,
    #[serde(default)]
    world: World,
}

/// One part's "world" section, or the merge of several. The platform's carries the vocabulary
/// (budgets, shapes) and the generic city kit; a module's carries its models, its content keys
/// and its city landmarks.
#[derive(Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
struct World {
    #[allow(dead_code)]
    #[serde(default)]
    generator: String,
    #[allow(dead_code)]
    #[serde(default)]
    note: String,
    #[allow(dead_code)]
    #[serde(default)]
    axes: String,
    #[serde(default)]
    budgets: BTreeMap<String, [usize; 3]>,
    /// Budget class -> (max footprint radius, max height, lowest allowed point).
    #[serde(default)]
    shapes: BTreeMap<String, [f32; 3]>,
    #[serde(default)]
    city_tile_lod0_max: usize,
    #[serde(default)]
    models: Vec<Entry>,
    #[serde(default)]
    keys: BTreeMap<String, String>,
    #[serde(default)]
    icon_only: BTreeMap<String, String>,
    #[serde(default)]
    no_map_presence: BTreeMap<String, String>,
    #[serde(default)]
    city_kit: CityKit,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Entry {
    /// The models folder of the part that owns the entry (set after parsing, never in the file).
    #[serde(skip)]
    dir: PathBuf,
    #[serde(skip)]
    part: String,
    id: String,
    file: String,
    kind: String,
    budget: String,
    ground: String,
    site: String,
    site_shape: String,
    scale: f32,
    pivot: [f32; 3],
    bounds: Bounds,
    radius: f32,
    size: f32,
    materials: Vec<String>,
    team_mask: bool,
    lods: Vec<Lod>,
    lod_px: [f32; 2],
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Lod {
    node: String,
    tris: usize,
}

#[derive(Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
struct CityKit {
    #[serde(default)]
    styles: Vec<String>,
    #[serde(default)]
    centers: BTreeMap<String, String>,
    #[serde(default)]
    houses: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    houses_by_population: Vec<(u32, usize)>,
    #[serde(default)]
    older_style_share: u32,
    #[serde(default)]
    max_landmarks: usize,
    /// (x, z, yaw degrees, radius, ring) in game coordinates.
    #[serde(default)]
    slots: Vec<(f32, f32, f32, f32, String)>,
    /// Building key -> landmark; the platform's kit has none, each module adds its own.
    #[serde(default)]
    landmarks: BTreeMap<String, Landmark>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Landmark {
    model: String,
    slot: String,
    priority: u32,
    #[serde(default)]
    supersedes: Option<String>,
}

#[derive(Deserialize)]
struct Docs {
    items: Vec<DocItem>,
}

#[derive(Deserialize)]
struct DocItem {
    id: String,
    #[serde(default)]
    kind: Option<String>,
}

pub fn bake(parts: &[Part], root: &Path, out_dir: &Path) {
    for doc in DOCS {
        if super::units::doc_file(root, doc).is_file() {
            println!("cargo:rerun-if-changed={}", super::units::doc_file(root, doc).display());
        }
    }
    let mut problems = Problems(Vec::new());
    let mut palette: BTreeMap<String, [f32; 3]> = BTreeMap::new();
    let roots: Vec<(Root, World)> = parts
        .iter()
        .map(|part| {
            let mut r: Root = serde_json::from_str(&part.text).unwrap_or_else(|e| {
                panic!("world models: {} is malformed: {e}", part.manifest_path().display())
            });
            let mut w = std::mem::take(&mut r.world);
            check_header(part, &r, &w, &mut problems);
            for e in &mut w.models {
                e.dir = part.models.clone();
                e.part = part.name.clone();
            }
            palette.extend(r.palette.iter().map(|(k, v)| (k.clone(), *v)));
            (r, w)
        })
        .collect();
    // Every manifest on its own with the platform's vocabulary, then everything together.
    for set in crate::parts::sets(parts) {
        let members: Vec<(&Part, &World)> = set.iter().map(|&i| (&parts[i], &roots[i].1)).collect();
        let merged = merge(&members, &mut problems);
        let index = check_models(&merged, &palette, &mut problems);
        check_keys(&merged, &index, &mut problems);
        check_unused(&merged, &members, &mut problems);
        if members.iter().any(|(part, _)| part.name == "classic") {
            check_coverage(root, &merged, &mut problems);
        }
        check_city_kit(&merged, &index, &mut problems);
    }
    let members: Vec<(&Part, &World)> = parts.iter().zip(&roots).map(|(part, (_, w))| (part, w)).collect();
    let w = merge(&members, &mut problems);
    let index = check_models(&w, &palette, &mut problems);
    check_files(parts, &w, &mut problems);
    let meshes: Vec<Option<[Mesh; 3]>> = w
        .models
        .iter()
        .map(|e| load_model(&palette, &w, e, &mut problems))
        .collect();
    problems.dedup();
    if problems.0.is_empty() {
        check_city_tile(&w, &index, &mut problems);
    }
    problems.dedup();
    if !problems.0.is_empty() {
        panic!(
            "\n\nworld models: {} problem(s) in the models manifests (regenerate with `sh <part>/tools/blender/export_all.sh` in the part that owns the model; docs/models-ownership.md):\n  - {}\n",
            problems.0.len(),
            problems.0.join("\n  - ")
        );
    }
    let meshes: Vec<[Mesh; 3]> = meshes.into_iter().map(Option::unwrap).collect();
    emit(out_dir, &w, &index, &meshes, &palette);
}

/// What one manifest's world section may hold: the platform the vocabulary and the generic city
/// kit, never a content key; a module its models, keys and landmarks, all in its key namespace.
fn check_header(part: &Part, r: &Root, w: &World, p: &mut Problems) {
    let who = &part.name;
    let mut keys: Vec<&String> = w.keys.keys().collect();
    keys.extend(w.icon_only.keys());
    keys.extend(w.no_map_presence.keys());
    keys.extend(w.city_kit.landmarks.keys());
    if part.is_platform() {
        for key in keys {
            p.add(format!("platform: content key {key} belongs to a module's manifest"));
        }
        return;
    }
    let kit = &w.city_kit;
    if !(kit.styles.is_empty()
        && kit.centers.is_empty()
        && kit.houses.is_empty()
        && kit.houses_by_population.is_empty()
        && kit.slots.is_empty()
        && kit.older_style_share == 0
        && kit.max_landmarks == 0
        && w.city_tile_lod0_max == 0)
    {
        p.add(format!("{who}: the city kit's styles, slots and limits belong to the platform's manifest"));
    }
    let prefix = format!("{}:", r.key_namespace);
    for key in keys {
        if !key.starts_with(&prefix) {
            p.add(format!("{who}: key {key} is outside the namespace {prefix}"));
        }
    }
}

/// The union of the world sections of `members` (the platform first); a name declared twice is a
/// problem; the models come out sorted by id.
fn merge(members: &[(&Part, &World)], p: &mut Problems) -> World {
    let mut out = members[0].1.clone();
    for (part, w) in &members[1..] {
        for (name, limits) in &w.budgets {
            if out.budgets.insert(name.clone(), *limits).is_some() {
                p.add(format!("{}: world budget class {name} is declared twice", part.name));
            }
        }
        for (name, shape) in &w.shapes {
            if out.shapes.insert(name.clone(), *shape).is_some() {
                p.add(format!("{}: world shape {name} is declared twice", part.name));
            }
        }
        out.models.extend(w.models.iter().cloned());
        for (table, dst, src) in [
            ("keys", &mut out.keys, &w.keys),
            ("icon_only", &mut out.icon_only, &w.icon_only),
            ("no_map_presence", &mut out.no_map_presence, &w.no_map_presence),
        ] {
            for (key, value) in src {
                if dst.insert(key.clone(), value.clone()).is_some() {
                    p.add(format!("{}: {table} key {key} is bound in two parts", part.name));
                }
            }
        }
        for (building, landmark) in &w.city_kit.landmarks {
            if out.city_kit.landmarks.insert(building.clone(), landmark.clone()).is_some() {
                p.add(format!("{}: landmark {building} is declared in two parts", part.name));
            }
        }
    }
    out.models.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Every model is drawn: a module's by its own keys and landmarks; a platform model by the city
/// kit (centers, houses) or, as units.rs, by every module present, and by at least two once two
/// modules are present.
fn check_unused(w: &World, members: &[(&Part, &World)], p: &mut Problems) {
    let modules: Vec<&(&Part, &World)> = members.iter().filter(|(part, _)| !part.is_platform()).collect();
    if modules.is_empty() {
        return;
    }
    let kit = &w.city_kit;
    let mut generic: BTreeSet<&str> = kit.centers.values().map(String::as_str).collect();
    generic.extend(kit.houses.values().flatten().map(String::as_str));
    let mut users: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (part, mw) in &modules {
        let models = mw.keys.values().map(String::as_str).chain(mw.city_kit.landmarks.values().map(|l| l.model.as_str()));
        for model in models {
            users.entry(model).or_default().insert(part.name.as_str());
        }
    }
    for e in &w.models {
        if generic.contains(e.id.as_str()) {
            continue;
        }
        let drawn_by = users.get(e.id.as_str()).map_or(0, BTreeSet::len);
        if e.part == crate::parts::PLATFORM {
            if modules.len() >= 2 && drawn_by < 2 {
                p.add(format!(
                    "model {} is owned by the platform (shared) but {} module(s) draw it: move it to the module that draws it",
                    e.id, drawn_by
                ));
            }
        } else if drawn_by == 0 {
            p.add(format!("model {} is declared by {} but nothing of {} draws it", e.id, e.part, e.part));
        }
    }
}

const DOCS: [&str; 4] = [
    "docs/classic/improvements.json",
    "docs/classic/resources.json",
    "docs/classic/natural_wonders.json",
    "docs/classic/wonders.json",
];

fn budget_kind(budget: &str) -> &str {
    if budget.starts_with("city-") {
        "city"
    } else {
        budget
    }
}

/// Entry checks; returns model id -> index.
fn check_models(
    w: &World,
    palette: &BTreeMap<String, [f32; 3]>,
    p: &mut Problems,
) -> BTreeMap<String, usize> {
    let mut index = BTreeMap::new();
    for (i, e) in w.models.iter().enumerate() {
        if index.insert(e.id.clone(), i).is_some() {
            p.add(format!("{}: duplicate model id", e.id));
        }
        check_entry(w, palette, e, p);
    }
    index
}

fn check_entry(w: &World, palette: &BTreeMap<String, [f32; 3]>, e: &Entry, p: &mut Problems) {
    let id = &e.id;
    let Some((kind, slug)) = id.split_once('/') else {
        p.add(format!("{id}: id must be <kind>/<slug>"));
        return;
    };
    let slug_ok = !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !KINDS.contains(&kind) || kind != e.kind || !slug_ok {
        p.add(format!("{id}: id must be <kind>/<lowercase-slug> with kind {}", e.kind));
    }
    if e.file != format!("world/{kind}/{slug}.glb") {
        p.add(format!("{id}: file must be world/{kind}/{slug}.glb, got {}", e.file));
    }
    if budget_kind(&e.budget) != e.kind {
        p.add(format!("{id}: budget class {} does not fit kind {}", e.budget, e.kind));
    }
    if !GROUNDS.contains(&e.ground.as_str()) {
        p.add(format!("{id}: ground must be one of {GROUNDS:?}"));
    }
    if !SITES.contains(&e.site.as_str()) {
        p.add(format!("{id}: site must be one of {SITES:?}"));
    }
    if !SITE_SHAPES.contains(&e.site_shape.as_str()) {
        p.add(format!("{id}: site_shape must be one of {SITE_SHAPES:?}"));
    }
    if e.scale != 1.0 || !e.pivot.iter().all(|v| *v == 0.0) {
        p.add(format!("{id}: world models are authored in hex units (scale 1, pivot 0)"));
    }
    let [near, far] = e.lod_px;
    if !(near.is_finite() && far.is_finite() && near > far && far > 0.0) {
        p.add(format!("{id}: lod_px {:?} must be decreasing and positive", e.lod_px));
    }
    if !(e.size.is_finite() && e.size > 0.0) {
        p.add(format!("{id}: size {} must be positive", e.size));
    }
    for material in &e.materials {
        if !palette.contains_key(material) {
            p.add(format!("{id}: material {material} is not in the palette"));
        }
    }
    if e.team_mask != e.materials.iter().any(|x| x == "team") {
        p.add(format!("{id}: team_mask disagrees with materials"));
    }
    let names: Vec<&str> = e.lods.iter().map(|l| l.node.as_str()).collect();
    if names != LOD_NODES {
        p.add(format!("{id}: lods must be {LOD_NODES:?}, got {names:?}"));
        return;
    }
    let Some(limits) = w.budgets.get(&e.budget) else {
        p.add(format!("{id}: unknown budget class {}", e.budget));
        return;
    };
    for (lod, limit) in e.lods.iter().zip(limits) {
        if lod.tris > *limit {
            p.add(format!(
                "{id} {}: {} triangles exceed the {} budget {limit}",
                lod.node, lod.tris, e.budget
            ));
        }
    }
    let tris: Vec<usize> = e.lods.iter().map(|l| l.tris).collect();
    if !(tris[0] >= tris[1] && tris[1] >= tris[2] && tris[2] > 0) {
        p.add(format!("{id}: LOD triangle counts must be positive and not grow, got {tris:?}"));
    }
    if !w.shapes.contains_key(&e.budget) {
        p.add(format!("{id}: no shape limits for budget class {}", e.budget));
    }
}

fn check_keys(w: &World, index: &BTreeMap<String, usize>, p: &mut Problems) {
    for landmark in w.city_kit.landmarks.values() {
        if !index.contains_key(&landmark.model) {
            p.add(format!("landmark model {} is not declared", landmark.model));
        }
    }
    for (key, model) in &w.keys {
        let plain = key.split('@').next().unwrap_or("");
        // The classic module's keys name their table; another module's keys are its own ids under
        // its namespace.
        let well_formed = match plain.split_once(':') {
            Some(("core", rest)) => rest.contains('/'),
            Some((space, id)) => {
                !space.is_empty() && space.bytes().all(|b| b.is_ascii_lowercase()) && !id.is_empty()
            }
            None => false,
        };
        if !well_formed || key.split('@').count() > 2 {
            p.add(format!("key {key}: must be core:<table>/<slug> or <module>:<id>, [@<qualifier>]"));
        }
        if !index.contains_key(model) {
            p.add(format!("key {key} -> {model}: model is not declared"));
        }
    }
}

fn read_docs(root: &Path, rel: &str) -> Vec<DocItem> {
    let text = std::fs::read_to_string(super::units::doc_file(root, rel))
        .unwrap_or_else(|e| panic!("world models: {rel}: {e}"));
    serde_json::from_str::<Docs>(&text)
        .unwrap_or_else(|e| panic!("world models: {rel} is malformed: {e}"))
        .items
}

/// Every map object of classic/docs/classic has a model of the right kind or an explicit exemption.
fn check_coverage(root: &Path, w: &World, p: &mut Problems) {
    // Skipped when the module's catalogs are not beside the platform (see `units::doc_file`).
    if !super::units::doc_file(root, DOCS[0]).is_file() {
        return;
    }
    let model_kind: BTreeMap<&str, &str> =
        w.models.iter().map(|e| (e.id.as_str(), e.kind.as_str())).collect();
    let mut drawn: BTreeMap<&str, &str> = BTreeMap::new();
    for (key, model) in &w.keys {
        let plain = key.split('@').next().unwrap_or("");
        if !key.contains('@') || !drawn.contains_key(plain) {
            drawn.insert(plain, model);
        }
    }
    let need = |key: &str, kind: &str, what: &str, p: &mut Problems| match drawn.get(key) {
        None => p.add(format!("{what} {key} has no model")),
        Some(model) if model_kind.get(model) != Some(&kind) => {
            p.add(format!("{what} {key} is drawn by {model}, expected a {kind} model"))
        }
        Some(_) => {}
    };
    for item in read_docs(root, DOCS[0]) {
        need(&item.id, "improvement", "improvement", p);
    }
    for item in read_docs(root, DOCS[1]) {
        if !w.icon_only.contains_key(&item.id) {
            need(&item.id, "resource", "resource", p);
        } else if drawn.contains_key(item.id.as_str()) {
            p.add(format!("resource {} is icon-only and also has a model", item.id));
        }
    }
    for item in read_docs(root, DOCS[2]) {
        need(&item.id, "natural-wonder", "natural wonder", p);
    }
    for item in read_docs(root, DOCS[3]) {
        match item.kind.as_deref() {
            Some("world_wonder") => need(&item.id, "wonder", "world wonder", p),
            Some("national_wonder") if !w.city_kit.landmarks.contains_key(&item.id) => {
                p.add(format!("national wonder {} has no city landmark", item.id))
            }
            Some("national_wonder") => {}
            _ if !w.no_map_presence.contains_key(&item.id) => p.add(format!(
                "{} is neither drawn nor listed in no_map_presence",
                item.id
            )),
            _ => {}
        }
    }
}

fn check_city_kit(w: &World, index: &BTreeMap<String, usize>, p: &mut Problems) {
    let kit = &w.city_kit;
    let budget_of = |id: &str| index.get(id).map(|i| w.models[*i].budget.as_str());
    let piece = |id: &str, budget: &str, p: &mut Problems| {
        if budget_of(id) != Some(budget) {
            p.add(format!("city kit: {id} must be a declared {budget} model"));
        }
    };
    if kit.styles.is_empty() {
        p.add("city kit: no styles".into());
    }
    for style in &kit.styles {
        match kit.centers.get(style) {
            Some(center) => piece(center, "city-center", p),
            None => p.add(format!("city kit: style {style} has no center")),
        }
        match kit.houses.get(style) {
            Some(houses) if !houses.is_empty() => {
                for house in houses {
                    piece(house, "city-house", p);
                }
            }
            _ => p.add(format!("city kit: style {style} has no houses")),
        }
    }
    for (building, landmark) in &kit.landmarks {
        if !SLOT_KINDS.contains(&landmark.slot.as_str()) {
            p.add(format!("city kit: {building} slot {} not in {SLOT_KINDS:?}", landmark.slot));
        }
        let budget = if landmark.slot == "ring" { "city-walls" } else { "city-landmark" };
        piece(&landmark.model, budget, p);
        if let Some(other) = &landmark.supersedes {
            if !kit.landmarks.contains_key(other) {
                p.add(format!("city kit: {building} supersedes {other}, which has no landmark"));
            }
        }
    }
    let rows = &kit.houses_by_population;
    let ascending = rows.windows(2).all(|r| r[0].0 < r[1].0 && r[0].1 <= r[1].1);
    if rows.is_empty() || !ascending {
        p.add("city kit: houses_by_population must ascend".into());
    }
    let max_houses = rows.last().map_or(0, |r| r.1);
    if kit.slots.len() < max_houses {
        p.add("city kit: fewer slots than the largest house count".into());
    }
    if kit.older_style_share == 0 {
        p.add("city kit: older_style_share must be positive".into());
    }
    for (i, (x, z, yaw, r, ring)) in kit.slots.iter().enumerate() {
        let finite = [x, z, yaw, r].iter().all(|v| v.is_finite());
        if !finite || !(ring == "inner" || ring == "outer") || x.hypot(*z) + r > 0.86 {
            p.add(format!("city kit: slot {i} is malformed or leaves the hex"));
        }
    }
}

/// Worst case of one city tile at LOD0: center, ring landmarks, the heaviest slotted
/// landmarks and houses in the remaining slots.
fn check_city_tile(w: &World, index: &BTreeMap<String, usize>, p: &mut Problems) {
    let kit = &w.city_kit;
    let tris = |id: &str| w.models[index[id]].lods[0].tris;
    let rings: usize = kit
        .landmarks
        .values()
        .filter(|l| l.slot == "ring")
        .map(|l| tris(&l.model))
        .sum();
    let mut slotted: Vec<usize> = kit
        .landmarks
        .values()
        .filter(|l| l.slot != "ring")
        .map(|l| tris(&l.model))
        .collect();
    slotted.sort_unstable_by(|a, b| b.cmp(a));
    let max_houses = kit.houses_by_population.last().map_or(0, |r| r.1);
    for style in &kit.styles {
        let house = kit.houses[style].iter().map(|h| tris(h)).max().unwrap_or(0);
        for landmarks in 0..=kit.max_landmarks.min(slotted.len()) {
            let houses = max_houses.min(kit.slots.len() - landmarks);
            let total = tris(&kit.centers[style])
                + rings
                + slotted[..landmarks].iter().sum::<usize>()
                + houses * house;
            if total > w.city_tile_lod0_max {
                p.add(format!(
                    "city tile ({style}, {landmarks} landmarks): {total} LOD0 triangles exceed {}",
                    w.city_tile_lod0_max
                ));
            }
        }
    }
}

/// No stray GLB in `<part>/assets/models/world/<kind>`: every file is declared by an entry that
/// names that very file in the part's own folder.
fn check_files(parts: &[Part], w: &World, p: &mut Problems) {
    let declared: BTreeSet<PathBuf> = w.models.iter().map(|e| e.dir.join(&e.file)).collect();
    for part in parts {
        for kind in KINDS {
            let dir = part.models.join("world").join(kind);
            glb::check_stray_files(&dir, &declared, &format!("{}/world/{kind}", part.name), p);
        }
    }
}

fn load_model(
    palette: &BTreeMap<String, [f32; 3]>,
    w: &World,
    e: &Entry,
    p: &mut Problems,
) -> Option<[Mesh; 3]> {
    let source = glb::Source {
        id: &e.id,
        file: &e.file,
        pivot: e.pivot,
        scale: e.scale,
    };
    let lods = glb::load_lods(&e.dir, &source, p)?;
    for ((lod, name), mesh) in e.lods.iter().zip(LOD_NODES).zip(&lods) {
        if mesh.triangles() != lod.tris {
            p.add(format!(
                "{} {name}: {} triangles in GLB, manifest says {}",
                e.id,
                mesh.triangles(),
                lod.tris
            ));
        }
        glb::check_mesh(palette, &e.id, name, &e.materials, mesh, p);
    }
    check_shape(w, e, &lods, p);
    Some(lods)
}

/// Declared bounds match the GLB; footprint, height and grounding fit the budget class.
fn check_shape(w: &World, e: &Entry, lods: &[Mesh; 3], p: &mut Problems) {
    let id = &e.id;
    let (lo, hi) = extent(&lods[0].pos);
    let stale = (0..3).any(|k| {
        (lo[k] - e.bounds.min[k]).abs() > SHAPE_EPS || (hi[k] - e.bounds.max[k]).abs() > SHAPE_EPS
    });
    if stale {
        p.add(format!(
            "{id}: LOD0 bounds {lo:?}..{hi:?} disagree with manifest (stale export?)"
        ));
    }
    let radius = lods[0]
        .pos
        .iter()
        .map(|v| v[0].hypot(v[2]))
        .fold(0.0f32, f32::max);
    if (radius - e.radius).abs() > SHAPE_EPS {
        p.add(format!("{id}: radius {radius:.4} disagrees with manifest {}", e.radius));
    }
    let Some([max_r, max_h, low]) = w.shapes.get(&e.budget).copied() else {
        return;
    };
    if radius > max_r + SHAPE_EPS {
        p.add(format!("{id}: footprint radius {radius:.3} exceeds {max_r} ({})", e.budget));
    }
    if hi[1] > max_h + SHAPE_EPS {
        p.add(format!("{id}: height {:.3} exceeds {max_h} ({})", hi[1], e.budget));
    }
    if lo[1] < low - SHAPE_EPS {
        p.add(format!("{id}: lowest point {:.3} below {low} ({})", lo[1], e.budget));
    }
    match e.ground.as_str() {
        "water" if !(lo[1] < 0.0 && hi[1] > 0.0) => p.add(format!(
            "{id}: water model must cross the waterline, y {:.3}..{:.3}",
            lo[1], hi[1]
        )),
        "upright" | "plane" if lo[1] > 0.02 => {
            p.add(format!("{id}: floats, lowest point y = {:.4}", lo[1]))
        }
        _ => {}
    }
    // Reduced LODs may differ in detail, not in size: within 10% of the model size.
    let tolerance = LOD_SLACK + LOD_SLACK_SIZE * e.size;
    for (lod, mesh) in lods.iter().enumerate().skip(1) {
        let (l, h) = extent(&mesh.pos);
        let inside = (0..3).all(|k| l[k] >= lo[k] - tolerance && h[k] <= hi[k] + tolerance);
        if !inside {
            p.add(format!("{id} {}: bounds {l:?}..{h:?} leave LOD0 bounds", LOD_NODES[lod]));
        }
    }
}

/// A model made here from the baked ones (support kit, wall parts): not in the manifest.
struct Synthetic {
    id: String,
    kind: &'static str,
    ground: &'static str,
    seat: &'static str,
    lod_px: [f32; 2],
    meshes: [Mesh; 3],
}

/// How a manifest model stands on the ground (world_models::Seat).
fn seat_variant(w: &World, e: &Entry) -> &'static str {
    let shore = w.city_kit.landmarks.values().any(|l| l.slot == "water" && l.model == e.id);
    match e.kind.as_str() {
        _ if e.ground != "upright" => "Bare",
        _ if shore => "Shore",
        "city" | "wonder" => "Terraced",
        _ => "Bare",
    }
}

fn emit(
    out_dir: &Path,
    w: &World,
    index: &BTreeMap<String, usize>,
    meshes: &[[Mesh; 3]],
    palette: &BTreeMap<String, [f32; 3]>,
) {
    let mut bytes: Vec<u8> = Vec::new();
    let mut code =
        String::from("// @generated by build.rs from assets/models/manifest.json (world) — do not edit.\n");
    let mut table = String::new();
    let mut vertex = 0u32;
    let row = |table: &mut String,
                   bytes: &mut Vec<u8>,
                   vertex: &mut u32,
                   id: &str,
                   kind: &str,
                   ground: &str,
                   water_site: bool,
                   site_shape: &str,
                   seat: &str,
                   lod_px: [f32; 2],
                   size: f32,
                   radius: f32,
                   lods: &[Mesh; 3]| {
        let mut ranges = Vec::new();
        for mesh in lods {
            ranges.push((*vertex, mesh.pos.len() as u32));
            glb::write_vertices(bytes, mesh);
            *vertex += mesh.pos.len() as u32;
        }
        let (lo, hi) = extent(&lods[0].pos);
        let foot = support::foot(&lods[0], support::MAX_CONTACTS);
        let contacts: Vec<String> = foot.points.iter().map(|p| format!("[{:?}, {:?}]", p[0], p[1])).collect();
        let _ = writeln!(
            table,
            "    WorldModel {{ id: {:?}, kind: WorldKind::{}, ground: Ground::{}, water_site: {}, \
             site_shape: SiteShape::{}, seat: Seat::{}, lods: [({}, {}), ({}, {}), ({}, {})], lod_px: [{:?}, {:?}], size: {:?}, radius: {:?}, \
             low: {:?}, height: {:?}, contacts: &[{}], foot: [{:?}, {:?}, {:?}] }},",
            id,
            kind,
            ground,
            water_site,
            site_shape,
            seat,
            ranges[0].0, ranges[0].1, ranges[1].0, ranges[1].1, ranges[2].0, ranges[2].1,
            lod_px[0], lod_px[1], size, radius, lo[1], hi[1],
            contacts.join(", "),
            foot.centre[0], foot.centre[1], foot.radius
        );
    };
    for (e, lods) in w.models.iter().zip(meshes) {
        row(
            &mut table,
            &mut bytes,
            &mut vertex,
            &e.id,
            kind_variant(&e.kind),
            ground_variant(&e.ground),
            e.site == "water",
            site_shape_variant(&e.site_shape),
            seat_variant(w, e),
            e.lod_px,
            e.size,
            e.radius,
            lods,
        );
    }
    // Models made here: the support kit, then the parts of the wall ring.
    let mut synthetic: Vec<Synthetic> = vec![
        Synthetic { id: "support/plinth".into(), kind: "Support", ground: "Upright", seat: "Bare", lod_px: [60.0, 24.0], meshes: support::plinth(palette) },
        Synthetic { id: "support/pile".into(), kind: "Support", ground: "Upright", seat: "Bare", lod_px: [60.0, 24.0], meshes: support::pile(palette) },
    ];
    let walls = index.get("city/walls").copied();
    let first_part = synthetic.len();
    if let Some(i) = walls {
        let entry = &w.models[i];
        for (k, part) in support::wall_parts(&meshes[i], palette).into_iter().enumerate() {
            synthetic.push(Synthetic {
                id: format!("city/walls#{k}"),
                kind: "City",
                ground: "Plane",
                seat: "Bare",
                lod_px: entry.lod_px,
                meshes: part,
            });
        }
    }
    let base = w.models.len();
    for s in &synthetic {
        let (lo, hi) = extent(&s.meshes[0].pos);
        let radius = s.meshes[0].pos.iter().map(|v| v[0].hypot(v[2])).fold(0.0f32, f32::max);
        let size = radius.max(hi[1] - lo[1].min(0.0));
        row(&mut table, &mut bytes, &mut vertex, &s.id, s.kind, s.ground, false, "Pad", s.seat, s.lod_px, size, radius, &s.meshes);
    }
    let total = w.models.len() + synthetic.len();
    let _ = writeln!(code, "pub const PLINTH_MODEL: u16 = {};", base);
    let _ = writeln!(code, "pub const PILE_MODEL: u16 = {};", base + 1);
    match walls {
        Some(i) => {
            let parts: Vec<String> = (base + first_part..total).map(|k| k.to_string()).collect();
            let _ = writeln!(code, "/// The manifest model of the wall ring, drawn as the parts below.");
            let _ = writeln!(code, "pub const WALLS_MODEL: Option<u16> = Some({i});");
            let _ = writeln!(code, "pub static WALL_PARTS: [u16; {}] = [{}];", parts.len(), parts.join(", "));
        }
        None => {
            let _ = writeln!(code, "pub const WALLS_MODEL: Option<u16> = None;");
            let _ = writeln!(code, "pub static WALL_PARTS: [u16; 0] = [];");
        }
    }
    let _ = writeln!(
        code,
        "pub static WORLD_MODELS: [WorldModel; {}] = [\n{table}];",
        total
    );
    let _ = writeln!(code, "/// Content key (with optional @qualifier) -> model, sorted for binary search.");
    let _ = writeln!(code, "pub static WORLD_KEYS: [(&str, u16); {}] = [", w.keys.len());
    for (key, model) in &w.keys {
        let _ = writeln!(code, "    ({key:?}, {}),", index[model]);
    }
    let _ = writeln!(code, "];");
    emit_city_kit(&mut code, w, index);
    let _ = writeln!(code, "pub const WORLD_VERTEX_COUNT: usize = {vertex};");
    std::fs::write(out_dir.join("world_models.bin"), &bytes).unwrap();
    std::fs::write(out_dir.join("world_models.rs"), code).unwrap();
}

fn emit_city_kit(code: &mut String, w: &World, index: &BTreeMap<String, usize>) {
    let kit = &w.city_kit;
    let mut styles = String::new();
    for style in &kit.styles {
        let houses: Vec<String> = kit.houses[style].iter().map(|h| index[h].to_string()).collect();
        let _ = write!(
            styles,
            "CityStyle {{ name: {style:?}, center: {}, houses: &[{}] }}, ",
            index[&kit.centers[style]],
            houses.join(", ")
        );
    }
    let slots: Vec<String> = kit
        .slots
        .iter()
        .map(|(x, z, yaw, r, ring)| {
            format!(
                "CitySlot {{ x: {x:?}, z: {z:?}, yaw: {yaw:?}, radius: {r:?}, outer: {} }}",
                ring == "outer"
            )
        })
        .collect();
    let mut landmarks: Vec<(&String, &Landmark)> = kit.landmarks.iter().collect();
    landmarks.sort_by(|a, b| (a.1.priority, a.0).cmp(&(b.1.priority, b.0)));
    let landmarks: Vec<String> = landmarks
        .iter()
        .map(|(building, l)| {
            format!(
                "CityLandmark {{ building: {building:?}, model: {}, slot: SlotKind::{}, supersedes: {:?} }}",
                index[&l.model],
                slot_variant(&l.slot),
                l.supersedes
            )
        })
        .collect();
    let rows: Vec<String> = kit
        .houses_by_population
        .iter()
        .map(|(pop, houses)| format!("({pop}, {houses})"))
        .collect();
    let _ = writeln!(
        code,
        "pub static CITY_KIT: CityKit = CityKit {{\n    styles: &[{styles}],\n    houses_by_population: &[{}],\n    \
         older_style_share: {},\n    max_landmarks: {},\n    slots: &[{}],\n    landmarks: &[{}],\n}};",
        rows.join(", "),
        kit.older_style_share,
        kit.max_landmarks,
        slots.join(", "),
        landmarks.join(", ")
    );
}

fn kind_variant(kind: &str) -> &'static str {
    match kind {
        "improvement" => "Improvement",
        "resource" => "Resource",
        "natural-wonder" => "NaturalWonder",
        "wonder" => "Wonder",
        _ => "City",
    }
}

fn ground_variant(ground: &str) -> &'static str {
    match ground {
        "plane" => "Plane",
        "water" => "Water",
        _ => "Upright",
    }
}

fn site_shape_variant(shape: &str) -> &'static str {
    match shape {
        "crater" => "Crater",
        "reef" => "Reef",
        _ => "Pad",
    }
}

fn slot_variant(slot: &str) -> &'static str {
    match slot {
        "inner" => "Inner",
        "outer" => "Outer",
        "water" => "Water",
        _ => "Ring",
    }
}
