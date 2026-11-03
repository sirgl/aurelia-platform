//! Where things stand on the observer's screen: the bar of the clock at the top, the
//! grid of the panels (or the one zoomed panel) below it and a hint line at the bottom.
//! A pure function of the window and the count, so the panels' pixel sizes are known
//! before the frame is drawn and the tests can check every window size.
use egui::{pos2, vec2, Rect};

use super::grid;
use crate::shell::layout::{GAP, MARGIN};

/// Height of the bar.
pub const BAR_HEIGHT: f32 = 44.0;
/// The widest the bar gets.
pub const BAR_WIDTH: f32 = 980.0;
/// Height of the hint line under the grid.
pub const HINT_HEIGHT: f32 = 20.0;
/// Height of a panel's header in the grid: the name, then the figures.
pub const HEADER_HEIGHT: f32 = 58.0;
/// Height of the header of one power shown alone: also its research and income, and its
/// last events.
pub const HEADER_SINGLE: f32 = 112.0;
/// Height of the row of the powers' chips under the bar.
pub const CHIPS_HEIGHT: f32 = 34.0;
/// The share of a panel's pixels its picture is drawn at while the grid shows several
/// panels (a smaller texture is cheaper to draw and to keep).
pub const GRID_RESOLUTION: f32 = 0.75;
/// The most pixels a panel's picture has on a side.
pub const LARGEST_SIDE: u32 = 4096;

/// A size in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// One panel on screen: the power it shows and where it stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub power: usize,
    pub rect: Rect,
}

/// The screen of the observer.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub bar: Rect,
    /// The row of the powers' chips and the camera buttons (the view of one power only).
    pub chips: Option<Rect>,
    /// How tall the headers of the cells are.
    pub head: f32,
    /// Every panel on screen. Hidden panels are absent.
    pub cells: Vec<Cell>,
    pub hint: Rect,
}

/// The frame of `count` panels in `screen`; `focus` shows one panel over the whole grid.
pub fn frame(screen: Rect, count: usize, focus: Option<usize>) -> Frame {
    let inner = screen.shrink(MARGIN);
    let width = BAR_WIDTH.min(inner.width());
    let bar = Rect::from_min_size(pos2(inner.center().x - width / 2.0, inner.top()), vec2(width, BAR_HEIGHT));
    let hint = Rect::from_min_max(pos2(inner.left(), inner.bottom() - HINT_HEIGHT), inner.max);
    let single = focus.filter(|f| *f < count);
    let chips = single.map(|_| Rect::from_min_size(pos2(inner.left(), bar.bottom() + GAP), vec2(inner.width(), CHIPS_HEIGHT)));
    let top = chips.map_or(bar.bottom(), |c| c.bottom()) + GAP;
    let area = Rect::from_min_max(pos2(inner.left(), top), pos2(inner.right(), hint.top() - GAP / 2.0));
    let cells = match single {
        Some(power) => vec![Cell { power, rect: area }],
        None => grid::cells(count, area, GAP).into_iter().enumerate().map(|(power, rect)| Cell { power, rect }).collect(),
    };
    let head = if single.is_some() { HEADER_SINGLE } else { HEADER_HEIGHT };
    Frame { bar, chips, head, cells, hint }
}

impl Frame {
    /// The rect of the power's panel, if it is on screen.
    pub fn cell(&self, power: usize) -> Option<Rect> {
        self.cells.iter().find(|c| c.power == power).map(|c| c.rect)
    }

    pub fn power_at(&self, at: egui::Pos2) -> Option<usize> {
        self.cells.iter().find(|c| c.rect.contains(at)).map(|c| c.power)
    }
}

/// The part of a cell that shows the map, under a header `head` tall.
pub fn picture(cell: Rect, head: f32) -> Rect {
    Rect::from_min_max(pos2(cell.left(), (cell.top() + head).min(cell.bottom())), cell.max)
}

/// The header strip of a cell, `head` tall.
pub fn header(cell: Rect, head: f32) -> Rect {
    Rect::from_min_max(cell.min, pos2(cell.right(), (cell.top() + head).min(cell.bottom())))
}

/// The pixel size of a cell's picture: `scale` of its physical pixels (rounded to 16 so a
/// window drag does not remake the texture at every pixel), at least 16 and at most
/// `LARGEST_SIDE`.
pub fn pixels(picture: Rect, points_to_pixels: f32, scale: f32) -> Size {
    let side = |points: f32| ((points * points_to_pixels * scale / 16.0).round() as u32 * 16).clamp(16, LARGEST_SIDE);
    Size { width: side(picture.width()), height: side(picture.height()) }
}

/// The resolution a panel is drawn at: full when one panel fills the grid.
pub fn resolution(focus: Option<usize>) -> f32 {
    match focus {
        Some(_) => 1.0,
        None => GRID_RESOLUTION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZES: [(f32, f32); 4] = [(1280.0, 720.0), (1664.0, 936.0), (2560.0, 1440.0), (1024.0, 600.0)];

    fn screen(size: (f32, f32)) -> Rect {
        Rect::from_min_size(egui::Pos2::ZERO, vec2(size.0, size.1))
    }

    fn overlap(a: Rect, b: Rect) -> bool {
        a.min.x < b.max.x - 0.5 && b.min.x < a.max.x - 0.5 && a.min.y < b.max.y - 0.5 && b.min.y < a.max.y - 0.5
    }

    #[test]
    fn the_bar_the_cells_and_the_hint_never_overlap_and_stay_in_the_window() {
        for size in SIZES {
            for count in 1..=grid::MOST_PANELS {
                for focus in [None, Some(0), Some(count - 1)] {
                    let f = frame(screen(size), count, focus);
                    let mut rects = vec![f.bar, f.hint];
                    rects.extend(f.chips);
                    rects.extend(f.cells.iter().map(|c| c.rect));
                    for (i, a) in rects.iter().enumerate() {
                        assert!(screen(size).expand(0.01).contains_rect(*a), "{size:?} {count} {focus:?}: {a:?} leaves the window");
                        for b in &rects[i + 1..] {
                            assert!(!overlap(*a, *b), "{size:?} {count} {focus:?}: {a:?} overlaps {b:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_zoomed_panel_takes_the_whole_grid() {
        let f = frame(screen(SIZES[1]), 6, Some(3));
        assert_eq!(f.cells.len(), 1);
        assert_eq!(f.cells[0].power, 3);
        let grid = frame(screen(SIZES[1]), 6, None);
        assert!(f.cells[0].rect.width() > 2.0 * grid.cells[0].rect.width() * 0.9);
    }

    #[test]
    fn six_panels_fit_with_a_readable_picture_at_the_smallest_window() {
        let f = frame(screen(SIZES[0]), 8, None);
        for Cell { rect: cell, .. } in &f.cells {
            assert!(picture(*cell, f.head).height() > 120.0 && picture(*cell, f.head).width() > 200.0, "{cell:?}");
        }
    }

    #[test]
    fn pixel_sizes_are_rounded_and_bounded() {
        let r = Rect::from_min_size(egui::Pos2::ZERO, vec2(401.0, 233.0));
        assert_eq!(pixels(r, 2.0, 0.75), Size { width: 608, height: 352 });
        assert_eq!(pixels(Rect::ZERO, 2.0, 1.0), Size { width: 16, height: 16 });
        assert_eq!(pixels(Rect::from_min_size(egui::Pos2::ZERO, vec2(9000.0, 20.0)), 1.0, 1.0).width, LARGEST_SIDE);
    }
}
