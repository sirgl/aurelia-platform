//! GPU time of the frame's passes, for the headless benches (`AURELIA_GPU_TIMING=1`).
//!
//! Each timed pass writes a begin and an end timestamp into one query set; the last
//! encoder of the frame resolves them, and after the frame is idle `read` returns the
//! milliseconds of every pass that ran. Off by default: the device is opened without the
//! timestamp feature and every call is a no-op.
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// A pass of the frame whose GPU time is measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The static shadow map (terrain, features and models in the lit area).
    ShadowStatic,
    /// The static map copied and the units drawn over it.
    ShadowUnits,
    /// The MSAA scene pass: terrain, models, water, features and overlays.
    Scene,
    /// egui over the scene.
    Ui,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::ShadowStatic, Stage::ShadowUnits, Stage::Scene, Stage::Ui];

    pub fn name(self) -> &'static str {
        match self {
            Stage::ShadowStatic => "GPU shadow map (static)",
            Stage::ShadowUnits => "GPU shadow map (units)",
            Stage::Scene => "GPU scene pass",
            Stage::Ui => "GPU egui pass",
        }
    }

    fn slot(self) -> u32 {
        self as u32 * 2
    }
}

const SLOTS: u32 = Stage::ALL.len() as u32 * 2;
const BYTES: u64 = SLOTS as u64 * 8;

/// The query set and its buffers; clones share them.
#[derive(Clone)]
pub struct GpuTiming {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period: f32,
    /// Stages written since the last `resolve`.
    written: Arc<AtomicU32>,
    /// Stages the last `resolve` covered.
    resolved: Arc<AtomicU32>,
}

impl GpuTiming {
    /// The timer when the device was opened with `TIMESTAMP_QUERY`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let set = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("pass timers"), ty: wgpu::QueryType::Timestamp, count: SLOTS });
        let buffer = |label, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: BYTES, usage, mapped_at_creation: false });
        Some(Self {
            set,
            resolve: buffer("pass timers resolve", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            read: buffer("pass timers read", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
            period: queue.get_timestamp_period(),
            written: Arc::default(),
            resolved: Arc::default(),
        })
    }

    /// The timestamps `stage`'s pass writes (its begin and end).
    pub fn writes(&self, stage: Stage) -> wgpu::RenderPassTimestampWrites<'_> {
        self.written.fetch_or(1 << stage as u32, Ordering::Relaxed);
        wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(stage.slot()),
            end_of_pass_write_index: Some(stage.slot() + 1),
        }
    }

    /// Copies the timestamps into the readable buffer; the frame's last encoder calls it.
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let stages = self.written.swap(0, Ordering::Relaxed);
        self.resolved.store(stages, Ordering::Relaxed);
        if stages == 0 {
            return;
        }
        encoder.resolve_query_set(&self.set, 0..SLOTS, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, BYTES);
    }

    /// Milliseconds of every stage the last resolved frame ran; call after the GPU is idle.
    pub fn read(&self, device: &wgpu::Device) -> Vec<(Stage, f64)> {
        let stages = self.resolved.load(Ordering::Relaxed);
        if stages == 0 {
            return Vec::new();
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        self.read.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if !matches!(receiver.recv(), Ok(Ok(()))) {
            return Vec::new();
        }
        let mut out = Vec::new();
        {
            let Ok(data) = self.read.slice(..).get_mapped_range() else {
                return Vec::new();
            };
            let ticks: &[u64] = bytemuck::cast_slice(&data);
            for stage in Stage::ALL {
                if stages & (1 << stage as u32) != 0 {
                    let (begin, end) = (ticks[stage.slot() as usize], ticks[stage.slot() as usize + 1]);
                    out.push((stage, end.saturating_sub(begin) as f64 * f64::from(self.period) / 1e6));
                }
            }
        }
        self.read.unmap();
        out
    }
}
