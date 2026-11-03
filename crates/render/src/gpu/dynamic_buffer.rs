//! A GPU vertex buffer that keeps its allocation and grows by powers of two.
use super::context::GpuCore;
use super::memory::{Category, GpuBuffer};

pub struct DynamicBuffer {
    label: &'static str,
    category: Category,
    usage: wgpu::BufferUsages,
    buffer: GpuBuffer,
    capacity: u64,
    /// Elements written by the last `write`.
    pub len: u32,
}

impl DynamicBuffer {
    pub fn new(core: &GpuCore, category: Category, label: &'static str, usage: wgpu::BufferUsages) -> Self {
        let usage = usage | wgpu::BufferUsages::COPY_DST;
        let capacity = 4096;
        let mut buffer = core.buffer(
            category,
            &wgpu::BufferDescriptor {
                label: Some(label),
                size: capacity,
                usage,
                mapped_at_creation: false,
            },
        );
        buffer.set_used(0);
        Self { label, category, usage, buffer, capacity, len: 0 }
    }

    pub fn vertex(core: &GpuCore, category: Category, label: &'static str) -> Self {
        Self::new(core, category, label, wgpu::BufferUsages::VERTEX)
    }

    /// Replaces the contents; returns the bytes uploaded.
    pub fn write<T: bytemuck::Pod>(&mut self, core: &GpuCore, data: &[T]) -> u64 {
        self.len = data.len() as u32;
        let bytes = std::mem::size_of_val(data) as u64;
        if bytes == 0 {
            self.buffer.set_used(0);
            return 0;
        }
        if bytes > self.capacity {
            self.capacity = bytes.next_power_of_two();
            self.buffer = core.buffer(
                self.category,
                &wgpu::BufferDescriptor {
                    label: Some(self.label),
                    size: self.capacity,
                    usage: self.usage,
                    mapped_at_creation: false,
                },
            );
        }
        self.buffer.set_used(bytes);
        core.queue
            .write_buffer(&self.buffer, 0, bytemuck::cast_slice(data));
        bytes
    }

    /// Makes the buffer hold `data` when only the elements in `changed` differ from the
    /// last contents; writes just those (everything after a resize or a length change).
    /// Returns the bytes uploaded.
    pub fn write_range<T: bytemuck::Pod>(&mut self, core: &GpuCore, data: &[T], changed: std::ops::Range<usize>) -> u64 {
        let bytes = std::mem::size_of_val(data) as u64;
        if data.len() as u32 != self.len || bytes > self.capacity {
            return self.write(core, data);
        }
        let changed = changed.start.min(data.len())..changed.end.min(data.len());
        if changed.is_empty() {
            return 0;
        }
        let offset = (changed.start * std::mem::size_of::<T>()) as u64;
        let slice = &data[changed];
        core.queue.write_buffer(&self.buffer, offset, bytemuck::cast_slice(slice));
        std::mem::size_of_val(slice) as u64
    }

    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub fn slice(&self) -> wgpu::BufferSlice<'_> {
        self.buffer.slice(..)
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}
