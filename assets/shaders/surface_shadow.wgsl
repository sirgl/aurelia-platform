// Shared directional shadow receiver. Append before terrain/feature/model/water shaders.
// Group 2 is identical for every receiving pipeline.
struct SurfaceShadowUniform {
    view_proj: mat4x4<f32>,
    // texel size, depth bias, world normal offset, enabled
    settings: vec4<f32>,
};
@group(2) @binding(0) var<uniform> surface_shadow_data: SurfaceShadowUniform;
@group(2) @binding(1) var surface_shadow_depth: texture_depth_2d;
@group(2) @binding(2) var surface_shadow_sampler: sampler_comparison;

fn surface_shadow(world: vec3<f32>, normal: vec3<f32>) -> f32 {
    if (surface_shadow_data.settings.w < 0.5) { return 1.0; }
    let p = surface_shadow_data.view_proj * vec4<f32>(world + normal * surface_shadow_data.settings.z, 1.0);
    let ndc = p.xyz / p.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (ndc.z <= 0.0 || ndc.z >= 1.0 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) { return 1.0; }
    var visibility = 0.0;
    // Explicit-level comparisons keep this valid under fog/feature branches.
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let sample_uv = uv + vec2<f32>(f32(x), f32(y)) * surface_shadow_data.settings.x;
            visibility += textureSampleCompareLevel(surface_shadow_depth, surface_shadow_sampler, sample_uv, ndc.z - surface_shadow_data.settings.y);
        }
    }
    return visibility / 9.0;
}
