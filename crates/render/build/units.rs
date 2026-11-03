//! Unit models: the manifests of every part present (`<part>/assets/models/manifest.json`, see
//! parts.rs) + `<part>/assets/models/units/*.glb` -> baked vertices.
//!
//! Each manifest is validated against the platform's vocabulary (palette, budgets), then the
//! merged set against the GLBs and classic/docs/classic (coverage of every unit class and unique
//! unit). The merge is checked for the platform alone, for the platform with each module and for
//! everything together, so a module builds with the platform alone. Writes OUT_DIR/unit_models.bin
//! (UnitVertex array, little endian) and OUT_DIR/unit_models.rs (model table sorted by id +
//! content-key index). Every violation is collected and reported together; the build fails if
//! there is any.

use crate::glb::{self, extent, Mesh, Problems, LOD_NODES};
use crate::parts::Part;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "aurelia-models/2";
/// Inner radius of a hex (HEX_SIZE = 1): every figure of a formation stays inside it.
const HEX_INNER: f32 = 0.866;
const MAX_FIGURES: usize = 6;
/// Far markers of formations are drawn larger (catalog.MARKER_BOOST); bounds allow it.
const MARKER_BOUNDS: f32 = 1.35;
const BOUNDS_EPS: f32 = 3e-3;

/// One part's manifest, or the merge of several. The platform's carries the vocabulary (palette,
/// budgets, the fallback model); a module's carries its models and its content keys.
#[derive(Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    /// The part that owns the file ("platform", "classic", "lands"); must be the folder's name.
    #[serde(default)]
    owner: String,
    /// The namespace of the content keys a module's manifest binds ("core", "lal").
    #[serde(default)]
    key_namespace: String,
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
    fallback: String,
    #[serde(default)]
    palette: BTreeMap<String, [f32; 3]>,
    #[serde(default)]
    budgets: BTreeMap<String, [usize; 3]>,
    #[serde(default)]
    formation_lod0_max: usize,
    #[serde(default)]
    models: Vec<Entry>,
    #[serde(default)]
    classes: BTreeMap<String, String>,
    #[serde(default)]
    units: BTreeMap<String, String>,
    #[serde(default)]
    fallbacks: BTreeMap<String, String>,
    /// World objects: validated and baked by world.rs.
    #[allow(dead_code)]
    #[serde(default)]
    world: serde::de::IgnoredAny,
    /// Terrain props (sea stacks): validated and baked by terrain.rs.
    #[allow(dead_code)]
    #[serde(default)]
    terrain: serde::de::IgnoredAny,
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
    category: String,
    budget: String,
    variant_of: Option<String>,
    contact: String,
    scale: f32,
    pivot: [f32; 3],
    bounds: Bounds,
    materials: Vec<String>,
    team_mask: bool,
    lods: Vec<Lod>,
    lod_px: [f32; 2],
    formation: Formation,
    /// Animation rig, weapon and the yaw (degrees) baked into the model (platform/tools/blender/units/rigs.py).
    rig: String,
    weapon: String,
    baked_yaw: f32,
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

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Formation {
    figures: usize,
    slots: Vec<[f32; 3]>,
}

#[derive(Deserialize)]
struct Docs {
    items: Vec<DocUnit>,
}

#[derive(Deserialize)]
struct DocUnit {
    id: String,
    class: String,
}

pub fn bake(parts: &[Part], root: &Path, out_dir: &Path) {
    for doc in ["docs/classic/units.json", "docs/classic/unique_units.json"] {
        if doc_file(root, doc).is_file() {
            println!("cargo:rerun-if-changed={}", doc_file(root, doc).display());
        }
    }
    let mut problems = Problems(Vec::new());
    let manifests: Vec<Manifest> = parts
        .iter()
        .map(|part| {
            let mut m: Manifest = serde_json::from_str(&part.text).unwrap_or_else(|e| {
                panic!(
                    "unit models: {} is malformed: {e}",
                    part.manifest_path().display()
                )
            });
            check_header(part, &m, &mut problems);
            for e in &mut m.models {
                e.dir = part.models.clone();
                e.part = part.name.clone();
            }
            m
        })
        .collect();
    // Every manifest on its own with the platform's vocabulary (a module must stand with the
    // platform alone), then everything together.
    for set in crate::parts::sets(parts) {
        let members: Vec<(&Part, &Manifest)> =
            set.iter().map(|&i| (&parts[i], &manifests[i])).collect();
        let merged = merge(&members, &mut problems);
        check_manifest(&merged, &mut problems);
        check_targets(&merged, &mut problems);
        check_unused(&merged, &members, &mut problems);
        if members.iter().any(|(part, _)| part.name == "classic") {
            check_coverage(root, &merged, &mut problems);
        }
    }
    let members: Vec<(&Part, &Manifest)> = parts.iter().zip(&manifests).collect();
    let manifest = merge(&members, &mut problems);
    check_files(parts, &manifest, &mut problems);
    let meshes: Vec<Option<[Mesh; 3]>> = manifest
        .models
        .iter()
        .map(|entry| load_model(&manifest, entry, &mut problems))
        .collect();
    problems.dedup();
    if !problems.0.is_empty() {
        panic!(
            "\n\nunit models: {} problem(s) in the models manifests (regenerate with `sh <part>/tools/blender/export_all.sh` in the part that owns the model; docs/models-ownership.md):\n  - {}\n",
            problems.0.len(),
            problems.0.join("\n  - ")
        );
    }
    let meshes: Vec<[Mesh; 3]> = meshes.into_iter().map(Option::unwrap).collect();
    emit(out_dir, &manifest, &meshes);
}

/// What one manifest must say about itself: the schema, the owner (the folder's name) and, for a
/// module, the namespace of every key it binds. The platform binds no content key at all.
fn check_header(part: &Part, m: &Manifest, p: &mut Problems) {
    let who = &part.name;
    if m.schema != SCHEMA {
        p.add(format!(
            "{who}: schema is {:?}, expected {SCHEMA:?}",
            m.schema
        ));
    }
    if m.owner != part.name {
        p.add(format!(
            "{who}: owner is {:?}, expected {:?}",
            m.owner, part.name
        ));
    }
    let keys = m
        .classes
        .keys()
        .chain(m.units.keys())
        .chain(m.fallbacks.keys());
    if part.is_platform() {
        if !m.key_namespace.is_empty() {
            p.add("platform: a key_namespace belongs to a module's manifest".into());
        }
        for key in keys {
            p.add(format!(
                "platform: content key {key} belongs to a module's manifest"
            ));
        }
        if m.fallback.is_empty() {
            p.add("platform: the manifest must name the fallback model".into());
        }
        return;
    }
    if m.key_namespace.is_empty() || !m.key_namespace.bytes().all(|b| b.is_ascii_lowercase()) {
        p.add(format!(
            "{who}: key_namespace {:?} must be lowercase letters",
            m.key_namespace
        ));
    }
    if !m.fallback.is_empty() || m.formation_lod0_max != 0 || !m.axes.is_empty() {
        p.add(format!(
            "{who}: fallback, axes and formation_lod0_max belong to the platform's manifest"
        ));
    }
    let prefix = format!("{}:", m.key_namespace);
    for key in keys {
        if !key.starts_with(&prefix) {
            p.add(format!(
                "{who}: key {key} is outside the namespace {prefix}"
            ));
        }
    }
}

/// The union of the manifests of `members` (the platform first). A name declared twice is a
/// problem; the models come out sorted by id, so the result does not depend on the order of parts.
fn merge(members: &[(&Part, &Manifest)], p: &mut Problems) -> Manifest {
    let mut out = members[0].1.clone();
    out.models.sort_by(|a, b| a.id.cmp(&b.id));
    for (part, m) in &members[1..] {
        for (name, rgb) in &m.palette {
            if out
                .palette
                .insert(name.clone(), *rgb)
                .is_some_and(|old| old != *rgb)
            {
                p.add(format!(
                    "{}: palette color {name} redefines the platform's",
                    part.name
                ));
            }
        }
        for (name, tris) in &m.budgets {
            if out.budgets.insert(name.clone(), *tris).is_some() {
                p.add(format!(
                    "{}: budget class {name} is declared twice",
                    part.name
                ));
            }
        }
        out.models.extend(m.models.iter().cloned());
        for (table, dst, src) in [
            ("classes", &mut out.classes, &m.classes),
            ("units", &mut out.units, &m.units),
            ("fallbacks", &mut out.fallbacks, &m.fallbacks),
        ] {
            for (key, model) in src {
                if dst.insert(key.clone(), model.clone()).is_some() {
                    p.add(format!(
                        "{}: {table} key {key} is bound in two parts",
                        part.name
                    ));
                }
            }
        }
    }
    out.models.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Every model is drawn: a module's by its own keys; a platform model (shared) by the keys of
/// every module that is present, and by at least two of them once two modules are present
/// (a model only one module draws belongs to that module).
fn check_unused(m: &Manifest, members: &[(&Part, &Manifest)], p: &mut Problems) {
    let modules: Vec<&(&Part, &Manifest)> = members
        .iter()
        .filter(|(part, _)| !part.is_platform())
        .collect();
    if modules.is_empty() {
        return;
    }
    let mut users: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (part, pm) in &modules {
        for model in pm
            .classes
            .values()
            .chain(pm.units.values())
            .chain(pm.fallbacks.values())
        {
            users
                .entry(model.as_str())
                .or_default()
                .insert(part.name.as_str());
        }
    }
    for e in &m.models {
        let drawn_by = users.get(e.id.as_str()).map_or(0, BTreeSet::len);
        if e.id == m.fallback {
            continue;
        }
        if e.part == crate::parts::PLATFORM {
            if modules.len() >= 2 && drawn_by < 2 {
                p.add(format!(
                    "model {} is owned by the platform (shared) but {} module(s) draw it: move it to the module that draws it",
                    e.id, drawn_by
                ));
            }
        } else if drawn_by == 0 {
            p.add(format!(
                "model {} is declared by {} but no unit of {} uses it",
                e.id, e.part, e.part
            ));
        }
    }
}

fn check_manifest(m: &Manifest, p: &mut Problems) {
    if m.schema != SCHEMA {
        p.add(format!("schema is {:?}, expected {SCHEMA:?}", m.schema));
    }
    glb::check_palette(&m.palette, p);
    let ids: BTreeSet<&str> = m.models.iter().map(|e| e.id.as_str()).collect();
    if ids.len() != m.models.len() {
        p.add("duplicate model ids".into());
    }
    if !ids.contains(m.fallback.as_str()) {
        p.add(format!("fallback model {} is not declared", m.fallback));
    }
    for e in &m.models {
        check_entry(m, e, &ids, p);
    }
}

fn check_entry(m: &Manifest, e: &Entry, ids: &BTreeSet<&str>, p: &mut Problems) {
    let id = &e.id;
    let slug = id.strip_prefix("unit/").unwrap_or("");
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        p.add(format!("{id}: id must be unit/<lowercase-slug>"));
    }
    if e.file != format!("units/{slug}.glb") {
        p.add(format!(
            "{id}: file must be units/{slug}.glb, got {}",
            e.file
        ));
    }
    if !matches!(
        e.category.as_str(),
        "military" | "naval" | "air" | "civilian" | "religious" | "great-person" | "spaceship"
    ) {
        p.add(format!("{id}: unknown category {}", e.category));
    }
    if !matches!(e.contact.as_str(), "ground" | "water" | "hover") {
        p.add(format!(
            "{id}: contact must be ground | water | hover, got {}",
            e.contact
        ));
    }
    if !(e.scale.is_finite() && e.scale > 0.0 && e.scale <= 1.0) {
        p.add(format!("{id}: scale {} must be in (0, 1]", e.scale));
    }
    if !e.pivot.iter().all(|v| v.is_finite()) {
        p.add(format!("{id}: pivot {:?} not finite", e.pivot));
    }
    let [near, far] = e.lod_px;
    if !(near.is_finite() && far.is_finite() && near > far && far > 0.0) {
        p.add(format!(
            "{id}: lod_px {:?} must be decreasing and positive",
            e.lod_px
        ));
    }
    if let Some(base) = &e.variant_of {
        if !ids.contains(base.as_str()) {
            p.add(format!("{id}: variant_of {base} is not declared"));
        }
    }
    for material in &e.materials {
        if !m.palette.contains_key(material) {
            p.add(format!("{id}: material {material} is not in the palette"));
        }
    }
    if e.team_mask != e.materials.iter().any(|x| x == "team") {
        p.add(format!("{id}: team_mask disagrees with materials"));
    }
    check_budgets(m, e, p);
    check_formation(e, p);
}

fn check_budgets(m: &Manifest, e: &Entry, p: &mut Problems) {
    let id = &e.id;
    let names: Vec<&str> = e.lods.iter().map(|l| l.node.as_str()).collect();
    if names != LOD_NODES {
        p.add(format!("{id}: lods must be {LOD_NODES:?}, got {names:?}"));
        return;
    }
    let Some(limits) = m.budgets.get(&e.budget) else {
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
    if !(e.lods[0].tris >= e.lods[1].tris && e.lods[1].tris >= e.lods[2].tris) {
        let tris: Vec<usize> = e.lods.iter().map(|l| l.tris).collect();
        p.add(format!(
            "{id}: LOD triangle counts must not grow, got {tris:?}"
        ));
    }
    let formation = e.lods[0].tris * e.formation.figures;
    if formation > m.formation_lod0_max {
        p.add(format!(
            "{id}: formation {} x {} = {formation} triangles exceed {}",
            e.formation.figures, e.lods[0].tris, m.formation_lod0_max
        ));
    }
}

fn check_formation(e: &Entry, p: &mut Problems) {
    let id = &e.id;
    let f = &e.formation;
    if !(1..=MAX_FIGURES).contains(&f.figures) || f.slots.len() != f.figures {
        p.add(format!(
            "{id}: formation needs 1..={MAX_FIGURES} figures and one slot each"
        ));
        return;
    }
    let radius = footprint(&e.bounds.min, &e.bounds.max);
    for (i, [x, z, s]) in f.slots.iter().enumerate() {
        if ![x, z, s].iter().all(|v| v.is_finite()) || *s <= 0.0 {
            p.add(format!("{id}: slot {i} is not finite / positive"));
            continue;
        }
        let reach = x.hypot(*z) + radius * s;
        if reach > HEX_INNER {
            p.add(format!(
                "{id}: slot {i} reaches {reach:.3} > hex inner radius {HEX_INNER}"
            ));
        }
    }
}

fn footprint(min: &[f32; 3], max: &[f32; 3]) -> f32 {
    [min[0].abs(), max[0].abs(), min[2].abs(), max[2].abs()]
        .into_iter()
        .fold(0.0, f32::max)
}

/// Every unit class and unit of classic/docs/classic resolves to a model, or to an explicit fallback.
fn check_coverage(root: &Path, m: &Manifest, p: &mut Problems) {
    // The coverage check needs the module's reference catalogs; without them (the platform built
    // alone) it is skipped. They sit in the sibling part checkout, see `doc_file`.
    if !doc_file(root, "docs/classic/units.json").is_file() {
        return;
    }
    let regular = read_docs(root, "docs/classic/units.json");
    let unique = read_docs(root, "docs/classic/unique_units.json");
    for unit in regular.iter().chain(unique.iter()) {
        if !m.classes.contains_key(&unit.class) && !m.fallbacks.contains_key(&unit.class) {
            p.add(format!(
                "unit class {} ({}) has no model and no explicit fallback",
                unit.class, unit.id
            ));
        }
        if !m.units.contains_key(&unit.id) && !m.fallbacks.contains_key(&unit.id) {
            p.add(format!(
                "unit {} has no model and no explicit fallback",
                unit.id
            ));
        }
    }
    for unit in &unique {
        let (Some(model), Some(class_model)) = (m.units.get(&unit.id), m.classes.get(&unit.class))
        else {
            continue;
        };
        let variant_of = m
            .models
            .iter()
            .find(|e| &e.id == model)
            .and_then(|e| e.variant_of.as_ref());
        if model != class_model && variant_of != Some(class_model) {
            p.add(format!(
                "unique unit {} uses {model}, which is neither its class model {class_model} nor a variant of it",
                unit.id
            ));
        }
    }
}

/// Every key binds a declared model.
fn check_targets(m: &Manifest, p: &mut Problems) {
    let ids: BTreeSet<&str> = m.models.iter().map(|e| e.id.as_str()).collect();
    for (key, model) in m.classes.iter().chain(&m.units).chain(&m.fallbacks) {
        if !ids.contains(model.as_str()) {
            p.add(format!("{key} -> {model}: model is not declared"));
        }
    }
}

/// The module catalogs live next to the platform folder, in the part that owns them
/// (`<repository>/classic/docs/classic/`); `rel` names them as `classic/docs/classic/<file>`.
pub fn doc_file(root: &Path, rel: &str) -> std::path::PathBuf {
    root.join("..").join("classic").join(rel)
}

fn read_docs(root: &Path, rel: &str) -> Vec<DocUnit> {
    let text = std::fs::read_to_string(doc_file(root, rel))
        .unwrap_or_else(|e| panic!("unit models: {rel}: {e}"));
    serde_json::from_str::<Docs>(&text)
        .unwrap_or_else(|e| panic!("unit models: {rel} is malformed: {e}"))
        .items
}

/// No stray GLB in `<part>/assets/models/units`: an undeclared file is a stale export, and a
/// file in the wrong part is a model that was moved without its manifest entry.
fn check_files(parts: &[Part], m: &Manifest, p: &mut Problems) {
    let declared: BTreeSet<PathBuf> = m.models.iter().map(|e| e.dir.join(&e.file)).collect();
    for part in parts {
        let units = part.models.join("units");
        if !units.is_dir() {
            if m.models.iter().any(|e| e.part == part.name) {
                p.add(format!("{}: assets/models/units is missing", part.name));
            }
            continue;
        }
        glb::check_stray_files(&units, &declared, &format!("{}/units", part.name), p);
    }
}

fn load_model(m: &Manifest, e: &Entry, p: &mut Problems) -> Option<[Mesh; 3]> {
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
        glb::check_mesh(&m.palette, &e.id, name, &e.materials, mesh, p);
    }
    check_bounds(e, &lods, p);
    Some(lods)
}

/// Grounding, footprint, declared bounds (stale manifest) and LOD bounds.
fn check_bounds(e: &Entry, lods: &[Mesh], p: &mut Problems) {
    let id = &e.id;
    let (lo, hi) = extent(&lods[0].pos);
    for k in 0..3 {
        if (lo[k] - e.bounds.min[k]).abs() > BOUNDS_EPS
            || (hi[k] - e.bounds.max[k]).abs() > BOUNDS_EPS
        {
            p.add(format!(
                "{id}: LOD0 bounds {lo:?}..{hi:?} disagree with manifest {:?}..{:?} (stale export?)",
                e.bounds.min, e.bounds.max
            ));
            break;
        }
    }
    match e.contact.as_str() {
        "ground" if !(-0.01..=0.02).contains(&lo[1]) => p.add(format!(
            "{id}: not grounded, lowest point y = {:.4} (contact ground needs 0 +- 0.01)",
            lo[1]
        )),
        "hover" if lo[1] < 0.02 => p.add(format!(
            "{id}: hover model touches the ground (y = {:.4})",
            lo[1]
        )),
        "water" if !(lo[1] < 0.0 && lo[1] > -0.15 && hi[1] > 0.0) => p.add(format!(
            "{id}: hull must cross the waterline, y range {:.3}..{:.3}",
            lo[1], hi[1]
        )),
        _ => {}
    }
    let radius = footprint(&lo, &hi);
    if radius > HEX_INNER {
        p.add(format!(
            "{id}: footprint radius {radius:.3} exceeds the hex inner radius {HEX_INNER}"
        ));
    }
    for (lod, mesh) in lods.iter().enumerate().skip(1) {
        let (l, h) = extent(&mesh.pos);
        let limit = if lod == 2 { MARKER_BOUNDS } else { 1.0 };
        let inside = (0..3).all(|k| l[k] >= lo[k] * limit - 0.02 && h[k] <= hi[k] * limit + 0.02);
        if !inside {
            p.add(format!(
                "{id} {}: bounds {l:?}..{h:?} leave LOD0 bounds",
                LOD_NODES[lod]
            ));
        }
    }
}

fn emit(out_dir: &Path, m: &Manifest, meshes: &[[Mesh; 3]]) {
    let mut bytes: Vec<u8> = Vec::new();
    let mut code =
        String::from("// @generated by build.rs from assets/models/manifest.json — do not edit.\n");
    let mut table = String::new();
    let mut vertex = 0u32;
    for (e, lods) in m.models.iter().zip(meshes) {
        let mut ranges = Vec::new();
        for mesh in lods {
            ranges.push((vertex, mesh.pos.len() as u32));
            glb::write_vertices(&mut bytes, mesh);
            vertex += mesh.pos.len() as u32;
        }
        let height = extent(&lods[0].pos).1[1];
        let slots: Vec<String> = e
            .formation
            .slots
            .iter()
            .map(|s| format!("[{:?}, {:?}, {:?}]", s[0], s[1], s[2]))
            .collect();
        let _ = writeln!(
            table,
            "    UnitModel {{ id: {:?}, category: UnitCategory::{}, contact: Contact::{}, slots: &[{}], \
             lods: [({}, {}), ({}, {}), ({}, {})], lod_px: [{:?}, {:?}], height: {:?}, \
             rig: Rig::{}, weapon: Weapon::{}, baked_yaw: {:?}, scale: {:?} }},",
            e.id,
            category_variant(&e.category),
            contact_variant(&e.contact),
            slots.join(", "),
            ranges[0].0, ranges[0].1, ranges[1].0, ranges[1].1, ranges[2].0, ranges[2].1,
            e.lod_px[0], e.lod_px[1], height,
            rig_variant(&e.rig),
            weapon_variant(&e.weapon),
            e.baked_yaw.to_radians(),
            e.scale
        );
    }
    let index: BTreeMap<&str, usize> = m
        .models
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.as_str(), i))
        .collect();
    let mut keys: BTreeMap<&str, usize> = BTreeMap::new();
    for (key, model) in m.classes.iter().chain(&m.units).chain(&m.fallbacks) {
        keys.insert(key.as_str(), index[model.as_str()]);
    }
    let _ = writeln!(
        code,
        "pub static UNIT_MODELS: [UnitModel; {}] = [\n{table}];",
        m.models.len()
    );
    let _ = writeln!(
        code,
        "/// Content key (unit or unit class) -> model, sorted for binary search."
    );
    let _ = writeln!(
        code,
        "pub static UNIT_KEYS: [(&str, u16); {}] = [",
        keys.len()
    );
    for (key, model) in &keys {
        let _ = writeln!(code, "    ({key:?}, {model}),");
    }
    let _ = writeln!(
        code,
        "];\npub const FALLBACK_MODEL: u16 = {};",
        index[m.fallback.as_str()]
    );
    let _ = writeln!(code, "pub const UNIT_VERTEX_COUNT: usize = {vertex};");
    std::fs::write(out_dir.join("unit_models.bin"), &bytes).unwrap();
    std::fs::write(out_dir.join("unit_models.rs"), code).unwrap();
}

/// The manifest's art category -> the render category: every non-combatant is a civilian.
fn category_variant(category: &str) -> &'static str {
    match category {
        "military" => "Military",
        "naval" => "Naval",
        "air" => "Air",
        _ => "Civilian",
    }
}

/// The manifest's rig name -> the `Rig` variant (an unknown name fails the build).
fn rig_variant(rig: &str) -> &'static str {
    match rig {
        "biped" => "Biped",
        "horse" => "Horse",
        "elephant" => "Elephant",
        "camel" => "Camel",
        "chariot" => "Chariot",
        "cart" => "Cart",
        "engine" => "Engine",
        "vehicle" => "Vehicle",
        "vessel" => "Vessel",
        "plane" => "Plane",
        "missile" => "Missile",
        "static" => "Static",
        other => panic!("unknown rig {other:?} in the manifest"),
    }
}

fn weapon_variant(weapon: &str) -> &'static str {
    match weapon {
        "none" => "None",
        "blade" => "Blade",
        "club" => "Club",
        "polearm" => "Polearm",
        "bow" => "Bow",
        "crossbow" => "Crossbow",
        "thrown" => "Thrown",
        "firearm" => "Firearm",
        "autogun" => "Autogun",
        "rocket" => "Rocket",
        "cannon" => "Cannon",
        "catapult" => "Catapult",
        "ballista" => "Ballista",
        "ram" => "Ram",
        "broadside" => "Broadside",
        "torpedo" => "Torpedo",
        "bomb" => "Bomb",
        "missile" => "Missile",
        other => panic!("unknown weapon {other:?} in the manifest"),
    }
}

fn contact_variant(contact: &str) -> &'static str {
    match contact {
        "water" => "Water",
        "hover" => "Hover",
        _ => "Ground",
    }
}
