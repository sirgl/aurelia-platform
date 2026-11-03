//! The shell's own words: the menu, the header, the settings, the saves and the HUD
//! frame, in the active language and the running module's wording.
//!
//! The words are messages of the catalogue (`assets/lang/<code>/shell.ftl`, keys
//! `shell.*`); [`ShellWords`] is the plain words resolved once per language and module
//! (`current`, `for_scope`) so the drawing code reads fields. A module words a few of them
//! differently with `key@scope` overrides (Lands & Laws: `shell.end-turn-tip@lal`). Words
//! with a number or a name are not fields: the code asks the catalogue with named arguments
//! (`tf!("shell.hud.health", current = hp, max = max)`), because their order and plural form
//! belong to the language.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use i18n::t;

pub use super::observer::words::ObserverWords;

/// A label and the line that explains it (a settings row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub label: &'static str,
    pub help: &'static str,
}

/// The main menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuWords {
    pub resume: &'static str,
    pub new_game: &'static str,
    pub save_game: &'static str,
    pub load_game: &'static str,
    pub quit: &'static str,
    /// Over the summary of the running match.
    pub your_world: &'static str,
    /// The showcase's menu: explore its world, the art gallery, its title block.
    pub explore: &'static str,
    pub continue_journey: &'static str,
    pub gallery: &'static str,
    pub showcase_title: &'static str,
    pub showcase_line: &'static str,
}

/// What the menu says after a save or a load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveWords {
    pub wait_for_turns: &'static str,
    pub nothing_saved: &'static str,
    /// The menu's summary of a loaded match.
    pub loaded_world: &'static str,
    pub unknown_module: &'static str,
}

/// The settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsWords {
    pub audio: &'static str,
    pub graphics: &'static str,
    pub game: &'static str,
    pub controls: &'static str,
    pub applied: &'static str,
    pub off: &'static str,
    pub on: &'static str,
    pub master_volume: Line,
    pub music_volume: Line,
    pub mute_music: Line,
    pub effects_volume: Line,
    pub mute_effects: Line,
    pub audio_failed: &'static str,
    pub window_mode: Line,
    pub windowed: &'static str,
    pub fullscreen: &'static str,
    /// The help line while windowed; the fullscreen one is `window_size_fullscreen`.
    pub window_size: Line,
    pub window_size_fullscreen: &'static str,
    /// The help line follows the chosen quality.
    pub quality: &'static str,
    pub quality_levels: [Line; 3],
    pub render_scale: Line,
    /// The choice that lets the game pick (render scale, anti-aliasing).
    pub auto: &'static str,
    pub anti_aliasing: Line,
    pub interface_scale: Line,
    pub brightness: Line,
    pub autosave: Line,
    pub never: &'static str,
    pub every_turn: &'static str,
    pub saves_kept: Line,
    pub other_turns: Line,
    pub speeds: [&'static str; 3],
    pub reduced_motion: Line,
    pub wonder_camera: Line,
    pub wonder_camera_off: &'static str,
    pub controls_help: &'static str,
    /// The language row: its label and help, the automatic choice, how a custom bundle is
    /// marked, and the line under a language whose translation is not reviewed.
    pub language: Line,
    pub language_auto: &'static str,
    pub language_custom: &'static str,
}

/// The HUD frame over the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudWords {
    pub settings_tip: &'static str,
    pub center_capital: &'static str,
    pub toggle_grid: &'static str,
    pub toggle_labels: &'static str,
    pub toggle_yields: &'static str,
    pub minimap_tip: &'static str,
    pub orders_unavailable: &'static str,
    pub turn_unavailable: &'static str,
    /// A badge of mixed units, and the line of a city's garrison.
    pub units: &'static str,
    pub garrison: &'static str,
    /// The end of a route: reached this turn (or in N turns: `shell.hud.turns`).
    pub this_turn: &'static str,
    /// Floating words over the map.
    pub captured: &'static str,
    pub destroyed: &'static str,
    pub not_growing: &'static str,
    pub manage_city: &'static str,
    pub strike_ready: &'static str,
}

/// The shell's own words on the map and around the screens, in the module's
/// language (the classic module and the sandbox keep the English ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellWords {
    /// Before the turn number in the top bar.
    pub turn: &'static str,
    pub end_turn: &'static str,
    pub end_turn_tip: &'static str,
    pub game_over: &'static str,
    /// The end-turn button while the other players act.
    pub others_moving: &'static str,
    /// The menu button over the map.
    pub empire: &'static str,
    pub navigate: &'static str,
    pub back: &'static str,
    pub back_to_map: &'static str,
    pub world_map: &'static str,
    pub menu: &'static str,
    pub settings: &'static str,
    /// City banners: the growth bar's label, a city that does not grow, an empty queue.
    pub grows: &'static str,
    pub stagnant: &'static str,
    pub choose_production: &'static str,
    pub main_menu: MenuWords,
    pub saves: SaveWords,
    pub options: SettingsWords,
    pub hud: HudWords,
    pub observer: ObserverWords,
}


impl ShellWords {
    /// The words of the active language and module scope, read from the catalogue.
    fn build() -> ShellWords {
        ShellWords {
            turn: t!("shell.turn"),
            end_turn: t!("shell.end-turn"),
            end_turn_tip: t!("shell.end-turn-tip"),
            game_over: t!("shell.game-over"),
            others_moving: t!("shell.others-moving"),
            empire: t!("shell.empire"),
            navigate: t!("shell.navigate"),
            back: t!("shell.back"),
            back_to_map: t!("shell.back-to-map"),
            world_map: t!("shell.world-map"),
            menu: t!("shell.menu"),
            settings: t!("shell.settings"),
            grows: t!("shell.grows"),
            stagnant: t!("shell.stagnant"),
            choose_production: t!("shell.choose-production"),
            main_menu: MenuWords {
                resume: t!("shell.main-menu.resume"),
                new_game: t!("shell.main-menu.new-game"),
                save_game: t!("shell.main-menu.save-game"),
                load_game: t!("shell.main-menu.load-game"),
                quit: t!("shell.main-menu.quit"),
                your_world: t!("shell.main-menu.your-world"),
                explore: t!("shell.main-menu.explore"),
                continue_journey: t!("shell.main-menu.continue-journey"),
                gallery: t!("shell.main-menu.gallery"),
                showcase_title: t!("shell.main-menu.showcase-title"),
                showcase_line: t!("shell.main-menu.showcase-line"),
            },
            saves: SaveWords {
                wait_for_turns: t!("shell.saves.wait-for-turns"),
                nothing_saved: t!("shell.saves.nothing-saved"),
                loaded_world: t!("shell.saves.loaded-world"),
                unknown_module: t!("shell.saves.unknown-module"),
            },
            options: SettingsWords {
                audio: t!("shell.options.audio"),
                graphics: t!("shell.options.graphics"),
                game: t!("shell.options.game"),
                controls: t!("shell.options.controls"),
                applied: t!("shell.options.applied"),
                off: t!("shell.options.off"),
                on: t!("shell.options.on"),
                master_volume: Line { label: t!("shell.options.master-volume.label"), help: t!("shell.options.master-volume.help") },
                music_volume: Line { label: t!("shell.options.music-volume.label"), help: t!("shell.options.music-volume.help") },
                mute_music: Line { label: t!("shell.options.mute-music.label"), help: t!("shell.options.mute-music.help") },
                effects_volume: Line { label: t!("shell.options.effects-volume.label"), help: t!("shell.options.effects-volume.help") },
                mute_effects: Line { label: t!("shell.options.mute-effects.label"), help: t!("shell.options.mute-effects.help") },
                audio_failed: t!("shell.options.audio-failed"),
                window_mode: Line { label: t!("shell.options.window-mode.label"), help: t!("shell.options.window-mode.help") },
                windowed: t!("shell.options.windowed"),
                fullscreen: t!("shell.options.fullscreen"),
                window_size: Line { label: t!("shell.options.window-size.label"), help: t!("shell.options.window-size.help") },
                window_size_fullscreen: t!("shell.options.window-size-fullscreen"),
                quality: t!("shell.options.quality"),
                quality_levels: [
                    Line { label: t!("shell.options.quality-low.label"), help: t!("shell.options.quality-low.help") },
                    Line { label: t!("shell.options.quality-medium.label"), help: t!("shell.options.quality-medium.help") },
                    Line { label: t!("shell.options.quality-high.label"), help: t!("shell.options.quality-high.help") },
                ],
                render_scale: Line { label: t!("shell.options.render-scale.label"), help: t!("shell.options.render-scale.help") },
                auto: t!("shell.options.auto"),
                anti_aliasing: Line { label: t!("shell.options.anti-aliasing.label"), help: t!("shell.options.anti-aliasing.help") },
                interface_scale: Line { label: t!("shell.options.interface-scale.label"), help: t!("shell.options.interface-scale.help") },
                brightness: Line { label: t!("shell.options.brightness.label"), help: t!("shell.options.brightness.help") },
                autosave: Line { label: t!("shell.options.autosave.label"), help: t!("shell.options.autosave.help") },
                never: t!("shell.options.never"),
                every_turn: t!("shell.options.every-turn"),
                saves_kept: Line { label: t!("shell.options.saves-kept.label"), help: t!("shell.options.saves-kept.help") },
                other_turns: Line { label: t!("shell.options.other-turns.label"), help: t!("shell.options.other-turns.help") },
                speeds: [t!("shell.options.speed-normal"), t!("shell.options.speed-fast"), t!("shell.options.speed-skip")],
                reduced_motion: Line { label: t!("shell.options.reduced-motion.label"), help: t!("shell.options.reduced-motion.help") },
                wonder_camera: Line { label: t!("shell.options.wonder-camera.label"), help: t!("shell.options.wonder-camera.help") },
                wonder_camera_off: t!("shell.options.wonder-camera-off"),
                controls_help: t!("shell.options.controls-help"),
                language: Line { label: t!("shell.options.language.label"), help: t!("shell.options.language.help") },
                language_auto: t!("shell.options.language-auto"),
                language_custom: t!("shell.options.language-custom"),
            },
            hud: HudWords {
                settings_tip: t!("shell.hud.settings-tip"),
                center_capital: t!("shell.hud.center-capital"),
                toggle_grid: t!("shell.hud.toggle-grid"),
                toggle_labels: t!("shell.hud.toggle-labels"),
                toggle_yields: t!("shell.hud.toggle-yields"),
                minimap_tip: t!("shell.hud.minimap-tip"),
                orders_unavailable: t!("shell.hud.orders-unavailable"),
                turn_unavailable: t!("shell.hud.turn-unavailable"),
                units: t!("shell.hud.units"),
                garrison: t!("shell.hud.garrison"),
                this_turn: t!("shell.hud.this-turn"),
                captured: t!("shell.hud.captured"),
                destroyed: t!("shell.hud.destroyed"),
                not_growing: t!("shell.hud.not-growing"),
                manage_city: t!("shell.hud.manage-city"),
                strike_ready: t!("shell.hud.strike-ready"),
            },
            observer: ObserverWords {
                entry: t!("shell.observer.entry"),
                entry_tip: t!("shell.observer.entry-tip"),
                pause: t!("shell.observer.pause"),
                resume: t!("shell.observer.resume"),
                step: t!("shell.observer.step"),
                paused: t!("shell.observer.paused"),
                speeds: [t!("shell.observer.speed-1"), t!("shell.observer.speed-2"), t!("shell.observer.speed-3"), t!("shell.observer.speed-4"), t!("shell.observer.speed-5"), t!("shell.observer.speed-6"), t!("shell.observer.speed-7"), t!("shell.observer.speed-8")],
                slower_tip: t!("shell.observer.slower-tip"),
                faster_tip: t!("shell.observer.faster-tip"),
                step_tip: t!("shell.observer.step-tip"),
                pause_tip: t!("shell.observer.pause-tip"),
                fallen: t!("shell.observer.fallen"),
                grid_hint: t!("shell.observer.grid-hint"),
                zoom_hint: t!("shell.observer.zoom-hint"),
                view_single: t!("shell.observer.view-single"),
                view_grid: t!("shell.observer.view-grid"),
                view_single_tip: t!("shell.observer.view-single-tip"),
                view_grid_tip: t!("shell.observer.view-grid-tip"),
                camera_follow: t!("shell.observer.camera-follow"),
                camera_free: t!("shell.observer.camera-free"),
                camera_follow_tip: t!("shell.observer.camera-follow-tip"),
                camera_free_tip: t!("shell.observer.camera-free-tip"),
                watch_all: t!("shell.observer.watch-all"),
                watch_one: t!("shell.observer.watch-one"),
                watch_tip: t!("shell.observer.watch-tip"),
                events_title: t!("shell.observer.events-title"),
                no_events: t!("shell.observer.no-events"),
                ev_fell: t!("shell.observer.ev-fell"),
                finale_title: t!("shell.observer.finale-title"),
                nobody: t!("shell.observer.nobody"),
                cannot_play: t!("shell.observer.cannot-play"),
                keep_playing: t!("shell.observer.keep-playing"),
                to_menu: t!("shell.observer.to-menu"),
                final_table: t!("shell.observer.final-table"),
                no_save: t!("shell.observer.no-save"),
                reveal: t!("shell.observer.reveal"),
                reveal_tip: t!("shell.observer.reveal-tip"),
                reveal_tag: t!("shell.observer.reveal-tag"),
                cities: t!("shell.observer.cities"),
                cities_tip: t!("shell.observer.cities-tip"),
                card_hint: t!("shell.observer.card-hint"),
                card_close: t!("shell.observer.card-close"),
                card_close_tip: t!("shell.observer.card-close-tip"),
                card_prev_tip: t!("shell.observer.card-prev-tip"),
                card_next_tip: t!("shell.observer.card-next-tip"),
                city_click: t!("shell.observer.city-click"),
                units_title: t!("shell.observer.units-title"),
                sight_unknown: t!("shell.observer.sight-unknown"),
                unit_fortified: t!("shell.observer.unit-fortified"),
                sight_label: t!("shell.observer.sight-label"),
                health_label: t!("shell.observer.health-label"),
                nation_label: t!("shell.observer.nation-label"),
                unit_hint: t!("shell.observer.unit-hint"),
                sight_note: t!("shell.observer.sight-note"),
                keys: leak_pairs(&[
                    (t!("shell.observer.key-pause.keys"), t!("shell.observer.key-pause.what")),
                    (t!("shell.observer.key-speed.keys"), t!("shell.observer.key-speed.what")),
                    (t!("shell.observer.key-step.keys"), t!("shell.observer.key-step.what")),
                    (t!("shell.observer.key-grid.keys"), t!("shell.observer.key-grid.what")),
                    (t!("shell.observer.key-tab.keys"), t!("shell.observer.key-tab.what")),
                    (t!("shell.observer.key-arrows.keys"), t!("shell.observer.key-arrows.what")),
                    (t!("shell.observer.key-numbers.keys"), t!("shell.observer.key-numbers.what")),
                    (t!("shell.observer.key-cities.keys"), t!("shell.observer.key-cities.what")),
                    (t!("shell.observer.key-reveal.keys"), t!("shell.observer.key-reveal.what")),
                    (t!("shell.observer.key-follow.keys"), t!("shell.observer.key-follow.what")),
                    (t!("shell.observer.key-hover.keys"), t!("shell.observer.key-hover.what")),
                    (t!("shell.observer.key-click-city.keys"), t!("shell.observer.key-click-city.what")),
                    (t!("shell.observer.key-click-unit.keys"), t!("shell.observer.key-click-unit.what")),
                    (t!("shell.observer.key-escape.keys"), t!("shell.observer.key-escape.what")),
                ]),
            },
        }
    }
}

type Context = (String, Option<&'static str>, u64);

/// The shell's words in the active language (the thread's override, or the process's) and
/// scope. Built once per language, scope and catalogue, then shared.
pub fn current() -> &'static ShellWords {
    static CACHE: OnceLock<Mutex<HashMap<Context, &'static ShellWords>>> = OnceLock::new();
    let context = i18n::context();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    if let Some(words) = cache.get(&context) {
        return words;
    }
    let words: &'static ShellWords = Box::leak(Box::new(ShellWords::build()));
    cache.insert(context, words);
    words
}

/// The words of a game module: its overrides (`key@scope`) apply over the shell's.
pub fn for_scope(scope: Option<&'static str>) -> &'static ShellWords {
    let _guard = i18n::enter_scope(scope);
    current()
}

/// A list of (keys, what they do) kept for the life of the process.
pub(super) fn leak_pairs(pairs: &[(&'static str, &'static str)]) -> &'static [(&'static str, &'static str)] {
    Box::leak(pairs.to_vec().into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A word that is the same in every language: the names of keys and signs.
    fn international(word: &str) -> bool {
        word.chars().filter(|c| c.is_alphabetic()).count() <= 2 || word.starts_with("×")
    }

    #[test]
    fn english_words_are_the_catalogue_text() {
        let w = i18n::with_language("en", current);
        assert_eq!(w.end_turn, "End turn");
        assert_eq!(w.options.quality_levels[2].label, "High");
        assert_eq!(w.observer.speeds[7], "Max");
    }

    #[test]
    fn the_module_scope_overrides_single_words() {
        let (shell, lal) = i18n::with_language("en", || (for_scope(None), for_scope(Some("lal"))));
        assert!(shell.end_turn_tip.contains("nations") && lal.end_turn_tip.contains("factions"));
        assert_eq!(shell.end_turn, lal.end_turn, "the other words stay the shell's");
    }

    /// Every plain word of the set, in field order (the Debug text between quotes).
    fn texts(words: &ShellWords) -> Vec<String> {
        let debug = format!("{words:?}");
        debug.split('"').skip(1).step_by(2).map(str::to_owned).collect()
    }

    #[test]
    fn russian_words_are_russian() {
        let en = texts(i18n::with_language("en", current));
        let ru = texts(i18n::with_language("ru", current));
        assert_eq!(en.len(), ru.len());
        for (a, b) in en.iter().zip(ru.iter()) {
            if international(a) || ["Tab", "Shift", "Esc"].iter().any(|k| a.contains(k)) {
                continue;
            }
            assert_ne!(a, b, "left in English in the Russian shell: {a}");
        }
    }

    #[test]
    fn a_language_switch_gives_other_words() {
        let ru = i18n::with_language("ru", current);
        assert_eq!(ru.end_turn, "\u{41a}\u{43e}\u{43d}\u{435}\u{446} \u{445}\u{43e}\u{434}\u{430}"); // Russian: "End turn"
        let ru_lal = i18n::with_language("ru", || for_scope(Some("lal")));
        assert_eq!(ru_lal.empire, "\u{418}\u{43c}\u{43f}\u{435}\u{440}\u{438}\u{44f}"); // Russian: "Empire"
        assert_eq!(i18n::with_language("ru", || for_scope(None)).empire, "\u{414}\u{435}\u{440}\u{436}\u{430}\u{432}\u{430}"); // Russian: "Realm"
    }
}
