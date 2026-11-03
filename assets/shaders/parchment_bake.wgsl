// Bakes the parchment of cartography.wgsl into textures. This file is appended to cartography.wgsl, so
// the bake runs the very same noise functions as the per-pixel path (AURELIA_PARCHMENT=procedural).
//   fs_fine: the fine tile (R8, FINE_TEXELS^2, mip 0): map_fine_procedural over FINE_PERIOD units.
//   fs_mid:  the mid tile (RG8, MID_TEXELS^2, mip 0): map_mid_procedural over MID_PERIOD units.
//   fs_mip:  the next mip level of a tile (2x2 box average).
//   fs_low:  the low-frequency field over the map extent, RGBA16F (parchment_low_procedural).

struct ParchmentBake {
    // World XZ of texel (0, 0)'s corner and world units per texel (z, w unused).
    origin_texel: vec4<f32>,
};
@group(0) @binding(12) var<uniform> parchment_bake: ParchmentBake;
@group(0) @binding(13) var mip_source: texture_2d<f32>;
@group(0) @binding(14) var mip_sampler: sampler;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(corners[i], 0.0, 1.0);
}

// The sampler reads texel (x, y) at uv (x + 0.5) / TEXELS, i.e. at world (x + 0.5) * PERIOD / TEXELS,
// which is where the bake evaluates it (pixel.xy is the texel centre).
@fragment
fn fs_fine(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let p = pixel.xy * (FINE_PERIOD / FINE_TEXELS);
    return vec4<f32>(0.5 + map_fine_procedural(p) / FINE_RANGE, 0.0, 0.0, 1.0);
}

@fragment
fn fs_mid(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let p = pixel.xy * (MID_PERIOD / MID_TEXELS);
    return vec4<f32>(map_mid_procedural(p), 0.0, 1.0);
}

@fragment
fn fs_mip(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    // The centre of a destination texel is the corner shared by four source texels: a box filter.
    let uv = pixel.xy / (vec2<f32>(textureDimensions(mip_source)) * 0.5);
    return textureSampleLevel(mip_source, mip_sampler, uv, 0.0);
}

@fragment
fn fs_low(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {
    let p = parchment_bake.origin_texel.xy + pixel.xy * parchment_bake.origin_texel.z;
    return parchment_low_procedural(p);
}
