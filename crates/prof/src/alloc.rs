//! A counting global allocator: install it in a binary with
//! `#[global_allocator] static A: prof::alloc::CountingAlloc = prof::alloc::CountingAlloc;`.
//!
//! Counts per thread (allocation calls and requested bytes; a reallocation counts as one
//! call of its new size, as `client::perf::CountingAllocator` does) and process-wide live
//! and peak bytes. Without the allocator installed every reading is zero.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static CALLS: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

/// The system allocator plus the counters.
pub struct CountingAlloc;

#[inline(always)]
fn note(bytes: usize) {
    if !INSTALLED.load(Ordering::Relaxed) {
        INSTALLED.store(true, Ordering::Relaxed);
    }
    let _ = CALLS.try_with(|c| c.set(c.get() + 1));
    let _ = BYTES.try_with(|c| c.set(c.get() + bytes as u64));
    let live = LIVE.fetch_add(bytes as u64, Ordering::Relaxed) + bytes as u64;
    if live > PEAK.load(Ordering::Relaxed) {
        PEAK.fetch_max(live, Ordering::Relaxed);
    }
}

// SAFETY: every call goes to `System` unchanged; only counters are added.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            note(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            note(layout.size());
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
            note(new_size);
        }
        new
    }
}

/// Allocation calls and requested bytes made by the calling thread so far.
#[inline(always)]
pub fn thread_totals() -> (u64, u64) {
    (
        CALLS.try_with(Cell::get).unwrap_or(0),
        BYTES.try_with(Cell::get).unwrap_or(0),
    )
}

/// Live and peak heap bytes of the process; None when the allocator is not installed.
pub fn heap() -> Option<(u64, u64)> {
    INSTALLED
        .load(Ordering::Relaxed)
        .then(|| (LIVE.load(Ordering::Relaxed), PEAK.load(Ordering::Relaxed)))
}

/// Starts a new peak window at the live bytes.
pub fn reset_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}
