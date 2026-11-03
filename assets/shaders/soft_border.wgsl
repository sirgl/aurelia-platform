struct Camera { view_proj: mat4x4<f32> };
@group(0) @binding(0) var<uniform> cam: Camera;
struct In { @location(0) pos: vec3<f32>, @location(1) color: vec3<f32>, @location(2) across: f32, @location(3) opacity: f32 };
struct Out { @builtin(position) clip: vec4<f32>, @location(0) color: vec3<f32>, @location(1) across: f32, @location(2) opacity: f32, @location(3) wpos: vec3<f32> };
@vertex fn vs_main(v: In) -> Out {
    var o: Out; o.clip=cam.view_proj*vec4<f32>(v.pos,1.0);o.color=v.color;o.across=v.across;o.opacity=v.opacity;o.wpos=v.pos;return o;
}
// `across` is 0 on the border and 1 at the inner end of the territory band
// (gpu/soft_borders.rs): a crisp line on the edge, then a wash fading inward.
@fragment fn fs_main(v: Out) -> @location(0) vec4<f32> {
    // Borders fade out of the currently visible area (knowledge comes from the fog field).
    let visible = clamp((map_fog(v.wpos.xz) - 0.55) / 0.4, 0.0, 1.0);
    let line = 1.0 - smoothstep(0.12, 0.24, v.across);
    let wash = 0.38 * (1.0 - smoothstep(0.15, 1.0, v.across));
    let alpha = v.opacity * visible * max(line, wash);
    if (alpha < 0.002) {discard;}
    return vec4<f32>(pow(v.color,vec3<f32>(2.2)),alpha);
}
