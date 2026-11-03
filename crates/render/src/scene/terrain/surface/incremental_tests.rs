//! Differential tests of the incremental build (platform/MESHING.md §3.2, §3.7): a builder that
//! reuses the parts of unchanged regions gives exactly the surface a fresh build gives,
//! after every step of a map being revealed.
use super::*;
use crate::scene::style::{compile, default_style};
use crate::world::projection::{rebuild_render_world, refresh};
use kernel::ids::{PlayerId, TileId};

/// Every field of two meshes, bit for bit (floats compared exactly).
fn assert_same(a: &SurfaceMesh, b: &SurfaceMesh, step: &str) {
    assert_eq!(a.vertices.len(), b.vertices.len(), "{step}: vertex count");
    for (i, (x, y)) in a.vertices.iter().zip(&b.vertices).enumerate() {
        assert!(format!("{x:?}") == format!("{y:?}"), "{step}: vertex {i} differs: {x:?} vs {y:?}");
    }
    assert_eq!(a.land_triangles, b.land_triangles, "{step}: land triangles");
    assert_eq!(a.water_triangles, b.water_triangles, "{step}: water triangles");
    assert_eq!(a.mountain, b.mountain, "{step}: mountain metadata");
    assert_eq!(a.lakes, b.lakes, "{step}: lakes");
    assert_eq!(a.wonder_sites, b.wonder_sites, "{step}: wonder sites");
    assert_eq!(format!("{:?}", a.coast_props), format!("{:?}", b.coast_props), "{step}: coast props");
    let regions = |m: &SurfaceMesh| m.regions.iter().map(|r| (r.id, r.land.clone(), r.water.clone())).collect::<Vec<_>>();
    assert_eq!(regions(a), regions(b), "{step}: regions");
}

/// A generated classic-style map revealed in growing patches along a walk: coasts, cliffs,
/// rivers, mountains and features enter the observer's knowledge piece by piece.
#[test]
fn incremental_builds_equal_fresh_builds_while_the_map_is_revealed() {
    let style = compile(default_style()).unwrap();
    let rules = sandbox_content::rules().expect("sandbox rules");
    let config = kernel::scenario::ScenarioConfig { seed: 7, players: 2, width: 56, height: 40, cities: 0, units: 0, water_ratio: 30 };
    let mut game = kernel::scenario::generate(&config, &rules);
    let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    world.visual_surface = true;
    let mut builder = SurfaceBuilder::default();
    let mut previous: Option<SurfaceMesh> = None;
    let (mut reused, mut rebuilt) = (0, 0);
    let walk = [(8, 8), (14, 10), (20, 14), (26, 18), (31, 22), (36, 25), (40, 30), (44, 33), (47, 20)];
    for (step, &(cq, cr)) in walk.iter().enumerate() {
        let patch: Vec<TileId> = (0..config.height)
            .flat_map(|r| (0..config.width).map(move |q| (q, r)))
            .filter(|&(q, r)| {
                let (dq, dr) = (q as i32 - cq, r as i32 - cr);
                (dq.abs() + dr.abs() + (dq + dr).abs()) / 2 <= 3
            })
            .map(|(q, r)| TileId(r * config.width + q))
            .collect();
        game.debug_reveal(PlayerId(0), &patch);
        refresh(&mut world, game.view(), PlayerId(0), &rules);
        let incremental = builder.build(&world, &style, REGION, &Levels::default(), previous.as_ref());
        let fresh = build_in_regions(&world, &style, REGION);
        assert_same(&incremental, &fresh, &format!("step {step}"));
        if step > 0 {
            rebuilt += builder.rebuilt.len();
            reused += incremental.regions.len() - builder.rebuilt.len();
        }
        previous = Some(incremental);
    }
    assert!(reused > 0, "a local reveal keeps regions out of its reach: reused {reused}, rebuilt {rebuilt}");
}

/// Assembly per region: an edit of coast cells next to region corners reassembles
/// only the regions around them (with their neighbours as context), and the surface
/// still equals a fresh build — water levels, contacts and shore distances across
/// the seams included.
#[test]
fn local_edits_reassemble_only_nearby_regions_and_match_a_fresh_build() {
    let style = compile(default_style()).unwrap();
    let rules = sandbox_content::rules().expect("sandbox rules");
    let config = kernel::scenario::ScenarioConfig { seed: 21, players: 2, width: 96, height: 64, cities: 0, units: 0, water_ratio: 35 };
    let game = kernel::scenario::generate(&config, &rules);
    let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    world.visual_surface = true;
    world.visible.fill(1);
    world.explored.fill(1);
    let mut builder = SurfaceBuilder::default();
    let mut previous = builder.build(&world, &style, REGION, &Levels::default(), None);
    let total = previous.regions.len();
    // Coast cells near region corners: flip sea and land around (16k, 16j).
    let mut edits = 0;
    for (cq, cr) in [(16u32, 16u32), (48, 32), (80, 48)] {
        for (dq, dr) in [(0, 0), (-1, 0), (0, -1), (-1, -1)] {
            let i = ((cr as i32 + dr) as u32 * world.width + (cq as i32 + dq) as u32) as usize;
            world.terrain[i].terrain = if world.terrain[i].terrain == 4 { 1 } else { 4 };
            edits += 1;
        }
        let incremental = builder.build(&world, &style, REGION, &Levels::default(), Some(&previous));
        let fresh = build_in_regions(&world, &style, REGION);
        assert_same(&incremental, &fresh, &format!("edit at {cq},{cr}"));
        assert!(builder.reassembled.len() < total, "edit at {cq},{cr}: reassembled {} of {total}", builder.reassembled.len());
        previous = incremental;
    }
    assert_eq!(edits, 12);
}

/// No global dependency: a local edit changes the fingerprints of the regions whose
/// query box reaches it, and of no other region.
#[test]
fn a_local_edit_changes_only_nearby_fingerprints() {
    let style = compile(default_style()).unwrap();
    let rules = sandbox_content::rules().expect("sandbox rules");
    let config = kernel::scenario::ScenarioConfig { seed: 21, players: 2, width: 96, height: 64, cities: 0, units: 0, water_ratio: 35 };
    let game = kernel::scenario::generate(&config, &rules);
    let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    world.visual_surface = true;
    world.visible.fill(1);
    world.explored.fill(1);
    let prints = |w: &RenderWorld| -> Vec<(RegionId, u64)> {
        let field = field::Landscape::new(w, &style);
        let layout = regions::Layout::new(w, REGION);
        let ids = layout.known(w);
        let plan = regions::SeamPlan::new(&field, &layout, &ids, &field.constraints(), &Levels::default());
        ids.iter().copied().zip(fingerprint::fingerprints(&field, &layout, &plan, &ids, &Levels::default())).collect()
    };
    let before = prints(&world);
    let (q, r) = (50u32, 37u32);
    for (dq, dr) in [(0, 0), (-1, 0), (0, -1), (-1, -1)] {
        let i = ((r as i32 + dr) as u32 * world.width + (q as i32 + dq) as u32) as usize;
        world.terrain[i].terrain = if world.terrain[i].terrain == 4 { 1 } else { 4 };
    }
    let after = prints(&world);
    let layout = regions::Layout::new(&world, REGION);
    let (x, z) = crate::scene::layout::tile_to_world(q, r);
    let mut changed = 0;
    for ((id, a), (_, b)) in before.iter().zip(&after) {
        if a == b {
            continue;
        }
        changed += 1;
        // Contours of a coast change up to a cell or two around the edit.
        let [low, high] = layout.bounds(*id);
        let reach = fingerprint::REACH + 4.0;
        let near = x > low[0] - reach && x < high[0] + reach && z > low[1] - reach && z < high[1] + reach;
        assert!(near, "region {id:?} changed by an edit far from it");
    }
    assert!(changed > 0 && changed < before.len() / 2, "{changed} of {} regions changed", before.len());
}

/// Detail levels (platform/MESHING.md §2.5): far regions at a coarser level have fewer
/// triangles; changing some regions' levels rebuilds them and the neighbours whose
/// seams change, and the result equals a fresh build at the same levels.
#[test]
fn level_changes_rebuild_like_a_fresh_build_and_coarsen_the_mesh() {
    let style = compile(default_style()).unwrap();
    let rules = sandbox_content::rules().expect("sandbox rules");
    let config = kernel::scenario::ScenarioConfig { seed: 5, players: 2, width: 64, height: 48, cities: 0, units: 0, water_ratio: 30 };
    let game = kernel::scenario::generate(&config, &rules);
    let mut world = rebuild_render_world(game.view(), PlayerId(0), &rules);
    world.visual_surface = true;
    world.visible.fill(1);
    world.explored.fill(1);
    let full = build_in_regions(&world, &style, REGION);
    let mut builder = SurfaceBuilder::default();
    let mut previous = builder.build(&world, &style, REGION, &Levels::default(), None);
    let mut levels = Levels::default();
    for (step, (count, level)) in [(3, 1u8), (6, 2), (12, 2), (4, 0)].into_iter().enumerate() {
        for id in full.regions.iter().map(|r| r.id).take(count) {
            levels.set(id, level);
        }
        let incremental = builder.build(&world, &style, REGION, &levels, Some(&previous));
        let fresh = SurfaceBuilder::default().build(&world, &style, REGION, &levels, None);
        assert_same(&incremental, &fresh, &format!("levels step {step}"));
        assert!(builder.rebuilt.len() < full.regions.len(), "step {step}: only changed regions and their seams");
        previous = incremental;
    }
    let mut coarse = Levels::default();
    for region in &full.regions {
        coarse.set(region.id, 2);
    }
    let far = SurfaceBuilder::default().build(&world, &style, REGION, &coarse, None);
    assert!(
        far.land_triangles.len() * 3 < full.land_triangles.len() * 2,
        "level 2 has far fewer triangles: {} vs {}",
        far.land_triangles.len(),
        full.land_triangles.len()
    );
}

/// The sandbox island, its knowledge growing ring by ring from the coast inland and
/// then a terrain edit far from most regions: same surface as fresh builds.
#[test]
fn incremental_island_matches_fresh_builds_under_reveals_and_edits() {
    let style = compile(default_style()).unwrap();
    let mut world = fixtures::island();
    let mut builder = SurfaceBuilder::default();
    let mut previous: Option<SurfaceMesh> = None;
    for step in 0..5 {
        let known: Vec<usize> = (0..world.terrain.len()).filter(|&i| world.is_known(i)).collect();
        // Grow knowledge by one ring around every known cell.
        for i in known {
            let (q, r) = ((i as u32 % world.width) as i32, (i as u32 / world.width) as i32);
            for (dq, dr) in crate::scene::layout::EDGE_NEIGHBORS {
                let (nq, nr) = (q + dq, r + dr);
                if nq >= 0 && nr >= 0 && (nq as u32) < world.width && (nr as u32) < world.height && (nq + nr + step) % 3 == 0 {
                    world.explored[(nr as u32 * world.width + nq as u32) as usize] = 1;
                }
            }
        }
        if step == 3 {
            let tile = (world.width * 20 + 6) as usize;
            world.terrain[tile].terrain = 6;
        }
        let incremental = builder.build(&world, &style, 8, &Levels::default(), previous.as_ref());
        let fresh = build_in_regions(&world, &style, 8);
        assert_same(&incremental, &fresh, &format!("island step {step}"));
        previous = Some(incremental);
    }
}

/// The builder copies unchanged regions only from the mesh its last build returned:
/// handed another mesh, it assembles every region again, and the surface is exact.
#[test]
fn regions_are_kept_only_from_the_last_build() {
    let style = compile(default_style()).unwrap();
    let world = fixtures::island();
    let mut builder = SurfaceBuilder::default();
    let own = builder.build(&world, &style, 8, &Levels::default(), None);
    let foreign = SurfaceBuilder::default().build(&world, &style, 8, &Levels::default(), None);
    let again = builder.build(&world, &style, 8, &Levels::default(), Some(&foreign));
    assert!(!again.regions.is_empty());
    assert_eq!(builder.reassembled.len(), again.regions.len(), "nothing is copied from a foreign mesh");
    assert_same(&again, &own, "foreign previous");
    let kept = builder.build(&world, &style, 8, &Levels::default(), Some(&again));
    assert!(builder.reassembled.is_empty(), "an unchanged world copies every region");
    assert_same(&kept, &own, "own previous");
}
