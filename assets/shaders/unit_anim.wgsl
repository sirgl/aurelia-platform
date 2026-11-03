// Procedural unit animation: walk cycles, idle breathing, attacks, hits.
//
// The baked unit models are bone-less meshes. A vertex is moved by what it is: which body
// part it belongs to is read from where it lies in the model (legs under the hips, arms
// at the sides of the chest, a horse's legs under its belly), in the model's own axes
// (`rig.w` is the yaw the catalog baked in; `rig.z` converts hex units back to the
// catalog's authoring units, where a person is 1.0 tall). The same function drives the
// scene pass and the shadow pass. docs/animation.md describes the rigs.
//
// anim: x walk phase (radians, one step = pi), y walk amplitude 0..1,
//       z aim 0..1 (wind-up, draw, shouldering), w strike 0..1 (from release to rest).
// rig:  x action (0 none, 1 + weapon index = attack, 100 = hit reaction with amount in
//       anim.z), y rig index (0 none), z 1 / model scale, w baked yaw (radians).

struct Posed {
    pos: vec3<f32>,
    normal: vec3<f32>,
};

const RIG_BIPED: u32 = 1u;
const RIG_HORSE: u32 = 2u;
const RIG_ELEPHANT: u32 = 3u;
const RIG_CAMEL: u32 = 4u;
const RIG_CHARIOT: u32 = 5u;
const RIG_CART: u32 = 6u;
const RIG_ENGINE: u32 = 7u;
const RIG_VEHICLE: u32 = 8u;
const RIG_VESSEL: u32 = 9u;
const RIG_PLANE: u32 = 10u;

const W_NONE: u32 = 1u;
const W_BLADE: u32 = 2u;
const W_CLUB: u32 = 3u;
const W_POLEARM: u32 = 4u;
const W_BOW: u32 = 5u;
const W_CROSSBOW: u32 = 6u;
const W_THROWN: u32 = 7u;
const W_FIREARM: u32 = 8u;
const W_AUTOGUN: u32 = 9u;
const W_ROCKET: u32 = 10u;
const W_CANNON: u32 = 11u;
const ACTION_HIT: u32 = 100u;

// Rotation about the model's x axis (the lateral one) around the (y, z) pivot: for a
// point under the pivot a positive angle swings it backwards.
fn rot_x(p: vec3<f32>, pivot: vec2<f32>, ang: f32) -> vec3<f32> {
    let c = cos(ang);
    let s = sin(ang);
    let dy = p.y - pivot.x;
    let dz = p.z - pivot.y;
    return vec3<f32>(p.x, pivot.x + dy * c - dz * s, pivot.y + dy * s + dz * c);
}

fn rot_x_dir(n: vec3<f32>, ang: f32) -> vec3<f32> {
    let c = cos(ang);
    let s = sin(ang);
    return vec3<f32>(n.x, n.y * c - n.z * s, n.y * s + n.z * c);
}

// Rotation about the forward (z) axis around the (x, y) pivot: a roll.
fn rot_z(p: vec3<f32>, pivot: vec2<f32>, ang: f32) -> vec3<f32> {
    let c = cos(ang);
    let s = sin(ang);
    let dx = p.x - pivot.x;
    let dy = p.y - pivot.y;
    return vec3<f32>(pivot.x + dx * c - dy * s, pivot.y + dx * s + dy * c, p.z);
}

fn rot_z_dir(n: vec3<f32>, ang: f32) -> vec3<f32> {
    let c = cos(ang);
    let s = sin(ang);
    return vec3<f32>(n.x * c - n.y * s, n.x * s + n.y * c, n.z);
}

fn swing(p: ptr<function, Posed>, pivot: vec2<f32>, ang: f32, weight: f32) {
    let a = ang * weight;
    (*p).pos = rot_x((*p).pos, pivot, a);
    (*p).normal = rot_x_dir((*p).normal, a);
}

fn unit_hash(seed: f32) -> f32 {
    return fract(sin(seed * 12.9898) * 43758.5453);
}

fn smooth01(a: f32, b: f32, x: f32) -> f32 {
    return smoothstep(a, b, x);
}

// Arm angle of an attack: `aim` 0..1 and `strike` 0..1; right arm (side > 0) and left.
// Negative angles reach forward and up, positive ones swing back.
fn arm_attack(action: u32, side: f32, aim: f32, strike: f32) -> f32 {
    let right = side > 0.0;
    let recover = clamp(1.0 - strike, 0.0, 1.0);
    switch (action) {
        case 5u: { // bow: the left arm holds the bow out, the right draws and lets go
            if (!right) {
                return -1.45 * aim * (0.35 + 0.65 * recover);
            }
            let draw = -1.25 * min(aim * 2.0, 1.0) + 1.15 * smoothstep(0.5, 1.0, aim);
            return mix(draw, -0.5, smoothstep(0.0, 0.25, strike) * recover);
        }
        case 6u: { // crossbow: both arms hold it forward
            return -1.15 * aim + 0.25 * recover * strike;
        }
        case 7u: { // thrown: wind up over the shoulder, throw
            if (!right) {
                return -0.5 * aim;
            }
            let back = 1.9 * aim;
            let thrown = mix(back, -1.2, smoothstep(0.0, 0.3, strike));
            return mix(thrown, 0.0, smoothstep(0.3, 1.0, strike));
        }
        case 8u, 9u, 10u: { // firearm, automatic gun, rocket: shouldered, kicks when it fires
            let kick = strike * exp(-strike * 5.0) * 2.4;
            var shake = 0.0;
            if (action == 9u) {
                shake = sin(strike * 90.0) * 0.07 * recover;
            }
            return -1.3 * aim + 0.3 * kick + shake;
        }
        default: { // blade, club, polearm, bare hands: raise, strike, recover
            if (!right) {
                return -0.5 * aim * recover;
            }
            let raised = 1.7 * aim;
            let struck = mix(raised, -0.95, smoothstep(0.0, 0.28, strike));
            return mix(struck, 0.0, smoothstep(0.28, 1.0, strike));
        }
    }
}

// Deforms one vertex of a model. `model_pos` and `model_normal` are in hex units, in
// the model's own frame. `time` is the shader clock (frozen when the game is paused);
// `seed` differs per figure.
fn animate_unit(model_pos: vec3<f32>, model_normal: vec3<f32>, anim: vec4<f32>, rig: vec4<f32>, time: f32, seed: f32) -> Posed {
    var out: Posed;
    out.pos = model_pos;
    out.normal = model_normal;
    let kind = u32(rig.y + 0.5);
    if (kind == 0u) {
        return out;
    }
    // Into the aligned frame: forward +z, right +x, authoring units.
    let yaw = rig.w;
    let cy = cos(yaw);
    let sy = sin(yaw);
    let k = rig.z;
    var p = vec3<f32>(
        (model_pos.x * cy - model_pos.z * sy) * k,
        model_pos.y * k,
        (model_pos.x * sy + model_pos.z * cy) * k,
    );
    var n = vec3<f32>(model_normal.x * cy - model_normal.z * sy, model_normal.y, model_normal.x * sy + model_normal.z * cy);
    var posed = Posed(p, n);
    let action = u32(rig.x + 0.5);
    let walk = anim.y;
    let step = anim.x;
    let s = sin(step);
    let aim = anim.z;
    let strike = anim.w;
    let idle = (1.0 - min(walk, 1.0)) * select(1.0, 0.0, action != 0u);
    let phase = seed * 6.283 + time;
    let lateral = select(-1.0, 1.0, p.x >= 0.0);

    if (kind == RIG_BIPED) {
        let hip = 0.47;
        let shoulder = vec2<f32>(0.745, 0.0);
        // Legs: swing under the hips, opposite sides in opposite directions.
        if (p.y < hip && walk > 0.0) {
            let w = smooth01(hip, hip - 0.12, p.y) * smooth01(0.0, 0.03, abs(p.x));
            swing(&posed, vec2<f32>(hip, 0.0), lateral * s * 0.7 * walk, w);
        }
        // Arms: the sides of the chest and everything the hands hold.
        let reach = abs(p.x);
        var arm = smooth01(0.10, 0.14, reach) * select(smooth01(0.17, 0.2, reach), 1.0, p.y < 0.82) * smooth01(0.30, 0.45, p.y);
        var ang = -lateral * s * 0.55 * walk;
        if (action >= W_NONE && action <= 18u) {
            ang = arm_attack(action, lateral, aim, strike);
            if (action == W_CROSSBOW || action == W_FIREARM || action == W_AUTOGUN || action == 10u) {
                // Two-handed: the chest in front of the arms stays, both arms follow the weapon.
                ang = arm_attack(action, 1.0, aim, strike);
            }
        } else if (action == ACTION_HIT) {
            ang = 1.1 * aim;
        }
        if (arm > 0.0) {
            let pivot = vec2<f32>(shoulder.x, 0.0);
            swing(&posed, pivot, ang, arm);
        }
        // Torso: breathing at rest, a lean with the step, a lean back when hit.
        let upper = smooth01(hip, hip + 0.1, p.y);
        posed.pos.y = posed.pos.y * (1.0 + upper * (0.012 * sin(phase * 2.2) * idle));
        posed.pos.z = posed.pos.z + upper * 0.05 * walk * s * s * 0.4;
        if (action == ACTION_HIT) {
            posed.pos.z = posed.pos.z - upper * 0.22 * aim;
        }
        posed.pos.y = posed.pos.y + abs(s) * 0.025 * walk;
    } else if (kind == RIG_HORSE || kind == RIG_ELEPHANT || kind == RIG_CAMEL || kind == RIG_CHARIOT) {
        var leg_top = 0.46;
        var hip_y = 0.56;
        if (kind == RIG_ELEPHANT) {
            leg_top = 0.42;
            hip_y = 0.6;
        } else if (kind == RIG_CAMEL) {
            leg_top = 0.64;
            hip_y = 0.74;
        }
        var swing_w = smooth01(leg_top, leg_top - 0.12, p.y);
        if (kind == RIG_CHARIOT) {
            swing_w = swing_w * smooth01(-0.15, 0.0, p.z);
        }
        if (walk > 0.0 && swing_w > 0.0) {
            let front = select(-1.0, 1.0, p.z > -0.05);
            var amp = 0.62;
            if (kind == RIG_ELEPHANT) {
                amp = 0.35;
            }
            swing(&posed, vec2<f32>(leg_top, 0.0), lateral * front * s * amp * walk, swing_w);
        }
        // The whole animal rocks and bobs; a strike rears it, a blow pushes it back.
        var rear = 0.0;
        if (action >= W_NONE && action <= 18u) {
            rear = -0.32 * sin(clamp(aim * 0.4 + strike * 1.6, 0.0, 1.0) * 3.14159) * select(1.0, 0.0, action == 5u || action == 8u);
        }
        let pitch = rear + 0.04 * walk * s + 0.012 * sin(phase * 1.5) * idle;
        posed.pos = rot_x(posed.pos, vec2<f32>(0.0, -0.3), pitch);
        posed.normal = rot_x_dir(posed.normal, pitch);
        posed.pos.y = posed.pos.y + abs(s) * 0.03 * walk;
        if (action == ACTION_HIT) {
            posed.pos.z = posed.pos.z - 0.12 * aim;
        }
    } else if (kind == RIG_VESSEL) {
        let moving = walk;
        let t = time * 1.3 + seed * 6.283;
        let pitch = 0.025 * sin(t * 0.9) + 0.03 * moving * sin(step * 0.7);
        let roll = 0.035 * sin(t * 1.1 + 1.0) + 0.02 * moving * sin(step * 0.5 + 0.5);
        posed.pos = rot_x(posed.pos, vec2<f32>(0.0, 0.0), pitch * 3.0);
        posed.normal = rot_x_dir(posed.normal, pitch * 3.0);
        posed.pos = rot_z(posed.pos, vec2<f32>(0.0, 0.0), roll * 3.0);
        posed.normal = rot_z_dir(posed.normal, roll * 3.0);
        posed.pos.y = posed.pos.y + 0.012 * sin(t * 1.7);
        if (action >= W_NONE && action <= 18u) {
            // A broadside heels the ship away from the shot.
            posed.pos = rot_z(posed.pos, vec2<f32>(0.0, 0.0), -0.05 * strike * exp(-strike * 4.0) * 4.0);
        }
    } else if (kind == RIG_ENGINE || kind == RIG_VEHICLE || kind == RIG_CART) {
        // Rolling: a rocking on the springs; firing: the front kicks up as the piece recoils.
        let kick = strike * exp(-strike * 6.0) * 2.7;
        var pitch = 0.025 * walk * s + 0.07 * kick * select(0.0, 1.0, action >= W_NONE && action <= 18u);
        if (kind == RIG_VEHICLE) {
            pitch = pitch * 0.5;
        }
        posed.pos = rot_x(posed.pos, vec2<f32>(0.0, -0.2), -pitch);
        posed.normal = rot_x_dir(posed.normal, -pitch);
        posed.pos.y = posed.pos.y + abs(s) * 0.012 * walk;
        if (action == ACTION_HIT) {
            posed.pos.z = posed.pos.z - 0.1 * aim;
        }
    } else if (kind == RIG_PLANE) {
        posed.pos.y = posed.pos.y + 0.025 * sin(time * 1.4 + seed * 6.283) + 0.0;
        let roll = 0.06 * sin(time * 0.8 + seed * 4.0) + 0.1 * walk * sin(step * 0.4);
        posed.pos = rot_z(posed.pos, vec2<f32>(0.0, 0.4), roll);
        posed.normal = rot_z_dir(posed.normal, roll);
    }
    // Back to the model's own frame in hex units.
    let inv = 1.0 / k;
    let q = posed.pos * inv;
    out.pos = vec3<f32>(q.x * cy + q.z * sy, q.y, -q.x * sy + q.z * cy);
    let m = posed.normal;
    out.normal = normalize(vec3<f32>(m.x * cy + m.z * sy, m.y, -m.x * sy + m.z * cy));
    return out;
}
