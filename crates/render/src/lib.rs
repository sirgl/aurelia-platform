//! render: RenderWorld (CPU) + GPU (wgpu). Not authoritative (§30-33).
//! gpu/ works only with RenderWorld/assets, reconstructable after device loss.

pub mod camera;
pub mod debug_view;
pub mod gpu;
pub mod ground;
pub mod model_data;
pub mod models;
pub mod renderer;
pub mod scene;
pub mod unit_models;
pub mod world_models;
pub mod world;

pub use camera::CameraController;
pub use renderer::{FrameFlags, RenderStats, Renderer};
pub use world::state::RenderWorld;
