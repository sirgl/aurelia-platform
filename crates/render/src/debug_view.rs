//! Switches of the developer gallery (`integration/crates/gallery`): a camera turned about the vertical axis
//! and the shadow map switched off. Both are process-wide and off by default, so the game
//! draws exactly what it drew before; only the gallery sets them.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// Camera yaw in thousandths of a degree.
static YAW_MILLI: AtomicI32 = AtomicI32::new(0);
static SHADOWS_OFF: AtomicBool = AtomicBool::new(false);

/// Turns the camera about the view centre (degrees, 0: the game's view from the south).
pub fn set_yaw(degrees: f32) {
    YAW_MILLI.store(
        (degrees.rem_euclid(360.0) * 1000.0) as i32,
        Ordering::Relaxed,
    );
}

/// The camera yaw in radians (0 in the game).
pub fn yaw() -> f32 {
    (YAW_MILLI.load(Ordering::Relaxed) as f32 / 1000.0).to_radians()
}

/// Draws no shadows (the shadow map stays empty).
pub fn set_shadows_enabled(on: bool) {
    SHADOWS_OFF.store(!on, Ordering::Relaxed);
}

pub fn shadows_enabled() -> bool {
    !SHADOWS_OFF.load(Ordering::Relaxed)
}

// ---- A synthetic relief for the slope probes (gallery "Slopes" view, `desktop --gallery-slopes`).

/// Shape of the synthetic relief laid over the generated ground.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReliefShape {
    /// One plane tilted by `degrees` (downhill toward `azimuth`).
    Plane,
    /// A ridge: the ground falls off by `degrees` to both sides of the line through the origin.
    Ridge,
    /// A valley: the same, rising to both sides.
    Valley,
    /// A cliff step: a face of `degrees` and `STEP_HEIGHT` high across the line through the origin.
    Step,
}

/// A relief added to the ground height (stage 4 of the surface field), around `origin`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TestRelief {
    pub shape: ReliefShape,
    pub degrees: f32,
    /// Direction of the slope in the ground plane, radians (0: toward +x).
    pub azimuth: f32,
    pub origin: [f32; 2],
}

/// Height of the step of `ReliefShape::Step`.
pub const STEP_HEIGHT: f32 = 0.6;
/// The relief never goes beyond this height either way.
const RELIEF_LIMIT: f32 = 6.0;

static RELIEF: std::sync::RwLock<Option<TestRelief>> = std::sync::RwLock::new(None);

/// Sets (or clears) the synthetic relief. Process-wide; the game never sets it.
pub fn set_relief(relief: Option<TestRelief>) {
    if let Ok(mut slot) = RELIEF.write() {
        *slot = relief;
    }
}

pub fn relief() -> Option<TestRelief> {
    RELIEF.read().ok().and_then(|r| *r)
}

impl TestRelief {
    /// Signed distance from the origin along the slope direction.
    fn along(&self, x: f32, z: f32) -> f32 {
        (x - self.origin[0]) * self.azimuth.cos() + (z - self.origin[1]) * self.azimuth.sin()
    }

    /// The height added at (x, z).
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let tan = self.degrees.to_radians().tan();
        let u = self.along(x, z);
        let y = match self.shape {
            // Downhill toward the azimuth: the ground falls as u grows.
            ReliefShape::Plane => -tan * u,
            ReliefShape::Ridge => -tan * u.abs(),
            ReliefShape::Valley => tan * u.abs(),
            ReliefShape::Step => {
                let width = STEP_HEIGHT / tan.max(1e-3);
                let t = ((u + width / 2.0) / width).clamp(0.0, 1.0);
                -STEP_HEIGHT * t
            }
        };
        y.clamp(-RELIEF_LIMIT, RELIEF_LIMIT)
    }

    /// The edge length the mesh needs at (x, z): fine where the relief bends.
    pub fn edge_length(&self, x: f32, z: f32) -> Option<f32> {
        let u = self.along(x, z);
        match self.shape {
            ReliefShape::Plane => None,
            ReliefShape::Ridge | ReliefShape::Valley => (u.abs() < 0.6).then_some(0.12),
            ReliefShape::Step => {
                let width = STEP_HEIGHT / self.degrees.to_radians().tan().max(1e-3);
                (u.abs() < width / 2.0 + 0.5).then_some(0.1)
            }
        }
    }

    /// The relief as words for region fingerprints.
    pub fn words(&self) -> [u32; 5] {
        [self.shape as u32, self.degrees.to_bits(), self.azimuth.to_bits(), self.origin[0].to_bits(), self.origin[1].to_bits()]
    }
}

// ---- The slope mode: the old object seating or the new one with terrain pads (platform/docs/slopes.md).

static SLOPE_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// True when objects on uneven ground use the pads and the new seating (default); the
/// environment variable `AURELIA_SLOPE_MODE=old` keeps the previous behaviour for comparison.
pub fn slope_mode_new() -> bool {
    match SLOPE_MODE.load(Ordering::Relaxed) {
        0 => {
            let old = std::env::var("AURELIA_SLOPE_MODE").is_ok_and(|v| v == "old");
            SLOPE_MODE.store(if old { 1 } else { 2 }, Ordering::Relaxed);
            !old
        }
        m => m == 2,
    }
}

/// Sets the slope mode (the gallery key, the probes).
pub fn set_slope_mode_new(new: bool) {
    SLOPE_MODE.store(if new { 2 } else { 1 }, Ordering::Relaxed);
}
