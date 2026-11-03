//! Units the presentation moves this frame: walking along a path, striking, recoiling,
//! falling. Procedural, bone-less animation: every figure of the formation gets its own
//! transform (position on the surface, yaw, height scale, flash, fade).
//!
//! A posed unit is drawn from this small per-frame array instead of the resting unit
//! instances: `UnitBatches` collapses its resting figures (same array length, so only
//! those figures are written once when the pose starts and once when it ends). The
//! array here is rebuilt and uploaded every frame while poses exist — a few hundred
//! instances at most — and never touches terrain, features or the resting units.
//!
//! The presentation names units by their persistent id (`UnitMarker::id`); a pose of a
//! unit the projection no longer shows (it died in this command) is drawn all the same.

use std::collections::HashMap;
use std::ops::Range;

use crate::gpu::buffers::{owner_color, HexInstance};
use crate::gpu::figures::{rank as figure_rank, visible_figures};
use crate::gpu::unit_batches::{seat_figure, stack_offset, Stance, UnitDraw};
use crate::scene::fog::fog_at;
use crate::scene::style::CompiledTerrainStyle;
use crate::scene::terrain::fields::TerrainGeometry;
use crate::unit_models::{select_lod, UnitModel, UnitModelBook, UnitModelId, Weapon, MARKER_LOD};
use crate::world::state::RenderWorld;

/// What the figures of a posed unit do this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gait {
    /// Standing (waiting for its turn in the presentation queue, or settling).
    Stand,
    /// Walking: `phase` counts steps (one bob per unit of phase).
    Walk { phase: f32 },
    /// An attack with the unit's weapon (its `Weapon` decides what that looks like): `aim`
    /// 0..1 is the wind-up (draw, shouldering, taking aim); `strike` 0..1 runs from the
    /// moment the blow lands or the weapon fires to the end of the recovery (0 before).
    Attack { aim: f32, strike: f32 },
    /// Pushed back by a hit: 0 none, 1 the full step back.
    Recoil { amount: f32 },
    /// Falling: 0 standing, 1 down and faded.
    Fall { progress: f32 },
    /// In the air (aircraft, missiles, a paratrooper under a canopy): `altitude` above the
    /// ground in world units, `pitch` (nose up) and `bank` (right wing down) in radians.
    /// A flat shadow is drawn on the ground below (`gpu::flight`).
    Flight { altitude: f32, pitch: f32, bank: f32 },
    /// Not drawn (it rides inside something else: an airlifted soldier in the transport).
    Hidden,
}

/// One unit drawn by the presentation this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitPose {
    /// Persistent id of the unit (its resting figures collapse while it is posed).
    pub id: u64,
    pub kind: u16,
    pub owner: u8,
    /// Tile under the unit: its fog factor.
    pub tile: u32,
    /// Formation centre, world XZ.
    pub position: [f32; 2],
    /// Yaw: models face +z at 0 (`facing_towards` turns them).
    pub facing: f32,
    pub gait: Gait,
    /// Hit flash: 0 none, 1 white.
    pub flash: f32,
    /// Health in percent before and after the blow this pose shows (`figures`): the
    /// figures that stood before and do not after fall as `change` runs 0..1. Equal
    /// values: a plain count. Zeros: unknown, the resting figures' count is kept.
    pub hp: [u8; 2],
    pub change: f32,
    /// 0 while acting, 1 at the unit's resting place (its stack slot and stance): the
    /// last moments of a pose blend into the resting figures, so the hand-over is seamless.
    pub settle: f32,
}

/// Yaw that turns a model (facing +z) towards the XZ direction `(dx, dz)`.
pub fn facing_towards(dx: f32, dz: f32) -> f32 {
    if dx.abs() + dz.abs() < 1e-6 {
        return 0.0;
    }
    (-dx).atan2(dz)
}

/// Where a posed unit would rest: its stack slot and stance in the projection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Rest {
    pub stack: u8,
    pub fortified: bool,
}

/// The posed figures of the frame, grouped by model (leaders first).
#[derive(Debug, Default)]
pub struct PosedFigures {
    instances: Vec<HexInstance>,
    /// Per model with figures: (model, leaders, all) ranges into `instances`.
    runs: Vec<(UnitModelId, Range<u32>, Range<u32>)>,
}

/// One figure before grouping.
struct Placed {
    model: UnitModelId,
    tier: u8,
    instance: HexInstance,
}

impl PosedFigures {
    pub fn instances(&self) -> &[HexInstance] {
        &self.instances
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Places every figure of every pose on the surface `geom`.
    pub fn build(
        &mut self,
        poses: &[UnitPose],
        rests: &HashMap<u64, Rest>,
        book: &UnitModelBook,
        world: &RenderWorld,
        style: &CompiledTerrainStyle,
        geom: &TerrainGeometry,
    ) {
        let mut placed = Vec::new();
        for pose in poses {
            let rest = rests.get(&pose.id).copied().unwrap_or_default();
            place_pose(pose, rest, book, world, style, geom, &mut placed);
        }
        placed.sort_by_key(|p| (p.model, p.tier));
        self.instances.clear();
        self.runs.clear();
        let mut start = 0;
        while start < placed.len() {
            let model = placed[start].model;
            let end = start + placed[start..].iter().take_while(|p| p.model == model).count();
            let leaders = placed[start..end].iter().take_while(|p| p.tier == 0).count();
            let first = start as u32;
            self.runs.push((model, first..first + leaders as u32, first..end as u32));
            start = end;
        }
        self.instances.extend(placed.into_iter().map(|p| p.instance));
    }

    /// Draws at the LOD for `px_per_unit`; the far marker draws leaders only.
    pub fn draws(&self, px_per_unit: f32) -> Vec<UnitDraw> {
        let mut draws = Vec::with_capacity(self.runs.len());
        for (id, leaders, all) in &self.runs {
            let model = id.model();
            let lod = select_lod(model, px_per_unit, 0);
            let (start, count) = model.lods[lod];
            let instances = if lod == MARKER_LOD { leaders.clone() } else { all.clone() };
            if !instances.is_empty() {
                draws.push(UnitDraw { vertices: start..start + count, instances });
            }
        }
        draws
    }

    /// Shadow casters: every posed figure at the reduced model.
    pub fn caster_draws(&self) -> Vec<UnitDraw> {
        self.runs
            .iter()
            .map(|(id, _, all)| {
                let (start, count) = id.model().lods[1];
                UnitDraw { vertices: start..start + count, instances: all.clone() }
            })
            .collect()
    }
}

fn place_pose(
    pose: &UnitPose,
    rest: Rest,
    book: &UnitModelBook,
    world: &RenderWorld,
    style: &CompiledTerrainStyle,
    geom: &TerrainGeometry,
    out: &mut Vec<Placed>,
) {
    // On water a land unit is a boat, along the whole walk (`UnitModelBook::embarks`).
    let afloat = book.embarks(pose.kind) && world.is_water(pose.tile as usize);
    let id = book.model_for_unit(pose.kind, afloat);
    let model = id.model();
    let settle = pose.settle.clamp(0.0, 1.0);
    let offset = stack_offset(rest.stack);
    let centre = [pose.position[0] + offset[0] * settle, pose.position[1] + offset[1] * settle];
    let stance = Stance::of(rest.fortified);
    let spread = 1.0 + (stance.spread - 1.0) * settle;
    let crouch = 1.0 + (stance.height - 1.0) * settle;
    let fog = fog_at(world, kernel::ids::TileId(pose.tile));
    // A foreign aircraft over a tile the observer does not see right now is not drawn.
    let unseen = world.visible.get(pose.tile as usize).copied().unwrap_or(0) == 0;
    if unseen && pose.owner != world.observer && matches!(pose.gait, Gait::Flight { .. }) {
        return;
    }
    let color = flashed(owner_color(pose.owner), pose.flash);
    let (sin, cos) = pose.facing.sin_cos();
    let forward = [-sin, cos];
    let total = model.slots.len();
    let hp = if pose.hp == [0, 0] { [100, 100] } else { pose.hp };
    let (standing, before) = (visible_figures(total, hp[1]), visible_figures(total, hp[0]));
    for (i, [x, z, s]) in model.slots.iter().enumerate() {
        let rank = figure_rank(pose.id, i, total);
        // Figures that left with this blow fall while it plays and are gone after.
        let falling = rank >= standing && rank < before && pose.change < 1.0;
        if rank >= standing && !falling {
            continue;
        }
        let gait = if falling { Gait::Fall { progress: pose.change.clamp(0.0, 1.0) } } else { pose.gait };
        let motion = FigureMotion::of(gait, i, model.height * s, model);
        if motion.hidden {
            continue;
        }
        let (lx, lz) = (x * spread, z * spread);
        let px = centre[0] + lx * cos - lz * sin + forward[0] * motion.forward;
        let pz = centre[1] + lx * sin + lz * cos + forward[1] * motion.forward;
        let (ground, slope) = seat_figure(world, style, geom, pose.tile, px, pz, id, *s);
        let mut instance = HexInstance::upright(
            [px, ground + motion.lift, pz],
            color,
            [s * motion.width, s * crouch * motion.height],
            fog * motion.light,
        );
        // The catalog bakes a turn into every model (people look towards the camera): the
        // heading of a unit under way is its own, blended away as it settles.
        instance.yaw = pose.facing + model.baked_yaw * (1.0 - settle) + motion.twist;
        instance.slope = slope;
        instance.anim = motion.anim;
        instance.rig = model.rig_state();
        instance.rig[0] = motion.action;
        if motion.pitch != 0.0 || motion.bank != 0.0 {
            instance.slope = crate::gpu::flight::shear(instance.yaw, motion.pitch, motion.bank);
        }
        if motion.airborne > 0.02 {
            out.push(Placed { model: id, tier: u8::from(i > 0), instance: crate::gpu::flight::ground_shadow(&instance, ground, motion.airborne) });
        }
        out.push(Placed { model: id, tier: u8::from(i > 0), instance });
    }
}

/// One figure's share of a gait: offsets in hex units, scales, light and the shader's
/// animation state.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FigureMotion {
    forward: f32,
    lift: f32,
    twist: f32,
    width: f32,
    height: f32,
    light: f32,
    /// `HexInstance::anim`.
    anim: [f32; 4],
    /// `HexInstance::rig[0]`: the action.
    action: f32,
    /// Nose-up and right-wing-down angles (radians): the instance's ground shear.
    pitch: f32,
    bank: f32,
    /// Height of the figure above the ground it throws a flat shadow on (0: none).
    airborne: f32,
    hidden: bool,
}

/// Seconds of a recoil kick: 1 at the shot, falling to 0 (`strike` runs 0..1).
fn kick(strike: f32) -> f32 {
    if strike <= 0.0 {
        return 0.0;
    }
    (1.0 - strike.min(1.0)).powi(3)
}

impl FigureMotion {
    const REST: Self = Self {
        forward: 0.0,
        lift: 0.0,
        twist: 0.0,
        width: 1.0,
        height: 1.0,
        light: 1.0,
        anim: [0.0; 4],
        action: 0.0,
        pitch: 0.0,
        bank: 0.0,
        airborne: 0.0,
        hidden: false,
    };

    /// `figure`: index in the formation (followers lag a little); `size`: figure height.
    fn of(gait: Gait, figure: usize, size: f32, model: &UnitModel) -> Self {
        let lag = figure as f32;
        match gait {
            Gait::Stand => Self::REST,
            Gait::Hidden => Self { hidden: true, ..Self::REST },
            Gait::Flight { altitude, pitch, bank } => {
                let profile = crate::gpu::flight::profile(model);
                Self {
                    lift: altitude,
                    pitch: pitch * profile.pitch_gain,
                    bank: bank * profile.bank_gain,
                    airborne: altitude,
                    ..Self::REST
                }
            }
            Gait::Walk { .. } if model.contact == crate::unit_models::Contact::Hover => {
                // A helicopter on the move: it rises a little and leans into the flight.
                let Gait::Walk { phase } = gait else { unreachable!() };
                let step = std::f32::consts::PI * (phase + lag * 0.37);
                Self {
                    lift: 0.10 + step.sin().abs() * 0.01,
                    pitch: -0.12,
                    airborne: 0.10,
                    anim: [step, 1.0, 0.0, 0.0],
                    ..Self::REST
                }
            }
            Gait::Walk { phase } => {
                let step = std::f32::consts::PI * (phase + lag * 0.37);
                Self {
                    lift: step.sin().abs() * size * 0.04,
                    twist: (step * 0.5).sin() * 0.05,
                    anim: [step, 1.0, 0.0, 0.0],
                    ..Self::REST
                }
            }
            Gait::Attack { aim, strike } => {
                let weapon = model.weapon;
                // Followers act a moment later, so a formation does not move as one.
                let aim = (aim * 1.05 - lag * 0.03).clamp(0.0, 1.0);
                let strike = if strike > 0.0 { (strike - lag * 0.05).max(0.0) } else { 0.0 };
                let (forward, twist, lift) = match weapon {
                    Weapon::None | Weapon::Blade | Weapon::Club | Weapon::Polearm | Weapon::Ram => {
                        // Draw back, lunge to the blow, return.
                        let lunge = if strike <= 0.0 {
                            0.0
                        } else if strike < 0.3 {
                            let u = strike / 0.3;
                            u * u * (3.0 - 2.0 * u)
                        } else {
                            let u = (strike - 0.3) / 0.7;
                            1.0 - u * u * (3.0 - 2.0 * u)
                        };
                        let back = -0.10 * aim * (1.0 - (strike / 0.15).min(1.0));
                        (back + 0.55 * lunge, 0.3 * lunge - 0.2 * aim * (1.0 - lunge), (lunge * std::f32::consts::PI).sin() * size * 0.05)
                    }
                    Weapon::Bow | Weapon::Crossbow | Weapon::Thrown => (-0.03 * aim - 0.04 * kick(strike), 0.0, 0.0),
                    Weapon::Firearm | Weapon::Autogun | Weapon::Rocket => (-0.07 * kick(strike), 0.0, 0.0),
                    Weapon::Cannon | Weapon::Catapult | Weapon::Ballista => (-0.15 * kick(strike), 0.0, 0.0),
                    Weapon::Broadside | Weapon::Torpedo | Weapon::Bomb | Weapon::Missile => (-0.05 * kick(strike), 0.0, 0.0),
                };
                Self {
                    forward,
                    twist,
                    lift,
                    anim: [0.0, 0.0, aim, strike],
                    action: weapon.action(),
                    ..Self::REST
                }
            }
            Gait::Recoil { amount } => Self {
                forward: -amount * 0.14,
                twist: -amount * 0.2,
                anim: [0.0, 0.0, amount, 0.0],
                action: crate::unit_models::ACTION_HIT,
                ..Self::REST
            },
            Gait::Fall { progress } => {
                let fall = (progress * 1.3 - lag * 0.07).clamp(0.0, 1.0);
                Self {
                    lift: -fall * size * 0.35,
                    twist: fall * 0.7,
                    width: 1.0 - 0.2 * fall,
                    height: 1.0 - 0.8 * fall,
                    light: 1.0 - 0.75 * fall,
                    ..Self::REST
                }
            }
        }
    }
}

fn flashed(color: [f32; 3], flash: f32) -> [f32; 3] {
    let t = flash.clamp(0.0, 1.0);
    let white = [1.0, 0.96, 0.86];
    [0, 1, 2].map(|i| color[i] + (white[i] - color[i]) * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::heightfield::build_heightfield;
    use crate::scene::style::{compile, default_style};
    use crate::scene::layout::tile_to_world;
    use crate::world::state::TerrainCell;
    use kernel::ids::TileId;

    fn flat_world() -> (RenderWorld, CompiledTerrainStyle, TerrainGeometry) {
        let mut world = RenderWorld { width: 8, height: 8, ..Default::default() };
        for tile in 0..64 {
            world.terrain.push(TerrainCell { tile: TileId(tile), terrain: 0, owner: None, resource: None });
        }
        world.visible = vec![1; 64];
        world.explored = vec![1; 64];
        let style = compile(default_style()).unwrap();
        let hf = build_heightfield(&world, &style);
        let geom = crate::scene::terrain::fields::build_terrain_geometry(&world, &style, &hf);
        (world, style, geom)
    }

    fn pose(gait: Gait, facing: f32) -> UnitPose {
        let (x, z) = tile_to_world(3, 3);
        UnitPose { id: 7, kind: 0, owner: 1, tile: 27, position: [x, z], facing, gait, flash: 0.0, settle: 0.0, hp: [100, 100], change: 0.0 }
    }

    fn figures(poses: &[UnitPose]) -> PosedFigures {
        let (world, style, geom) = flat_world();
        let book = UnitModelBook::from_models([crate::unit_models::model_by_id("unit/warrior")]);
        let mut posed = PosedFigures::default();
        posed.build(poses, &HashMap::new(), &book, &world, &style, &geom);
        posed
    }

    #[test]
    fn a_turned_formation_keeps_its_offsets_and_faces_its_direction() {
        let still = figures(&[pose(Gait::Stand, 0.0)]);
        let turned = figures(&[pose(Gait::Stand, std::f32::consts::FRAC_PI_2)]);
        let (x, z) = tile_to_world(3, 3);
        let distance = |i: &HexInstance| (i.offset[0] - x).hypot(i.offset[2] - z);
        for (a, b) in still.instances().iter().zip(turned.instances()) {
            assert!((distance(a) - distance(b)).abs() < 1e-4, "same distance from the centre");
            let baked = crate::unit_models::model_by_id("unit/warrior").unwrap().model().baked_yaw;
            assert!((b.yaw - std::f32::consts::FRAC_PI_2 - baked).abs() < 1e-6, "heading plus the turn baked into the model");
        }
        let direction = facing_towards(1.0, 0.0);
        let (sin, cos) = direction.sin_cos();
        assert!((-sin - 1.0).abs() < 1e-6 && cos.abs() < 1e-6, "facing east turns +z to +x");
    }

    #[test]
    fn a_strike_lunges_forward_and_a_fall_sinks_and_fades() {
        let rest = figures(&[pose(Gait::Stand, 0.0)]);
        let strike = figures(&[pose(Gait::Attack { aim: 1.0, strike: 0.3 }, 0.0)]);
        let fall = figures(&[pose(Gait::Fall { progress: 1.0 }, 0.0)]);
        let leader = |p: &PosedFigures| p.instances()[0];
        assert!(leader(&strike).offset[2] > leader(&rest).offset[2] + 0.3, "lunge along +z");
        assert!(leader(&fall).offset[1] < leader(&rest).offset[1]);
        assert!(leader(&fall).scale_height[1] < leader(&rest).scale_height[1] * 0.5);
        assert!(leader(&fall).fog < leader(&rest).fog * 0.5);
    }

    #[test]
    fn a_flier_hangs_above_the_ground_with_a_shadow_and_foreign_ones_hide_in_the_fog() {
        use crate::unit_models::{models, Rig, UnitModelId};
        // Aircraft belong to the classic part's models; a build without them has nothing to fly.
        let Some(flier) = models().iter().position(|m| m.rig == Rig::Plane).map(|i| UnitModelId(i as u16)) else {
            eprintln!("no aircraft model in this build (the classic part is absent): nothing to check");
            return;
        };
        let (mut world, style, geom) = flat_world();
        world.observer = 1;
        let book = UnitModelBook::from_models([Some(flier)]);
        let bomber = flier.model();
        let fly = |world: &RenderWorld, owner: u8| {
            let mut posed = PosedFigures::default();
            let up = UnitPose { kind: 0, owner, ..pose(Gait::Flight { altitude: 0.6, pitch: 0.2, bank: 0.3 }, 0.0) };
            posed.build(&[up], &HashMap::new(), &book, world, &style, &geom);
            posed
        };
        let own = fly(&world, 1);
        assert_eq!(own.instances().len(), bomber.slots.len() * 2, "the aircraft and its ground shadow");
        let ground = crate::scene::terrain::evaluator::sample_surface(&geom, own.instances()[0].offset[0], own.instances()[0].offset[2])
            .map(|s| s.y)
            .unwrap_or(0.0);
        let (shadow, body) = (own.instances()[0], own.instances()[1]);
        assert!(body.offset[1] > ground + 0.55, "flies at altitude: {} over {ground}", body.offset[1]);
        assert!(shadow.offset[1] < ground + 0.05 && shadow.scale_height[1] < 0.05, "the shadow lies on the ground");
        assert!(body.slope != [0.0, 0.0], "banked and pitched");
        world.visible[27] = 0;
        assert!(fly(&world, 2).instances().is_empty(), "a foreign aircraft in the fog is not drawn");
        assert_eq!(fly(&world, 1).instances().len(), bomber.slots.len() * 2, "your own always is");
    }

    #[test]
    fn walking_figures_stay_on_the_ground_or_above_it() {
        let (world, style, geom) = flat_world();
        let walk = figures(&[pose(Gait::Walk { phase: 0.5 }, 0.3)]);
        for instance in walk.instances() {
            let ground = crate::scene::terrain::evaluator::sample_surface(&geom, instance.offset[0], instance.offset[2])
                .map(|s| s.y)
                .unwrap_or_else(|| style.height(world.terrain[0].terrain).unwrap_or(0.0));
            assert!(instance.offset[1] >= ground, "figure under the ground");
        }
    }

    #[test]
    fn a_land_unit_walking_on_water_is_posed_as_the_boat_and_on_land_as_its_soldiers() {
        use crate::unit_models::model_by_id;
        let model = |id: &str| model_by_id(id).unwrap_or_else(|| panic!("{id} is not a baked model"));
        let (mut world, style, _) = flat_world();
        world.terrain[27].terrain = world.water_code();
        let hf = build_heightfield(&world, &style);
        let geom = crate::scene::terrain::fields::build_terrain_geometry(&world, &style, &hf);
        let book = UnitModelBook::from_models([Some(model("unit/swordsman")), Some(model("unit/trireme"))])
            .with_embarked(Some(model("unit/workboat")), [true, false]);
        let posed = |kind: u16, tile: u32| {
            let mut posed = PosedFigures::default();
            posed.build(&[UnitPose { kind, tile, ..pose(Gait::Walk { phase: 0.5 }, 0.3) }], &HashMap::new(), &book, &world, &style, &geom);
            posed.instances().len()
        };
        let boat = model("unit/workboat").model().slots.len();
        let soldiers = model("unit/swordsman").model().slots.len();
        assert!(soldiers > boat);
        assert_eq!(posed(0, 27), boat, "afloat: the boat along the whole walk");
        assert_eq!(posed(0, 26), soldiers, "ashore: the soldiers");
        assert_eq!(posed(1, 27), model("unit/trireme").model().slots.len(), "a ship is a ship");
    }

    #[test]
    fn draws_cover_every_posed_figure_and_markers_only_leaders() {
        let posed = figures(&[pose(Gait::Stand, 0.0), UnitPose { id: 8, ..pose(Gait::Stand, 0.0) }]);
        let figures_drawn = |draws: &[UnitDraw]| draws.iter().map(|d| d.instances.len()).sum::<usize>();
        assert_eq!(figures_drawn(&posed.draws(60.0)), posed.instances().len());
        assert_eq!(figures_drawn(&posed.draws(0.2)), 2, "one marker per unit far away");
        assert_eq!(figures_drawn(&posed.caster_draws()), posed.instances().len());
    }
}
