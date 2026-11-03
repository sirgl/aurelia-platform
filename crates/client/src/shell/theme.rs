//! The look a module gives the shell: colour tokens, panel shapes, nine-slice frames,
//! fonts and the folder of its own art. The app installs the running module's theme
//! into the egui context once, before the first frame (`install`); the screen kit, the
//! shell's screens and the HUD frame read it back (`Theme::of`).
//!
//! The default, `Theme::AURELIA`, is the shell's own look, the one the classic module
//! and the sandbox keep: its tokens are the kit's and the HUD's constants and it loads no font,
//! so their frames do not change by a pixel. A module that wants another look returns
//! its own `&'static Theme` from `ModuleUi::theme`.
use egui::{Color32, FontFamily, FontId, Stroke};

use super::{hud, kit};

/// The colour tokens. Screens (menus, settings, module screens) use the first group,
/// the HUD over the map the `hud_*` group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colors {
    /// Behind full screens.
    pub background: Color32,
    /// Panels on a screen.
    pub panel: Color32,
    /// Cards and idle buttons inside a panel.
    pub raised: Color32,
    /// The thin edge of panels, cards and idle buttons.
    pub edge: Color32,
    /// A hovered button.
    pub hover: Color32,
    /// A selected button, tab or row.
    pub selected: Color32,
    /// The edge of a selected or hovered button.
    pub selected_edge: Color32,
    /// The screen's main action (one wide button).
    pub primary: Color32,
    /// Headings, rules and finished things.
    pub accent: Color32,
    /// Icons drawn in the accent's metal (brighter than the lines).
    pub accent_icon: Color32,
    /// Main and secondary text.
    pub text: Color32,
    pub muted: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    /// A progress bar's fill and its track.
    pub progress: Color32,
    pub track: Color32,
    /// The HUD over the map: panel fill, its edge, text and muted text.
    pub hud_fill: Color32,
    pub hud_accent: Color32,
    pub hud_text: Color32,
    pub hud_muted: Color32,
}

/// Corner radii and the edge width, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shapes {
    pub panel_radius: f32,
    pub card_radius: f32,
    pub button_radius: f32,
    /// Thumbnails of items in lists and cards.
    pub thumbnail_radius: f32,
    pub edge_width: f32,
}

/// A frame drawn from art: the image's borders keep their size, its middle stretches
/// (`kit::nine_slice`). Both measures are independent of the image's resolution (art
/// is uploaded downscaled).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NineSlice {
    /// Screen art by name (`art::ScreenArt`).
    pub art: &'static str,
    /// Where the image is cut, as shares of its size: left, top, right, bottom.
    pub cuts: [f32; 4],
    /// How wide each border is on screen, in logical pixels: left, top, right, bottom.
    pub borders: [f32; 4],
}

/// Frames from art for the shell's panels and buttons (None: drawn with the tokens).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frames {
    pub panel: Option<NineSlice>,
    pub button: Option<NineSlice>,
}

/// Font files under the asset folder; the first file of a list that exists is used.
/// An empty list keeps egui's own fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fonts {
    /// All text (egui's proportional family starts with it).
    pub text: &'static [&'static str],
    /// Titles, the logo and quotes (`Theme::title`).
    pub title: &'static [&'static str],
}

/// A module's look (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub colors: Colors,
    pub shapes: Shapes,
    pub frames: Frames,
    pub fonts: Fonts,
    /// The module's art folder under the asset folder: screen art and atlas regions are
    /// looked up there first, then in the shared `visual/generated`.
    pub art_root: Option<&'static str>,
}

/// The font family of titles while a theme with title fonts is installed.
const TITLE_FAMILY: &str = "theme-title";

impl Theme {
    /// The shell's own look (the classic module and the sandbox).
    pub const AURELIA: Theme = Theme {
        colors: Colors {
            background: kit::BG,
            panel: kit::PANEL,
            raised: kit::RAISED,
            edge: Color32::from_rgb(52, 72, 73),
            hover: Color32::from_rgb(42, 69, 71),
            selected: kit::TEAL,
            selected_edge: kit::GOLD,
            primary: kit::TEAL,
            accent: kit::GOLD,
            accent_icon: kit::GOLD,
            text: kit::CREAM,
            muted: kit::MUTED,
            success: Color32::from_rgb(126, 196, 110),
            warning: kit::GOLD,
            danger: Color32::from_rgb(214, 96, 84),
            progress: kit::TEAL,
            track: kit::RAISED,
            hud_fill: hud::INK,
            hud_accent: hud::GOLD,
            hud_text: hud::LIGHT,
            hud_muted: hud::MUTED,
        },
        shapes: Shapes { panel_radius: 7.0, card_radius: 5.0, button_radius: 4.0, thumbnail_radius: 4.0, edge_width: 1.0 },
        frames: Frames { panel: None, button: None },
        fonts: Fonts { text: &[], title: &[] },
        art_root: None,
    };

    /// The theme installed in `ctx` (the shell's own before `install`).
    pub fn of(ctx: &egui::Context) -> &'static Theme {
        ctx.data(|data| data.get_temp::<&'static Theme>(theme_id())).unwrap_or(&Self::AURELIA)
    }

    /// The font of titles: the theme's title font, or the text font when it has none.
    pub fn title(&self, size: f32) -> FontId {
        if self.fonts.title.is_empty() {
            FontId::proportional(size)
        } else {
            FontId::new(size, FontFamily::Name(TITLE_FAMILY.into()))
        }
    }

    /// The thin edge of panels and idle buttons.
    pub fn edge(&self) -> Stroke {
        Stroke::new(self.shapes.edge_width, self.colors.edge)
    }
}

fn theme_id() -> egui::Id {
    egui::Id::new("shell_module_theme")
}

/// Make `theme` the look of `ctx`: its tokens for the kit, its widget visuals and its
/// fonts. Call before the first frame: egui switches fonts at the start of a pass.
pub fn install(ctx: &egui::Context, theme: &'static Theme) {
    ctx.data_mut(|data| data.insert_temp(theme_id(), theme));
    ctx.set_visuals(visuals(&theme.colors));
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.animation_time = 0.0;
    });
    if let Some(fonts) = font_definitions(&theme.fonts, &|path| super::assets::read(&super::assets::path(path))) {
        ctx.set_fonts(fonts);
    }
}

/// Smallest touch target in points (dp on a phone: Material asks for 48).
pub const MIN_TOUCH_TARGET: f32 = 48.0;

/// Touch screens: egui's buttons, rows and checkboxes are at least `MIN_TOUCH_TARGET` high, with
/// a little more air between rows. On when `AURELIA_TOUCH=1` (the Android host sets it) or when an
/// offscreen capture plays a phone density (`AURELIA_VISUAL_NATIVE_SCALE` of 2 or more), or when
/// the host said so (`device::set_touch`: the page's mobile mode).
pub fn touch_wanted() -> bool {
    // The environment cannot change while the process runs: read once (this is asked per widget).
    static ENV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    super::device::touch()
        || *ENV.get_or_init(|| std::env::var("AURELIA_TOUCH").is_ok_and(|v| v == "1") || super::capture::simulated_native_scale() >= 2.0)
}

/// The area a finger may press for a painted control of `r`: on a touch screen the control is
/// reached from up to `cap` points around it so that it is at least `MIN_TOUCH_TARGET` in each
/// direction (the cap keeps neighbours from sharing a hit area). `r` itself otherwise.
pub fn touch_hit(r: egui::Rect, cap: f32) -> egui::Rect {
    if !touch_wanted() {
        return r;
    }
    let grow = |len: f32| ((MIN_TOUCH_TARGET - len) * 0.5).clamp(0., cap);
    r.expand2(egui::vec2(grow(r.width()), grow(r.height())))
}

pub fn touch_targets(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.interact_size = egui::vec2(MIN_TOUCH_TARGET, MIN_TOUCH_TARGET);
        style.spacing.item_spacing.y = style.spacing.item_spacing.y.max(8.0);
    });
}

/// The font files that give a language the glyphs Inter and egui's fonts lack, first choice
/// first (subsets of Noto Sans CJK, built by `platform/tools/i18n/subset_fonts.py`). Chinese and
/// Japanese share Han characters but draw them differently, so each language lists its own
/// font first.
pub fn language_fonts(tag: &str) -> &'static [&'static str] {
    match tag.split(['-', '_']).next().unwrap_or("") {
        "ja" => &["fonts/NotoSansJP-Subset.otf", "fonts/NotoSansSC-Subset.otf"],
        "zh" => &["fonts/NotoSansSC-Subset.otf", "fonts/NotoSansJP-Subset.otf"],
        _ => &[],
    }
}

/// Install the theme's fonts with the fallback fonts of `language` behind them. Returns
/// false while a font file of the language has not arrived yet (a browser fetches files on
/// demand): call again on the next frame; true when the fonts are in place (a file that
/// the asset folder does not have is skipped with a note on stderr, the text shows as boxes).
pub fn install_fonts(ctx: &egui::Context, theme: &Theme, language: &str) -> bool {
    let read = |path: &str| super::assets::read(&super::assets::path(path));
    let mut pending = false;
    let mut extra = Vec::new();
    for path in language_fonts(language) {
        match read(path) {
            Some(bytes) => extra.push((*path, bytes)),
            None if super::assets::is_file(&super::assets::path(path)) => pending = true,
            None => eprintln!("fonts: {path} is not in the asset folder; {language} text may show as boxes"),
        }
    }
    if pending {
        return false;
    }
    if let Some(fonts) = font_definitions_with(&theme.fonts, &read, &extra) {
        ctx.set_fonts(fonts);
    }
    true
}

/// egui's widget look from the HUD tokens.
fn visuals(colors: &Colors) -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = colors.hud_fill;
    visuals.panel_fill = colors.hud_fill;
    visuals.override_text_color = Some(colors.hud_text);
    visuals.window_stroke = Stroke::new(1.0, colors.hud_accent);
    visuals.selection.bg_fill = Color32::from_rgb(43, 83, 109);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 66, 75);
    visuals
}

/// egui's fonts with the theme's in front (None: the theme names no font, egui's stay).
/// A listed file that cannot be read is skipped; when none of a list is found, that
/// role keeps egui's fonts and the title family falls back to them.
fn font_definitions(fonts: &Fonts, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<egui::FontDefinitions> {
    font_definitions_with(fonts, read, &[])
}

/// As `font_definitions`, with `extra` fonts (name, bytes) appended behind the others as
/// fallbacks for glyphs the others lack. With no extra font and none named by the theme
/// the result is None when the theme names no font at all.
fn font_definitions_with(fonts: &Fonts, read: &dyn Fn(&str) -> Option<Vec<u8>>, extra: &[(&str, Vec<u8>)]) -> Option<egui::FontDefinitions> {
    if fonts.text.is_empty() && fonts.title.is_empty() && extra.is_empty() {
        return None;
    }
    let mut definitions = egui::FontDefinitions::default();
    let mut first_found = |list: &[&str], name: &str| {
        let bytes = list.iter().find_map(|path| read(path));
        if bytes.is_none() && !list.is_empty() {
            eprintln!("theme: none of the fonts {list:?} is in the asset folder; egui's font stands in");
        }
        bytes.map(|bytes| {
            definitions.font_data.insert(name.to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            name.to_owned()
        })
    };
    let text = first_found(fonts.text, "theme-text");
    let title = first_found(fonts.title, "theme-title-font");
    let proportional = definitions.families.entry(FontFamily::Proportional).or_default();
    if let Some(text) = text {
        proportional.insert(0, text);
    }
    for (i, (_, bytes)) in extra.iter().enumerate() {
        let name = format!("language-fallback-{i}");
        definitions.font_data.insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(bytes.clone())));
        definitions.families.entry(FontFamily::Proportional).or_default().push(name);
    }
    let mut titles: Vec<String> = title.into_iter().collect();
    titles.extend(definitions.families[&FontFamily::Proportional].iter().cloned());
    definitions.families.insert(FontFamily::Name(TITLE_FAMILY.into()), titles);
    Some(definitions)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONTS: Fonts = Fonts { text: &["missing.ttf", "text.ttf"], title: &["title.ttf"] };

    #[test]
    fn the_default_theme_is_the_shell_constants_and_loads_no_font() {
        let theme = Theme::AURELIA;
        assert_eq!(theme.colors.panel, kit::PANEL);
        assert_eq!(theme.colors.hud_fill, hud::INK);
        assert_eq!(theme.title(20.0), FontId::proportional(20.0));
        assert!(font_definitions(&theme.fonts, &|_| panic!("no font is read")).is_none());
        let ctx = egui::Context::default();
        assert_eq!(Theme::of(&ctx), &Theme::AURELIA, "nothing installed: the shell's look");
    }

    #[test]
    fn installed_theme_is_read_back() {
        static OWN: Theme = Theme { art_root: Some("own"), ..Theme::AURELIA };
        let ctx = egui::Context::default();
        install(&ctx, &OWN);
        assert_eq!(Theme::of(&ctx).art_root, Some("own"));
    }

    #[test]
    fn language_fallback_fonts_go_last_in_both_families() {
        let real = egui::FontDefinitions::default();
        let bytes = real.font_data["Ubuntu-Light"].font.to_vec();
        let none = Fonts { text: &[], title: &[] };
        let definitions = font_definitions_with(&none, &|_| None, &[("fonts/a.otf", bytes.clone()), ("fonts/b.otf", bytes)]).unwrap();
        let proportional = &definitions.families[&FontFamily::Proportional];
        assert_eq!(&proportional[proportional.len() - 2..], ["language-fallback-0", "language-fallback-1"]);
        assert_eq!(&proportional[..proportional.len() - 2], &real.families[&FontFamily::Proportional][..]);
        let titles = &definitions.families[&FontFamily::Name(TITLE_FAMILY.into())];
        assert_eq!(titles.last().map(String::as_str), Some("language-fallback-1"));
        assert!(font_definitions_with(&none, &|_| None, &[]).is_none());
        assert_eq!(language_fonts("ja-JP")[0], "fonts/NotoSansJP-Subset.otf");
        assert_eq!(language_fonts("zh-Hans")[0], "fonts/NotoSansSC-Subset.otf");
        assert!(language_fonts("de").is_empty());
    }

    #[test]
    fn found_fonts_go_first_and_the_title_family_falls_back_to_text() {
        let real = egui::FontDefinitions::default();
        let read = |path: &str| (path != "missing.ttf").then(|| real.font_data["Ubuntu-Light"].font.to_vec());
        let definitions = font_definitions(&FONTS, &read).unwrap();
        let proportional = &definitions.families[&FontFamily::Proportional];
        assert_eq!(proportional[0], "theme-text");
        let titles = &definitions.families[&FontFamily::Name(TITLE_FAMILY.into())];
        assert_eq!(titles[0], "theme-title-font");
        assert_eq!(&titles[1..], &proportional[..]);

        let none = font_definitions(&FONTS, &|_| None).unwrap();
        let titles = &none.families[&FontFamily::Name(TITLE_FAMILY.into())];
        assert_eq!(titles, &real.families[&FontFamily::Proportional], "missing files: egui's fonts");
    }
}
