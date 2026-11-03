// Yield icons: billboards of constant screen size (as in Nation).
// The quad is expanded in clip space (x clip.w keeps the perspective honest),
// so they read at any camera angle instead of smearing into the depth.
// No fog (explored only, so remembered yields are honest). Transparency is discard.
// time_.y is the screen aspect (W/H) for square icons.

struct Camera {
    view_proj: mat4x4<f32>,
    sun: vec4<f32>,
    time_: vec4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var icons_tex: texture_2d<f32>;
@group(1) @binding(1) var icons_smp: sampler;

struct VertexIn {
    @location(0) uv: vec2<f32>,
    @location(1) offset: vec3<f32>,
    @location(2) cell_scale: vec2<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) cell: f32,
};

@vertex
fn vs_main(v: VertexIn) -> VsOut {
    var o: VsOut;
    // The anchor is the icon centre above the tile; the size is a fraction of the screen height,
    // shrunk with distance (otherwise the wide view is confetti over the whole map).
    // anchor.w is the perspective depth (~6 close, ~22 far):
    // 1.0 close, 0.30 far.
    let anchor = cam.view_proj * vec4<f32>(v.offset, 1.0);
    let dist_k = clamp(6.0 / max(anchor.w, 0.5), 0.40, 1.0);
    let k = 0.06 * v.cell_scale.y / 0.32 * dist_k;
    let asp = max(cam.time_.y, 0.5);
    var clip = anchor;
    clip.x = clip.x + (v.uv.x - 0.5) * k * asp * anchor.w;
    clip.y = clip.y + (v.uv.y - 0.5) * k * anchor.w;
    o.clip = clip;
    o.uv = v.uv;
    o.cell = v.cell_scale.x;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // 6 atlas cells (keep in sync with YIELD_ATLAS_CELLS in buffers.rs!).
    let u = (in.cell + in.uv.x) / 6.0;
    let texel = textureSample(icons_tex, icons_smp, vec2<f32>(u, in.uv.y));
    if (texel.a < 0.5) {
        discard;
    }
    return vec4<f32>(texel.rgb, 1.0);
}
