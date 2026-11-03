//! The river junctions: mouths, sources, widths, curves (`mouths.rs` fixtures from real maps).
use super::*;
use crate::scene::style::{compile, default_style};

fn mesh_of(w: &RenderWorld) -> SurfaceMesh {
    build(w, &compile(default_style()).unwrap())
}

fn near_river(mesh: &SurfaceMesh, p: [f32; 2], reach: f32) -> bool {
    mesh.rivers.iter().any(|r| hydrology::closest(p, r.a, r.b).0 < reach)
}

/// Slope (rise over run) of a triangle.
fn slope(p: [[f32; 3]; 3]) -> f32 {
    let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
    let v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    n[0].hypot(n[2]) / n[1].abs().max(1e-9)
}

fn fixtures() -> Vec<(&'static str, RenderWorld)> {
    vec![
        ("beach", mouths::beach_mouth()),
        ("cliff", mouths::cliff_mouth()),
        ("stream", mouths::stream_mouth()),
        ("lake", mouths::lake_mouth()),
    ]
}

/// The worst slope of land triangles around the rivers of a mesh, off the rock of a cliff.
fn worst_slope(mesh: &SurfaceMesh) -> f32 {
    let mut worst = 0f32;
    for t in &mesh.land_triangles {
        let p = t.map(|i| mesh.vertices[i as usize].position);
        let c = [(p[0][0] + p[1][0] + p[2][0]) / 3., (p[0][2] + p[1][2] + p[2][2]) / 3.];
        if t.iter().any(|&i| mesh.vertices[i as usize].coastal_rock > 0.05 || mesh.vertices[i as usize].shore_distance.abs() < 0.1) || !near_river(mesh, c, 1.2) {
            continue;
        }
        worst = worst.max(slope(p));
    }
    worst
}

/// A tributary and its trunk both shape the ground around their junction: the ground is one
/// continuous surface there, not a sliver where the nearest river changes (it was a wall
/// of slope 100 on the beach fixture).
#[test]
fn ground_around_rivers_has_no_steep_slivers() {
    for (name, w) in fixtures() {
        let worst = worst_slope(&mesh_of(&w));
        assert!(worst < 12., "{name}: worst slope near rivers is {worst}");
    }
}

/// The river runs on to the coast and its water goes on as the sea's: wet all along the axis,
/// from upstream into the sea, and the sea floor is not lifted into a shoal at the mouth.
#[test]
fn river_mouth_is_continuous_water_into_the_sea() {
    let style = compile(default_style()).unwrap();
    for (name, w) in fixtures() {
        let mesh = mesh_of(&w);
        let field = field::Landscape::new(&w, &style);
        let mut mouths = 0;
        for r in mesh.rivers.iter().filter(|r| r.mouth_at(1.) > 0.99) {
            mouths += 1;
            let dir = [r.b[0] - r.a[0], r.b[1] - r.a[1]];
            let len = dir[0].hypot(dir[1]).max(1e-6);
            let dir = [dir[0] / len, dir[1] / len];
            let end = field.vertex(r.b[0], r.b[1]);
            assert!((-0.5..0.2).contains(&end.shore_distance), "{name}: the river ends {} from the coast", end.shore_distance);
            // No dry gap between the channel and the sea.
            for k in -12..=12 {
                let along = k as f32 * 0.06;
                let p = [r.b[0] + dir[0] * along, r.b[1] + dir[1] * along];
                let wet = field.water_at(p[0], p[1]).kind != SurfaceWaterKind::None;
                assert!(wet || along > 0.1 && field.vertex(p[0], p[1]).shore_distance > 0.15, "{name}: dry gap at {along} from the river end {:?}", r.b);
            }
        }
        assert!(mouths > 0, "{name}: no river mouth in the fixture");
        for v in &mesh.vertices {
            let at_mouth = mesh.rivers.iter().filter(|r| r.mouth_at(1.) > 0.99).any(|r| (v.position[0] - r.b[0]).hypot(v.position[2] - r.b[1]) < 1.0);
            if v.shore_distance < -0.3 && at_mouth && v.water_kind == SurfaceWaterKind::None {
                assert!(v.position[1] <= banks::sea_floor(v.shore_distance) + 0.1, "{name}: the sea floor is lifted to {} (floor {}, shore {}) at {:?}", v.position[1], banks::sea_floor(v.shore_distance), v.shore_distance, v.position);
            }
        }
    }
}

/// A rocky coast keeps no cliff shards inside a river's channel.
#[test]
fn channels_hold_no_cliff_rock() {
    for (name, w) in fixtures() {
        let mesh = mesh_of(&w);
        for v in &mesh.vertices {
            let p = [v.position[0], v.position[2]];
            let inside = mesh.rivers.iter().any(|r| {
                let (d, _, t) = hydrology::closest(p, r.a, r.b);
                d < r.width_at(t) * 0.9
            });
            if inside && v.shore_distance > 0.05 {
                assert!(v.coastal_rock < 0.05, "{name}: rock {} in a channel at {p:?}", v.coastal_rock);
            }
        }
    }
}

/// Width follows the flow: a river never narrows downstream, steps are small along a chain, a
/// source is a thread, and a confluence is wider than any branch that joins it.
#[test]
fn river_width_grows_downstream_without_steps() {
    for (name, w) in fixtures().into_iter().chain([("plain", mouths::plain_river()), ("confluence", mouths::confluence()), ("slope", mouths::slope_river())]) {
        let mesh = mesh_of(&w);
        let rivers = &mesh.rivers;
        let mut sources = 0;
        for (i, r) in rivers.iter().enumerate() {
            // A mouth may narrow a little where its curls into the coast (`reaches::bend_caps`).
            if r.mouth_at(0.) < 0.01 {
                assert!(r.width_b >= r.width - 1e-4, "{name}: segment {i} narrows downstream: {} to {}", r.width, r.width_b);
            }
            if r.spring_at(0.) > 0.99 {
                sources += 1;
                assert!(r.width <= 0.03, "{name}: the source is {} wide", r.width);
            }
            if i > 0 && rivers[i - 1].b == r.a && r.mouth_at(0.) < 0.01 {
                let step = (r.width - rivers[i - 1].width_b).abs();
                assert!(step < 0.02, "{name}: width steps by {step} between segments {} and {i}", i - 1);
            }
        }
        assert!(sources > 0, "{name}: no source in the fixture");
        // At a junction the trunk carries the sum of its branches.
        for r in rivers {
            for other in rivers.iter().filter(|o| o.b == r.b && o.a != r.a) {
                let trunk = rivers.iter().filter(|t| t.a == r.b).map(|t| t.width).fold(0., f32::max);
                if trunk > 0. {
                    assert!(trunk >= r.width_b.max(other.width_b) - 1e-4, "{name}: a trunk of {trunk} is thinner than a branch");
                }
            }
        }
    }
    let widest = mesh_of(&mouths::confluence()).rivers.iter().map(|r| r.width_b).fold(0., f32::max);
    assert!(widest > 3. * 0.045, "rivers have no variety: the widest is {widest}");
}

/// The shape of rivers comes from the data alone: the same world builds the same mesh.
#[test]
fn river_surface_is_deterministic() {
    for (name, w) in fixtures().into_iter().chain([("plain", mouths::plain_river()), ("confluence", mouths::confluence())]) {
        let (a, b) = (mesh_of(&w), mesh_of(&w));
        assert_eq!(a.vertices.len(), b.vertices.len(), "{name}");
        for (u, v) in a.vertices.iter().zip(&b.vertices) {
            assert_eq!(u.position, v.position, "{name}");
            assert_eq!(u.tone, v.tone, "{name}");
        }
        assert_eq!(a.rivers.len(), b.rivers.len(), "{name}");
        for (u, v) in a.rivers.iter().zip(&b.rivers) {
            assert_eq!((u.a, u.b, u.width, u.width_b), (v.a, v.b, v.width, v.width_b), "{name}");
        }
    }
}

/// A brook thins and shoals toward its source into a marsh, with no pit: the bed at the source
/// is shallower than farther down.
#[test]
fn source_shoals_into_a_spring() {
    let w = mouths::source_window();
    let mesh = mesh_of(&w);
    let source = mesh.rivers.iter().find(|r| r.spring_at(0.) > 0.99).expect("a source");
    let down = mesh.rivers.iter().find(|r| r.spring_at(0.) < 0.01 && r.mouth_at(0.) < 0.01).expect("a reach below the spring");
    assert!(source.depth_at(0.) < 0.5 * down.depth_at(0.), "{} against {}", source.depth_at(0.), down.depth_at(0.));
    assert!(source.width < 0.6 * down.width);
}
