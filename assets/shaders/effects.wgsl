// Particles, projectiles and flashes (crates/render/src/gpu/effects.rs).
//
// One instance per particle, written once when it is spawned. Everything that moves is a
// closed-form function of the effects clock `fx.time.x`, so a frame costs one small
// uniform write and one instanced draw (6 vertices per particle, no vertex buffer for the
// quad). A particle outside its lifetime collapses to a clipped point.
//
// Kinds (Particle::kind): 0 puff, 1 glow, 2 streak, 3 ground ring, 4 arrow, 5 shell,
// 6 debris, 7 scorch, 8 flame. Premultiplied blending: alpha 0 with colour is additive.

struct Camera {
    view_proj: mat4x4<f32>,
    sun: vec4<f32>,
};

struct Fx {
    // Screen right / up in world space (billboards), xyz.
    right: vec4<f32>,
    up: vec4<f32>,
    // x: effects clock in seconds.
    time: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var<uniform> fx: Fx;

struct PIn {
    @location(0) pos_born: vec4<f32>,
    @location(1) vel_life: vec4<f32>,
    @location(2) accel_kind: vec4<f32>,
    @location(3) size_drag_floor: vec4<f32>,
    @location(4) color0: vec4<f32>,
    @location(5) color1: vec4<f32>,
    @location(6) seed_spin: vec2<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) wpos: vec3<f32>,
    @location(3) @interpolate(flat) kind: u32,
    @location(4) u: f32,
    @location(5) seed: f32,
};

fn corner(vi: u32) -> vec2<f32> {
    var c = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    return c[vi];
}

// Same integral as Particle::position_at on the CPU.
fn travel(vel: vec3<f32>, accel: vec3<f32>, drag: f32, t: f32) -> vec3<f32> {
    var s = t;
    if (drag > 0.001) {
        s = (1.0 - exp(-drag * t)) / drag;
    }
    return vel * s + 0.5 * accel * t * t;
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, p: PIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    let life = p.vel_life.w;
    let t = fx.time.x - p.pos_born.w;
    if (t < 0.0 || t > life || life <= 0.0) {
        return o;
    }
    let u = t / life;
    let kind = u32(p.accel_kind.w + 0.5);
    let c = corner(vi);
    var centre = p.pos_born.xyz + travel(p.vel_life.xyz, p.accel_kind.xyz, p.size_drag_floor.z, t);
    centre.y = max(centre.y, p.size_drag_floor.w);
    var size = mix(p.size_drag_floor.x, p.size_drag_floor.y, u);
    var w = centre;
    let right = fx.right.xyz;
    let up = fx.up.xyz;
    if (kind == 3u || kind == 7u) {
        // Flat on the ground.
        w = centre + vec3<f32>(c.x * size, 0.05, c.y * size);
    } else if (kind == 2u || kind == 4u) {
        // Along the velocity as the screen sees it: size0 is the half width, size1 the length.
        let v = p.vel_life.xyz + p.accel_kind.xyz * t;
        var d = vec2<f32>(dot(v, right), dot(v, up));
        let l = length(d);
        if (l < 1e-4) {
            d = vec2<f32>(0.0, 1.0);
        } else {
            d = d / l;
        }
        let axis = right * d.x + up * d.y;
        let side = right * (-d.y) + up * d.x;
        let length_world = p.size_drag_floor.y;
        // The head at the position, the tail behind it.
        w = centre + axis * ((c.x * 0.5 - 0.5) * length_world) + side * (c.y * p.size_drag_floor.x);
    } else if (kind == 8u) {
        // A flame: taller than wide, flickering, rooted at its base.
        let flick = 1.0 + 0.18 * sin(t * 19.0 + p.seed_spin.x * 40.0);
        w = centre + right * (c.x * size * 0.8 * flick) + up * ((c.y * 0.5 + 0.5) * size * 1.5 * flick);
    } else {
        let a = p.seed_spin.y * t;
        let ca = cos(a);
        let sa = sin(a);
        let r = vec2<f32>(c.x * ca - c.y * sa, c.x * sa + c.y * ca);
        w = centre + right * (r.x * size) + up * (r.y * size);
    }
    o.clip = cam.view_proj * vec4<f32>(w, 1.0);
    o.uv = c;
    o.wpos = w;
    o.kind = kind;
    o.u = u;
    o.seed = p.seed_spin.x;
    // A short fade-in so nothing pops; the colours blend over the lifetime.
    let fade_in = clamp(t / 0.04, 0.0, 1.0);
    var col = mix(p.color0, p.color1, u);
    col.a = col.a * fade_in;
    o.color = col;
    return o;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let r2 = dot(in.uv, in.uv);
    var shape = 0.0;
    var additive = false;
    switch (in.kind) {
        case 0u: {
            // Smoke / dust: a soft blob with a ragged edge.
            let a = atan2(in.uv.y, in.uv.x);
            let r = sqrt(r2) * (1.0 + 0.16 * sin(a * 3.0 + in.seed * 30.0) + 0.1 * sin(a * 5.0 + in.seed * 17.0));
            shape = clamp(1.0 - r, 0.0, 1.0);
            shape = shape * shape * (3.0 - 2.0 * shape);
        }
        case 1u: {
            additive = true;
            shape = pow(clamp(1.0 - sqrt(r2), 0.0, 1.0), 1.6);
        }
        case 2u: {
            additive = true;
            let along = in.uv.x * 0.5 + 0.5; // 0 tail, 1 head
            shape = clamp(1.0 - abs(in.uv.y), 0.0, 1.0) * pow(along, 1.5);
        }
        case 3u: {
            let d = abs(sqrt(r2) - 0.8);
            shape = clamp(1.0 - d / 0.2, 0.0, 1.0);
            shape = shape * shape;
            if (sqrt(r2) > 1.0) {
                shape = 0.0;
            }
        }
        case 4u: {
            // Shaft, head at the front, fletching at the back.
            let along = in.uv.x * 0.5 + 0.5;
            let shaft = step(abs(in.uv.y), 0.22);
            let head = step(abs(in.uv.y), (1.0 - along) * 3.0 * step(0.82, along));
            let tail = step(abs(in.uv.y), 0.8) * step(along, 0.16) * step(0.04, along);
            shape = max(max(shaft, head), tail);
        }
        case 5u: {
            // A solid ball with a highlight.
            shape = step(r2, 1.0);
        }
        case 6u: {
            shape = step(r2, 1.0);
        }
        case 7u: {
            shape = clamp(1.0 - sqrt(r2), 0.0, 1.0);
            shape = shape * shape * (3.0 - 2.0 * shape);
        }
        default: {
            // Flame: a tongue that is wide at the base and narrows to its tip.
            additive = true;
            let y = in.uv.y * 0.5 + 0.5;
            let d = vec2<f32>(in.uv.x / max(0.8 * (1.0 - 0.55 * y), 0.1), (y - 0.42) / 0.58);
            shape = pow(clamp(1.0 - length(d), 0.0, 1.0), 1.1);
        }
    }
    // Nothing shows through the fog of war.
    let seen = clamp((map_fog(in.wpos.xz) - 0.55) / 0.4, 0.0, 1.0);
    var a = shape * in.color.a * seen;
    if (a < 0.004) {
        discard;
    }
    var rgb = pow(in.color.rgb, vec3<f32>(2.2));
    if (in.kind == 5u) {
        let h = clamp(0.5 - 0.5 * (in.uv.x * 0.5 - in.uv.y * 0.8), 0.0, 1.0);
        rgb = rgb * (0.55 + 0.9 * h);
    }
    if (in.kind == 8u) {
        // Hot core to red tip over the flame's height.
        let y = in.uv.y * 0.5 + 0.5;
        rgb = mix(rgb * 1.6, rgb * vec3<f32>(1.0, 0.45, 0.2), y);
    }
    if (additive) {
        return vec4<f32>(rgb * a, 0.0);
    }
    return vec4<f32>(rgb * a, a);
}
