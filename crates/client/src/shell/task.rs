//! Work off the frame. A native build runs it on a thread; a browser page has no threads
//! (the build has no wasm atomics), so the work is dropped there: the screen that asked
//! for it keeps showing what it shows when its pictures are missing.
//! Work that must happen on a page (a match being made) is run in place by its caller.

/// Runs `work` on its own thread; in a browser it is dropped unrun, which closes the
/// channels it holds, so its owner sees a finished worker with no results.
pub fn spawn(work: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::spawn(work);
    }
    #[cfg(target_arch = "wasm32")]
    drop(work);
}
