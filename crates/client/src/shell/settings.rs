//! Player settings persisted between runs: display, graphics quality, sound
//! and autosave. Stored as `key = value` lines in `settings.cfg` under the
//! config directory; unknown keys and bad values fall back to defaults, so an
//! old or damaged file never stops the game from starting.
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowMode {
    Windowed,
    /// Borderless fullscreen on the current monitor.
    Fullscreen,
}

/// Graphics presets over what the renderer exposes: terrain level of detail
/// and animation. Shadows are fixed by the renderer; the render scale and
/// anti-aliasing have settings of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Far terrain coarser, animation frozen: the map redraws only on input.
    Low,
    /// Far terrain coarser, animated water and rivers.
    Medium,
    /// Full terrain detail everywhere, animated.
    High,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Low, Quality::Medium, Quality::High];

    /// The value in `settings.cfg`.
    pub fn key(self) -> &'static str {
        match self {
            Quality::Low => "low",
            Quality::Medium => "medium",
            Quality::High => "high",
        }
    }

    pub fn terrain_lod(self) -> bool {
        self != Quality::High
    }

    pub fn animated(self) -> bool {
        self != Quality::Low
    }
}

/// Fraction of the window's pixels the 3D map is drawn at before it is stretched to
/// the window (the interface always stays sharp).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderScale {
    /// Chosen from the graphics card and the window size.
    #[default]
    Auto,
    Full,
    ThreeQuarters,
    TwoThirds,
    Half,
}

impl RenderScale {
    pub const ALL: [RenderScale; 5] =
        [RenderScale::Auto, RenderScale::Full, RenderScale::ThreeQuarters, RenderScale::TwoThirds, RenderScale::Half];

    /// The value in `settings.cfg`.
    pub fn key(self) -> &'static str {
        match self {
            RenderScale::Auto => "auto",
            RenderScale::Full => "100",
            RenderScale::ThreeQuarters => "75",
            RenderScale::TwoThirds => "67",
            RenderScale::Half => "50",
        }
    }

    /// The fixed fraction (None: Auto).
    pub fn fraction(self) -> Option<f32> {
        match self {
            RenderScale::Auto => None,
            RenderScale::Full => Some(1.0),
            RenderScale::ThreeQuarters => Some(0.75),
            RenderScale::TwoThirds => Some(2.0 / 3.0),
            RenderScale::Half => Some(0.5),
        }
    }
}

/// Multisample anti-aliasing of the 3D map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AntiAliasing {
    /// Chosen from the graphics card.
    #[default]
    Auto,
    Off,
    /// Four samples per pixel.
    X4,
}

impl AntiAliasing {
    pub const ALL: [AntiAliasing; 3] = [AntiAliasing::Auto, AntiAliasing::Off, AntiAliasing::X4];

    /// The value in `settings.cfg`.
    pub fn key(self) -> &'static str {
        match self {
            AntiAliasing::Auto => "auto",
            AntiAliasing::Off => "off",
            AntiAliasing::X4 => "4x",
        }
    }

    /// Samples per pixel (None: Auto).
    pub fn samples(self) -> Option<u32> {
        match self {
            AntiAliasing::Auto => None,
            AntiAliasing::Off => Some(1),
            AntiAliasing::X4 => Some(4),
        }
    }
}

/// Pixels of the 3D scene `Auto` aims at on a slow graphics card.
pub const AUTO_SCENE_PIXELS: f32 = 2.0e6;

/// Whether the graphics card is one `Auto` spares: integrated into the processor or in
/// software. Discrete and other devices get full quality.
pub fn is_constrained(device: wgpu::DeviceType) -> bool {
    matches!(device, wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::Cpu)
}

/// `Auto` render scale for a window of `size` pixels: about `AUTO_SCENE_PIXELS` pixels
/// of scene, in steps of 0.05, between one half and one.
pub fn auto_render_scale(size: (u32, u32)) -> f32 {
    auto_render_scale_for(size, AUTO_SCENE_PIXELS)
}

/// The same for a card that should draw about `target` pixels of scene.
pub fn auto_render_scale_for(size: (u32, u32), target: f32) -> f32 {
    let pixels = size.0.max(1) as f32 * size.1.max(1) as f32;
    let scale = (target / pixels).sqrt().clamp(0.5, 1.0);
    ((scale * 20.0).round() / 20.0).clamp(0.5, 1.0)
}

/// What the renderer is told: the scene's fraction of the window and samples per pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneQuality {
    pub scale: f32,
    pub samples: u32,
}

/// Window sizes offered in windowed mode (logical pixels).
pub const RESOLUTIONS: [(u32, u32); 6] = [(1280, 720), (1440, 900), (1600, 900), (1920, 1080), (2560, 1440), (3840, 2160)];
/// Autosave intervals offered (turns; 0 = off).
pub const AUTOSAVE_INTERVALS: [u32; 5] = [0, 1, 2, 5, 10];

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub window: WindowMode,
    pub resolution: (u32, u32),
    /// UI zoom over the platform scale, 0.75 ..= 1.5.
    pub ui_scale: f32,
    pub quality: Quality,
    pub render_scale: RenderScale,
    pub anti_aliasing: AntiAliasing,
    pub master_volume: f32,
    pub music_volume: f32,
    pub music_muted: bool,
    /// Brightness of menu and screen artwork, 0.5 ..= 1.
    pub brightness: f32,
    /// Autosave every this many turns (0 = never).
    pub autosave_every: u32,
    /// Autosaves kept; older ones are removed.
    pub autosaves_kept: u32,
    pub effects_volume: f32,
    pub effects_muted: bool,
    /// How actions on the map are shown: other players' turns, reduced motion, glides.
    pub animation: crate::shell::presentation::Settings,
    /// Collapsible panels the player left open (`fold`), sorted; the others are folded.
    pub open_panels: Vec<String>,
    /// The interface language: `auto` (the system's) or a language tag (`de`, `zh-Hans`).
    pub language: String,
    /// The player's keyboard changes (`keymap`): `<set>.<action>` -> keys. Only what differs
    /// from the defaults; lines of modules that are not running stay.
    pub keys: crate::shell::keymap::Overrides,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            window: WindowMode::Windowed,
            resolution: (1440, 900),
            ui_scale: 1.0,
            quality: Quality::Medium,
            render_scale: RenderScale::Auto,
            anti_aliasing: AntiAliasing::Auto,
            master_volume: 1.0,
            music_volume: 0.35,
            music_muted: false,
            brightness: 1.0,
            autosave_every: 1,
            autosaves_kept: 5,
            effects_volume: 0.7,
            effects_muted: false,
            animation: crate::shell::presentation::Settings::default(),
            open_panels: Vec::new(),
            language: crate::shell::language::AUTO.to_owned(),
            keys: Default::default(),
        }
    }
}

impl Settings {
    /// Music loudness after the master volume.
    pub fn music_level(&self) -> f32 {
        self.master_volume * self.music_volume
    }

    /// Effects loudness after the master volume.
    pub fn effects_level(&self) -> f32 {
        self.master_volume * self.effects_volume
    }

    /// Render scale and samples for a window of `size` pixels on a graphics card that is
    /// `constrained` (see `is_constrained`): `Auto` spares a constrained card, a fixed
    /// choice is taken as it is.
    pub fn scene_quality(&self, constrained: bool, size: (u32, u32)) -> SceneQuality {
        self.scene_quality_for(constrained, size, AUTO_SCENE_PIXELS)
    }

    /// The same, `Auto` on a constrained card aiming at `target` pixels of scene.
    pub fn scene_quality_for(&self, constrained: bool, size: (u32, u32), target: f32) -> SceneQuality {
        let scale = match (self.render_scale.fraction(), constrained) {
            (Some(fraction), _) => fraction,
            (None, true) => auto_render_scale_for(size, target),
            (None, false) => 1.0,
        };
        let samples = self.anti_aliasing.samples().unwrap_or(if constrained { 1 } else { 4 });
        SceneQuality { scale, samples }
    }

    pub fn to_text(&self) -> String {
        let window = match self.window {
            WindowMode::Windowed => "windowed",
            WindowMode::Fullscreen => "fullscreen",
        };
        let mut lines = vec![
            "# Aurelia settings".to_owned(), // i18n-ok: header of the config file
            format!("window = {window}"),
            format!("resolution = {}x{}", self.resolution.0, self.resolution.1),
            format!("ui_scale = {:.2}", self.ui_scale),
            format!("quality = {}", self.quality.key()),
            format!("render_scale = {}", self.render_scale.key()),
            format!("anti_aliasing = {}", self.anti_aliasing.key()),
            format!("master_volume = {:.2}", self.master_volume),
            format!("music_volume = {:.2}", self.music_volume),
            format!("music_muted = {}", self.music_muted),
            format!("brightness = {:.2}", self.brightness),
            format!("autosave_every = {}", self.autosave_every),
            format!("autosaves_kept = {}", self.autosaves_kept),
            format!("effects_volume = {:.2}", self.effects_volume),
            format!("effects_muted = {}", self.effects_muted),
            format!("other_turns = {}", self.animation.speed.key()),
            format!("reduced_motion = {}", self.animation.reduced_motion),
            format!("camera_glides = {}", self.animation.camera_glides),
            format!("language = {}", self.language),
        ];
        let open = crate::shell::fold::sanitize(self.open_panels.iter().map(String::as_str));
        if !open.is_empty() {
            lines.push(format!("open_panels = {}", open.join(",")));
        }
        for (action, keys) in &self.keys {
            lines.push(format!("key.{action} = {keys}")); // i18n-ok: keycap names and file keys
        }
        lines.join("\n") + "\n"
    }

    /// Whether the settings differ in anything but the open panels (those need no
    /// renderer or UI re-apply).
    pub fn differs_apart_from_panels(&self, other: &Settings) -> bool {
        Settings { open_panels: other.open_panels.clone(), ..self.clone() } != *other
    }

    /// Settings from a file's text; every missing or bad value keeps its default.
    pub fn from_text(text: &str) -> Self {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            s.set(key.trim(), value.trim());
        }
        s
    }

    fn set(&mut self, key: &str, value: &str) {
        if let Some(action) = key.strip_prefix("key.") {
            // A line that is not a list of chords is dropped, the action keeps its defaults.
            if !action.is_empty() && crate::shell::keymap::parse_chords(value).is_some() {
                self.keys.insert(action.to_owned(), value.to_owned());
            }
            return;
        }
        let unit = |v: &str, low: f32, high: f32| v.parse::<f32>().ok().filter(|x| x.is_finite()).map(|x| x.clamp(low, high));
        match key {
            "window" => match value {
                "windowed" => self.window = WindowMode::Windowed,
                "fullscreen" => self.window = WindowMode::Fullscreen,
                _ => {}
            },
            "resolution" => {
                if let Some((w, h)) = value.split_once('x') {
                    if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                        if (640..=7680).contains(&w) && (480..=4320).contains(&h) {
                            self.resolution = (w, h);
                        }
                    }
                }
            }
            "ui_scale" => self.ui_scale = unit(value, 0.75, 1.5).unwrap_or(self.ui_scale),
            "quality" => {
                if let Some(q) = Quality::ALL.into_iter().find(|q| q.key().eq_ignore_ascii_case(value)) {
                    self.quality = q;
                }
            }
            "render_scale" => {
                if let Some(r) = RenderScale::ALL.into_iter().find(|r| r.key() == value.to_lowercase()) {
                    self.render_scale = r;
                }
            }
            "anti_aliasing" => {
                if let Some(a) = AntiAliasing::ALL.into_iter().find(|a| a.key() == value.to_lowercase()) {
                    self.anti_aliasing = a;
                }
            }
            "master_volume" => self.master_volume = unit(value, 0., 1.).unwrap_or(self.master_volume),
            "music_volume" => self.music_volume = unit(value, 0., 1.).unwrap_or(self.music_volume),
            "music_muted" => self.music_muted = value == "true",
            "brightness" => self.brightness = unit(value, 0.5, 1.).unwrap_or(self.brightness),
            "autosave_every" => {
                if let Ok(turns) = value.parse::<u32>() {
                    self.autosave_every = turns.min(100);
                }
            }
            "effects_volume" => self.effects_volume = unit(value, 0., 1.).unwrap_or(self.effects_volume),
            "effects_muted" => self.effects_muted = value == "true",
            "other_turns" => {
                let speeds = crate::shell::presentation::Speed::ALL;
                if let Some(speed) = speeds.into_iter().find(|s| s.key().eq_ignore_ascii_case(value)) {
                    self.animation.speed = speed;
                }
            }
            "reduced_motion" => self.animation.reduced_motion = value == "true",
            "camera_glides" => self.animation.camera_glides = value != "false",
            "language" => {
                if value == crate::shell::language::AUTO {
                    self.language = value.to_owned();
                } else if let Some(tag) = i18n::locale::normalize(value) {
                    self.language = tag;
                }
            }
            "open_panels" => self.open_panels = crate::shell::fold::sanitize(value.split(',').map(str::trim)),
            "autosaves_kept" => {
                if let Ok(kept) = value.parse::<u32>() {
                    self.autosaves_kept = kept.clamp(1, 50);
                }
            }
            _ => {}
        }
    }

    /// Settings of the config file, or the defaults when there is none.
    pub fn load(path: &std::path::Path) -> Self {
        match crate::shell::storage::read_to_string(path) {
            Ok(text) => Self::from_text(&text),
            Err(_) => Self::default(),
        }
    }

    /// Write through a temporary file, so a crash never leaves half a file.
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        use crate::shell::storage;
        if let Some(dir) = path.parent() {
            storage::create_dir_all(dir).map_err(|e| i18n::tf!("shell.errors.cannot-create", path = format!("{dir:?}"), error = e.to_string()))?;
        }
        let temporary = path.with_extension("tmp");
        storage::write(&temporary, self.to_text().as_bytes()).map_err(|e| i18n::tf!("shell.errors.cannot-save-settings", error = e.to_string()))?;
        storage::rename(&temporary, path).map_err(|e| i18n::tf!("shell.errors.cannot-save-settings", error = e.to_string()))
    }
}

/// `$AURELIA_CONFIG_DIR/settings.cfg`, else the platform's config directory
/// (`~/Library/Application Support/Aurelia`, `%APPDATA%\Aurelia`,
/// `$XDG_CONFIG_HOME/aurelia` or `~/.config/aurelia`).
pub fn config_path() -> PathBuf {
    config_dir().join("settings.cfg")
}

fn config_dir() -> PathBuf {
    // A page has no home directory: the settings are a file of the browser's store (`storage`).
    #[cfg(target_arch = "wasm32")]
    return PathBuf::from("config");
    #[cfg(not(target_arch = "wasm32"))]
    native_config_dir()
}

#[cfg(not(target_arch = "wasm32"))]
fn native_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("AURELIA_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        return home.join("Library/Application Support/Aurelia");
    }
    if cfg!(target_os = "windows") {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("Aurelia");
        }
    }
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(dir) => PathBuf::from(dir).join("aurelia"),
        Err(_) => home.join(".config/aurelia"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_the_file() {
        let s = Settings {
            window: WindowMode::Fullscreen,
            resolution: (1920, 1080),
            ui_scale: 1.25,
            quality: Quality::Low,
            render_scale: RenderScale::TwoThirds,
            anti_aliasing: AntiAliasing::Off,
            master_volume: 0.5,
            music_volume: 0.8,
            music_muted: true,
            brightness: 0.75,
            autosave_every: 5,
            autosaves_kept: 3,
            effects_volume: 0.4,
            effects_muted: true,
            animation: crate::shell::presentation::Settings {
                speed: crate::shell::presentation::Speed::Skip,
                reduced_motion: true,
                camera_glides: false,
            },
            open_panels: vec!["city_list".to_owned(), "events".to_owned()],
            language: "zh-Hans".to_owned(),
            keys: [("classic.order-found-city".to_owned(), "J".to_owned())].into(),
        };
        let dir = std::env::temp_dir().join(format!("aurelia-settings-{}", std::process::id()));
        let path = dir.join("nested").join("settings.cfg");
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_scale_and_anti_aliasing_choice_round_trips() {
        for render_scale in RenderScale::ALL {
            for anti_aliasing in AntiAliasing::ALL {
                let s = Settings { render_scale, anti_aliasing, ..Settings::default() };
                assert_eq!(Settings::from_text(&s.to_text()), s);
            }
        }
    }

    #[test]
    fn an_old_file_without_the_new_keys_reads_as_auto() {
        let s = Settings::from_text("window = fullscreen\nquality = high\n");
        assert_eq!((s.render_scale, s.anti_aliasing), (RenderScale::Auto, AntiAliasing::Auto));
        let bad = Settings::from_text("render_scale = 33\nanti_aliasing = 8x\n");
        assert_eq!((bad.render_scale, bad.anti_aliasing), (RenderScale::Auto, AntiAliasing::Auto));
    }

    #[test]
    fn auto_spares_integrated_and_software_cards_only() {
        assert!(is_constrained(wgpu::DeviceType::IntegratedGpu) && is_constrained(wgpu::DeviceType::Cpu));
        assert!(!is_constrained(wgpu::DeviceType::DiscreteGpu));
        let auto = Settings::default();
        // A 5K display at 2x: 2880x1800 pixels of window, about 2 million of scene.
        let slow = auto.scene_quality(true, (2880, 1800));
        assert_eq!((slow.scale, slow.samples), (0.6, 1));
        assert!((slow.scale * 2880.0) * (slow.scale * 1800.0) < 2.0e6 * 1.1);
        // A small window already is under the budget.
        assert_eq!(auto.scene_quality(true, (1280, 720)).scale, 1.0);
        assert_eq!(auto.scene_quality(true, (7680, 4320)).scale, 0.5);
        let fast = auto.scene_quality(false, (2880, 1800));
        assert_eq!((fast.scale, fast.samples), (1.0, 4));
    }

    #[test]
    fn fixed_choices_ignore_the_card() {
        let s = Settings { render_scale: RenderScale::Half, anti_aliasing: AntiAliasing::X4, ..Settings::default() };
        for constrained in [false, true] {
            assert_eq!(s.scene_quality(constrained, (2880, 1800)), SceneQuality { scale: 0.5, samples: 4 });
        }
        let full = Settings { render_scale: RenderScale::Full, anti_aliasing: AntiAliasing::Off, ..Settings::default() };
        assert_eq!(full.scene_quality(true, (2880, 1800)), SceneQuality { scale: 1.0, samples: 1 });
    }

    #[test]
    fn the_language_is_auto_until_a_tag_is_chosen_and_bad_text_changes_nothing() {
        assert_eq!(Settings::default().language, "auto");
        assert_eq!(Settings::from_text("language = de_DE.UTF-8\n").language, "de-DE");
        assert_eq!(Settings::from_text("language = zh-hans\n").language, "zh-Hans");
        assert_eq!(Settings::from_text("language = ../../etc\n").language, "auto");
        assert_eq!(Settings::from_text("language = \n").language, "auto");
        assert!(Settings::default().to_text().contains("language = auto\n"));
    }

    #[test]
    fn damaged_or_missing_values_keep_the_defaults() {
        let s = Settings::from_text("window = sideways\nui_scale = 9\nresolution = 10x10\nquality = ultra\nmaster_volume = NaN\nnonsense\nautosave_every = -3\n");
        let d = Settings::default();
        assert_eq!(s.window, d.window);
        assert_eq!(s.ui_scale, 1.5, "clamped to the range");
        assert_eq!(s.resolution, d.resolution);
        assert_eq!(s.quality, d.quality);
        assert_eq!(s.master_volume, d.master_volume);
        assert_eq!(s.autosave_every, d.autosave_every);
        assert_eq!(Settings::load(std::path::Path::new("/no/such/settings.cfg")), d);
    }

    #[test]
    fn open_panels_are_written_as_a_list_and_bad_names_are_dropped() {
        let s = Settings { open_panels: vec!["events".to_owned(), "city_list".to_owned()], ..Settings::default() };
        assert!(s.to_text().contains("open_panels = city_list,events\n"));
        assert!(!Settings::default().to_text().contains("open_panels"), "folded by default: no line");
        let read = Settings::from_text("open_panels = events, Bad Name,,a-b,events,top_2\n");
        assert_eq!(read.open_panels, ["events", "top_2"]);
        assert_eq!(Settings::from_text("open_panels =\n").open_panels, Vec::<String>::new());
    }

    #[test]
    fn an_opened_panel_reaches_the_settings_and_a_new_context() {
        let first = egui::Context::default();
        let mut settings = Settings::default();
        crate::shell::fold::set_open(&first, "events", true);
        assert!(crate::shell::fold::export(&first, &mut settings.open_panels));
        let reread = Settings::from_text(&settings.to_text());
        let second = egui::Context::default();
        crate::shell::fold::load(&second, &reread.open_panels);
        assert!(crate::shell::fold::is_open(&second, "events"));
        assert!(!crate::shell::fold::take_changed(&second), "loading is not a change to save");
        assert!(!reread.differs_apart_from_panels(&settings));
    }

    #[test]
    fn quality_presets_map_to_renderer_switches() {
        assert!(Quality::Low.terrain_lod() && !Quality::Low.animated());
        assert!(Quality::Medium.terrain_lod() && Quality::Medium.animated());
        assert!(!Quality::High.terrain_lod() && Quality::High.animated());
    }

    #[test]
    fn keyboard_changes_survive_the_file_and_old_files_have_none() {
        let mut s = Settings::default();
        s.keys.insert("classic.order-found-city".into(), "J".into());
        s.keys.insert("shell.zoom-in".into(), "Plus Equals PageUp".into());
        s.keys.insert("classic.order-heal".into(), "none".into());
        let text = s.to_text();
        assert!(text.contains("key.classic.order-found-city = J\n"), "{text}");
        assert_eq!(Settings::from_text(&text).keys, s.keys);
        assert!(Settings::from_text("window = windowed\nquality = low\n").keys.is_empty());
        // A line that is not a list of chords is dropped.
        assert!(Settings::from_text("key.shell.zoom-in = Ctrl+Nothing\nkey. = B\n").keys.is_empty());
    }
}
