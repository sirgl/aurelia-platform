//! Shared GLB reading and checks for the manifest bakes (units.rs, world.rs).
//!
//! A model GLB holds nodes LOD0 / LOD1 / LOD2 with identity transforms; each primitive
//! carries POSITION and COLOR_0 (rgb = palette albedo, a = team mask). The bake
//! de-indexes, removes the pivot, applies the scale and computes flat normals itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const LOD_NODES: [&str; 3] = ["LOD0", "LOD1", "LOD2"];
const PALETTE_EPS: f32 = 2e-3;

/// One baked LOD: positions in hex units, colors, team mask; three entries per triangle.
pub struct Mesh {
    pub pos: Vec<[f32; 3]>,
    pub rgb: Vec<[f32; 3]>,
    pub team: Vec<f32>,
}

impl Mesh {
    pub fn triangles(&self) -> usize {
        self.pos.len() / 3
    }
}

/// Report collector: every problem is listed, the build fails once at the end.
pub struct Problems(pub Vec<String>);

impl Problems {
    pub fn add(&mut self, message: String) {
        self.0.push(message);
    }

    /// The same problem found by two of the checked sets is listed once (first occurrence kept).
    pub fn dedup(&mut self) {
        let mut seen = BTreeSet::new();
        self.0.retain(|m| seen.insert(m.clone()));
    }

    /// One message per (key) prefix keeps the report readable.
    pub fn once(&mut self, key: &str, message: String) {
        if !self.0.iter().any(|x| x.starts_with(key)) {
            self.0.push(format!("{key}: {message}"));
        }
    }
}

/// Where a model's GLB is and how its positions map to hex units.
pub struct Source<'a> {
    pub id: &'a str,
    pub file: &'a str,
    pub pivot: [f32; 3],
    pub scale: f32,
}

/// The three LODs of a model, or None (problems recorded).
pub fn load_lods(models_dir: &Path, src: &Source<'_>, p: &mut Problems) -> Option<[Mesh; 3]> {
    let path = models_dir.join(src.file);
    let (doc, buffers, _) = match gltf::import(&path) {
        Ok(x) => x,
        Err(err) => {
            p.add(format!("{}: cannot import {}: {err}", src.id, src.file));
            return None;
        }
    };
    let mut lods = Vec::new();
    for name in LOD_NODES {
        lods.push(read_node(&doc, &buffers, name, src, p)?);
    }
    lods.try_into().ok()
}

fn read_node(
    doc: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    name: &str,
    src: &Source<'_>,
    p: &mut Problems,
) -> Option<Mesh> {
    let Some(node) = doc.nodes().find(|n| n.name() == Some(name)) else {
        p.add(format!("{}: node {name} missing in {}", src.id, src.file));
        return None;
    };
    let (t, r, s) = node.transform().decomposed();
    if t != [0.0; 3] || r != [0.0, 0.0, 0.0, 1.0] || s != [1.0; 3] {
        p.add(format!(
            "{} {name}: node transform must be identity (apply transforms in the script)",
            src.id
        ));
    }
    let Some(gmesh) = node.mesh() else {
        p.add(format!("{} {name}: node has no mesh", src.id));
        return None;
    };
    let mut mesh = Mesh {
        pos: Vec::new(),
        rgb: Vec::new(),
        team: Vec::new(),
    };
    for prim in gmesh.primitives() {
        if prim.mode() != gltf::mesh::Mode::Triangles {
            p.add(format!("{} {name}: non-triangle primitive", src.id));
            return None;
        }
        let reader = prim.reader(|b| Some(&buffers[b.index()]));
        let pos: Vec<[f32; 3]> = reader.read_positions()?.collect();
        let Some(colors) = reader.read_colors(0) else {
            p.add(format!(
                "{} {name}: COLOR_0 missing (rgb albedo + team alpha)",
                src.id
            ));
            return None;
        };
        let col: Vec<[f32; 4]> = colors.into_rgba_f32().collect();
        let indices: Vec<u32> = match reader.read_indices() {
            Some(it) => it.into_u32().collect(),
            None => (0..pos.len() as u32).collect(),
        };
        for i in indices {
            let (v, c) = (pos[i as usize], col[i as usize]);
            mesh.pos
                .push(std::array::from_fn(|k| (v[k] - src.pivot[k]) * src.scale));
            mesh.rgb.push([c[0], c[1], c[2]]);
            mesh.team.push(c[3]);
        }
    }
    Some(mesh)
}

/// Finite, non-degenerate triangles; palette colors only, team mask only on "team",
/// every used color declared in `materials`.
pub fn check_mesh(
    palette: &BTreeMap<String, [f32; 3]>,
    id: &str,
    lod: &str,
    materials: &[String],
    mesh: &Mesh,
    p: &mut Problems,
) {
    let mut used = BTreeSet::new();
    for t in 0..mesh.triangles() {
        let [a, b, c] = [mesh.pos[3 * t], mesh.pos[3 * t + 1], mesh.pos[3 * t + 2]];
        if ![a, b, c].iter().flatten().all(|v| v.is_finite()) {
            p.once(&format!("{id} {lod} finite"), format!("non-finite position in triangle {t}"));
            continue;
        }
        let n = cross(sub(b, a), sub(c, a));
        if (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() < 1e-9 {
            p.once(&format!("{id} {lod} degenerate"), format!("degenerate triangle {t} at {a:?}"));
        }
        for k in 0..3 {
            let (rgb, team) = (mesh.rgb[3 * t + k], mesh.team[3 * t + k]);
            if team != 0.0 && team != 1.0 {
                p.once(&format!("{id} {lod} team"), format!("team mask must be 0 or 1, got {team}"));
            }
            match nearest(palette, rgb) {
                Some(name) => {
                    used.insert(name);
                    if (team == 1.0) != (name == "team") {
                        p.once(&format!("{id} {lod} team"), format!("team mask {team} on palette color {name}"));
                    }
                }
                None => p.once(&format!("{id} {lod} palette"), format!("color {rgb:?} is not a palette color")),
            }
        }
    }
    let declared: BTreeSet<&str> = materials.iter().map(String::as_str).collect();
    for name in &used {
        if !declared.contains(name) {
            p.once(&format!("{id} {lod} materials"), format!("uses {name}, not listed in materials"));
        }
    }
}

fn nearest(palette: &BTreeMap<String, [f32; 3]>, rgb: [f32; 3]) -> Option<&str> {
    palette
        .iter()
        .find(|(_, c)| (0..3).all(|i| (c[i] - rgb[i]).abs() <= PALETTE_EPS))
        .map(|(name, _)| name.as_str())
}

pub fn check_palette(palette: &BTreeMap<String, [f32; 3]>, p: &mut Problems) {
    for (name, rgb) in palette {
        if !rgb.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) {
            p.add(format!("palette {name}: color {rgb:?} out of gamut"));
        }
    }
    if palette.get("team") != Some(&[1.0, 1.0, 1.0]) {
        p.add("palette must define team = [1, 1, 1] (the owner-color placeholder)".into());
    }
}

pub fn extent(pos: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    for v in pos {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    (lo, hi)
}

pub fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Appends the mesh as UnitVertex records (pos, flat normal, rgb, team), little endian.
pub fn write_vertices(bytes: &mut Vec<u8>, mesh: &Mesh) {
    for t in 0..mesh.triangles() {
        let [a, b, c] = [mesh.pos[3 * t], mesh.pos[3 * t + 1], mesh.pos[3 * t + 2]];
        let n = cross(sub(b, a), sub(c, a));
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let n = [n[0] / len, n[1] / len, n[2] / len];
        for k in 0..3 {
            let i = 3 * t + k;
            let floats = mesh.pos[i]
                .into_iter()
                .chain(n)
                .chain(mesh.rgb[i])
                .chain([mesh.team[i]]);
            for f in floats {
                bytes.extend_from_slice(&f.to_le_bytes());
            }
        }
    }
}

/// Every GLB under `dir` (one level) must be declared; "._*" AppleDouble files are ignored.
pub fn check_stray_files(dir: &Path, declared: &BTreeSet<std::path::PathBuf>, label: &str, p: &mut Problems) {
    let Ok(items) = std::fs::read_dir(dir) else {
        return;
    };
    for item in items.flatten() {
        let path = item.path();
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with("._") || path.extension().and_then(|x| x.to_str()) != Some("glb") {
            continue;
        }
        if !declared.contains(&path) {
            p.add(format!("{label}/{name} is not declared in manifest.json"));
        }
    }
}
