//! GPU adapters for the shared continuous surface, one set of meshes per region.
use super::buffers::{BorderVertex, ChunkMesh, TerrainVertex, WaterVertex, UV_TILING};
use crate::scene::fog::fog_at_position;
use crate::scene::terrain::surface::{RegionId, SurfaceMesh, SurfaceVertex, SurfaceWaterKind};
use crate::world::state::RenderWorld;
use glam::Vec3;
use std::collections::HashMap;

/// The alpha channel of an authored albedo tells the terrain shader how to
/// treat a face (platform/assets/shaders/terrain.wgsl). `1 + w` is ground with a
/// flagstone weight `w` in 0..=1; `WALL_MATERIAL` is a cliff wall whose
/// normal and color are final, so the shader adds no face facet of its own.
pub const GROUND_MATERIAL: f32 = 1.0;
pub const WALL_MATERIAL: f32 = 3.0;
/// Share of the wall-smoothed normal in a wall vertex normal: narrow fracture
/// columns shade like their neighbours, broad bends of the wall stay visible.
const WALL_SMOOTHING: f32 = 0.08;
/// Side of a wall panel in world units, see `facet_shade`.
const PANEL_SIZE: f32 = 0.32;

/// GPU-ready meshes of one surface region: its ground, its water and the hex grid
/// draped over it.
pub struct RegionMeshes {
    pub id: RegionId,
    pub land: ChunkMesh,
    pub water: Vec<WaterVertex>,
    pub grid: Vec<[f32; 3]>,
}

/// Meshes of the regions `which` (indices into `surface.regions`). `grid_lines` are the
/// hex edges of the known tiles (`build_grid_lines`); each goes to the region of its
/// middle. Regions are independent once their normals are summed (`Normals`), so they
/// are built on worker threads; the result is in the order of `which`.
pub fn region_meshes(surface: &SurfaceMesh, world: &RenderWorld, grid_lines: &[[f32; 3]], which: &[usize]) -> Vec<RegionMeshes> {
    let mut lines: HashMap<usize, Vec<[f32; 3]>> = which.iter().map(|&r| (r, Vec::new())).collect();
    for pair in grid_lines.chunks_exact(2) {
        let middle = [(pair[0][0] + pair[1][0]) * 0.5, (pair[0][2] + pair[1][2]) * 0.5];
        if let Some(slot) = surface.region_slot_of_point(middle[0], middle[1]).and_then(|r| lines.get_mut(&r)) {
            slot.extend_from_slice(pair);
        }
    }
    crate::scene::terrain::surface::parallel_map(which, |&r| {
        let region = &surface.regions[r];
        let normals = Normals::around(surface, r);
        let mut chunk = SurfaceChunk::new(surface.region_cells(r));
        for face in &surface.land_triangles[region.land.start as usize..region.land.end as usize] {
            chunk.push_face(surface, &normals, *face);
        }
        let water_faces = &surface.water_triangles[region.water.start as usize..region.water.end as usize];
        RegionMeshes {
            id: region.id,
            land: chunk.finish(),
            water: water_vertices(surface, water_faces),
            grid: grid(surface, world, &lines[&r]),
        }
    })
}

/// The whole ground as one mesh (meshes built without regions, tests).
pub fn whole(surface: &SurfaceMesh) -> ChunkMesh {
    let normals = Normals::over(surface, surface.land_triangles.iter().copied());
    let mut chunk = SurfaceChunk::new(0);
    for face in &surface.land_triangles {
        chunk.push_face(surface, &normals, *face);
    }
    chunk.finish()
}

/// Vertex normals of the ground, and of cliff walls only: a wall vertex averages the
/// walls around it, never the grass above the brow. Normals are summed per point: the
/// copies of a seam vertex in neighbouring regions sum the faces of both, so the two
/// sides shade alike.
struct Normals {
    ground: HashMap<(u32, u32), Vec3>,
    wall: HashMap<(u32, u32), Vec3>,
}

fn point_key(p: [f32; 3]) -> (u32, u32) {
    (p[0].to_bits(), p[2].to_bits())
}

impl Normals {
    /// Sums for every point of region `r`: its own faces and the faces of the
    /// neighbouring regions that reach its box (the only ones that can share a point).
    /// Faces are summed in region order, so every region adds the faces around a seam
    /// point in the same order and gets the same bits.
    fn around(surface: &SurfaceMesh, r: usize) -> Self {
        let [low, high] = surface.regions[r].bounds();
        let touches = |face: &[u32; 3]| {
            face.iter().any(|&i| {
                let p = surface.vertices[i as usize].position;
                p[0] >= low[0] && p[0] <= high[0] && p[2] >= low[1] && p[2] <= high[1]
            })
        };
        let mut slots = surface.neighbour_slots(r);
        slots.push(r);
        slots.sort_unstable();
        let faces = slots
            .into_iter()
            .flat_map(|s| surface.region_faces(s).iter().copied().filter(move |face| s == r || touches(face)));
        Self::over(surface, faces)
    }

    fn over(surface: &SurfaceMesh, faces: impl Iterator<Item = [u32; 3]>) -> Self {
        let mut ground: HashMap<(u32, u32), Vec3> = HashMap::new();
        let mut wall: HashMap<(u32, u32), Vec3> = HashMap::new();
        for face in faces {
            let [a, b, c] = face.map(|i| Vec3::from(surface.vertices[i as usize].position));
            let mut normal = (b - a).cross(c - a);
            if normal.y < 0.0 {
                normal = -normal;
            }
            let is_wall = mountain_face_normal(surface, face).is_none()
                && coastal_face_normal(surface, face).is_some_and(is_wall_normal);
            for &id in &face {
                let key = point_key(surface.vertices[id as usize].position);
                *ground.entry(key).or_default() += normal;
                if is_wall {
                    *wall.entry(key).or_default() += normal;
                }
            }
        }
        Self { ground, wall }
    }

    fn ground(&self, surface: &SurfaceMesh, id: u32) -> Vec3 {
        Self::resolve(&self.ground, surface, id)
    }

    fn wall(&self, surface: &SurfaceMesh, id: u32) -> Vec3 {
        Self::resolve(&self.wall, surface, id)
    }

    fn resolve(sums: &HashMap<(u32, u32), Vec3>, surface: &SurfaceMesh, id: u32) -> Vec3 {
        let key = point_key(surface.vertices[id as usize].position);
        sums.get(&key).and_then(|n| n.try_normalize()).unwrap_or(Vec3::Y)
    }
}

/// Coastal stone steeper than this is a wall, not a shelf or a rim.
fn is_wall_normal(normal: Vec3) -> bool {
    normal.y <= 0.78
}
struct SurfaceChunk {
    mesh: ChunkMesh,
    remap: HashMap<u32, u32>,
}
impl SurfaceChunk {
    fn new(tiles: usize) -> Self {
        Self {
            mesh: ChunkMesh {
                vertices: vec![],
                indices: vec![],
                center: [0.0; 3],
                radius: 0.0,
                tiles,
            },
            remap: HashMap::new(),
        }
    }
    fn push_face(&mut self, surface: &SurfaceMesh, normals: &Normals, face: [u32; 3]) {
        match FaceKind::of(surface, face) {
            FaceKind::Ground => {
                // Grass shares indexed vertices across chunks.
                for id in face {
                    let index = match self.remap.get(&id) {
                        Some(&index) => index,
                        None => {
                            let v = &surface.vertices[id as usize];
                            let index = self.push_vertex(v, normals.ground(surface, id), v.albedo, ground_material(v));
                            self.remap.insert(id, index);
                            index
                        }
                    };
                    self.mesh.indices.push(index);
                }
            }
            FaceKind::Faceted { normal, albedo } => {
                // Exposed geology splits normals; no triangle or draw is added.
                for id in face {
                    let v = &surface.vertices[id as usize];
                    let index = self.push_vertex(v, normal, albedo.unwrap_or(v.albedo), ground_material(v));
                    self.mesh.indices.push(index);
                }
            }
            FaceKind::Wall { normal } => {
                // Each facet its own shade of the rock: a faceted wall, not a sheet.
                let center = face.iter().fold(Vec3::ZERO, |sum, &id| sum + Vec3::from(surface.vertices[id as usize].position)) / 3.0;
                let facet = facet_shade(center);
                for id in face {
                    let v = &surface.vertices[id as usize];
                    let smooth = snap_wall_normal(normal.lerp(normals.wall(surface, id), WALL_SMOOTHING).normalize());
                    let rgb = wall_color(v.position).map(|c| c * facet);
                    let index = self.push_vertex(v, smooth, rgb, WALL_MATERIAL);
                    self.mesh.indices.push(index);
                }
            }
        }
    }
    fn push_vertex(&mut self, v: &SurfaceVertex, normal: Vec3, rgb: [f32; 3], material: f32) -> u32 {
        let index = u32::try_from(self.mesh.vertices.len()).expect("surface vertex count fits u32");
        self.mesh.vertices.push(TerrainVertex {
            pos: v.position,
            normal: normal.to_array(),
            // uv.x is the legacy texture coordinate; uv.y carries the fallout share (terrain.wgsl).
            uv: [v.position[0] / UV_TILING, v.fallout],
            blend_idx: 0,
            blend_w: 255,
            fog: v.fog,
            water: v.water,
            albedo: [rgb[0], rgb[1], rgb[2], material],
        });
        index
    }
    fn finish(mut self) -> ChunkMesh {
        if !self.mesh.vertices.is_empty() {
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for v in &self.mesh.vertices {
                lo = lo.min(v.pos.into());
                hi = hi.max(v.pos.into());
            }
            self.mesh.center = ((lo + hi) * 0.5).to_array();
            self.mesh.radius = (hi - lo).length() * 0.5 + 0.1;
        }
        self.mesh
    }
}

/// How a ground face is shaded: smooth grass, a faceted mountain or coastal
/// shelf, or a cliff wall.
enum FaceKind {
    Ground,
    Faceted { normal: Vec3, albedo: Option<[f32; 3]> },
    Wall { normal: Vec3 },
}

impl FaceKind {
    fn of(surface: &SurfaceMesh, face: [u32; 3]) -> Self {
        if let Some(normal) = mountain_face_normal(surface, face) {
            return Self::Faceted { normal, albedo: mountain_face_albedo(surface, face, normal) };
        }
        match coastal_face_normal(surface, face) {
            Some(normal) if is_wall_normal(normal) => Self::Wall { normal },
            Some(normal) => Self::Faceted { normal, albedo: None },
            None => Self::Ground,
        }
    }
}

/// Ground carries its flagstone weight to the shader; see `GROUND_MATERIAL`.
fn ground_material(v: &SurfaceVertex) -> f32 {
    GROUND_MATERIAL + v.coastal_rock.clamp(0., 1.)
}

fn mountain_face_normal(surface: &SurfaceMesh, face: [u32; 3]) -> Option<Vec3> {
    if !face.iter().any(|&i| surface.mountain.get(i as usize).map_or(0., |m| m.lift) > 0.075) {
        return None;
    }
    let [a, b, c] = face.map(|i| Vec3::from(surface.vertices[i as usize].position));
    let mut normal = (b - a).cross(c - a).try_normalize()?;
    if normal.y < 0. { normal = -normal; }
    Some(normal)
}

/// A snow boundary follows complete low-poly faces, with no interpolated white
/// streaks down adjacent rock. Low-lift feet retain their vertex grass blend.
fn mountain_face_albedo(surface: &SurfaceMesh, face: [u32; 3], normal: Vec3) -> Option<[f32; 3]> {
    let material = face.map(|i| surface.mountain.get(i as usize).copied().unwrap_or_default());
    if material.iter().map(|v| v.snow).sum::<f32>() >= 1.5 && normal.y > 0.43 {
        return Some([0.92, 0.94, 0.95]);
    }
    if material.iter().any(|v| v.lift < 0.18) { return None; }
    let center = face.map(|i| surface.vertices[i as usize].position);
    Some(crate::scene::terrain::mountains::rock_color([
        center.iter().map(|v| v[0]).sum::<f32>() / 3.,
        center.iter().map(|v| v[2]).sum::<f32>() / 3.,
    ]))
}

fn coastal_face_normal(surface: &SurfaceMesh, face: [u32; 3]) -> Option<Vec3> {
    let vertices = face.map(|i| &surface.vertices[i as usize]);
    if vertices.iter().any(|v| v.water_kind != SurfaceWaterKind::None) {
        return None;
    }
    let stones = vertices.iter().filter(|v| v.coastal_rock >= 0.5).count();
    let [a, b, c] = vertices.map(|v| Vec3::from(v.position));
    let mut normal = (b - a).cross(c - a).try_normalize()?;
    if normal.y < 0. { normal = -normal; }
    (stones >= 2 && normal.y < 0.94).then_some(normal)
}

/// Wall stone by height above the sea, per vertex: cool blue-gray rock low
/// on the wall, warmer sunlit stone toward the brow. The color depends on
/// height and broad mineral bands only, so narrow fracture columns never
/// alternate in color. The thin wet band at the waterline is drawn per pixel
/// by the terrain shader: no wall row lies that low.
fn wall_color(p: [f32; 3]) -> [f32; 3] {
    const SEA_LEVEL: f32 = 0.08;
    let sunlit = smoothstep(0.05, 0.45, p[1] - SEA_LEVEL);
    let rock = crate::scene::terrain::mountains::rock_color([p[0], p[2]]);
    // Darker and cooler than the sunlit slabs above: slate low, grey-blue high.
    let cool = [0.36, 0.39, 0.46];
    let warm = [0.62, 0.60, 0.56];
    std::array::from_fn(|k| ((cool[k] * (1. - sunlit) + warm[k] * sunlit) * 0.7 + rock[k] * 0.3) * 0.91)
}

/// Brightness of one wall facet. Slivers inside one broad panel (a cell of
/// `PANEL_SIZE`) share most of their shade, so the wall reads as a few planar
/// faces of different brightness, with a faint per-triangle variation.
fn facet_shade(center: Vec3) -> f32 {
    let hash = |x: f32, z: f32, y: f32| {
        let key = x.to_bits() ^ z.to_bits().rotate_left(11) ^ y.to_bits().rotate_left(22);
        crate::scene::terrain::fields::hash_u32(key) % 1000
    };
    let panel = hash((center.x / PANEL_SIZE).floor(), (center.z / PANEL_SIZE).floor(), (center.y / PANEL_SIZE).floor());
    let own = hash(center.x * 37.0, center.z * 53.0, center.y * 71.0);
    0.84 + panel as f32 / 1000. * 0.26 + own as f32 / 1000. * 0.06
}

/// Wall normals snap to steps of azimuth and slope: neighbouring slivers of one
/// panel light up alike, as the flat faces of a low-poly cliff do.
fn snap_wall_normal(normal: Vec3) -> Vec3 {
    const AZIMUTH_STEP: f32 = std::f32::consts::TAU / 16.;
    const SLOPE_STEP: f32 = 0.10;
    let flat = Vec3::new(normal.x, 0., normal.z).try_normalize().unwrap_or(Vec3::X);
    let azimuth = (flat.z.atan2(flat.x) / AZIMUTH_STEP).round() * AZIMUTH_STEP;
    let up = (normal.y / SLOPE_STEP).round() * SLOPE_STEP;
    let out = (1. - up * up).max(0.).sqrt();
    Vec3::new(azimuth.cos() * out, up, azimuth.sin() * out).normalize()
}

fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// Water vertices of the given water faces, three per face.
pub fn water_vertices(surface: &SurfaceMesh, faces: &[[u32; 3]]) -> Vec<WaterVertex> {
    faces
        .iter()
        .flat_map(|face| face.iter())
        .map(|&i| {
            let v = &surface.vertices[i as usize];
            let sea = v.water_kind == SurfaceWaterKind::Sea;
            // Shade: 1 sea, 0.75 lake (calm, greener, no surf), 0 river.
            let shade = match (sea, sea && surface.is_lake(v.position[0], v.position[2])) {
                (true, true) => 0.75,
                (true, false) => 1.0,
                // A river's water at its mouth names the water it flows into (the shader
                // blends to it): 0.2 the sea, 0.3 a lake. Elsewhere 0.
                _ if v.tone[0] > 0.0 => {
                    let near_lake = (0..6).any(|k| {
                        let a = k as f32 * std::f32::consts::FRAC_PI_3;
                        surface.is_lake(v.position[0] + 0.6 * a.cos(), v.position[2] + 0.6 * a.sin())
                    });
                    if near_lake { 0.3 } else { 0.2 }
                }
                _ => 0.0,
            };
            let across =
                (1.0 - v.water_depth.max(0.0) / if sea { 0.8 } else { 0.25 }).clamp(0.0, 1.0);
            WaterVertex {
                pos: [v.position[0], v.position[1] + 0.003, v.position[2]],
                uv: [v.flow_uv[0], 0.5 + 0.5 * if sea { across } else { v.flow_uv[1] }],
                fog: v.fog,
                shade,
                shore: (-v.shore_distance).max(0.0),
                tone: v.tone,
            }
        })
        .collect()
}

fn draped(surface: &SurfaceMesh, p: [f32; 3], lift: f32) -> [f32; 3] {
    let ground = surface.sample_ground(p[0], p[2]).map_or(p[1], |s| s.y);
    let water = surface
        .sample_water(p[0], p[2])
        .map_or(ground, |s| s.y.max(ground));
    [p[0], water + lift, p[2]]
}

pub fn grid(surface: &SurfaceMesh, world: &RenderWorld, lines: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    for pair in lines.chunks_exact(2) {
        let a = Vec3::from(pair[0]);
        let b = Vec3::from(pair[1]);
        let steps = ((a - b).length() / 0.12).ceil().max(1.0) as usize;
        for step in 0..steps {
            let p = a.lerp(b, step as f32 / steps as f32);
            let q = a.lerp(b, (step + 1) as f32 / steps as f32);
            if fog_at_position(world, p.x, p.z).min(fog_at_position(world, q.x, q.z)) < 0.55 {
                continue;
            }
            // Rivers already express a tile edge. A draped line through their
            // centre looked like a seam in the water; ocean hexes remain visible.
            let mid = (p + q) * 0.5;
            if surface.water_kind_at(mid.x, mid.z) == SurfaceWaterKind::River {
                continue;
            }
            out.extend([
                draped(surface, p.to_array(), 0.022),
                draped(surface, q.to_array(), 0.022),
            ]);
        }
    }
    out
}

pub fn subdivide_lines(lines: &[BorderVertex]) -> Vec<BorderVertex> {
    let mut out = Vec::new();
    for pair in lines.chunks_exact(2) {
        let a = Vec3::from(pair[0].pos);
        let b = Vec3::from(pair[1].pos);
        let steps = ((a - b).length() / 0.15).ceil().max(1.0) as usize;
        for step in 0..steps {
            for k in [step, step + 1] {
                let t = k as f32 / steps as f32;
                out.push(BorderVertex {
                    pos: a.lerp(b, t).to_array(),
                    color: Vec3::from(pair[0].color)
                        .lerp(pair[1].color.into(), t)
                        .to_array(),
                });
            }
        }
    }
    out
}

pub fn drape_ribbon(surface: &SurfaceMesh, vertices: &mut [BorderVertex], lift: f32) {
    for vertex in vertices {
        vertex.pos = draped(surface, vertex.pos, lift);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vertex(position: [f32; 3]) -> SurfaceVertex {
        SurfaceVertex {
            position,
            albedo: [0.5; 3],
            coastal_rock: 0.,
            fallout: 0.,
            terrain: 1,
            water: 0.0,
            water_depth: 0.0,
            shore_distance: 1.0,
            fog: 1.0,
            water_kind: SurfaceWaterKind::None,
            flow_uv: [0.0; 2],
            tone: [0.; 3],
        }
    }
    #[test]
    fn cliff_faces_split_normals_without_changing_positions_or_triangle_count() {
        let mut surface = SurfaceMesh::default();
        surface.vertices = [[2., 0., 2.], [2., 1., 2.2], [3., 1., 2.2], [3., 0., 3.]]
            .map(|p| { let mut v = vertex(p); v.shore_distance = 1.2; v.coastal_rock = 0.8; v.albedo = [0.66, 0.50, 0.30]; v }).to_vec();
        surface.land_triangles = vec![[0, 1, 2], [0, 2, 3]];
        let chunk = &whole(&surface);
        assert_eq!(chunk.indices.len(), 6);
        assert_eq!(chunk.vertices.len(), 6, "wall faces own their vertices");
        for v in &chunk.vertices {
            assert!(surface.vertices.iter().any(|original| original.position == v.pos));
            assert_eq!(v.albedo[3], WALL_MATERIAL);
        }
        // Vertex 0 is shared by both faces: its normals differ by face, but
        // a little less than the two face normals do: faceted, without hard stripes.
        let face_normal = |f: [u32; 3]| {
            let [a, b, c] = f.map(|i| Vec3::from(surface.vertices[i as usize].position));
            let n = (b - a).cross(c - a).normalize();
            if n.y < 0. { -n } else { n }
        };
        let faces_apart = face_normal([0, 1, 2]).angle_between(face_normal([0, 2, 3]));
        let first = Vec3::from(chunk.vertices[chunk.indices[0] as usize].normal);
        let second = Vec3::from(chunk.vertices[chunk.indices[3] as usize].normal);
        assert!(first.angle_between(second) > 1e-3);
        // Normals sit on the snap grid (`snap_wall_normal`): broad panels, not stripes.
        for n in [first, second] {
            assert!(n.angle_between(snap_wall_normal(n)) < 1e-3, "wall normals are snapped");
        }
        assert!(first.angle_between(second) < 1.5 * faces_apart, "snapping moves a normal by at most one step");
        let low = chunk.vertices.iter().find(|v| v.pos[1] == 0.).unwrap().albedo;
        let top = chunk.vertices.iter().find(|v| v.pos[1] == 1.).unwrap().albedo;
        assert!(low[0] < top[0] && low[2] / low[0] > top[2] / top[0], "cool low rock, warm sunlit top");
    }

    #[test]
    fn coastal_material_excludes_gray_ground_water_and_low_coverage() {
        let mut surface = SurfaceMesh::default();
        surface.vertices = [[0., 0., 0.], [0., 1., 0.2], [1., 1., 0.2]]
            .map(vertex).to_vec();
        assert!(coastal_face_normal(&surface, [0, 1, 2]).is_none(),
            "gray albedo alone does not identify coastal rock");
        surface.vertices[0].coastal_rock = 0.8;
        assert!(coastal_face_normal(&surface, [0, 1, 2]).is_none(),
            "one rock vertex keeps the grass transition smooth");
        surface.vertices[1].coastal_rock = 0.8;
        assert!(coastal_face_normal(&surface, [0, 1, 2]).is_some(),
            "explicit rock is recognized beyond the old shore cutoff");
        surface.vertices[2].water_kind = SurfaceWaterKind::Sea;
        assert!(coastal_face_normal(&surface, [0, 1, 2]).is_none(),
            "water faces never inherit rock faceting metadata");
        assert_eq!(std::mem::size_of::<TerrainVertex>(), 64);
    }

    #[test]
    fn mountain_metadata_facets_the_same_ground_without_new_faces_or_gpu_fields() {
        let mut surface = SurfaceMesh::default();
        surface.vertices = [[2., 0., 2.], [2., 1., 2.2], [3., 1., 2.2], [3., 0., 3.]]
            .map(|p| { let mut v = vertex(p); v.shore_distance = 4.; v.albedo = [0.3, 0.6, 0.2]; v }).to_vec();
        surface.land_triangles = vec![[0, 1, 2], [0, 2, 3]];
        surface.mountain = [0., 1., 1., 0.].map(|lift| crate::scene::terrain::surface::MountainVertex { lift, snow: 0. }).to_vec();
        let chunk = &whole(&surface);
        assert_eq!(chunk.indices.len(), 6);
        for face in chunk.indices.chunks_exact(3) {
            let v = face.iter().map(|i| &chunk.vertices[*i as usize]).collect::<Vec<_>>();
            assert_eq!(v[0].normal, v[1].normal);
            assert_eq!(v[1].normal, v[2].normal);
            assert!(v[0].normal[1] >= 0.);
            for vertex in v { assert!(surface.vertices.iter().any(|p| p.position == vertex.pos)); }
        }
        assert_ne!(chunk.vertices[chunk.indices[0] as usize].normal,
                   chunk.vertices[chunk.indices[3] as usize].normal);
        assert_eq!(std::mem::size_of::<TerrainVertex>(), 64);
    }

    #[test]
    fn mountain_snow_is_a_crisp_face_material_and_grass_foot_stays_blended() {
        use crate::scene::terrain::surface::MountainVertex;
        let mut surface = SurfaceMesh::default();
        surface.vertices = [[2., 0., 2.], [2., 0.3, 3.], [3., 0.3, 3.], [3., 0., 2.]]
            .map(|p| { let mut v = vertex(p); v.shore_distance = 4.; v.albedo = [0.3, 0.6, 0.2]; v }).to_vec();
        surface.land_triangles = vec![[0, 1, 2], [0, 2, 3]];
        surface.mountain = [1., 1., 0., 0.].map(|snow| MountainVertex { lift: 1., snow }).to_vec();
        let chunk = &whole(&surface);
        let colors: Vec<_> = chunk.indices.chunks_exact(3).map(|f| {
            let c = f.iter().map(|i| chunk.vertices[*i as usize].albedo).collect::<Vec<_>>();
            assert_eq!(c[0], c[1]); assert_eq!(c[1], c[2]); c[0]
        }).collect();
        assert_eq!(colors[0], [0.92, 0.94, 0.95, 1.]);
        assert!(colors[1][0] < 0.6, "one snowy vertex cannot smear white across the rock face");
        assert_eq!(chunk.indices.len(), 6);
        assert!(mountain_face_albedo(&surface, [0, 1, 2], Vec3::new(0.9, 0.3, 0.)).unwrap()[0] < 0.6,
            "steep snow-covered candidates expose rock");
        surface.mountain[3].lift = 0.04;
        assert!(mountain_face_albedo(&surface, [0, 2, 3], Vec3::Y).is_none(),
            "low mountain foot preserves authored grass/rock vertex blend");
    }

    /// The meshes of all regions hold every ground face once, and the copies of a seam
    /// point in neighbouring regions get bit-identical smooth normals: the normal of a
    /// point sums the faces of every region around it.
    #[test]
    fn region_meshes_keep_every_face_once_and_share_seam_normals() {
        let style = crate::scene::style::compile(crate::scene::style::default_style()).unwrap();
        let world = crate::scene::terrain::surface::fixtures::island();
        let surface = crate::scene::terrain::surface::build_in_regions(&world, &style, 8);
        assert!(surface.regions.len() > 4, "the fixture spans several regions");
        let all: Vec<usize> = (0..surface.regions.len()).collect();
        let meshes = region_meshes(&surface, &world, &[], &all);
        let position_key = |p: [f32; 3]| p.map(f32::to_bits);
        let mut expected: Vec<_> = surface
            .land_triangles
            .iter()
            .map(|t| t.map(|i| position_key(surface.vertices[i as usize].position)))
            .collect();
        let mut actual: Vec<_> = meshes
            .iter()
            .flat_map(|m| m.land.indices.chunks_exact(3).map(|f| std::array::from_fn(|i| position_key(m.land.vertices[f[i] as usize].pos))))
            .collect();
        expected.sort();
        actual.sort();
        assert_eq!(actual, expected, "no duplicated or lost face");
        // The normal of every point a region sums is the one the whole ground gives:
        // the copies of a seam point in two regions shade alike, bit for bit.
        let global = Normals::over(&surface, surface.land_triangles.iter().copied());
        let mut seams = 0;
        for r in 0..surface.regions.len() {
            let local = Normals::around(&surface, r);
            for &id in surface.region_faces(r).iter().flatten() {
                let (a, b) = (local.ground(&surface, id), global.ground(&surface, id));
                assert_eq!(a.to_array().map(f32::to_bits), b.to_array().map(f32::to_bits), "region {r} vertex {id}");
                let (a, b) = (local.wall(&surface, id), global.wall(&surface, id));
                assert_eq!(a.to_array().map(f32::to_bits), b.to_array().map(f32::to_bits), "region {r} wall vertex {id}");
            }
            seams += surface.neighbour_slots(r).len();
        }
        assert!(seams > 8, "the fixture has seams: {seams}");
        // One region alone gets the same meshes as with all the others.
        let one = region_meshes(&surface, &world, &[], &[3]);
        assert_eq!(bytemuck::cast_slice::<_, u8>(&one[0].land.vertices), bytemuck::cast_slice::<_, u8>(&meshes[3].land.vertices));
    }

    #[test]
    fn empty_mesh_has_no_ground() {
        assert!(whole(&SurfaceMesh::default()).vertices.is_empty());
        assert!(region_meshes(&SurfaceMesh::default(), &RenderWorld::default(), &[], &[]).is_empty());
    }
}
