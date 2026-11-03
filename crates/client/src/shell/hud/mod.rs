//! Map HUD. Map contents, labels, selection and camera navigation use the observer's view;
//! game figures come from the module's `HudFeed` (absent in the showcase, whose dev mode
//! paints composition samples instead) and its panels are module HUD layers. The module
//! picks where the shell's controls stand (`frame`); its panels are laid out around them
//! (`layout`), and the map's labels and badges hide under both.
use super::module::{EndTurn, HudFeed, HudLayer, HudView, KindLabel, ModuleUi, Screen, ShellWords};
use super::layout;
pub use frame::{FrameRects, HudFrame};
use super::theme::Theme;
use render::{camera::CameraController, world::state::RenderWorld};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};

pub mod city_banner;
pub mod frame;
pub mod fog_ornaments;
pub mod map_paint;
mod tile_yields;
pub use tile_yields::tile_on_screen;
pub mod tips;

pub const INK: Color32 = Color32::from_rgba_premultiplied(10, 20, 29, 240);
pub const GOLD: Color32 = Color32::from_rgb(187, 156, 92);
pub const LIGHT: Color32 = Color32::from_rgb(235, 231, 213);
pub const MUTED: Color32 = Color32::from_rgb(166, 183, 190);

#[derive(Debug)]
pub struct VisualHudState {
    pub selected_tile: Option<u32>,
    pub hovered_tile: Option<u32>,
    pub hover_over_badge: bool,
    pub turn: u32,
    pub dev_mode: bool,
    /// The running match; `None` in the showcase.
    pub feed: Option<HudFeed>,
    pub show_grid: bool,
    pub show_labels: bool,
    pub show_yields: bool,
    pub show_settings: bool,
    /// Unit kinds of the running ruleset by index (badge names and symbols).
    pub unit_kinds: Vec<KindLabel>,
    /// Map feedback of the presentation, the selection ring and the route on the ground.
    pub overlay: super::map_feedback::MapOverlay,
    /// The unit badge under the pointer (its units' details arrive next frame).
    pub hovered_badge: Option<u32>,
    /// The top bar's row as the last frame laid it out.
    pub top_bar: layout::HudLayout,
    /// The cells of the top bar's values as the last frame drew them, in the feed's order.
    pub value_cells: Vec<Rect>,
    /// The city banners and unit badges the last frame drew (overlap checks).
    pub banners: Vec<Rect>,
    pub badges: Vec<Rect>,
}

impl Default for VisualHudState {
    fn default() -> Self {
        Self {
            selected_tile: None,
            hovered_tile: None,
            hover_over_badge: false,
            turn: 42,
            dev_mode: false,
            feed: None,
            show_grid: true,
            show_labels: true,
            show_yields: true,
            show_settings: false,
            unit_kinds: Vec::new(),
            overlay: Default::default(),
            hovered_badge: None,
            top_bar: Default::default(),
            value_cells: Vec::new(),
            banners: Vec::new(),
            badges: Vec::new(),
        }
    }
}
impl VisualHudState {
    /// The last frame's marks on the map and the top bar's items, named for
    /// `layout::shown_overlaps`.
    pub fn marks(&self) -> Vec<(String, Rect)> {
        let banners = self.banners.iter().map(|r| ("banner:".to_string(), *r));
        let badges = self.badges.iter().map(|r| ("badge:".to_string(), *r));
        let top = self.top_bar.placed().iter().map(|p| (format!("top:{}", p.name), p.rect));
        banners.chain(badges).chain(top).collect()
    }
}

#[derive(Debug, Default)]
pub struct VisualHudActions {
    pub toggle_grid: bool,
    pub camera_target: Option<(f32, f32)>,
    /// A city's label was clicked (the module checks whose it is).
    pub select_city: Option<u32>,
    /// The bow of an own city's banner was clicked.
    pub strike_city: Option<u32>,
    pub end_turn: bool,
    /// The top bar's reference button.
    pub open: Option<Screen>,
    /// The module's status cell in the top bar (`HudFeed::status`) was clicked.
    pub status_clicked: bool,
    /// The id of the top-bar value that was clicked (`TopBarValue::click`).
    pub top_clicked: Option<&'static str>,
}

/// The shell's own look (the showcase, which runs no module theme).
pub fn theme(ctx: &egui::Context) {
    super::theme::install(ctx, &super::theme::Theme::AURELIA);
}

/// The HUD over the map. The module's layers are drawn at their places in the order
/// (egui stacks areas in the order they are first drawn). `scene`: the world when it is
/// not the module's match (the showcase); the HUD otherwise draws over the module's view.
pub fn draw(
    ctx: &egui::Context,
    scene: Option<&RenderWorld>,
    camera: &CameraController,
    state: &mut VisualHudState,
    surface: Option<&render::scene::terrain::surface::SurfaceMesh>,
    module: &mut dyn ModuleUi,
) -> VisualHudActions {
    state.hover_over_badge = false;
    state.hovered_badge = None;
    let mut actions = VisualHudActions::default();
    let screen = ctx.content_rect();
    let view = HudView { scene, screen, selected_tile: state.selected_tile };
    let reserved = module.hud_reserved(&view);
    let frame = module.hud_frame();
    let rects = frame.rects(screen);
    layout::begin(ctx, screen, rects.controls());
    for panel in module.hud_panels(&view) {
        layout::place(ctx, panel);
    }
    let words = module.words();
    let world = world_of(scene, module);
    fog_ornaments::draw(ctx, world, camera);
    if let Some(paint) = module.map_paint() {
        map_paint::draw(ctx, world, camera, surface, paint);
    }
    let fit = tile_yields::label_fit(camera, screen, module.labels_follow_zoom());
    if state.show_yields && module.yields_shown() {
        tile_yields::draw(ctx, world, camera, state, surface, reserved, &*module, fit);
    }
    if let Some(feed) = &state.feed {
        let (this_turn, turns) = feed.route.as_ref().map_or((0, 0), |route| (route.this_turn, route.turns));
        super::map_feedback::draw_ground(ctx, camera, &state.overlay, this_turn, turns, words);
    }
    super::map_feedback::draw_marks(ctx, camera, &state.overlay, words);
    let bar = top_bar(ctx, frame, &rects, state, words);
    actions.open = bar.open;
    actions.status_clicked = bar.status_clicked;
    actions.top_clicked = bar.value_clicked;
    if state.feed.is_none() && state.dev_mode && screen.width() >= 800. {
        layout::add_control(ctx, research(ctx));
    }
    module.draw_hud(ctx, HudLayer::Status, &view);
    let world = world_of(scene, module);
    let banners = match state.show_labels {
        true => city_labels(ctx, screen, world, camera, state, reserved, &mut actions, words, fit),
        false => Vec::new(),
    };
    state.badges.clear();
    unit_badges(ctx, screen, world, camera, state, &banners, words, fit);
    layout::set_labels(ctx, banners.clone());
    state.banners = banners;
    map_controls(ctx, &rects, world, state, &mut actions, words);
    minimap(ctx, frame, &rects, world, camera, &mut actions, state.feed.as_ref(), words);
    module.draw_hud(ctx, HudLayer::Panels, &view);
    actions
}

/// The world the HUD draws over: the scene, else the module's match, else nothing.
fn world_of<'a>(scene: Option<&'a RenderWorld>, module: &'a dyn ModuleUi) -> &'a RenderWorld {
    static EMPTY: std::sync::OnceLock<RenderWorld> = std::sync::OnceLock::new();
    scene.or_else(|| module.view()).unwrap_or_else(|| EMPTY.get_or_init(RenderWorld::default))
}

pub fn text(
    p: &egui::Painter,
    at: Pos2,
    align: Align2,
    value: impl ToString,
    size: f32,
    color: Color32,
) {
    p.text(at, align, value, FontId::proportional(size), color);
}
pub fn panel(p: &egui::Painter, rect: Rect, radius: f32) {
    p.rect_filled(
        rect.translate(vec2(0.0, 3.0)),
        radius,
        Color32::from_black_alpha(60),
    );
    let hud = &Theme::of(p.ctx()).colors;
    p.rect_filled(rect, radius, hud.hud_fill);
    p.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, hud.hud_accent.gamma_multiply(0.8)),
        egui::StrokeKind::Inside,
    );
}

/// The composition samples of the showcase's top bar (dev mode, no match).
fn sample_values() -> Vec<super::module::TopBarValue> {
    let sample = |icon, text: &str, tint| super::module::TopBarValue {
        icon,
        tint,
        text: text.into(),
        compact: text.into(),
        tip: None,
        click: None,
        ready: false,
    };
    vec![
        sample(Icon::Coin, "312 (+18)", GOLD),
        sample(Icon::Apple, "120 (+9)", Color32::from_rgb(137, 188, 83)),
        sample(Icon::Flask, "56 (+7)", Color32::from_rgb(100, 198, 239)),
        sample(Icon::Note, "28 (+4)", Color32::from_rgb(186, 136, 216)),
    ]
}

/// What was pressed in the top bar.
#[derive(Debug, Default)]
struct TopBarClicks {
    /// The screen the reference button opens.
    open: Option<Screen>,
    /// The module's status cell.
    status_clicked: bool,
    /// The click target id of the value that was clicked.
    value_clicked: Option<&'static str>,
}

/// Room a status cell needs beyond its text: padding at both ends and the chevron.
const STATUS_EXTRA: f32 = 8. + 8. + 16.;

/// The top bar: the module's values, the turn (or the module's status cell), settings and
/// the reference button (returns what was pressed). Its row is laid out (`TopBarRow`), so
/// nothing in it covers anything else.
fn top_bar(ctx: &egui::Context, frame: HudFrame, rects: &FrameRects, state: &mut VisualHudState, words: &ShellWords) -> TopBarClicks {
    let mut clicks = TopBarClicks::default();
    let screen = rects.top_bar;
    let hud = Theme::of(ctx).colors;
    let fill = if frame.opaque_top() { hud.background } else { hud.hud_fill };
    let values = match &state.feed {
        Some(feed) => feed.top_bar.clone(),
        None if state.dev_mode => sample_values(),
        None => Vec::new(),
    };
    let status = state.feed.as_ref().and_then(|feed| feed.status.clone());
    let (turn, short_turn) = match (&state.feed, &status) {
        (_, Some(status)) => (status.text.clone(), status.short.clone()),
        (Some(feed), None) => (format!("{}   |   {} {}", feed.nation, words.turn, feed.turn), format!("{} {}", words.turn, feed.turn)),
        (None, None) => (format!("{} {}", words.turn, state.turn), format!("{} {}", words.turn, state.turn)),
    };
    let reference = state.feed.as_ref().and_then(|feed| feed.reference);
    // The full values on a wide window, the numbers alone on a narrow one.
    let compact = screen.width() < 1300.;
    let shown: Vec<String> = values.iter().map(|v| if compact { v.compact.clone() } else { v.text.clone() }).collect();
    let font = FontId::proportional(14.);
    let measure = |text: &str| ctx.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), hud.hud_text).size().x);
    let values_width = shown.iter().map(|t| measure(t) + 28.).sum::<f32>() + 18. * shown.len().saturating_sub(1) as f32;
    let bar = rects.top_bar;
    let empire = bar.contains_rect(rects.empire).then_some(rects.empire);
    let extra = if status.is_some() { STATUS_EXTRA } else { 0. };
    let turn_widths = if screen.width() >= 1300. { (measure(&turn) + extra, measure(&short_turn) + extra) } else { (measure(&short_turn) + extra, measure(&short_turn) + extra) };
    let row = TopBarRow::lay_out(bar, empire, values_width, turn_widths, reference.is_some());
    let mut cells = Vec::with_capacity(values.len());
    let turn = if row.turn.width() + 0.5 >= turn_widths.0 && screen.width() >= 1300. { turn } else { short_turn };
    egui::Area::new("visual_top".into())
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(bar.size(), Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, 0.0, fill);
            p.line_segment(
                [r.left_bottom(), r.right_bottom()],
                Stroke::new(1.0, hud.hud_accent.gamma_multiply(0.65)),
            );
            // The values own a clipped, scrollable strip: the turn and the buttons keep their room.
            ui.scope_builder(egui::UiBuilder::new().max_rect(row.values), |ui| {
                ui.set_clip_rect(row.values);
                // Touch targets raise the row's minimum height to 48: the 30 point cells would hang
                // below the centre of a 40 point bar.
                ui.spacing_mut().interact_size.y = 30.;
                egui::ScrollArea::horizontal().id_salt("hud_economy")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 18.;
                            for (value, shown) in values.iter().zip(shown) {
                                let galley = ui.painter().layout_no_wrap(shown, font.clone(), hud.hud_text);
                                // A value with a target is a button (touch and keyboard included).
                                let sense = if value.click.is_some() { Sense::click() } else { Sense::hover() };
                                let (cell, response) = ui.allocate_exact_size(vec2(galley.size().x + 28., 30.), sense);
                                cells.push(cell);
                                value_backdrop(ui.painter(), cell, &response, value, hud.hud_accent);
                                icon(ui.painter(), cell.left_center() + vec2(10., 0.), 9., value.icon, value.tint);
                                ui.painter().galley(cell.left_center() + vec2(25., -galley.size().y * 0.5), galley, hud.hud_text);
                                let response = if value.click.is_some() { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response };
                                if response.clicked() {
                                    clicks.value_clicked = value.click.as_ref().map(|click| click.id);
                                }
                                response.on_hover_ui(|ui| {
                                    if let Some(tip) = &value.tip { tips::yield_tip(ui, tip); }
                                    else { ui.label(&value.text); }
                                    if let Some(click) = &value.click { tips::click_hint(ui, &click.hint); }
                                });
                            }
                        });
                    });
            });
            match &status {
                Some(status) => clicks.status_clicked = status_cell(ui, row.turn, &turn, status, &font),
                None => {
                    let turn_at = pos2(row.turn.right(), row.turn.center().y);
                    ui.painter().with_clip_rect(row.turn).text(turn_at, Align2::RIGHT_CENTER, turn, font.clone(), hud.hud_text);
                }
            }
            let face = |slot: Rect| Rect::from_center_size(slot.center(), vec2(29., 29.));
            if round_button(ui, face(row.gear), "visual_settings", Icon::Gear, state.show_settings, words.hud.settings_tip) {
                state.show_settings = !state.show_settings;
            }
            if let (Some((screen, tip)), Some(book)) = (reference, row.book) {
                if round_button(ui, face(book), "visual_pedia", Icon::Book, false, tip) {
                    clicks.open = Some(screen);
                }
            }
        });
    state.top_bar = row.layout;
    state.value_cells = cells;
    clicks
}

/// The cell behind a top-bar value: lit under the pointer when it can be clicked, outlined
/// when the player can act on it now (`TopBarValue::ready`), ringed while it has the keyboard focus.
fn value_backdrop(p: &egui::Painter, cell: Rect, response: &egui::Response, value: &super::module::TopBarValue, accent: Color32) {
    let clickable = value.click.is_some();
    let lit = clickable && response.hovered();
    let fill = match (lit, value.ready) {
        (true, _) => Some(Color32::from_rgb(40, 77, 97)),
        (false, true) => Some(accent.gamma_multiply(0.22)),
        _ => None,
    };
    if let Some(fill) = fill {
        p.rect_filled(cell, 4., fill);
    }
    if value.ready {
        p.rect_stroke(cell, 4., Stroke::new(1.5, accent), egui::StrokeKind::Inside);
    } else if clickable && response.has_focus() {
        p.rect_stroke(cell, 4., Stroke::new(1., accent.gamma_multiply(0.8)), egui::StrokeKind::Inside);
    }
}

/// The module's status cell: `shown` (the full or the short text) left of a chevron, a
/// button that is lit under the pointer and pressed while its panel is open. True when clicked.
fn status_cell(ui: &mut egui::Ui, rect: Rect, shown: &str, status: &super::module::StatusLine, font: &FontId) -> bool {
    let hud = Theme::of(ui.ctx()).colors;
    let cell = rect.shrink2(vec2(0., 1.));
    let response = ui.interact(cell, "visual_status".into(), Sense::click());
    let p = ui.painter();
    let fill = if response.hovered() {
        Some(Color32::from_rgb(40, 77, 97))
    } else if status.open {
        Some(Color32::from_rgb(25, 58, 82))
    } else {
        None
    };
    if let Some(fill) = fill {
        p.rect_filled(cell, 3., fill);
    }
    if status.open {
        p.rect_stroke(cell, 3., Stroke::new(1., hud.hud_accent.gamma_multiply(0.8)), egui::StrokeKind::Inside);
    }
    let text_box = Rect::from_min_max(cell.min + vec2(8., 0.), pos2(cell.right() - (STATUS_EXTRA - 8.), cell.max.y));
    p.with_clip_rect(text_box).text(pos2(text_box.left(), cell.center().y), Align2::LEFT_CENTER, shown, font.clone(), hud.hud_text);
    super::widgets::fold::chevron(p, pos2(cell.right() - 12., cell.center().y), status.open, hud.hud_text);
    let clicked = response.clicked();
    if status.tip.is_empty() {
        return clicked;
    }
    response.on_hover_text(&status.tip);
    clicked
}

/// The top bar's row: the values at the left, the turn, the reference book and the
/// settings gear at the right, laid out like the HUD's panels (`layout`): the buttons
/// keep their size, the turn drops to its short form, the values shrink into a strip.
struct TopBarRow {
    values: Rect,
    turn: Rect,
    book: Option<Rect>,
    gear: Rect,
    layout: layout::HudLayout,
}

impl TopBarRow {
    /// `empire`: the Empire button when the frame puts it into the bar (drawn by the screens).
    fn lay_out(bar: Rect, empire: Option<Rect>, values_width: f32, (turn, short_turn): (f32, f32), book: bool) -> Self {
        use layout::{Anchor, Panel};
        let mut row = layout::HudLayout::new(bar, empire.into_iter().collect());
        let cy = bar.center().y;
        // A round button's slot holds its shadow too (`round_button` paints 2 beyond).
        let button = Rect::from_center_size(pos2(bar.right() - 29., cy), vec2(33., 33.));
        let gear = row.place(Panel::at("top_gear", Anchor::TopRight, button).priority(100)).unwrap_or(button);
        let book = book.then(|| row.place(Panel::at("top_book", Anchor::TopRight, button).priority(90))).flatten();
        let text = |width: f32| Rect::from_min_max(pos2(gear.left() - layout::GAP - width, cy - 15.), pos2(gear.left() - layout::GAP, cy + 15.));
        let turn_rect = text(turn);
        let turn = row.place(Panel::at("top_turn", Anchor::TopRight, turn_rect).min(vec2(short_turn, 30.)).priority(80)).unwrap_or(turn_rect);
        // The strip keeps 4 points above and below, 3 in a bar as thin as 36 (its cells are 30).
        let pad = ((bar.height() - 30.) / 2.).min(4.);
        let strip = Rect::from_min_size(bar.min + vec2(10., pad), vec2(values_width.max(1.), bar.height() - 2. * pad));
        let least = vec2(values_width.clamp(1., 120.), strip.height());
        let values = row.place(Panel::at("top_values", Anchor::TopLeft, strip).min(least).priority(50)).unwrap_or(strip);
        TopBarRow { values, turn, book, gear, layout: row }
    }
}

/// The research sample of the showcase; returns its rect.
fn research(ctx: &egui::Context) -> Rect {
    let bounds = Rect::from_min_size(pos2(0.0, 59.0), vec2(249.0, 87.0));
    egui::Area::new("visual_research".into())
        .fixed_pos(bounds.min)
        .show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(bounds.size(), Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, 0.0, Color32::from_rgba_premultiplied(13, 26, 36, 204));
            p.line_segment(
                [r.left_bottom(), r.right_bottom()],
                Stroke::new(1.0, GOLD.gamma_multiply(0.48)),
            );
            let c = pos2(r.left() + 35.0, r.center().y - 3.0);
            p.circle_filled(c, 26.0, INK);
            p.circle_stroke(c, 26.0, Stroke::new(1.0, GOLD));
            p.circle_stroke(c, 23.0, Stroke::new(1.0, Color32::from_rgb(70, 88, 100)));
            icon(p, c, 14.0, Icon::Flask, Color32::from_rgb(108, 207, 242));
            text(
                p,
                r.min + vec2(76.0, 12.0),
                Align2::LEFT_TOP,
                i18n::t!("shell.showcase.research"),
                10.0,
                MUTED,
            );
            text(
                p,
                r.min + vec2(76.0, 30.0),
                Align2::LEFT_TOP,
                i18n::t!("shell.showcase.research-name"),
                18.0,
                LIGHT,
            );
            text(
                p,
                r.min + vec2(76.0, 53.0),
                Align2::LEFT_TOP,
                i18n::t!("shell.showcase.research-turns"),
                12.0,
                MUTED,
            );
            let bar = Rect::from_min_size(r.min + vec2(76.0, 73.0), vec2(156.0, 3.0));
            p.rect_filled(bar, 1.0, Color32::from_rgb(48, 67, 77));
            p.rect_filled(
                Rect::from_min_size(bar.min, vec2(102.0, 3.0)),
                1.0,
                Color32::from_rgb(104, 193, 223),
            );
        });
    bounds
}

fn city_labels(
    ctx: &egui::Context,
    screen: Rect,
    world: &RenderWorld,
    camera: &CameraController,
    state: &mut VisualHudState,
    reserved: Option<Rect>,
    out: &mut VisualHudActions,
    words: &ShellWords,
    fit: city_banner::Fit,
) -> Vec<Rect> {
    let mut drawn = Vec::new();
    // A hex too narrow for a readable banner has none: the city shows by its model.
    if world.width == 0 || !fit.banners {
        return drawn;
    }
    let layout = layout::current(ctx);
    for city in &world.cities {
        let tile = city.tile.0;
        let (x, z) =
            render::scene::layout::tile_to_world(tile % world.width, tile / world.width);
        let Some((sx, sy)) = camera.project_to_screen(
            [x, 1.65, z],
            screen.aspect_ratio(),
            screen.width(),
            screen.height(),
        ) else {
            continue;
        };
        let banner = state.feed.as_ref().and_then(|feed| feed.banners.iter().find(|b| b.tile == tile));
        let at = city_banner::rect_at(pos2(sx, sy), banner, fit);
        let size = at.size();
        // The banner's top edge stays below the top bar: 20 px above the anchor (or above
        // the anchor's lift when the banner stands above the city).
        let top_anchor = if fit.above { at.top() + 20. * fit.k } else { sy };
        if top_anchor < 66.0 || sy > screen.height() - 55.0 {
            continue;
        }
        let label_rect = Rect::from_min_max(at.min - vec2(0., 3. * fit.k), at.max + vec2(0., 3. * fit.k));
        if reserved.is_some_and(|panel| panel.intersects(label_rect)) || layout.covers(label_rect) {
            continue;
        }
        drawn.push(at);
        egui::Area::new(egui::Id::new(("visual_city", tile)))
            .fixed_pos(at.min)
            .show(ctx, |ui| {
                let (r, response) = ui.allocate_exact_size(size, Sense::click());
                let rgb = render::gpu::buffers::owner_color(city.owner);
                let tint = Color32::from_rgb((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8);
                let selected = state.selected_tile == Some(tile);
                city_banner::paint_at(ui.painter(), r, city, banner, tint, selected, words, fit.k);
                let own = city.owner == world.observer;
                if response.clicked() {
                    // Any owner: the module decides what a foreign city's label does.
                    if own {
                        state.selected_tile = Some(tile);
                    }
                    out.select_city = Some(tile);
                }
                match banner.and_then(|b| b.tip.clone()) {
                    Some(tip) => response.on_hover_text(short_tip(&tip)),
                    None => response.on_hover_text(city_banner::tooltip(city, banner, words)),
                };
            });
        if banner.is_some_and(|b| b.can_strike) {
            // The bow left of the banner: aim the city's ranged strike.
            let bow = city_banner::strike_rect_at(at, fit.k);
            egui::Area::new(egui::Id::new(("city_strike", tile)))
                .fixed_pos(bow.min)
                .show(ctx, |ui| {
                    let (r, response) = ui.allocate_exact_size(bow.size(), Sense::click());
                    city_banner::bow(ui.painter(), r);
                    if response.clicked() {
                        out.strike_city = Some(tile);
                    }
                    response.on_hover_text(words.hud.strike_ready);
                });
        }
    }
    drawn
}

/// Visual lines a short hover tip holds at most, and the characters of one: a tip card is
/// 300-360 points wide at 12-14 points, which is about 44 characters of Russian text.
pub const TIP_LINES: usize = 3;
pub const TIP_LINE_CHARS: usize = 44;

/// `text` as a hover tip of at most `TIP_LINES` visual lines: the lines of the text (empty
/// ones dropped) are wrapped at `TIP_LINE_CHARS` characters between words, and what does not
/// fit ends the last line with «…» (the rest is on the screen the control leads to). The
/// wraps are written into the result, so the card shows exactly the lines counted here.
pub fn short_tip(text: &str) -> String {
    short_text(text, TIP_LINES, TIP_LINE_CHARS)
}

/// `short_tip` for a card of another size: at most `lines` lines of `chars` characters.
pub fn short_text(text: &str, lines: usize, chars: usize) -> String {
    let mut wrapped: Vec<String> = Vec::new();
    for source in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        wrap(source, chars, &mut wrapped);
    }
    if wrapped.len() > lines {
        wrapped.truncate(lines);
        if let Some(last) = wrapped.last_mut() {
            while last.chars().count() >= chars {
                last.pop();
            }
            let kept = last.trim_end().len();
            last.truncate(kept);
            last.push('…');
        }
    }
    wrapped.join("\n")
}

/// Greedy wrap of one line at `width` characters between words; a word longer than the
/// width is broken.
fn wrap(source: &str, width: usize, out: &mut Vec<String>) {
    let mut line = String::new();
    let mut length = 0;
    for word in source.split_whitespace() {
        let mut word = word;
        let mut size = word.chars().count();
        if length > 0 && length + 1 + size <= width {
            line.push(' ');
            line.push_str(word);
            length += 1 + size;
            continue;
        }
        if length > 0 {
            out.push(std::mem::take(&mut line));
        }
        while size > width {
            let at = word.char_indices().nth(width).map_or(word.len(), |(i, _)| i);
            out.push(word[..at].to_owned());
            word = &word[at..];
            size -= width;
        }
        line.push_str(word);
        length = size;
    }
    if length > 0 {
        out.push(line);
    }
}

/// These are observational markers, not buttons. Count means actual unit markers,
/// never the number of decorative soldiers emitted for one formation.
#[derive(Debug, PartialEq)]
struct UnitBadge {
    tile: u32,
    owner: u8,
    kind: Option<u16>,
    count: usize,
    /// Feet of a unit the presentation moves: its badge follows it.
    at: Option<[f32; 3]>,
}
/// Badges per tile and owner; a unit the presentation moves (`posed`) has its own badge
/// that follows it.
fn visible_unit_badges(world: &RenderWorld, posed: &[(u64, [f32; 3])]) -> Vec<UnitBadge> {
    let mut groups = std::collections::BTreeMap::<(u32, u8), UnitBadge>::new();
    let mut moving = Vec::new();
    let tile_count = world.width.saturating_mul(world.height);
    for unit in &world.units {
        let tile = unit.tile.0;
        if tile >= tile_count || world.visible.get(tile as usize).copied().unwrap_or(0) == 0 {
            continue;
        }
        if let Some((_, at)) = posed.iter().find(|(id, _)| *id == unit.id) {
            let at = Some(*at);
            moving.push(UnitBadge { tile, owner: unit.owner, kind: Some(unit.kind), count: 1, at });
            continue;
        }
        let group = groups.entry((tile, unit.owner)).or_insert(UnitBadge {
            tile,
            owner: unit.owner,
            kind: Some(unit.kind),
            count: 0,
            at: None,
        });
        group.count += 1;
        if group.kind != Some(unit.kind) {
            group.kind = None;
        }
    }
    groups.into_values().chain(moving).collect()
}
/// A badge never covers a city's banner: one that would is lifted above it (and above a
/// banner it meets there); None when it finds no room within a few banners.
fn clear_of_banners(mut rect: Rect, banners: &[Rect]) -> Option<Rect> {
    for _ in 0..4 {
        let Some(banner) = banners.iter().find(|b| layout::overlap(**b, rect)) else { return Some(rect) };
        rect = rect.translate(vec2(0.0, banner.top() - 3.0 - rect.bottom()));
    }
    None
}

fn unit_badge_name(kinds: &[KindLabel], kind: Option<u16>, words: &ShellWords) -> String {
    match kind.and_then(|k| kinds.get(k as usize)) {
        Some(unit) => unit.name.clone(),
        None => words.hud.units.into(),
    }
}
fn unit_badges(
    ctx: &egui::Context,
    screen: Rect,
    world: &RenderWorld,
    camera: &CameraController,
    state: &mut VisualHudState,
    banners: &[Rect],
    words: &ShellWords,
    fit: city_banner::Fit,
) {
    if world.width == 0 {
        return;
    }
    let k = fit.k;
    // A map overlay: below every panel (decision D309), and not drawn where one stands.
    let painter = layout::map_painter(ctx, "visible_unit_badges");
    let layout = layout::current(ctx);
    let mut previous_tile = None;
    let mut row = 0;
    for badge in visible_unit_badges(world, &state.overlay.posed) {
        if previous_tile == Some(badge.tile) {
            row += 1;
        } else {
            row = 0;
        }
        previous_tile = Some(badge.tile);
        let city = badge.at.is_none() && state.show_labels && world.cities.iter().any(|c| c.tile.0 == badge.tile);
        let (x, z) = render::scene::layout::tile_to_world(
            badge.tile % world.width,
            badge.tile / world.width,
        );
        let height = if city { 1.65 } else { 1.95 };
        let anchor = match badge.at {
            Some([x, y, z]) => [x, y + 1.5, z],
            None => [x, height, z],
        };
        let Some((sx, sy)) = camera.project_to_screen(
            anchor,
            screen.aspect_ratio(),
            screen.width(),
            screen.height(),
        ) else {
            continue;
        };
        let width = (if badge.count > 1 { 49.0 } else { 29.0 }) * k;
        let mut origin = if city {
            // Beside the city's banner, level with its top edge.
            match banners.iter().find(|b| fit.above && (b.center().x - sx).abs() < 1.0 && (b.bottom() - (sy - 20.0 * k)).abs() < 1.0) {
                Some(banner) => pos2(banner.right() + 6.0 * k, banner.top() + 4.0 * k),
                None => pos2(sx + city_banner::WIDTH * k * 0.5 + 6.0 * k, sy - 16.0 * k),
            }
        } else {
            pos2(sx - width * 0.5, sy - 32.0 * k)
        };
        if city && origin.x + width > screen.right() - 12.0 {
            origin = pos2(sx - width * 0.5, sy + 19.0 * k);
        }
        origin.y += row as f32 * 30.0 * k;
        let Some(rect) = clear_of_banners(Rect::from_min_size(origin, vec2(width, 27.0 * k)), banners) else { continue };
        // A badge lifted over a banner leaves its leader out: it would cross the name.
        let lifted = rect.min != origin;
        // Panels and the shell's controls stay unobstructed.
        if rect.top() < 48.0 || rect.bottom() > screen.bottom() - 12.0 || layout.covers(rect) {
            continue;
        }
        state.badges.push(rect);
        let rgb = render::gpu::buffers::owner_color(badge.owner);
        let tint = Color32::from_rgb(
            (rgb[0] * 255.0) as u8,
            (rgb[1] * 255.0) as u8,
            (rgb[2] * 255.0) as u8,
        );
        if !city && !lifted {
            painter.line_segment(
                [rect.center_bottom(), pos2(sx, sy + 5.0 * k)],
                Stroke::new(3.0, Color32::from_black_alpha(150)),
            );
            painter.line_segment(
                [rect.center_bottom(), pos2(sx, sy + 5.0 * k)],
                Stroke::new(1.0, LIGHT),
            );
            painter.circle_filled(pos2(sx, sy + 5.0 * k), 2.3 * k, tint);
        }
        painter.rect_filled(
            rect.translate(vec2(0.0, 2.0 * k)),
            4.0 * k,
            Color32::from_black_alpha(90),
        );
        painter.rect_filled(rect, 4.0 * k, Color32::from_rgb(12, 24, 33));
        let selected = state.selected_tile == Some(badge.tile);
        painter.rect_stroke(
            rect,
            4.0 * k,
            Stroke::new(
                if selected { 2.0 } else { 1.0 },
                if selected {
                    GOLD
                } else {
                    LIGHT.gamma_multiply(0.65)
                },
            ),
            egui::StrokeKind::Inside,
        );
        let emblem = Rect::from_min_size(rect.min + vec2(2.0, 2.0) * k, vec2(25.0, 23.0) * k);
        painter.rect_filled(emblem, 3.0 * k, tint);
        let civilian = badge
            .kind
            .and_then(|k| state.unit_kinds.get(k as usize))
            .is_some_and(|k| k.civilian);
        let kind = if civilian {
            Icon::Leaf
        } else {
            Icon::Shield
        };
        icon(&painter, emblem.center(), 7.8 * k, kind, LIGHT);
        if ctx.pointer_hover_pos().is_some_and(|p| rect.contains(p)) {
            state.hover_over_badge = true;
            state.hovered_badge = Some(badge.tile);
            let details = match state.feed.as_ref().and_then(|feed| feed.units.as_ref()) {
                Some((tile, units)) if *tile == badge.tile => units.iter().filter(|u| u.player == badge.owner).cloned().collect(),
                _ => Vec::new(),
            };
            egui::Tooltip::always_open(
                ctx.clone(),
                painter.layer_id(),
                egui::Id::new(("unit_details", badge.tile, badge.owner)),
                rect,
            )
            .show(|ui| {
                if details.is_empty() {
                    ui.label(unit_badge_name(&state.unit_kinds, badge.kind, words));
                }
                for unit in &details {
                    tips::unit_tip(ui, unit, words);
                }
                if city {
                    ui.label(words.hud.garrison);
                }
            });
        }
        if badge.count > 1 {
            text(
                &painter,
                rect.right_center() - vec2(7.0 * k, 0.0),
                Align2::RIGHT_CENTER,
                badge.count,
                11.0 * k,
                GOLD,
            );
        }
    }
}

fn map_controls(
    ctx: &egui::Context,
    rects: &FrameRects,
    world: &RenderWorld,
    state: &mut VisualHudState,
    out: &mut VisualHudActions,
    words: &ShellWords,
) {
    let (side, step) = (rects.button_side, rects.button_step);
    let bounds = rects.map_buttons;
    egui::Area::new("visual_map_controls".into())
        .fixed_pos(bounds.min)
        .show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(bounds.size(), Sense::hover());
            let first = Rect::from_center_size(
                r.min + vec2(side * 0.5 + 1., side * 0.5 + 1.),
                vec2(side, side),
            );
            if round_button(
                ui,
                first,
                "visual_home",
                Icon::Compass,
                false,
                &super::keymap::with_hint(ui.ctx(), words.hud.center_capital, "center-selection"),
            ) {
                if let Some(city) = world
                    .cities
                    .iter()
                    .find(|c| c.owner == world.observer)
                    .or(world.cities.first())
                {
                    out.camera_target = Some(render::scene::layout::tile_to_world(
                        city.tile.0 % world.width.max(1),
                        city.tile.0 / world.width.max(1),
                    ));
                }
            }
            if round_button(
                ui,
                first.translate(vec2(step, 0.0)),
                "visual_grid",
                Icon::Hex,
                state.show_grid,
                &super::keymap::with_hint(ui.ctx(), words.hud.toggle_grid, "toggle-grid"),
            ) {
                out.toggle_grid = true;
            }
            if round_button(
                ui,
                first.translate(vec2(step * 2., 0.0)),
                "visual_labels",
                Icon::City,
                state.show_labels,
                words.hud.toggle_labels,
            ) {
                state.show_labels = !state.show_labels;
            }
            if round_button(
                ui,
                first.translate(vec2(step * 3., 0.0)),
                "visual_yields",
                Icon::Hammer,
                state.show_yields,
                &super::keymap::with_hint(ui.ctx(), words.hud.toggle_yields, "toggle-yields"),
            ) {
                state.show_yields = !state.show_yields;
            }
        });
}

/// The minimap, and the end-turn button where the frame puts it.
fn minimap(
    ctx: &egui::Context,
    frame: HudFrame,
    rects: &FrameRects,
    world: &RenderWorld,
    camera: &CameraController,
    out: &mut VisualHudActions,
    feed: Option<&HudFeed>,
    words: &ShellWords,
) {
    let compact = rects.compact;
    let button = feed.and_then(|feed| feed.end_turn.as_ref());
    if let (HudFrame::Wide, Some(button)) = (frame, button) {
        egui::Area::new("visual_end_turn".into()).fixed_pos(rects.end_turn.min).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(rects.end_turn.size(), Sense::hover());
            out.end_turn = end_turn(ui, r, EndTurnLook::Primary, button, words);
        });
    }
    egui::Area::new("visual_minimap".into())
        .fixed_pos(rects.minimap_block.min)
        .show(ctx, |ui| {
            let (bounds, _) = ui.allocate_exact_size(rects.minimap_block.size(), Sense::hover());
            let r = rects.minimap.translate(bounds.min - rects.minimap_block.min);
            let p = ui.painter();
            panel(p, r, 5.0);
            let accent = Theme::of(ctx).colors.hud_accent;
            let map = r.shrink2(vec2(10.0, 12.0));
            p.rect_filled(map, 2.0, Color32::from_rgb(10, 22, 32));
            draw_minimap(p, map, world, camera);
            for corner in [
                r.left_top(),
                r.right_top(),
                r.left_bottom(),
                r.right_bottom(),
            ] {
                let dx = if corner.x == r.left() { 1.0 } else { -1.0 };
                let dy = if corner.y == r.top() { 1.0 } else { -1.0 };
                p.line_segment(
                    [
                        corner + vec2(dx * 5.0, dy * 18.0),
                        corner + vec2(dx * 5.0, dy * 5.0),
                    ],
                    Stroke::new(2.0, accent),
                );
                p.line_segment(
                    [
                        corner + vec2(dx * 5.0, dy * 5.0),
                        corner + vec2(dx * 18.0, dy * 5.0),
                    ],
                    Stroke::new(2.0, accent),
                );
            }
            let response = ui.interact(map, "visual_minimap_click".into(), Sense::click_and_drag());
            if (response.clicked() || response.dragged()) && world.width > 0 && world.height > 0 {
                if let Some(pointer) = response.interact_pointer_pos() {
                    let ext = map_extent(world);
                    out.camera_target = Some((
                        (pointer.x - map.left()) / map.width() * ext.0,
                        (pointer.y - map.top()) / map.height() * ext.1,
                    ));
                }
            }
            response.on_hover_text(words.hud.minimap_tip);
            if let Some(button) = button {
                if frame == HudFrame::Classic {
                    let look = if compact { EndTurnLook::Compact } else { EndTurnLook::Classic };
                    out.end_turn = end_turn(ui, rects.end_turn, look, button, words);
                }
                if frame == HudFrame::Compact {
                    out.end_turn = end_turn(ui, rects.end_turn, EndTurnLook::Slim, button, words);
                }
                return;
            }
            if frame == HudFrame::Wide {
                return;
            }
            if frame == HudFrame::Compact {
                ui.painter().rect_filled(rects.end_turn, 3., INK);
                text(ui.painter(), rects.end_turn.center(), Align2::CENTER_CENTER, words.hud.orders_unavailable, 12., MUTED);
                ui.interact(rects.end_turn, "turn_unavailable".into(), Sense::hover()).on_hover_text(words.hud.turn_unavailable);
                return;
            }
            if compact {
                ui.painter().rect_filled(
                    Rect::from_min_size(r.left_top() + vec2(0., -25.), vec2(r.width(), 23.)),
                    3.,
                    INK,
                );
                text(
                    ui.painter(),
                    r.left_top() + vec2(6., -20.),
                    Align2::LEFT_TOP,
                    words.hud.orders_unavailable,
                    11.,
                    MUTED,
                );
                return;
            }
            let turn_rect =
                Rect::from_center_size(pos2(r.right() - 32.0, r.top() - 27.0), vec2(70.0, 70.0));
            let label = Rect::from_min_max(
                pos2(r.left() + 45.0, r.top() - 45.0),
                pos2(r.right() - 57.0, r.top() - 8.0),
            );
            ui.painter().rect_filled(label, 2.0, INK);
            text(
                ui.painter(),
                label.center(),
                Align2::CENTER_CENTER,
                words.hud.orders_unavailable,
                14.0,
                LIGHT,
            );
            ui.painter()
                .circle_filled(turn_rect.center(), turn_rect.width() * 0.5, INK);
            ui.painter().circle_stroke(
                turn_rect.center(),
                turn_rect.width() * 0.5,
                Stroke::new(1., MUTED),
            );
            icon(
                ui.painter(),
                turn_rect.center(),
                18.,
                Icon::Hourglass,
                MUTED,
            );
            ui.interact(
                turn_rect.union(label),
                "turn_unavailable".into(),
                Sense::hover(),
            )
            .on_hover_text(words.hud.turn_unavailable);
        });
}

/// How the end-turn button looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndTurnLook {
    /// A bar over the minimap (the classic frame), small on a narrow window.
    Classic,
    Compact,
    /// A slim bar over the small minimap (the compact frame).
    Slim,
    /// The screen's main button (the wide frame): the primary colour, a large label and
    /// an arrow; a pending decision keeps the button and frames it in the warning colour.
    Primary,
}

/// The end-turn button in `button`; returns true when pressed.
fn end_turn(ui: &mut egui::Ui, button: Rect, look: EndTurnLook, state: &EndTurn, words: &ShellWords) -> bool {
    let (label, tip) = match state {
        EndTurn::Ready => (words.end_turn, words.end_turn_tip),
        EndTurn::Decide { label, tip } => (label.as_str(), tip.as_str()),
        EndTurn::Waiting => (words.others_moving, words.end_turn_tip),
        EndTurn::Unavailable => (words.game_over, words.end_turn_tip),
    };
    let c = Theme::of(ui.ctx()).colors;
    let can_end_turn = !matches!(state, EndTurn::Waiting | EndTurn::Unavailable);
    let decide = matches!(state, EndTurn::Decide { .. });
    let sense = if can_end_turn { Sense::click() } else { Sense::hover() };
    let response = ui.interact(crate::shell::theme::touch_hit(button, 6.), "game_end_turn".into(), sense);
    let hovered = response.hovered() && can_end_turn;
    let ink = if can_end_turn { c.hud_text } else { c.hud_muted };
    let p = ui.painter();
    if look == EndTurnLook::Primary {
        let fill = match (can_end_turn, hovered) {
            (false, _) => c.raised,
            (true, false) => c.primary,
            (true, true) => c.primary.lerp_to_gamma(Color32::WHITE, 0.15),
        };
        let edge = if decide { Stroke::new(2.5, c.warning) } else { Stroke::new(1.0, c.selected_edge) };
        p.rect_filled(button.translate(vec2(0., 3.)), 8., Color32::from_black_alpha(80));
        p.rect_filled(button, 8., fill);
        p.rect_stroke(button, 8., edge, egui::StrokeKind::Inside);
        let size = if decide { 17. } else { 20. };
        let shown = if can_end_turn { format!("{label}  \u{2192}") } else { label.to_owned() };
        p.with_clip_rect(button.shrink(6.)).text(button.center(), Align2::CENTER_CENTER, shown, FontId::proportional(size), ink);
    } else {
        let fill = match (decide, hovered) {
            (true, true) => Color32::from_rgb(132, 104, 50),
            (true, false) => Color32::from_rgb(104, 80, 36),
            (false, true) => Color32::from_rgb(40, 77, 97),
            (false, false) => c.hud_fill,
        };
        p.rect_filled(button, 3.0, fill);
        p.rect_stroke(button, 3.0, Stroke::new(1.5, c.hud_accent), egui::StrokeKind::Inside);
        let size = match look {
            EndTurnLook::Compact => 12.0,
            EndTurnLook::Slim => 14.0,
            _ => 16.0,
        };
        text(p, button.center(), Align2::CENTER_CENTER, label, size, ink);
    }
    let clicked = response.clicked() && can_end_turn;
    response.on_hover_text(super::keymap::with_hint(ui.ctx(), tip, "end-turn"));
    clicked
}

fn map_extent(world: &RenderWorld) -> (f32, f32) {
    let (x, z) = render::scene::layout::tile_to_world(
        world.width.saturating_sub(1),
        world.height.saturating_sub(1),
    );
    ((x + 1.73).max(1.0), (z + 1.5).max(1.0))
}
/// The minimap's tiles as one texture, rebuilt when the terrain or the knowledge changes
/// (or the minimap is resized), not painted as a rectangle per tile every frame (25 000
/// tiles took 3 ms of tessellation per frame on a 200x125 map). Drawn at twice the
/// physical resolution and filtered, so the 1.2-cell blobs keep their soft edges.
#[derive(Clone)]
struct MinimapBake {
    key: (u64, u64, u32, u32, [u32; 2], u32),
    texture: egui::TextureHandle,
}

fn minimap_texture(ctx: &egui::Context, r: Rect, world: &RenderWorld, ext: (f32, f32), cell: egui::Vec2) -> egui::TextureHandle {
    let scale = ctx.pixels_per_point() * 2.0;
    let size = [(r.width() * scale).ceil().max(1.0) as usize, (r.height() * scale).ceil().max(1.0) as usize];
    let key = (
        world.layers.terrain,
        world.layers.knowledge,
        world.width,
        world.height,
        [size[0] as u32, size[1] as u32],
        scale.to_bits(),
    );
    let id = egui::Id::new("minimap_bake");
    if let Some(bake) = ctx.data(|d| d.get_temp::<MinimapBake>(id)) {
        if bake.key == key {
            return bake.texture;
        }
    }
    let image = minimap_image(world, size, scale, r.size(), ext, cell);
    let texture = ctx.load_texture("minimap", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, MinimapBake { key, texture: texture.clone() }));
    texture
}

/// The tiles of the minimap as pixels: a blob of 1.2 cells at every tile, in the order of
/// `world.terrain` (later tiles over earlier ones), `size` pixels for `panel` points.
fn minimap_image(world: &RenderWorld, size: [usize; 2], scale: f32, panel: egui::Vec2, ext: (f32, f32), cell: egui::Vec2) -> egui::ColorImage {
    let r = Rect::from_min_size(pos2(0.0, 0.0), panel);
    let mut image = egui::ColorImage::filled(size, Color32::TRANSPARENT);
    let half = cell * 1.2 * 0.5 * scale;
    for tile in &world.terrain {
        let id = tile.tile.0 as usize;
        let seen = world.visible.get(id).copied().unwrap_or(0) != 0;
        let known = world.explored.get(id).copied().unwrap_or(0) != 0;
        let color = if !known && !seen {
            Color32::from_rgb(31, 44, 54)
        } else {
            let base = match tile.terrain {
                4 => Color32::from_rgb(31, 105, 148),
                6 => Color32::from_rgb(157, 158, 154),
                3 => Color32::from_rgb(181, 162, 92),
                5 => Color32::from_rgb(195, 211, 215),
                2 => Color32::from_rgb(120, 139, 73),
                _ => Color32::from_rgb(108, 144, 73),
            };
            if seen {
                base
            } else {
                base.gamma_multiply(0.52)
            }
        };
        let (x, z) = render::scene::layout::tile_to_world(tile.tile.0 % world.width, tile.tile.0 / world.width);
        let center = vec2(x / ext.0 * r.width(), z / ext.1 * r.height()) * scale;
        let (x0, x1) = ((center.x - half.x).round().max(0.0) as usize, ((center.x + half.x).round().max(0.0) as usize).min(size[0]));
        let (y0, y1) = ((center.y - half.y).round().max(0.0) as usize, ((center.y + half.y).round().max(0.0) as usize).min(size[1]));
        for row in y0..y1 {
            image.pixels[row * size[0] + x0.min(x1)..row * size[0] + x1].fill(color);
        }
    }
    image
}

fn draw_minimap(p: &egui::Painter, r: Rect, world: &RenderWorld, camera: &CameraController) {
    if world.width == 0 || world.height == 0 {
        return;
    }
    let ext = map_extent(world);
    let to_map = |x: f32, z: f32| {
        pos2(
            r.left() + x / ext.0 * r.width(),
            r.top() + z / ext.1 * r.height(),
        )
    };
    let cell = vec2(
        (r.width() / (world.width as f32 * 1.5)).max(1.0),
        (r.height() / world.height as f32).max(1.0),
    );
    let p = p.with_clip_rect(r);
    let baked = minimap_texture(p.ctx(), r, world, ext, cell);
    p.image(baked.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    for city in &world.cities {
        let (x, z) = render::scene::layout::tile_to_world(
            city.tile.0 % world.width,
            city.tile.0 / world.width,
        );
        p.circle_filled(to_map(x, z), 2.6, LIGHT);
    }
    let view_size = vec2(
        camera.distance() * 1.0 / ext.0 * r.width(),
        camera.distance() * 0.7 / ext.1 * r.height(),
    );
    p.rect_stroke(
        Rect::from_center_size(to_map(camera.x, camera.y), view_size),
        0.0,
        Stroke::new(1.0, LIGHT),
        egui::StrokeKind::Inside,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Coin,
    Leaf,
    Apple,
    Flask,
    Note,
    Sun,
    /// Faith and religion: a dove in flight (no one religion's sign).
    Faith,
    Compass,
    Hex,
    City,
    Hourglass,
    Gear,
    Shield,
    Hammer,
    Book,
    /// A list of rows (a module's agenda).
    List,
    /// A scroll with lines (a module's log of events).
    Scroll,
    /// The module's own picture by art name (`art::ScreenArt`: a file or an atlas
    /// region, from the module's art folder first), fitted in the icon's square and
    /// drawn in its own colours; a ring in the tint stands in while it loads.
    Art(&'static str),
}
/// A round button of the map HUD (the dock's too): `active` draws it pressed, `tip` is its hover.
pub fn round_button(
    ui: &mut egui::Ui,
    rect: Rect,
    id: &'static str,
    kind: Icon,
    active: bool,
    tip: &str,
) -> bool {
    let response = ui.interact(crate::shell::theme::touch_hit(rect, 3.), id.into(), Sense::click());
    let p = ui.painter();
    let hud = Theme::of(ui.ctx()).colors;
    let c = rect.center();
    let radius = rect.width() * 0.5;
    p.circle_filled(
        c + vec2(0.0, 2.0),
        radius + 2.0,
        Color32::from_black_alpha(90),
    );
    p.circle_filled(
        c,
        radius,
        if response.hovered() {
            Color32::from_rgb(40, 77, 97)
        } else if active {
            Color32::from_rgb(25, 58, 82)
        } else {
            hud.hud_fill
        },
    );
    p.circle_stroke(c, radius, Stroke::new(1.5, hud.hud_accent));
    p.circle_stroke(
        c,
        radius - 3.0,
        Stroke::new(1.0, Color32::from_rgb(63, 84, 93)),
    );
    icon(p, c, radius * 0.48, kind, hud.hud_text);
    let clicked = response.clicked();
    response.on_hover_text(tip);
    clicked
}

/// The dark edge that keeps a light glyph legible on a light ground.
const HALO: Color32 = Color32::from_rgba_premultiplied(18, 22, 16, 200);

/// A lens from `base` to `tip` (in icon units, × `s` around `c`) whose sides bow out by
/// `half_width` near the base and close to a point at the tip, as a polygon.
fn lens(c: Pos2, s: f32, base: egui::Vec2, tip: egui::Vec2, half_width: f32) -> Vec<Pos2> {
    const STEPS: usize = 8;
    let axis = tip - base;
    let normal = vec2(-axis.y, axis.x).normalized();
    let side = |sign: f32, t: f32| c + (base + axis * t + normal * sign * half_width * (std::f32::consts::PI * t.powf(0.8)).sin()) * s;
    (0..=STEPS)
        .map(|i| side(1.0, i as f32 / STEPS as f32))
        .chain((1..STEPS).map(|i| side(-1.0, 1.0 - i as f32 / STEPS as f32)))
        .collect()
}

pub fn icon(p: &egui::Painter, c: Pos2, s: f32, kind: Icon, color: Color32) {
    let pt = |x: f32, y: f32| c + vec2(x * s, y * s);
    let stroke = Stroke::new(1.6, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([pt(a.0, a.1), pt(b.0, b.1)], stroke);
    };
    match kind {
        Icon::Coin => {
            p.circle_filled(c, s, color);
            p.circle_stroke(c, s * 0.7, Stroke::new(1.3, INK));
            p.line_segment([pt(0.0, -0.43), pt(0.0, 0.43)], Stroke::new(1.5, INK));
        }
        Icon::Apple => {
            // Dark silhouette around each fruit makes the symbols legible on
            // grass, forest and sand without putting a card behind the group.
            for (scale, fill) in [(1.19, Color32::from_rgb(24, 45, 13)), (0.94, color)] {
                let fruit = |x: f32, y: f32| pt(x * scale, y * scale);
                p.circle_filled(fruit(-0.29, -0.18), s * 0.53 * scale, fill);
                p.circle_filled(fruit(0.29, -0.18), s * 0.53 * scale, fill);
                p.add(Shape::convex_polygon(
                    vec![
                        fruit(-0.80, -0.12),
                        fruit(-0.57, 0.61),
                        fruit(0.0, 0.83),
                        fruit(0.57, 0.61),
                        fruit(0.80, -0.12),
                    ],
                    fill,
                    Stroke::NONE,
                ));
            }
            p.line_segment(
                [pt(0.0, -0.49), pt(0.12, -1.0)],
                Stroke::new(1.3, Color32::from_rgb(47, 57, 16)),
            );
            p.add(Shape::convex_polygon(
                vec![
                    pt(0.12, -0.79),
                    pt(0.29, -1.12),
                    pt(0.68, -1.06),
                    pt(0.44, -0.81),
                ],
                Color32::from_rgb(77, 138, 30),
                Stroke::NONE,
            ));
            p.circle_filled(pt(-0.34, -0.23), s * 0.18, Color32::from_rgb(222, 248, 157));
        }
        Icon::Leaf => {
            p.add(Shape::convex_polygon(
                vec![
                    pt(-0.8, 0.65),
                    pt(-0.75, -0.15),
                    pt(0.8, -0.85),
                    pt(0.55, 0.3),
                    pt(0.0, 0.8),
                ],
                color,
                Stroke::NONE,
            ));
            p.line_segment([pt(-0.8, 0.95), pt(0.45, -0.45)], Stroke::new(1.3, INK));
        }
        Icon::Hammer => {
            p.add(Shape::convex_polygon(
                vec![
                    pt(-0.66, 0.80),
                    pt(-0.38, 1.0),
                    pt(0.50, -0.39),
                    pt(0.22, -0.58),
                ],
                color,
                Stroke::new(1.5, Color32::from_rgb(69, 40, 10)),
            ));
            p.add(Shape::convex_polygon(
                vec![
                    pt(-0.40, -0.85),
                    pt(-0.15, -1.06),
                    pt(0.95, -0.29),
                    pt(0.69, 0.05),
                ],
                color,
                Stroke::new(1.5, Color32::from_rgb(69, 40, 10)),
            ));
        }
        Icon::Flask => {
            p.add(Shape::convex_polygon(
                vec![
                    pt(-0.27, -1.0),
                    pt(0.27, -1.0),
                    pt(0.27, -0.25),
                    pt(0.85, 0.85),
                    pt(-0.85, 0.85),
                    pt(-0.27, -0.25),
                ],
                color.gamma_multiply(0.7),
                stroke,
            ));
            line((-0.42, -1.0), (0.42, -1.0));
            line((-0.54, 0.4), (0.54, 0.4));
        }
        Icon::Note => {
            line((-0.35, 0.5), (-0.35, -0.75));
            line((-0.35, -0.75), (0.65, -1.0));
            line((0.65, -1.0), (0.65, 0.25));
            p.circle_filled(pt(-0.6, 0.6), s * 0.32, color);
            p.circle_filled(pt(0.4, 0.35), s * 0.32, color);
        }
        Icon::Faith => {
            // A dove flying left: body, head with a beak, a raised wing, a fanned tail; a dark
            // edge under the whole bird keeps the light glyph legible on desert and snow.
            let halo = HALO.gamma_multiply(color.a() as f32 / 255.0);
            let parts = [
                lens(c, s, vec2(-0.78, 0.26), vec2(0.6, 0.46), 0.3),
                vec![pt(0.35, 0.3), pt(1.0, 0.08), pt(1.0, 0.66), pt(0.42, 0.58)],
                lens(c, s, vec2(-0.25, 0.28), vec2(0.62, -0.7), 0.34),
                vec![pt(-0.78, -0.02), pt(-1.02, 0.16), pt(-0.8, 0.22)],
            ];
            let head = (pt(-0.6, 0.12), s * 0.25);
            p.circle_filled(head.0, head.1 + 1.2, halo);
            for part in &parts {
                p.add(Shape::convex_polygon(part.clone(), halo, Stroke::new(1.4, halo)));
            }
            for part in parts {
                p.add(Shape::convex_polygon(part, color, Stroke::NONE));
            }
            p.circle_filled(head.0, head.1, color);
        }
        Icon::Sun => {
            p.circle_stroke(c, s * 0.45, stroke);
            for n in 0..8 {
                let a = n as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * s * 0.65, c + d * s], stroke);
            }
        }
        Icon::Gear => {
            // A cog: a thick ring with eight square teeth, set between the sun's rays.
            p.circle_stroke(c, s * 0.53, Stroke::new(s * 0.3, color));
            for n in 0..8 {
                let a = (n as f32 + 0.5) * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                let side = vec2(-d.y, d.x);
                p.add(Shape::convex_polygon(
                    vec![c + (d * 0.6 - side * 0.2) * s, c + (d * 1.0 - side * 0.15) * s, c + (d * 1.0 + side * 0.15) * s, c + (d * 0.6 + side * 0.2) * s],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        Icon::Compass => {
            for n in 0..4 {
                let a = n as f32 * std::f32::consts::TAU / 4.0;
                let d = vec2(a.cos(), a.sin());
                let normal = vec2(-d.y, d.x);
                p.add(Shape::convex_polygon(
                    vec![c + d * s, c + normal * s * 0.23, c - normal * s * 0.23],
                    if n % 2 == 0 { LIGHT } else { GOLD },
                    Stroke::NONE,
                ));
            }
            p.circle_filled(c, s * 0.18, INK);
        }
        Icon::Hex => {
            let points = (0..6)
                .map(|n| {
                    let a = (n as f32 * 60.0 + 30.0).to_radians();
                    pt(a.cos(), a.sin())
                })
                .collect();
            p.add(Shape::closed_line(points, stroke));
            line((0.0, -1.0), (0.0, 1.0));
            line((-0.86, -0.5), (0.86, 0.5));
            line((-0.86, 0.5), (0.86, -0.5));
        }
        Icon::City => {
            for (x, height) in [(-0.72, 0.6), (0.0, 1.0), (0.72, 0.5)] {
                let r = Rect::from_min_max(pt(x - 0.22, -height), pt(x + 0.22, 0.9));
                p.rect_filled(r, 0.0, color);
                p.rect_filled(
                    Rect::from_center_size(pt(x, 0.55), vec2(s * 0.14, s * 0.4)),
                    0.0,
                    INK,
                );
            }
            line((-1.0, 0.95), (1.0, 0.95));
        }
        Icon::Hourglass => {
            line((-0.7, -1.0), (0.7, -1.0));
            line((-0.7, 1.0), (0.7, 1.0));
            p.add(Shape::closed_line(
                vec![
                    pt(-0.55, -0.82),
                    pt(0.55, -0.82),
                    pt(0.4, -0.35),
                    pt(-0.4, 0.35),
                    pt(-0.55, 0.82),
                    pt(0.55, 0.82),
                    pt(0.4, 0.35),
                    pt(-0.4, -0.35),
                ],
                stroke,
            ));
            p.add(Shape::convex_polygon(
                vec![pt(-0.38, 0.7), pt(0.0, 0.2), pt(0.38, 0.7)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Art(name) => {
            let square = Rect::from_center_size(c, vec2(2.0 * s, 2.0 * s));
            let art = super::art::ScreenArt::shared(p.ctx());
            let painted = art.lock().is_ok_and(|mut art| art.paint_fit(p.ctx(), p, square, name, Color32::WHITE));
            if !painted {
                p.circle_stroke(c, s * 0.8, stroke);
            }
        }
        Icon::Book => {
            for side in [-1.0_f32, 1.0] {
                let page = Rect::from_min_max(pt(0.0_f32.min(side * 0.9), -0.75), pt(0.0_f32.max(side * 0.9), 0.8));
                p.rect_stroke(page, 1.0, stroke, egui::StrokeKind::Inside);
            }
            line((-0.6, -0.35), (-0.2, -0.35));
            line((0.2, -0.35), (0.6, -0.35));
            line((-0.6, 0.05), (-0.2, 0.05));
            line((0.2, 0.05), (0.6, 0.05));
        }
        Icon::List => {
            for y in [-0.6_f32, 0.0, 0.6] {
                p.circle_filled(pt(-0.75, y), s * 0.12, color);
                line((-0.4, y), (0.9, y));
            }
        }
        Icon::Scroll => {
            p.rect_stroke(Rect::from_min_max(pt(-0.7, -0.9), pt(0.7, 0.9)), 2.0, stroke, egui::StrokeKind::Inside);
            line((-0.35, -0.45), (0.35, -0.45));
            line((-0.35, 0.0), (0.35, 0.0));
            line((-0.35, 0.45), (0.1, 0.45));
        }
        Icon::Shield => {
            p.add(Shape::convex_polygon(
                vec![
                    pt(-0.8, -0.85),
                    pt(0.8, -0.85),
                    pt(0.68, 0.3),
                    pt(0.0, 1.0),
                    pt(-0.68, 0.3),
                ],
                color,
                Stroke::NONE,
            ));
        }
    }
}

// A test-only file (`*_tests.rs`: the i18n scan reads its icon names as test data, not UI text).
#[cfg(test)]
mod icon_pixels_tests;

#[cfg(test)]
mod visibility_badge_tests {
    use super::*;
    use kernel::ids::TileId;
    use render::world::state::UnitMarker;

    fn visual_lines(tip: &str) -> usize {
        tip.lines().count()
    }

    #[test]
    fn a_tip_shows_three_visual_lines_at_most() {
        assert_eq!(short_tip("a\nb"), "a\nb");
        assert_eq!(short_tip("a\n\n  b  \n"), "a\nb", "empty lines are dropped, the rest trimmed");
        assert_eq!(short_tip("a\nb\nc\nd\ne"), "a\nb\nc…");
        assert_eq!(short_tip(""), "");
        // One line of text longer than three cards' lines: wrapped by width, cut with «…».
        let long = "The road lets food reach Steppeton and Riverbank if the cities keep a surplus and the trail is not taken by a neighbour caravan, and a war does not cut the way";
        assert!(long.chars().count() > 3 * TIP_LINE_CHARS);
        let tip = short_tip(long);
        assert_eq!(visual_lines(&tip), 3, "{tip}");
        assert!(tip.ends_with('…') && tip.lines().all(|l| l.chars().count() <= TIP_LINE_CHARS), "{tip}");
        // Text that fits in three visual lines is whole and keeps its words.
        let fits = "The road lets food reach Steppeton and Riverbank if a surplus remains";
        let tip = short_tip(fits);
        assert_eq!(tip.split_whitespace().collect::<Vec<_>>().join(" "), fits);
        assert!(visual_lines(&tip) == 2 && !tip.contains('…'), "{tip}");
    }

    #[test]
    fn a_tip_of_many_short_lines_and_of_one_wide_word_stays_within_three_lines() {
        for tip in ["one\ntwo\nthree\nfour", &"w".repeat(500), &format!("{}\n{}", "words ".repeat(30), "yet")] {
            let shown = short_tip(tip);
            assert!(visual_lines(&shown) <= TIP_LINES, "{shown}");
            assert!(shown.lines().all(|l| l.chars().count() <= TIP_LINE_CHARS), "{shown}");
            assert!(shown.ends_with('…'), "{shown}");
        }
        let words = "words ".repeat(7).trim().to_owned();
        assert_eq!(visual_lines(&short_tip(&words)), 1, "41 characters are one line");
        assert_eq!(visual_lines(&short_tip(&"words ".repeat(8))), 2, "47 characters wrap into two");
    }

    #[test]
    fn economy_stays_clipped_and_utility_buttons_work_on_narrow_hud() {
        for width in [1440., 960., 480.] {
            let ctx = egui::Context::default();
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(width, 640.));
            let mut state = VisualHudState::default();
            let value = |text: &str| super::super::module::TopBarValue {
                icon: Icon::Coin, tint: GOLD, text: text.into(), compact: text.into(), tip: None, click: None, ready: false,
            };
            state.feed = Some(HudFeed {
                turn: 1234, nation: "A very long nation name".into(),
                top_bar: vec![value("123456 (+9999) Gold"), value("Golden age 12345/500"), value("999 Iron · 999 Horses · 999 Aluminum")],
                end_turn: Some(EndTurn::Ready),
                ..Default::default()
            });
            let frame = |state: &mut VisualHudState, events| {
                let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| { top_bar(ui.ctx(), HudFrame::Classic, &HudFrame::Classic.rects(screen), state, crate::shell::words::current()); });
                output.textures_delta.clear(); output
            };
            frame(&mut state, vec![]);
            let output = frame(&mut state, vec![]);
            let turn = output.shapes.iter().find_map(|shape| match &shape.shape {
                Shape::Text(text) if text.galley.text().contains("Turn 1234") => Some(text.pos.x), _ => None,
            }).unwrap();
            for shape in &output.shapes {
                if let Shape::Text(text) = &shape.shape {
                    if text.galley.text().contains("123456") || text.galley.text().contains("999 Iron") {
                        assert!(shape.clip_rect.right() < turn, "economy never paints over turn or buttons at {width}");
                    }
                }
            }
            let at = pos2(width - 29., 20.);
            for pressed in [true, false] {
                frame(&mut state, vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos:at, button:egui::PointerButton::Primary, pressed, modifiers:egui::Modifiers::NONE }]);
            }
            assert!(state.show_settings, "settings remains reachable at {width}");
        }
    }

    /// Every language keeps the top bar's slots apart: the economy is clipped before the turn
    /// text, whatever the word for "turn" is (German, Chinese, the pseudo-locale...).
    #[test]
    fn the_top_bar_keeps_its_slots_apart_in_every_language() {
        for tag in ["en", "ru", "de", "fr", "es", "ja", "zh-Hans", "en-XA"] {
            i18n::with_language(tag, || {
                for width in [1440., 960., 480.] {
                    let ctx = egui::Context::default();
                    let screen = Rect::from_min_size(Pos2::ZERO, vec2(width, 640.));
                    let mut state = VisualHudState::default();
                    let value = |text: &str| super::super::module::TopBarValue { icon: Icon::Coin, tint: GOLD, text: text.into(), compact: text.into(), tip: None, click: None, ready: false };
                    state.feed = Some(HudFeed {
                        turn: 1234,
                        nation: "A very long nation name".into(),
                        top_bar: vec![value("123456 (+9999) Gold"), value("Golden age 12345/500")],
                        end_turn: Some(EndTurn::Ready),
                        ..Default::default()
                    });
                    let frame = |state: &mut VisualHudState| {
                        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                            top_bar(ui.ctx(), HudFrame::Classic, &HudFrame::Classic.rects(screen), state, crate::shell::words::current());
                        });
                        output.textures_delta.clear();
                        output
                    };
                    frame(&mut state);
                    let output = frame(&mut state);
                    let turn_text = format!("{} 1234", crate::shell::words::current().turn);
                    let turn = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            Shape::Text(text) if text.galley.text().contains(&turn_text) => Some(text.pos.x),
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("{tag}: no {turn_text:?} at {width}"));
                    for shape in &output.shapes {
                        if let Shape::Text(text) = &shape.shape {
                            if text.galley.text().contains("123456") {
                                assert!(shape.clip_rect.right() <= turn + 0.5, "{tag}: the economy paints over the turn at {width}: clip right {} against turn at {turn}", shape.clip_rect.right());
                            }
                        }
                    }
                }
            });
        }
    }

    #[test]
    fn a_module_status_cell_replaces_the_turn_and_reports_its_click() {
        use super::super::module::StatusLine;
        for (width, wide) in [(1440., true), (900., false)] {
            let ctx = egui::Context::default();
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(width, 640.));
            let mut state = VisualHudState::default();
            let status = StatusLine { text: "Realm of Aurelia, year 12".into(), short: "Year 12".into(), tip: "Open the realm".into(), open: false };
            state.feed = Some(HudFeed { turn: 7, nation: "Rome".into(), status: Some(status), ..Default::default() });
            let clicked = std::cell::Cell::new(false);
            let frame = |state: &mut VisualHudState, events| {
                let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                    clicked.set(clicked.get() | top_bar(ui.ctx(), HudFrame::Classic, &HudFrame::Classic.rects(screen), state, crate::shell::words::current()).status_clicked);
                });
                output.textures_delta.clear();
                output
            };
            frame(&mut state, vec![]);
            let output = frame(&mut state, vec![]);
            let texts: Vec<String> = output.shapes.iter().filter_map(|s| match &s.shape { Shape::Text(t) => Some(t.galley.text().to_owned()), _ => None }).collect();
            assert!(!texts.iter().any(|t| t.contains("Turn") || t.contains("Rome")), "the shell's turn text is gone at {width}: {texts:?}");
            let wanted = if wide { "Realm of Aurelia, year 12" } else { "Year 12" };
            assert!(texts.iter().any(|t| t == wanted), "{wanted:?} in {texts:?}");
            let cell = state.top_bar.placed().iter().find(|p| p.name == "top_turn").expect("the status cell holds the turn slot").rect;
            assert!(cell.width() >= 60., "{cell:?}");
            let at = cell.center();
            assert!(!clicked.get());
            for pressed in [true, false] {
                frame(&mut state, vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }]);
            }
            assert!(clicked.get(), "a click on the status cell reaches the module at {width}");
        }
    }

    #[test]
    fn a_top_bar_value_with_a_target_is_a_button_and_a_plain_one_is_not() {
        use super::super::module::TopBarClick;
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1440., 640.));
        let mut state = VisualHudState::default();
        let value = |text: &str, click: Option<&'static str>, ready| super::super::module::TopBarValue {
            icon: Icon::Flask, tint: GOLD, text: text.into(), compact: text.into(), tip: None,
            click: click.map(|id| TopBarClick { id, hint: "Click to open: Research".into() }), ready,
        };
        state.feed = Some(HudFeed {
            turn: 3,
            top_bar: vec![value("12 Science", Some("research"), false), value("Plain", None, false), value("24/45 Culture", Some("policies"), true)],
            ..Default::default()
        });
        let clicked = std::cell::Cell::new(None);
        let frame = |state: &mut VisualHudState, events| {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
                if let Some(id) = top_bar(ui.ctx(), HudFrame::Classic, &HudFrame::Classic.rects(screen), state, crate::shell::words::current()).value_clicked {
                    clicked.set(Some(id));
                }
            });
            output.textures_delta.clear();
            output
        };
        frame(&mut state, vec![]);
        frame(&mut state, vec![]);
        assert_eq!(state.value_cells.len(), 3, "one cell per value");
        let press = |state: &mut VisualHudState, at: Pos2| {
            for pressed in [true, false] {
                frame(state, vec![egui::Event::PointerMoved(at), egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }]);
            }
        };
        let cells = state.value_cells.clone();
        press(&mut state, cells[1].center());
        assert_eq!(clicked.get(), None, "a value without a target ignores the click");
        press(&mut state, cells[0].center());
        assert_eq!(clicked.take(), Some("research"));
        press(&mut state, cells[2].center());
        assert_eq!(clicked.take(), Some("policies"));
    }

    #[test]
    fn badges_never_show_remembered_missing_or_invalid_units() {
        let world = RenderWorld {
            width: 3,
            height: 1,
            visible: vec![1, 0],
            explored: vec![1, 1, 1],
            units: (0..4)
                .map(|tile| UnitMarker {
                    id: 0, embarked: false, fortified: false, tile: TileId(tile),
                    owner: 2,
                    kind: 4,
                    hp: 100,
                })
                .collect(),
            ..Default::default()
        };
        assert_eq!(
            visible_unit_badges(&world, &[]),
            vec![UnitBadge {
                tile: 0,
                owner: 2,
                kind: Some(4),
                count: 1,
                at: None,
            }]
        );
    }
    #[test]
    fn badges_count_units_not_decorative_formation_members() {
        let unit = UnitMarker {
            id: 0, embarked: false, fortified: false, tile: TileId(0),
            owner: 1,
            kind: 4,
            hp: 100,
        };
        let mut world = RenderWorld {
            width: 1,
            height: 1,
            visible: vec![1],
            units: vec![unit],
            ..Default::default()
        };
        assert_eq!(visible_unit_badges(&world, &[])[0].count, 1);
        let moving = visible_unit_badges(&world, &[(0, [1.0, 0.5, 2.0])]);
        assert_eq!(moving.len(), 1);
        assert_eq!(moving[0].at, Some([1.0, 0.5, 2.0]), "a moving unit's badge follows it");
        world.units.push(UnitMarker { kind: 3, ..unit });
        assert_eq!(
            visible_unit_badges(&world, &[]),
            vec![UnitBadge {
                tile: 0,
                owner: 1,
                kind: None,
                count: 2,
                at: None,
            }]
        );
    }
}

#[cfg(test)]
mod minimap_tests {
    use super::*;
    use kernel::ids::TileId;

    fn minimap_world() -> RenderWorld {
        let cells = |terrain: u16| (0..12).map(|i| render::world::state::TerrainCell { tile: TileId(i), terrain, owner: None, resource: None }).collect::<Vec<_>>();
        RenderWorld {
            width: 4,
            height: 3,
            terrain: cells(4),
            visible: vec![1; 12],
            explored: vec![1; 12],
            ..Default::default()
        }
    }

    #[test]
    fn the_minimap_image_colours_tiles_by_what_the_observer_knows() {
        let mut world = minimap_world();
        world.visible[0] = 0; // remembered: dimmed
        world.explored[1] = 0;
        world.visible[1] = 0; // unknown: the fog colour
        let (ext, panel) = (map_extent(&world), vec2(40.0, 30.0));
        let cell = vec2(10.0, 10.0);
        let image = minimap_image(&world, [80, 60], 2.0, panel, ext, cell);
        let at = |tile: u32| {
            let (x, z) = render::scene::layout::tile_to_world(tile % 4, tile / 4);
            let (px, py) = ((x / ext.0 * panel.x * 2.0) as usize, (z / ext.1 * panel.y * 2.0) as usize);
            image.pixels[py * 80 + px]
        };
        assert_eq!(at(1), Color32::from_rgb(31, 44, 54));
        assert_eq!(at(2), Color32::from_rgb(31, 105, 148), "a visible sea tile");
        assert_eq!(at(0), Color32::from_rgb(31, 105, 148).gamma_multiply(0.52), "a remembered one is dimmed");
    }

    #[test]
    fn the_minimap_texture_is_baked_again_only_when_the_knowledge_changes() {
        let ctx = egui::Context::default();
        let mut world = minimap_world();
        let (ext, panel, cell) = (map_extent(&world), Rect::from_min_size(pos2(0.0, 0.0), vec2(40.0, 30.0)), vec2(10.0, 10.0));
        let first = minimap_texture(&ctx, panel, &world, ext, cell);
        assert_eq!(minimap_texture(&ctx, panel, &world, ext, cell).id(), first.id(), "the frame after reuses it");
        world.layers.knowledge += 1;
        assert_ne!(minimap_texture(&ctx, panel, &world, ext, cell).id(), first.id(), "a new view of the map bakes it again");
    }
}
