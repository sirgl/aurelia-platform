//! A stack sampler for `perf`-less machines: a SIGPROF interval timer (process CPU time)
//! whose handler walks the frame pointers of the interrupted thread into a preallocated
//! buffer; addresses are turned into function names afterwards with `addr2line`.
//!
//! Available with the `sampler` feature on Linux (glibc) x86_64 and aarch64; elsewhere
//! [`available`] is false and [`start`] says so (on macOS use `sample <pid>` or
//! Instruments instead). Frame pointers are needed for deep stacks: build the profiled
//! binary with `RUSTFLAGS="-C force-frame-pointers=yes"`. Without them a stack is the
//! interrupted function and its caller's caller at best.
//!
//! The handler only reads memory (the stack, bounds-checked against the thread's stack)
//! and writes atomics; it allocates nothing and takes no lock.

use std::collections::HashMap;

/// What a sampling run collected.
#[derive(Debug, Default, Clone)]
pub struct Samples {
    /// Sampling rate asked for, samples per CPU second.
    pub hz: u32,
    /// Each sample: instruction pointers, innermost first.
    pub stacks: Vec<Vec<usize>>,
    /// Samples that arrived after the buffer was full or on another thread.
    pub dropped: u64,
    /// Where the executable is mapped (load base) and its executable range, for symbols.
    pub base: usize,
    pub text: (usize, usize),
}

#[cfg(all(
    feature = "sampler",
    target_os = "linux",
    target_env = "gnu",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod on {
    use super::Samples;
    use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

    const DEPTH: usize = 48;
    const SLOT: usize = DEPTH + 1;

    static BUF: AtomicPtr<AtomicUsize> = AtomicPtr::new(std::ptr::null_mut());
    static CAP: AtomicUsize = AtomicUsize::new(0);
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    static DROPPED: AtomicUsize = AtomicUsize::new(0);
    static STACK_LO: AtomicUsize = AtomicUsize::new(0);
    static STACK_HI: AtomicUsize = AtomicUsize::new(0);
    static TEXT_LO: AtomicUsize = AtomicUsize::new(0);
    static TEXT_HI: AtomicUsize = AtomicUsize::new(0);

    pub fn available() -> bool {
        true
    }

    /// Mapping of the executable: (load base, executable range).
    pub fn exe_map() -> (usize, (usize, usize)) {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| std::fs::canonicalize(p).ok());
        let Some(exe) = exe else {
            return (0, (0, 0));
        };
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
        let (mut base, mut lo, mut hi) = (usize::MAX, usize::MAX, 0usize);
        for line in maps.lines() {
            let mut parts = line.split_whitespace();
            let (Some(range), Some(perms), Some(_off), Some(_dev), Some(_inode)) = (
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
                parts.next(),
            ) else {
                continue;
            };
            let path = parts.collect::<Vec<_>>().join(" ");
            if std::path::Path::new(&path) != exe.as_path() {
                continue;
            }
            let Some((a, b)) = range.split_once('-') else {
                continue;
            };
            let (Ok(a), Ok(b)) = (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16))
            else {
                continue;
            };
            base = base.min(a);
            if perms.contains('x') {
                lo = lo.min(a);
                hi = hi.max(b);
            }
        }
        if base == usize::MAX {
            return (0, (0, 0));
        }
        (base, (lo, hi))
    }

    unsafe fn context_registers(ctx: *mut libc::c_void) -> (usize, usize, usize) {
        let uc = ctx as *mut libc::ucontext_t;
        #[cfg(target_arch = "x86_64")]
        {
            let g = unsafe { &(*uc).uc_mcontext.gregs };
            (
                g[libc::REG_RIP as usize] as usize,
                g[libc::REG_RBP as usize] as usize,
                g[libc::REG_RSP as usize] as usize,
            )
        }
        #[cfg(target_arch = "aarch64")]
        {
            let m = unsafe { &(*uc).uc_mcontext };
            (m.pc as usize, m.regs[29] as usize, m.sp as usize)
        }
    }

    extern "C" fn handler(_sig: libc::c_int, _info: *mut libc::siginfo_t, ctx: *mut libc::c_void) {
        let buf = BUF.load(Ordering::Relaxed);
        if buf.is_null() || ctx.is_null() {
            return;
        }
        let (pc, mut fp, sp) = unsafe { context_registers(ctx) };
        let (lo, hi) = (
            STACK_LO.load(Ordering::Relaxed),
            STACK_HI.load(Ordering::Relaxed),
        );
        if sp < lo || sp >= hi {
            DROPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let at = NEXT.fetch_add(1, Ordering::Relaxed);
        if at >= CAP.load(Ordering::Relaxed) {
            DROPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let slot = unsafe { buf.add(at * SLOT) };
        let mut n = 0usize;
        let put = |n: &mut usize, ip: usize| {
            if *n < DEPTH {
                unsafe { (*slot.add(1 + *n)).store(ip, Ordering::Relaxed) };
                *n += 1;
            }
        };
        put(&mut n, pc);
        let (tlo, thi) = (
            TEXT_LO.load(Ordering::Relaxed),
            TEXT_HI.load(Ordering::Relaxed),
        );
        // A leaf in libc (memcpy, malloc fast paths) keeps its return address near sp and the
        // caller's frame pointer in rbp: look for the first word that points into our code.
        if pc < tlo || pc >= thi {
            let mut p = sp;
            for _ in 0..8 {
                if p + 8 > hi {
                    break;
                }
                let w = unsafe { *(p as *const usize) };
                if w >= tlo && w < thi {
                    put(&mut n, w);
                    break;
                }
                p += 8;
            }
        }
        while n < DEPTH && fp % 8 == 0 && fp >= sp && fp + 16 <= hi {
            let next = unsafe { *(fp as *const usize) };
            let ret = unsafe { *((fp + 8) as *const usize) };
            if ret == 0 {
                break;
            }
            put(&mut n, ret);
            if next <= fp {
                break;
            }
            fp = next;
        }
        unsafe { (*slot).store(n, Ordering::Release) };
    }

    pub fn start(hz: u32, capacity: usize) -> Result<(), String> {
        if !BUF.load(Ordering::Relaxed).is_null() {
            return Err("the sampler is already running".into());
        }
        let (base, text) = exe_map();
        let _ = base;
        if text.1 == 0 {
            return Err("cannot find the executable's mapping in /proc/self/maps".into());
        }
        TEXT_LO.store(text.0, Ordering::Relaxed);
        TEXT_HI.store(text.1, Ordering::Relaxed);
        // The calling thread's stack bounds.
        unsafe {
            let mut attr: libc::pthread_attr_t = std::mem::zeroed();
            if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
                return Err("pthread_getattr_np failed".into());
            }
            let (mut addr, mut size) = (std::ptr::null_mut(), 0usize);
            libc::pthread_attr_getstack(&attr, &mut addr, &mut size);
            libc::pthread_attr_destroy(&mut attr);
            STACK_LO.store(addr as usize, Ordering::Relaxed);
            STACK_HI.store(addr as usize + size, Ordering::Relaxed);
        }
        let words = capacity * SLOT;
        let storage: Box<[AtomicUsize]> = (0..words).map(|_| AtomicUsize::new(0)).collect();
        let ptr = Box::leak(storage).as_mut_ptr();
        CAP.store(capacity, Ordering::Relaxed);
        NEXT.store(0, Ordering::Relaxed);
        DROPPED.store(0, Ordering::Relaxed);
        BUF.store(ptr, Ordering::SeqCst);
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handler as *const () as usize;
            action.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(libc::SIGPROF, &action, std::ptr::null_mut()) != 0 {
                return Err("sigaction(SIGPROF) failed".into());
            }
            let micros = (1_000_000 / hz.max(1)) as libc::suseconds_t;
            let interval = libc::timeval {
                tv_sec: 0,
                tv_usec: micros,
            };
            let timer = libc::itimerval {
                it_interval: interval,
                it_value: interval,
            };
            if libc::setitimer(libc::ITIMER_PROF, &timer, std::ptr::null_mut()) != 0 {
                return Err("setitimer(ITIMER_PROF) failed".into());
            }
        }
        Ok(())
    }

    pub fn stop(hz: u32) -> Samples {
        unsafe {
            let off = libc::itimerval {
                it_interval: libc::timeval {
                    tv_sec: 0,
                    tv_usec: 0,
                },
                it_value: libc::timeval {
                    tv_sec: 0,
                    tv_usec: 0,
                },
            };
            libc::setitimer(libc::ITIMER_PROF, &off, std::ptr::null_mut());
            libc::signal(libc::SIGPROF, libc::SIG_IGN);
        }
        let buf = BUF.swap(std::ptr::null_mut(), Ordering::SeqCst);
        let taken = NEXT
            .load(Ordering::Relaxed)
            .min(CAP.load(Ordering::Relaxed));
        let (base, text) = exe_map();
        let mut out = Samples {
            hz,
            base,
            text,
            dropped: DROPPED.load(Ordering::Relaxed) as u64,
            ..Samples::default()
        };
        if buf.is_null() {
            return out;
        }
        for at in 0..taken {
            let slot = unsafe { buf.add(at * SLOT) };
            let n = unsafe { (*slot).load(Ordering::Acquire) };
            if n == 0 || n > DEPTH {
                continue;
            }
            out.stacks.push(
                (0..n)
                    .map(|i| unsafe { (*slot.add(1 + i)).load(Ordering::Relaxed) })
                    .collect(),
            );
        }
        // The buffer stays leaked: a late signal must not write freed memory.
        out
    }
}

#[cfg(not(all(
    feature = "sampler",
    target_os = "linux",
    target_env = "gnu",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
mod on {
    use super::Samples;

    pub fn available() -> bool {
        false
    }

    pub fn start(_hz: u32, _capacity: usize) -> Result<(), String> {
        Err("the stack sampler needs the `sampler` feature on Linux (glibc) x86_64/aarch64; use `perf`, `sample` or Instruments here".into())
    }

    pub fn stop(hz: u32) -> Samples {
        Samples {
            hz,
            ..Samples::default()
        }
    }
}

/// True when this build can sample stacks.
pub fn available() -> bool {
    on::available()
}

/// Starts sampling the calling thread at `hz` samples per CPU second, keeping at most
/// `capacity` samples.
pub fn start(hz: u32, capacity: usize) -> Result<(), String> {
    on::start(hz, capacity)
}

/// Stops sampling and returns what was collected.
pub fn stop(hz: u32) -> Samples {
    on::stop(hz)
}

/// Function names of instruction pointers (innermost-frame addresses as `ip`, callers as
/// `ip - 1` are the caller's job), by `addr2line -f -C` on the running executable; an
/// address with no answer maps to its hex text. Without `addr2line` every address is hex.
pub fn symbolize(samples: &Samples, addrs: &[usize]) -> HashMap<usize, String> {
    let mut out: HashMap<usize, String> = addrs.iter().map(|a| (*a, format!("{a:#x}"))).collect();
    let Ok(exe) = std::env::current_exe() else {
        return out;
    };
    // Addresses outside the executable are named by the library that holds them.
    let libraries = library_ranges();
    for address in addrs {
        if let Some((_, _, name)) = libraries
            .iter()
            .find(|(lo, hi, _)| address >= lo && address < hi)
        {
            out.insert(*address, format!("[{name}]"));
        }
    }
    let addrs: Vec<usize> = addrs
        .iter()
        .copied()
        .filter(|a| samples.text.0 <= *a && *a < samples.text.1)
        .collect();
    let addrs = &addrs[..];
    let mut unique: Vec<usize> = addrs.to_vec();
    unique.sort_unstable();
    unique.dedup();
    if unique.is_empty() {
        return out;
    }
    let input: String = unique
        .iter()
        .map(|a| format!("{:#x}\n", a.wrapping_sub(samples.base)))
        .collect();
    let child = std::process::Command::new("addr2line")
        .args(["-f", "-C", "-e"])
        .arg(&exe)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return out;
    };
    let writer = child.stdin.take().map(|mut stdin| {
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(input.as_bytes());
        })
    });
    let Ok(output) = child.wait_with_output() else {
        return out;
    };
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    for addr in &unique {
        let (Some(function), Some(_place)) = (lines.next(), lines.next()) else {
            break;
        };
        if function != "??" {
            out.insert(*addr, function.to_owned());
        }
    }
    out
}

/// Executable mappings of other files (libc and friends) as (start, end, file name).
fn library_ranges() -> Vec<(usize, usize, String)> {
    let Ok(maps) = std::fs::read_to_string("/proc/self/maps") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in maps.lines() {
        let mut parts = line.split_whitespace();
        let (Some(range), Some(perms)) = (parts.next(), parts.next()) else {
            continue;
        };
        let path = parts.skip(3).collect::<Vec<_>>().join(" ");
        if !perms.contains('x') || !path.starts_with('/') {
            continue;
        }
        let Some((a, b)) = range.split_once('-') else {
            continue;
        };
        if let (Ok(a), Ok(b)) = (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16)) {
            let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
            out.push((a, b, name));
        }
    }
    out
}
