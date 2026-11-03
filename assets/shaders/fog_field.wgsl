// Bakes the observer's knowledge into a world-space fog field.
// Exact port of scene::fog::fog_at_position: tile states are 0 unknown, 1 explored,
// 2 visible; the result is 0.18 / 0.45 / 1.0 with the irregular inward wash.
// Runs only when knowledge changes; every receiver samples the baked field.

struct Bake {
    // World XZ of field texel (0, 0) corner, world units per texel, unused.
    origin_texel: vec4<f32>,
    // Map size in tiles.
    map: vec4<u32>,
};

@group(0) @binding(0) var tile_states: texture_2d<u32>;
@group(0) @binding(1) var<uniform> bake: Bake;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(corners[i], 0.0, 1.0);
}

fn hash_u32(seed: u32) -> u32 {
    var x = seed;
    x ^= x >> 16u;
    x *= 0x7feb352du;
    x ^= x >> 15u;
    x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}

fn lattice_hash(a: i32, b: i32) -> f32 {
    let seed = (bitcast<u32>(a) * 0x9e3779b9u) ^ (bitcast<u32>(b) * 0x85ebca6bu);
    return f32(hash_u32(seed)) / 4294967296.0;
}

fn smooth01(t: f32) -> f32 {
    return t * t * (3.0 - 2.0 * t);
}

fn wash_noise(x: f32, z: f32) -> f32 {
    let ix = i32(floor(x));
    let iz = i32(floor(z));
    let tx = smooth01(x - floor(x));
    let tz = smooth01(z - floor(z));
    let near = lattice_hash(ix, iz) * (1.0 - tx) + lattice_hash(ix + 1, iz) * tx;
    let far = lattice_hash(ix, iz + 1) * (1.0 - tx) + lattice_hash(ix + 1, iz + 1) * tx;
    return near * (1.0 - tz) + far * tz;
}

fn tile_fog(q: i32, r: i32) -> f32 {
    if (q < 0 || r < 0 || q >= i32(bake.map.x) || r >= i32(bake.map.y)) {
        return 0.18;
    }
    let state = textureLoad(tile_states, vec2<i32>(q, r), 0).r;
    if (state == 2u) { return 1.0; }
    if (state == 1u) { return 0.45; }
    return 0.18;
}

// Rust f32::round: half away from zero.
fn round_away(v: f32) -> i32 {
    return i32(sign(v) * floor(abs(v) + 0.5));
}

fn fog_at_position(x: f32, z: f32) -> f32 {
    let rf = z / 1.5;
    let qf = x / 1.7320508 - rf * 0.5;
    let q0 = round_away(qf);
    let r0 = round_away(rf);
    var fog = 1.0;
    let wash = wash_noise(x * 0.39 + 13.7, z * 0.39 - 8.2) * 0.73
        + wash_noise(x * 1.13 - 3.9, z * 1.13 + 25.1) * 0.27;
    let radius = 1.01 + wash * 0.12;
    let width = 0.30 + wash_noise(x * 0.18 + 7.0, z * 0.18 + 41.0) * 0.12;
    for (var r = r0 - 2; r <= r0 + 2; r += 1) {
        for (var q = q0 - 2; q <= q0 + 2; q += 1) {
            let known = tile_fog(q, r);
            if (known >= 1.0) {
                continue;
            }
            let cx = 1.7320508 * (f32(q) + f32(r) * 0.5);
            let cz = 1.5 * f32(r);
            let distance = sqrt((x - cx) * (x - cx) + (z - cz) * (z - cz));
            let t = clamp((distance - radius) / width, 0.0, 1.0);
            let blend = t * t * (3.0 - 2.0 * t);
            fog = min(fog, known + (1.0 - known) * blend);
        }
    }
    return fog;
}

@fragment
fn fs_main(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let world = bake.origin_texel.xy + pixel.xy * bake.origin_texel.z;
    return vec4<f32>(fog_at_position(world.x, world.y), 0.0, 0.0, 1.0);
}
