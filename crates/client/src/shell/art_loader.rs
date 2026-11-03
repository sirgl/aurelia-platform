//! Screen art decoded off the UI thread (`art::ScreenArt` asks, this module decodes).
//!
//! Why it is shaped so: a screen with many pictures scrolled fast asks for dozens of
//! pictures a second, each cut from a 1254 px source. One thread decoding them in the
//! order asked, re-reading the atlas index and re-decoding a whole sheet for each region,
//! showed most slots empty for seconds. Now:
//! - a small pool of threads takes only what the latest painted pass still misses
//!   (`Loader::begin_pass` drops the rest), so pictures scrolled away are never decoded;
//! - each art folder's atlas index is parsed once (`AtlasIndex`);
//! - a sheet decodes once and the threads cutting regions from it share it (`Sheets`);
//! - a bounded card picture is kept on disk after its first decode (`Thumbnails`), so the
//!   next run reads a small file instead of the 1254 px source. The stored pixels are the
//!   decoded ones, losslessly, so a picture looks the same from either.
use super::art::ArtKinds;
use egui::{ColorImage, Context};
use image::{DynamicImage, RgbaImage};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// A decoded picture, or `None` when no art folder has it.
pub(super) type Pixels = Option<ColorImage>;

/// The longest side of an uploaded card picture; scenic heroes keep their resolution.
const CARD_BOUND: u32 = 384;
/// Decoded atlas sheets kept for the regions still to cut (about 6 MB each).
const SHEETS: usize = 6;
/// Bump when the stored thumbnails' pixels would change.
const THUMBNAIL_FORMAT: u32 = 1;

#[cfg(target_arch = "wasm32")]
pub(super) use inline::Loader;
#[cfg(not(target_arch = "wasm32"))]
pub(super) use threaded::Loader;

#[cfg(not(target_arch = "wasm32"))]
mod threaded {
    use super::*;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::collections::HashSet;
    use std::sync::Condvar;

    /// The decoding threads of one `ScreenArt` and the names they are to decode.
    pub(in super::super) struct Loader {
        pub(in super::super) queue: Arc<Queue>,
        results: Receiver<(String, Pixels)>,
    }

    #[derive(Default)]
    pub(in super::super) struct Queue {
        state: Mutex<QueueState>,
        wake: Condvar,
    }

    #[derive(Default)]
    pub(in super::super) struct QueueState {
        /// Asked for by the current pass, in paint order, not started yet.
        pub(in super::super) waiting: VecDeque<String>,
        /// Being decoded now.
        busy: HashSet<String>,
        closed: bool,
    }

    impl Loader {
        /// Threads decoding with `decoder`; each result repaints `ctx`.
        pub(in super::super) fn start(decoder: Decoder, ctx: &Context) -> Self {
            let queue = Arc::new(Queue::default());
            let decoder = Arc::new(decoder);
            let (send, results) = mpsc::channel();
            for _ in 0..threads() {
                let (queue, decoder, send, ctx) =
                    (queue.clone(), decoder.clone(), send.clone(), ctx.clone());
                std::thread::spawn(move || work(&queue, &decoder, &send, &ctx));
            }
            Self { queue, results }
        }

        /// A new pass paints: what earlier passes asked for and no thread took is dropped;
        /// the new pass asks again for what it still shows.
        pub(in super::super) fn begin_pass(&self) {
            self.queue.lock().waiting.clear();
        }

        /// Decode `name` after the names asked for before it in this pass.
        pub(in super::super) fn want(&self, name: &str) {
            let mut state = self.queue.lock();
            if state.busy.contains(name) || state.waiting.iter().any(|n| n == name) {
                return;
            }
            state.waiting.push_back(name.to_owned());
            drop(state);
            self.queue.wake.notify_one();
        }

        /// A picture decoded and not yet taken.
        pub(in super::super) fn next_finished(&self) -> Option<(String, Pixels)> {
            self.results.try_recv().ok()
        }
    }

    impl Drop for Loader {
        fn drop(&mut self) {
            self.queue.lock().closed = true;
            self.queue.wake.notify_all();
        }
    }

    impl Queue {
        pub(in super::super) fn lock(&self) -> std::sync::MutexGuard<'_, QueueState> {
            self.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        /// The next name to decode, or `None` once the loader is gone.
        fn take(&self) -> Option<String> {
            let mut state = self.lock();
            loop {
                if state.closed {
                    return None;
                }
                if let Some(name) = state.waiting.pop_front() {
                    state.busy.insert(name.clone());
                    return Some(name);
                }
                state = self
                    .wake
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }
    }

    /// A quarter of the cores, two to four: decoding keeps up with a scroll and leaves the
    /// rest to the game.
    fn threads() -> usize {
        std::thread::available_parallelism().map_or(2, |n| (n.get() / 4).clamp(2, 4))
    }

    fn work(
        queue: &Queue,
        decoder: &Decoder,
        results: &Sender<(String, Pixels)>,
        repaint: &Context,
    ) {
        while let Some(name) = queue.take() {
            let pixels = decoder.decode(&name);
            // Sent before it stops being busy: the UI never asks again for a picture in between.
            let sent = results.send((name.clone(), pixels));
            queue.lock().busy.remove(&name);
            if sent.is_err() {
                return;
            }
            repaint.request_repaint();
        }
    }
}

/// The loader of a browser page, which has no threads: the page's art service (`lands/launcher/web/static/art.js`)
/// fetches the picture files and decodes them with the browser's own decoder, off this thread,
/// in the order of `PictureRequest::priority` and of the screen. A finished picture comes back as
/// RGBA pixels and asks for a repaint; the frame takes it (`next_finished`) within the upload budget.
#[cfg(target_arch = "wasm32")]
mod inline {
    use super::*;
    use crate::shell::assets::web::{self, PictureRequest};

    pub(in super::super) struct Loader {
        decoder: Decoder,
        ctx: Context,
        /// Names asked for and not finished: the request each one is waiting for.
        waiting: Mutex<Vec<(String, PictureRequest)>>,
        /// Names the decoder cannot place (no art folder has them): finished at once, as missing.
        missing: Mutex<VecDeque<String>>,
    }

    impl Loader {
        pub(in super::super) fn start(decoder: Decoder, ctx: &Context) -> Self {
            web::set_repaint(ctx);
            Self {
                decoder,
                ctx: ctx.clone(),
                waiting: Mutex::default(),
                missing: Mutex::default(),
            }
        }

        pub(in super::super) fn begin_pass(&self) {
            web::begin_picture_pass(self.ctx.cumulative_pass_nr());
        }

        pub(in super::super) fn want(&self, name: &str) {
            let mut waiting = self.waiting.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some((_, request)) = waiting.iter().find(|(n, _)| n == name) {
                web::want_picture(request);
                return;
            }
            let Some(request) = self.decoder.request(name) else {
                self.missing.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_back(name.to_owned());
                self.ctx.request_repaint();
                return;
            };
            web::want_picture(&request);
            waiting.push((name.to_owned(), request));
        }

        pub(in super::super) fn next_finished(&self) -> Option<(String, Pixels)> {
            if let Some(name) = self.missing.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).pop_front() {
                return Some((name, None));
            }
            let mut waiting = self.waiting.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let at = waiting.iter().position(|(_, request)| web::has_picture(&request.key))?;
            let (name, request) = waiting.remove(at);
            let pixels = web::take_picture(&request.key)?;
            Some((name, pixels))
        }
    }
}

/// Finds and decodes pictures by name: in the module's art folder first, then the
/// shared generated art. Safe to share between threads.
pub(super) struct Decoder {
    folders: Vec<ArtFolder>,
    kinds: ArtKinds,
    sheets: Sheets,
    thumbnails: Option<Thumbnails>,
}

/// An art folder and its atlas index (`runtime-atlas-index.tsv`).
struct ArtFolder {
    root: PathBuf,
    index: AtlasIndex,
}

/// Where a picture's pixels come from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    File(PathBuf),
    Region(Region),
}

impl Decoder {
    /// The art folders of the theme installed in `ctx`, thumbnails kept in the user's
    /// cache folder.
    pub(super) fn of(ctx: &Context, kinds: ArtKinds) -> Self {
        let module = super::theme::Theme::of(ctx)
            .art_root
            .map(super::assets::path);
        let roots = module
            .into_iter()
            .chain([super::assets::path("visual/generated")]);
        Self::new(roots.collect(), kinds, Thumbnails::user())
    }

    fn new(roots: Vec<PathBuf>, kinds: ArtKinds, thumbnails: Option<Thumbnails>) -> Self {
        let folders = roots
            .into_iter()
            .map(|root| ArtFolder {
                index: AtlasIndex::read(&root),
                root,
            })
            .collect();
        Self {
            folders,
            kinds,
            sheets: Sheets::default(),
            thumbnails,
        }
    }

    pub(super) fn decode(&self, name: &str) -> Pixels {
        let hero = self.kinds.hero(name);
        let image = self.folders.iter().find_map(|folder| {
            let source = folder.source(name)?;
            if hero {
                self.full(&source)
            } else {
                self.card(&source)
            }
        })?;
        Some(ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ))
    }

    /// What the browser's art service is asked for to make `name` (None: no art folder has it).
    #[cfg(target_arch = "wasm32")]
    pub(super) fn request(&self, name: &str) -> Option<crate::shell::assets::web::PictureRequest> {
        use crate::shell::assets::web;
        let hero = self.kinds.hero(name);
        let (file, rect) = self.folders.iter().find_map(|folder| match folder.source(name)? {
            Source::File(path) => Some((path, None)),
            Source::Region(region) => Some((region.sheet, Some(region.rect))),
        })?;
        let path = web::key(&super::assets::image_file(&file));
        let max_side = if hero { 0 } else { CARD_BOUND };
        Some(web::PictureRequest::new(path, rect, max_side, hero))
    }

    fn full(&self, source: &Source) -> Option<RgbaImage> {
        Some(self.open(source)?.to_rgba8())
    }

    /// A bounded card picture: from the stored thumbnail, else decoded and stored.
    fn card(&self, source: &Source) -> Option<RgbaImage> {
        let stored = self.thumbnails.as_ref().map(|t| t.entry(source));
        if let Some(image) = stored.as_ref().and_then(Thumbnail::load) {
            return Some(image);
        }
        let image = bounded(self.open(source)?);
        if let Some(entry) = stored {
            entry.save(&image);
        }
        Some(image)
    }

    fn open(&self, source: &Source) -> Option<DynamicImage> {
        match source {
            Source::File(path) => super::assets::open_image(path),
            Source::Region(region) => {
                let sheet = self.sheets.get(&region.sheet)?;
                let [x, y, width, height] = region.rect;
                let inside = x.checked_add(width)? <= sheet.width()
                    && y.checked_add(height)? <= sheet.height();
                (width > 0 && height > 0 && inside).then(|| sheet.crop_imm(x, y, width, height))
            }
        }
    }
}

impl ArtFolder {
    /// `sheet:item` names are atlas regions; any other is `<name>-v1.png` or `<name>.png`.
    fn source(&self, name: &str) -> Option<Source> {
        if name.contains(':') {
            return self.index.region(name).cloned().map(Source::Region);
        }
        [format!("{name}-v1.png"), format!("{name}.png")]
            .into_iter()
            .map(|file| self.root.join(file))
            .find(|path| super::assets::image_exists(path))
            .map(Source::File)
    }
}

// Card images remain crisp at UI scale without uploading multi-megapixel originals.
pub(super) fn bounded(decoded: DynamicImage) -> RgbaImage {
    decoded.thumbnail(CARD_BOUND, CARD_BOUND).to_rgba8()
}

/// A region of an atlas sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Region {
    pub(super) sheet: PathBuf,
    /// The sheet's path as the index writes it (relative to the art folder).
    pub(super) sheet_name: String,
    /// x, y, width, height in the sheet's pixels.
    pub(super) rect: [u32; 4],
}

/// An art folder's `runtime-atlas-index.tsv`: `id path x y width height` rows. The first
/// row of an id wins.
#[derive(Debug, Default)]
pub(super) struct AtlasIndex {
    rows: Vec<(String, Region)>,
    by_id: HashMap<String, usize>,
}

impl AtlasIndex {
    pub(super) fn read(root: &Path) -> Self {
        let text = super::assets::read_to_string(&root.join("runtime-atlas-index.tsv"))
            .unwrap_or_default();
        Self::parse(root, &text)
    }

    fn parse(root: &Path, text: &str) -> Self {
        let mut index = Self::default();
        for line in text.lines().skip(1) {
            let columns: Vec<_> = line.split('\t').collect();
            let [id, sheet, x, y, width, height] = columns[..] else {
                continue;
            };
            let Ok(rect) = [x, y, width, height]
                .map(str::parse::<u32>)
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
            else {
                continue;
            };
            let region = Region {
                sheet: root.join(sheet),
                sheet_name: sheet.to_owned(),
                rect: [rect[0], rect[1], rect[2], rect[3]],
            };
            index.by_id.entry(id.to_owned()).or_insert(index.rows.len());
            index.rows.push((id.to_owned(), region));
        }
        index
    }

    fn region(&self, id: &str) -> Option<&Region> {
        self.by_id.get(id).map(|&row| &self.rows[row].1)
    }

    /// Every row, in the file's order.
    pub(super) fn rows(&self) -> impl Iterator<Item = (&str, &Region)> {
        self.rows.iter().map(|(id, region)| (id.as_str(), region))
    }
}

/// Atlas sheets decoded once and shared by the threads cutting regions from them; the
/// most recently used `SHEETS` stay. Threads asking for a sheet another one is decoding
/// wait for it instead of decoding it again.
#[derive(Default)]
struct Sheets(Mutex<VecDeque<(PathBuf, Arc<OnceLock<Option<Arc<DynamicImage>>>>)>>);

impl Sheets {
    fn get(&self, path: &Path) -> Option<Arc<DynamicImage>> {
        let slot = {
            let mut sheets = self
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let slot = match sheets.iter().position(|(p, _)| p == path) {
                Some(at) => sheets.remove(at).map(|(_, slot)| slot).unwrap_or_default(),
                None => Arc::default(),
            };
            sheets.push_back((path.to_owned(), slot.clone()));
            if sheets.len() > SHEETS {
                sheets.pop_front();
            }
            slot
        };
        slot.get_or_init(|| super::assets::open_image(path).map(Arc::new))
            .clone()
    }
}

/// Bounded card pictures kept on disk between runs. A thumbnail is named by its source
/// file's path, size and modification time and the region cut from it, so changed art
/// misses and is decoded anew. Every failure only means decoding from the source.
pub(super) struct Thumbnails {
    dir: PathBuf,
}

/// One source's stored thumbnail.
struct Thumbnail {
    path: PathBuf,
}

impl Thumbnails {
    pub(super) fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// `$AURELIA_ART_CACHE`, else `art` in the platform's cache folder
    /// (`~/Library/Caches/Aurelia`, `%LOCALAPPDATA%\Aurelia`, `$XDG_CACHE_HOME/aurelia`
    /// or `~/.cache/aurelia`).
    fn user() -> Option<Self> {
        if let Some(dir) = std::env::var_os("AURELIA_ART_CACHE") {
            return Some(Self::at(PathBuf::from(dir)));
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let base = if cfg!(target_os = "macos") {
            home?.join("Library/Caches/Aurelia")
        } else if cfg!(target_os = "windows") {
            PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Aurelia")
        } else {
            match std::env::var_os("XDG_CACHE_HOME") {
                Some(dir) => PathBuf::from(dir).join("aurelia"),
                None => home?.join(".cache/aurelia"),
            }
        };
        Some(Self::at(base.join("art")))
    }

    fn entry(&self, source: &Source) -> Thumbnail {
        let (file, rect) = match source {
            Source::File(path) => (path, [0; 4]),
            Source::Region(region) => (&region.sheet, region.rect),
        };
        let file = &super::assets::image_file(file);
        let stamp = std::fs::metadata(file).ok().map(|m| {
            let modified = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
            (m.len(), modified.map_or(0, |d| d.as_nanos()))
        });
        let key = format!(
            "{THUMBNAIL_FORMAT}|{CARD_BOUND}|{}|{stamp:?}|{rect:?}",
            file.display()
        );
        Thumbnail {
            path: self.dir.join(format!("{:016x}.png", fnv1a(key.as_bytes()))),
        }
    }
}

impl Thumbnail {
    fn load(&self) -> Option<RgbaImage> {
        image::open(&self.path).ok().map(DynamicImage::into_rgba8)
    }

    /// Written aside and renamed into place: a reader never sees half a file.
    fn save(&self, image: &RgbaImage) {
        use image::ImageEncoder;
        let Some(dir) = self.path.parent() else {
            return;
        };
        let mut bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new_with_quality(
            &mut bytes,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::Adaptive,
        );
        if encoder
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                image::ExtendedColorType::Rgba8,
            )
            .is_err()
        {
            return;
        }
        let aside = self.path.with_extension(format!(
            "{}.{:?}.tmp",
            std::process::id(),
            std::thread::current().id()
        ));
        let written = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&aside, &bytes));
        if written
            .and_then(|()| std::fs::rename(&aside, &self.path))
            .is_err()
        {
            let _ = std::fs::remove_file(&aside);
        }
    }
}

/// FNV-1a: a stable 64-bit name for a thumbnail key.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurelia-art-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A 600×300 sheet with a gradient, two regions in the index.
    fn art_folder(dir: &Path) {
        let sheet = RgbaImage::from_fn(600, 300, |x, y| {
            image::Rgba([
                (x % 251) as u8,
                (y % 241) as u8,
                ((x + y) % 7) as u8 * 30,
                255,
            ])
        });
        std::fs::create_dir_all(dir.join("sheets")).unwrap();
        sheet.save(dir.join("sheets/a.png")).unwrap();
        RgbaImage::from_pixel(800, 500, image::Rgba([9, 8, 7, 255]))
            .save(dir.join("card-v1.png"))
            .unwrap();
        let index = "id\tpath\tx\ty\twidth\theight\nsheet:left\tsheets/a.png\t0\t0\t300\t300\nsheet:left\tsheets/a.png\t1\t1\t2\t2\nsheet:right\tsheets/a.png\t300\t0\t300\t300\nsheet:outside\tsheets/a.png\t500\t0\t300\t300\nbroken\tsheets/a.png\tx\t0\t1\t1\n";
        std::fs::write(dir.join("runtime-atlas-index.tsv"), index).unwrap();
    }

    #[test]
    fn the_index_keeps_the_first_row_of_an_id_and_skips_broken_rows() {
        let index = AtlasIndex::parse(Path::new("/art"), "id\tpath\tx\ty\twidth\theight\na:b\ts.png\t1\t2\t3\t4\na:b\ts.png\t9\t9\t9\t9\nbad\ts.png\t1\n");
        let region = index.region("a:b").unwrap();
        assert_eq!(region.rect, [1, 2, 3, 4]);
        assert_eq!(region.sheet, Path::new("/art/s.png"));
        assert!(index.region("bad").is_none());
        assert_eq!(index.rows().count(), 2);
    }

    #[test]
    fn stored_thumbnails_give_the_pixels_of_a_fresh_decode() {
        let art = temp_dir("store-art");
        let cache = temp_dir("store-cache");
        art_folder(&art);
        let fresh = Decoder::new(vec![art.clone()], ArtKinds::default(), None);
        let storing = Decoder::new(
            vec![art.clone()],
            ArtKinds::default(),
            Some(Thumbnails::at(cache.clone())),
        );
        for name in ["sheet:left", "sheet:right", "card"] {
            let expected = fresh.decode(name).expect("decoded from the source");
            assert_eq!(
                storing.decode(name).as_ref(),
                Some(&expected),
                "{name}: first decode stores"
            );
            let stored = Decoder::new(
                vec![art.clone()],
                ArtKinds::default(),
                Some(Thumbnails::at(cache.clone())),
            );
            assert_eq!(
                stored.decode(name).as_ref(),
                Some(&expected),
                "{name}: read back from the store"
            );
        }
        assert_eq!(
            std::fs::read_dir(&cache).unwrap().count(),
            3,
            "one thumbnail per source, no leftovers"
        );
        assert_eq!(fresh.decode("card").map(|c| c.size), Some([384, 240]));
        assert!(
            fresh.decode("sheet:outside").is_none(),
            "a region past the sheet's edge is no picture"
        );
        assert!(fresh.decode("missing").is_none());
        std::fs::remove_dir_all(art).unwrap();
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_changed_source_misses_its_old_thumbnail() {
        let art = temp_dir("changed-art");
        let cache = temp_dir("changed-cache");
        art_folder(&art);
        let decoder = Decoder::new(
            vec![art.clone()],
            ArtKinds::default(),
            Some(Thumbnails::at(cache.clone())),
        );
        assert!(decoder.decode("card").is_some());
        RgbaImage::from_pixel(300, 300, image::Rgba([200, 0, 0, 255]))
            .save(art.join("card-v1.png"))
            .unwrap();
        let changed = decoder.decode("card").unwrap();
        assert_eq!(
            (changed.size, changed.pixels[0]),
            ([384, 384], egui::Color32::from_rgb(200, 0, 0))
        );
        std::fs::remove_dir_all(art).unwrap();
        std::fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn the_pool_decodes_what_the_latest_pass_wants() {
        let art = temp_dir("pool-art");
        art_folder(&art);
        let ctx = Context::default();
        let loader = Loader::start(
            Decoder::new(vec![art.clone()], ArtKinds::default(), None),
            &ctx,
        );
        for name in ["sheet:left", "sheet:right", "card", "missing", "card"] {
            loader.want(name);
        }
        let mut done = HashMap::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while done.len() < 4 && std::time::Instant::now() < deadline {
            done.extend(loader.next_finished());
            std::thread::yield_now();
        }
        assert_eq!(
            done.len(),
            4,
            "each wanted name decoded once: {:?}",
            done.keys()
        );
        assert!(done["missing"].is_none() && done["card"].is_some());
        loader.begin_pass();
        assert!(loader.queue.lock().waiting.is_empty());
        std::fs::remove_dir_all(art).unwrap();
    }
}
