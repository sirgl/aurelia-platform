//! The pack's art catalog: what the renderer draws for the sandbox's rows.

use sandbox_rules::Rules;

use crate::pack::Pack;

/// Side of a ground texture layer (the renderer's layers are square and of
/// one size).
pub const LAYER_SIZE: u32 = 128;

/// The model asset of every unit kind of `rules`, in kind order (None: the
/// pack names no model, the renderer draws its fallback).
pub fn unit_models<'p>(pack: &'p Pack, rules: &Rules) -> Vec<Option<&'p str>> {
    let model = |key: &str| pack.units.iter().find(|u| u.key == key).map(|u| u.model.as_str());
    rules.units.iter().map(|u| model(&u.key)).collect()
}

/// The ground texture layers (width, height, RGBA): each colour of the pack
/// with a faint deterministic grain.
pub fn terrain_layers(pack: &Pack) -> Vec<(u32, u32, Vec<u8>)> {
    let size = LAYER_SIZE;
    let layer = |(i, &(r, g, b)): (usize, &(u8, u8, u8))| {
        let mut rgba = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let grain = (grain(x, y, i as u32) % 9) as i16 - 4;
                for c in [r, g, b] {
                    rgba.push((i16::from(c) + grain).clamp(0, 255) as u8);
                }
                rgba.push(255);
            }
        }
        (size, size, rgba)
    };
    pack.layers.iter().enumerate().map(layer).collect()
}

fn grain(x: u32, y: u32, salt: u32) -> u32 {
    let mut h = x.wrapping_mul(0x85EB_CA6B) ^ y.wrapping_mul(0xC2B2_AE35) ^ salt.wrapping_mul(0x27D4_EB2F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}
