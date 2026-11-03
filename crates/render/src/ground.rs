//! The ground texture layers of the terrain, shared by every module (the classic module
//! and Lands & Laws draw the same ground): procedural low-poly layers - a flat palette
//! colour with soft blotches (and a turf pattern on the grass), deterministic
//! (index hashes, no RNG). The result is raw RGBA8; mips, padding and upload are
//! the renderer's business (`gpu::textures`).

/// Low-poly palette of the layers (the order is the layers 0..=5 of the texture
/// array). In step with the style colours of the renderer (a sync test in
/// `gpu::textures` checks that the mean of a layer is about its style colour):
/// change only together with them.
pub const PALETTE: [(&str, [u8; 3]); 6] = [
    ("plains", [168, 179, 92]),
    ("grass", [140, 171, 84]),
    ("hills", [150, 163, 99]),
    ("desert", [203, 178, 110]),
    ("snow", [217, 222, 230]),
    ("mountain", [117, 125, 143]),
];

/// Side of a procedural layer (square; the mip chains of the array need equal sizes).
pub const LAYER_SIZE: u32 = 128;

/// Integer coordinate hash (deterministic mottling, no RNG).
fn hash2(x: u32, y: u32, salt: u32) -> u32 {
    let mut h = x.wrapping_mul(0x85EB_CA6B) ^ y.wrapping_mul(0xC2B2_AE35) ^ salt;
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    h
}

/// Small irregular material patches, baked once into the existing grass layer.
/// Wrapped feature points keep the pattern continuous across repeated UVs.
fn grass_detail(x: u32, y: u32) -> [i16; 3] {
    const CELLS: i32 = 12;
    let px = x as f32 / LAYER_SIZE as f32 * CELLS as f32;
    let py = y as f32 / LAYER_SIZE as f32 * CELLS as f32;
    let mut nearest = f32::INFINITY;
    let mut seed = 0;
    let mut local = [0.; 2];
    for gy in py.floor() as i32 - 1..=py.floor() as i32 + 1 {
        for gx in px.floor() as i32 - 1..=px.floor() as i32 + 1 {
            let h = hash2(
                gx.rem_euclid(CELLS) as u32,
                gy.rem_euclid(CELLS) as u32,
                1907,
            );
            let fx = gx as f32 + 0.15 + (h & 255) as f32 / 255. * 0.7;
            let fy = gy as f32 + 0.15 + ((h >> 8) & 255) as f32 / 255. * 0.7;
            let distance = (fx - px).powi(2) + (fy - py).powi(2);
            if distance < nearest {
                nearest = distance;
                seed = h;
                local = [px - fx, py - fy];
            }
        }
    }
    let shade = ((seed >> 16) % 29) as i16 - 14;
    let grain = (hash2(x, y, 2399) % 3) as i16 - 1;
    // Small angular clover/dry-grass patches, with quiet turf between them.
    // All detail is baked into the existing mipmapped layer, not per-frame noise.
    let patch = (local[0] * 0.78 + local[1] * 0.22).abs()
        + (local[1] * 0.91 - local[0] * 0.18).abs();
    let motif = (seed >> 24) % 9;
    let tint = if patch < 0.22 && motif < 2 {
        [20, 8, -6]
    } else if patch < 0.43 && motif < 5 {
        [-14, -6, -6]
    } else if motif == 8 {
        [8, -7, -3]
    } else {
        [0; 3]
    };
    std::array::from_fn(|c| shade + grain + tint[c])
}

/// All the ground layers as (width, height, RGBA8): squares of `LAYER_SIZE`, in
/// the order of `PALETTE`.
pub fn layers() -> Vec<(u32, u32, Vec<u8>)> {
    const S: u32 = LAYER_SIZE;
    let mut out = Vec::with_capacity(PALETTE.len());
    for (li, (_, base)) in PALETTE.iter().enumerate() {
        let mut rgba = Vec::with_capacity((S * S * 4) as usize);
        // An 8x8 lattice of random values + bilinear interpolation:
        // smooth blobs without square block borders.
        let mut grid = [[0i16; 9]; 9];
        for (gy, row) in grid.iter_mut().enumerate() {
            for (gx, v) in row.iter_mut().enumerate() {
                *v = (hash2(gx as u32, gy as u32, li as u32) % 9) as i16 - 4;
            }
        }
        for y in 0..S {
            for x in 0..S {
                let fx = x as f32 / S as f32 * 8.0;
                let fy = y as f32 / S as f32 * 8.0;
                let (gx, gy) = (fx.floor() as usize, fy.floor() as usize);
                let (tx, ty) = (fx - gx as f32, fy - gy as f32);
                let sx = tx * tx * (3.0 - 2.0 * tx);
                let sy = ty * ty * (3.0 - 2.0 * ty);
                let blotch = grid[gy][gx] as f32 * (1.0 - sx) * (1.0 - sy)
                    + grid[gy][gx + 1] as f32 * sx * (1.0 - sy)
                    + grid[gy + 1][gx] as f32 * (1.0 - sx) * sy
                    + grid[gy + 1][gx + 1] as f32 * sx * sy;
                let grain = (hash2(x, y, 77 + li as u32) % 5) as i16 - 2;
                let d = blotch as i16 + grain;
                let detail = if li == 1 { grass_detail(x, y) } else { [d; 3] };
                for c in 0..3 {
                    rgba.push((base[c] as i16 + detail[c]).clamp(0, 255) as u8);
                }
                rgba.push(255);
            }
        }
        out.push((S, S, rgba));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a 64 of the bytes of a layer.
    fn fnv(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
    }

    /// The bytes of the layers are the ones the generator gave before it moved here
    /// from `classic-content` (the hashes were taken from the old code): the classic module does
    /// not change by a pixel.
    #[test]
    fn layer_bytes_are_unchanged() {
        const HASHES: [u64; 6] = [
            0xefcbdac1f70c1a18, // plains
            0x543647aa97743784, // grass
            0xfcf5e7432c5bfaeb, // hills
            0x084b8bd15d3f8926, // desert
            0x9f413224be28161c, // snow
            0xc5999d45fa53ea11, // mountain
        ];
        let set = layers();
        assert_eq!(set.len(), HASHES.len());
        for (i, (_, _, rgba)) in set.iter().enumerate() {
            assert_eq!(fnv(rgba), HASHES[i], "layer {}", PALETTE[i].0);
        }
    }

    #[test]
    fn layers_decode() {
        let set = layers();
        assert_eq!(set.len(), 6);
        let (w, h, _) = set[0];
        assert!(w >= 64 && w == h);
        for (lw, lh, rgba) in &set {
            assert_eq!((*lw, *lh), (w, h));
            assert_eq!(rgba.len(), (w * h * 4) as usize);
        }
        // Each byte is near the palette base (flat + blotches), alpha is 255 everywhere:
        // catches stubs and a photo that slipped in.
        for ((_, _, rgba), (label, base)) in set.iter().zip(PALETTE.iter()) {
            for i in 0..(w * h) as usize {
                for (c, b) in base.iter().enumerate() {
                    let v = i16::from(rgba[i * 4 + c]);
                    assert!(
                        (v - i16::from(*b)).abs() <= if *label == "grass" { 36 } else { 6 },
                        "layer {label} pixel {i} ch {c}: {v} vs base {b}",
                    );
                }
                assert_eq!(rgba[i * 4 + 3], 255, "alpha");
            }
        }
    }
}
