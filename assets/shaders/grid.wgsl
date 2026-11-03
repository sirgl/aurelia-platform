// Hex grid: thin dark lines over the terrain (tile legibility).
// Toggled from the UI; does not touch the simulation.

struct Camera {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> cam: Camera;

struct VertexIn {
    @location(0) pos: vec3<f32>,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) wpos: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VsOut {
    var o: VsOut;
    o.clip = cam.view_proj * vec4<f32>(v.pos, 1.0);
    o.wpos = v.pos;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Lines are built over known land; only the currently visible part is drawn.
    if (map_fog(in.wpos.xz) < 0.55) { discard; }
    // Muted blue-grey lines (they used to be nearly black and fought with the terrain).
    return vec4<f32>(0.73, 0.76, 0.55, 0.32);
}
