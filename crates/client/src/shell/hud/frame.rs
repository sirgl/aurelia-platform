//! Where the shell's own controls stand over the map: the top bar, the Empire button, the
//! minimap with the map buttons, and the end-turn button. A module picks its frame
//! (`ModuleUi::hud_frame`); the module's panels are laid out around these controls
//! (`layout`), so the frame is the one place that decides the corners of the window.
use egui::{pos2, vec2, Rect, Vec2};

/// The shell's frame over the map.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HudFrame {
    /// The classic module's frame: a 40-point top bar, the Empire button under it at the right,
    /// the minimap at the bottom right with the end-turn button over it, the map buttons
    /// at the bottom left; the interface scale is the player's.
    #[default]
    Classic,
    /// The frame of the Lands & Laws mockups: a 56-point opaque top bar holding the Empire
    /// button, the minimap with the map buttons over it at the bottom left, a 260×72
    /// end-turn button at the bottom right; the interface grows with the window
    /// (`ui_scale`), so a large window shows the layout of 1600×900, not small panels.
    Wide,
    /// The thin frame of the Lands & Laws map: a 36-point translucent top bar, a small
    /// minimap at the bottom right with a slim end-turn button over it, four 36-point map
    /// buttons at the bottom left and a 110×28 Empire button under the bar. It leaves the
    /// map to the map: its controls cover about 9 % of a 1280×720 window (the classic
    /// frame's, 14 %). The interface scale is the player's.
    Compact,
}

/// The rects of the shell's controls on a screen (logical points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameRects {
    pub top_bar: Rect,
    /// The Empire button (inside the top bar in the wide frame).
    pub empire: Rect,
    /// The minimap's panel, and the block it makes with what sits on it (the classic
    /// frame's end-turn row).
    pub minimap: Rect,
    pub minimap_block: Rect,
    /// The four map buttons, and the side of one and the step between two.
    pub map_buttons: Rect,
    pub button_side: f32,
    pub button_step: f32,
    pub end_turn: Rect,
    /// A narrow window (< 800 points): the classic frame's small minimap.
    pub compact: bool,
}

impl FrameRects {
    /// The controls every module panel keeps out of.
    pub fn controls(&self) -> Vec<Rect> {
        let mut controls = vec![self.top_bar, self.minimap_block, self.map_buttons];
        if !self.top_bar.contains_rect(self.empire) {
            controls.push(self.empire);
        }
        if !self.minimap_block.contains_rect(self.end_turn) {
            controls.push(self.end_turn);
        }
        controls
    }
}

/// The wide frame's window of reference: a larger window scales the interface up.
const REFERENCE: Vec2 = Vec2::new(1600., 900.);

impl HudFrame {
    pub fn rects(self, screen: Rect) -> FrameRects {
        match self {
            HudFrame::Classic => classic(screen),
            HudFrame::Wide => wide(screen),
            HudFrame::Compact => compact(screen),
        }
    }

    /// The top bar is opaque (the wide frame), not a translucent strip over the map.
    pub fn opaque_top(self) -> bool {
        self == HudFrame::Wide
    }

    /// The interface scale for a window of `window` points (before any scale): the wide
    /// frame grows with the window up to twice, never shrinks; the classic one keeps 1.
    pub fn ui_scale(self, window: Vec2) -> f32 {
        match self {
            HudFrame::Classic | HudFrame::Compact => 1.,
            HudFrame::Wide => (window.x / REFERENCE.x).min(window.y / REFERENCE.y).clamp(1., 2.),
        }
    }
}

fn classic(screen: Rect) -> FrameRects {
    let compact = screen.width() < 800.;
    // A phone's finger needs 48 points: the buttons are 44 (and reached from 3 points around them).
    let touch = compact && crate::shell::theme::touch_wanted();
    let (side, step) = match (compact, touch) {
        (true, true) => (44., 50.),
        (true, false) => (39., 45.),
        _ => (53., 67.),
    };
    let heading = if compact { 28. } else { 73. };
    let width = if compact { 145. } else if screen.width() < 1300. { 245. } else { 290. };
    // On a phone the buttons take more of the bottom row: the minimap gives way.
    let width = if touch { (screen.width() - 19. - (step * 3. + side) - 8. - 16.).clamp(100., width) } else { width };
    let size = vec2(width, if compact { 105. } else { 183. });
    let minimap = Rect::from_min_size(pos2(screen.right() - size.x - 16., screen.bottom() - size.y - 17.), size);
    let end_turn = match compact {
        true if touch => Rect::from_min_size(minimap.left_top() + vec2(0., -42.), vec2(minimap.width(), 40.)),
        true => Rect::from_min_size(minimap.left_top() + vec2(0., -25.), vec2(minimap.width(), 23.)),
        false => Rect::from_min_max(pos2(minimap.left() + 45., minimap.top() - 45.), pos2(minimap.right(), minimap.top() - 8.)),
    };
    FrameRects {
        top_bar: Rect::from_min_size(screen.min, vec2(screen.width(), 40.)),
        empire: Rect::from_min_size(pos2(screen.right() - 152., screen.top() + 51.), vec2(136., 34.)),
        minimap,
        minimap_block: minimap.with_min_y(minimap.top() - heading),
        map_buttons: Rect::from_min_size(pos2(screen.left() + 19., screen.bottom() - 75.), vec2(step * 3. + side, side + 2.)),
        button_side: side,
        button_step: step,
        end_turn,
        compact,
    }
}

/// The compact frame: the sizes of a window from 800 points up are fixed (the panels of the
/// map stay readable, the controls stay small on a large window too); a narrower window gets
/// a smaller minimap.
fn compact(screen: Rect) -> FrameRects {
    const MARGIN: f32 = 8.;
    const GAP: f32 = 4.;
    let narrow = screen.width() < 800.;
    let top_bar = Rect::from_min_size(screen.min, vec2(screen.width(), 36.));
    let size = if narrow { vec2(130., 86.) } else { vec2(180., 118.) };
    let minimap = Rect::from_min_size(pos2(screen.right() - MARGIN - size.x, screen.bottom() - MARGIN - size.y), size);
    let end_turn = Rect::from_min_size(minimap.left_top() - vec2(0., GAP + 34.), vec2(size.x, 34.));
    let (side, step) = if narrow { (32., 38.) } else { (36., 42.) };
    FrameRects {
        top_bar,
        empire: Rect::from_min_size(pos2(screen.right() - MARGIN - 110., top_bar.bottom() + GAP), vec2(110., 28.)),
        minimap,
        minimap_block: minimap.with_min_y(end_turn.top()),
        map_buttons: Rect::from_min_size(pos2(screen.left() + MARGIN, screen.bottom() - MARGIN - side - 2.), vec2(step * 3. + side, side + 2.)),
        button_side: side,
        button_step: step,
        end_turn,
        compact: false,
    }
}

fn wide(screen: Rect) -> FrameRects {
    const MARGIN: f32 = 12.;
    let top_bar = Rect::from_min_size(screen.min, vec2(screen.width(), 56.));
    let minimap = Rect::from_min_size(pos2(screen.left() + MARGIN, screen.bottom() - MARGIN - 196.), vec2(240., 196.));
    let (side, step) = (40., 48.);
    let map_buttons = Rect::from_min_size(pos2(minimap.left(), minimap.top() - 8. - side), vec2(step * 3. + side, side));
    FrameRects {
        top_bar,
        // Left of the settings gear (its button is 29 wide, 14.5 from the right edge).
        empire: Rect::from_min_size(pos2(screen.right() - 52. - 136., top_bar.center().y - 17.), vec2(136., 34.)),
        minimap,
        minimap_block: minimap,
        map_buttons,
        button_side: side,
        button_step: step,
        end_turn: Rect::from_min_size(pos2(screen.right() - MARGIN - 260., screen.bottom() - MARGIN - 72.), vec2(260., 72.)),
        compact: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::layout::overlap;

    #[test]
    fn controls_never_overlap_on_any_window() {
        for frame in [HudFrame::Classic, HudFrame::Wide, HudFrame::Compact] {
            for size in [vec2(640., 360.), vec2(960., 640.), vec2(1280., 720.), vec2(1600., 900.), vec2(2560., 1440.)] {
                let screen = Rect::from_min_size(egui::Pos2::ZERO, size);
                let controls = frame.rects(screen).controls();
                for (i, a) in controls.iter().enumerate() {
                    assert!(screen.contains_rect(*a), "{frame:?} {size:?}: {a:?} leaves the window");
                    for b in &controls[i + 1..] {
                        assert!(!overlap(*a, *b), "{frame:?} {size:?}: {a:?} overlaps {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_wide_frame_scales_with_the_window_between_one_and_two() {
        assert_eq!(HudFrame::Wide.ui_scale(vec2(1280., 720.)), 1.);
        assert_eq!(HudFrame::Wide.ui_scale(vec2(2560., 1440.)), 1.6);
        assert_eq!(HudFrame::Wide.ui_scale(vec2(5120., 2880.)), 2.);
        assert_eq!(HudFrame::Classic.ui_scale(vec2(2560., 1440.)), 1.);
        assert_eq!(HudFrame::Compact.ui_scale(vec2(2560., 1440.)), 1.);
    }

    /// The compact frame's controls are a fraction of the classic frame's, at every size.
    #[test]
    fn the_compact_frame_is_thinner_than_the_classic_one() {
        let area = |frame: HudFrame, size: Vec2| {
            let screen = Rect::from_min_size(egui::Pos2::ZERO, size);
            frame.rects(screen).controls().iter().map(|r| r.area()).sum::<f32>() / screen.area()
        };
        for size in [vec2(1280., 720.), vec2(1664., 936.), vec2(2560., 1440.)] {
            let (compact, classic) = (area(HudFrame::Compact, size), area(HudFrame::Classic, size));
            assert!(compact <= 0.10 && compact < classic * 0.7, "{size:?}: compact {compact:.3}, classic {classic:.3}");
        }
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(1280., 720.));
        let rects = HudFrame::Compact.rects(screen);
        assert_eq!(rects.top_bar.height(), 36.);
        assert_eq!(rects.empire.size(), vec2(110., 28.));
        assert_eq!((rects.button_side, rects.button_step), (36., 42.));
    }
}
