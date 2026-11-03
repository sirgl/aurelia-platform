//! Continuous presentation surface. Logical hexes supply semantics, never triangles.
//!
//! Build (platform/MESHING.md §2): the field's contributors are derived once from the
//! world observation (`field.rs`); each known region is triangulated on its
//! own, with canonical seams (`regions.rs`), possibly in parallel; the
//! regions are assembled and stage 5 runs over the whole (`assembly.rs`).
//! Queries go through the triangle index of the region under the point.
use super::evaluator::SurfaceSample;
use crate::{scene::style::CompiledTerrainStyle, world::state::RenderWorld};
mod assembly;
mod banks;
mod biomes;
mod cliffs;
mod cutouts;
mod field;
mod fingerprint;
/// Test and bench worlds: the sandbox island.
pub mod fixtures;
/// Windows of real maps around river mouths and sources: test and review worlds.
pub mod mouths;
mod hydrology;
mod index;
mod massifs;
mod pads;
mod reaches;
mod regions;
mod sizing;
mod sites;
mod stacks;
mod walls;
mod transitions;
use field::Landscape;
pub use regions::{Levels, RegionId, HALO_RINGS, MAX_LEVEL, REGION};
pub use sites::{SiteShape, WonderSite};
pub use stacks::{CoastProp, CoastPropKind};
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceWaterKind {
    None,
    Sea,
    River,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceVertex {
    pub position: [f32; 3],
    pub albedo: [f32; 3],
    /// CPU-only exposed coastal stone coverage, independent of color and shore distance.
    pub coastal_rock: f32,
    /// Share of the fallout material at the point (0..1): the shader animates it.
    pub fallout: f32,
    pub terrain: u16,
    pub water: f32,
    pub water_depth: f32,
    /// World distance, positive on dry land, negative inside sea/river.
    pub shore_distance: f32,
    pub fog: f32,
    pub water_kind: SurfaceWaterKind,
    /// River longitudinal distance and signed normalized cross-section.
    pub flow_uv: [f32; 2],
    /// What the water of a river there shows: the weight of the river mouth (the sea's own
    /// colour at 1), the position on the water palette and the weight of white water.
    /// Zero away from a river and on the sea.
    pub tone: [f32; 3],
}
#[derive(Debug, Clone, Copy)]
pub struct SurfaceRiverAnchor {
    pub position: [f32; 3],
    pub tangent: [f32; 2],
    pub half_width: f32,
}
/// CPU material metadata; never changes the GPU vertex layout.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MountainVertex {
    pub lift: f32,
    pub snow: f32,
}

/// One built region of the assembled mesh: its triangle ranges and index.
/// A seam vertex has one copy in each region around it, at the same position.
#[derive(Debug, Clone)]
pub struct SurfaceRegion {
    pub id: RegionId,
    /// The region's own vertices (ground, then water) in a mesh of `from_regions`; its
    /// triangles use only these. Assembly contexts hold the ground vertices only.
    vertices: Range<u32>,
    pub land: Range<u32>,
    pub water: Range<u32>,
    /// XZ bounds of the region's triangles.
    bounds: [hydrology::Point; 2],
    /// Lowest and highest vertex.
    height: [f32; 2],
    land_bins: index::SegmentIndex,
    water_bins: index::SegmentIndex,
}

impl SurfaceRegion {
    /// XZ bounds of the region's triangles (low, high).
    pub fn bounds(&self) -> [[f32; 2]; 2] {
        self.bounds
    }
}

#[derive(Debug, Clone, Default)]
pub struct SurfaceMesh {
    pub vertices: Vec<SurfaceVertex>,
    /// Includes underwater ground; triangles are counter-clockwise viewed from above.
    pub land_triangles: Vec<[u32; 3]>,
    pub water_triangles: Vec<[u32; 3]>,
    /// Geological lift at each vertex; drives faceted mountain normals without extra faces.
    pub mountain: Vec<MountainVertex>,
    /// Rock objects of the cliffs, grounded by their GPU adapter with the mesh queries.
    pub coast_props: Vec<CoastProp>,
    /// Known natural wonder sites and the ground level their models stand on.
    pub wonder_sites: Vec<WonderSite>,
    /// Per tile: a lake (small known water body enclosed by land), not the sea.
    lakes: Vec<bool>,
    /// Built regions in row-major order; unknown regions are absent.
    pub regions: Vec<SurfaceRegion>,
    layout: Option<regions::Layout>,
    /// Index into `regions` for every region of the layout, if built.
    region_slots: Vec<Option<u32>>,
    rivers: Vec<hydrology::River>,
    river_index: index::SegmentIndex,
    /// Lowest and highest vertex: a ray outside this slab cannot hit the surface.
    height_range: [f32; 2],
    /// The `SurfaceBuilder` build that made this mesh (0: none): the builder takes
    /// the regions it keeps from the mesh of its last build only.
    build: u64,
}
const QUERY_BIN: f32 = 1.0;

/// `build`, plus an opt-in measurement: with `AURELIA_SURFACE_BENCH=N` the surface
/// is rebuilt once to warm up and N more times, and the median and p95 CPU
/// build time and the triangle counts are printed to stderr.
pub fn build_measured(world: &RenderWorld, style: &CompiledTerrainStyle) -> SurfaceMesh {
    let runs: usize = std::env::var("AURELIA_SURFACE_BENCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut mesh = build(world, style);
    if runs == 0 {
        return mesh;
    }
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = web_time::Instant::now();
        mesh = build(world, style);
        times.push(start.elapsed().as_secs_f64() * 1e3);
    }
    times.sort_by(f64::total_cmp);
    eprintln!(
        "surface build: {runs} runs, median {:.1} ms, p95 {:.1} ms, land {} water {} vertices {} regions {}",
        times[runs / 2],
        times[(runs - 1) * 95 / 100],
        mesh.land_triangles.len(),
        mesh.water_triangles.len(),
        mesh.vertices.len(),
        mesh.regions.len()
    );
    mesh
}

pub fn build(world: &RenderWorld, style: &CompiledTerrainStyle) -> SurfaceMesh {
    build_in_regions(world, style, REGION)
}

/// The surface of the known regions of `size`×`size` cells, at full detail.
pub fn build_in_regions(world: &RenderWorld, style: &CompiledTerrainStyle, size: u32) -> SurfaceMesh {
    SurfaceBuilder::default().build(world, style, size, &Levels::default(), None)
}

/// Builds the surfaces of successive worlds. A region whose fingerprint (everything
/// its part reads, `fingerprint.rs`) is the same as at the last build keeps its part;
/// only the others are triangulated again. A region whose part and neighbours did not
/// change keeps its final data (platform/MESHING.md §2.2, stage 5 per region), copied from the
/// mesh of the last build. The result is always exactly `build_in_regions` of the
/// same world.
#[derive(Default)]
pub struct SurfaceBuilder {
    /// Region size and map size the cached parts belong to.
    layout: Option<(u32, u32, u32)>,
    parts: std::collections::HashMap<RegionId, (u64, std::sync::Arc<RegionPart>)>,
    /// The stamp of the mesh the last build returned (`SurfaceMesh::build`).
    last_build: u64,
    /// Regions the last build triangulated, and assembled again.
    pub rebuilt: Vec<RegionId>,
    pub reassembled: Vec<RegionId>,
}

/// Stamps of the meshes builders return; 0 is no build.
static BUILDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl SurfaceBuilder {
    /// The surface of `world`. `previous` is the mesh the last call returned, if the
    /// caller still holds it: regions that did not change are copied from it. Without
    /// it (or with another mesh) every region is assembled again.
    pub fn build(
        &mut self,
        world: &RenderWorld,
        style: &CompiledTerrainStyle,
        size: u32,
        levels: &Levels,
        previous: Option<&SurfaceMesh>,
    ) -> SurfaceMesh {
        let mut previous = previous.filter(|mesh| self.last_build != 0 && mesh.build == self.last_build);
        if world.width == 0 || world.height == 0 {
            self.parts.clear();
            self.last_build = 0;
            self.rebuilt.clear();
            return SurfaceMesh::default();
        }
        let shape = (size.max(1), world.width, world.height);
        if self.layout != Some(shape) {
            self.layout = Some(shape);
            self.parts.clear();
            previous = None;
        }
        let field = Landscape::new(world, style);
        let layout = regions::Layout::new(world, size);
        let ids = layout.known(world);
        let plan = regions::SeamPlan::new(&field, &layout, &ids, &field.constraints(), levels);
        let prints = fingerprint::fingerprints(&field, &layout, &plan, &ids, levels);
        let cached = |i: usize| self.parts.get(&ids[i]).filter(|(print, _)| *print == prints[i]).map(|(_, part)| part.clone());
        let stale: Vec<usize> = (0..ids.len()).filter(|&i| cached(i).is_none()).collect();
        let mut fresh = build_parts(&field, &layout, &plan, &ids, &stale, levels).into_iter();
        let mut parts = Vec::with_capacity(ids.len());
        for i in 0..ids.len() {
            let part = match cached(i) {
                Some(part) => part,
                None => std::sync::Arc::new(fresh.next().expect("one fresh part per stale region")),
            };
            parts.push(part);
        }
        self.parts = ids.iter().zip(&prints).zip(&parts).map(|((&id, &print), part)| (id, (print, part.clone()))).collect();
        self.rebuilt = stale.iter().map(|&i| ids[i]).collect();
        let sources = self.assemble(&field, world, &ids, &parts, &stale, previous);
        let mut mesh = SurfaceMesh::from_regions(layout, &sources, previous);
        mesh.build = BUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.last_build = mesh.build;
        mesh.rivers = field.river_segments().to_vec();
        mesh.river_index = index::SegmentIndex::new(mesh.rivers.iter().map(|r| [r.a, r.b]), 1.0);
        mesh.coast_props = field.coast_props().to_vec();
        mesh.wonder_sites = field.wonder_sites().to_vec();
        mesh.lakes = field.lakes();
        mesh
    }
}

impl SurfaceBuilder {
    /// The final data of every region in `ids`: assembled again for a region whose
    /// part or a neighbour's part was rebuilt, or whose neighbours came or went (with
    /// its neighbours as context), kept in `previous` for the others.
    fn assemble(
        &mut self,
        field: &Landscape,
        world: &RenderWorld,
        ids: &[RegionId],
        parts: &[std::sync::Arc<RegionPart>],
        stale: &[usize],
        previous: Option<&SurfaceMesh>,
    ) -> Vec<assembly::RegionSource> {
        let kept_slot = |id: RegionId| previous.and_then(|mesh| mesh.region_slot(id));
        let targets = Self::targets(ids, stale, kept_slot);
        let context = context_of(ids, &targets);
        let chosen: Vec<usize> = (0..ids.len()).filter(|&i| context[i]).collect();
        let context_ids: Vec<RegionId> = chosen.iter().map(|&i| ids[i]).collect();
        let context_parts: Vec<std::sync::Arc<RegionPart>> = chosen.iter().map(|&i| parts[i].clone()).collect();
        let keep: Vec<bool> = chosen.iter().map(|&i| targets[i]).collect();
        let mut fresh = SurfaceMesh::assemble_regions(field, world, &context_ids, &context_parts, &keep).into_iter();
        let mut sources = Vec::with_capacity(ids.len());
        for (i, &id) in ids.iter().enumerate() {
            let region = if context[i] { fresh.next().expect("one result per context region") } else { None };
            sources.push(match region {
                Some(region) => assembly::RegionSource::Fresh(region),
                None => assembly::RegionSource::Kept(kept_slot(id).expect("a region not assembled again was built before")),
            });
        }
        self.reassembled = ids.iter().zip(&targets).filter(|(_, t)| **t).map(|(id, _)| *id).collect();
        sources
    }
}

impl SurfaceBuilder {
    /// Regions to assemble again: new ones (not in the last mesh), rebuilt ones, and
    /// those next to a rebuilt region or to one that came or went.
    fn targets(ids: &[RegionId], stale: &[usize], kept_slot: impl Fn(RegionId) -> Option<usize>) -> Vec<bool> {
        let present: std::collections::HashSet<RegionId> = ids.iter().copied().collect();
        let rebuilt: std::collections::HashSet<RegionId> = stale.iter().map(|&i| ids[i]).collect();
        let was_present = |id: RegionId| kept_slot(id).is_some();
        let target = |id: RegionId| {
            !was_present(id)
                || rebuilt.contains(&id)
                || region_neighbours(id).any(|n| rebuilt.contains(&n) || present.contains(&n) != was_present(n))
        };
        ids.iter().map(|&id| target(id)).collect()
    }
}

/// The assembly context: the targets and every region next to one.
fn context_of(ids: &[RegionId], targets: &[bool]) -> Vec<bool> {
    let slot: std::collections::HashMap<RegionId, usize> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut context = targets.to_vec();
    for (i, &id) in ids.iter().enumerate() {
        if !targets[i] && region_neighbours(id).any(|n| slot.get(&n).is_some_and(|&j| targets[j])) {
            context[i] = true;
        }
    }
    context
}

/// The up to 8 regions around `id` (without it), in row order.
fn region_neighbours(id: RegionId) -> impl Iterator<Item = RegionId> {
    (-1i64..=1).flat_map(move |dy| {
        (-1i64..=1).filter_map(move |dx| {
            let (x, y) = (id.x as i64 + dx, id.y as i64 + dy);
            ((dx, dy) != (0, 0) && x >= 0 && y >= 0).then_some(RegionId { x: x as u32, y: y as u32 })
        })
    })
}

/// One region before assembly: its triangulation and the field at its vertices.
pub struct RegionPart {
    samples: Vec<field::FieldSample>,
    faces: Vec<([u32; 3], SurfaceWaterKind)>,
}

fn region_part(field: &Landscape, layout: &regions::Layout, plan: &regions::SeamPlan, id: RegionId, scale: f32) -> RegionPart {
    let triangles = regions::triangulate(field, layout, plan, id, scale);
    let samples = triangles.positions.iter().map(|&p| field.sample(p[0], p[1], plan.row(p))).collect();
    RegionPart { samples, faces: triangles.faces }
}

/// Wall time of each build step, measured on one thread, for benches.
#[derive(Debug, Clone)]
pub struct BuildProfile {
    /// Contributors from the world observation: coasts, rivers, walls, massifs.
    pub observation_ms: f64,
    pub seam_plan_ms: f64,
    /// Triangulation and field samples of each region.
    pub regions: Vec<RegionProfile>,
    /// Assembly, stage 5 and the region indexes.
    pub assembly_ms: f64,
    pub land_triangles: usize,
    pub water_triangles: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct RegionProfile {
    pub id: RegionId,
    pub ms: f64,
    pub land_triangles: usize,
    pub vertices: usize,
}

/// The same build as `build_in_regions`, on one thread, timing each step.
pub fn profile(world: &RenderWorld, style: &CompiledTerrainStyle, size: u32) -> BuildProfile {
    ONE_THREAD.with(|one| one.set(true));
    let profile = profile_steps(world, style, size);
    ONE_THREAD.with(|one| one.set(false));
    profile
}

thread_local! {
    /// Set while `profile` runs: `parallel_map` then stays on the calling thread.
    static ONE_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn profile_steps(world: &RenderWorld, style: &CompiledTerrainStyle, size: u32) -> BuildProfile {
    let ms = |start: web_time::Instant| start.elapsed().as_secs_f64() * 1e3;
    let start = web_time::Instant::now();
    let field = Landscape::new(world, style);
    let observation_ms = ms(start);
    let start = web_time::Instant::now();
    let layout = regions::Layout::new(world, size);
    let ids = layout.known(world);
    let plan = regions::SeamPlan::new(&field, &layout, &ids, &field.constraints(), &Levels::default());
    let seam_plan_ms = ms(start);
    let mut parts = vec![];
    let mut regions = vec![];
    for &id in &ids {
        let start = web_time::Instant::now();
        let part = region_part(&field, &layout, &plan, id, 1.0);
        regions.push(RegionProfile { id, ms: ms(start), land_triangles: part.faces.len(), vertices: part.samples.len() });
        parts.push(std::sync::Arc::new(part));
    }
    let start = web_time::Instant::now();
    let all = vec![true; ids.len()];
    let sources: Vec<_> = SurfaceMesh::assemble_regions(&field, world, &ids, &parts, &all)
        .into_iter()
        .map(|f| assembly::RegionSource::Fresh(f.expect("every region kept")))
        .collect();
    let mesh = SurfaceMesh::from_regions(layout, &sources, None);
    let assembly_ms = ms(start);
    BuildProfile {
        observation_ms,
        seam_plan_ms,
        regions,
        assembly_ms,
        land_triangles: mesh.land_triangles.len(),
        water_triangles: mesh.water_triangles.len(),
    }
}

/// Parts of the regions `ids[i]` for each `i` in `which` (ascending), in that order.
/// Regions are independent: they are triangulated and sampled on worker threads, and
/// the parts come back in order, so the result does not depend on the thread count.
fn build_parts(field: &Landscape, layout: &regions::Layout, plan: &regions::SeamPlan, ids: &[RegionId], which: &[usize], levels: &Levels) -> Vec<RegionPart> {
    // Largest first (by known cells), so no big region starts last.
    let mut order: Vec<usize> = (0..which.len()).collect();
    let known: Vec<usize> = which.iter().map(|&i| layout.known_cells(field.world(), ids[i])).collect();
    order.sort_by_key(|&k| std::cmp::Reverse(known[k]));
    let parts = parallel_map(&order, |&k| {
        let id = ids[which[k]];
        region_part(field, layout, plan, id, Levels::scale(levels.of(id)))
    });
    let mut slots: Vec<Option<RegionPart>> = (0..which.len()).map(|_| None).collect();
    for (k, part) in order.into_iter().zip(parts) {
        slots[k] = Some(part);
    }
    slots.into_iter().map(|part| part.expect("every region built")).collect()
}

/// `items.iter().map(f)` on worker threads, results in item order. Workers
/// take the next item from one shared counter, so items of uneven cost keep
/// every worker busy; each result has one owner until it is returned.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(items.len());
    if threads <= 1 || ONE_THREAD.with(|one| one.get()) {
        return items.iter().map(f).collect();
    }
    let (f, next) = (&f, &AtomicUsize::new(0));
    let lanes: Vec<Vec<(usize, R)>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(move || {
                    let mut done = vec![];
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= items.len() {
                            return done;
                        }
                        done.push((i, f(&items[i])));
                    }
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().expect("surface worker")).collect()
    });
    let mut results: Vec<Option<R>> = (0..items.len()).map(|_| None).collect();
    for (i, result) in lanes.into_iter().flatten() {
        results[i] = Some(result);
    }
    results.into_iter().map(|r| r.expect("one result per item")).collect()
}

/// A browser page has no threads to spawn (no wasm atomics): the items run in order.
#[cfg(target_arch = "wasm32")]
pub(crate) fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    items.iter().map(f).collect()
}

impl SurfaceMesh {
    /// Distance along a unit ray to the nearest land or water triangle. The
    /// ray is clipped to the height slab of the mesh and walked through the
    /// query grid cell by cell, near to far, testing the triangles the region
    /// indexes list there; it stops at the first cell beyond a hit. A mesh
    /// built without regions is scanned whole.
    pub fn ray_hit(&self, origin: [f32; 3], dir: [f32; 3]) -> Option<f32> {
        // A level ray has no slab to clip to; it is scanned whole.
        if self.regions.is_empty() || dir[1].abs() < 1e-6 {
            return self.ray_hit_scan(origin, dir);
        }
        let [low, high] = self.height_range;
        let (a, b) = ((low - 1e-3 - origin[1]) / dir[1], (high + 1e-3 - origin[1]) / dir[1]);
        let (t0, t1) = (a.min(b).max(0.), a.max(b));
        if t1 < t0 {
            return None;
        }
        let mut nearest: Option<f32> = None;
        grid_walk([origin[0] + dir[0] * t0, origin[2] + dir[2] * t0], [dir[0], dir[2]], t1 - t0, QUERY_BIN, |cell, entry| {
            if nearest.is_some_and(|n| n < t0 + entry) {
                return false;
            }
            let (low, high) = ([cell.0 as f32 * QUERY_BIN, cell.1 as f32 * QUERY_BIN], [(cell.0 + 1) as f32 * QUERY_BIN, (cell.1 + 1) as f32 * QUERY_BIN]);
            for region in &self.regions {
                let [rl, rh] = region.bounds;
                if rh[0] < low[0] || rl[0] > high[0] || rh[1] < low[1] || rl[1] > high[1] {
                    continue;
                }
                let land = region.land_bins.bin(cell.0, cell.1).iter().map(|&i| self.land_triangles[(region.land.start + i) as usize]);
                let water = region.water_bins.bin(cell.0, cell.1).iter().map(|&i| self.water_triangles[(region.water.start + i) as usize]);
                for triangle in land.chain(water) {
                    if let Some(d) = self.ray_triangle(origin, dir, triangle) {
                        nearest = Some(nearest.map_or(d, |n| n.min(d)));
                    }
                }
            }
            true
        });
        nearest
    }

    /// `ray_hit` by testing every triangle: the oracle of the indexed walk.
    pub fn ray_hit_scan(&self, origin: [f32; 3], dir: [f32; 3]) -> Option<f32> {
        let all = self.land_triangles.iter().chain(&self.water_triangles);
        all.filter_map(|t| self.ray_triangle(origin, dir, *t)).reduce(f32::min)
    }

    fn ray_triangle(&self, origin: [f32; 3], dir: [f32; 3], triangle: [u32; 3]) -> Option<f32> {
        let positions = triangle.map(|i| self.vertices.get(i as usize).map(|v| v.position));
        let [Some(a), Some(b), Some(c)] = positions else {
            return None;
        };
        crate::scene::picking::ray_triangle_distance(origin, dir, [a, b, c])
    }

    /// Whether the water at a point is a lake.
    pub fn is_lake(&self, x: f32, z: f32) -> bool {
        let Some(layout) = &self.layout else { return false };
        let tile = crate::scene::picking::world_to_tile(x, z, (layout.width, layout.height));
        tile.is_some_and(|t| self.lakes.get(t.0 as usize) == Some(&true))
    }

    /// Index into `regions` of the built region under a point.
    pub fn region_slot_of_point(&self, x: f32, z: f32) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let id = layout.of_point([x, z])?;
        self.region_slots.get(layout.index(id)).copied().flatten().map(|slot| slot as usize)
    }

    /// Index into `regions` of a built region.
    pub fn region_slot(&self, id: RegionId) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        if id.x >= layout.columns() || id.y >= layout.rows() {
            return None;
        }
        self.region_slots.get(layout.index(id)).copied().flatten().map(|slot| slot as usize)
    }

    /// XZ box (low, high) of the cells of region `id`, built or not.
    pub fn region_box(&self, id: RegionId) -> Option<[[f32; 2]; 2]> {
        Some(self.layout.as_ref()?.bounds(id))
    }

    /// Ground faces of region `r`.
    pub fn region_faces(&self, r: usize) -> &[[u32; 3]] {
        let region = &self.regions[r];
        &self.land_triangles[region.land.start as usize..region.land.end as usize]
    }

    /// Logical cells of region `r`.
    pub fn region_cells(&self, r: usize) -> usize {
        self.layout.as_ref().map_or(0, |layout| layout.cells(self.regions[r].id).count())
    }

    /// Built regions next to region `r`, diagonals included: the only regions that
    /// can share a point with it.
    pub fn neighbour_slots(&self, r: usize) -> Vec<usize> {
        let id = self.regions[r].id;
        let mut out = Vec::new();
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                let (x, y) = (id.x as i64 + dx, id.y as i64 + dy);
                if (dx, dy) == (0, 0) || x < 0 || y < 0 {
                    continue;
                }
                if let Some(slot) = self.region_slot(RegionId { x: x as u32, y: y as u32 }) {
                    out.push(slot);
                }
            }
        }
        out
    }

    /// A hash of everything region `r` shows: its ground and water faces with all the
    /// data of their vertices, and whether each water vertex is a lake. Two builds give
    /// the region the same meshes if their digests and their neighbours' agree.
    pub fn region_digest(&self, r: usize) -> u64 {
        use std::hash::Hasher;
        let region = &self.regions[r];
        let mut hasher = rustc_hash::FxHasher::default();
        for face in self.region_faces(r) {
            for &i in face {
                self.hash_vertex(&mut hasher, i);
            }
        }
        hasher.write_u8(0xff);
        for face in &self.water_triangles[region.water.start as usize..region.water.end as usize] {
            for &i in face {
                self.hash_vertex(&mut hasher, i);
            }
        }
        hasher.finish()
    }

    /// A hash of the ground and water under one tile: every face, of any region, whose
    /// box overlaps the box of the tile's hex. What stands on the tile reads only these
    /// faces, so it needs placing again only when this digest changes.
    pub fn tile_digest(&self, tile: u32) -> u64 {
        use std::hash::Hasher;
        let Some(layout) = &self.layout else { return 0 };
        let (cx, cz) = crate::scene::layout::tile_to_world(tile % layout.width, tile / layout.width);
        let (low, high) = ([cx - 1., cz - 1.], [cx + 1., cz + 1.]);
        let mut hasher = rustc_hash::FxHasher::default();
        // The tile's region and the regions around it whose faces reach its box.
        let home = layout.of_cell(tile % layout.width, tile / layout.width);
        let mut slots: Vec<usize> =
            std::iter::once(home).chain(region_neighbours(home)).filter_map(|id| self.region_slot(id)).collect();
        slots.sort_unstable();
        for slot in slots {
            let region = &self.regions[slot];
            let [rl, rh] = region.bounds;
            if rh[0] < low[0] || rl[0] > high[0] || rh[1] < low[1] || rl[1] > high[1] {
                continue;
            }
            self.hash_faces_in(&mut hasher, &region.land_bins, &self.land_triangles, &region.land, [low, high]);
            self.hash_faces_in(&mut hasher, &region.water_bins, &self.water_triangles, &region.water, [low, high]);
        }
        hasher.finish()
    }

    /// Hashes the vertices of a region's faces (of one kind) that overlap a box, in
    /// face order, then a terminator.
    fn hash_faces_in(
        &self,
        hasher: &mut rustc_hash::FxHasher,
        bins_of: &index::SegmentIndex,
        triangles: &[[u32; 3]],
        range: &std::ops::Range<u32>,
        [low, high]: [[f32; 2]; 2],
    ) {
        use std::hash::Hasher;
        let bins = |v: f32| (v / QUERY_BIN).floor() as i32;
        let mut faces: Vec<u32> = Vec::new();
        for z in bins(low[1])..=bins(high[1]) {
            for x in bins(low[0])..=bins(high[0]) {
                faces.extend_from_slice(bins_of.bin(x, z));
            }
        }
        faces.sort_unstable();
        faces.dedup();
        for face in faces {
            let triangle = triangles[(range.start + face) as usize];
            let p = triangle.map(|i| self.vertices[i as usize].position);
            let outside = p.iter().all(|v| v[0] < low[0])
                || p.iter().all(|v| v[0] > high[0])
                || p.iter().all(|v| v[2] < low[1])
                || p.iter().all(|v| v[2] > high[1]);
            if !outside {
                for i in triangle {
                    self.hash_vertex(hasher, i);
                }
            }
        }
        hasher.write_u8(0xff);
    }

    /// Every field of a vertex that reaches the GPU or a query, its massif data, and
    /// whether a water vertex is a lake.
    fn hash_vertex(&self, hasher: &mut rustc_hash::FxHasher, i: u32) {
        use std::hash::Hasher;
        let v = &self.vertices[i as usize];
        for value in v.position.iter().chain(&v.albedo).chain(&v.flow_uv).chain(&v.tone) {
            hasher.write_u32(value.to_bits());
        }
        for value in [v.coastal_rock, v.water, v.water_depth, v.shore_distance, v.fog] {
            hasher.write_u32(value.to_bits());
        }
        hasher.write_u16(v.terrain);
        hasher.write_u8(v.water_kind as u8);
        let mountain = self.mountain.get(i as usize).copied().unwrap_or_default();
        hasher.write_u32(mountain.lift.to_bits());
        hasher.write_u32(mountain.snow.to_bits());
        if v.water_kind != SurfaceWaterKind::None {
            hasher.write_u8(u8::from(self.is_lake(v.position[0], v.position[2])));
        }
    }

    fn region_at(&self, x: f32, z: f32) -> Option<&SurfaceRegion> {
        let layout = self.layout.as_ref()?;
        let id = layout.of_point([x, z])?;
        let slot = (*self.region_slots.get(layout.index(id))?)?;
        self.regions.get(slot as usize)
    }
    /// The region of `size` cells a world point falls in, built or not.
    pub fn region_of_point(&self, x: f32, z: f32) -> Option<RegionId> {
        self.layout.as_ref()?.of_point([x, z])
    }
    pub fn nearest_river_anchor(&self, p: [f32; 2]) -> Option<SurfaceRiverAnchor> {
        let nearest = self.river_index.nearest(p, f32::INFINITY, |i| {
            let r = &self.rivers[i as usize];
            hydrology::closest(p, r.a, r.b).0
        })?;
        let river = &self.rivers[nearest.index as usize];
        let (_, q, t) = hydrology::closest(p, river.a, river.b);
        let dx = river.b[0] - river.a[0];
        let dz = river.b[1] - river.a[1];
        let length = dx.hypot(dz).max(1e-6);
        let level = self
            .sample_water(q[0], q[1])
            .map_or(river.ya * (1. - t) + river.yb * t, |s| s.y);
        Some(SurfaceRiverAnchor {
            position: [q[0], level, q[1]],
            tangent: [dx / length, dz / length],
            half_width: river.width_at(t),
        })
    }
    /// Material under a map overlay, queried from the same local triangle index.
    pub fn water_kind_at(&self, x: f32, z: f32) -> SurfaceWaterKind {
        let Some(region) = self.region_at(x, z) else {
            return SurfaceWaterKind::None;
        };
        for &i in region.water_bins.candidates([x, z]) {
            let triangle = self.water_triangles[(region.water.start + i) as usize];
            let points = triangle.map(|v| self.vertices[v as usize].position);
            if sample_triangle(points, x, z).is_some() {
                return self.vertices[triangle[0] as usize].water_kind;
            }
        }
        SurfaceWaterKind::None
    }

    pub fn sample_water(&self, x: f32, z: f32) -> Option<SurfaceSample> {
        let region = self.region_at(x, z)?;
        region
            .water_bins
            .candidates([x, z])
            .iter()
            .filter_map(|&i| {
                let triangle = self.water_triangles[(region.water.start + i) as usize];
                sample_triangle(triangle.map(|v| self.vertices[v as usize].position), x, z)
            })
            .max_by(|a, b| a.y.total_cmp(&b.y))
    }
    pub fn sample_display(&self, x: f32, z: f32) -> Option<SurfaceSample> {
        let ground = self.sample_ground(x, z);
        let water = self.sample_water(x, z);
        match (ground, water) {
            (Some(a), Some(b)) => Some(if a.y > b.y { a } else { b }),
            (a, None) => a,
            (None, b) => b,
        }
    }
    pub fn sample_ground(&self, x: f32, z: f32) -> Option<SurfaceSample> {
        let region = self.region_at(x, z)?;
        for &i in region.land_bins.candidates([x, z]) {
            let triangle = self.land_triangles[(region.land.start + i) as usize];
            if let Some(sample) = sample_triangle(triangle.map(|v| self.vertices[v as usize].position), x, z) {
                return Some(sample);
            }
        }
        None
    }
}

/// Visits the grid cells of size `cell` that the segment from `start` along
/// `dir` (XZ, any length) for parameter length `reach` passes, in order, with
/// the ray parameter at which the segment enters each; `visit` returns false
/// to stop. Amanatides–Woo traversal.
fn grid_walk(start: hydrology::Point, dir: hydrology::Point, reach: f32, cell: f32, mut visit: impl FnMut((i32, i32), f32) -> bool) {
    let mut at = ((start[0] / cell).floor() as i32, (start[1] / cell).floor() as i32);
    let step = |d: f32| if d > 0. { 1 } else { -1 };
    let first = |s: f32, d: f32, i: i32| {
        if d.abs() < 1e-12 {
            return f32::INFINITY;
        }
        let edge = if d > 0. { (i + 1) as f32 * cell } else { i as f32 * cell };
        (edge - s) / d
    };
    let (mut next_x, mut next_z) = (first(start[0], dir[0], at.0), first(start[1], dir[1], at.1));
    let (delta_x, delta_z) = ((cell / dir[0]).abs(), (cell / dir[1]).abs());
    let mut entry = 0.;
    loop {
        if !visit(at, entry) {
            return;
        }
        entry = next_x.min(next_z);
        if entry > reach {
            return;
        }
        if next_x < next_z {
            at.0 += step(dir[0]);
            next_x += delta_x;
        } else {
            at.1 += step(dir[1]);
            next_z += delta_z;
        }
    }
}

fn sample_triangle(p: [[f32; 3]; 3], x: f32, z: f32) -> Option<SurfaceSample> {
    let [a, b, c] = p;
    let d = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
    if d.abs() < 1e-10 {
        return None;
    }
    let u = ((b[2] - c[2]) * (x - c[0]) + (c[0] - b[0]) * (z - c[2])) / d;
    let v = ((c[2] - a[2]) * (x - c[0]) + (a[0] - c[0]) * (z - c[2])) / d;
    if u < -1e-4 || v < -1e-4 || u + v > 1.0001 {
        return None;
    }
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let mut n = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if n[1] < 0. {
        n = n.map(|v| -v);
    }
    n = n.map(|v| v / length);
    Some(SurfaceSample {
        y: u * a[1] + v * b[1] + (1. - u - v) * c[1],
        normal: n,
        slope: 1. - n[1],
    })
}
#[cfg(test)]
mod tests;
#[cfg(test)]
mod region_tests;
#[cfg(test)]
mod incremental_tests;
#[cfg(test)]
mod mouth_tests;
