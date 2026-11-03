//! Printed atlas illustrations on strictly unexplored parchment. Their positions
//! are world-anchored, and only visibility masks determine clipping — never terrain.
use render::{camera::CameraController, world::state::RenderWorld};
use egui::{Color32, Pos2, Rect, TextureHandle};
use std::sync::Arc;

const GRID: usize = 24;
const PAPER_HEIGHT: f32 = 0.35;
const INK_OPACITY: f32 = 0.36;
const NAMES: [&str; 3] = ["atlas serpent", "atlas kraken", "atlas compass"];
/// The three engravings (PNG), compiled in: 6.7 MB.
#[cfg(not(target_arch = "wasm32"))]
const ASSETS: [&[u8]; 3] = [
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/visual/fog-serpent-v1.png")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/visual/fog-kraken-v1.png")),
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/visual/fog-compass-v1.png")),
];
/// In the browser the page fetches the PNGs after the menu is up and hands them over with `provide`
/// (serpent, kraken, compass); the map is drawn without the engravings until they arrive.
#[cfg(target_arch = "wasm32")]
static PROVIDED: std::sync::OnceLock<[Vec<u8>; 3]> = std::sync::OnceLock::new();

/// Browser only: the PNG files of the three engravings.
#[cfg(target_arch = "wasm32")]
pub fn provide(serpent: Vec<u8>, kraken: Vec<u8>, compass: Vec<u8>) {
    let _ = PROVIDED.set([serpent, kraken, compass]);
}

#[cfg(not(target_arch = "wasm32"))]
fn asset_bytes() -> Option<[&'static [u8]; 3]> {
    Some(ASSETS)
}

#[cfg(target_arch = "wasm32")]
fn asset_bytes() -> Option<[&'static [u8]; 3]> {
    PROVIDED.get().map(|files| [files[0].as_slice(), files[1].as_slice(), files[2].as_slice()])
}

#[derive(Clone, Copy)]
struct PaperVertex {
    world: [f32; 2],
    uv: Pos2,
    alpha: f32,
}
struct PaperDrawing {
    image: usize,
    center: [f32; 2],
    vertices: Vec<PaperVertex>,
}
struct CachedDrawings {
    key: (u32, u32, u64),
    drawings: Vec<PaperDrawing>,
}

pub fn draw(ctx: &egui::Context, world: &RenderWorld, camera: &CameraController) {
    if world.width == 0 || world.height == 0 {
        return;
    }
    let drawings = drawings(ctx, world);
    if drawings.drawings.is_empty() {
        return;
    }
    let Some(textures) = textures(ctx) else {
        return;
    };
    let screen = ctx.content_rect();
    let matrix = camera.view_proj(screen.aspect_ratio());
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            "atlas_illustrations".into(),
        ))
        .with_clip_rect(screen);
    for drawing in &drawings.drawings {
        // Conservative cull only; placement and visibility do not follow the camera.
        let Some(center) = project(&matrix, screen, drawing.center) else {
            continue;
        };
        if !screen.expand(700.0).contains(center) {
            continue;
        }
        let mut mesh = egui::Mesh::with_texture(textures[drawing.image].id());
        for cell in drawing.vertices.chunks_exact(4) {
            let projected: [Option<Pos2>; 4] =
                std::array::from_fn(|i| project(&matrix, screen, cell[i].world));
            let [Some(a), Some(b), Some(c), Some(d)] = projected else {
                continue;
            };
            let first = mesh.vertices.len() as u32;
            for (v, pos) in cell.iter().zip([a, b, c, d]) {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos,
                    uv: v.uv,
                    color: Color32::from_white_alpha((v.alpha * INK_OPACITY * 255.0) as u8),
                });
            }
            mesh.indices.extend_from_slice(&[
                first,
                first + 1,
                first + 2,
                first,
                first + 2,
                first + 3,
            ]);
        }
        painter.add(egui::Shape::mesh(mesh));
    }
}
fn textures(ctx: &egui::Context) -> Option<Arc<[TextureHandle; 3]>> {
    let id = egui::Id::new("atlas_illustration_textures_v1");
    if let Some(cached) = ctx.data(|data| data.get_temp::<Arc<[TextureHandle; 3]>>(id)) {
        return Some(cached);
    }
    let files = asset_bytes()?;
    let loaded = Arc::new(std::array::from_fn(|i| {
        let (name, bytes) = (NAMES[i], files[i]);
        let mut decoded = image::load_from_memory(bytes)
            .expect("bundled atlas PNG must decode")
            .to_rgba8();
        // Generated illustrations include pale body fills and soft edge haze.
        // Keep only the engraved dark marks; parchment must show through them.
        for pixel in decoded.pixels_mut() {
            pixel.0 = engraving_ink(pixel.0);
        }
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [decoded.width() as usize, decoded.height() as usize],
            decoded.as_raw(),
        );
        ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
    }));
    ctx.data_mut(|data| data.insert_temp(id, loaded.clone()));
    Some(loaded)
}
fn engraving_ink(rgba: [u8; 4]) -> [u8; 4] {
    let luminance = 0.2126 * rgba[0] as f32 + 0.7152 * rgba[1] as f32 + 0.0722 * rgba[2] as f32;
    let darkness = ((165.0 - luminance) / 95.0).clamp(0.0, 1.0);
    let coverage = ((rgba[3] as f32 - 32.0) / 160.0).clamp(0.0, 1.0);
    let coverage = coverage * coverage * (3.0 - 2.0 * coverage);
    [65, 39, 18, (rgba[3] as f32 * darkness * coverage) as u8]
}
fn visibility_key(world: &RenderWorld) -> (u32, u32, u64) {
    // Masks are hashed as well as dimensions: a caller changing fog without a
    // renderer revision must not leave a stale monster over a newly visible tile.
    let mut hash = 0xcbf29ce484222325u64;
    for bytes in [&world.visible, &world.explored] {
        hash = (hash ^ bytes.len() as u64).wrapping_mul(0x100000001b3);
        for byte in bytes {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    (world.width, world.height, hash)
}
fn drawings(ctx: &egui::Context, world: &RenderWorld) -> Arc<CachedDrawings> {
    let id = egui::Id::new("atlas_illustration_geometry_v1");
    let key = visibility_key(world);
    if let Some(cached) = ctx.data(|data| data.get_temp::<Arc<CachedDrawings>>(id)) {
        if cached.key == key {
            return cached;
        }
    }
    let mut drawings = Vec::new();
    // Three widely separated atlas drawings, anchored to map dimensions. Their
    // placement never searches hidden geography or follows the view. Exploration
    // can erase them, but must not relocate them onto another unexplored region.
    for (image, fraction, size) in [
        (0, [0.065, 0.407], [6.8, 6.2]),
        (1, [0.80, 0.26], [6.8, 6.2]),
        (2, [0.032, 0.70], [5.3, 5.3]),
    ] {
        let (x, z) = render::scene::layout::tile_to_world(
            ((world.width - 1) as f32 * fraction[0]).round() as u32,
            ((world.height - 1) as f32 * fraction[1]).round() as u32,
        );
        let center = [x, z];
        let drawing = drawing(world, center, size, image);
        if !drawing.vertices.is_empty() {
            drawings.push(drawing);
        }
    }
    let cached = Arc::new(CachedDrawings { key, drawings });
    ctx.data_mut(|data| data.insert_temp(id, cached.clone()));
    cached
}
fn drawing(world: &RenderWorld, center: [f32; 2], size: [f32; 2], image: usize) -> PaperDrawing {
    let mut vertices = Vec::new();
    let dx = size[0] / GRID as f32;
    let dz = size[1] / GRID as f32;
    // A cell fits inside this disk. Clearance from known hex circumcircles
    // proves its entire two triangles remain outside every known tile.
    let radius = (dx * dx + dz * dz).sqrt() * 0.5;
    for row in 0..GRID {
        for col in 0..GRID {
            let x = center[0] - size[0] * 0.5 + col as f32 * dx;
            let z = center[1] - size[1] * 0.5 + row as f32 * dz;
            let clearance = unknown_clearance(world, x + dx * 0.5, z + dz * 0.5);
            if clearance <= radius {
                continue;
            }
            let alpha = ((clearance - radius) / 0.72).clamp(0.0, 1.0);
            let alpha = alpha * alpha * (3.0 - 2.0 * alpha);
            let u = col as f32 / GRID as f32;
            let v = row as f32 / GRID as f32;
            let du = 1.0 / GRID as f32;
            // UV top is north; artwork is printed flat onto the world-space sheet.
            for (world, uv) in [
                ([x, z], Pos2::new(u, v)),
                ([x + dx, z], Pos2::new(u + du, v)),
                ([x + dx, z + dz], Pos2::new(u + du, v + du)),
                ([x, z + dz], Pos2::new(u, v + du)),
            ] {
                vertices.push(PaperVertex { world, uv, alpha });
            }
        }
    }
    PaperDrawing {
        image,
        center,
        vertices,
    }
}
fn unknown_clearance(world: &RenderWorld, x: f32, z: f32) -> f32 {
    let Some(tile) = render::scene::picking::world_to_tile(x, z, (world.width, world.height))
    else {
        return 0.0;
    };
    let i = tile.0 as usize;
    if world.visible.get(i).copied() != Some(0) || world.explored.get(i).copied() != Some(0) {
        return 0.0;
    }
    if render::scene::fog::fog_at_position(world, x, z) > 0.18001 {
        return 0.0;
    }
    let q = (tile.0 % world.width) as i32;
    let r = (tile.0 / world.width) as i32;
    let mut clearance = 2.0f32;
    for rr in r - 2..=r + 2 {
        for qq in q - 2..=q + 2 {
            if qq < 0 || rr < 0 || qq >= world.width as i32 || rr >= world.height as i32 {
                continue;
            }
            let index = (rr as u32 * world.width + qq as u32) as usize;
            if world.visible.get(index).copied() == Some(0)
                && world.explored.get(index).copied() == Some(0)
            {
                continue;
            }
            let (cx, cz) = render::scene::layout::tile_to_world(qq as u32, rr as u32);
            clearance = clearance.min(((x - cx).powi(2) + (z - cz).powi(2)).sqrt() - 1.0);
        }
    }
    clearance.max(0.0)
}
fn project(matrix: &[[f32; 4]; 4], screen: Rect, world: [f32; 2]) -> Option<Pos2> {
    let p = [world[0], PAPER_HEIGHT, world[1], 1.0];
    let mut clip = [0.0; 4];
    for row in 0..4 {
        for col in 0..4 {
            clip[row] += matrix[col][row] * p[col];
        }
    }
    if clip[3] <= 0.0 {
        return None;
    }
    Some(Pos2::new(
        screen.left() + (clip[0] / clip[3] + 1.0) * screen.width() * 0.5,
        screen.top() + (1.0 - clip[1] / clip[3]) * screen.height() * 0.5,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atlas_ink_preserves_engraving_but_removes_pale_fill_and_haze() {
        assert_eq!(engraving_ink([235, 220, 190, 255])[3], 0);
        assert_eq!(engraving_ink([60, 40, 20, 24])[3], 0);
        assert_eq!(engraving_ink([0, 0, 0, 0])[3], 0);
        assert_eq!(engraving_ink([65, 39, 18, 255]), [65, 39, 18, 255]);
        assert!(engraving_ink([120, 100, 80, 255])[3] > 100);
    }
    fn world() -> RenderWorld {
        RenderWorld {
            width: 7,
            height: 7,
            visible: vec![0; 49],
            explored: vec![0; 49],
            ..Default::default()
        }
    }
    #[test]
    fn atlas_ink_rejects_visible_remembered_and_missing_mask_entries() {
        let mut w = world();
        let i = 3 * 7 + 3;
        let (x, z) = render::scene::layout::tile_to_world(3, 3);
        assert!(unknown_clearance(&w, x, z) > 1.0);
        w.visible[i] = 1;
        assert_eq!(unknown_clearance(&w, x, z), 0.0);
        w.visible[i] = 0;
        w.explored[i] = 1;
        assert_eq!(unknown_clearance(&w, x, z), 0.0);
        w.explored.clear();
        assert_eq!(unknown_clearance(&w, x, z), 0.0);
    }
    #[test]
    fn atlas_ink_padding_keeps_whole_cells_out_of_neighboring_known_hex() {
        let mut w = world();
        w.visible[3 * 7 + 4] = 1;
        w.explored[3 * 7 + 4] = 1;
        let (x, z) = render::scene::layout::tile_to_world(3, 3);
        assert_eq!(unknown_clearance(&w, x + 0.8, z), 0.0);
        let mesh = drawing(&w, [x, z], [4.0, 4.0], 0);
        for cell in mesh.vertices.chunks_exact(4) {
            for vertex in cell {
                assert!(unknown_clearance(&w, vertex.world[0], vertex.world[1]) > 0.0);
            }
        }
    }
    #[test]
    fn atlas_cache_key_changes_when_visibility_changes_without_revision() {
        let mut w = world();
        let before = visibility_key(&w);
        w.explored[10] = 1;
        assert_ne!(before, visibility_key(&w));
    }
}
