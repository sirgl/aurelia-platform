//! Camera: a platform-independent controller (pan/zoom) + a view-proj matrix.
//! The GPU knows nothing about input; desktop maps events to pan_by/zoom_by.

/// Camera pitch override in thousandths of a degree (0: the game's 45 degrees). Only captures set
/// it (AURELIA_GAME_PITCH of the offscreen runs): a low angle shows how objects meet the ground.
static PITCH_MILLI: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Looks at the ground from `degrees` above the horizon (None: the game's 45); the eye keeps its
/// distance from the view centre.
pub fn set_pitch_override(degrees: Option<f32>) {
    let milli = degrees.map_or(0, |d| (d.clamp(5.0, 89.0) * 1000.0) as u32);
    PITCH_MILLI.store(milli, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Debug, Clone)]
pub struct CameraController {
    /// View centre in world units (x is east, y is south on the map).
    pub x: f32,
    pub y: f32,
    /// Distance: more = farther (smaller). Initially 1.0.
    pub zoom: f32,
}

impl Default for CameraController {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

impl CameraController {
    /// Centre on a map of w x h tiles (hex layout pointy-top).
    pub fn fit_map(width: u32, height: u32) -> Self {
        let (cx, cz) = crate::scene::layout::map_center(width, height);
        Self {
            x: cx,
            y: cz,
            zoom: 1.0,
        }
    }

    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        // Pan speed scales with distance (natural at any zoom).
        let k = self.distance() * 0.0016;
        self.x -= dx * k;
        self.y -= dy * k;
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(0.15, 8.0);
    }

    pub fn distance(&self) -> f32 {
        42.0 / self.zoom.max(0.05)
    }

    /// Screen pixels per hex unit at the view centre for a viewport `height` px tall
    /// (unit LOD selection: projected model size, R-07 section 8).
    pub fn pixels_per_unit(&self, height: f32) -> f32 {
        let eye_distance = self.distance() * std::f32::consts::SQRT_2;
        height / (2.0 * eye_distance * (Self::FOV_DEGREES.to_radians() * 0.5).tan())
    }

    const FOV_DEGREES: f32 = 42.0;

    /// Row-major view-proj for the uniform buffer (glam -> cols array).
    /// The directx convention (depth 0..1) is what wgpu/Metal expect.
    pub fn view_proj(&self, aspect: f32) -> [[f32; 4]; 4] {
        use glam::Vec3;
        let dist = self.distance();
        // An oblique top-down view: the eye is shifted south, looking at the centre.
        let milli = PITCH_MILLI.load(std::sync::atomic::Ordering::Relaxed);
        let eye = if milli == 0 {
            Vec3::new(self.x, dist, self.y + dist * 1.0)
        } else {
            let (sin, cos) = (milli as f32 / 1000.0).to_radians().sin_cos();
            let reach = dist * std::f32::consts::SQRT_2;
            Vec3::new(self.x, reach * sin, self.y + reach * cos)
        };
        let center = Vec3::new(self.x, 0.0, self.y);
        // The developer gallery turns the camera about the view centre; the game never does.
        let yaw = crate::debug_view::yaw();
        let eye = if yaw == 0.0 {
            eye
        } else {
            let (sin, cos) = yaw.sin_cos();
            let (dx, dz) = (eye.x - self.x, eye.z - self.y);
            Vec3::new(self.x + dx * cos + dz * sin, eye.y, self.y - dx * sin + dz * cos)
        };
        let view = glam::camera::rh::view::look_at_mat4(eye, center, Vec3::Y);
        let proj = glam::camera::rh::proj::directx::perspective(
            Self::FOV_DEGREES.to_radians(),
            aspect.max(0.01),
            0.5,
            4000.0,
        );
        (proj * view).to_cols_array_2d()
    }

    /// 6 frustum planes (normalised) for chunk cull.
    /// Extraction from matrix rows: left/right/bottom/top/near/far.
    pub fn frustum_planes(&self, aspect: f32) -> [glam::Vec4; 6] {
        let vp = glam::Mat4::from_cols_array_2d(&self.view_proj(aspect));
        let (r0, r1, r2, r3) = (vp.row(0), vp.row(1), vp.row(2), vp.row(3));
        let norm = |v: glam::Vec4| v / v.truncate().length().max(1e-6);
        [
            norm(r3 + r0),
            norm(r3 - r0),
            norm(r3 + r1),
            norm(r3 - r1),
            norm(r3 + r2),
            norm(r3 - r2),
        ]
    }

    /// XZ box (low, high corners) of the ground the view can show: where the four edge
    /// rays of the frustum cross the horizontal slab `heights` (lowest, highest ground).
    pub fn ground_footprint(&self, aspect: f32, heights: (f32, f32)) -> [[f32; 2]; 2] {
        let inverse = glam::Mat4::from_cols_array_2d(&self.view_proj(aspect)).inverse();
        let mut low = [f32::INFINITY; 2];
        let mut high = [f32::NEG_INFINITY; 2];
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let near = inverse.project_point3(glam::Vec3::new(x, y, 0.0));
            let far = inverse.project_point3(glam::Vec3::new(x, y, 1.0));
            for level in [heights.0, heights.1] {
                let t = if (far.y - near.y).abs() > 1e-6 { ((level - near.y) / (far.y - near.y)).clamp(0.0, 1.0) } else { 1.0 };
                let p = near.lerp(far, t);
                low = [low[0].min(p.x), low[1].min(p.z)];
                high = [high[0].max(p.x), high[1].max(p.z)];
            }
        }
        [low, high]
    }

    /// Whether a sphere (centre, radius) is visible. Conservative: on the boundary it is visible.
    pub fn sphere_visible(planes: &[glam::Vec4; 6], center: [f32; 3], radius: f32) -> bool {
        planes
            .iter()
            .all(|p| p.x * center[0] + p.y * center[1] + p.z * center[2] + p.w > -radius)
    }

    /// World point -> screen pixels (for screen-space banners).
    /// None: behind the camera (w <= 0) or outside the frame. The same matrix as the GPU.
    pub fn project_to_screen(
        &self,
        world: [f32; 3],
        aspect: f32,
        w: f32,
        h: f32,
    ) -> Option<(f32, f32)> {
        let vp = glam::Mat4::from_cols_array_2d(&self.view_proj(aspect));
        let clip = vp * glam::Vec4::new(world[0], world[1], world[2], 1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        if ndc.x < -1.0 || ndc.x > 1.0 || ndc.y < -1.0 || ndc.y > 1.0 {
            return None;
        }
        Some(((ndc.x + 1.0) * 0.5 * w, (1.0 - ndc.y) * 0.5 * h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_centers_map_and_rejects_behind() {
        let cam = CameraController::fit_map(64, 64);
        let (cx, cz) = crate::scene::layout::map_center(64, 64);
        // The map centre goes to the screen centre (the same matrix as the GPU).
        let (sx, sy) = cam
            .project_to_screen([cx, 0.0, cz], 1.0, 800.0, 600.0)
            .expect("center must project");
        assert!((sx - 400.0).abs() < 1.0, "sx={}", sx);
        assert!((sy - 300.0).abs() < 1.0, "sy={}", sy);
        // A point at the camera eye (w = 0) is None, not NaN in the UI.
        let dist = cam.distance();
        assert!(cam
            .project_to_screen([cam.x, dist, cam.y + dist * 1.0], 1.0, 800.0, 600.0)
            .is_none());
        // Far beyond the map: outside the frame.
        assert!(cam
            .project_to_screen([cx + 5000.0, 0.0, cz], 1.0, 800.0, 600.0)
            .is_none());
    }

    #[test]
    fn ground_footprint_holds_every_ground_point_on_screen() {
        let mut cam = CameraController::fit_map(64, 64);
        cam.zoom = 2.25;
        let [low, high] = cam.ground_footprint(16.0 / 9.0, (0.0, 1.0));
        let (w, h) = (1600.0, 900.0);
        let mut inside = 0;
        for i in 0..400 {
            let (x, z) = (low[0] - 20.0 + (i % 20) as f32 * 0.1 * (high[0] - low[0] + 40.0), low[1] - 20.0 + (i / 20) as f32 * 0.1 * (high[1] - low[1] + 40.0));
            for y in [0.0, 0.5, 1.0] {
                if cam.project_to_screen([x, y, z], w / h, w, h).is_some() {
                    inside += 1;
                    assert!(x >= low[0] && x <= high[0] && z >= low[1] && z <= high[1], "({x}, {z}) on screen, outside the footprint");
                }
            }
        }
        assert!(inside > 20);
        assert!(high[0] - low[0] < 120.0 && high[1] - low[1] < 60.0, "a game view sees a local area: {low:?}..{high:?}");
    }

    #[test]
    fn frustum_culls_far_sphere_keeps_center() {
        let cam = CameraController::fit_map(64, 64);
        let planes = cam.frustum_planes(1.0);
        let (cx, cz) = crate::scene::layout::map_center(64, 64);
        assert!(CameraController::sphere_visible(
            &planes,
            [cx, 0.0, cz],
            5.0
        ));
        assert!(!CameraController::sphere_visible(
            &planes,
            [cx + 5000.0, 0.0, cz],
            5.0
        ));
    }
}
