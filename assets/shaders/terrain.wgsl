// Terrain chunks: texture splat + flat light per face.
// Vertex: up to 4 texture-array layers with weights (sum 255), world UVs.
// Layers: 0..=4 land (plains/grass/hills/desert/snow), 5 water (procedural).

struct Camera {
    view_proj: mat4x4<f32>,
    // Direction TO the sun (xyz) + ambient (w); time_.x is the animation clock (frozen at 0 on
    // Quality::Low and with reduced motion), read by the fallout glow below.
    sun: vec4<f32>,
    time_: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var terrain_tex: texture_2d_array<f32>;
@group(1) @binding(1) var terrain_smp: sampler;

// Compiled out for AURELIA_VISUAL_ROADS=geometry; one lookup for surface material mode.
const ROAD_MASK_ENABLED: bool = true;
const GROUND_DETAIL_ENABLED: bool = true;
@group(3) @binding(0) var road_mask: texture_2d<f32>;
@group(3) @binding(1) var road_sampler: sampler;
@group(3) @binding(2) var<uniform> road_bounds: vec4<f32>;


struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) blend_idx: u32,
    @location(4) blend_w: u32,
    @location(5) fog: f32,
    @location(6) water: f32,
    @location(7) authored_albedo: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // World position: the faceted normal and the snow are computed in the fragment.
    @location(1) wpos: vec3<f32>,
    @location(2) @interpolate(flat) blend_idx: u32,
    @location(3) @interpolate(flat) blend_w: u32,
    @location(4) fog: f32,
    @location(5) water: f32,
    // Smoothed vertex normal (faces accumulated in the emitter).
    @location(6) normal: vec3<f32>,
    @location(7) authored_albedo: vec4<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VsOut {
    var o: VsOut;
    o.clip = cam.view_proj * vec4<f32>(v.pos, 1.0);
    o.uv = v.uv;
    o.wpos = v.pos;
    o.blend_idx = v.blend_idx;
    o.blend_w = v.blend_w;
    o.fog = v.fog;
    o.water = v.water;
    o.authored_albedo = v.authored_albedo;
    o.normal = v.normal;
    return o;
}

// Palette values are authored in display space, lighting and output are linear.
fn biome_color(layer: i32) -> vec3<f32> {
    switch layer {
        case 0: { return vec3<f32>(0.59, 0.71, 0.34); }
        case 1: { return vec3<f32>(0.48, 0.67, 0.31); }
        case 2: { return vec3<f32>(0.56, 0.69, 0.38); }
        case 3: { return vec3<f32>(0.83, 0.72, 0.48); }
        case 4: { return vec3<f32>(0.89, 0.92, 0.94); }
        case 5: { return vec3<f32>(0.47, 0.50, 0.56); }
        default: { return vec3<f32>(0.07, 0.43, 0.64); }
    }
}
fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Irregular flagstones: jittered Voronoi cells in stone units.
// x: rank of the stone (which stones survive a low weight), y: distance to
// the joint (F2 - F1), z: tint of the stone.
fn flagstone_cells(p: vec2<f32>) -> vec3<f32> {
    let cell = floor(p);
    var nearest = 10.0;
    var second = 10.0;
    var id = cell;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let c = cell + vec2<f32>(f32(x), f32(y));
            let jitter = vec2<f32>(hash12(c + 3.17), hash12(c + 9.71));
            let d = distance(p, c + 0.12 + jitter * 0.76);
            if (d < nearest) {
                second = nearest;
                nearest = d;
                id = c;
            } else {
                second = min(second, d);
            }
        }
    }
    return vec3<f32>(hash12(id + 5.3), second - nearest, hash12(id + 17.9));
}

// Weathered natural flagstone; `weight` comes from the CPU stone field.
// Large irregular slabs, each with its own tint and inner mottling, split by
// thin dark cracks (no light grout); grass overgrows the rock in broad
// organic patches rather than in every joint, and a low weight leaves only
// the strongest slabs breaking through the turf. Below a few pixels per slab
// the pattern resolves to its average instead of aliasing.
// Returns the stone color and its coverage over the grass.
fn flagstones(p: vec2<f32>, height: f32, weight: f32) -> vec4<f32> {
    let stones_per_unit = 2.7;
    let warp = vec2<f32>(map_noise(p * 1.1), map_noise(p * 1.1 + 7.7)) - 0.5;
    let q = p * stones_per_unit + warp * 1.2;
    let s = flagstone_cells(q);
    let aa = max(fwidth(s.y), 0.002);
    // Thin, uneven cracks between slabs.
    let crack = 1.0 - smoothstep(0.015 - aa, 0.045 + aa, s.y * (0.7 + 0.6 * map_noise(q * 0.9)));
    let threshold = 0.08 + 0.80 * s.x;
    let present = smoothstep(threshold - 0.08, threshold + 0.08, weight * 1.05);
    // Grass reclaims the rock in broad patches, more where the stone is thin.
    let moss = smoothstep(0.52, 0.70, map_noise(p * 1.7 + warp * 2.0) + (1.0 - weight) * 0.25);
    let sunlit = clamp((height - 0.10) / 0.40, 0.0, 1.0);
    var stone = mix(vec3<f32>(0.64, 0.62, 0.56), vec3<f32>(0.75, 0.70, 0.59), s.z);
    stone = mix(stone, vec3<f32>(0.60, 0.61, 0.60), step(0.7, fract(s.z * 7.3)) * 0.6);
    // Mottled lichen and weathering inside each slab.
    stone *= 0.90 + 0.16 * map_noise(p * 9.0 + s.z * 13.0);
    stone *= 0.96 + 0.06 * sunlit;
    stone = mix(stone, stone * vec3<f32>(0.42, 0.40, 0.38), crack);
    let pixels_per_stone = 1.0 / max(fwidth(q.x), 1e-4);
    let resolved = smoothstep(2.5, 6.0, pixels_per_stone);
    let coverage = mix(weight * 0.6, present * (1.0 - moss * 0.85), resolved);
    let color = mix(vec3<f32>(0.57, 0.55, 0.50), stone, resolved);
    return vec4<f32>(color, coverage);
}
// The lit ground colour (linear): biome blend, stone, detail, roads, light and shadow.
// Only the visible part of the map shows it (reveal > 0).
// `stone_here`: some pixel of this 2x2 quad may carry flagstones (the same for the whole quad).
fn live_ground(in: VsOut, n: vec3<f32>, wall: bool, stone_here: bool) -> vec3<f32> {
    var albedo = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0u; i < 4u; i = i + 1u) {
        let layer = i32((in.blend_idx >> (i * 8u)) & 255u);
        let weight = f32((in.blend_w >> (i * 8u)) & 255u);
        albedo += biome_color(layer) * weight;
        total += weight;
    }
    albedo /= max(total, 1.0);
    // Broad subtle triangles support the geometry; no high-frequency green noise.
    let p = in.wpos.xz * 1.4;
    let cell = floor(p);
    let half = select(0.0, 17.0, fract(p.x) + fract(p.y) > 1.0);
    albedo *= 0.985 + hash12(cell + half) * 0.03;
    let meadow = map_meadow(in.wpos.xz);
    albedo *= mix(vec3<f32>(0.90, 0.965, 0.91), vec3<f32>(1.05, 1.018, 0.95), meadow);
    let stone_weight = clamp(in.authored_albedo.a - 1.0, 0.0, 1.0);
    // Flagstones only where the mesh carries stone (weight 0 gives no stone and no tint).
    var paved = vec4<f32>(0.0);
    if (stone_here) {
        paved = flagstones(in.wpos.xz, in.wpos.y - 0.08, stone_weight);
    }
    let cliff = 1.0 - smoothstep(0.58, 0.82, n.y);
    albedo = mix(albedo, vec3<f32>(0.48, 0.49, 0.49), cliff * 0.86);
    let shore = smoothstep(0.95, 1.85, in.water);
    albedo = mix(albedo, vec3<f32>(0.90, 0.83, 0.64), shore * 0.88);
    // Existing mipmapped atlas: one sample, no added geometry or render pass.
    var ground_detail = vec3<f32>(1.0);
    if (GROUND_DETAIL_ENABLED) {
        let detail_uv = vec2<f32>(
            dot(in.wpos.xz, vec2<f32>(0.94, 0.342)),
            dot(in.wpos.xz, vec2<f32>(-0.342, 0.94))) / 2.8;
        ground_detail = textureSample(terrain_tex, terrain_smp, detail_uv, 1).rgb
            / vec3<f32>(140.0 / 255.0, 171.0 / 255.0, 84.0 / 255.0);
    }
    if (in.authored_albedo.a > 0.5) {
        albedo = in.authored_albedo.rgb;
        let grass = smoothstep(0.01, 0.10, albedo.g - albedo.r)
            * smoothstep(0.04, 0.18, albedo.g - albedo.b)
            * smoothstep(0.50, 0.80, n.y);
        albedo *= mix(vec3<f32>(1.0), ground_detail, grass * 0.62);
        if (wall) {
            // A thin dark wet band where the swash reaches the rock.
            let wet = 1.0 - smoothstep(0.004, 0.055 + 0.03 * map_noise(in.wpos.xz * 3.0), in.wpos.y - 0.08);
            albedo = mix(albedo, vec3<f32>(0.21, 0.25, 0.26), wet * 0.85);
        } else {
            // Turf between the slabs sits a little in their shade.
            albedo *= 1.0 - 0.10 * stone_weight;
            albedo = mix(albedo, paved.rgb, paved.a);
        }
    }
    if (ROAD_MASK_ENABLED && in.authored_albedo.a > 0.5) {
        let road_uv = (in.wpos.xz - road_bounds.xy) * road_bounds.zw;
        let coverage = textureSampleLevel(road_mask, road_sampler, road_uv, 0.0).r;
        albedo = mix(albedo, vec3<f32>(0.74, 0.68, 0.44), coverage);
    }
    // Fallout (the vertex carries its share in uv.y): drifting patches of toxic green that pulse
    // and flicker, and a faint glow of their own. Time only; nothing here changes the map.
    var glow = vec3<f32>(0.0);
    let fallout = in.uv.y;
    if (fallout > 0.01) {
        let t = cam.time_.x;
        let p = in.wpos.xz;
        let drift = vec2<f32>(0.045, -0.03) * t;
        let blob = map_noise(p * 1.7 + drift) * 0.6 + map_noise(p * 4.1 - drift * 1.9 + 7.3) * 0.4;
        let spots = smoothstep(0.48, 0.70, blob) * fallout;
        let pulse = 0.55 + 0.45 * sin(t * 1.4 + map_noise(p * 0.7) * 9.0);
        let flicker = 0.85 + 0.15 * sin(t * 9.0 + blob * 40.0);
        albedo = mix(albedo, vec3<f32>(0.40, 0.70, 0.16), spots * 0.42);
        glow = vec3<f32>(0.10, 0.60, 0.06) * (spots * pulse * flicker * 0.60 + fallout * 0.05);
    }
    let sun = max(dot(n, cam.sun.xyz), 0.0) * surface_shadow(in.wpos, n);
    let sky = vec3<f32>(0.28, 0.36, 0.50) * (0.65 + 0.35 * n.y);
    let light = sky + vec3<f32>(1.06, 1.015, 0.89) * sun * 0.98;
    return pow(max(albedo, vec3<f32>(0.0)), vec3<f32>(2.2)) * light + glow;
}

// The ground colour of a remembered pixel (display space): the biome blend, cliff and shore of
// `live_ground` without stone, detail, roads or light, so the last observation keeps its materials.
fn remembered_albedo(in: VsOut, n: vec3<f32>) -> vec3<f32> {
    var albedo = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0u; i < 4u; i = i + 1u) {
        let layer = i32((in.blend_idx >> (i * 8u)) & 255u);
        let weight = f32((in.blend_w >> (i * 8u)) & 255u);
        albedo += biome_color(layer) * weight;
        total += weight;
    }
    albedo /= max(total, 1.0);
    let cliff = 1.0 - smoothstep(0.58, 0.82, n.y);
    albedo = mix(albedo, vec3<f32>(0.48, 0.49, 0.49), cliff * 0.86);
    let shore = smoothstep(0.95, 1.85, in.water);
    albedo = mix(albedo, vec3<f32>(0.90, 0.83, 0.64), shore * 0.88);
    if (in.authored_albedo.a > 0.5) {
        albedo = in.authored_albedo.rgb;
    }
    return albedo;
}

// Each quad computes only what the knowledge fog shows of its pixels (the fog regimes,
// cartography.wgsl): the bare parchment up to BARE_FOG_MAX; the remembered paper and ink up
// to EDGE_FOG_MIN; the live ground, the reveal and its edge band above; the lit ground alone
// at VISIBLE_FOG_MIN. The branches follow the fog, which is continuous. They are taken on
// quad_any(), i.e. identically by the four pixels of a 2x2 quad, so the derivatives (dpdx,
// fwidth) stay well defined inside them; WGSL's uniformity analysis cannot prove that, hence
// the diagnostic is off here.
@diagnostic(off, derivative_uniformity)
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let fog = min(in.fog, map_fog(in.wpos.xz));
    // Every branch below is taken by all four pixels of a 2x2 quad together (quad_any), so the
    // derivatives inside stay defined. A quad that mixes regimes computes the parts its pixels
    // do not show as well; each such part has weight 0 in the pixels that do not need it.
    // Unknown land (fog <= BARE_FOG_MAX) is the bare parchment, nothing else is visible.
    if (!quad_any(fog > BARE_FOG_MAX)) {
        return vec4<f32>(map_paper_with(in.wpos.xz, quad_any(map_compass_near(in.wpos.xz))), 1.0);
    }
    let any_live = quad_any(fog > REMEMBERED_FOG_MAX);
    let any_remembered = quad_any(fog < VISIBLE_FOG_MIN);
    let any_edge = quad_any(fog > EDGE_FOG_MIN);
    // Ground with a flagstone weight (1 + w, see surface_mesh.rs); 1.003 passes interpolated 1.0.
    let any_stone = quad_any(in.authored_albedo.a > 1.003 && in.authored_albedo.a < 2.5);
    var face = normalize(cross(dpdx(in.wpos), dpdy(in.wpos)));
    if (face.y < 0.0) { face = -face; }
    let vertex_normal = normalize(in.normal);
    var face_weight = mix(0.65, 1.0, 1.0 - smoothstep(0.45, 0.86, vertex_normal.y));
    // Cliff walls (surface_mesh.rs WALL_MATERIAL) carry their final normal.
    let wall = in.authored_albedo.a > 2.5;
    if (wall) { face_weight = 0.0; }
    let n = normalize(mix(vertex_normal, face, face_weight));
    var linear = vec3<f32>(0.0);
    if (any_live) {
        linear = live_ground(in, n, wall, any_stone);
    }
    if (!any_remembered) {
        return vec4<f32>(linear, 1.0);
    }
    let paper = map_paper_with(in.wpos.xz, quad_any(map_compass_near(in.wpos.xz)));
    let known_mask = map_known(fog);
    let coast = map_line(in.water * 2.8, 0.016) * smoothstep(0.48, 0.75, in.water);
    let relief = map_line(in.wpos.y * 4.3, 0.025) * smoothstep(0.35, 0.8, 1.0 - n.y);
    var pigment = map_pigment(remembered_albedo(in, n), MEMORY_CHROMA, 1.0);
    if (ROAD_MASK_ENABLED && in.authored_albedo.a > 0.5) {
        let road_uv = (in.wpos.xz - road_bounds.xy) * road_bounds.zw;
        let road = textureSampleLevel(road_mask, road_sampler, road_uv, 0.0).r;
        pigment = mix(pigment, vec3<f32>(0.20, 0.14, 0.08), road * 0.62);
    }
    // Thin ink along the shore and the relief contours.
    let line_ink = mix(vec3<f32>(0.20, 0.17, 0.12), vec3<f32>(0.10, 0.14, 0.16), coast);
    let ink = mix(map_ink_over(in.wpos.xz, n, pigment, paper), line_ink, coast * 0.55 + relief * 0.12);
    let remembered = mix(paper, ink, known_mask);
    // Up to EDGE_FOG_MIN the reveal is 0 and the edge band absent: the remembered colour alone.
    if (!any_edge) {
        return vec4<f32>(remembered, 1.0);
    }
    let finish = map_finish_reveal(in.wpos.xz, fog, remembered, linear);
    // No hard dark perimeter. A small neutral wash stays within the mixed transition.
    let edge = finish.a * (1.0 - finish.a) * 0.06;
    return vec4<f32>(mix(finish.rgb, vec3<f32>(0.36, 0.43, 0.45), edge), 1.0);
}
