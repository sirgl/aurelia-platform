// Living water: river beds and lake surfaces. Depth along V (banks to middle),
// the current is smooth noise along U (no floor cells: no squares
// and no snapping animation), foam at the banks, rare glints in the deep.
// Time comes from the camera uniform (updated every frame).

struct Camera {
    view_proj: mat4x4<f32>,
    sun: vec4<f32>,
    time_: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;

struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) fog: f32,
    @location(3) shade: f32,
    @location(4) shore: f32,
    // River water: weight of the river mouth, palette position, weight of white water.
    @location(5) tone: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) wpos: vec3<f32>,
    @location(2) fog: f32,
    @location(3) shade: f32,
    @location(4) shore: f32,
    @location(5) tone: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VsOut {
    var o: VsOut;
    o.clip = cam.view_proj * vec4<f32>(v.pos, 1.0);
    o.uv = v.uv;
    o.wpos = v.pos;
    o.fog = v.fog;
    o.shade = v.shade;
    o.shore = v.shore;
    o.tone = v.tone;
    return o;
}

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 = p3 + dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

// Smooth value noise: interpolated lattice hashes (no floor steps).
fn vnoise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    let a = hash12(cell);
    let b = hash12(cell + vec2<f32>(1.0, 0.0));
    let c = hash12(cell + vec2<f32>(0.0, 1.0));
    let d = hash12(cell + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

// Irregular polygonal color patches echo the low-poly underwater shelf.
// Unlike a square hash grid, neighbouring patches have unrelated silhouettes.
fn sea_facet(p: vec2<f32>) -> vec2<f32> {
    let cell = floor(p);
    var nearest = 10.0;
    var second = 10.0;
    var tint = 0.5;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let id = cell + vec2<f32>(f32(x), f32(y));
            let offset = vec2<f32>(hash12(id + 19.3), hash12(id + 47.8));
            let d = distance(p, id + 0.15 + offset * 0.70);
            if (d < nearest) {
                second = nearest;
                nearest = d;
                tint = hash12(id + 71.2);
            } else {
                second = min(second, d);
            }
        }
    }
    return vec2<f32>(tint, second - nearest);
}

// Analytic slopes from three travelling wave bands. The same normal controls
// body-light modulation, reflected sky and the sun highlight.
fn sea_normal(p: vec2<f32>, t: f32) -> vec3<f32> {
    let d0 = normalize(vec2<f32>(0.85, 0.52));
    let d1 = normalize(vec2<f32>(-0.40, 0.916));
    let d2 = normalize(vec2<f32>(0.94, -0.34));
    let gradient = d0 * cos(dot(p, d0) * 2.2 - t * 0.90) * 0.19
        + d1 * cos(dot(p, d1) * 4.2 + t * 1.20) * 0.11
        + d2 * cos(dot(p, d2) * 8.4 - t * 0.70) * 0.035;
    return normalize(vec3<f32>(-gradient.x, 1.0, -gradient.y));
}

fn sea_sun(normal: vec3<f32>, view: vec3<f32>, sun: vec3<f32>) -> f32 {
    let halfway = normalize(view + sun);
    let nh = max(dot(normal, halfway), 0.0);
    let nv = max(dot(normal, view), 0.001);
    let nl = max(dot(normal, sun), 0.001);
    let vh = max(dot(view, halfway), 0.0);
    let roughness = 0.34;
    let alpha2 = roughness * roughness * roughness * roughness;
    let denominator = nh * nh * (alpha2 - 1.0) + 1.0;
    let distribution = alpha2 / (3.14159265 * denominator * denominator);
    let k = (roughness + 1.0) * (roughness + 1.0) / 8.0;
    let masking = (nv / (nv * (1.0 - k) + k)) * (nl / (nl * (1.0 - k) + k));
    let fresnel = 0.025 + 0.975 * pow(1.0 - vh, 5.0);
    return distribution * masking * fresnel / max(4.0 * nv * nl, 0.001);
}

// The body colours of river water on the palette of the country it crosses (linear):
// 0 clear mountain water, 0.3 lowland green, 0.62 turbid marsh or floodplain, 1 icy.
fn river_deep(tint: f32) -> vec3<f32> {
    let a = vec3<f32>(0.020, 0.200, 0.290);
    let b = vec3<f32>(0.011, 0.170, 0.245);
    let c = vec3<f32>(0.060, 0.130, 0.090);
    let d = vec3<f32>(0.055, 0.210, 0.300);
    if (tint < 0.3) { return mix(a, b, tint / 0.3); }
    if (tint < 0.62) { return mix(b, c, (tint - 0.3) / 0.32); }
    return mix(c, d, clamp((tint - 0.62) / 0.38, 0.0, 1.0));
}

fn river_shelf(tint: f32) -> vec3<f32> {
    let a = vec3<f32>(0.075, 0.430, 0.450);
    let b = vec3<f32>(0.046, 0.370, 0.365);
    let c = vec3<f32>(0.170, 0.270, 0.170);
    let d = vec3<f32>(0.300, 0.520, 0.600);
    if (tint < 0.3) { return mix(a, b, tint / 0.3); }
    if (tint < 0.62) { return mix(b, c, (tint - 0.3) / 0.32); }
    return mix(c, d, clamp((tint - 0.62) / 0.38, 0.0, 1.0));
}

// River U is a shared graph coordinate increasing toward the known outlet.
// Travelling highlights therefore turn with the channel, rather than sliding
// along a fixed world axis. This branch also avoids the sea's Voronoi search.
// The tone says what the river is like there: the colours of its country, white
// water where it falls fast (quicker streaks and foam on the stones).
fn river_surface(in: VsOut, time: f32) -> vec3<f32> {
    let across = clamp(abs(in.uv.y - 0.5) * 2.0, 0.0, 1.0);
    let shelf = smoothstep(0.12, 0.94, across);
    let rapids = in.tone.z;
    let u = in.uv.x;
    let advected = u - time * (0.42 + 0.60 * rapids);
    let transverse = in.uv.y * 2.0 - 1.0;
    let strands = vnoise(vec2<f32>(advected * 1.5, transverse * 6.0 + 13.7));
    let ripple = sin(advected * (8.0 + 6.0 * rapids) + transverse * 1.2 + strands * 0.6);
    let streak = smoothstep(0.35, 0.96, ripple)
        * smoothstep(0.38, 0.68, strands)
        * (1.0 - smoothstep(0.80, 1.0, across));
    var color = mix(river_deep(in.tone.y), river_shelf(in.tone.y), shelf);
    color *= 0.95 + strands * 0.10;
    let shadow = surface_shadow(in.wpos, vec3<f32>(0.0, 1.0, 0.0));
    color *= 0.89 + 0.14 * shadow;
    color += vec3<f32>(0.040, 0.11, 0.125) * streak * mix(0.45, 1.0, shadow);
    // White water: foam clumps on the current, thicker toward the middle of a fast reach.
    let clumps = smoothstep(0.58, 0.86, vnoise(vec2<f32>(advected * 3.2, transverse * 4.5 + 5.1)));
    color = mix(color, vec3<f32>(0.62, 0.78, 0.74), clumps * rapids * (1.0 - 0.5 * across) * 0.75);
    // Sparse narrow bank flecks advect with the current, not sea swash.
    let bank = 1.0 - smoothstep(0.012, 0.035, max(in.shore, 0.0));
    let flecks = smoothstep(0.70, 0.84, strands) * bank;
    color = mix(color, vec3<f32>(0.56, 0.72, 0.66), flecks * 0.28);
    return color;
}

// Surf at the observed dry contact. `shore` is the world distance from the
// wet/dry edge of the mesh (surface.rs assign_shore_distances): zero on the
// contact, 3 far from any known coast. The surf is froth, not an outline:
// short curled arcs in two or three rows that follow the coast at wobbling
// distances, each row broken into dashes by its own noise, with ragged
// frothy edges and wide gaps of clear water between them; the rows breathe
// in and out with the swell. A faint shallow halo lifts the water near the
// rock. Below about a pixel a line fades out instead of shimmering.
fn surf_row(p: vec2<f32>, d: f32, at: f32, width: f32, coverage: f32, seed: f32, pixel: f32, t: f32) -> f32 {
    // The row wobbles toward and away from the shore along the coast.
    let wobble = (vnoise(p * 3.3 + seed) - 0.5) * 0.06 + (vnoise(p * 9.0 + seed) - 0.5) * 0.02;
    let offset = abs(d - at - wobble);
    // Short dashes: thick in the middle, thin at the ends.
    let dash = smoothstep(1.0 - coverage, 1.0 - coverage + 0.14, vnoise(p * 6.3 + vec2<f32>(seed * 1.7, t * 0.03)));
    // Froth: clumps and holes along the dash, a ragged edge across it.
    let clumps = 0.55 + 0.45 * vnoise(p * 19.0 + seed + vec2<f32>(t * 0.05, 0.0));
    let half = width * dash * clumps;
    let body = 1.0 - smoothstep(half * 0.45, half + pixel, offset);
    let holes = smoothstep(0.22, 0.42, vnoise(p * 31.0 - seed));
    return body * holes * smoothstep(0.35, 0.65, dash);
}

fn coast_wash(color: vec3<f32>, p: vec2<f32>, shore: f32, pixel: f32, t: f32, calm: f32) -> vec3<f32> {
    let d = max(shore, 0.0);
    let visible = smoothstep(0.6, 1.6, 0.03 / pixel);
    let swell = 0.5 + 0.5 * sin(t * 0.85 + vnoise(p * 0.9) * 6.283);
    // Lace at the waterline, then two rows of curls farther out.
    let lace = surf_row(p, d, 0.03, 0.026, 0.46, 3.1, pixel, t);
    let first = surf_row(p, d, 0.09 + 0.025 * swell, 0.040, 0.44, 11.0, pixel, t);
    let second = surf_row(p, d, 0.17 + 0.035 * swell, 0.024, 0.30, 23.0, pixel, t) * (1.0 - 0.4 * swell);
    let foam = clamp(max(lace, max(first, second * 0.85)) * visible * (1.0 - calm), 0.0, 1.0);
    let halo = (1.0 - smoothstep(0.0, 0.25, d)) * 0.22;
    let shallow = mix(color, vec3<f32>(0.10, 0.52, 0.52), halo);
    return mix(shallow, vec3<f32>(0.80, 0.96, 0.96), foam * 0.88);
}

// The lit sea (linear): body colour, waves, caustics, reflections, sun and surf.
fn sea_live(in: VsOut, p: vec2<f32>, t: f32, shore_pixel: f32) -> vec3<f32> {
    let across = clamp(abs(in.uv.y - 0.5) * 2.0, 0.0, 1.0);
    let shelf = smoothstep(0.12, 0.94, across);
    // Water body scattering is authored in LINEAR space. The slope-dependent
    // reflected component below adds a real response to light, not white noise.
    let deep = mix(vec3<f32>(0.011, 0.17, 0.245), vec3<f32>(0.004, 0.175, 0.52), in.shade);
    let shallow = mix(vec3<f32>(0.046, 0.37, 0.365), vec3<f32>(0.028, 0.48, 0.50), in.shade);
    var body = mix(deep, shallow, shelf);
    let current = vnoise(p * 0.23 + vec2<f32>(4.7, 11.1));
    body *= 0.91 + current * 0.18;
    let facet = sea_facet(p * 3.15);
    body *= 0.91 + facet.x * 0.18;
    // Caustic light follows broad intersecting wave bands, never the polygon
    // boundaries: outlining every facet made the sea resemble pool tiles.
    let caustic_wave = sin(p.x * 3.1 + p.y * 1.7 - t * 0.52)
        * sin(p.x * -1.3 + p.y * 3.8 + t * 0.43);
    let caustic = pow(max(caustic_wave, 0.0), 7.0);
    body += vec3<f32>(0.018, 0.048, 0.042) * caustic * shelf;

    let n = sea_normal(p, t);
    // The sandbox camera is orthographic, looking down at 45 degrees.
    let view = normalize(vec3<f32>(0.0, 1.0, 1.0));
    let sun = normalize(cam.sun.xyz);
    let reflected = reflect(-view, n);
    let sky = mix(vec3<f32>(0.39, 0.57, 0.69), vec3<f32>(0.17, 0.34, 0.55), clamp(reflected.y, 0.0, 1.0));
    let fresnel = 0.025 + 0.975 * pow(1.0 - max(dot(n, view), 0.0), 5.0);
    let visibility = surface_shadow(in.wpos, vec3<f32>(0.0, 1.0, 0.0));
    let body_light = 0.89 + 0.17 * max(dot(n, sun), 0.0) * visibility;
    // Open ocean, far from every known coast, is darker and deeper; a lake
    // (shade 0.75) is calm and greener, without surf.
    let calm = 1.0 - smoothstep(0.8, 0.95, in.shade);
    let ocean = smoothstep(1.3, 2.9, in.shore) * (1.0 - calm);
    body = mix(body, vec3<f32>(0.004, 0.085, 0.30), ocean * 0.6);
    body = mix(body, body * vec3<f32>(0.80, 1.05, 0.88), calm);
    var linear = mix(body * body_light, sky, fresnel * (1.0 - 0.5 * calm));
    linear += vec3<f32>(1.0, 0.96, 0.82) * sea_sun(n, view, sun) * 5.5 * visibility;

    // Broad broken sky reflections carry readable motion at strategic zoom.
    // The seabed facets stay fixed, so the sea never becomes a sliding mosaic.
    // This is surface shading only: no CPU remesh, texture or extra draw pass.
    let travelling = sin(dot(p, vec2<f32>(2.8, 1.7)) - t * 1.05 + current * 2.0);
    let crossing = sin(dot(p, vec2<f32>(-2.1, 4.8)) + t * 0.74);
    let reflection_band = smoothstep(0.35, 0.98, travelling)
        * smoothstep(-0.45, 0.90, crossing);
    linear += vec3<f32>(0.025, 0.070, 0.095) * reflection_band
        * mix(0.25, 1.0, in.shade) * mix(0.50, 1.0, visibility);

    // The wash is exactly zero beyond its reach: the farthest surf row (0.205 + 0.04
    // wobble + 0.024 froth) and the halo (0.25) end before 0.28, plus a pixel of edge
    // softness. Open water skips its fifteen noise evaluations per pixel.
    if (max(in.shore, 0.0) < 0.28 + shore_pixel) {
        linear = coast_wash(linear, p, in.shore, shore_pixel, t, calm);
    }

    return linear;
}

// Pixels compute only what the fog shows of them (the regimes of cartography.wgsl): the
// lit water alone where the map is visible, the remembered paper alone where it is not. As in
// terrain.wgsl every fog branch is taken by the four pixels of a 2x2 quad together (quad_any).
@diagnostic(off, derivative_uniformity)
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = cam.time_.x;
    // Derivatives before any branch: uniform control flow.
    let shore_pixel = max(fwidth(max(in.shore, 0.0)), 1e-4);
    let p = in.wpos.xz;
    let fog = min(in.fog, map_fog(p));
    if (!quad_any(fog > BARE_FOG_MAX)) {
        return vec4<f32>(map_paper_with(p, quad_any(map_compass_near(p))), 1.0);
    }
    let any_live = quad_any(fog > REMEMBERED_FOG_MAX);
    let any_remembered = quad_any(fog < VISIBLE_FOG_MIN);
    let any_edge = quad_any(fog > EDGE_FOG_MIN);
    var linear = vec3<f32>(0.0);
    var remembered_tone = vec3<f32>(0.0);
    if (in.shade < 0.5) {
        let across = clamp(abs(in.uv.y - 0.5) * 2.0, 0.0, 1.0);
        // A river keeps its thin water line and dark banks outside sight.
        remembered_tone = pow(mix(vec3<f32>(0.34, 0.50, 0.58), vec3<f32>(0.20, 0.31, 0.38), smoothstep(0.55, 1.0, across)), vec3<f32>(2.2));
        if (any_live) {
            linear = river_surface(in, t);
            // At a mouth the river's water becomes the sea's, without a seam: the sea's own body
            // colour of the water it flows into (shade 0.2: sea, 0.3: lake), blended by the weight.
            if (in.tone.x > 0.001) {
                var sea_in = in;
                sea_in.shade = select(1.0, 0.75, in.shade > 0.25);
                sea_in.shore = 0.4;
                sea_in.uv.y = mix(in.uv.y, 1.0, smoothstep(0.0, 0.6, in.tone.x));
                linear = mix(linear, sea_live(sea_in, p, t, shore_pixel), smoothstep(0.0, 1.0, in.tone.x));
            }
        }
    } else {
        if (any_live) {
            linear = sea_live(in, p, t, shore_pixel);
        }
        // Remembered water is static: no animated wave, sky or sun contribution.
        // A calm blue-grey wash, a little deeper far from the coast; lakes lean green.
        let depth = smoothstep(0.3, 2.6, max(in.shore, 0.0));
        let wash = mix(vec3<f32>(0.40, 0.54, 0.60), vec3<f32>(0.25, 0.38, 0.49), depth);
        let lake = mix(wash, vec3<f32>(0.34, 0.50, 0.52), 1.0 - smoothstep(0.8, 0.95, in.shade));
        remembered_tone = pow(lake, vec3<f32>(2.2));
    }
    if (!any_remembered) {
        return vec4<f32>(linear, 1.0);
    }
    let paper = map_paper_with(p, quad_any(map_compass_near(p)));
    let remembered = mix(paper, map_ink_over(p, vec3<f32>(0.0, 1.0, 0.0), remembered_tone, paper), map_known(fog));
    if (!any_edge) {
        return vec4<f32>(remembered, 1.0);
    }
    return vec4<f32>(map_finish(p, fog, remembered, linear), 1.0);
}
