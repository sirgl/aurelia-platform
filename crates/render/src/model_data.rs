//! The baked model vertices in the browser build (wasm32): they arrive from the page as words.
//! Native builds compile them in (`include_bytes!`), so this module is empty there.
#![cfg(target_arch = "wasm32")]

use crate::gpu::buffers::UnitVertex;

/// `words` (the `.bin` file, little-endian) as `count` vertices, leaked for the program's life.
/// The words keep their storage when its capacity divides evenly, else they are copied once.
pub fn vertices_from_words(words: Vec<u32>, count: usize, name: &str) -> Result<&'static [UnitVertex], String> {
    let want = count * std::mem::size_of::<UnitVertex>();
    if words.len() * 4 != want {
        return Err(format!("{name}: {} bytes, the build expects {want}", words.len() * 4));
    }
    let vertices: Vec<UnitVertex> = match bytemuck::allocation::try_cast_vec(words) {
        Ok(vertices) => vertices,
        Err((_, words)) => bytemuck::allocation::pod_collect_to_vec(&words),
    };
    Ok(Vec::leak(vertices))
}
