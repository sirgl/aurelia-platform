//! City banner over a city on the map (classic style): population in a disc,
//! the name, the combat strength, and for own cities a growth bar and the
//! production item with its progress. A click opens the city.
use crate::shell::module::{CityBanner, ShellWords};
use render::world::state::CityMarker;
use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Stroke};

pub const WIDTH: f32 = 210.;
/// Height of a foreign banner; own banners add the bars below.
pub const HEIGHT: f32 = 34.;
pub const OWN_HEIGHT: f32 = 52.;

const INK: Color32 = Color32::from_rgba_premultiplied(10, 20, 29, 240);
const GOLD: Color32 = Color32::from_rgb(187, 156, 92);
const LIGHT: Color32 = Color32::from_rgb(235, 231, 213);
const MUTED: Color32 = Color32::from_rgb(166, 183, 190);
const FOOD: Color32 = Color32::from_rgb(137, 188, 83);
const PRODUCTION: Color32 = Color32::from_rgb(214, 150, 70);

/// One line of a visitor's name under the banner.
const VISITOR_LINE: f32 = 13.;

/// How the labels of a frame are sized and placed: the classic module's banners keep one size
/// over their city (`CLASSIC`); a module that asks for it (`ModuleUi::labels_follow_zoom`)
/// has them shrink with the camera and stand above the city's model (`zoomed`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// Every length of the banner and the unit badges (the sizes of the text too) is
    /// multiplied by it.
    pub k: f32,
    /// The banner stands above the anchor by its own height instead of starting at it.
    pub above: bool,
    /// The banners are drawn at all: a hex narrower than the smallest readable label has
    /// none (the city stands out by its model and its icon).
    pub banners: bool,
}

impl Fit {
    pub const CLASSIC: Fit = Fit { k: 1., above: false, banners: true };

    /// The fit of a map whose hexes are `tile_width` pixels wide.
    pub fn zoomed(tile_width: f32) -> Fit {
        Fit { k: scale_for(tile_width), above: true, banners: tile_width >= SMALLEST_TILE }
    }
}

/// The pixels of a hex's width at which the labels have their full size, and the smallest
/// share of it they shrink to: the city's name is 15 points, so 0.6 of it is 9 points, the
/// least that reads.
const FULL_TILE: f32 = 110.;
const SMALLEST: f32 = 0.6;
/// A banner is drawn only where it is at most this many hexes wide: the banner is
/// `WIDTH · k` wide, and `k` stops shrinking at `SMALLEST` (a name under 9 points does not
/// read), so a far camera would make it a wall over the neighbours and the model.
const MAX_BANNER_HEXES: f32 = 2.4;
/// The narrowest hex that still has banners (52.5 points: the 9-point name's banner of
/// 126 points is 2.4 hexes of it).
pub const SMALLEST_TILE: f32 = WIDTH * SMALLEST / MAX_BANNER_HEXES;
/// The city name's size on the screen never goes below this many points.
pub const MIN_NAME_POINTS: f32 = 9.;

/// The labels' scale at a hex `tile_width` pixels wide: the width over 110, within 0.6..1.
pub fn scale_for(tile_width: f32) -> f32 {
    (tile_width / FULL_TILE).clamp(SMALLEST, 1.)
}

pub fn height(banner: Option<&CityBanner>) -> f32 {
    height_at(banner, 1.)
}

/// The banner's height at scale `k`.
pub fn height_at(banner: Option<&CityBanner>, k: f32) -> f32 {
    let base = match banner {
        Some(b) if b.own => OWN_HEIGHT,
        _ => HEIGHT,
    };
    (base + banner.map_or(0., |b| b.visitors.len() as f32 * VISITOR_LINE)) * k
}

/// The banner's rectangle for the city whose anchor (the point over its model) is at
/// `anchor`.
pub fn rect_at(anchor: Pos2, banner: Option<&CityBanner>, fit: Fit) -> Rect {
    let size = vec2(WIDTH * fit.k, height_at(banner, fit.k));
    let lift = if fit.above { size.y } else { 0. };
    Rect::from_min_size(pos2(anchor.x - size.x * 0.5, anchor.y - 20. * fit.k - lift), size)
}

/// Paints the banner into `r`; `tint` is the owner's colour.
pub fn paint(p: &egui::Painter, r: Rect, city: &CityMarker, banner: Option<&CityBanner>, tint: Color32, selected: bool, words: &ShellWords) {
    paint_at(p, r, city, banner, tint, selected, words, 1.);
}

/// `paint` at scale `k` (`r` is the banner's rectangle at that scale).
pub fn paint_at(p: &egui::Painter, r: Rect, city: &CityMarker, banner: Option<&CityBanner>, tint: Color32, selected: bool, words: &ShellWords, k: f32) {
    p.rect_filled(r.translate(vec2(0., 3. * k)), 3. * k, Color32::from_black_alpha(60));
    p.rect_filled(r, 3. * k, INK);
    let stroke = if selected { Stroke::new(2., GOLD) } else { Stroke::new(1., GOLD.gamma_multiply(0.8)) };
    p.rect_stroke(r, 3. * k, stroke, egui::StrokeKind::Inside);
    let top = Rect::from_min_size(r.min, vec2(r.width(), HEIGHT * k));
    let disc = pos2(top.left() + 17. * k, top.center().y);
    p.circle_filled(disc, 14. * k, tint);
    p.circle_stroke(disc, 14. * k, Stroke::new(1.2, GOLD));
    p.text(disc, Align2::CENTER_CENTER, city.population_label(), FontId::proportional(14. * k), LIGHT);
    let name_right = top.right() - if banner.is_some() { 56. * k } else { 8. * k };
    name(p, pos2(top.left() + 38. * k, top.center().y), &city.name, name_right - top.left() - 38. * k, k);
    if let Some(banner) = banner {
        let value = p.text(pos2(top.right() - 8. * k, top.center().y), Align2::RIGHT_CENTER, banner.strength, FontId::proportional(13. * k), LIGHT);
        shield(p, pos2(value.left() - 9. * k, top.center().y), 6. * k, MUTED);
        visitors(p, r, banner, k);
        if banner.own {
            bars(p, Rect::from_min_max(pos2(r.left() + 8. * k, top.bottom() - 2. * k), pos2(r.right() - 8. * k, r.top() + OWN_HEIGHT * k - 4. * k)), banner, words, k);
        }
        if banner.hp < banner.max_hp {
            health_bar(p, top, banner, k);
        }
    }
}

/// The city name at 15 points, smaller when the long name would reach the strength (down
/// to 10, and never under `MIN_NAME_POINTS` on the screen); a name that still does not fit
/// ends in «…» (the whole name is on the hover).
fn name(p: &egui::Painter, at: egui::Pos2, name: &str, width: f32, k: f32) {
    let mut size = 15.;
    let fits = |text: &str, size: f32| p.layout_no_wrap(text.to_owned(), FontId::proportional(size * k), LIGHT).size().x <= width;
    while size > 10. && (size - 1.) * k >= MIN_NAME_POINTS && !fits(name, size) {
        size -= 1.;
    }
    let shown = if fits(name, size) { name.to_owned() } else { cut(name, |text| fits(text, size)) };
    p.text(at, Align2::LEFT_CENTER, shown, FontId::proportional(size * k), LIGHT);
}

/// `text` cut to the longest start that `fits` with «…».
fn cut(text: &str, fits: impl Fn(&str) -> bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(&format!("{}…", chars[..mid].iter().collect::<String>().trim_end())) { lo = mid } else { hi = mid - 1 }
    }
    format!("{}…", chars[..lo].iter().collect::<String>().trim_end())
}

/// The viewer's agents in the city, one name per line under the banner's bars.
fn visitors(p: &egui::Painter, r: Rect, banner: &CityBanner, k: f32) {
    let base = if banner.own { OWN_HEIGHT } else { HEIGHT };
    for (i, who) in banner.visitors.iter().enumerate() {
        let y = r.top() + base * k + i as f32 * VISITOR_LINE * k;
        p.text(pos2(r.left() + 10. * k, y), Align2::LEFT_TOP, who, FontId::proportional(10. * k), GOLD);
    }
}

/// The city's hit points: a thin bar along the top edge (green to red).
fn health_bar(p: &egui::Painter, top: Rect, banner: &CityBanner, k: f32) {
    let share = f32::from(banner.hp) / f32::from(banner.max_hp.max(1));
    let back = Rect::from_min_size(top.min + vec2(4. * k, 2. * k), vec2(top.width() - 8. * k, 3. * k));
    p.rect_filled(back, 1., Color32::from_black_alpha(160));
    let color = if share > 0.5 { FOOD } else if share > 0.25 { PRODUCTION } else { Color32::from_rgb(200, 70, 60) };
    p.rect_filled(Rect::from_min_size(back.min, vec2(back.width() * share, back.height())), 1., color);
}

/// Where the strike bow sits: a square left of the banner (outside it).
pub fn strike_rect(banner: Rect) -> Rect {
    strike_rect_at(banner, 1.)
}

/// `strike_rect` at scale `k` (`banner` is the banner's rectangle at that scale).
pub fn strike_rect_at(banner: Rect, k: f32) -> Rect {
    Rect::from_min_size(pos2(banner.left() - 30. * k, banner.top() + 3. * k), vec2(26. * k, 26. * k))
}

/// The city strike button: a bow on a gold disc (drawn at the scale of its square `r`).
pub fn bow(p: &egui::Painter, r: Rect) {
    let k = r.width() / 26.;
    let c = r.center();
    p.circle_filled(c, 13. * k, INK);
    p.circle_stroke(c, 13. * k, Stroke::new(1.5, GOLD));
    let arc: Vec<egui::Pos2> = (0..=8)
        .map(|i| {
            let a = -1.2 + 2.4 * i as f32 / 8.;
            c + vec2(-3. + 7. * a.cos(), 7. * a.sin()) * k
        })
        .collect();
    p.add(egui::Shape::line(arc, Stroke::new(1.8, LIGHT)));
    p.line_segment([c + vec2(1.4, -6.5) * k, c + vec2(1.4, 6.5) * k], Stroke::new(1., LIGHT));
    p.line_segment([c + vec2(-6., 0.) * k, c + vec2(6., 0.) * k], Stroke::new(1.4, GOLD));
}

fn shield(p: &egui::Painter, c: egui::Pos2, s: f32, color: Color32) {
    let points = vec![
        c + vec2(-s, -s),
        c + vec2(s, -s),
        c + vec2(s * 0.85, s * 0.35),
        c + vec2(0., s * 1.2),
        c + vec2(-s * 0.85, s * 0.35),
    ];
    p.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

fn bars(p: &egui::Painter, r: Rect, banner: &CityBanner, words: &ShellWords, k: f32) {
    let half = (r.width() - 8. * k) / 2.;
    let growth = Rect::from_min_size(r.min, vec2(half, r.height()));
    let production = Rect::from_min_size(r.min + vec2(half + 8. * k, 0.), vec2(half, r.height()));
    if let Some((stored, needed, turns)) = banner.growth {
        let label = match turns {
            Some(t) => format!("{} {t}", words.grows),
            None => words.stagnant.into(),
        };
        bar(p, growth, stored, needed, FOOD, &label, k);
    }
    match &banner.production {
        Some((item, done, cost, turns)) => {
            let label = match turns {
                Some(t) => format!("{} {t}", short(item)),
                None => short(item),
            };
            bar(p, production, *done, *cost, PRODUCTION, &label, k);
        }
        None => {
            p.text(production.left_center(), Align2::LEFT_CENTER, words.choose_production, FontId::proportional(10. * k), GOLD);
        }
    }
}

fn bar(p: &egui::Painter, r: Rect, done: u32, total: u32, color: Color32, label: &str, k: f32) {
    let track = Rect::from_min_size(pos2(r.left(), r.bottom() - 4. * k), vec2(r.width(), 3. * k));
    p.rect_filled(track, 1., Color32::from_rgb(48, 67, 77));
    let share = (done as f32 / total.max(1) as f32).clamp(0., 1.);
    p.rect_filled(Rect::from_min_size(track.min, vec2(track.width() * share, 3. * k)), 1., color);
    p.text(pos2(r.left(), r.top() + 1. * k), Align2::LEFT_TOP, label, FontId::proportional(10. * k), MUTED);
}

/// Item names fit half a banner.
fn short(name: &str) -> String {
    if name.chars().count() <= 13 {
        return name.to_owned();
    }
    let cut: String = name.chars().take(12).collect();
    format!("{cut}…")
}

/// Hover text of a banner: the numbers the bars show.
pub fn tooltip(city: &CityMarker, banner: Option<&CityBanner>, words: &ShellWords) -> String {
    let w = &words.hud;
    let mut lines = vec![i18n::tf!("shell.hud.population", name = &city.name, population = city.population_label())];
    if let Some(b) = banner {
        lines.push(i18n::tf!("shell.hud.combat-strength", strength = b.strength));
        if b.hp < b.max_hp {
            lines.push(i18n::tf!("shell.hud.hit-points", current = b.hp, max = b.max_hp));
        }
        if b.can_strike {
            lines.push(w.strike_ready.into());
        }
        if let Some((stored, needed, turns)) = b.growth {
            let when = turns.map_or_else(|| w.not_growing.to_owned(), |t| i18n::tf!("shell.hud.grows-in", n = t));
            lines.push(format!("{}, {when}", i18n::tf!("shell.hud.food", stored = stored, needed = needed)));
        }
        if let Some((item, done, cost, turns)) = &b.production {
            let what = i18n::tf!("shell.hud.producing", name = item, progress = done, cost = cost);
            lines.push(match turns {
                Some(t) => format!("{what}, {}", i18n::tf!("shell.hud.turns", n = *t)),
                None => what,
            });
        }
        lines.extend(b.visitors.iter().cloned());
        if b.own {
            lines.push(w.manage_city.into());
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own() -> CityBanner {
        CityBanner { tile: 0, own: true, strength: 0, hp: 0, max_hp: 0, can_strike: false, growth: None, production: None, tip: None, visitors: vec!["Scout".into()] }
    }

    #[test]
    fn the_scale_follows_the_hex_width_within_its_bounds() {
        assert_eq!(scale_for(300.), 1.);
        assert_eq!(scale_for(110.), 1.);
        assert!((scale_for(77.) - 0.7).abs() < 1e-6);
        assert!((scale_for(66.) - 0.6).abs() < 1e-6, "0.6 of 110 is 66");
        assert_eq!(scale_for(45.), 0.6, "narrower hexes keep the smallest label");
        assert_eq!(scale_for(0.), 0.6);
    }

    #[test]
    fn the_smallest_label_still_reads_and_a_narrower_hex_has_no_banner() {
        assert!(15. * scale_for(0.) >= MIN_NAME_POINTS - 1e-4, "the name at the smallest scale is {} points", 15. * scale_for(0.));
        for width in [52.6, 62., 110., 300.] {
            assert!(Fit::zoomed(width).banners, "a hex {width} px wide has banners");
        }
        for width in [52.4, 45., 27., 0.] {
            let fit = Fit::zoomed(width);
            assert!(!fit.banners, "a hex {width} px wide is too narrow for a readable banner");
            assert_eq!(fit.k, scale_for(width));
        }
        assert!(Fit::CLASSIC.banners && Fit::CLASSIC.k == 1. && !Fit::CLASSIC.above, "Nation's labels do not change");
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_before_it_goes_under_nine_points() {
        let width = |text: &str| text.chars().count() as f32 * 7.;
        assert_eq!(cut("Steppeton", |t| width(t) <= 56.), "Steppet…", "seven letters and the ellipsis are 56");
        let short = cut("New Steppeton Village", |t| width(t) <= 100.);
        assert!(short.ends_with('…') && width(&short) <= 100. && short.chars().count() > 5, "{short}");
        assert_eq!(cut("Steppeton", |_| false), "…");
    }

    #[test]
    fn the_classic_banner_starts_twenty_pixels_above_its_anchor_at_full_size() {
        for banner in [None, Some(&own())] {
            let at = rect_at(pos2(400., 300.), banner, Fit::CLASSIC);
            assert_eq!(at, Rect::from_min_size(pos2(400. - WIDTH * 0.5, 300. - 20.), vec2(WIDTH, height(banner))));
        }
    }

    #[test]
    fn a_zoomed_banner_stands_above_the_anchor_by_its_own_height() {
        let banner = own();
        let fit = Fit::zoomed(77.);
        let at = rect_at(pos2(400., 300.), Some(&banner), fit);
        assert!((at.width() - WIDTH * fit.k).abs() < 1e-3);
        assert!((at.height() - height(Some(&banner)) * fit.k).abs() < 1e-3);
        assert!((at.bottom() - (300. - 20. * fit.k)).abs() < 1e-3, "its lower edge keeps 20·k above the anchor: it does not cover the model");
        assert!((at.center().x - 400.).abs() < 1e-3);
        let smaller = rect_at(pos2(400., 300.), Some(&banner), Fit::zoomed(60.));
        assert!(smaller.height() < at.height(), "a farther camera makes a smaller banner");
    }

    #[test]
    fn the_strike_bow_scales_with_the_banner() {
        let banner = Rect::from_min_size(pos2(100., 50.), vec2(210., 52.));
        assert_eq!(strike_rect(banner), Rect::from_min_size(pos2(70., 53.), vec2(26., 26.)));
        assert_eq!(strike_rect_at(banner, 1.), strike_rect(banner));
        assert_eq!(strike_rect_at(banner, 0.5).size(), vec2(13., 13.));
    }
}
