// Entity models (Blender bake): real lighting instead of fullbright.
// Vertex: pos + flat normal + baked colour + team mask.
// Instance: offset + owner colour + scale + fog of war.
// The light is the same as the terrain/features (warm sun / cool sky / ACES), otherwise
// the figures would look like they came from another world (v36).

struct Camera {
    view_proj: mat4x4<f32>,
    sun: vec4<f32>,
    // x = the shader clock: the animation clock (seconds), frozen at 0 on Quality::Low and with
    // reduced motion, and frozen while the game is paused.
    time: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;

struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) team: f32,
};

struct InstanceIn {
    @location(4) offset: vec3<f32>,
    @location(5) color: vec3<f32>,
    @location(6) scale_height: vec2<f32>,
    @location(7) fog: f32,
    // Yaw (radians, +x toward +z) and the ground-plane shear of "plane" world models.
    @location(8) yaw: f32,
    @location(9) slope: vec2<f32>,
    // Animation state and the model's rig constants (unit_anim.wgsl).
    @location(10) anim: vec4<f32>,
    @location(11) rig: vec4<f32>,
    // Motion (gpu/world_anim.rs): x = kind | flags << 4, y = phase, z = amplitude, w = speed.
    @location(12) motion: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) fog: f32,
    @location(3) wpos: vec3<f32>,
    // Light of its own added after the lighting (fire, glints, radiation, embers).
    @location(4) glow: vec3<f32>,
};

const TAU: f32 = 6.2831853;
// Palette colours the shader animates wherever they appear (manifest palette).
const FIRE: vec3<f32> = vec3<f32>(0.95, 0.55, 0.15);
const LAVA: vec3<f32> = vec3<f32>(0.96, 0.36, 0.11);
const URANIUM: vec3<f32> = vec3<f32>(0.49, 0.81, 0.31);
const GEM_BLUE: vec3<f32> = vec3<f32>(0.21, 0.43, 0.87);
const GEM_RED: vec3<f32> = vec3<f32>(0.81, 0.15, 0.27);
const GOLD: vec3<f32> = vec3<f32>(0.90, 0.72, 0.30);
const WATER: vec3<f32> = vec3<f32>(0.25, 0.51, 0.63);
const TURQUOISE: vec3<f32> = vec3<f32>(0.31, 0.76, 0.73);
const FOAM: vec3<f32> = vec3<f32>(0.87, 0.93, 0.95);
const STEAM: vec3<f32> = vec3<f32>(0.93, 0.95, 0.97);
const NEON: vec3<f32> = vec3<f32>(0.40, 0.95, 0.85);

fn is_pal(c: vec3<f32>, k: vec3<f32>) -> bool {
    let d = abs(c - k);
    return max(d.x, max(d.y, d.z)) < 0.006;
}
fn hash1(x: f32) -> f32 { return fract(sin(x * 127.1 + 311.7) * 43758.5453); }

@vertex
fn vs_main(v: VertexIn, i: InstanceIn) -> VsOut {
    var o: VsOut;
    let t = cam.time.x;
    let packed = u32(i.motion.x + 0.5);
    let kind = packed & 15u;
    let pillaged = (packed & 16u) != 0u;
    let living = (packed & 32u) != 0u;
    let phase = i.motion.y;
    // The animal code of a part of a herd model (what it is, which animal) rides in the team channel (world_anim.rs).
    let level = floor(v.team * 0.5 + 0.001);
    let team = v.team - 2.0 * level;
    var local = v.pos;
    var local_normal = v.normal;
    var wind = vec2<f32>(0.0);
    var glow = vec3<f32>(0.0);
    var tint = vec3<f32>(1.0);
    let spot = hash1(dot(v.pos, vec3<f32>(37.1, 91.7, 53.3)) + phase);

    // Per-class motion (data: world_anim.rs).
    if (kind == 1u || kind == 5u) {
        // Sway: a travelling gust (the wave follows the world position, so a field ripples),
        // growing with the square of the height (to 0.6). Kind 5 moves only the green parts
        // (the trees of a mountain, a garden), never the rock or the gold.
        var leafy = 1.0;
        if (kind == 5u) { leafy = smoothstep(0.0, 0.08, v.color.g - max(v.color.r, v.color.b)); }
        let h = min(local.y, 0.6);
        let reach = i.motion.z * h * h * leafy;
        let gust = sin(t * i.motion.w + phase + dot(i.offset.xz, vec2<f32>(0.9, 0.6)) * 1.7)
            + 0.45 * sin(t * i.motion.w * 2.3 + phase * 1.7);
        wind = vec2<f32>(0.8, 0.6) * (reach * gust);
    } else if (kind == 2u && level > 0.5) {
        // Graze: each animal lives on its own beat (what the code holds is set out at `herd_codes`,
        // gpu/world_anim.rs). One cycle (9-15 s): the head lowers slowly (0..0.11), grazes with a chewing
        // tremor, lifts quickly (0.05 long) and looks about; then a few steps along a short arc, the legs
        // stepping in diagonal pairs, and the next cycle begins. Everything is a function of the clock, the
        // tile's phase and the animal's own numbers, so a frozen clock draws the same frame every time.
        let c = u32(level + 0.5) - 1u;
        let cls = c & 3u;
        let wq = f32((c >> 2u) & 7u) / 7.0;
        let down = f32((c >> 5u) & 1u);
        let angle = f32((c >> 6u) & 15u) * (TAU / 16.0);
        let hdir = vec2<f32>(cos(angle), sin(angle));
        let side = vec2<f32>(-hdir.y, hdir.x);
        let pivot = vec2<f32>(f32((c >> 10u) & 63u), f32((c >> 16u) & 63u)) * (1.28 / 63.0) - vec2<f32>(0.64);
        let id = fract(f32(((c >> 6u) * 2654435761u) >> 20u) / 4096.0 + phase / TAU);
        let id2 = fract(f32(((c >> 6u) * 40503u + 12345u) >> 18u) / 16384.0 + phase * 0.37);
        let cyc = t * i.motion.w * (0.75 + 0.5 * id) / TAU + id2;
        let k = floor(cyc);
        let u = cyc - k;
        let lifted = 0.58 + 0.10 * id;
        // 1 = head at the grass. A head baked at the grass (down = 1) is lifted by the same curve.
        let g = smoothstep(0.0, 0.11, u) * (1.0 - smoothstep(lifted, lifted + 0.05, u));
        let w0 = lifted + 0.08;
        let walk = smoothstep(w0, min(w0 + 0.22, 0.97), u);
        let env = 4.0 * walk * (1.0 - walk);
        let stride = walk * TAU * 2.5;
        var off = vec3<f32>(0.0);
        if (cls == 1u) {
            // Head, neck, horns: lowered and pushed forward with the weight growing from the shoulder (the
            // neck bends instead of the head sliding off it); a chewing tremor while down, a slow look
            // to the side while up.
            let wp = pow(wq, 1.4);
            let dip = (g - down) * i.motion.z;
            let chew = 0.06 * sin(t * 6.0 + id * 40.0) * g * i.motion.z;
            let look = 0.3 * (1.0 - g) * sin(t * 0.8 + id * 17.0) * i.motion.z;
            off = vec3<f32>(hdir.x * 0.3 * dip + side.x * look, -(dip + chew), hdir.y * 0.3 * dip + side.y * look) * wp;
        } else if (cls >= 2u) {
            // Legs: the foot swings along the heading and lifts while the animal walks; the hip stays.
            let a = stride + select(0.0, 3.14159, cls == 3u);
            let swing = 0.025 * env * sin(a) * wq;
            off = vec3<f32>(hdir.x * swing, 0.010 * env * max(cos(a), 0.0) * wq, hdir.y * swing);
        } else {
            // Body: it breathes (the back rises, the belly stays) and rocks a little as it walks.
            let sway = 0.005 * env * sin(stride) * wq;
            off = vec3<f32>(side.x * sway, 0.008 * sin(t * 1.9 + id * 30.0) * wq, side.y * sway);
        }
        local += off;
        // The wander: the whole animal turns about a point beside it while it walks (so it goes forward
        // along an arc of radius 0.05), half a radian further every cycle, in its own direction.
        let sgn = select(-1.0, 1.0, id > 0.5);
        let theta = (k + walk) * 0.5;
        let ca = cos(sgn * theta);
        let sa = sin(sgn * theta);
        let rel = vec2<f32>(local.x, local.z) - pivot;
        let moved = vec2<f32>(ca * rel.x - sa * rel.y, sa * rel.x + ca * rel.y) + pivot
            + 0.05 * (sin(theta) * hdir + sgn * (1.0 - cos(theta)) * side);
        local.x = moved.x;
        local.z = moved.y;
        local_normal = vec3<f32>(ca * local_normal.x - sa * local_normal.z, local_normal.y, sa * local_normal.x + ca * local_normal.z);
    } else if (kind == 3u) {
        // Afloat: heave and roll.
        let a = t * i.motion.w + phase;
        local.y += i.motion.z * sin(a) + local.x * 0.05 * sin(a * 0.8 + 1.3) + local.z * 0.04 * sin(a * 0.6);
    } else if (kind == 4u && (is_pal(v.color, STEAM) || (local.y > i.motion.z + 0.3 && min(v.color.r, min(v.color.g, v.color.b)) > 0.85))) {
        // Geyser: the steam column rises, holds, collapses and rests (about 12 s); a frozen clock
        // shows it standing.
        let u = fract(t * i.motion.w / TAU + phase / TAU);
        var f = 0.78;
        if (t != 0.0) {
            // The column itself is particles (ambient_fx.rs); the model keeps a short jet at the vent.
            f = 0.06 + 0.20 * smoothstep(0.05, 0.30, u) * (1.0 - smoothstep(0.55, 0.85, u));
            f *= 0.92 + 0.08 * sin(t * 5.0 + phase);
        }
        let vent = i.motion.z;
        local.y = vent + (local.y - vent) * f;
        let widen = 0.35 + 0.65 * f;
        local.x *= widen;
        local.z *= widen;
        local.x += 0.025 * f * sin(t * 4.0 + local.y * 8.0 + phase) * (local.y - vent);
    }

    if (kind == 6u && t != 0.0 && local.y > i.motion.z + 0.04) {
        // A crater's own smoke gives way to the particles: it shrinks into the crater.
        local = vec3<f32>(local.x * 0.15, i.motion.z + (local.y - i.motion.z) * 0.05, local.z * 0.15);
    }

    // Material motion: palette colours that glow or glitter, on every world object.
    if (!living) {
    } else if (is_pal(v.color, FIRE) || is_pal(v.color, LAVA)) {
        let flick = 0.5 + 0.5 * sin(t * 7.0 + spot * 40.0 + phase) * sin(t * 3.3 + spot * 17.0);
        glow += v.color * v.color * (0.12 + 0.32 * flick);
    } else if (is_pal(v.color, URANIUM)) {
        glow += vec3<f32>(0.05, 0.34, 0.03) * (0.5 + 0.5 * sin(t * 1.6 + phase));
    } else if (is_pal(v.color, GEM_BLUE) || is_pal(v.color, GEM_RED) || is_pal(v.color, GOLD) || is_pal(v.color, NEON)) {
        // Glints: a sharp peak of a fast sine, at a different time on every facet.
        let g = pow(max(sin(t * 2.1 + spot * 60.0 + phase * 3.0), 0.0), 18.0);
        glow += vec3<f32>(0.75, 0.70, 0.55) * g * 0.55;
    } else if (is_pal(v.color, WATER) || is_pal(v.color, TURQUOISE) || is_pal(v.color, FOAM)) {
        let ripple = sin(t * 2.4 + (v.pos.x + v.pos.z * 0.7) * 14.0 + spot * 6.0 + phase);
        tint *= 1.0 + 0.10 * ripple;
        glow += vec3<f32>(0.06, 0.10, 0.12) * pow(max(ripple, 0.0), 8.0);
    }

    // A pillaged work: charred, sagging, broken at the top, embers still glowing.
    if (pillaged) {
        let lean = vec2<f32>(cos(phase), sin(phase));
        let broken = select(1.0, 0.55, spot > 0.82 && local.y > 0.08);
        local.y = (local.y - 0.10 * local.y * local.y) * broken;
        wind += lean * (0.16 * local.y);
        let scorch = mix(0.50, 0.85, step(0.55, spot));
        tint *= vec3<f32>(scorch * 0.95, scorch * 0.78, scorch * 0.66);
        let ember = step(0.90, spot) * (0.5 + 0.5 * sin(t * 4.0 + spot * 90.0));
        glow += vec3<f32>(0.80, 0.26, 0.05) * ember * 0.9;
    }

    let c = cos(i.yaw);
    let s = sin(i.yaw);
    // Units: the rigged pose (unit_anim.wgsl) on top of the world motion.
    if (i.rig.y > 0.5) {
        let posed = animate_unit(local, v.normal, i.anim, i.rig, cam.time.x, unit_hash(i.offset.x * 0.37 + i.offset.z * 0.91));
        local = posed.pos;
        local_normal = posed.normal;
    }
    let dx = (local.x * c - local.z * s) * i.scale_height.x;
    let dz = (local.x * s + local.z * c) * i.scale_height.x;
    let dy = local.y * i.scale_height.y + i.slope.x * dx + i.slope.y * dz;
    let world = i.offset + vec3<f32>(dx + wind.x, dy, dz + wind.y);
    o.clip = cam.view_proj * vec4<f32>(world, 1.0);
    o.glow = glow;
    // Team mask: cloth/shields/roofs take the owner's colour, the rest is baked.
    o.color = mix(v.color, i.color, clamp(team, 0.0, 1.0)) * tint;
    // Normal: rotate, inverse scale, then the inverse transpose of the ground shear.
    let rn = vec3<f32>(local_normal.x * c - local_normal.z * s, local_normal.y, local_normal.x * s + local_normal.z * c);
    let sn = vec3<f32>(
        rn.x / max(i.scale_height.x, 1e-4),
        rn.y / max(i.scale_height.y, 1e-4),
        rn.z / max(i.scale_height.x, 1e-4),
    );
    o.normal = normalize(vec3<f32>(sn.x - i.slope.x * sn.y, sn.y, sn.z - i.slope.y * sn.y));
    o.fog = i.fog;
    o.wpos = world;
    return o;
}

// A fully visible pixel is its lit colour alone (see `feature.wgsl`).
@diagnostic(off, derivative_uniformity)
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Decode authored sRGB colors; the sRGB render target performs final encoding.
    var lin = pow(in.color, vec3<f32>(2.2));
    let n = normalize(in.normal);
    let diff = max(dot(n, cam.sun.xyz), 0.0) * surface_shadow(in.wpos, n);
    let sun_col = vec3<f32>(1.0, 0.94, 0.84) * 1.0;
    let sky_col = vec3<f32>(0.42, 0.52, 0.72) * (cam.sun.w * 1.5);
    let sky_amb = sky_col * (0.55 + 0.45 * n.y);
    let bounce = vec3<f32>(0.30, 0.26, 0.22) * 0.18 * clamp(0.5 - 0.5 * n.y, 0.0, 1.0);
    var lit = lin * (sun_col * diff + sky_amb + bounce);
    var e = lit * 0.85;
    var tm = e * (2.51 * e + vec3<f32>(0.03))
        / (e * (2.43 * e + vec3<f32>(0.59)) + vec3<f32>(0.14));
    // Instance fog is an upper bound from knowledge (world objects are built from it);
    // current visibility comes from the GPU fog field, as for features.
    let fog = min(in.fog, map_fog(in.wpos.xz));
    if (fog >= VISIBLE_FOG_MIN) {
        return vec4<f32>(tm + in.glow, 1.0);
    }
    let luminance = dot(tm, vec3<f32>(0.2126, 0.7152, 0.0722));
    let pigment = map_pigment(in.color, MEMORY_OBJECT_CHROMA, 1.0) * clamp(0.62 + luminance * 1.1, 0.78, 1.15);
    let paper = map_paper(in.wpos.xz);
    let remembered = mix(paper, map_ink_over(in.wpos.xz, n, pigment, paper), map_known(fog));
    return vec4<f32>(map_finish(in.wpos.xz, fog, remembered, tm), 1.0);
}
