// Shared cartographic fog. All colors are linear for the sRGB render target.
// Unknown-land ornament depends only on world XZ, never on hidden terrain data.
// Observer knowledge baked into a world-space field (gpu/fog_field.rs): 1.0 visible,
// 0.45 remembered, 0.18 unknown, with the inward wash of scene::fog::fog_at_position.
// Geometry is built from knowledge only; current visibility always comes from here.
@group(0) @binding(1) var fog_field: texture_2d<f32>;
@group(0) @binding(2) var fog_sampler: sampler;
// Field origin XZ and inverse extent XZ.
@group(0) @binding(3) var<uniform> fog_bounds: vec4<f32>;
// The parchment: baked into textures (gpu/parchment.rs) or computed per pixel. The host sets this
// constant (AURELIA_PARCHMENT=procedural turns it off, also where a texture cannot be made).
const PARCHMENT_BAKED: bool = true;
// Low-frequency parchment field over the map: wash(p), wash(1.17 p), broad(p), meadow(p).
@group(0) @binding(4) var parchment_low_field: texture_2d<f32>;
// Seamless world-space tiles of periodic value noise (the sampler repeats):
// fine (R8): the grain and fibre residual of the paper colour, period FINE_PERIOD world units;
// mid (RG8): the pigment flecks and the feather of the fog edge, period MID_PERIOD.
@group(0) @binding(5) var parchment_fine: texture_2d<f32>;
@group(0) @binding(6) var parchment_mid: texture_2d<f32>;
@group(0) @binding(7) var parchment_tile_sampler: sampler;
// Origin XZ and inverse extent XZ of the low field.
@group(0) @binding(8) var<uniform> parchment_bounds: vec4<f32>;
const FINE_PERIOD: f32 = 8.0;
const FINE_TEXELS: f32 = 512.0;
// The residual is stored as 0.5 + residual / FINE_RANGE (its extent is 0.0135, see map_fine_procedural).
const FINE_RANGE: f32 = 0.03;
const MID_PERIOD: f32 = 10.0;
const MID_TEXELS: f32 = 512.0;
// Fog regimes of `map_finish`: from VISIBLE_FOG_MIN up the reveal is 1 and the edge band 0
// (the pixel is its live colour); up to REMEMBERED_FOG_MAX the reveal is 0 (the pixel is
// its remembered colour, with the deposits of the band).
const VISIBLE_FOG_MIN: f32 = 0.9999;
const REMEMBERED_FOG_MAX: f32 = 0.52;
// Below BARE_FOG_MAX `map_known` is 0 and the pixel is the bare parchment (nothing remembered, no
// edge); up to EDGE_FOG_MIN the edge band of `map_finish` is 0 and the pixel is its remembered colour.
const BARE_FOG_MAX: f32 = 0.20;
const EDGE_FOG_MIN: f32 = 0.48;
// True when `cond` holds for any pixel of this 2x2 quad: the same answer for the four pixels, so a
// branch on it keeps fwidth/dpdx valid inside, which a branch on a per-pixel value would not. The
// compiler's uniformity analysis cannot see that: the caller turns derivative_uniformity off.
// Must be called from control flow that all four pixels of the quad reach.
fn quad_any(cond: bool) -> bool {
    let c = select(0.0, 1.0, cond);
    // Own value, row neighbour, column neighbour; then the row neighbour's view adds the diagonal.
    let near = max(c, max(abs(dpdxFine(c)), abs(dpdyFine(c))));
    return max(near, abs(dpdxFine(near))) > 0.5;
}
fn map_fog(p: vec2<f32>) -> f32 {
    let uv = (p - fog_bounds.xy) * fog_bounds.zw;
    return textureSampleLevel(fog_field, fog_sampler, uv, 0.0).r;
}
fn map_hash(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}
fn map_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(map_hash(i), map_hash(i + vec2<f32>(1.0, 0.0)), u.x),
               mix(map_hash(i + vec2<f32>(0.0, 1.0)), map_hash(i + vec2<f32>(1.0)), u.x), u.y);
}
// Value noise on a lattice that wraps after `period` cells in X and Z: periodic, so it can be baked
// into a seamless tile. `seed` picks one of several independent patterns.
fn map_tile_hash(i: vec2<f32>, period: vec2<f32>, seed: f32) -> f32 {
    let wrapped = i - period * floor(i / period);
    return map_hash(wrapped + vec2<f32>(seed * 61.0, seed * 29.0));
}
fn map_tile_noise(q: vec2<f32>, period: vec2<f32>, seed: f32) -> f32 {
    let i = floor(q);
    let f = fract(q);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(map_tile_hash(i, period, seed), map_tile_hash(i + vec2<f32>(1.0, 0.0), period, seed), u.x),
               mix(map_tile_hash(i + vec2<f32>(0.0, 1.0), period, seed), map_tile_hash(i + vec2<f32>(1.0), period, seed), u.x), u.y);
}
// Fine paper structure, added to the paper colour: grain (23.75 cells per unit) and fibres
// (49 x 8.25, long in X). Lattice cells per FINE_PERIOD: 190, and 392 x 66.
fn map_fine_procedural(p: vec2<f32>) -> f32 {
    let grain = map_tile_noise(p * 23.75, vec2<f32>(190.0), 0.0);
    let fibres = map_tile_noise(p * vec2<f32>(49.0, 8.25), vec2<f32>(392.0, 66.0), 1.0);
    return (grain - 0.5) * 0.017 + (fibres - 0.5) * 0.010;
}
fn map_fine(p: vec2<f32>) -> f32 {
    if (PARCHMENT_BAKED) {
        return (textureSample(parchment_fine, parchment_tile_sampler, p / FINE_PERIOD).r - 0.5) * FINE_RANGE;
    }
    return map_fine_procedural(p);
}
// The pigment flecks (x) and the feather of the fog edge (y), 0..1 each: 17 and 7.3 cells per unit,
// 170 and 73 cells per MID_PERIOD.
fn map_mid_procedural(p: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(map_tile_noise(p * 17.0, vec2<f32>(170.0), 2.0), map_tile_noise(p * 7.3, vec2<f32>(73.0), 3.0));
}
fn map_mid(p: vec2<f32>) -> vec2<f32> {
    if (PARCHMENT_BAKED) {
        return textureSample(parchment_mid, parchment_tile_sampler, p / MID_PERIOD).rg;
    }
    return map_mid_procedural(p);
}
fn map_line(phase: f32, width: f32) -> f32 {
    let distance = abs(fract(phase) - 0.5);
    let aa = max(fwidth(phase), 0.012);
    return (1.0 - smoothstep(width, width + aa, distance)) * (1.0 - smoothstep(0.35, 0.65, aa));
}
fn map_rule(distance: f32, width: f32) -> f32 {
    let aa = max(fwidth(distance), 0.012);
    return 1.0 - smoothstep(width, width + aa, abs(distance));
}
// Whether this pixel can show part of a navigator's compass (about 3% of the plane): its
// outer ring and the antialiasing of the rules reach 2.185 + aa, aa <= max(fwidth, 0.012), and
// fwidth of a distance in compass space is at most the sum of the fwidth of the world axes.
// A caller in quad-uniform flow passes quad_any() of this as `compass_here` to the *_with functions.
fn map_compass_near(p: vec2<f32>) -> bool {
    let w = (p + vec2<f32>(2.7, 7.3)) / 18.0;
    let q = (fract(w) - 0.5) * 18.0;
    let footprint = fwidth(p.x) + fwidth(p.y);
    return length(q) < 2.23 + max(footprint, 0.012) && step(0.38, map_hash(floor(w) + 11.0)) > 0.5;
}
fn map_ornament(p: vec2<f32>) -> f32 {
    return map_ornament_with(p, true);
}
// `compass_here` false: the caller proved no compass reaches this pixel (map_compass_near).
fn map_ornament_with(p: vec2<f32>, compass_here: bool) -> f32 {
    // A sparse navigator's compass and fine rhumb lines: decoration, not invented geography.
    var compass = 0.0;
    if (compass_here) {
        let cell = floor((p + vec2<f32>(2.7, 7.3)) / 18.0);
        let q = (fract((p + vec2<f32>(2.7, 7.3)) / 18.0) - 0.5) * 18.0;
        let radius = length(q);
        let circle = map_rule(radius - 2.05, 0.007) + map_rule(radius - 2.18, 0.005);
        let axes = max(map_rule(q.x, 0.006), map_rule(q.y, 0.006));
        let diagonals = max(map_rule((q.x + q.y) * 0.7071, 0.005), map_rule((q.x - q.y) * 0.7071, 0.005));
        let spokes = max(axes, diagonals * 0.65) * (1.0 - smoothstep(2.00, 2.22, radius));
        let diamond = min(abs(q.x) + abs(q.y) * 4.1, abs(q.y) + abs(q.x) * 4.1);
        let rose = (1.0 - smoothstep(1.12, 1.20, diamond)) * 0.45;
        compass = max(circle, max(spokes, rose)) * step(0.38, map_hash(cell + 11.0));
    }
    let rules = max(map_line((p.x + p.y * 0.43) / 7.3, 0.0012), map_line((p.x - p.y * 0.71) / 9.1, 0.0012));
    return clamp(compass * 0.060 + rules * 0.020, 0.0, 0.09);
}
fn map_wash(p: vec2<f32>) -> f32 {
    let warp = vec2<f32>(map_noise(p * 0.31 + 17.1), map_noise(p * 0.27 - 8.3));
    let q = p + (warp - 0.5) * 1.65;
    return map_noise(q * 0.44 + 9.1) * 0.52
         + map_noise(q * 1.23 + 31.4) * 0.29
         + map_noise(q * 3.71 - 5.3) * 0.19;
}
// The low-frequency field computed per pixel: the one definition both the bake and the
// procedural path use. x wash(p), y wash(1.17 p), z broad(p), w meadow(p) (terrain.wgsl).
fn parchment_low_procedural(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(map_wash(p), map_wash(p * 1.17), map_noise(p * 0.105 + 41.7), map_noise(p * 0.61 + 21.0));
}
fn parchment_low_sample(p: vec2<f32>) -> vec4<f32> {
    let uv = (p - parchment_bounds.xy) * parchment_bounds.zw;
    return textureSampleLevel(parchment_low_field, fog_sampler, uv, 0.0);
}
// (wash, broad) of the paper.
fn paper_wash_broad(p: vec2<f32>) -> vec2<f32> {
    if (PARCHMENT_BAKED) { return parchment_low_sample(p).xz; }
    return vec2<f32>(map_wash(p), map_noise(p * 0.105 + 41.7));
}
// `map_wash(p * 1.17)`, the wash of the fog edge.
fn edge_wash(p: vec2<f32>) -> f32 {
    if (PARCHMENT_BAKED) { return parchment_low_sample(p).y; }
    return map_wash(p * 1.17);
}
fn map_meadow(p: vec2<f32>) -> f32 {
    if (PARCHMENT_BAKED) { return parchment_low_sample(p).w; }
    return map_noise(p * 0.61 + 21.0);
}
fn map_paper(p: vec2<f32>) -> vec3<f32> {
    return map_paper_with(p, true);
}
// `compass_here`: see map_ornament_with.
fn map_paper_with(p: vec2<f32>, compass_here: bool) -> vec3<f32> {
    // Warm aged parchment. Pigment remains world-anchored and independent of terrain.
    let low = paper_wash_broad(p);
    let wash = low.x;
    let broad = low.y;
    let tide = smoothstep(0.42, 0.67, wash) * (0.08 + broad * 0.12);
    let deposit = (1.0 - smoothstep(0.005, 0.031, abs(wash - 0.53))) * 0.030;
    let paper = vec3<f32>(0.56, 0.397, 0.235) + (broad - 0.5) * 0.065 + map_fine(p);
    let stained = mix(paper, vec3<f32>(0.30, 0.19, 0.10), tide + deposit);
    return mix(stained, vec3<f32>(0.18, 0.105, 0.050), map_ornament_with(p, compass_here) * 1.8);
}
fn map_known(fog: f32) -> f32 {
    return smoothstep(0.20, 0.45, fog);
}
fn map_edge(p: vec2<f32>, fog: f32) -> f32 {
    return map_edge_with(fog, edge_wash(p), map_mid(p).y);
}
// `wash` is `edge_wash(p)` and `feather` the second of `map_mid(p)`, which `map_finish` needs too.
fn map_edge_with(fog: f32, wash: f32, feather: f32) -> f32 {
    let progress = clamp((fog - 0.52) / 0.48, 0.0, 1.0);
    return progress - (wash * 0.76 + feather * 0.08) * (1.0 - progress);
}
fn map_reveal(p: vec2<f32>, fog: f32) -> f32 {
    // Several scales erode inward from visibility. No reveal at or below .52.
    return smoothstep(0.015, 0.61, map_edge(p, fog));
}
// Remembered land keeps a fraction of its live chroma: the last observation reads as a map
// (olive land, dark olive forest, ochre sand, grey-brown rock), not as sepia or as a veil.
// Unknown land never reaches this path: it stays parchment (`map_paper`).
const MEMORY_CHROMA: f32 = 0.40;
// Objects (trees, peaks, buildings) are small: they keep a little more chroma to stay readable.
const MEMORY_OBJECT_CHROMA: f32 = 0.55;
const MEMORY_VALUE: f32 = 0.92;
// `display` is an authored colour (display space); the result is linear, ready for `map_ink`.
// `warm` pulls land toward the map's yellow ink; water keeps its own hue (`warm` 0).
fn map_pigment(display: vec3<f32>, chroma: f32, warm: f32) -> vec3<f32> {
    let luma = dot(display, vec3<f32>(0.2126, 0.7152, 0.0722));
    // Light materials (snow, chalk) are pulled toward the map's value range: snow must
    // never read as a white veil over the parchment.
    let value = MEMORY_VALUE * min(1.0, (0.62 + max(luma - 0.62, 0.0) * 0.45) / max(luma, 0.001));
    let muted = mix(vec3<f32>(luma), display, chroma) * value;
    let tone = mix(vec3<f32>(1.0), vec3<f32>(1.07, 1.0, 0.82), warm);
    return pow(max(muted * tone, vec3<f32>(0.0)), vec3<f32>(2.2));
}
// Hatching and the pen's pressure support the form (slope, shade) over the pigment.
fn map_ink(p: vec2<f32>, normal: vec3<f32>, pigment: vec3<f32>) -> vec3<f32> {
    return map_ink_over(p, normal, pigment, map_paper(p));
}
// `paper` is `map_paper(p)`, which the caller usually has already.
fn map_ink_over(p: vec2<f32>, normal: vec3<f32>, pigment: vec3<f32>, paper: vec3<f32>) -> vec3<f32> {
    let n = normalize(normal);
    let slope = 1.0 - clamp(n.y, 0.0, 1.0);
    let shade = 1.0 - max(dot(n, normalize(vec3<f32>(-0.4, 0.9, -0.6))), 0.0);
    let stroke = map_line((p.x + p.y * 0.71) * 5.3, 0.027);
    let cross_stroke = map_line((p.x - p.y * 0.86) * 4.7, 0.019);
    let pressure = 0.01 + slope * 0.18 + shade * 0.07;
    let hatch = stroke * (0.020 + slope * 0.12 + shade * 0.050)
        + cross_stroke * smoothstep(0.65, 0.95, shade) * 0.060;
    // A trace of the paper's wash ties the pigment to the parchment around it.
    let ground = mix(pigment, paper, 0.16);
    return mix(ground, vec3<f32>(0.075, 0.062, 0.040), clamp(pressure + hatch, 0.0, 0.60));
}

// Broken pigment deposits follow the existing visibility band inward.
// No color or geography is introduced where the visibility factor is unknown.
fn map_finish(p: vec2<f32>, fog: f32, remembered: vec3<f32>, live: vec3<f32>) -> vec3<f32> {
    return map_finish_reveal(p, fog, remembered, live).rgb;
}
// The same with the reveal factor (`map_reveal`) in alpha, so a caller needing it repeats no noise.
fn map_finish_reveal(p: vec2<f32>, fog: f32, remembered: vec3<f32>, live: vec3<f32>) -> vec4<f32> {
    let wash = edge_wash(p);
    let mid = map_mid(p);
    let edge = map_edge_with(fog, wash, mid.y);
    let reveal = smoothstep(0.015, 0.61, edge);
    let edge_band = smoothstep(0.48, 0.57, fog) * (1.0 - smoothstep(0.94, 1.0, fog));
    let wet = smoothstep(-0.15, 0.02, edge) * (1.0 - smoothstep(0.38, 0.69, edge));
    let first_rim = 1.0 - smoothstep(0.007, 0.044, abs(edge - 0.01));
    let second_rim = 1.0 - smoothstep(0.006, 0.023, abs(edge - 0.16));
    let flecks = smoothstep(0.47, 0.78, mid.x);
    let deposit = wet * (0.10 + wash * 0.22) + first_rim * 0.12 + second_rim * 0.06 + flecks * wet * 0.05;
    let color = mix(remembered, live, reveal);
    return vec4<f32>(mix(color, vec3<f32>(0.33, 0.28, 0.21), deposit * edge_band), reveal);
}
