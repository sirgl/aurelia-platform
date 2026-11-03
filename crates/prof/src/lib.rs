//! Scoped timers, allocation counters and a stack sampler for the logic profile
//! (integration/docs/session-profile.md).
//!
//! - [`span`] / [`span_debug`]: a guard that times a scope into a per-thread call tree
//!   (calls, inclusive and exclusive time, allocations made directly in the scope). Without
//!   the `enabled` feature both are empty inline functions; the tree is then always empty.
//! - [`alloc::CountingAlloc`]: a global allocator wrapper a binary may install; it counts
//!   allocations and bytes per thread (spans read the counters) and live/peak bytes overall.
//! - [`sampler`]: a SIGPROF-driven stack sampler (feature `sampler`, Linux x86_64/aarch64).
//!
//! Nothing here touches game state: instrumented and plain builds play the same game.

pub mod alloc;
pub mod sampler;

mod tree;

pub use tree::{drain, set_active, Record};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
// std::time::Instant panics on wasm32-unknown-unknown; web_time reads the browser's clock.
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

static CPU_CLOCK: AtomicBool = AtomicBool::new(false);

/// Makes spans (and [`now_ns`]) read the calling thread's CPU clock instead of the wall
/// clock: on a shared, loaded machine the wall clock grows when the core is taken away and
/// the CPU clock does not. Unix only (`CLOCK_THREAD_CPUTIME_ID`); returns whether the CPU
/// clock is in effect. The CPU clock costs a system call per reading (about 0.3 us).
pub fn set_cpu_clock(on: bool) -> bool {
    let on = on && cfg!(unix);
    CPU_CLOCK.store(on, Ordering::Relaxed);
    on
}

/// The clock spans use, in nanoseconds from an arbitrary start; differences are what count.
/// Wall time by default, the thread's CPU time after `set_cpu_clock(true)`.
#[inline]
pub fn now_ns() -> u64 {
    if CPU_CLOCK.load(Ordering::Relaxed) {
        if let Some(ns) = thread_cpu_ns() {
            return ns;
        }
    }
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

#[cfg(unix)]
fn thread_cpu_ns() -> Option<u64> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: fills the struct it is given.
    if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } == 0 {
        return Some(ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64);
    }
    None
}

#[cfg(not(unix))]
fn thread_cpu_ns() -> Option<u64> {
    None
}

/// A scope being timed; the time is booked when it drops.
#[must_use = "a span times the scope it lives in"]
pub struct Span {
    #[cfg(feature = "enabled")]
    armed: bool,
}

/// Times the scope up to the guard's drop under `name` (nested spans form a tree).
#[inline(always)]
pub fn span(name: &'static str) -> Span {
    #[cfg(feature = "enabled")]
    {
        Span {
            armed: tree::enter(name),
        }
    }
    #[cfg(not(feature = "enabled"))]
    {
        let _ = name;
        Span {}
    }
}

/// Like [`span`] with a name made of a prefix and the `Debug` text of `what`
/// (`span_debug("turn/end/", &step)`); the text is only built when spans are measuring.
#[inline(always)]
pub fn span_debug(prefix: &'static str, what: &dyn std::fmt::Debug) -> Span {
    #[cfg(feature = "enabled")]
    {
        Span {
            armed: tree::enter_debug(prefix, what),
        }
    }
    #[cfg(not(feature = "enabled"))]
    {
        let _ = (prefix, what);
        Span {}
    }
}

#[cfg(feature = "enabled")]
impl Drop for Span {
    #[inline(always)]
    fn drop(&mut self) {
        if self.armed {
            tree::exit();
        }
    }
}

/// True when this build measures spans (the `enabled` feature).
pub const fn compiled_in() -> bool {
    cfg!(feature = "enabled")
}
