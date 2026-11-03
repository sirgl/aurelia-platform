//! Device loss (§38 platform/structure.md): GPU state is disposable and fully
//! reconstructable from the CPU RenderWorld/assets. We recreate pipelines/buffers,
//! not the projection and content.
//!
//! wgpu 30: a frame is returned as the enum CurrentSurfaceTexture, not a Result.

/// Whether the surface needs reconfiguring after this frame outcome.
pub fn needs_reconfigure(frame: &wgpu::CurrentSurfaceTexture) -> bool {
    matches!(
        frame,
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost
    )
}

/// Skip the frame without reconfiguring (occlusion, timeout, validation).
pub fn should_skip(frame: &wgpu::CurrentSurfaceTexture) -> bool {
    matches!(
        frame,
        wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation
    )
}
