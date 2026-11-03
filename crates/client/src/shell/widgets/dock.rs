//! Marks on a round button of a module's dock: a counter or an exclamation sign in a
//! coloured disc at the button's upper right, and a one-letter label at its lower right.
//! The button itself is `hud::round_button`; the module decides what each mark means.
use crate::shell::hud::{GOLD, INK, LIGHT};
use egui::{pos2, vec2, Align2, Color32, FontId, Painter, Rect, Stroke};

/// Something needs the player: red.
pub const ALERT: Color32 = Color32::from_rgb(222, 96, 84);
/// A chance, not a duty: gold.
pub const CHANCE: Color32 = GOLD;
/// News to read: light blue.
pub const NEWS: Color32 = Color32::from_rgb(100, 170, 214);

/// A counter on a button.
#[derive(Debug, Clone, PartialEq)]
pub struct Badge {
    /// «3», «!», at most three characters.
    pub text: String,
    pub color: Color32,
}

impl Badge {
    pub fn alert(text: impl Into<String>) -> Self {
        Badge { text: text.into(), color: ALERT }
    }

    pub fn chance(text: impl Into<String>) -> Self {
        Badge { text: text.into(), color: CHANCE }
    }

    pub fn news(text: impl Into<String>) -> Self {
        Badge { text: text.into(), color: NEWS }
    }

    /// A count: «99+» beyond two digits.
    pub fn count(n: usize, color: Color32) -> Self {
        let text = if n > 99 { "99+".to_owned() } else { n.to_string() };
        Badge { text, color }
    }
}

/// The disc's radius, and how far the button's edge overlaps it.
pub const RADIUS: f32 = 9.;

/// The badge at the upper right of the round button `button`.
pub fn badge(p: &Painter, button: Rect, badge: &Badge) {
    let c = pos2(button.right() - 4., button.top() + 4.);
    let font = FontId::proportional(if badge.text.chars().count() > 2 { 9. } else { 11. });
    let width = p.layout_no_wrap(badge.text.clone(), font.clone(), INK).size().x;
    // A pill when the text is wider than the disc.
    let pill = Rect::from_center_size(c, vec2((width + 8.).max(2. * RADIUS), 2. * RADIUS));
    p.rect_filled(pill.translate(vec2(0., 1.)), RADIUS, Color32::from_black_alpha(110));
    p.rect_filled(pill, RADIUS, badge.color);
    p.rect_stroke(pill, RADIUS, Stroke::new(1., INK), egui::StrokeKind::Outside);
    p.text(pill.center(), Align2::CENTER_CENTER, &badge.text, font, INK);
}

/// A one-letter label at the lower right of the round button `button`.
pub fn label(p: &Painter, button: Rect, letter: &str) {
    let c = pos2(button.right() - 5., button.bottom() - 5.);
    let disc = Rect::from_center_size(c, vec2(2. * RADIUS - 2., 2. * RADIUS - 2.));
    p.circle_filled(disc.center(), disc.width() * 0.5, INK);
    p.circle_stroke(disc.center(), disc.width() * 0.5, Stroke::new(1., GOLD));
    p.text(disc.center(), Align2::CENTER_CENTER, letter, FontId::proportional(10.), LIGHT);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(draw: impl Fn(&Painter, Rect)) -> Vec<String> {
        let ctx = egui::Context::default();
        let button = Rect::from_min_size(pos2(20., 20.), vec2(53., 53.));
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| draw(ui.painter(), button));
        output.textures_delta.clear();
        output.shapes.iter().filter_map(|s| match &s.shape { egui::Shape::Text(t) => Some(t.galley.text().to_owned()), _ => None }).collect()
    }

    #[test]
    fn a_badge_and_a_label_write_their_text() {
        assert_eq!(texts(|p, b| badge(p, b, &Badge::alert("!"))), ["!"]);
        assert_eq!(texts(|p, b| label(p, b, "C")), ["C"]);
    }

    #[test]
    fn a_count_beyond_two_digits_is_capped() {
        assert_eq!(Badge::count(7, ALERT).text, "7");
        assert_eq!(Badge::count(100, ALERT).text, "99+");
    }
}
