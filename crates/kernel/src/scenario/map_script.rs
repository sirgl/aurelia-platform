//! Map scripts of a new game (continents, Pangaea, archipelago,
//! fractal, small continents), in spirit:
//! where the land lies is a shape mask of elliptic blobs over the same
//! value noise every script uses, and the sea level takes the lowest share
//! of the result as an exact quota (`landscape::land_shaped`).
//!
//! | Script | Land masses | Water at medium sea level |
//! |---|---|---|
//! | Fractal | noise only (the original generator) | the chosen share |
//! | Pangaea | one central mass with two lobes | +30 % |
//! | Continents | two masses, west and east | +35 % |
//! | Small continents | 4–8 masses on a jittered grid | +40 % |
//! | Archipelago | many small islands on a jittered grid | +45 % |
//!
//! Integer arithmetic over the seed only: a map reproduces bit for bit.

use super::landscape::field;

/// The shape of the land of a generated map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MapScript {
    /// Noise only: the generator before map scripts existed.
    #[default]
    Fractal,
    Continents,
    Pangaea,
    Archipelago,
    SmallContinents,
}

/// Upper bound of the water share any script asks for.
const MAX_WATER: u8 = 85;

/// Full scale of the noise and of the mask (2^20).
const SCALE: u64 = 1 << 20;

impl MapScript {
    pub const ALL: [MapScript; 5] = [
        MapScript::Continents,
        MapScript::Pangaea,
        MapScript::Archipelago,
        MapScript::Fractal,
        MapScript::SmallContinents,
    ];

    /// Stable name (setup screen, settings, saves of the setup).
    pub fn key(self) -> &'static str {
        match self {
            MapScript::Fractal => "fractal",
            MapScript::Continents => "continents",
            MapScript::Pangaea => "pangaea",
            MapScript::Archipelago => "archipelago",
            MapScript::SmallContinents => "small-continents",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        MapScript::ALL.into_iter().find(|s| s.key() == key)
    }

    /// Water share in percent for a sea level of `sea_level` percent (the
    /// share Fractal uses as is): ocean scripts keep more water around
    /// their land masses.
    pub fn water_ratio(self, sea_level: u8) -> u8 {
        let extra = match self {
            MapScript::Fractal => 0,
            MapScript::Pangaea => 30,
            MapScript::Continents => 35,
            MapScript::SmallContinents => 40,
            MapScript::Archipelago => 45,
        };
        sea_level.saturating_add(extra).min(MAX_WATER)
    }

    /// Elevation of every tile, row-major: the order the sea level takes
    /// water from (lowest first) and the slope rivers follow.
    pub fn elevation(self, width: u32, height: u32, seed: u64) -> Vec<u32> {
        let tiles = width as u64 * height as u64;
        let noise = |i: u64| field(i % width as u64, i / width as u64, 5, 0xC0FFEE, seed) as u64;
        if self == MapScript::Fractal {
            return (0..tiles).map(|i| noise(i) as u32).collect();
        }
        let blobs = self.blobs(width, height, seed);
        // The mask decides where land lies; the noise only frays the outlines
        // (outside every blob the elevation stays below a quarter scale).
        let (mask_weight, noise_weight) = (3, 1);
        (0..tiles)
            .map(|i| {
                let (q, r) = (i % width as u64, i / width as u64);
                let mask = mask_at(&blobs, q, r);
                ((mask * mask_weight + noise(i) * noise_weight) / (mask_weight + noise_weight)) as u32
            })
            .collect()
    }

    fn blobs(self, width: u32, height: u32, seed: u64) -> Vec<Blob> {
        let (w, h) = (width as i64 * 16, height as i64 * 16);
        let jitter = |salt: u64, span: i64| -> i64 {
            if span <= 0 {
                return 0;
            }
            (hash(salt, seed) % (2 * span as u64 + 1)) as i64 - span
        };
        match self {
            MapScript::Fractal => Vec::new(),
            MapScript::Pangaea => {
                let (cx, cy) = (w / 2 + jitter(1, w / 25), h / 2 + jitter(2, h / 25));
                vec![
                    Blob::new(cx, cy, w * 30 / 100, h * 34 / 100),
                    Blob::new(cx - w * 16 / 100, cy + jitter(3, h / 8), w * 20 / 100, h * 26 / 100),
                    Blob::new(cx + w * 16 / 100, cy + jitter(4, h / 8), w * 20 / 100, h * 26 / 100),
                ]
            }
            MapScript::Continents => (0..2)
                .map(|side| {
                    let cx = w * (25 + 50 * side) / 100 + jitter(10 + side as u64, w / 50);
                    let cy = h / 2 + jitter(20 + side as u64, h / 12);
                    Blob::new(cx, cy, w * 19 / 100, h * 40 / 100)
                })
                .collect(),
            MapScript::SmallContinents => grid_blobs(width, height, seed, (width * height / 500).clamp(4, 8), 40),
            MapScript::Archipelago => grid_blobs(width, height, seed, (width * height / 60).clamp(8, 200), 38),
        }
    }
}

/// `count` blobs on a jittered grid covering the map, each with radii of
/// `fill_pct` percent of its cell.
fn grid_blobs(width: u32, height: u32, seed: u64, count: u32, fill_pct: i64) -> Vec<Blob> {
    let (w, h) = (width as i64 * 16, height as i64 * 16);
    let mut columns = 1;
    while columns * columns * height < count * width {
        columns += 1;
    }
    let rows = count.div_ceil(columns);
    let (cell_w, cell_h) = (w / columns as i64, h / rows as i64);
    let mut blobs = Vec::with_capacity(count as usize);
    for n in 0..count as i64 {
        let (col, row) = (n % columns as i64, n / columns as i64);
        let jx = (hash(200 + 2 * n as u64, seed) % (cell_w as u64 / 8 + 1)) as i64 - cell_w / 16;
        let jy = (hash(201 + 2 * n as u64, seed) % (cell_h as u64 / 8 + 1)) as i64 - cell_h / 16;
        let cx = cell_w * col + cell_w / 2 + jx;
        let cy = cell_h * row + cell_h / 2 + jy;
        blobs.push(Blob::new(cx, cy, cell_w * fill_pct / 100, cell_h * fill_pct / 100));
    }
    blobs
}

/// An elliptic land mass in sixteenths of a tile.
#[derive(Debug, Clone, Copy)]
struct Blob {
    cx: i64,
    cy: i64,
    rx: i64,
    ry: i64,
}

impl Blob {
    fn new(cx: i64, cy: i64, rx: i64, ry: i64) -> Self {
        Self { cx, cy, rx: rx.max(16), ry: ry.max(16) }
    }

    /// SCALE at the centre falling to 0 at the outline (squared distance).
    fn strength(&self, x: i64, y: i64) -> u64 {
        let (dx, dy) = (x - self.cx, y - self.cy);
        let d2 = (dx * dx) as u64 * SCALE / (self.rx * self.rx) as u64
            + (dy * dy) as u64 * SCALE / (self.ry * self.ry) as u64;
        SCALE.saturating_sub(d2)
    }
}

/// The strongest blob at a tile (odd rows sit half a tile to the east).
fn mask_at(blobs: &[Blob], q: u64, r: u64) -> u64 {
    let x = q as i64 * 16 + (r as i64 & 1) * 8 + 8;
    let y = r as i64 * 16 + 8;
    blobs.iter().map(|b| b.strength(x, y)).max().unwrap_or(0)
}

fn hash(salt: u64, seed: u64) -> u64 {
    let mut z = salt.wrapping_mul(0x9E3779B97F4A7C15) ^ seed.rotate_left(23) ^ 0x5C21_7A9E;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_water_is_capped() {
        for script in MapScript::ALL {
            assert_eq!(MapScript::from_key(script.key()), Some(script));
        }
        assert_eq!(MapScript::Fractal.water_ratio(25), 25);
        assert_eq!(MapScript::Archipelago.water_ratio(60), MAX_WATER);
    }

    #[test]
    fn fractal_elevation_is_the_plain_noise() {
        let elevation = MapScript::Fractal.elevation(10, 6, 7);
        assert_eq!(elevation[13], field(3, 1, 5, 0xC0FFEE, 7));
    }

    #[test]
    fn shapes_are_deterministic_and_differ_by_seed() {
        for script in MapScript::ALL {
            let a = script.elevation(40, 24, 11);
            assert_eq!(a, script.elevation(40, 24, 11), "{script:?}");
            assert_ne!(a, script.elevation(40, 24, 12), "{script:?}");
        }
    }
}
