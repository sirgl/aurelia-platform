//! Text in a scaled layer, drawn at the size it ends up on the screen.
//!
//! A phone draws a module screen in one layer shrunk by about 0.85 (`screens::draw_body`). egui
//! lays text out at the size the code asked for, rasterises the glyphs at that size and then
//! scales the finished mesh with the layer, so every glyph is a bitmap of `size` pixels shrunk to
//! `size * scale`: the one-pixel dots of an ellipsis, hairlines and small labels fade out.
//!
//! The fix is one pass over the layer once the screen has drawn: every text shape is laid out
//! again from its own job at `size * scale` (widths, spacing and line heights too) and its
//! galley is scaled by `1 / scale`, which the layer's own scale takes back. The glyph bitmaps then
//! land on the screen one to one. Screens keep drawing in design units with no knowledge of it.
use egui::epaint::{Shape, TextShape};
use egui::emath::TSTransform;
use egui::{Context, LayerId};
use std::sync::Arc;

/// The scales below this are drawn as they are: the layer is not shrunk.
const NO_SCALE: f32 = 1.0e-3;

/// How many shapes the layer holds now: where `sharpen` starts.
pub fn mark(ctx: &Context, layer: LayerId) -> usize {
    ctx.graphics_mut(|graphics| graphics.get(layer).map_or(0, |list| list.all_entries().len()))
}

/// Lay out again the texts of `layer` drawn since `from` (a `mark`) for a layer drawn at `scale`.
/// Call it once per drawn screen, after the screen and before the frame ends.
pub fn sharpen(ctx: &Context, layer: LayerId, from: usize, scale: f32) {
    if (scale - 1.0).abs() < NO_SCALE || scale <= 0.0 {
        return;
    }
    // The shapes leave the paint list while the fonts work: both sit behind the context's lock.
    let mut taken: Vec<(usize, Shape)> = Vec::new();
    ctx.graphics_mut(|graphics| {
        let Some(list) = graphics.get_mut(layer) else { return };
        let end = list.all_entries().len();
        for index in from..end {
            list.mutate_shape(egui::layers::ShapeIdx(index), |clipped| {
                if holds_text(&clipped.shape) {
                    taken.push((index, std::mem::replace(&mut clipped.shape, Shape::Noop)));
                }
            });
        }
    });
    if taken.is_empty() {
        return;
    }
    ctx.fonts_mut(|fonts| {
        for (_, shape) in &mut taken {
            sharpen_shape(shape, scale, &mut |job| fonts.layout_job(job));
        }
    });
    ctx.graphics_mut(|graphics| {
        let Some(list) = graphics.get_mut(layer) else { return };
        for (index, shape) in taken {
            list.mutate_shape(egui::layers::ShapeIdx(index), |clipped| clipped.shape = shape);
        }
    });
}

fn holds_text(shape: &Shape) -> bool {
    match shape {
        Shape::Text(_) => true,
        Shape::Vec(shapes) => shapes.iter().any(holds_text),
        _ => false,
    }
}

fn sharpen_shape(shape: &mut Shape, scale: f32, layout: &mut dyn FnMut(egui::text::LayoutJob) -> Arc<egui::Galley>) {
    match shape {
        Shape::Text(text) => sharpen_text(text, scale, layout),
        Shape::Vec(shapes) => shapes.iter_mut().for_each(|inner| sharpen_shape(inner, scale, layout)),
        _ => {}
    }
}

fn sharpen_text(text: &mut TextShape, scale: f32, layout: &mut dyn FnMut(egui::text::LayoutJob) -> Arc<egui::Galley>) {
    // A rotated text keeps its bitmaps: its glyphs are not axis-aligned in the layer.
    if text.angle != 0.0 {
        return;
    }
    let (pos, underline) = (text.pos, text.underline);
    text.galley = layout(scaled_job(&text.galley.job, scale));
    // The galley is in screen units now; the layer's scale brings it back to the layer's own.
    text.pos = egui::Pos2::ZERO;
    text.transform(TSTransform::from_scaling(1.0 / scale));
    text.pos = pos;
    text.underline = underline;
}

/// `job` with every length in it multiplied by `scale`.
fn scaled_job(job: &egui::text::LayoutJob, scale: f32) -> egui::text::LayoutJob {
    let mut job = job.clone();
    job.wrap.max_width *= scale;
    job.first_row_min_height *= scale;
    for section in &mut job.sections {
        section.leading_space *= scale;
        let format = &mut section.format;
        format.font_id.size *= scale;
        format.extra_letter_spacing *= scale;
        format.line_height = format.line_height.map(|height| height * scale);
        format.expand_bg *= scale;
        format.underline.width *= scale;
        format.strikethrough.width *= scale;
    }
    job
}

/// The least that any glyph of `shapes` is drawn at, as a fraction of its bitmap in the font
/// atlas (1 is one to one; a layer shrunk to 0.85 gives 0.85, and thin strokes fade). Shapes are
/// tessellated at `pixels_per_point`; `None` when no glyph is drawn.
pub fn least_glyph_scale(ctx: &Context, shapes: Vec<egui::epaint::ClippedShape>, pixels_per_point: f32) -> Option<f32> {
    let [atlas_w, atlas_h] = ctx.fonts(|fonts| fonts.font_image_size());
    let mut least: Option<f32> = None;
    for primitive in ctx.tessellate(shapes, pixels_per_point) {
        let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else { continue };
        if mesh.texture_id != egui::TextureId::Managed(0) {
            continue;
        }
        for triangle in mesh.indices.chunks_exact(3) {
            let corners: Vec<_> = triangle.iter().map(|&i| mesh.vertices[i as usize]).collect();
            let extent = |f: &dyn Fn(&egui::epaint::Vertex) -> f32| {
                let values = corners.iter().map(f);
                values.clone().fold(f32::MIN, f32::max) - values.fold(f32::MAX, f32::min)
            };
            let (screen_w, screen_h) = (extent(&|v| v.pos.x) * pixels_per_point, extent(&|v| v.pos.y) * pixels_per_point);
            let (bitmap_w, bitmap_h) = (extent(&|v| v.uv.x) * atlas_w as f32, extent(&|v| v.uv.y) * atlas_h as f32);
            // The text's cursor and selection plates have no bitmap; a sliver is no glyph.
            if bitmap_w < 1.0 || bitmap_h < 1.0 {
                continue;
            }
            let ratio = (screen_w / bitmap_w).min(screen_h / bitmap_h);
            least = Some(least.map_or(ratio, |l| l.min(ratio)));
        }
    }
    least
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2, Color32, FontId, Rect};

    const LAYER: &str = "scaled_test_layer";

    /// One frame: texts in a layer scaled by `scale` (a phone's), sharpened or not; the least
    /// glyph scale on the screen.
    fn drawn(scale: f32, sharp: bool, pixels_per_point: f32) -> f32 {
        let ctx = Context::default();
        ctx.set_pixels_per_point(pixels_per_point);
        let layer = LayerId::new(egui::Order::Foreground, egui::Id::new(LAYER));
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0., 0.), vec2(844., 390.))), ..Default::default() };
        let frame = |ui: &mut egui::Ui| {
            ctx.set_transform_layer(layer, TSTransform::new(vec2(10., 10.), scale));
            let from = mark(ui.ctx(), layer);
            egui::Area::new(layer.id).order(layer.order).fixed_pos(pos2(0., 0.)).show(ui.ctx(), |ui| {
                let painter = ui.painter();
                painter.text(pos2(10., 10.), egui::Align2::LEFT_TOP, "Cut at the edge \u{2026} and a thin line: ill, 1.0", FontId::proportional(14.), Color32::WHITE);
                ui.label("A label with a wrapped second line that goes on for a while, so it breaks");
                ui.set_max_width(120.);
                ui.label("Narrow column, wrapped by the Ui width");
            });
            if sharp {
                sharpen(ui.ctx(), layer, from, scale);
            }
        };
        // An area is not drawn on its first frame (it measures itself).
        ctx.run_ui(raw.clone(), frame).textures_delta.clear();
        let mut output = ctx.run_ui(raw, frame);
        output.textures_delta.clear();
        least_glyph_scale(&ctx, output.shapes, pixels_per_point).expect("glyphs are drawn")
    }

    #[test]
    fn a_scaled_layer_shrinks_glyph_bitmaps_unless_the_text_is_laid_out_at_the_drawn_size() {
        for ppp in [1.0, 2.0, 3.0] {
            let blurred = drawn(0.85, false, ppp);
            assert!(blurred < 0.9, "the test sees the defect at {ppp}x: glyphs at {blurred}");
            let sharp = drawn(0.85, true, ppp);
            assert!(sharp > 0.97 && sharp < 1.03, "glyphs are one to one at {ppp}x, got {sharp}");
        }
    }

    #[test]
    fn the_touch_scales_stay_one_to_one() {
        // A landscape phone's canvas is about 0.85, a portrait one about 0.33.
        for scale in [0.33, 0.47, 0.6, 0.85, 1.0] {
            let sharp = drawn(scale, true, 2.0);
            assert!(sharp > 0.95, "scale {scale}: glyphs at {sharp}");
        }
    }

    #[test]
    fn the_layer_is_the_shells_only_scaled_one() {
        // Any other layer scaled by `set_transform_layer` would bring the fade back unnoticed.
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![src];
        let mut users = Vec::new();
        while let Some(path) = stack.pop() {
            for entry in std::fs::read_dir(&path).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let code = text.split("#[cfg(test)]").next().unwrap_or("");
                    users.extend(code.lines().filter(|l| l.contains("set_transform_layer(") && !l.trim_start().starts_with("//")).map(|_| path.clone()));
                }
            }
        }
        assert_eq!(users.len(), 1, "one scaled layer in the client, drawn by screens::draw_body: {users:?}");
        assert!(users[0].ends_with("shell/screens.rs"), "{users:?}");
    }
}
