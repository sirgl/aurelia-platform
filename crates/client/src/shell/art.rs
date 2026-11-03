//! Runtime-loaded generated artwork. Missing images retry slowly while assets arrive.
//! How a picture is sized and fitted is the module's knowledge (`ArtKinds`); the shell
//! knows only its menu covers. Decoding runs off the UI thread (`art_loader`); until a
//! picture is ready its frame shows a quiet placeholder.
use super::art_loader::{AtlasIndex, Decoder, Loader, Pixels};
use egui::{Color32, Context, Painter, Rect, TextureHandle};
use std::collections::HashMap;
use std::time::Duration;

/// Uploaded pictures kept (about 0.6 MB each); a pass that shows more keeps them all.
const CACHE_LIMIT: usize = 64;
/// Bytes of pictures uploaded in one pass at most (about a dozen cards): a screen opened
/// with many pictures fills in over a few frames instead of stalling one.
const UPLOAD_BUDGET: usize = 8 << 20;
/// How long a picture without art waits before it is looked for again.
const RETRY_MISSING: f64 = 2.0;

/// One art cache per egui context (`ScreenArt::shared`).
#[derive(Clone, Default)]
struct SharedArt(std::sync::Arc<std::sync::Mutex<ScreenArt>>);

/// The module's pictures that are not ordinary cards (bounded to 384 px, cropped to fill
/// their frame). Atlas regions (`sheet:item`) are always items.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArtKinds {
    /// Scenic heroes uploaded at full resolution.
    pub heroes: &'static [&'static str],
    /// Whole objects: fitted inside the frame, never cropped.
    pub items: &'static [&'static str],
    /// Groups of figures: in a wide frame the whole group shows, right-aligned.
    pub groups: &'static [&'static str],
}

impl ArtKinds {
    /// Menu covers (`menu`, a module's `<name>-menu`) fill the screen: always heroes.
    pub(super) fn hero(&self, name: &str) -> bool {
        name == "menu" || name.ends_with("-menu") || self.heroes.contains(&name)
    }
    fn item(&self, name: &str) -> bool {
        name.contains(':') || self.items.contains(&name)
    }
}

/// The pictures one egui pass asked for: drawn, still loading, or without art
/// (`--bench-art` waits for `loading == 0`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArtFrame {
    pub pass: u64,
    pub shown: u32,
    pub loading: u32,
    pub missing: u32,
    /// Bytes of pictures uploaded in the pass.
    pub uploaded: usize,
}

/// A picture asked for by name. One not in `ScreenArt::pictures` is loading.
enum Picture {
    /// Uploaded; `used` is the last egui pass that painted it.
    Ready { texture: TextureHandle, used: u64 },
    /// No art folder has it (looked for at `since`, egui time).
    Missing { since: f64 },
}

#[derive(Default)]
pub struct ScreenArt {
    kinds: ArtKinds,
    pictures: HashMap<String, Picture>,
    /// Started by the first picture painted.
    loader: Option<Loader>,
    frame: ArtFrame,
}

/// Screen art and the tint it is painted with (the player's brightness): what a widget
/// paints its pictures with.
pub struct ArtPen<'a> {
    pub art: &'a mut ScreenArt,
    pub tint: Color32,
}

impl ArtPen<'_> {
    pub fn paint(&mut self, ui: &egui::Ui, rect: Rect, name: &str) {
        self.art.paint(ui.ctx(), ui.painter(), rect, name, self.tint);
    }
}

impl ScreenArt {
    pub fn new(kinds: ArtKinds) -> Self {
        Self { kinds, ..Default::default() }
    }
    pub fn paint(
        &mut self,
        ctx: &Context,
        painter: &Painter,
        rect: Rect,
        name: &str,
        tint: Color32,
    ) {
        // A picture scrolled out of its list is neither fetched nor decoded in a page.
        #[cfg(target_arch = "wasm32")]
        if !painter.clip_rect().intersects(rect) {
            return;
        }
        let Some(texture) = self.texture(ctx, name) else {
            painter.rect_filled(rect, 0.0, Color32::from_rgb(24, 47, 53));
            let c = rect.center();
            painter.circle_stroke(
                c,
                rect.height().min(rect.width()) * 0.22,
                egui::Stroke::new(1.0, Color32::from_rgb(92, 104, 89)),
            );
            return;
        };
        let size = texture.size_vec2();
        // Atlas regions can be rectangular: item cards must retain the whole object.
        if self.kinds.item(name) {
            painter.rect_filled(rect, 0.0, Color32::from_rgb(17, 36, 43));
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            painter.image(
                texture.id(),
                Rect::from_center_size(rect.center(), size * scale),
                Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                tint,
            );
            return;
        }
        if self.kinds.groups.contains(&name) && rect.width() > rect.height() * 1.2 {
            painter.rect_filled(rect, 0.0, Color32::from_rgb(17, 36, 43));
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let image_rect = Rect::from_min_size(
                egui::pos2(rect.right() - size.x * scale, rect.top()),
                size * scale,
            );
            painter.image(
                texture.id(),
                image_rect,
                Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                tint,
            );
            return;
        }
        let ratio = (rect.width() / size.x).max(rect.height() / size.y);
        let visible = rect.size() / (size * ratio);
        let uv = Rect::from_center_size(egui::pos2(0.5, 0.5), visible);
        painter.image(texture.id(), rect, uv, tint);
    }
    fn sheet_items(sheet: &str) -> Vec<(String, String)> {
        let index = AtlasIndex::read(&super::assets::path("visual/generated"));
        let mut seen = std::collections::HashSet::new();
        index.rows()
            .filter(|(_, region)| region.sheet_name.contains(sheet) && seen.insert((region.sheet_name.clone(), region.rect)))
            .map(|(id, _)| (id.to_owned(), id.rsplit('/').next().unwrap_or(id).replace(['-', '_'], " ")))
            .collect()
    }
    pub fn preload_sheet(&mut self, ctx: &Context, sheet: &str) {
        let names = Self::sheet_items(sheet);
        self.preload(ctx, &names.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>());
    }
    /// Offscreen extraction QA, reachable only by explicit capture environment variable.
    pub fn draw_sheet_review(&mut self, ui: &mut egui::Ui, sheet: &str) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(format!("Atlas extraction · {sheet}"));
            let r = ui.available_rect_before_wrap();
            let items = Self::sheet_items(sheet);
            let rows = items.len().div_ceil(3).max(1);
            let cell = egui::vec2(r.width() / 3., r.height() / rows as f32);
            let side = (cell.y - 120.).min(cell.x - 24.).min(180.).max(24.);
            for (i, (id, name)) in items.iter().enumerate() {
                let origin = r.min + egui::vec2((i % 3) as f32 * cell.x, (i / 3) as f32 * cell.y);
                let center = origin + egui::vec2(cell.x * 0.5, side * 0.5 + 4.);
                // The same uploaded runtime crop is drawn at large, 48px and 72px
                // sizes. This QA-only layout never rewrites or resamples the source PNG.
                if let Some(t) = self.texture(&ctx, id) {
                    let size = t.size_vec2();
                    for (center, extent) in [
                        (center, side),
                        (origin + egui::vec2(cell.x * 0.5 - 48., side + 46.), 48.),
                        (origin + egui::vec2(cell.x * 0.5 + 40., side + 46.), 72.),
                    ] {
                        let fitted = size * (extent / size.x).min(extent / size.y);
                        ui.painter().image(t.id(), Rect::from_center_size(center, fitted),
                            Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.,1.)), Color32::WHITE);
                    }
                }
                for (offset, label) in [(-48., "48 px"), (40., "72 px")] {
                    ui.painter().text(origin + egui::vec2(cell.x * 0.5 + offset, side + 86.),
                        egui::Align2::CENTER_TOP, label, egui::FontId::proportional(10.), Color32::GRAY);
                }
                ui.painter().text(origin + egui::vec2(cell.x * 0.5, cell.y - 16.), egui::Align2::CENTER_TOP,
                    name, egui::FontId::proportional(14.), Color32::WHITE);
            }
        });
    }
    fn install(&mut self, ctx: &Context, name: &str, pixels: Pixels) {
        #[cfg(target_arch = "wasm32")]
        super::assets::web::trace("install", name, if pixels.is_some() { "ready" } else { "missing" });
        let picture = match pixels {
            Some(image) => {
                self.frame.uploaded += image.pixels.len() * 4;
                self.make_room(ctx.cumulative_pass_nr());
                let texture = ctx.load_texture(format!("screen art {name}"), image, egui::TextureOptions::LINEAR);
                Picture::Ready { texture, used: ctx.cumulative_pass_nr() }
            }
            None => Picture::Missing { since: ctx.input(|i| i.time) },
        };
        self.pictures.insert(name.to_owned(), picture);
    }
    /// Below `CACHE_LIMIT` uploads: the least recently painted goes, never one the pass
    /// `pass` painted (a screen showing more than the limit keeps them all).
    fn make_room(&mut self, pass: u64) {
        let ready = self.pictures.values().filter(|p| matches!(p, Picture::Ready { .. })).count();
        if ready < CACHE_LIMIT {
            return;
        }
        let oldest = self.pictures.iter()
            .filter_map(|(name, picture)| match picture {
                Picture::Ready { used, .. } if *used < pass => Some((*used, name)),
                _ => None,
            })
            .min()
            .map(|(_, name)| name.clone());
        if let Some(name) = oldest {
            self.pictures.remove(&name);
        }
    }
    /// The decoded pictures, as many as this pass's upload budget takes; the rest wait
    /// for the next pass.
    fn install_finished(&mut self, ctx: &Context) {
        while let Some(loader) = &self.loader {
            if self.frame.uploaded >= UPLOAD_BUDGET {
                ctx.request_repaint();
                return;
            }
            let Some((name, pixels)) = loader.next_finished() else { return };
            self.install(ctx, &name, pixels);
        }
    }
    /// What the last pass that painted art asked for.
    pub fn frame_stats(&self) -> ArtFrame {
        self.frame
    }
    /// The picture `name` if it is uploaded; else it is asked for (or waits to be looked
    /// for again, when no art folder had it).
    fn texture(&mut self, ctx: &Context, name: &str) -> Option<TextureHandle> {
        if skip_offline(ctx, name) {
            return None;
        }
        self.begin_pass(ctx);
        self.install_finished(ctx);
        if let Some(texture) = self.ready(ctx, name) {
            #[cfg(target_arch = "wasm32")]
            super::assets::web::trace_once("shown", name);
            self.frame.shown += 1;
            return Some(texture);
        }
        if self.recently_missing(ctx, name) {
            self.frame.missing += 1;
            return None;
        }
        self.frame.loading += 1;
        #[cfg(target_arch = "wasm32")]
        super::assets::web::trace_once("ask", name);
        self.loader.as_ref()?.want(name);
        None
    }
    /// The first picture of a pass starts its count and drops what earlier passes asked
    /// for (the decoders start with the first picture painted).
    fn begin_pass(&mut self, ctx: &Context) {
        let pass = ctx.cumulative_pass_nr();
        let loader = self.loader.get_or_insert_with(|| Loader::start(Decoder::of(ctx, self.kinds), ctx));
        if self.frame.pass != pass {
            self.frame = ArtFrame { pass, ..ArtFrame::default() };
            loader.begin_pass();
        }
    }
    fn ready(&mut self, ctx: &Context, name: &str) -> Option<TextureHandle> {
        match self.pictures.get_mut(name)? {
            Picture::Ready { texture, used } => {
                *used = ctx.cumulative_pass_nr();
                Some(texture.clone())
            }
            Picture::Missing { .. } => None,
        }
    }
    /// No art folder had `name` less than `RETRY_MISSING` ago; after that it is forgotten
    /// and looked for again.
    fn recently_missing(&mut self, ctx: &Context, name: &str) -> bool {
        let Some(Picture::Missing { since }) = self.pictures.get(name) else { return false };
        let waited = ctx.input(|i| i.time) - since;
        if waited < RETRY_MISSING {
            ctx.request_repaint_after(Duration::from_secs_f64(RETRY_MISSING - waited));
            return true;
        }
        self.pictures.remove(name);
        false
    }
    /// Paint `name` whole inside `rect`, centred (icons, emblems); false while it loads
    /// or when it is missing, so the caller can draw its own stand-in.
    pub fn paint_fit(&mut self, ctx: &Context, painter: &Painter, rect: Rect, name: &str, tint: Color32) -> bool {
        #[cfg(target_arch = "wasm32")]
        if !painter.clip_rect().intersects(rect) {
            return false;
        }
        let Some(texture) = self.texture(ctx, name) else { return false };
        let size = texture.size_vec2();
        let scale = (rect.width() / size.x).min(rect.height() / size.y);
        let full = Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
        painter.image(texture.id(), Rect::from_center_size(rect.center(), size * scale), full, tint);
        true
    }

    /// Paint the frame `slice` over `rect` (`kit::nine_slice`); false while its art
    /// loads or when it is missing.
    pub fn paint_nine_slice(&mut self, ctx: &Context, painter: &Painter, rect: Rect, slice: &super::theme::NineSlice, tint: Color32) -> bool {
        #[cfg(target_arch = "wasm32")]
        if !painter.clip_rect().intersects(rect) {
            return false;
        }
        let Some(texture) = self.texture(ctx, slice.art) else { return false };
        super::kit::nine_slice(painter, texture.id(), rect, slice, tint);
        true
    }

    /// The art cache of `ctx` shared by drawing code that keeps no state of its own
    /// (HUD icons from art, frames of the shell's panels).
    pub fn shared(ctx: &Context) -> std::sync::Arc<std::sync::Mutex<ScreenArt>> {
        let id = egui::Id::new("shared_screen_art");
        ctx.data_mut(|data| data.get_temp_mut_or_default::<SharedArt>(id).0.clone())
    }

    /// Explicit synchronous warmup for headless QA only. Live screens load off-thread.
    pub fn preload(&mut self, ctx: &Context, names: &[&str]) {
        let decoder = Decoder::of(ctx, self.kinds);
        for name in names { self.install(ctx, name, decoder.decode(name)); }
    }

}

/// The pictures a context without art was asked for (`stay_offline`).
#[derive(Clone, Default)]
struct OfflineArt(std::sync::Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>);

fn offline_id() -> egui::Id {
    egui::Id::new("screen_art_offline")
}

/// `ctx` loads no art from now on: every picture paints its stand-in, so a frame never
/// depends on which pictures a decoder thread finished (text snapshots,
/// `snapshot::Stage`). The names asked for are kept (`take_offline_requests`).
pub fn stay_offline(ctx: &Context) {
    ctx.data_mut(|data| data.insert_temp(offline_id(), OfflineArt::default()));
}

/// True when `ctx` loads no art (`stay_offline`); `name` is then noted as asked for. Every
/// loader of pictures asks this before it decodes anything.
pub fn skip_offline(ctx: &Context, name: &str) -> bool {
    let Some(offline) = ctx.data(|data| data.get_temp::<OfflineArt>(offline_id())) else {
        return false;
    };
    offline.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(name.to_owned());
    true
}

/// The pictures asked for since the last call, sorted (empty when `ctx` loads art).
pub fn take_offline_requests(ctx: &Context) -> Vec<String> {
    let Some(offline) = ctx.data(|data| data.get_temp::<OfflineArt>(offline_id())) else {
        return Vec::new();
    };
    let mut names = offline.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    std::mem::take(&mut *names).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: ArtKinds = ArtKinds { heroes: &["research"], items: &["mining", "warrior"], groups: &[] };

    fn ready(art: &ScreenArt, name: &str) -> Option<egui::TextureId> {
        match art.pictures.get(name) {
            Some(Picture::Ready { texture, .. }) => Some(texture.id()),
            _ => None,
        }
    }

    #[test]
    fn item_cards_keep_full_uvs_and_aspect_while_heroes_fill_the_frame() {
        for (width, height) in [(418, 370), (418, 482)] {
            for name in ["sheet:radar", "mining", "warrior", "research"] {
                let ctx = Context::default();
                let mut art = ScreenArt::new(KINDS);
                art.install(&ctx, name, Some(egui::ColorImage::filled(
                    [width, height], Color32::WHITE,
                )));
                let texture = ready(&art, name).unwrap();
                let rect = Rect::from_min_size(egui::pos2(20., 30.), egui::vec2(96., 96.));
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    art.paint(ui.ctx(), ui.painter(), rect, name, Color32::WHITE);
                });
                let mesh = output.shapes.iter().find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh),
                    _ => None,
                }).expect("painted item texture");
                let bounds = mesh.calc_bounds();
                assert!(rect.contains_rect(bounds));
                if KINDS.item(name) {
                    assert!((bounds.aspect_ratio() - width as f32 / height as f32).abs() < 0.001);
                    assert!(mesh.vertices.iter().any(|v| v.uv == egui::pos2(0., 0.)));
                    assert!(mesh.vertices.iter().any(|v| v.uv == egui::pos2(1., 1.)));
                } else {
                    assert_eq!(bounds, rect);
                    assert!(!mesh.vertices.iter().any(|v| v.uv == egui::pos2(0., 0.)));
                }
                output.textures_delta.clear();
                ctx.tex_manager().write().take_delta().clear();
            }
        }
    }

    #[test]
    fn card_upload_is_bounded_but_hero_resolution_is_preserved() {
        use super::super::art_loader::bounded;
        assert!(!KINDS.hero("warrior") && !KINDS.hero("portrait") && KINDS.hero("research"));
        assert_eq!(bounded(image::DynamicImage::new_rgba8(1254, 1254)).dimensions(), (384, 384));
        assert_eq!(bounded(image::DynamicImage::new_rgba8(800, 1200)).dimensions(), (256, 384));
        assert!(KINDS.hero("menu") && KINDS.hero("game-menu"), "menu covers are heroes for every module");
        assert!(!ArtKinds::default().hero("research"));
    }

    #[test]
    fn module_art_root_is_searched_before_the_shared_art() {
        let dir = std::env::temp_dir().join(format!("aurelia-art-root-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbaImage::new(3, 2).save(dir.join("module-only.png")).unwrap();
        let root: &'static str = Box::leak(dir.to_string_lossy().into_owned().into_boxed_str());
        let theme: &'static crate::shell::theme::Theme =
            Box::leak(Box::new(crate::shell::theme::Theme { art_root: Some(root), ..crate::shell::theme::Theme::AURELIA }));
        let ctx = Context::default();
        let mut art = ScreenArt::default();
        art.preload(&ctx, &["module-only"]);
        assert!(ready(&art, "module-only").is_none(), "the shell's own theme has no module folder");
        crate::shell::theme::install(&ctx, theme);
        art.preload(&ctx, &["module-only"]);
        assert!(ready(&art, "module-only").is_some(), "found in the module folder");
        ctx.tex_manager().write().take_delta().clear();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn art_icons_fit_their_square_and_ring_while_missing() {
        let ctx = Context::default();
        let shared = ScreenArt::shared(&ctx);
        shared.lock().unwrap().install(&ctx, "emblem", Some(egui::ColorImage::filled([40, 20], Color32::WHITE)));
        let texture = ready(&shared.lock().unwrap(), "emblem").unwrap();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let c = egui::pos2(100., 100.);
            crate::shell::hud::icon(ui.painter(), c, 10., crate::shell::hud::Icon::Art("emblem"), Color32::RED);
            crate::shell::hud::icon(ui.painter(), c, 10., crate::shell::hud::Icon::Art("missing"), Color32::RED);
        });
        output.textures_delta.clear();
        ctx.tex_manager().write().take_delta().clear();
        let image = output.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.calc_bounds()),
            _ => None,
        });
        assert_eq!(image, Some(Rect::from_center_size(egui::pos2(100., 100.), egui::vec2(20., 10.))));
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Circle(c) if c.stroke.color == Color32::RED)));
    }

    #[test]
    fn a_full_cache_drops_the_least_recently_painted_but_never_this_pass() {
        let ctx = Context::default();
        let mut art = ScreenArt::default();
        let image = || Some(egui::ColorImage::filled([2, 2], Color32::WHITE));
        for i in 0..=CACHE_LIMIT {
            art.install(&ctx, &format!("p{i}"), image());
        }
        let count = |art: &ScreenArt| art.pictures.values().filter(|p| matches!(p, Picture::Ready { .. })).count();
        assert_eq!(count(&art), CACHE_LIMIT + 1, "one pass showing more than the limit keeps them all");
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        for (name, picture) in &mut art.pictures {
            if let Picture::Ready { used, .. } = picture {
                *used = if name == "p7" { 0 } else { 1 };
            }
        }
        art.install(&ctx, "late", image());
        assert!(ready(&art, "p7").is_none() && ready(&art, "late").is_some());
        assert_eq!(count(&art), CACHE_LIMIT + 1);
        ctx.tex_manager().write().take_delta().clear();
    }

    #[test]
    fn pictures_without_art_count_as_missing_not_loading() {
        let ctx = Context::default();
        let mut art = ScreenArt::default();
        art.install(&ctx, "gone", None);
        art.install(&ctx, "here", Some(egui::ColorImage::filled([2, 2], Color32::WHITE)));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(10., 10.));
            for name in ["gone", "here", "never-asked-before"] {
                art.paint(ui.ctx(), ui.painter(), rect, name, Color32::WHITE);
            }
        });
        output.textures_delta.clear();
        ctx.tex_manager().write().take_delta().clear();
        let frame = art.frame_stats();
        assert_eq!((frame.shown, frame.loading, frame.missing), (1, 1, 1));
    }

    #[test]
    fn cached_art_reuses_texture_without_new_upload() {
        let ctx = Context::default();
        let mut art = ScreenArt::default();
        let texture = ctx.load_texture(
            "cached",
            egui::ColorImage::filled([2, 2], Color32::WHITE),
            egui::TextureOptions::LINEAR,
        );
        let id = texture.id();
        art.pictures.insert("cached".into(), Picture::Ready { texture, used: 0 });
        ctx.tex_manager().write().take_delta().clear();
        assert_eq!(art.texture(&ctx, "cached").unwrap().id(), id);
        assert_eq!(art.texture(&ctx, "cached").unwrap().id(), id);
        let mut delta = ctx.tex_manager().write().take_delta();
        let no_upload = delta.set.is_empty();
        delta.clear();
        assert!(no_upload);
    }
}
