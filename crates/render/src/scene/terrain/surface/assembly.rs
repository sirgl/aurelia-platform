//! Assembly of the regions and stage 5 of the field, protection (platform/MESHING.md §2.2).
//!
//! Regions are appended in order; a seam vertex then has one copy per region
//! around it, at bit-identical XZ. Everything stage 5 decides about a point
//! (the water level at a wet/wet junction, the ground clamped under water,
//! the wet/dry contact, the shore distance, whether massif relief may rise)
//! is decided for the XZ point over the faces of all regions, so the copies of
//! a seam vertex stay identical and no crack opens.
//!
//! Every decision about a point reads only the faces around it, or the wet/dry
//! contacts within 3 world units: faces of its own region and of the eight around it
//! (a region is wider than 24 units). So the final data of a region depends on its
//! own part and its neighbours' parts only, and is assembled again only when one of
//! them changed (`assemble_regions`, `RegionFinal`). The others are copied from the
//! mesh of the last build (`RegionSource::Kept`): the builder holds no second copy.
use super::{
    field::{FieldSample, Landscape},
    hydrology, index, MountainVertex, RegionId, RegionPart, SurfaceMesh, SurfaceRegion, SurfaceWaterKind, QUERY_BIN,
};
use crate::{scene::fog::fog_at_position, world::state::RenderWorld};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

/// A vertex position in XZ, bit for bit: one point however many copies it has.
type PointKey = (u32, u32);
/// No water copy of a ground vertex yet.
const NO_WATER: u32 = u32::MAX;

fn key(position: [f32; 3]) -> PointKey {
    (position[0].to_bits(), position[2].to_bits())
}

impl SurfaceMesh {
    pub(super) fn assemble(&mut self, field: &Landscape, world: &RenderWorld, ids: &[RegionId], parts: &[std::sync::Arc<RegionPart>]) {
        let vertex_count: usize = parts.iter().map(|p| p.samples.len()).sum();
        let face_count: usize = parts.iter().map(|p| p.faces.len()).sum();
        let mut samples: Vec<FieldSample> = Vec::with_capacity(vertex_count);
        self.vertices.reserve(vertex_count + vertex_count / 4);
        self.land_triangles.reserve(face_count);
        let mut wet_by_region = Vec::with_capacity(parts.len());
        for (&id, part) in ids.iter().zip(parts) {
            let base = self.vertices.len() as u32;
            self.vertices.extend(part.samples.iter().map(|s| s.vertex));
            samples.extend_from_slice(&part.samples);
            let start = self.land_triangles.len() as u32;
            let mut wet = vec![];
            for &(triangle, kind) in &part.faces {
                let triangle = triangle.map(|i| i + base);
                self.land_triangles.push(triangle);
                if kind != SurfaceWaterKind::None {
                    wet.push((triangle, kind));
                }
            }
            let end = self.land_triangles.len() as u32;
            let bounds = [[f32::INFINITY; 2], [f32::NEG_INFINITY; 2]];
            let vertices = base..self.vertices.len() as u32;
            let (land_bins, water_bins, height) = Default::default();
            self.regions.push(SurfaceRegion { id, vertices, land: start..end, water: 0..0, bounds, height, land_bins, water_bins });
            wet_by_region.push(wet);
        }
        // The water copy of each ground vertex, per water kind (sea, river).
        let mut water_ids = vec![[NO_WATER; 2]; self.vertices.len()];
        let mut wet_pairs = vec![];
        for (region, wet) in wet_by_region.into_iter().enumerate() {
            let start = self.water_triangles.len() as u32;
            for (ground, kind) in wet {
                let water = ground.map(|id| self.water_vertex(&mut water_ids, id, kind, field, world));
                self.water_triangles.push(water);
                wet_pairs.push((ground, water));
            }
            self.regions[region].water = start..self.water_triangles.len() as u32;
        }
        // A wet/dry edge is a shared physical contact, not a floating water decal.
        self.join_water_contacts(&wet_pairs);
        self.protect(&samples, &wet_pairs);
    }

    /// Assembles the regions `ids` with their parts and returns the final data of the
    /// regions marked in `keep`. A kept region whose neighbours are all among `ids`
    /// comes out exactly as in an assembly of the whole map.
    pub(super) fn assemble_regions(
        field: &Landscape,
        world: &RenderWorld,
        ids: &[RegionId],
        parts: &[std::sync::Arc<RegionPart>],
        keep: &[bool],
    ) -> Vec<Option<RegionFinal>> {
        let mut context = SurfaceMesh::default();
        context.assemble(field, world, ids, parts);
        let mut ground = Vec::with_capacity(parts.len());
        let mut base = 0u32;
        for part in parts {
            ground.push(base..base + part.samples.len() as u32);
            base += part.samples.len() as u32;
        }
        let kept: Vec<usize> = (0..ids.len()).filter(|&r| keep[r]).collect();
        let finals = super::parallel_map(&kept, |&r| context.region_final(r, ground[r].clone()));
        let mut out: Vec<Option<RegionFinal>> = (0..ids.len()).map(|_| None).collect();
        for (r, region) in kept.into_iter().zip(finals) {
            out[r] = Some(region);
        }
        out
    }

    /// Region `r` of this mesh on its own: its ground vertices (`ground`, in part
    /// order), then its water vertices in the order its water faces first use them.
    fn region_final(&self, r: usize, ground: std::ops::Range<u32>) -> RegionFinal {
        let region = &self.regions[r];
        let mut vertices = Vec::with_capacity(ground.len());
        let mut mountain = Vec::with_capacity(ground.len());
        for i in ground.clone() {
            vertices.push(self.vertices[i as usize]);
            mountain.push(self.mountain[i as usize]);
        }
        let land: Vec<[u32; 3]> =
            self.land_triangles[region.land.start as usize..region.land.end as usize].iter().map(|t| t.map(|i| i - ground.start)).collect();
        let mut local: HashMap<u32, u32> = HashMap::default();
        let mut water = Vec::with_capacity(region.water.len());
        for t in &self.water_triangles[region.water.start as usize..region.water.end as usize] {
            water.push(t.map(|i| {
                *local.entry(i).or_insert_with(|| {
                    vertices.push(self.vertices[i as usize]);
                    mountain.push(self.mountain[i as usize]);
                    (vertices.len() - 1) as u32
                })
            }));
        }
        RegionFinal::new(region.id, vertices, mountain, land, water)
    }

    /// The surface of the regions' final data, appended in order. `previous` is the
    /// mesh the `Kept` slots refer to.
    pub(super) fn from_regions(layout: super::regions::Layout, sources: &[RegionSource], previous: Option<&SurfaceMesh>) -> SurfaceMesh {
        let mut mesh = SurfaceMesh { layout: Some(layout), ..SurfaceMesh::default() };
        let regions: Vec<RegionSlice<'_>> = sources.iter().map(|source| source.slice(previous)).collect();
        let vertices: usize = regions.iter().map(|r| r.vertices.len()).sum();
        mesh.vertices.reserve(vertices);
        mesh.mountain.reserve(vertices);
        for region in regions {
            mesh.append_region(region);
        }
        let mut height = [f32::INFINITY, f32::NEG_INFINITY];
        for region in &mesh.regions {
            height = [height[0].min(region.height[0]), height[1].max(region.height[1])];
        }
        mesh.height_range = height;
        mesh.region_slots = vec![None; (layout.columns() * layout.rows()) as usize];
        for (slot, region) in mesh.regions.iter().enumerate() {
            mesh.region_slots[layout.index(region.id)] = Some(slot as u32);
        }
        mesh
    }

    /// Appends one region's final data; its triangles index its vertices from `base`.
    fn append_region(&mut self, region: RegionSlice<'_>) {
        let start = self.vertices.len() as u32;
        let shift = |t: &[u32; 3]| t.map(|i| i - region.base + start);
        self.vertices.extend_from_slice(region.vertices);
        self.mountain.extend_from_slice(region.mountain);
        let land = self.land_triangles.len() as u32;
        self.land_triangles.extend(region.land.iter().map(shift));
        let water = self.water_triangles.len() as u32;
        self.water_triangles.extend(region.water.iter().map(shift));
        self.regions.push(SurfaceRegion {
            id: region.id,
            vertices: start..self.vertices.len() as u32,
            land: land..self.land_triangles.len() as u32,
            water: water..self.water_triangles.len() as u32,
            bounds: region.bounds,
            height: region.height,
            land_bins: region.land_bins.clone(),
            water_bins: region.water_bins.clone(),
        });
    }

    /// For every vertex, whether it is a ground copy of a point of a wet face
    /// in any region. Only such points are clamped, joined or contacts.
    fn wet_points(&self, wet: &[([u32; 3], [u32; 3])]) -> Vec<bool> {
        let points: HashSet<PointKey> =
            wet.iter().flat_map(|(ground, _)| ground.iter().map(|&i| key(self.vertices[i as usize].position))).collect();
        self.vertices.iter().map(|v| v.water_kind == SurfaceWaterKind::None && points.contains(&key(v.position))).collect()
    }

    /// The water copy of a ground vertex, one per water kind. Sea and river can
    /// meet at the same physical level; they retain distinct flow UVs.
    fn water_vertex(&mut self, ids: &mut [[u32; 2]], id: u32, kind: SurfaceWaterKind, field: &Landscape, world: &RenderWorld) -> u32 {
        let slot = if kind == SurfaceWaterKind::River { 1 } else { 0 };
        if ids[id as usize][slot] != NO_WATER {
            return ids[id as usize][slot];
        }
        let sample = field.water_for_kind(self.vertices[id as usize].position[0], self.vertices[id as usize].position[2], kind);
        // A constrained water face owns the contact: no dry vertex may protrude.
        let ground = &mut self.vertices[id as usize];
        ground.position[1] = ground.position[1].min(sample.level);
        let mut v = *ground;
        v.water_depth = (sample.level - v.position[1]).max(0.);
        v.position[1] = sample.level;
        v.water_kind = kind;
        v.coastal_rock = 0.;
        v.flow_uv = sample.flow;
        if kind != SurfaceWaterKind::River {
            v.tone = [0.; 3];
        }
        v.fog = fog_at_position(world, v.position[0], v.position[2]);
        let wet = self.vertices.len() as u32;
        self.vertices.push(v);
        ids[id as usize][slot] = wet;
        wet
    }

    pub(super) fn join_water_contacts(&mut self, wet: &[([u32; 3], [u32; 3])]) {
        self.join_water_levels(wet);
        let wet_points = self.wet_points(wet);
        self.join_ground_copies(&wet_points);
        let mut contact_levels: HashMap<PointKey, f32> = HashMap::default();
        let mut shore_edges = Vec::new();
        for ends in shore_contacts(&self.vertices, &self.land_triangles, wet, &wet_points) {
            for (ground, water) in ends {
                contact_levels.insert(key(self.vertices[ground as usize].position), self.vertices[water as usize].position[1]);
            }
            let endpoints = ends.map(|(_, water)| self.vertices[water as usize]);
            if endpoints.iter().all(|v| v.fog >= 0.55) {
                shore_edges.push(endpoints.map(|v| [v.position[0], v.position[2]]));
            }
        }
        for (v, _) in self.vertices.iter_mut().zip(&wet_points).filter(|(_, &wet)| wet) {
            if let Some(&level) = contact_levels.get(&key(v.position)) {
                v.position[1] = level;
            }
        }
        self.assign_shore_distances(&shore_edges);
        for &(ground, water) in wet {
            for i in 0..3 {
                let depth = self.vertices[water[i] as usize].position[1] - self.vertices[ground[i] as usize].position[1];
                self.vertices[water[i] as usize].water_depth = depth.max(0.);
            }
        }
    }

    /// Copies of one wet ground point in neighbouring regions take the lowest
    /// of their heights: each region clamped its copy under its own water
    /// faces. Dry copies come from the same field sample and already agree.
    fn join_ground_copies(&mut self, wet_points: &[bool]) {
        let mut lowest: HashMap<PointKey, f32> = HashMap::default();
        for (v, _) in self.vertices.iter().zip(wet_points).filter(|(_, &wet)| wet) {
            let y = lowest.entry(key(v.position)).or_insert(v.position[1]);
            *y = y.min(v.position[1]);
        }
        for (v, _) in self.vertices.iter_mut().zip(wet_points).filter(|(_, &wet)| wet) {
            v.position[1] = lowest[&key(v.position)];
        }
    }

    /// Distance to observed wet/dry mesh contacts, not depth or a wet/wet seam.
    fn assign_shore_distances(&mut self, edges: &[[[f32; 2]; 2]]) {
        const REACH: f32 = 3.0;
        let index = index::SegmentIndex::new(edges.iter().copied(), REACH);
        let water: Vec<u32> = (0..self.vertices.len() as u32).filter(|&i| self.vertices[i as usize].water_kind != SurfaceWaterKind::None).collect();
        let distances = super::parallel_map(&water, |&i| {
            let v = &self.vertices[i as usize];
            let p = [v.position[0], v.position[2]];
            let nearest = index.nearest(p, REACH, |e| {
                let [a, b] = edges[e as usize];
                hydrology::closest(p, a, b).0
            });
            -nearest.map_or(REACH, |n| n.distance)
        });
        for (i, distance) in water.into_iter().zip(distances) {
            self.vertices[i as usize].shore_distance = distance;
        }
    }

    /// Flow/material vertices may be duplicated, but physical water at one XZ
    /// has one height. Sea contact fixes the level; tributaries share the lower
    /// incident level. Reconcile before wet/dry banks and depth are finalized.
    fn join_water_levels(&mut self, wet: &[([u32; 3], [u32; 3])]) {
        let mut levels: HashMap<PointKey, (bool, f32)> = HashMap::default();
        for v in &self.vertices {
            if v.water_kind == SurfaceWaterKind::None {
                continue;
            }
            let sea = v.water_kind == SurfaceWaterKind::Sea;
            let level = levels.entry(key(v.position)).or_insert((sea, v.position[1]));
            level.0 |= sea;
            level.1 = if level.0 { 0.08 } else { level.1.min(v.position[1]) };
        }
        for v in &mut self.vertices {
            if v.water_kind != SurfaceWaterKind::None {
                v.position[1] = levels[&key(v.position)].1;
            }
        }
        for &(ground, water) in wet {
            for i in 0..3 {
                let level = self.vertices[water[i] as usize].position[1];
                let bed = &mut self.vertices[ground[i] as usize];
                bed.position[1] = bed.position[1].min(level);
            }
        }
    }

    /// Stage 5, protection: every point of a wet face, in any region, stays
    /// exactly at its contact level, so massif relief rises only on dry ground.
    /// XZ does not change, so the triangle query bins stay valid.
    fn protect(&mut self, samples: &[FieldSample], wet: &[([u32; 3], [u32; 3])]) {
        let mut protected = HashSet::default();
        for &(ground, water) in wet {
            for i in ground.into_iter().chain(water) {
                protected.insert(key(self.vertices[i as usize].position));
            }
        }
        self.mountain = vec![MountainVertex::default(); self.vertices.len()];
        for (i, sample) in samples.iter().enumerate() {
            if sample.relief.lift <= 0. || protected.contains(&key(self.vertices[i].position)) {
                continue;
            }
            let lifted = sample.lifted();
            self.vertices[i].position[1] = lifted.position[1];
            self.vertices[i].albedo = lifted.albedo;
            self.mountain[i] = sample.relief;
        }
    }

}

/// Where a region's final data comes from in a build.
pub(super) enum RegionSource {
    /// Assembled in this build.
    Fresh(RegionFinal),
    /// Unchanged since the last build: region `slot` of the mesh it returned.
    Kept(usize),
}

impl RegionSource {
    /// The region's data; `previous` is the mesh a kept region is in.
    fn slice<'a>(&'a self, previous: Option<&'a SurfaceMesh>) -> RegionSlice<'a> {
        match self {
            RegionSource::Fresh(f) => RegionSlice {
                id: f.id,
                vertices: &f.vertices,
                mountain: &f.mountain,
                land: &f.land,
                water: &f.water,
                base: 0,
                bounds: f.bounds,
                height: f.height,
                land_bins: &f.land_bins,
                water_bins: &f.water_bins,
            },
            RegionSource::Kept(slot) => {
                let from = previous.expect("kept regions come with the mesh they were kept in");
                let r = &from.regions[*slot];
                let range = r.vertices.start as usize..r.vertices.end as usize;
                RegionSlice {
                    id: r.id,
                    vertices: &from.vertices[range.clone()],
                    mountain: &from.mountain[range],
                    land: &from.land_triangles[r.land.start as usize..r.land.end as usize],
                    water: &from.water_triangles[r.water.start as usize..r.water.end as usize],
                    base: r.vertices.start,
                    bounds: r.bounds,
                    height: r.height,
                    land_bins: &r.land_bins,
                    water_bins: &r.water_bins,
                }
            }
        }
    }
}

/// One region's final data, wherever it is stored.
struct RegionSlice<'a> {
    id: RegionId,
    vertices: &'a [super::SurfaceVertex],
    mountain: &'a [MountainVertex],
    land: &'a [[u32; 3]],
    water: &'a [[u32; 3]],
    /// Index of the first of `vertices` in the triangles.
    base: u32,
    bounds: [hydrology::Point; 2],
    height: [f32; 2],
    land_bins: &'a index::SegmentIndex,
    water_bins: &'a index::SegmentIndex,
}

/// One assembled region on its own: its vertices with their final data, its faces
/// in its own indices, and its triangle index (platform/MESHING.md §2.6).
#[derive(Debug)]
pub struct RegionFinal {
    id: RegionId,
    vertices: Vec<super::SurfaceVertex>,
    mountain: Vec<MountainVertex>,
    land: Vec<[u32; 3]>,
    water: Vec<[u32; 3]>,
    /// XZ bounds of the ground faces.
    bounds: [hydrology::Point; 2],
    /// Lowest and highest vertex.
    height: [f32; 2],
    land_bins: index::SegmentIndex,
    water_bins: index::SegmentIndex,
}

impl RegionFinal {
    fn new(id: RegionId, vertices: Vec<super::SurfaceVertex>, mountain: Vec<MountainVertex>, land: Vec<[u32; 3]>, water: Vec<[u32; 3]>) -> Self {
        let boxes = |triangles: &[[u32; 3]]| -> Vec<[hydrology::Point; 2]> {
            triangles
                .iter()
                .map(|t| {
                    let p = t.map(|v| vertices[v as usize].position);
                    let low = [p[0][0].min(p[1][0]).min(p[2][0]), p[0][2].min(p[1][2]).min(p[2][2])];
                    let high = [p[0][0].max(p[1][0]).max(p[2][0]), p[0][2].max(p[1][2]).max(p[2][2])];
                    [low, high]
                })
                .collect()
        };
        let land_bins = index::SegmentIndex::new(boxes(&land), QUERY_BIN);
        let water_bins = index::SegmentIndex::new(boxes(&water), QUERY_BIN);
        let mut bounds = [[f32::INFINITY; 2], [f32::NEG_INFINITY; 2]];
        for p in land.iter().flatten().map(|&v| vertices[v as usize].position) {
            bounds[0] = [bounds[0][0].min(p[0]), bounds[0][1].min(p[2])];
            bounds[1] = [bounds[1][0].max(p[0]), bounds[1][1].max(p[2])];
        }
        let mut height = [f32::INFINITY, f32::NEG_INFINITY];
        for v in &vertices {
            height = [height[0].min(v.position[1]), height[1].max(v.position[1])];
        }
        Self { id, vertices, mountain, land, water, bounds, height, land_bins, water_bins }
    }
}

/// Ground edges of wet faces that border exactly one wet face and two ground
/// faces: the wet/dry contacts, as (ground, water) vertex pairs, in edge order.
/// Edges are keyed by their points, so an edge on a seam counts the faces of
/// both regions. Only a face with a wet point can share an edge with a wet face.
fn shore_contacts(vertices: &[super::SurfaceVertex], land: &[[u32; 3]], wet: &[([u32; 3], [u32; 3])], wet_points: &[bool]) -> Vec<[(u32, u32); 2]> {
    type EdgeKey = (PointKey, PointKey);
    let point = |i: u32| key(vertices[i as usize].position);
    let edge = |a: u32, b: u32| {
        let (ka, kb) = (point(a), point(b));
        (ka.min(kb), ka.max(kb))
    };
    // How many wet faces border each edge, and the vertex pairs of the first.
    let mut wet_edges: HashMap<EdgeKey, (u32, [(u32, u32); 2])> = HashMap::default();
    for &(g, w) in wet {
        for i in 0..3 {
            let j = (i + 1) % 3;
            let entry = wet_edges.entry(edge(g[i], g[j])).or_insert((0, [(g[i], w[i]), (g[j], w[j])]));
            entry.0 += 1;
        }
    }
    let mut land_count: HashMap<EdgeKey, u32> = HashMap::default();
    for t in land.iter().filter(|t| t.iter().filter(|&&i| wet_points[i as usize]).count() >= 2) {
        for i in 0..3 {
            let e = edge(t[i], t[(i + 1) % 3]);
            if wet_edges.get(&e).is_some_and(|w| w.0 == 1) {
                *land_count.entry(e).or_default() += 1;
            }
        }
    }
    let mut contacts: Vec<(EdgeKey, [(u32, u32); 2])> = wet_edges
        .into_iter()
        .filter(|(e, (count, _))| *count == 1 && land_count.get(e) == Some(&2))
        .map(|(e, (_, pair))| (e, pair))
        .collect();
    contacts.sort_unstable_by_key(|(e, _)| *e);
    contacts.into_iter().map(|(_, pair)| pair).collect()
}
