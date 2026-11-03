//! CPU memory of the process, for the headless benches. Two views:
//!
//! - the heap the game itself holds: live and peak bytes, counted by `CountingAllocator`
//!   when the binary installs it as the global allocator (desktop feature `heap-stats`);
//! - what macOS charges the process (`phys_footprint`, the number Activity Monitor
//!   shows). On Apple GPUs it includes Metal memory, so it is read next to the GPU ledger
//!   (`render::gpu::memory`); malloc's free pages it still keeps are counted too.
//!
//! Reading the footprint: the Metal driver alone takes about 390 MiB on an M4 Max at the
//! first render pass and blit of any process (a 64x64 clear in plain Metal shows it),
//! and marks it purgeable a few seconds after the GPU goes idle. So a rendering game
//! shows ledger + heap + ~390 MiB, and less once it rests.
use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
static ALLOCATED: AtomicU64 = AtomicU64::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// The system allocator plus live, peak and allocated-so-far byte counters.
pub struct CountingAllocator;

// SAFETY: every call goes to `System` unchanged; only counters are added.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
            grow(new_size);
        }
        new
    }
}

fn grow(bytes: usize) {
    INSTALLED.store(true, Ordering::Relaxed);
    ALLOCATED.fetch_add(bytes as u64, Ordering::Relaxed);
    let live = LIVE.fetch_add(bytes as u64, Ordering::Relaxed) + bytes as u64;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

/// Heap bytes the game holds now, the most it held since the last `reset_heap_peak`, and
/// every byte allocated so far (a reallocation counts its new size).
#[derive(Debug, Clone, Copy)]
pub struct Heap {
    pub live: u64,
    pub peak: u64,
    pub allocated: u64,
}

/// None when `CountingAllocator` is not the global allocator.
pub fn heap() -> Option<Heap> {
    INSTALLED.load(Ordering::Relaxed).then(|| Heap {
        live: LIVE.load(Ordering::Relaxed),
        peak: PEAK.load(Ordering::Relaxed),
        allocated: ALLOCATED.load(Ordering::Relaxed),
    })
}

/// Starts a new peak window at the live bytes.
pub fn reset_heap_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// What the OS charges the process, in bytes.
#[derive(Debug, Clone, Copy)]
pub struct ProcessMemory {
    /// `phys_footprint`: dirty memory, GPU memory on unified-memory Macs, compressed pages.
    pub footprint: u64,
    pub footprint_peak: u64,
    /// Bytes in malloc blocks in use, and what malloc's zones hold for them (in use plus free).
    pub malloc_in_use: u64,
    pub malloc_held: u64,
}

#[cfg(target_vendor = "apple")]
pub fn process_memory() -> Option<ProcessMemory> {
    // SAFETY: both calls only fill the zeroed structs passed to them.
    let (usage, malloc) = unsafe {
        let mut usage: libc::rusage_info_v4 = std::mem::zeroed();
        let status = libc::proc_pid_rusage(
            std::process::id() as i32,
            libc::RUSAGE_INFO_V4,
            (&mut usage as *mut libc::rusage_info_v4).cast(),
        );
        if status != 0 {
            return None;
        }
        let mut malloc: libc::malloc_statistics_t = std::mem::zeroed();
        libc::malloc_zone_statistics(std::ptr::null_mut(), &mut malloc);
        (usage, malloc)
    };
    Some(ProcessMemory {
        footprint: usage.ri_phys_footprint,
        footprint_peak: usage.ri_lifetime_max_phys_footprint,
        malloc_in_use: malloc.size_in_use as u64,
        malloc_held: malloc.size_allocated as u64,
    })
}

#[cfg(not(target_vendor = "apple"))]
pub fn process_memory() -> Option<ProcessMemory> {
    None
}

/// One line of a bench report: the process now, then the counted heap when installed.
pub struct MemoryLine;

impl fmt::Display for MemoryLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mib = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
        if let Some(p) = process_memory() {
            write!(
                f,
                "footprint {:.0} MiB (peak {:.0}), malloc in use {:.0} of {:.0} MiB held",
                mib(p.footprint),
                mib(p.footprint_peak),
                mib(p.malloc_in_use),
                mib(p.malloc_held)
            )?;
        }
        if let Some(h) = heap() {
            write!(f, ", heap live {:.1} MiB (peak {:.1}, allocated so far {:.0})", mib(h.live), mib(h.peak), mib(h.allocated))?;
        }
        Ok(())
    }
}
