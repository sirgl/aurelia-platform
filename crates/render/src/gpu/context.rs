//! GPU core: instance/adapter/device/queue without a surface (headless-capable).
//! The surface belongs to the platform (desktop creates it from the window, §38 platform/structure.md):
//! `core.instance.create_surface(window)` + capabilities for the format.
//! classic-rules does not look in here: the dependency is only render -> wgpu.

use std::sync::Arc;

use super::memory::{texture_bytes, Category, GpuBuffer, GpuMemory, GpuTexture};

/// Everything for drawing without a window: tests, offscreen, future workers.
pub struct GpuCore {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    /// Live bytes of every buffer and texture made through `buffer` / `texture`.
    pub memory: Arc<GpuMemory>,
    /// Pass timers (`AURELIA_GPU_TIMING=1` opens the device with timestamps).
    pub timing: Option<super::timing::GpuTiming>,
    /// Transient attachments take no memory here (Apple GPUs: Metal memoryless).
    transient_is_free: bool,
}

impl GpuCore {
    /// A blocking constructor (pollster inside). The error is human-readable:
    /// no adapter = an environment without a GPU, not a render bug.
    pub fn new_blocking() -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("wgpu: no suitable adapter ({e:?}); GPU required"))?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| format!("wgpu: device request failed ({e:?})"))?;
        Ok(Self::from_parts(instance, adapter, device, queue))
    }

    pub fn from_parts(instance: wgpu::Instance, adapter: wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let transient_is_free = adapter.get_info().transient_saves_memory == Some(true);
        let timing = super::timing::GpuTiming::new(&device, &queue);
        Self { instance, adapter, device, queue, memory: Arc::default(), timing, transient_is_free }
    }

    /// A buffer counted in `memory` under `category` until dropped.
    pub fn buffer(&self, category: Category, desc: &wgpu::BufferDescriptor<'_>) -> GpuBuffer {
        GpuBuffer::new(&self.memory, category, self.device.create_buffer(desc))
    }

    /// Usage of an attachment that lives only inside one render pass (cleared at its
    /// start, never stored): transient where that saves memory.
    pub fn pass_attachment_usage(&self) -> wgpu::TextureUsages {
        let transient = if self.transient_is_free { wgpu::TextureUsages::TRANSIENT_ATTACHMENT } else { wgpu::TextureUsages::empty() };
        wgpu::TextureUsages::RENDER_ATTACHMENT | transient
    }

    /// A texture counted in `memory` under `category` until dropped (a transient
    /// attachment counts nothing where it has no memory).
    pub fn texture(&self, category: Category, desc: &wgpu::TextureDescriptor<'_>) -> GpuTexture {
        let memoryless = self.transient_is_free && desc.usage.contains(wgpu::TextureUsages::TRANSIENT_ATTACHMENT);
        let bytes = if memoryless { 0 } else { texture_bytes(desc) };
        GpuTexture::new(&self.memory, category, self.device.create_texture(desc), bytes)
    }

    /// Bytes the driver says the device holds (Metal `currentAllocatedSize`: every
    /// buffer, texture, heap and internal allocation, with the driver's rounding); None
    /// on other backends.
    pub fn driver_allocated_bytes(&self) -> Option<u64> {
        #[cfg(target_vendor = "apple")]
        {
            use objc2_metal::MTLDevice;
            // SAFETY: only reads a counter of the device; nothing is created or destroyed.
            let device = unsafe { self.device.as_hal::<wgpu::hal::api::Metal>() }?;
            Some(device.raw_device().currentAllocatedSize() as u64)
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            None
        }
    }

    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.adapter.get_info()
    }
}
