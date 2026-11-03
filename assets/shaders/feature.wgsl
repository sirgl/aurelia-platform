// Hard features: pos + flat normal + baked colour + fog.
// The same light as the terrain (warm sun / cool sky / ACES), otherwise the mountains
// would look like they came from another world. No transparency (opaque peaks).

struct Camera {
    view_proj: mat4x4<f32>,
    sun: vec4<f32>,
    time_: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;

// Normal and colour arrive as half floats (`GpuFeatureVertex`).
struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) fog: f32,
    @location(2) normal: vec4<f32>,
    @location(3) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) fog: f32,
    @location(3) wpos: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VsOut {
    var o: VsOut;
    o.clip = cam.view_proj * vec4<f32>(v.pos, 1.0);
    o.color = v.color.rgb;
    o.normal = v.normal.xyz;
    o.fog = v.fog;
    o.wpos = v.pos;
    return o;
}

// A fully visible pixel is its lit colour alone (the reveal is 1, the edge band 0 in
// `map_finish`): it skips the remembered pigment, as the terrain and water shaders do.
@diagnostic(off, derivative_uniformity)
@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Unlit overlays (negative fog is their alpha) fade out of the visible area.
    if (in.fog < 0.0) { return vec4<f32>(in.color, -in.fog * clamp((map_fog(in.wpos.xz) - 0.55) / 0.4, 0.0, 1.0)); }
    let fog = min(in.fog, map_fog(in.wpos.xz));
    let n = normalize(in.normal);
    let sun = max(dot(n, cam.sun.xyz), 0.0) * surface_shadow(in.wpos, n);
    let sky = vec3<f32>(0.34, 0.43, 0.59) * (0.72 + 0.28 * max(n.y, 0.0));
    let light = sky + vec3<f32>(1.0, 0.94, 0.82) * sun * 0.95;
    let linear = pow(max(in.color, vec3<f32>(0.0)), vec3<f32>(2.2)) * light;
    if (fog >= VISIBLE_FOG_MIN) {
        return vec4<f32>(linear, 1.0);
    }
    let luminance = dot(linear, vec3<f32>(0.2126, 0.7152, 0.0722));
    // The object's own colour at the map's chroma; the light only modulates its value.
    let pigment = map_pigment(in.color, MEMORY_OBJECT_CHROMA, 1.0) * clamp(0.62 + luminance * 1.1, 0.78, 1.15);
    let paper = map_paper(in.wpos.xz);
    let remembered = mix(paper, map_ink_over(in.wpos.xz, n, pigment, paper), map_known(fog));
    return vec4<f32>(map_finish(in.wpos.xz, fog, remembered, linear), 1.0);
}
