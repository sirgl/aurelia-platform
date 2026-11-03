//! The observer's grid: how many columns and rows hold the powers' panels, where each
//! panel stands in the window and which one a click hits. Pure functions of the count
//! and the area, so the same grid comes out on every run and in the tests.
use egui::{pos2, vec2, Pos2, Rect, Vec2};

/// The cell aspect (width over height, the header included) the shape aims at.
const WANTED_ASPECT: f32 = 1.3;
/// How much a cell left empty costs, next to a cell far from the wanted aspect.
const EMPTY_COST: f32 = 2.0;
/// The most panels a grid holds: the renderer palette names eight owners, and the
/// window has no room to read more.
pub const MOST_PANELS: usize = 12;

/// Columns and rows of the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub cols: u32,
    pub rows: u32,
}

impl Shape {
    pub fn cells(self) -> usize {
        (self.cols * self.rows) as usize
    }
}

/// The shape for `count` panels in an area of `area` points: the one whose cells come
/// nearest the wanted aspect, with as few empty cells as possible (2×2 for four powers,
/// 3×2 for five and six, 4×2 for seven and eight on a wide window).
pub fn shape(count: usize, area: Vec2) -> Shape {
    let count = count.clamp(1, MOST_PANELS);
    let cost = |cols: usize| {
        let rows = count.div_ceil(cols);
        let aspect = (area.x / cols as f32) / (area.y / rows as f32).max(1.0);
        let empty = (cols * rows - count) as f32 / (cols * rows) as f32;
        (aspect / WANTED_ASPECT).ln().abs() + EMPTY_COST * empty
    };
    let cols = (1..=count).min_by(|a, b| cost(*a).total_cmp(&cost(*b))).unwrap_or(1);
    Shape { cols: cols as u32, rows: count.div_ceil(cols) as u32 }
}

/// The rect of each of `count` panels in `area`, row by row, `gap` apart. The last row
/// is centred when it is not full.
pub fn cells(count: usize, area: Rect, gap: f32) -> Vec<Rect> {
    let count = count.min(MOST_PANELS);
    if count == 0 {
        return Vec::new();
    }
    let shape = shape(count, area.size());
    let cell = vec2(
        ((area.width() - gap * (shape.cols - 1) as f32) / shape.cols as f32).max(1.0),
        ((area.height() - gap * (shape.rows - 1) as f32) / shape.rows as f32).max(1.0),
    );
    (0..count)
        .map(|i| {
            let (row, col) = ((i as u32) / shape.cols, (i as u32) % shape.cols);
            let in_row = (count as u32 - row * shape.cols).min(shape.cols);
            let spare = (shape.cols - in_row) as f32 * (cell.x + gap) / 2.0;
            let min = pos2(area.left() + spare + col as f32 * (cell.x + gap), area.top() + row as f32 * (cell.y + gap));
            Rect::from_min_size(min, cell)
        })
        .collect()
}

/// The panel under `at`.
pub fn cell_at(cells: &[Rect], at: Pos2) -> Option<usize> {
    cells.iter().position(|cell| cell.contains(at))
}

/// The panel a digit key names (1 is the first), if there is one.
pub fn index_of_digit(digit: u8, count: usize) -> Option<usize> {
    let index = usize::from(digit).checked_sub(1)?;
    (index < count).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: Vec2 = vec2(1664.0, 860.0);

    fn dims(count: usize, area: Vec2) -> (u32, u32) {
        let s = shape(count, area);
        (s.cols, s.rows)
    }

    #[test]
    fn four_to_eight_powers_get_the_usual_grids() {
        assert_eq!(dims(1, WIDE), (1, 1));
        assert_eq!(dims(2, WIDE), (2, 1));
        assert_eq!(dims(4, WIDE), (2, 2));
        assert_eq!(dims(5, WIDE), (3, 2));
        assert_eq!(dims(6, WIDE), (3, 2));
        assert_eq!(dims(7, WIDE), (4, 2));
        assert_eq!(dims(8, WIDE), (4, 2));
    }

    #[test]
    fn a_tall_window_stacks_the_rows() {
        let (cols, rows) = dims(6, vec2(700.0, 1200.0));
        assert!(rows > cols, "{cols}x{rows}");
    }

    #[test]
    fn every_shape_holds_its_panels_with_less_than_a_row_spare() {
        for count in 1..=MOST_PANELS {
            for area in [WIDE, vec2(1280.0, 640.0), vec2(2560.0, 1300.0), vec2(600.0, 900.0)] {
                let s = shape(count, area);
                assert!(s.cells() >= count && s.cells() < count + s.cols as usize, "{count} in {area:?}: {s:?}");
            }
        }
    }

    #[test]
    fn cells_tile_the_area_without_overlap_or_overflow() {
        let area = Rect::from_min_size(pos2(10.0, 50.0), WIDE);
        for count in 1..=MOST_PANELS {
            let rects = cells(count, area, 6.0);
            assert_eq!(rects.len(), count);
            for (i, a) in rects.iter().enumerate() {
                assert!(area.expand(0.01).contains_rect(*a), "{count}: {a:?} leaves {area:?}");
                for b in &rects[i + 1..] {
                    assert!(!(a.min.x < b.max.x - 0.5 && b.min.x < a.max.x - 0.5 && a.min.y < b.max.y - 0.5 && b.min.y < a.max.y - 0.5), "{count}: {a:?} overlaps {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_short_last_row_is_centred() {
        let area = Rect::from_min_size(Pos2::ZERO, WIDE);
        let rects = cells(5, area, 0.0);
        let last = &rects[3..];
        let left = last[0].left() - area.left();
        let right = area.right() - last[1].right();
        assert!((left - right).abs() < 0.01, "{left} vs {right}");
    }

    #[test]
    fn a_click_hits_the_panel_under_it_and_a_gap_hits_none() {
        let area = Rect::from_min_size(Pos2::ZERO, WIDE);
        let rects = cells(6, area, 8.0);
        for (i, rect) in rects.iter().enumerate() {
            assert_eq!(cell_at(&rects, rect.center()), Some(i));
        }
        let gap = pos2(rects[0].right() + 4.0, rects[0].center().y);
        assert_eq!(cell_at(&rects, gap), None);
    }

    #[test]
    fn digits_name_panels_from_one() {
        assert_eq!(index_of_digit(1, 6), Some(0));
        assert_eq!(index_of_digit(6, 6), Some(5));
        assert_eq!(index_of_digit(7, 6), None);
        assert_eq!(index_of_digit(0, 6), None);
    }
}
