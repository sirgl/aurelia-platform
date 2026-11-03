//! Player settings, grouped into four themed sections. The existing Settings
//! object remains the source of truth; application and persistence stay in App.
use super::kit::{button, panel, text};
use super::theme::Theme;
use super::screens::ScreenState;
use super::settings::{AntiAliasing, Quality, RenderScale, WindowMode, AUTOSAVE_INTERVALS, RESOLUTIONS};
use super::words::{Line, SettingsWords, ShellWords};
use egui::{pos2, vec2, FontId, Rect, Sense, Stroke};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Page { #[default] Audio, Graphics, Game, Controls }
impl Page {
    const ALL: [Self; 4] = [Self::Audio, Self::Graphics, Self::Game, Self::Controls];
    fn label(self, words: &SettingsWords) -> &'static str { match self { Self::Audio=>words.audio, Self::Graphics=>words.graphics, Self::Game=>words.game, Self::Controls=>words.controls } }
}

impl ScreenState {
    /// Explicit offscreen inspection; normal navigation remembers the player's last tab.
    pub fn capture_settings_tab(&mut self, ctx: &egui::Context, tab: &str) {
        let page = match tab { "graphics"=>Page::Graphics, "game"=>Page::Game, "controls"=>Page::Controls, _=>Page::Audio };
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("settings_page"),page));
    }
}

/// Keys and what they do, shown on the Controls page.
pub type KeyHelp = [(&'static str, &'static str)];

pub(super) fn draw(ui: &mut egui::Ui, r: Rect, state: &mut ScreenState, module: &dyn super::module::ModuleUi, keys: &KeyHelp) {
    let words = module.words();
    let c = Theme::of(ui.ctx()).colors;
    let rect = Rect::from_center_size(r.center(), vec2(r.width().min(1050.), r.height()));
    panel(ui.painter(), rect);
    let compact = rect.height() < 360.;
    let inner = rect.shrink(if compact { 10. } else { 20. });
    let id = egui::Id::new("settings_page");
    let mut page = ui.ctx().data(|data| data.get_temp::<Page>(id)).unwrap_or_default();
    let tab_width = (inner.width() - 24.) / 4.;
    for (i, item) in Page::ALL.iter().enumerate() {
        let cell = Rect::from_min_size(inner.min + vec2(i as f32 * (tab_width + 8.), 0.), vec2(tab_width, 36.));
        if button(ui, cell, item.label(&words.options), page == *item, true) { page = *item; }
    }
    ui.ctx().data_mut(|data| data.insert_temp(id, page));
    let body = Rect::from_min_max(inner.min + vec2(0., 48.), inner.max - vec2(0., if compact { 18. } else { 32. }));
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        ui.set_clip_rect(body);
        let style = ui.style_mut();
        style.spacing.interact_size.y = 36.;
        style.spacing.slider_rail_height = 8.;
        style.visuals.slider_trailing_fill = true;
        style.visuals.selection.bg_fill = c.selected;
        style.visuals.widgets.inactive.bg_fill = c.raised;
        style.visuals.widgets.inactive.weak_bg_fill = c.raised;
        style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.5, c.accent);
        style.visuals.widgets.hovered.bg_fill = c.selected;
        style.visuals.widgets.active.bg_fill = c.selected;
        style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, c.text);
        style.visuals.widgets.active.fg_stroke = Stroke::new(2., c.text);
        egui::ScrollArea::vertical().id_salt(("settings_body", page as u8))
            .max_height(body.height()).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                match page {
                    Page::Audio => audio(ui, state, &words.options),
                    Page::Graphics => graphics(ui, state, &words.options),
                    Page::Game => game(ui, state, words),
                    Page::Controls => super::shortcuts::controls(ui, state, module, keys, words.options.controls_help),
                }
            });
    });
    text(ui.painter(), pos2(inner.left(), inner.bottom() - 16.), words.options.applied, 12., c.muted);
}

/// Give the description and the control their own space; compact layouts stack them. A long
/// description (a language that needs more words) makes the row taller instead of spilling
/// over the next one.
fn row(ui: &mut egui::Ui, label: &str, description: &str) -> Rect {
    let width = ui.available_width();
    let stacked = width < 620.;
    let label_width = if stacked { width - 32. } else { width * 0.43 - 24. };
    let theme = Theme::of(ui.ctx());
    let c = theme.colors;
    let help = ui.painter().layout(description.to_owned(), FontId::proportional(12.), c.muted, label_width);
    let base: f32 = if stacked { 108. } else { 70. };
    // The description starts 36 below the top; the stacked control sits under it.
    let height = if stacked { base.max(64. + 44. + (help.size().y - 24.).max(0.)) } else { base.max(36. + help.size().y + 12.) };
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter().rect_filled(rect, theme.shapes.card_radius, c.raised);
    text(ui.painter(), rect.min + vec2(16., 12.), label, 17., c.text);
    ui.painter().galley(rect.min + vec2(16., 36.), help.clone(), c.muted);
    ui.add_space(8.);
    if stacked {
        let top = (36. + help.size().y + 6.).max(64.);
        Rect::from_min_max(rect.min + vec2(16., top), rect.max - vec2(16., 10.))
    } else {
        Rect::from_min_max(rect.min + vec2(width * 0.43, 16.), rect.max - vec2(16., 16.))
    }
}

/// Controls paint inside an already allocated row. A child must not allocate the
/// same space again or move the parent's cursor back into the preceding card.
fn control<R>(ui: &mut egui::Ui, rect: Rect, id: &str, draw: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(rect));
    draw(&mut child)
}

fn percent(ui: &mut egui::Ui, line: Line, value: &mut f32, range: std::ops::RangeInclusive<f32>) {
    let rect = row(ui, line.label, line.help);
    control(ui, rect, line.label, |ui| {
        ui.spacing_mut().slider_width = (rect.width() - 76.).max(60.);
        ui.add(egui::Slider::new(value, range).custom_formatter(|v, _| format!("{:.0}%", v * 100.)));
    });
}

fn toggle(ui: &mut egui::Ui, line: Line, words: &SettingsWords, value: &mut bool, enabled: bool) {
    let rect = row(ui, line.label, line.help);
    control(ui, rect, line.label, |ui| {
        let gap = 8.; let width = (rect.width() - gap) * 0.5;
        for (i, (target, name)) in [(false,words.off),(true,words.on)].iter().enumerate() {
            let cell = Rect::from_min_size(rect.min + vec2(i as f32 * (width + gap),0.), vec2(width,rect.height()));
            if button(ui, cell, name, *value==*target, enabled) { *value=*target; }
        }
    });
}

fn choices<T: Copy + PartialEq>(ui: &mut egui::Ui, label: &str, description: &str, value: &mut T, options: &[(T, &str)]) {
    let rect = row(ui,label,description);
    control(ui, rect, label, |ui| {
        let width = (rect.width() - 8. * (options.len()-1) as f32) / options.len() as f32;
        for (i,(target,name)) in options.iter().enumerate() {
            let cell = Rect::from_min_size(rect.min + vec2(i as f32*(width+8.),0.), vec2(width,rect.height()));
            if button(ui,cell,name,*value==*target,true) { *value=*target; }
        }
    });
}

fn audio(ui: &mut egui::Ui, state: &mut ScreenState, w: &SettingsWords) {
    let c = Theme::of(ui.ctx()).colors;
    let failed = state.music_error().map(str::to_owned);
    let s = &mut state.settings;
    percent(ui,w.master_volume,&mut s.master_volume,0.0..=1.0);
    percent(ui,w.music_volume,&mut s.music_volume,0.0..=1.0);
    toggle(ui,w.mute_music,w,&mut s.music_muted,true);
    percent(ui,w.effects_volume,&mut s.effects_volume,0.0..=1.0);
    toggle(ui,w.mute_effects,w,&mut s.effects_muted,true);
    if let Some(error)=failed { ui.label(egui::RichText::new(w.audio_failed).color(c.accent)).on_hover_text(error); }
}

fn graphics(ui: &mut egui::Ui, state: &mut ScreenState, w: &SettingsWords) {
    let s=&mut state.settings;
    choices(ui,w.window_mode.label,w.window_mode.help,&mut s.window,&[(WindowMode::Windowed,w.windowed),(WindowMode::Fullscreen,w.fullscreen)]);
    let rect=row(ui,w.window_size.label,if s.window==WindowMode::Windowed { w.window_size.help } else { w.window_size_fullscreen });
    control(ui, rect, "resolution", |ui| {
        ui.add_enabled_ui(s.window==WindowMode::Windowed,|ui| {
            egui::ComboBox::from_id_salt("settings_resolution").width(rect.width()-8.)
                .selected_text(format!("{} × {}",s.resolution.0,s.resolution.1)).show_ui(ui,|ui| {
                    for size in RESOLUTIONS { ui.selectable_value(&mut s.resolution,size,format!("{} × {}",size.0,size.1)); }
                });
        });
    });
    let level = |q: Quality| w.quality_levels[q as usize];
    let options = Quality::ALL.map(|q| (q, level(q).label));
    choices(ui,w.quality,level(s.quality).help,&mut s.quality,&options);
    let scales = RenderScale::ALL.map(|r| (r, match r.fraction() { None => w.auto.to_owned(), Some(_) => format!("{}%", r.key()) }));
    let scales = scales.each_ref().map(|(r, name)| (*r, name.as_str()));
    choices(ui,w.render_scale.label,w.render_scale.help,&mut s.render_scale,&scales);
    let aa = AntiAliasing::ALL.map(|a| (a, match a { AntiAliasing::Auto => w.auto, AntiAliasing::Off => w.off, AntiAliasing::X4 => "4×" }));
    choices(ui,w.anti_aliasing.label,w.anti_aliasing.help,&mut s.anti_aliasing,&aa);
    percent(ui,w.interface_scale,&mut s.ui_scale,0.75..=1.5);
    percent(ui,w.brightness,&mut s.brightness,0.5..=1.0);
}

fn language(ui: &mut egui::Ui, state: &mut ScreenState, w: &SettingsWords) {
    let c = Theme::of(ui.ctx()).colors;
    let with_pseudo = state.dev_mode || state.settings.language == i18n::locale::PSEUDO;
    let items = state.languages.get(with_pseudo).to_vec();
    let current = state.settings.language.clone();
    let shown = |tag: &str| items.iter().find(|i| i.tag == tag).map(|i| super::language::display_name(i, w.language_custom));
    let selected = if current == super::language::AUTO {
        w.language_auto.to_owned()
    } else {
        shown(&current).unwrap_or_else(|| current.clone())
    };
    let rect = row(ui, w.language.label, w.language.help);
    control(ui, rect, "language", |ui| {
        egui::ComboBox::from_id_salt("settings_language").width(rect.width()-8.).selected_text(selected).show_ui(ui, |ui| {
            ui.selectable_value(&mut state.settings.language, super::language::AUTO.to_owned(), w.language_auto);
            for item in &items {
                ui.selectable_value(&mut state.settings.language, item.tag.clone(), super::language::display_name(item, w.language_custom));
            }
        });
    });
    // What is not yet reviewed, said once under the row, in the language being shown.
    let active = if state.settings.language == super::language::AUTO { i18n::language() } else { state.settings.language.clone() };
    if let Some(status) = items.iter().find(|i| i.tag == active).and_then(|i| i.status.as_deref()) {
        let word = status.split_whitespace().next().unwrap_or("").trim_end_matches([':', ',', '.']).to_ascii_lowercase();
        let key = format!("shell.options.language-status.{word}");
        // i18n-keys: shell.options.language-status.*
        if let Some(text) = i18n::tr_dynamic(&key, &[]) {
            ui.label(egui::RichText::new(text).size(12.).color(c.muted));
            ui.add_space(8.);
        }
    }
}

/// A browser page offers its other mode (phone and tablet, desktop); a native build has no such row.
fn interface_mode(ui: &mut egui::Ui) {
    let Some(mode) = super::device::mode_switch() else { return };
    // i18n-keys: shell.options.web-mode.*
    let word = |key: &str| i18n::tr_dynamic(&format!("shell.options.web-mode.{key}"), &[]).unwrap_or_default();
    let (label, help, mobile, desktop) = (word("label"), word("help"), word("mobile"), word("desktop"));
    let rect = row(ui, &label, &help);
    control(ui, rect, "web_mode", |ui| {
        let width = (rect.width() - 8.) / 2.;
        for (i, (name, is_mobile)) in [(&mobile, true), (&desktop, false)].into_iter().enumerate() {
            let cell = Rect::from_min_size(rect.min + vec2(i as f32 * (width + 8.), 0.), vec2(width, rect.height()));
            if button(ui, cell, name, mode.mobile == is_mobile, true) && mode.mobile != is_mobile {
                (mode.switch)();
            }
        }
    });
}

fn game(ui: &mut egui::Ui,state: &mut ScreenState, words: &ShellWords) {
    use super::presentation::Speed;
    let w=&words.options;
    language(ui, state, w);
    interface_mode(ui);
    let s=&mut state.settings;
    let rect=row(ui,w.autosave.label,w.autosave.help);
    control(ui, rect, "autosave", |ui| {
        let name=|n| match n { 0=>w.never.to_owned(),1=>w.every_turn.to_owned(),n=>i18n::tf!("shell.options.every-turns", n = n) };
        egui::ComboBox::from_id_salt("settings_autosave").width(rect.width()-8.).selected_text(name(s.autosave_every)).show_ui(ui,|ui| {
            for n in AUTOSAVE_INTERVALS { ui.selectable_value(&mut s.autosave_every,n,name(n)); }
        });
    });
    let rect=row(ui,w.saves_kept.label,w.saves_kept.help);
    control(ui, rect, "autosaves_kept", |ui| {
        ui.spacing_mut().slider_width=(rect.width()-76.).max(60.);
        ui.add(egui::Slider::new(&mut s.autosaves_kept,1..=20));
    });
    choices(ui,w.other_turns.label,w.other_turns.help,&mut s.animation.speed,&Speed::ALL.map(|speed|(speed,w.speeds[speed as usize])));
    toggle(ui,w.reduced_motion,w,&mut s.animation.reduced_motion,true);
    let camera=Line { help: if s.animation.reduced_motion { w.wonder_camera_off } else { w.wonder_camera.help }, ..w.wonder_camera };
    toggle(ui,camera,w,&mut s.animation.camera_glides,!s.animation.reduced_motion);
}
