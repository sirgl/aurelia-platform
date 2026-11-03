//! Live GPU memory by category. wgpu's Metal backend reports no memory counters, so
//! every buffer and texture the game creates goes through `GpuCore::buffer` /
//! `GpuCore::texture`: the returned `GpuBuffer` / `GpuTexture` owns the wgpu object and
//! a ticket that adds its size to the ledger when created and takes it back when dropped.
//!
//! Next to the live bytes the ledger counts every byte ever created per category: the
//! churn of resources replaced instead of rewritten. Metal keeps freed resources for
//! reuse for a few seconds, so churn shows up in the process footprint (`client::perf`).
//!
//! Sizes are what was requested (buffer size; texture extent × block bytes × mips ×
//! layers × samples), not what the driver rounds them up to. Pools that over-allocate
//! also report the bytes their last write used (`GpuBuffer::set_used`); for every other
//! resource used equals size.
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// What a resource is for (one row of the report).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Ground vertices of the terrain regions (`terrain_gpu.rs`).
    TerrainVertices,
    /// Ground indices of the terrain regions.
    TerrainIndices,
    /// River and lake surfaces of the terrain regions.
    TerrainWater,
    /// Hex grid lines of the terrain regions.
    TerrainGrid,
    /// Trees, props and routes in feature chunks, static features, legacy decals.
    Features,
    /// Owner border bands.
    Borders,
    /// Baked unit and world model meshes (every LOD).
    Models,
    /// City piece, wonder and tile prop instances.
    WorldInstances,
    /// Unit figure instances (formations and posed units).
    UnitInstances,
    /// Selection ribbons and yield icons.
    Overlays,
    /// Fog of war tile states and the baked fog field.
    Fog,
    /// The baked parchment: seamless tile and low-frequency field (`parchment.rs`).
    Parchment,
    /// Shadow depth maps.
    Shadows,
    /// Terrain texture array, icon atlas, road mask.
    Materials,
    /// Depth, MSAA and capture targets.
    RenderTargets,
    /// Small uniform buffers.
    Uniforms,
    /// Transient readback buffers.
    Readback,
    /// egui textures.
    Ui,
}

impl Category {
    pub const ALL: [Category; 18] = [
        Category::TerrainVertices,
        Category::TerrainIndices,
        Category::TerrainWater,
        Category::TerrainGrid,
        Category::Features,
        Category::Borders,
        Category::Models,
        Category::WorldInstances,
        Category::UnitInstances,
        Category::Overlays,
        Category::Fog,
        Category::Parchment,
        Category::Shadows,
        Category::Materials,
        Category::RenderTargets,
        Category::Uniforms,
        Category::Readback,
        Category::Ui,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Category::TerrainVertices => "terrain vertices",
            Category::TerrainIndices => "terrain indices",
            Category::TerrainWater => "terrain water",
            Category::TerrainGrid => "terrain grid",
            Category::Features => "features",
            Category::Borders => "borders",
            Category::Models => "model meshes",
            Category::WorldInstances => "world instances",
            Category::UnitInstances => "unit instances",
            Category::Overlays => "overlays",
            Category::Fog => "fog",
            Category::Parchment => "parchment",
            Category::Shadows => "shadows",
            Category::Materials => "materials",
            Category::RenderTargets => "render targets",
            Category::Uniforms => "uniforms",
            Category::Readback => "readback",
            Category::Ui => "ui",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

const COUNT: usize = Category::ALL.len();

/// Live bytes, used bytes, resource count and bytes ever created per category.
#[derive(Default)]
pub struct GpuMemory {
    bytes: [AtomicU64; COUNT],
    used: [AtomicU64; COUNT],
    resources: [AtomicU64; COUNT],
    created: [AtomicU64; COUNT],
}

impl GpuMemory {
    /// A ticket for `bytes` of `category`, live until dropped.
    pub fn allocate(self: &Arc<Self>, category: Category, bytes: u64) -> Allocation {
        let i = category.index();
        self.bytes[i].fetch_add(bytes, Ordering::Relaxed);
        self.used[i].fetch_add(bytes, Ordering::Relaxed);
        self.resources[i].fetch_add(1, Ordering::Relaxed);
        self.created[i].fetch_add(bytes, Ordering::Relaxed);
        Allocation { memory: self.clone(), category, bytes, used: bytes }
    }

    pub fn report(&self) -> GpuMemoryReport {
        let rows = Category::ALL
            .iter()
            .map(|&category| {
                let i = category.index();
                MemoryRow {
                    category,
                    bytes: self.bytes[i].load(Ordering::Relaxed),
                    used: self.used[i].load(Ordering::Relaxed),
                    resources: self.resources[i].load(Ordering::Relaxed),
                    created: self.created[i].load(Ordering::Relaxed),
                }
            })
            .collect();
        GpuMemoryReport { rows }
    }
}

/// Bytes of one resource counted in the ledger while this lives.
pub struct Allocation {
    memory: Arc<GpuMemory>,
    category: Category,
    bytes: u64,
    used: u64,
}

impl Allocation {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Bytes of the allocation the last write filled (pools with growth slack).
    pub fn set_used(&mut self, used: u64) {
        let used = used.min(self.bytes);
        let counter = &self.memory.used[self.category.index()];
        counter.fetch_add(used, Ordering::Relaxed);
        counter.fetch_sub(self.used, Ordering::Relaxed);
        self.used = used;
    }
}

impl Drop for Allocation {
    fn drop(&mut self) {
        let i = self.category.index();
        self.memory.bytes[i].fetch_sub(self.bytes, Ordering::Relaxed);
        self.memory.used[i].fetch_sub(self.used, Ordering::Relaxed);
        self.memory.resources[i].fetch_sub(1, Ordering::Relaxed);
    }
}

/// A wgpu buffer counted in the ledger while it lives.
pub struct GpuBuffer {
    buffer: wgpu::Buffer,
    allocation: Allocation,
}

impl GpuBuffer {
    pub fn new(memory: &Arc<GpuMemory>, category: Category, buffer: wgpu::Buffer) -> Self {
        let allocation = memory.allocate(category, buffer.size());
        Self { buffer, allocation }
    }

    pub fn set_used(&mut self, used: u64) {
        self.allocation.set_used(used);
    }
}

impl std::ops::Deref for GpuBuffer {
    type Target = wgpu::Buffer;

    fn deref(&self) -> &wgpu::Buffer {
        &self.buffer
    }
}

/// A wgpu texture counted in the ledger while it lives. Views keep the GPU memory alive
/// after the texture is dropped, so keep this next to its views.
pub struct GpuTexture {
    texture: wgpu::Texture,
    _allocation: Allocation,
}

impl GpuTexture {
    pub fn new(memory: &Arc<GpuMemory>, category: Category, texture: wgpu::Texture, bytes: u64) -> Self {
        Self { texture, _allocation: memory.allocate(category, bytes) }
    }
}

impl std::ops::Deref for GpuTexture {
    type Target = wgpu::Texture;

    fn deref(&self) -> &wgpu::Texture {
        &self.texture
    }
}

/// Bytes of a texture: every mip level of every layer, times the samples. Depth24Plus
/// counts 4 bytes (Metal and Vulkan store it as 32-bit depth), Depth24PlusStencil8 8.
pub fn texture_bytes(desc: &wgpu::TextureDescriptor<'_>) -> u64 {
    let block = texel_block_bytes(desc.format);
    let (block_w, block_h) = desc.format.block_dimensions();
    let levels: u64 = (0..desc.mip_level_count)
        .filter_map(|level| desc.mip_level_size(level))
        .map(|size| {
            u64::from(size.width.div_ceil(block_w))
                * u64::from(size.height.div_ceil(block_h))
                * u64::from(size.depth_or_array_layers)
                * block
        })
        .sum();
    levels * u64::from(desc.sample_count)
}

/// Bytes of an existing texture (one made elsewhere, such as egui's).
pub fn texture_bytes_of(texture: &wgpu::Texture) -> u64 {
    texture_bytes(&wgpu::TextureDescriptor {
        label: None,
        size: texture.size(),
        mip_level_count: texture.mip_level_count(),
        sample_count: texture.sample_count(),
        dimension: texture.dimension(),
        format: texture.format(),
        usage: texture.usage(),
        view_formats: &[],
    })
}

fn texel_block_bytes(format: wgpu::TextureFormat) -> u64 {
    use wgpu::TextureAspect::{DepthOnly, StencilOnly};
    if let Some(bytes) = format.block_copy_size(None) {
        return u64::from(bytes);
    }
    match format {
        wgpu::TextureFormat::Depth24Plus => 4,
        wgpu::TextureFormat::Depth24PlusStencil8 => 8,
        _ => {
            let depth = format.block_copy_size(Some(DepthOnly)).unwrap_or(0);
            let stencil = format.block_copy_size(Some(StencilOnly)).unwrap_or(0);
            u64::from((depth + stencil).next_power_of_two())
        }
    }
}

/// One category of the report.
#[derive(Debug, Clone, Copy)]
pub struct MemoryRow {
    pub category: Category,
    pub bytes: u64,
    pub used: u64,
    pub resources: u64,
    /// Bytes of every resource of the category created so far, the live ones included.
    pub created: u64,
}

/// Live GPU memory at one moment, by category.
#[derive(Debug, Clone, Default)]
pub struct GpuMemoryReport {
    pub rows: Vec<MemoryRow>,
}

impl GpuMemoryReport {
    pub fn total(&self) -> u64 {
        self.rows.iter().map(|r| r.bytes).sum()
    }

    pub fn of(&self, category: Category) -> u64 {
        self.rows.iter().find(|r| r.category == category).map_or(0, |r| r.bytes)
    }
}

/// A table: category, MiB, share of the total, used MiB, resources, MiB created so far;
/// empty rows omitted.
impl fmt::Display for GpuMemoryReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const MIB: f64 = 1024.0 * 1024.0;
        let total = self.total();
        let share = |bytes: u64| if total == 0 { 0.0 } else { bytes as f64 * 100.0 / total as f64 };
        writeln!(f, "  {:<17} {:>10} {:>6} {:>10} {:>7} {:>12}", "gpu memory", "MiB", "%", "used MiB", "count", "created MiB")?;
        let mut rows: Vec<&MemoryRow> = self.rows.iter().filter(|r| r.resources > 0).collect();
        rows.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        for r in rows {
            writeln!(
                f,
                "  {:<17} {:>10.2} {:>6.1} {:>10.2} {:>7} {:>12.2}",
                r.category.name(),
                r.bytes as f64 / MIB,
                share(r.bytes),
                r.used as f64 / MIB,
                r.resources,
                r.created as f64 / MIB
            )?;
        }
        let used: u64 = self.rows.iter().map(|r| r.used).sum();
        let resources: u64 = self.rows.iter().map(|r| r.resources).sum();
        let created: u64 = self.rows.iter().map(|r| r.created).sum();
        let (total_mib, used_mib, created_mib) = (total as f64 / MIB, used as f64 / MIB, created as f64 / MIB);
        write!(f, "  {:<17} {:>10.2} {:>6.1} {:>10.2} {:>7} {:>12.2}", "total", total_mib, 100.0, used_mib, resources, created_mib)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc(format: wgpu::TextureFormat, size: (u32, u32, u32), mips: u32, samples: u32) -> wgpu::TextureDescriptor<'static> {
        wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: size.2 },
            mip_level_count: mips,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        }
    }

    #[test]
    fn texture_sizes_follow_format_mips_layers_and_samples() {
        use wgpu::TextureFormat as F;
        assert_eq!(texture_bytes(&desc(F::Depth32Float, (2048, 2048, 1), 1, 1)), 16 << 20);
        assert_eq!(texture_bytes(&desc(F::R8Unorm, (512, 512, 1), 1, 1)), 256 << 10);
        assert_eq!(texture_bytes(&desc(F::R16Float, (100, 60, 1), 1, 1)), 100 * 60 * 2);
        // Depth24Plus is 32-bit; 4x MSAA multiplies.
        assert_eq!(texture_bytes(&desc(F::Depth24Plus, (1280, 720, 1), 1, 4)), 1280 * 720 * 4 * 4);
        assert_eq!(texture_bytes(&desc(F::Depth24PlusStencil8, (16, 16, 1), 1, 1)), 16 * 16 * 8);
        // 4x4 RGBA8 with a full chain: 16 + 4 + 1 texels, 7 layers.
        assert_eq!(texture_bytes(&desc(F::Rgba8Unorm, (4, 4, 7), 3, 1)), (16 + 4 + 1) * 4 * 7);
        // Odd sizes halve down to 1: 5x3 -> 2x1 -> 1x1.
        assert_eq!(texture_bytes(&desc(F::Rgba8Unorm, (5, 3, 1), 3, 1)), (15 + 2 + 1) * 4);
        // BC1: 4x4 blocks of 8 bytes, partial blocks round up.
        assert_eq!(texture_bytes(&desc(F::Bc1RgbaUnorm, (6, 6, 1), 1, 1)), 4 * 8);
    }

    #[test]
    fn dropping_a_ticket_takes_its_bytes_back() {
        let memory = Arc::new(GpuMemory::default());
        let a = memory.allocate(Category::Features, 1000);
        let mut b = memory.allocate(Category::Features, 4096);
        let c = memory.allocate(Category::Shadows, 64);
        assert_eq!(memory.report().of(Category::Features), 5096);
        assert_eq!(memory.report().total(), 5160);
        b.set_used(100);
        let row = memory.report().rows[Category::Features.index()];
        assert_eq!((row.bytes, row.used, row.resources), (5096, 1100, 2));
        drop(b);
        let row = memory.report().rows[Category::Features.index()];
        assert_eq!((row.bytes, row.used, row.resources, row.created), (1000, 1000, 1, 5096));
        drop(a);
        drop(c);
        let report = memory.report();
        assert_eq!(report.total(), 0);
        assert!(report.rows.iter().all(|r| r.used == 0 && r.resources == 0));
    }
}
