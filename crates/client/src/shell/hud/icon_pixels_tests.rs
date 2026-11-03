//! The HUD's drawn icons as pixels: a frame's shapes tessellated by egui and filled on the CPU
//! (triangles with their vertex colours, the font atlas for labels), the way the GPU paints
//! them, so tests can compare glyphs and a sheet of every icon can be looked at.
//! `AURELIA_ICON_SHEET=<folder> cargo test -p aurelia-client icon_sheet` writes the sheets.
use super::{icon, text, Icon, INK, LIGHT};
use egui::epaint::{ClippedPrimitive, ColorImage, ImageData, ImageDelta, Primitive, TextureId};
use egui::{pos2, vec2, Align2, Color32, Painter, Pos2, Rect};
use std::collections::HashMap;

/// Paints `draw` on a `size` (points) canvas filled with `background` and returns the pixels at
/// `pixels_per_point`.
pub(super) fn render(size: egui::Vec2, pixels_per_point: f32, background: Color32, mut draw: impl FnMut(&Painter)) -> image::RgbaImage {
    let ctx = egui::Context::default();
    let mut input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..Default::default() };
    input.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(pixels_per_point);
    let mut output = ctx.run_ui(input, |ui| draw(ui.painter()));
    let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
    let set = std::mem::take(&mut output.textures_delta.set);
    output.textures_delta.clear();
    for (id, deltas) in set {
        for ImageDelta { image: ImageData::Color(patch), pos, .. } in deltas {
            match pos {
                None => {
                    textures.insert(id, (*patch).clone());
                }
                Some([x0, y0]) => {
                    let Some(whole) = textures.get_mut(&id) else { continue };
                    for y in 0..patch.size[1] {
                        for x in 0..patch.size[0] {
                            whole.pixels[(y0 + y) * whole.size[0] + x0 + x] = patch.pixels[y * patch.size[0] + x];
                        }
                    }
                }
            }
        }
    }
    let primitives = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
    let (w, h) = ((size.x * pixels_per_point).round() as u32, (size.y * pixels_per_point).round() as u32);
    let [r, g, b, _] = background.to_array();
    let mut pixels = vec![[r as f32, g as f32, b as f32]; (w * h) as usize];
    for ClippedPrimitive { primitive, .. } in &primitives {
        let Primitive::Mesh(mesh) = primitive else { continue };
        let texture = textures.get(&mesh.texture_id);
        for tri in mesh.indices.chunks_exact(3) {
            let v = [0, 1, 2].map(|k| mesh.vertices[tri[k] as usize]);
            let p = v.map(|v| v.pos.to_vec2() * output.pixels_per_point);
            let area = (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[2].x - p[0].x) * (p[1].y - p[0].y);
            if area.abs() < 1e-6 {
                continue;
            }
            let x0 = p.iter().map(|p| p.x).fold(f32::MAX, f32::min).floor().max(0.0) as u32;
            let x1 = (p.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil() as u32).min(w);
            let y0 = p.iter().map(|p| p.y).fold(f32::MAX, f32::min).floor().max(0.0) as u32;
            let y1 = (p.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil() as u32).min(h);
            for y in y0..y1 {
                for x in x0..x1 {
                    let q = vec2(x as f32 + 0.5, y as f32 + 0.5);
                    let edge = |a: egui::Vec2, b: egui::Vec2| ((b.x - a.x) * (q.y - a.y) - (q.x - a.x) * (b.y - a.y)) / area;
                    let l = [edge(p[1], p[2]), edge(p[2], p[0]), edge(p[0], p[1])];
                    if l.iter().any(|&l| l < 0.0) {
                        continue;
                    }
                    let mut src = [0.0f32; 4];
                    for k in 0..3 {
                        for (n, value) in v[k].color.to_array().iter().enumerate() {
                            src[n] += l[k] * *value as f32;
                        }
                    }
                    if let Some(image) = &texture {
                        let uv = v[0].uv.to_vec2() * l[0] + v[1].uv.to_vec2() * l[1] + v[2].uv.to_vec2() * l[2];
                        let tx = ((uv.x * image.size[0] as f32) as usize).min(image.size[0] - 1);
                        let ty = ((uv.y * image.size[1] as f32) as usize).min(image.size[1] - 1);
                        let texel = image.pixels[ty * image.size[0] + tx].to_array();
                        for n in 0..4 {
                            src[n] *= texel[n] as f32 / 255.0;
                        }
                    }
                    let dst = &mut pixels[(y * w + x) as usize];
                    let keep = 1.0 - src[3] / 255.0;
                    for n in 0..3 {
                        dst[n] = src[n] + dst[n] * keep;
                    }
                }
            }
        }
    }
    image::RgbaImage::from_fn(w, h, |x, y| {
        let [r, g, b] = pixels[(y * w + x) as usize];
        image::Rgba([r.round().clamp(0.0, 255.0) as u8, g.round().clamp(0.0, 255.0) as u8, b.round().clamp(0.0, 255.0) as u8, 255])
    })
}

/// One icon alone, centred on a square of `2.6 * s` points.
pub(super) fn glyph(kind: Icon, s: f32, pixels_per_point: f32, color: Color32, background: Color32) -> image::RgbaImage {
    let side = (2.6 * s).ceil();
    render(vec2(side, side), pixels_per_point, background, |p| icon(p, pos2(side / 2.0, side / 2.0), s, kind, color))
}

/// The share of pixels that differ by more than a faint edge between two same-sized pictures.
fn differing_share(a: &image::RgbaImage, b: &image::RgbaImage) -> f32 {
    assert_eq!(a.dimensions(), b.dimensions());
    let differing = a.pixels().zip(b.pixels()).filter(|(a, b)| a.0.iter().zip(b.0.iter()).any(|(a, b)| a.abs_diff(*b) > 48)).count();
    differing as f32 / (a.width() * a.height()) as f32
}

/// The map's backgrounds a glyph must read on: dark HUD, grassland, plains, desert, snow.
const GROUNDS: [Color32; 5] = [
    Color32::from_rgb(16, 30, 40),
    Color32::from_rgb(86, 118, 52),
    Color32::from_rgb(150, 138, 84),
    Color32::from_rgb(206, 186, 132),
    Color32::from_rgb(222, 228, 232),
];

#[test]
fn faith_gear_and_sun_are_different_pictures() {
    for (s, pixels_per_point) in [(5.0, 1.0), (5.0, 2.0), (9.0, 1.0), (24.0, 1.0)] {
        let pictures = [Icon::Faith, Icon::Gear, Icon::Sun].map(|kind| glyph(kind, s, pixels_per_point, LIGHT, GROUNDS[0]));
        for (a, b) in [(0, 1), (0, 2), (1, 2)] {
            let share = differing_share(&pictures[a], &pictures[b]);
            assert!(share > 0.08, "icons {a} and {b} at s={s} x{pixels_per_point} differ in {:.1}% of pixels", share * 100.0);
        }
    }
}

#[test]
fn the_six_yields_are_different_pictures_at_map_size() {
    for pixels_per_point in [1.0, 2.0] {
        let pictures = yields().map(|(kind, tint)| glyph(kind, 5.0, pixels_per_point, tint, GROUNDS[1]));
        for a in 0..pictures.len() {
            for b in a + 1..pictures.len() {
                let share = differing_share(&pictures[a], &pictures[b]);
                assert!(share > 0.08, "yields {a} and {b} x{pixels_per_point} differ in {:.1}% of pixels", share * 100.0);
            }
        }
    }
}

const ALL: [(Icon, &str); 17] = [
    (Icon::Apple, "Apple"),
    (Icon::Hammer, "Hammer"),
    (Icon::Coin, "Coin"),
    (Icon::Flask, "Flask"),
    (Icon::Note, "Note"),
    (Icon::Faith, "Faith"),
    (Icon::Sun, "Sun"),
    (Icon::Gear, "Gear"),
    (Icon::Leaf, "Leaf"),
    (Icon::Compass, "Compass"),
    (Icon::Hex, "Hex"),
    (Icon::City, "City"),
    (Icon::Hourglass, "Hourglass"),
    (Icon::Shield, "Shield"),
    (Icon::Book, "Book"),
    (Icon::List, "List"),
    (Icon::Scroll, "Scroll"),
];

/// The yields in map order with their tints (`tile_yields::YIELD_ICONS`).
fn yields() -> [(Icon, Color32); 6] {
    super::tile_yields::YIELD_ICONS
}

/// Every icon at the map's yield size (s 5), the top bar's (9) and large (24), on each ground;
/// below, the six yields side by side as a tile shows them. Written when `AURELIA_ICON_SHEET`
/// names a folder.
#[test]
fn icon_sheet() {
    let sheet = |pixels_per_point: f32| {
        let row = 64.0;
        let size = vec2(110.0 + GROUNDS.len() as f32 * 120.0, row * (ALL.len() as f32 + 2.0));
        render(size, pixels_per_point, Color32::from_rgb(40, 44, 48), |p| {
            for (n, ground) in GROUNDS.iter().enumerate() {
                let x = 110.0 + n as f32 * 120.0;
                p.rect_filled(Rect::from_min_size(pos2(x, 0.0), vec2(116.0, size.y)), 0.0, *ground);
            }
            for (r, (kind, name)) in ALL.iter().enumerate() {
                let y = r as f32 * row + row / 2.0;
                text(p, pos2(6.0, y), Align2::LEFT_CENTER, name, 13.0, LIGHT);
                for n in 0..GROUNDS.len() {
                    let x = 110.0 + n as f32 * 120.0;
                    let tint = yields().iter().find(|(k, _)| k == kind).map_or(if n == 4 { INK } else { LIGHT }, |(_, t)| *t);
                    icon(p, pos2(x + 14.0, y), 5.0, *kind, tint);
                    icon(p, pos2(x + 40.0, y), 9.0, *kind, tint);
                    icon(p, pos2(x + 86.0, y), 24.0, *kind, tint);
                }
            }
            let y = ALL.len() as f32 * row + row;
            text(p, pos2(6.0, y), Align2::LEFT_CENTER, "yields", 13.0, LIGHT);
            for n in 0..GROUNDS.len() {
                let x = 110.0 + n as f32 * 120.0;
                for (k, (kind, tint)) in yields().iter().enumerate() {
                    icon(p, pos2(x + 10.0 + k as f32 * 13.0, y - 14.0), 5.0, *kind, *tint);
                    icon(p, pos2(x + 10.0 + k as f32 * 19.0, y + 12.0), 8.0, *kind, *tint);
                }
            }
        })
    };
    let pictures = [(1.0, sheet(1.0)), (2.0, sheet(2.0))];
    if let Ok(folder) = std::env::var("AURELIA_ICON_SHEET") {
        for (pixels_per_point, picture) in &pictures {
            let path = std::path::Path::new(&folder).join(format!("icons-x{pixels_per_point}.png"));
            picture.save(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        }
    }
}
