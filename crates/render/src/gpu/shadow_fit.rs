//! Which part of the world the shadow map covers.
//!
//! The map covers what the camera sees: the camera's ground footprint plus the reach of
//! casters standing outside it, within the known world. Every change of the area
//! re-renders the static casters, so the area has slack: it grows past the footprint by
//! a margin and stays while the footprint fits inside it and is not much smaller. The
//! texels themselves sit on a grid fixed in the world (`surface_shadows::fit`), so a
//! moved area with the same texel size shades the same ground the same way: no shimmer.

/// An XZ box: low and high corners.
pub type Area = [[f32; 2]; 2];

/// Margin around the visible ground that may cast shadows into it (world units).
const REACH: f32 = 2.0;
/// Slack of a new area on each side, as a share of the footprint size.
const SLACK: f32 = 0.2;
/// Grid the area corners snap to (world units).
const STEP: f32 = 4.0;
/// A kept area may be at most this many times the size a fresh fit would have.
const MAX_GROWTH: f32 = 1.6;

#[derive(Debug, Default, Clone)]
pub struct ShadowArea {
    current: Option<Area>,
}

impl ShadowArea {
    /// The area for a camera `footprint` in a world whose known tiles span `known`;
    /// None when nothing known is in view.
    pub fn update(&mut self, footprint: Area, known: Option<Area>) -> Option<Area> {
        let wanted = known.and_then(|known| intersect(grow(footprint, REACH), grow(known, REACH)));
        let Some(wanted) = wanted else {
            self.current = None;
            return None;
        };
        let keep = self.current.filter(|current| {
            let fresh = slack(wanted);
            contains(*current, wanted) && size(*current) <= size(fresh) * MAX_GROWTH
        });
        self.current = Some(keep.unwrap_or_else(|| slack(wanted)));
        self.current
    }
}

fn slack(area: Area) -> Area {
    let margin = [(area[1][0] - area[0][0]) * SLACK, (area[1][1] - area[0][1]) * SLACK];
    let down = |v: f32| (v / STEP).floor() * STEP;
    let up = |v: f32| (v / STEP).ceil() * STEP;
    [
        [down(area[0][0] - margin[0]), down(area[0][1] - margin[1])],
        [up(area[1][0] + margin[0]), up(area[1][1] + margin[1])],
    ]
}

fn grow(area: Area, by: f32) -> Area {
    [[area[0][0] - by, area[0][1] - by], [area[1][0] + by, area[1][1] + by]]
}

fn intersect(a: Area, b: Area) -> Option<Area> {
    let low = [a[0][0].max(b[0][0]), a[0][1].max(b[0][1])];
    let high = [a[1][0].min(b[1][0]), a[1][1].min(b[1][1])];
    (low[0] < high[0] && low[1] < high[1]).then_some([low, high])
}

fn contains(outer: Area, inner: Area) -> bool {
    outer[0][0] <= inner[0][0] && outer[0][1] <= inner[0][1] && outer[1][0] >= inner[1][0] && outer[1][1] >= inner[1][1]
}

fn size(area: Area) -> f32 {
    (area[1][0] - area[0][0]).max(area[1][1] - area[0][1])
}

/// XZ box of the known tiles (visible now or explored), widened by one hex radius.
pub fn known_area(world: &crate::world::state::RenderWorld) -> Option<Area> {
    let width = world.width.max(1);
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for i in (0..world.terrain.len()).filter(|&i| world.is_known(i)) {
        let (x, z) = crate::scene::layout::tile_to_world(i as u32 % width, i as u32 / width);
        low = [low[0].min(x), low[1].min(z)];
        high = [high[0].max(x), high[1].max(z)];
    }
    (low[0] <= high[0]).then_some([[low[0] - 1.0, low[1] - 1.0], [high[0] + 1.0, high[1] + 1.0]])
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: Area = [[-500.0, -500.0], [500.0, 500.0]];

    #[test]
    fn the_area_covers_the_footprint_and_stays_while_the_view_pans_inside_it() {
        let mut area = ShadowArea::default();
        let view = [[10.0, 20.0], [70.0, 54.0]];
        let first = area.update(view, Some(KNOWN)).unwrap();
        assert!(contains(first, grow(view, REACH)));
        // A small pan keeps the area: the static casters are not drawn again.
        let panned = [[13.0, 21.5], [73.0, 55.5]];
        assert_eq!(area.update(panned, Some(KNOWN)), Some(first));
        // A long pan moves it, and the new one again covers the view.
        let far = [[60.0, 20.0], [120.0, 54.0]];
        let moved = area.update(far, Some(KNOWN)).unwrap();
        assert_ne!(moved, first);
        assert!(contains(moved, grow(far, REACH)));
    }

    #[test]
    fn zooming_in_shrinks_the_area_and_unknown_ground_casts_nothing() {
        let mut area = ShadowArea::default();
        let wide = area.update([[0.0, 0.0], [200.0, 120.0]], Some(KNOWN)).unwrap();
        let close = area.update([[90.0, 50.0], [110.0, 62.0]], Some(KNOWN)).unwrap();
        assert!(size(close) < size(wide) / 3.0, "a close view gets a dense map");
        let known = [[0.0, 0.0], [30.0, 30.0]];
        let clipped = area.update([[-100.0, -100.0], [100.0, 100.0]], Some(known)).unwrap();
        assert!(size(clipped) < 60.0, "only the known world is covered: {clipped:?}");
        assert_eq!(area.update([[0.0, 0.0], [10.0, 10.0]], None), None);
    }
}
