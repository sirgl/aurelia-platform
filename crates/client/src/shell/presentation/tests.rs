//! The presentation queue on hand-made cues: ordering, speed and Skip, holds, reset.
use kernel::ids::{EntityId, TileId};
use render::gpu::unit_poses::Gait;
use render::world::state::{RenderWorld, UnitMarker};

use super::cue::{Attack, AttackStyle, Cue, Derived, Figure, Origin};
use render::gpu::effects::{ExplosionKind, FxEvent, ImpactKind, MuzzleKind, ProjectileKind};
use render::unit_models::Weapon;
use super::motion::{self, Beat, Look, Map, MarkKind, Sound};
use super::{Presentation, Settings, Speed};

const WIDTH: u32 = 10;

fn figure(id: u64, owner: u8) -> Figure {
    Figure { id: EntityId(id), kind: 0, owner }
}

fn walk(id: u64, tiles: &[u32]) -> Cue {
    Cue::Move { unit: figure(id, 0), path: tiles.iter().map(|t| TileId(*t)).collect() }
}

fn own(cue: Cue) -> Derived {
    Derived { cue, origin: Origin::Own, concerns_observer: true }
}

fn other(cue: Cue, concerns_observer: bool) -> Derived {
    Derived { cue, origin: Origin::Other, concerns_observer }
}

fn presentation(speed: Speed) -> Presentation {
    let mut p = Presentation::default();
    p.reset(WIDTH);
    p.set_settings(Settings { speed, ..Settings::default() });
    p
}

/// The projection after the commands: every unit where the journal left it.
fn world(units: &[(u64, u32)]) -> RenderWorld {
    let units = units
        .iter()
        .map(|&(id, tile)| UnitMarker { id, embarked: false, fortified: false, tile: TileId(tile), owner: 0, kind: 0, hp: 100 })
        .collect();
    RenderWorld { width: WIDTH, height: WIDTH, units, ..Default::default() }
}

fn posed_ids(frame: &super::Frame) -> Vec<u64> {
    frame.poses.iter().map(|p| p.id).collect()
}

#[test]
fn cues_of_one_unit_play_in_order_and_other_units_move_at_once() {
    let mut p = presentation(Speed::Normal);
    p.push(vec![own(walk(1, &[0, 1, 2])), own(walk(1, &[2, 3])), own(walk(2, &[20, 21]))]);
    let w = world(&[(1, 3), (2, 21)]);
    let frame = p.advance(0.01, &w);
    assert_eq!(p.waiting(), 1, "the second move of unit 1 waits for the first");
    assert_eq!(posed_ids(&frame), vec![1, 2], "unit 2 moves together with unit 1");
    let walking = |frame: &super::Frame, id| frame.poses.iter().any(|p| p.id == id && matches!(p.gait, Gait::Walk { .. }));
    assert!(walking(&frame, 1) && walking(&frame, 2));
    let mut frames = 0;
    while p.waiting() > 0 {
        p.advance(0.05, &w);
        frames += 1;
        assert!(frames < 100);
    }
    // The first move takes two steps and a settle: about 0.8 s.
    assert!((14..=18).contains(&frames), "{frames} frames");
    for _ in 0..40 {
        p.advance(0.05, &w);
    }
    assert!(p.is_idle(), "everything played and the projection agrees");
}

#[test]
fn a_walk_follows_the_path_and_turns_to_face_it() {
    let mut p = presentation(Speed::Normal);
    p.push(vec![own(walk(1, &[0, 1]))]);
    let w = world(&[(1, 1)]);
    p.advance(0.0, &w);
    let frame = p.advance(0.15, &w);
    let pose = frame.poses[0];
    let (x0, _) = render::scene::layout::tile_to_world(0, 0);
    let (x1, _) = render::scene::layout::tile_to_world(1, 0);
    assert!(pose.position[0] > x0 && pose.position[0] < x1, "between the two tiles");
    let facing = render::gpu::unit_poses::facing_towards(1.0, 0.0);
    assert!((pose.facing - facing).abs() < 1e-3, "faces east");
    assert_eq!(pose.settle, 0.0);
}

#[test]
fn other_players_cues_follow_the_speed_setting() {
    let cue = || walk(5, &[0, 1, 2, 3]);
    let mut normal = presentation(Speed::Normal);
    let mut fast = presentation(Speed::Fast);
    normal.push(vec![other(cue(), false)]);
    fast.push(vec![other(cue(), false)]);
    let w = world(&[(5, 3)]);
    let frames = |p: &mut Presentation| {
        let mut n = 0;
        while !p.running.is_empty() || p.waiting() > 0 {
            p.advance(0.05, &w);
            n += 1;
        }
        n
    };
    let (slow, quick) = (frames(&mut normal), frames(&mut fast));
    assert!(quick * 2 < slow, "fast: {quick} frames, normal: {slow}");
    let mut skip = presentation(Speed::Skip);
    skip.push(vec![other(cue(), false), other(walk(6, &[4, 5]), true), own(walk(7, &[8, 9]))]);
    let frame = skip.advance(0.01, &world(&[(5, 3), (6, 5), (7, 9)]));
    assert_eq!(posed_ids(&frame), vec![6, 7], "skip drops what does not concern the player");
}

#[test]
fn an_attack_fires_its_sound_and_marks_then_the_death_follows() {
    let attack = Attack {
        attacker: figure(1, 0),
        from: TileId(0),
        target: TileId(1),
        defender: Some(figure(2, 1)),
        style: AttackStyle::Archery,
        weapon: Weapon::Bow,
        figures: 3,
        defender_hp: None,
        attacker_hp: None,
        reach: 1,
        dealt: Some(37),
        taken: None,
        kill: false,
        attacker_seen: true,
        target_owner: 1,
    };
    let mut p = presentation(Speed::Normal);
    p.push(vec![own(Cue::Attack(attack)), own(Cue::Death { unit: figure(2, 1), tile: TileId(1) })]);
    let w = world(&[(1, 0)]);
    let mut sounds = Vec::new();
    let mut marks = Vec::new();
    let mut fx = Vec::new();
    let mut fell = false;
    for _ in 0..60 {
        let frame = p.advance(0.05, &w);
        fx.extend(frame.fx);
        sounds.extend(frame.sounds);
        marks.extend(frame.marks.into_iter().map(|m| m.mark.kind));
        fell |= frame.poses.iter().any(|pose| pose.id == 2 && matches!(pose.gait, Gait::Fall { .. }));
    }
    assert_eq!(sounds, vec![Sound::Archery, Sound::Death]);
    let arrows = fx.iter().filter(|e| matches!(e, FxEvent::Projectile { kind: ProjectileKind::Arrow, .. })).count();
    assert_eq!(arrows, 3, "one arrow a figure of the formation");
    assert!(fx.iter().any(|e| matches!(e, FxEvent::Impact { kind: ImpactKind::Arrow, .. })));
    assert!(marks.contains(&MarkKind::Damage { amount: 37, owner: 1 }));
    assert!(fell, "the defender falls after the hit");
    assert!(p.is_idle(), "the fallen unit is released: the projection no longer shows it");
}

#[test]
fn a_finished_unit_is_held_until_the_projection_shows_it_there() {
    let mut p = presentation(Speed::Normal);
    p.push(vec![own(walk(1, &[0, 1]))]);
    let stale = world(&[(1, 0)]);
    for _ in 0..20 {
        p.advance(0.05, &stale);
    }
    let frame = p.advance(0.05, &stale);
    assert_eq!(posed_ids(&frame), vec![1], "held at the end of its walk while the projection lags");
    assert_eq!(frame.poses[0].tile, 1);
    let frame = p.advance(0.05, &world(&[(1, 1)]));
    assert!(frame.poses.is_empty(), "released once the projection agrees");
    // A projection that never agrees cannot hold a unit forever.
    p.push(vec![own(walk(1, &[1, 2]))]);
    for _ in 0..100 {
        p.advance(0.05, &stale);
    }
    assert!(p.is_idle());
}

#[test]
fn reset_drops_every_cue_hold_and_mark_of_the_old_world() {
    let mut p = presentation(Speed::Normal);
    p.push(vec![own(walk(1, &[0, 1, 2])), own(Cue::CityFounded { tile: TileId(4), owner: 0 }), own(walk(1, &[2, 3]))]);
    let frame = p.advance(0.1, &world(&[]));
    assert!(!frame.poses.is_empty() && !frame.marks.is_empty());
    p.reset(WIDTH);
    assert!(p.is_idle());
    // The new world has another unit 1 elsewhere: nothing of the old one is drawn.
    let frame = p.advance(0.1, &world(&[(1, 50)]));
    assert!(frame.poses.is_empty() && frame.marks.is_empty() && frame.sounds.is_empty());
    assert_eq!(p.settings().speed, Speed::Normal, "settings survive a reset");
}

#[test]
fn the_waiting_queue_is_bounded() {
    let mut p = presentation(Speed::Normal);
    let cues = (0..1000).map(|i| own(walk(1, &[i % 10, (i + 1) % 10]))).collect();
    p.push(cues);
    assert!(p.waiting() <= super::WAITING_LIMIT);
}

#[test]
fn a_wonder_glides_the_camera_unless_motion_is_reduced() {
    let wonder = || own(Cue::BuildingCompleted { tile: TileId(7), wonder: true });
    let mut p = presentation(Speed::Normal);
    p.push(vec![wonder()]);
    assert_eq!(p.advance(0.01, &world(&[])).camera, Some(TileId(7)));
    let mut calm = presentation(Speed::Normal);
    calm.set_settings(Settings { reduced_motion: true, ..Settings::default() });
    calm.push(vec![wonder()]);
    assert_eq!(calm.advance(0.01, &world(&[])).camera, None);
}

fn attack_with(weapon: Weapon) -> Attack {
    Attack {
        attacker: figure(1, 0),
        from: TileId(0),
        target: TileId(1),
        defender: Some(figure(2, 1)),
        style: AttackStyle::Melee,
        weapon,
        figures: 2,
        defender_hp: None,
        attacker_hp: None,
        reach: 1,
        dealt: Some(10),
        taken: None,
        kill: false,
        attacker_seen: true,
        target_owner: 1,
    }
}

/// Every effect of a cue with the clock time it starts at.
fn timeline(weapon: Weapon) -> Vec<(f32, FxEvent)> {
    let attack = attack_with(weapon);
    motion::moments(&Cue::Attack(attack), Map { width: 8 })
        .into_iter()
        .filter_map(|m| match m.beat {
            Beat::Fx(fx) => Some((m.at, fx)),
            _ => None,
        })
        .collect()
}

#[test]
fn every_weapon_visibly_fires_or_strikes_and_the_hit_comes_after_the_release() {
    use super::cue::Timing;
    for weapon in [
        Weapon::None, Weapon::Blade, Weapon::Club, Weapon::Polearm, Weapon::Bow, Weapon::Crossbow, Weapon::Thrown,
        Weapon::Firearm, Weapon::Autogun, Weapon::Rocket, Weapon::Cannon, Weapon::Catapult, Weapon::Ballista,
        Weapon::Ram, Weapon::Broadside, Weapon::Torpedo, Weapon::Bomb, Weapon::Missile,
    ] {
        let timing = Timing::of(weapon);
        assert!(timing.release <= timing.hit && timing.hit < timing.seconds, "{weapon:?}");
        let fx = timeline(weapon);
        assert!(!fx.is_empty(), "{weapon:?} has an effect");
        if weapon.is_ranged() {
            assert!(fx.iter().any(|(_, e)| matches!(e, FxEvent::Projectile { .. })), "{weapon:?} sends a projectile");
            let first = fx.iter().map(|(at, _)| *at).fold(f32::INFINITY, f32::min);
            assert!(first >= timing.release - 1e-4, "{weapon:?} fires at the release, not before");
            let last_flight = fx.iter().filter_map(|(at, e)| match e {
                FxEvent::Projectile { seconds, .. } => Some(at + seconds),
                _ => None,
            });
            for arrival in last_flight {
                assert!(arrival <= timing.seconds + 1e-4, "{weapon:?} lands before the cue ends");
            }
        } else {
            assert!(fx.iter().all(|(at, _)| (*at - timing.hit).abs() < 1e-4), "{weapon:?} strikes at the hit");
        }
    }
    let muzzles = |w| timeline(w).iter().filter(|(_, e)| matches!(e, FxEvent::Muzzle { .. })).count();
    assert!(muzzles(Weapon::Cannon) >= 1 && muzzles(Weapon::Firearm) == 2, "a formation of two fires two rifles");
    assert!(timeline(Weapon::Cannon).iter().any(|(_, e)| matches!(e, FxEvent::Explosion { kind: ExplosionKind::Shell, .. })));
    assert!(timeline(Weapon::Cannon).iter().any(|(_, e)| matches!(e, FxEvent::Muzzle { kind: MuzzleKind::Cannon, .. })));
}

#[test]
fn a_ranged_attacker_does_not_lunge_and_a_melee_one_does() {
    let at = |weapon: Weapon, t: f32| {
        let attack = attack_with(weapon);
        let entry = Look::resting(Map { width: 8 }, TileId(0));
        motion::look(&Cue::Attack(attack), EntityId(1), t, entry, Map { width: 8 }).expect("look")
    };
    for weapon in [Weapon::Bow, Weapon::Cannon, Weapon::Firearm] {
        let timing = super::cue::Timing::of(weapon);
        match at(weapon, timing.release - 0.01).gait {
            Gait::Attack { aim, strike } => assert!(aim > 0.9 && strike == 0.0, "{weapon:?} aims before it fires"),
            other => panic!("{other:?}"),
        }
        match at(weapon, timing.release + 0.1).gait {
            Gait::Attack { strike, .. } => assert!(strike > 0.0, "{weapon:?} follows through"),
            other => panic!("{other:?}"),
        }
    }
}

// ---- the air war: flights, sorties, nuclear launches, paradrops --------------------------

fn run_to_idle(p: &mut Presentation, w: &RenderWorld, limit: usize) -> Vec<super::Frame> {
    let mut frames = Vec::new();
    for _ in 0..limit {
        let frame = p.advance(1.0 / 30.0, w);
        let busy = frame.busy;
        frames.push(frame);
        if !busy {
            break;
        }
    }
    frames
}

fn flying(frame: &super::Frame, id: u64) -> Option<f32> {
    frame.poses.iter().find(|p| p.id == id).and_then(|p| match p.gait {
        Gait::Flight { altitude, .. } => Some(altitude),
        _ => None,
    })
}

#[test]
fn a_rebase_flies_an_arc_and_lands_without_teleporting() {
    use render::gpu::flight::FlightClass;
    let mut p = presentation(Speed::Normal);
    p.push(vec![own(Cue::Flight { unit: figure(1, 0), from: TileId(12), to: TileId(18), reach: 6, class: FlightClass::Plane })]);
    let w = world(&[(1, 18)]);
    let frames = run_to_idle(&mut p, &w, 400);
    let altitudes: Vec<f32> = frames.iter().filter_map(|f| flying(f, 1)).collect();
    assert!(altitudes.len() > 40, "{} flying frames", altitudes.len());
    assert!(altitudes.iter().cloned().fold(0.0, f32::max) > 0.5, "cruises well above the ground");
    // The position moves in small steps all the way (no jump from base to base).
    let positions: Vec<[f32; 2]> = frames.iter().filter_map(|f| f.poses.iter().find(|p| p.id == 1).map(|p| p.position)).collect();
    for pair in positions.windows(2) {
        let step = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
        assert!(step < 0.4, "frame step {step}");
    }
    let last = frames.last().unwrap();
    assert!(last.poses.iter().all(|pose| !matches!(pose.gait, Gait::Flight { .. })), "parked at the end");
}

#[test]
fn a_nuclear_launch_hides_the_weapon_at_the_blast_and_fires_one_blast() {
    use render::gpu::effects::{AirFx, FxEvent};
    let mut p = presentation(Speed::Normal);
    let blast = vec![TileId(18)];
    p.push(vec![own(Cue::Nuke { unit: figure(1, 0), from: TileId(2), target: TileId(18), reach: 5, blast })]);
    // A victim of the blast waits for the flash (it is keyed by the blast tiles).
    p.push(vec![own(Cue::Death { unit: figure(9, 1), tile: TileId(18) })]);
    let w = world(&[(9, 18)]);
    let frames = run_to_idle(&mut p, &w, 600);
    let blasts = frames.iter().flat_map(|f| f.fx.iter()).filter(|e| matches!(e, FxEvent::Air(AirFx::Blast { .. }))).count();
    assert_eq!(blasts, 1);
    let first_blast = frames.iter().position(|f| f.fx.iter().any(|e| matches!(e, FxEvent::Air(AirFx::Blast { .. })))).unwrap();
    let first_fall = frames.iter().position(|f| f.poses.iter().any(|p| p.id == 9 && matches!(p.gait, Gait::Fall { .. }))).unwrap_or(usize::MAX);
    assert!(first_fall >= first_blast, "the victim falls after the flash, not before the missile is launched");
    assert!(frames.iter().any(|f| f.marks.iter().any(|m| matches!(m.mark.kind, MarkKind::Warning { .. }))), "the target is marked");
}

#[test]
fn a_paradrop_shows_a_transport_plane_and_the_soldier_sinking() {
    let mut p = presentation(Speed::Normal);
    p.push(vec![own(Cue::Paradrop { unit: figure(1, 0), from: TileId(12), to: TileId(17), reach: 5, plane: 1 })]);
    let w = world(&[(1, 17)]);
    let frames = run_to_idle(&mut p, &w, 600);
    let plane = super::air::plane_id(1);
    assert!(frames.iter().any(|f| f.poses.iter().any(|pose| pose.id == plane && pose.kind == 1)), "the transport is a pose of its own");
    let hidden = frames.iter().any(|f| f.poses.iter().any(|pose| pose.id == 1 && matches!(pose.gait, Gait::Hidden)));
    let sinking = frames.iter().filter_map(|f| flying(f, 1)).collect::<Vec<_>>();
    assert!(hidden, "inside the plane first");
    assert!(sinking.len() > 15 && sinking.windows(2).all(|w| w[1] <= w[0] + 1e-3), "then sinks steadily: {sinking:?}");
}
