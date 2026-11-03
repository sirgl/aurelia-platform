//! Pointer-to-tile hover: when it is allowed and a cache that re-picks only when the
//! pointer, viewport, camera or world revision changed.
use render::{camera::CameraController, world::state::RenderWorld};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoverKey {
    pointer: [u32; 2],
    viewport: (u32, u32),
    camera: [u32; 3],
    revision: u64,
    dimensions: (u32, u32),
}
impl HoverKey {
    pub fn new(
        pointer: egui::Pos2,
        scale: f32,
        viewport: (u32, u32),
        camera: &CameraController,
        world: &RenderWorld,
    ) -> Self {
        Self {
            pointer: [(pointer.x * scale).to_bits(), (pointer.y * scale).to_bits()],
            viewport,
            camera: [
                camera.x.to_bits(),
                camera.y.to_bits(),
                camera.zoom.to_bits(),
            ],
            revision: world.revision(),
            dimensions: (world.width, world.height),
        }
    }
    pub fn physical_pointer(self) -> (f32, f32) {
        (
            f32::from_bits(self.pointer[0]),
            f32::from_bits(self.pointer[1]),
        )
    }
}
#[derive(Default)]
pub struct HoverCache {
    entry: Option<(HoverKey, Option<u32>)>,
}
impl HoverCache {
    pub fn resolve(
        &mut self,
        key: Option<HoverKey>,
        pick: impl FnOnce(HoverKey) -> Option<u32>,
    ) -> Option<u32> {
        let key = key?;
        if let Some((previous, tile)) = self.entry {
            if previous == key {
                return tile;
            }
        }
        let tile = pick(key);
        self.entry = Some((key, tile));
        tile
    }
}
pub fn hover_allowed(
    ctx: &egui::Context,
    screen: crate::shell::module::Screen,
    dragging: bool,
    badge: bool,
) -> bool {
    screen == crate::shell::module::Screen::Map
        && ctx.input(|i| i.focused)
        && !dragging
        && !badge
        && ctx.pointer_hover_pos().is_some()
        && !ctx.input(|i| i.pointer.any_down())
        && !ctx.is_pointer_over_egui()
        && !ctx.egui_is_using_pointer()
}

#[cfg(test)]
mod hover_tests {
    use super::*;
    #[test]
    fn physical_coordinates_and_pick_cache_track_only_geometry_inputs() {
        let mut cache = HoverCache::default();
        let camera = CameraController::default();
        let mut world = RenderWorld {
            width: 32,
            height: 24,
            layers: render::world::state::LayerRevisions::INITIAL,
            ..Default::default()
        };
        let key = HoverKey::new(egui::pos2(200., 100.), 2., (1600, 900), &camera, &world);
        assert_eq!(key.physical_pointer(), (400., 200.));
        let mut calls = 0;
        assert_eq!(
            cache.resolve(Some(key), |_| {
                calls += 1;
                Some(22)
            }),
            Some(22)
        );
        for _ in 0..100 {
            assert_eq!(
                cache.resolve(Some(key), |_| {
                    calls += 1;
                    None
                }),
                Some(22)
            );
        }
        assert_eq!(calls, 1);
        assert_eq!(
            cache.resolve(None, |_| {
                calls += 1;
                None
            }),
            None
        );
        assert_eq!(calls, 1);
        world.touch_all();
        let changed = HoverKey::new(egui::pos2(200., 100.), 2., (1600, 900), &camera, &world);
        assert_eq!(
            cache.resolve(Some(changed), |_| {
                calls += 1;
                None
            }),
            None
        );
        assert_eq!(calls, 2);
        let mut moved = camera.clone();
        moved.zoom = 2.;
        assert_ne!(
            changed,
            HoverKey::new(egui::pos2(200., 100.), 2., (1600, 900), &moved, &world)
        );
        assert_ne!(
            changed,
            HoverKey::new(egui::pos2(200., 100.), 2., (1200, 900), &camera, &world)
        );
    }
    #[test]
    fn losing_focus_hides_hover_even_when_pointer_position_remains() {
        let ctx = egui::Context::default();
        let mut first = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(egui::pos2(400., 300.))],
                ..Default::default()
            },
            |_| {},
        );
        first.textures_delta.clear();
        let mut output = ctx.run_ui(
            egui::RawInput {
                focused: false,
                events: vec![egui::Event::WindowFocused(false)],
                ..Default::default()
            },
            |ui| {
                assert!(ui.ctx().pointer_hover_pos().is_some());
                assert!(!hover_allowed(
                    ui.ctx(),
                    crate::shell::module::Screen::Map,
                    false,
                    false
                ));
            },
        );
        output.textures_delta.clear();
    }
}
