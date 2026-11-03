//! The HUD layout: where the panels over the map go.
//!
//! Panels used to place themselves at fixed offsets from the window's edges, so on a small
//! window, or when a list grew, they covered each other and the shell's own controls. Now a
//! panel declares where it belongs (`Anchor`), the size it wants and the least it can do
//! with, how much it matters (`Priority`) and whether it may be hidden (`Collapse`). The
//! layout gives it the free spot nearest its home: panels of one anchor stack away from it
//! (a side's or a corner's along its column, the top's or the bottom's along its row),
//! never across another panel or a control of the shell (the top bar, the Empire button,
//! the minimap, the end-turn button, the map buttons). A panel of the top or the bottom
//! edge never goes past the middle of the height, one of a side or a corner never past
//! the middle of the width, so the map's centre stays free.
//! Without room a panel shrinks towards its least size, then hides if it may. What is left
//! is the map's (`HudLayout::free_map`).
//!
//! Panels are immediate-mode like the rest of the HUD: a panel asks for its rect while it
//! draws (`place`). The frame's layout lives in the egui context next to egui's own area
//! memory. It solves the last frame's panels in priority order at the start of a frame
//! (`begin`), and a panel asking this frame keeps out of the planned spots of the panels
//! that outrank it, so the order panels are drawn in does not decide who gets the room;
//! a panel that changed its size gets its final spot one frame later.
use egui::{pos2, Context, Id, Pos2, Rect, Vec2};

/// Room between two panels, and between a panel and a control.
pub const GAP: f32 = 8.;
/// Room between a panel at its anchor's home and the window's edge (or the top bar).
pub const MARGIN: f32 = 12.;

/// Where a panel belongs on the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    Centre,
}

/// One axis of an anchor: the panel hugs the start, the middle or the end of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Start,
    Middle,
    End,
}

impl Side {
    /// The start of a span of `size` on this side of `from..to`.
    fn start(self, from: f32, to: f32, size: f32) -> f32 {
        match self {
            Side::Start => from,
            Side::Middle => (from + to - size) * 0.5,
            Side::End => to - size,
        }
    }
}

impl Anchor {
    /// The horizontal and the vertical side.
    fn sides(self) -> (Side, Side) {
        use Side::*;
        match self {
            Anchor::TopLeft => (Start, Start),
            Anchor::Top => (Middle, Start),
            Anchor::TopRight => (End, Start),
            Anchor::Left => (Start, Middle),
            Anchor::Right => (End, Middle),
            Anchor::BottomLeft => (Start, End),
            Anchor::Bottom => (Middle, End),
            Anchor::BottomRight => (End, End),
            Anchor::Centre => (Middle, Middle),
        }
    }

    /// Where a panel of `size` stands at this anchor of `area`.
    pub fn home(self, area: Rect, size: Vec2) -> Rect {
        let (h, v) = self.sides();
        let min = pos2(h.start(area.left(), area.right(), size.x), v.start(area.top(), area.bottom(), size.y));
        Rect::from_min_size(min, size)
    }

    /// `home` resized to `size`, keeping the edges that face the anchor.
    fn resize(self, home: Rect, size: Vec2) -> Rect {
        self.home(home, size)
    }

    /// How far a panel at `home` may move from the anchor: one anchored to the top or the
    /// bottom edge stays on its half of the height, one anchored to a side or a corner
    /// travels along its column but stays on its half of the width (a home that already
    /// crosses the middle keeps its place). The map's centre stays free.
    fn reach(self, screen: Rect, home: Rect) -> Rect {
        let centre = screen.center();
        let mut reach = screen;
        match self.sides() {
            (Side::Middle, Side::Start) => reach.max.y = home.bottom().max(centre.y),
            (Side::Middle, Side::End) => reach.min.y = home.top().min(centre.y),
            (Side::Start, _) => reach.max.x = home.right().max(centre.x),
            (Side::End, _) => reach.min.x = home.left().min(centre.x),
            (Side::Middle, Side::Middle) => {}
        }
        reach
    }
}

impl Anchor {
    /// What moving a step costs on each axis: a panel of a side or a corner stacks along
    /// its column, one of the top or the bottom edge along its row.
    fn stack_weight(self) -> Vec2 {
        match self.sides() {
            (Side::Middle, Side::Start | Side::End) => Vec2::new(1., 2.),
            (Side::Start | Side::End, _) => Vec2::new(2., 1.),
            (Side::Middle, Side::Middle) => Vec2::new(1., 1.),
        }
    }
}

/// Where a panel looks for a spot: its own lane first, then anywhere within its reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Room {
    Lane,
    Reach,
}

/// How much a panel matters: a higher one keeps its place and size when room is short.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority(pub u8);

/// What happens to a panel that finds no room even at its least size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Collapse {
    /// Shown anyway at its least size by its anchor (`HudLayout::overflow` names it).
    Never,
    /// Hidden until there is room again.
    WhenShort,
}

/// A panel as it asks for room.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Panel {
    pub id: Id,
    /// The id's name (the panel's egui area has the same id).
    pub name: &'static str,
    pub anchor: Anchor,
    /// Its spot when nothing is in the way; None: the anchor's home on the window.
    pub home: Option<Rect>,
    /// The size it wants, and the least it can do with.
    pub size: Vec2,
    pub min: Vec2,
    pub priority: Priority,
    pub collapse: Collapse,
}

impl Panel {
    /// A panel at its anchor's home, of a fixed size, that is never hidden.
    pub fn new(name: &'static str, anchor: Anchor, size: Vec2) -> Self {
        Panel { id: Id::new(name), name, anchor, home: None, size, min: size, priority: Priority::default(), collapse: Collapse::Never }
    }

    /// A panel whose spot is `home` while it is free (the place it always had).
    pub fn at(name: &'static str, anchor: Anchor, home: Rect) -> Self {
        Panel { home: Some(home), ..Panel::new(name, anchor, home.size()) }
    }

    pub fn min(self, min: Vec2) -> Self {
        Panel { min: min.min(self.size), ..self }
    }

    pub fn priority(self, priority: u8) -> Self {
        Panel { priority: Priority(priority), ..self }
    }

    /// The panel may be hidden while there is no room for it.
    pub fn collapsible(self) -> Self {
        Panel { collapse: Collapse::WhenShort, ..self }
    }
}

/// A panel with its spot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub id: Id,
    pub name: &'static str,
    pub rect: Rect,
    pub priority: Priority,
}

/// The panels and controls of one frame (see the module docs).
#[derive(Debug, Clone)]
pub struct HudLayout {
    screen: Rect,
    /// The shell's own controls: fixed, every panel keeps out of them.
    controls: Vec<Rect>,
    /// Where the last frame's panels go when they ask again, by priority.
    plan: Vec<Placed>,
    /// The panels that asked this frame, in order, and the spots they got.
    asked: Vec<Panel>,
    placed: Vec<Placed>,
    hidden: Vec<Id>,
    overflow: Vec<Id>,
    /// The city banners of the last frame.
    labels: Vec<Rect>,
    /// What a full screen (a module's card, the bars the shell paints over the world) draws
    /// opaque this frame: map overlays keep out of it like out of a panel.
    covers: Vec<(&'static str, Rect)>,
    /// The map overlays drawn this frame (`overlay`), by kind: what tests read.
    overlays: Vec<(&'static str, Rect)>,
    /// The egui pass this layout belongs to.
    pass: u64,
}

impl Default for HudLayout {
    fn default() -> Self {
        HudLayout::new(Rect::ZERO, Vec::new())
    }
}

impl HudLayout {
    /// An empty layout of `screen` around the shell's `controls`.
    pub fn new(screen: Rect, controls: Vec<Rect>) -> Self {
        HudLayout { screen, controls, plan: Vec::new(), asked: Vec::new(), placed: Vec::new(), hidden: Vec::new(), overflow: Vec::new(), labels: Vec::new(), covers: Vec::new(), overlays: Vec::new(), pass: 0 }
    }

    /// The next frame: the panels that asked this frame are planned again, by priority.
    fn next(&self, screen: Rect, controls: Vec<Rect>, pass: u64) -> Self {
        let mut ranked = self.asked.clone();
        ranked.sort_by_key(|panel| std::cmp::Reverse(panel.priority));
        let mut plan = HudLayout::new(screen, controls.clone());
        for panel in ranked {
            plan.place(panel);
        }
        HudLayout { plan: plan.placed, labels: self.labels.clone(), pass, ..HudLayout::new(screen, controls) }
    }

    /// The spot of a panel (None: hidden for want of room).
    pub fn place(&mut self, panel: Panel) -> Option<Rect> {
        // A panel declared before the frame (`ModuleUi::hud_panels`) keeps that spot.
        if let Some(placed) = self.placed.iter().find(|p| p.id == panel.id) {
            return Some(placed.rect);
        }
        if self.hidden.contains(&panel.id) {
            return None;
        }
        self.asked.push(panel);
        let blocked = self.blocked_for(&panel);
        let home = panel.home.unwrap_or_else(|| panel.anchor.home(self.screen.shrink(MARGIN), panel.size));
        let found = self.fit(&panel, home, &blocked);
        let rect = match (found, panel.collapse) {
            (Some(rect), _) => rect,
            (None, Collapse::WhenShort) => {
                self.hidden.push(panel.id);
                return None;
            }
            (None, Collapse::Never) => {
                self.overflow.push(panel.id);
                panel.anchor.resize(home, panel.min)
            }
        };
        self.placed.push(Placed { id: panel.id, name: panel.name, rect, priority: panel.priority });
        Some(rect)
    }

    /// What a panel keeps out of: the controls, the panels placed this frame and the
    /// planned spots of those that outrank it and have not asked yet.
    fn blocked_for(&self, panel: &Panel) -> Vec<Rect> {
        let waiting = self.plan.iter().filter(|p| p.priority > panel.priority && p.id != panel.id && !self.placed.iter().any(|q| q.id == p.id));
        let panels = self.placed.iter().chain(waiting).filter(|p| p.id != panel.id).map(|p| p.rect);
        self.controls.iter().copied().chain(panels).collect()
    }

    /// The largest size between the panel's wish and its least that has a free spot, and
    /// that spot: coarse steps down, then halving between the last miss and the first fit.
    /// A spot in the panel's own lane (the row or column of its anchor) comes first, even
    /// shrunk; only then one anywhere within its reach.
    fn fit(&self, panel: &Panel, home: Rect, blocked: &[Rect]) -> Option<Rect> {
        [Room::Lane, Room::Reach].into_iter().find_map(|room| self.fit_in(panel, home, blocked, room))
    }

    fn fit_in(&self, panel: &Panel, home: Rect, blocked: &[Rect], room: Room) -> Option<Rect> {
        let size = |t: f32| panel.min + (panel.size - panel.min) * t;
        let spot = |t: f32| self.spot(panel.anchor, home, size(t), blocked, room);
        let steps: &[f32] = if panel.size == panel.min { &[1.] } else { &[1., 0.75, 0.5, 0.25, 0.] };
        let mut miss = None;
        for &t in steps {
            let Some(mut found) = spot(t) else {
                miss = Some(t);
                continue;
            };
            let (mut fits, mut misses) = (t, miss.unwrap_or(t));
            for _ in 0..6 {
                let mid = (fits + misses) * 0.5;
                match spot(mid) {
                    Some(rect) => (fits, found) = (mid, rect),
                    None => misses = mid,
                }
            }
            return Some(found);
        }
        None
    }

    /// The free spot of `size` nearest `home` (its anchored edges), in the panel's lane or
    /// anywhere within its reach.
    fn spot(&self, anchor: Anchor, home: Rect, size: Vec2, blocked: &[Rect], room: Room) -> Option<Rect> {
        let home = anchor.resize(home, size);
        let reach = anchor.reach(self.screen, home).intersect(self.screen);
        let weight = anchor.stack_weight();
        let in_lane = |x: f32, y: f32| match room {
            Room::Reach => true,
            Room::Lane if weight.x > weight.y => x == home.left(),
            Room::Lane if weight.y > weight.x => y == home.top(),
            Room::Lane => true,
        };
        let xs = std::iter::once(home.left()).chain(blocked.iter().flat_map(|b| [b.right() + GAP, b.left() - GAP - size.x]));
        let ys: Vec<f32> = std::iter::once(home.top()).chain(blocked.iter().flat_map(|b| [b.bottom() + GAP, b.top() - GAP - size.y])).collect();
        let mut best: Option<(f32, Rect)> = None;
        for x in xs {
            for &y in &ys {
                let rect = Rect::from_min_size(pos2(x, y), size);
                let distance = weight.x * (x - home.left()).abs() + weight.y * (y - home.top()).abs();
                if best.is_some_and(|(d, _)| d <= distance) || !in_lane(x, y) || !contains(reach, rect) || blocked.iter().any(|b| overlap(*b, rect)) {
                    continue;
                }
                best = Some((distance, rect));
            }
        }
        best.map(|(_, rect)| rect)
    }

    /// The shell's controls this frame.
    pub fn controls(&self) -> &[Rect] {
        &self.controls
    }

    /// The panels placed this frame, in the order they asked.
    pub fn placed(&self) -> &[Placed] {
        &self.placed
    }

    /// The spot a panel got this frame.
    pub fn rect(&self, id: impl Into<Id>) -> Option<Rect> {
        let id = id.into();
        self.placed.iter().find(|p| p.id == id).map(|p| p.rect)
    }

    /// Panels hidden this frame for want of room.
    pub fn hidden(&self) -> &[Id] {
        &self.hidden
    }

    /// Whether a panel was hidden for want of room (a panel that shows another's content
    /// then, as the agenda shows the events as a tab).
    pub fn is_hidden(&self, name: &str) -> bool {
        self.hidden.contains(&Id::new(name))
    }

    /// The city banners on the map as the shell drew them in the last frame: map marks
    /// keep clear of them (they are not obstacles for panels, which they hide under).
    pub fn labels(&self) -> &[Rect] {
        &self.labels
    }

    /// Panels that may not hide and found no room: they cover something.
    pub fn overflow(&self) -> &[Id] {
        &self.overflow
    }

    /// Whether `rect` touches a control or a panel (placed, or planned and not drawn yet):
    /// map labels and badges hide there.
    pub fn covers(&self, rect: Rect) -> bool {
        self.obstacles().any(|o| overlap(o, rect))
    }

    /// Every control and panel: placed this frame, or planned and not drawn yet, and what a
    /// full screen declared opaque (`cover`).
    pub fn obstacles(&self) -> impl Iterator<Item = Rect> + '_ {
        let waiting = self.plan.iter().filter(|p| !self.placed.iter().any(|q| q.id == p.id));
        self.controls.iter().copied().chain(self.placed.iter().chain(waiting).map(|p| p.rect)).chain(self.covers.iter().map(|(_, rect)| *rect))
    }

    /// The map overlays drawn this frame, by kind and rect (`overlay`).
    pub fn overlays(&self) -> &[(&'static str, Rect)] {
        &self.overlays
    }

    /// What full screens declared opaque this frame, by name (`cover`).
    pub fn covers_declared(&self) -> &[(&'static str, Rect)] {
        &self.covers
    }

    /// The largest free rectangle of the window: the map no control or panel covers.
    pub fn free_map(&self) -> Rect {
        let obstacles: Vec<Rect> = self.obstacles().collect();
        let s = self.screen;
        let lefts: Vec<f32> = std::iter::once(s.left()).chain(obstacles.iter().map(|o| o.right())).collect();
        let rights: Vec<f32> = std::iter::once(s.right()).chain(obstacles.iter().map(|o| o.left())).collect();
        let tops: Vec<f32> = std::iter::once(s.top()).chain(obstacles.iter().map(|o| o.bottom())).collect();
        let bottoms: Vec<f32> = std::iter::once(s.bottom()).chain(obstacles.iter().map(|o| o.top())).collect();
        let mut best = Rect::NOTHING;
        for &top in &tops {
            for &bottom in bottoms.iter().filter(|&&b| b > top) {
                for &left in &lefts {
                    for &right in rights.iter().filter(|&&r| r > left) {
                        let rect = Rect::from_min_max(pos2(left, top), pos2(right, bottom)).intersect(s);
                        if area(rect) > area(best) && !obstacles.iter().any(|o| overlap(*o, rect)) {
                            best = rect;
                        }
                    }
                }
            }
        }
        best
    }
}

fn area(r: Rect) -> f32 {
    if r.is_positive() { r.width() * r.height() } else { 0. }
}

/// Two rectangles share area (touching edges do not count).
pub fn overlap(a: Rect, b: Rect) -> bool {
    a.min.x < b.max.x - 0.5 && b.min.x < a.max.x - 0.5 && a.min.y < b.max.y - 0.5 && b.min.y < a.max.y - 0.5
}

fn contains(outer: Rect, inner: Rect) -> bool {
    let slack = Pos2::new(0.5, 0.5).to_vec2();
    outer.expand2(slack).contains_rect(inner)
}

fn key() -> Id {
    Id::new("shell_hud_layout")
}

/// Start the HUD layout of a frame: the shell's `controls` on `screen`.
pub fn begin(ctx: &Context, screen: Rect, controls: Vec<Rect>) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let layout = d.get_temp_mut_or_default::<HudLayout>(key());
        *layout = layout.next(screen, controls, pass);
    });
}

/// A control the shell draws after the layout began (it keeps panels out from now on).
pub fn add_control(ctx: &Context, rect: Rect) {
    with(ctx, |layout| layout.controls.push(rect));
}

/// A full screen's opaque thing this frame (a card, a list, a bar the shell paints over
/// the world): map overlays keep out of it like out of a panel. A module's world-showing
/// screen declares its cards here, the shell its bars.
pub fn cover(ctx: &Context, name: &'static str, rect: Rect) {
    with(ctx, |layout| layout.covers.push((name, rect)));
}

/// Whether an opaque control, panel or card stands where `rect` is.
pub fn hides(ctx: &Context, rect: Rect) -> bool {
    with(ctx, |layout| layout.covers(rect))
}

/// A map overlay (a marker, a ring, a label placed on the ground by the camera) asks to be
/// drawn at `rect`: true, and noted for `HudLayout::overlays`, when no control, panel or
/// card stands there. One rule for every overlay of every module (decision D309): it is
/// drawn on `map_painter`'s layer below the HUD, and not at all where the HUD stands, so a
/// translucent panel shows no marker through it either.
pub fn overlay(ctx: &Context, kind: &'static str, rect: Rect) -> bool {
    with(ctx, |layout| {
        let free = !layout.covers(rect);
        if free {
            layout.overlays.push((kind, rect));
        }
        free
    })
}

/// The painter of the map overlays of the module `name`: the Background layer, below every
/// area of the HUD and of the full screens. egui draws a painter-only layer of an order
/// after the areas of that order, so a map overlay on Middle or Foreground lies over the
/// panels whatever order they were drawn in; this is the one layer that cannot.
pub fn map_painter(ctx: &Context, name: &'static str) -> egui::Painter {
    ctx.layer_painter(egui::LayerId::new(egui::Order::Background, Id::new(name))).with_clip_rect(ctx.content_rect())
}

/// The city banners the shell drew this frame (`HudLayout::labels` from the next frame on).
pub fn set_labels(ctx: &Context, labels: Vec<Rect>) {
    with(ctx, |layout| layout.labels = labels);
}

/// The spot of a panel this frame (None: hidden for want of room). Outside a HUD frame
/// (a module screen, a test) the layout of the whole window, without controls, is used.
pub fn place(ctx: &Context, panel: Panel) -> Option<Rect> {
    with(ctx, |layout| layout.place(panel))
}

/// The size an area had in the last frame (a panel whose content sizes it asks for the
/// room it took then).
pub fn last_size(ctx: &Context, area: impl Into<Id>) -> Option<Vec2> {
    let id = area.into();
    ctx.memory(|m| m.area_rect(id)).map(|r| r.size())
}

/// The layout of this frame so far.
pub fn current(ctx: &Context) -> HudLayout {
    with(ctx, |layout| layout.clone())
}

/// The layout as the last frame left it (read after a frame: tests and tools).
pub fn last(ctx: &Context) -> HudLayout {
    ctx.data(|d| d.get_temp::<HudLayout>(key())).unwrap_or_default()
}

/// The shell's areas over the map (`hud`, `screens`), by name.
const SHELL_AREAS: [&str; 6] = ["visual_top", "visual_minimap", "visual_map_controls", "visual_end_turn", "screen_navigation", "visual_research"];

/// What the last frame showed over the map, by name: the areas egui drew for the shell's
/// controls and the laid-out panels (their real rects, not the layout's), then `marks`
/// (labels and badges on the map, the top bar's items).
pub fn shown(ctx: &Context, marks: &[(String, Rect)]) -> Vec<(String, Rect)> {
    let layout = last(ctx);
    let names = SHELL_AREAS.iter().copied().chain(layout.placed().iter().map(|p| p.name));
    let visible = ctx.memory(|m| m.areas().visible_layer_ids());
    let mut shown: Vec<(String, Rect)> = names
        .filter_map(|name| {
            let layer = visible.iter().find(|l| l.id == Id::new(name))?;
            Some((name.to_string(), ctx.memory(|m| m.area_rect(layer.id))?))
        })
        .collect();
    shown.extend(marks.iter().cloned());
    shown
}

/// The share (0..=1) of `screen` the panels of `shown` cover together: the union of their
/// rects, marks on the map (names with a ':', the city labels and unit badges) left out.
pub fn covered_share(shown: &[(String, Rect)], screen: Rect) -> f32 {
    let rects: Vec<Rect> = shown.iter().filter(|(name, _)| !name.contains(':')).map(|(_, r)| r.intersect(screen)).filter(|r| r.is_positive()).collect();
    let mut xs: Vec<f32> = rects.iter().flat_map(|r| [r.left(), r.right()]).collect();
    xs.sort_by(f32::total_cmp);
    xs.dedup();
    let mut area = 0.;
    for slab in xs.windows(2) {
        let (from, to) = (slab[0], slab[1]);
        let mut spans: Vec<(f32, f32)> = rects.iter().filter(|r| r.left() <= from && r.right() >= to).map(|r| (r.top(), r.bottom())).collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut covered = 0.;
        let mut reach = f32::MIN;
        for (top, bottom) in spans {
            let top = top.max(reach);
            if bottom > top {
                covered += bottom - top;
            }
            reach = reach.max(bottom);
        }
        area += covered * (to - from);
    }
    area / screen.area().max(1.)
}

/// The overlaps among `shown`, as lines to read, and the panels that overflowed. Things
/// inside the top bar are its items, not overlaps with it; marks of one kind (the text
/// before ':', the top bar's items excepted) may touch each other: two cities close by.
pub fn shown_overlaps(ctx: &Context, shown: &[(String, Rect)]) -> Vec<String> {
    let top = shown.iter().find(|(n, _)| n == "visual_top").map(|(_, r)| *r);
    let kind = |name: &str| name.split(':').next().unwrap_or(name).to_owned();
    let in_top = |n: &str, r: &Rect| n == "visual_top" || top.is_some_and(|t| t.contains_rect(*r));
    let mut found = Vec::new();
    for (i, (a, ra)) in shown.iter().enumerate() {
        for (b, rb) in &shown[i + 1..] {
            let top_item = (a == "visual_top" && in_top(b, rb)) || (b == "visual_top" && in_top(a, ra));
            let same_kind = a.contains(':') && kind(a) == kind(b) && kind(a) != "top";
            if overlap(*ra, *rb) && !top_item && !same_kind {
                found.push(format!("{a} {ra:?} × {b} {rb:?}"));
            }
        }
    }
    found.extend(last(ctx).overflow().iter().map(|id| format!("overflow: {id:?}")));
    found
}

fn with<R>(ctx: &Context, f: impl FnOnce(&mut HudLayout) -> R) -> R {
    let pass = ctx.cumulative_pass_nr();
    let screen = ctx.content_rect();
    ctx.data_mut(|d| {
        let layout = d.get_temp_mut_or_default::<HudLayout>(key());
        if layout.pass != pass || layout.screen == Rect::ZERO {
            *layout = layout.next(screen, Vec::new(), pass);
        }
        f(layout)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1280., 720.))
    }

    fn top_bar() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1280., 40.))
    }

    fn no_overlaps(layout: &HudLayout) {
        let rects: Vec<Rect> = layout.controls().iter().copied().chain(layout.placed().iter().map(|p| p.rect)).collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(contains(screen(), *a), "{a:?} leaves the window");
            for b in &rects[i + 1..] {
                assert!(!overlap(*a, *b), "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn panels_of_one_anchor_stack_away_from_it() {
        let mut layout = HudLayout::new(screen(), vec![top_bar()]);
        let a = layout.place(Panel::new("a", Anchor::TopRight, vec2(300., 100.))).unwrap();
        let b = layout.place(Panel::new("b", Anchor::TopRight, vec2(300., 100.))).unwrap();
        assert_eq!(a.right_top(), pos2(1268., 40. + GAP));
        assert_eq!(b.top(), a.bottom() + GAP);
        assert_eq!(b.right(), a.right());
        let c = layout.place(Panel::new("c", Anchor::BottomLeft, vec2(200., 80.))).unwrap();
        let d = layout.place(Panel::new("d", Anchor::BottomLeft, vec2(200., 80.))).unwrap();
        assert_eq!(c.left_bottom(), pos2(12., 708.));
        assert_eq!(d.bottom(), c.top() - GAP);
        no_overlaps(&layout);
    }

    #[test]
    fn a_panel_keeps_its_own_home_while_it_is_free() {
        let mut layout = HudLayout::new(screen(), vec![top_bar()]);
        let home = Rect::from_min_size(pos2(0., 59.), vec2(249., 87.));
        assert_eq!(layout.place(Panel::at("research", Anchor::TopLeft, home)), Some(home));
    }

    #[test]
    fn panels_go_around_controls() {
        let buttons = Rect::from_min_size(pos2(19., 645.), vec2(254., 55.));
        let mut layout = HudLayout::new(screen(), vec![top_bar(), buttons]);
        let home = Rect::from_min_size(pos2(16., 596.), vec2(420., 106.));
        let unit = layout.place(Panel::at("unit", Anchor::BottomLeft, home)).unwrap();
        assert_eq!(unit, home.translate(vec2(0., buttons.top() - GAP - home.bottom())));
        no_overlaps(&layout);
    }

    #[test]
    fn short_room_shrinks_then_hides_the_least_important() {
        let minimap = Rect::from_min_max(pos2(1019., 447.), pos2(1264., 703.));
        let mut layout = HudLayout::new(screen(), vec![top_bar(), minimap]);
        let agenda = layout.place(Panel::new("agenda", Anchor::Bottom, vec2(440., 236.)).priority(80)).unwrap();
        let events = Panel::new("events", Anchor::Bottom, vec2(460., 170.)).min(vec2(240., 170.)).priority(40);
        let shrunk = layout.place(events).expect("room right of the agenda");
        assert!(shrunk.width() < 460. && shrunk.width() >= 240. && !overlap(shrunk, agenda), "{shrunk:?}");
        // A second wide panel finds no room on the bottom half: it hides.
        assert_eq!(layout.place(Panel::new("more", Anchor::Bottom, vec2(600., 200.)).collapsible()), None);
        assert_eq!(layout.hidden(), &[Id::new("more")]);
        // One that may not hide is shown at its least size and reported.
        layout.place(Panel::new("must", Anchor::Bottom, vec2(600., 200.)));
        assert_eq!(layout.overflow(), &[Id::new("must")]);
    }

    #[test]
    fn a_panel_drawn_later_keeps_its_place_when_it_matters_more() {
        let mut layout = HudLayout::new(screen(), vec![top_bar()]);
        let low = Panel::new("low", Anchor::BottomLeft, vec2(300., 100.)).priority(10);
        let high = Panel::new("high", Anchor::BottomLeft, vec2(300., 100.)).priority(90);
        for _ in 0..3 {
            layout = layout.next(screen(), vec![top_bar()], 0);
            layout.place(low);
            layout.place(high);
        }
        let home = Anchor::BottomLeft.home(screen().shrink(MARGIN), vec2(300., 100.));
        assert_eq!(layout.rect("high"), Some(home));
        assert_eq!(layout.rect("low").unwrap().bottom(), home.top() - GAP);
        no_overlaps(&layout);
    }

    #[test]
    fn the_free_map_avoids_every_panel_and_control() {
        let mut layout = HudLayout::new(screen(), vec![top_bar()]);
        layout.place(Panel::new("left", Anchor::Left, vec2(200., 300.)));
        layout.place(Panel::new("bottom", Anchor::Bottom, vec2(400., 100.)));
        let free = layout.free_map();
        assert!(!layout.covers(free));
        assert!(free.width() * free.height() > 1280. * 720. * 0.5, "{free:?}");
    }

    #[test]
    fn covered_share_counts_the_union_and_skips_marks() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(100., 100.));
        let at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(pos2(x, y), vec2(w, h));
        // Two panels of 50 × 50 that share a 25 × 25 corner: the union is 2500 + 2500 - 625.
        let shown = [("a".to_string(), at(0., 0., 50., 50.)), ("b".to_string(), at(25., 25., 50., 50.))];
        let share = covered_share(&shown, screen);
        assert!((share - 0.4375).abs() < 1e-4, "{share}");
        // A mark (a name with ':') on the map is not a panel.
        let marked = [shown[0].clone(), ("banner:".to_string(), at(60., 60., 40., 40.))];
        assert!((covered_share(&marked, screen) - 0.25).abs() < 1e-4);
        // What leaves the window counts only inside it; nothing shown covers nothing.
        assert!((covered_share(&[("c".to_string(), at(90., 90., 50., 50.))], screen) - 0.01).abs() < 1e-4);
        assert_eq!(covered_share(&[], screen), 0.);
    }

    #[test]
    fn the_check_reads_what_egui_drew() {
        let ctx = Context::default();
        let raw = egui::RawInput { screen_rect: Some(screen()), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            for (name, at) in [("visual_minimap", pos2(100., 100.)), ("visual_map_controls", pos2(150., 150.))] {
                egui::Area::new(name.into()).fixed_pos(at).show(ui.ctx(), |ui| ui.allocate_exact_size(vec2(100., 100.), egui::Sense::hover()));
            }
        });
        output.textures_delta.clear();
        let marks = [("badge:".to_string(), Rect::from_min_size(pos2(500., 500.), vec2(20., 20.)))];
        let shown = shown(&ctx, &marks);
        assert_eq!(shown.len(), 3, "{shown:?}");
        let overlaps = shown_overlaps(&ctx, &shown);
        assert_eq!(overlaps.len(), 1, "{overlaps:?}");
        assert!(overlaps[0].starts_with("visual_minimap"));
    }

    #[test]
    fn context_layout_lives_one_frame_and_remembers_the_plan() {
        let ctx = Context::default();
        let raw = || egui::RawInput { screen_rect: Some(screen()), ..Default::default() };
        let mut spots = Vec::new();
        for _ in 0..3 {
            let mut output = ctx.run_ui(raw(), |ui| {
                begin(ui.ctx(), screen(), vec![top_bar()]);
                place(ui.ctx(), Panel::new("low", Anchor::TopLeft, vec2(200., 100.)).priority(1));
                place(ui.ctx(), Panel::new("high", Anchor::TopLeft, vec2(200., 100.)).priority(2));
                spots.push(current(ui.ctx()).rect("high"));
            });
            output.textures_delta.clear();
        }
        let alone = HudLayout::new(screen(), vec![top_bar()]).place(Panel::new("high", Anchor::TopLeft, vec2(200., 100.)));
        assert_ne!(spots[0], alone, "the first frame has no plan yet");
        assert_eq!(spots[2], alone);
    }

    /// The fill colours of the rectangles of a frame, from the bottom layer to the top.
    fn fills(output: &egui::FullOutput) -> Vec<egui::Color32> {
        output.shapes.iter().filter_map(|s| if let egui::epaint::Shape::Rect(r) = &s.shape { Some(r.fill) } else { None }).collect()
    }

    fn frame(ctx: &Context, mut draw: impl FnMut(&Context)) -> egui::FullOutput {
        let raw = || egui::RawInput { screen_rect: Some(screen()), ..Default::default() };
        // Areas size themselves on their first frame, and fade in unless the animation is off.
        ctx.all_styles_mut(|style| style.animation_time = 0.);
        let mut output = ctx.run_ui(raw(), |ui| draw(ui.ctx()));
        output.textures_delta.clear();
        for _ in 0..2 {
            output = ctx.run_ui(raw(), |ui| draw(ui.ctx()));
            output.textures_delta.clear();
        }
        output
    }

    /// The cause of the defect of 19 October ("drawn over the gui"): egui draws a painter-only
    /// layer of an order after every area of that order, so a map overlay on Middle or
    /// Foreground lies over the panels however they were drawn. `map_painter` is the layer
    /// below all of them.
    #[test]
    fn a_map_overlay_lies_below_every_area_and_a_foreground_painter_above() {
        use egui::{Color32, Order};
        let (mark, wrong, middle, foreground) = (Color32::from_rgb(1, 2, 3), Color32::from_rgb(4, 5, 6), Color32::from_rgb(7, 8, 9), Color32::from_rgb(10, 11, 12));
        let spot = Rect::from_min_size(pos2(100., 100.), vec2(200., 100.));
        let ctx = Context::default();
        let output = frame(&ctx, |ctx| {
            for (name, order, fill) in [("a_middle_panel", Order::Middle, middle), ("a_foreground_card", Order::Foreground, foreground)] {
                egui::Area::new(name.into()).order(order).fixed_pos(spot.min).show(ctx, |ui| {
                    ui.allocate_exact_size(spot.size(), egui::Sense::hover());
                    ui.painter().rect_filled(spot, 0., fill);
                });
            }
            map_painter(ctx, "a_map_overlay").rect_filled(spot, 0., mark);
            ctx.layer_painter(egui::LayerId::new(Order::Foreground, "a_foreground_painter".into())).rect_filled(spot, 0., wrong);
        });
        let order = fills(&output);
        let at = |c: Color32| order.iter().position(|x| *x == c).unwrap_or_else(|| panic!("{c:?} not drawn: {order:?}"));
        assert!(at(mark) < at(middle) && at(mark) < at(foreground), "the map overlay is under the panel and the card: {order:?}");
        assert!(at(wrong) > at(foreground), "a Foreground painter-only layer is above the card: that was the defect: {order:?}");
    }

    #[test]
    fn an_overlay_is_refused_where_a_panel_or_a_card_stands_and_noted_where_it_is_drawn() {
        let ctx = Context::default();
        let panel = Rect::from_min_size(pos2(900., 100.), vec2(300., 200.));
        let card = Rect::from_min_size(pos2(100., 400.), vec2(300., 200.));
        let spot = |x: f32, y: f32| Rect::from_center_size(pos2(x, y), vec2(24., 24.));
        let mut verdicts = Vec::new();
        let output = frame(&ctx, |ctx| {
            verdicts.clear();
            begin(ctx, screen(), vec![top_bar()]);
            place(ctx, Panel::at("panel", Anchor::TopRight, panel));
            cover(ctx, "card", card);
            for rect in [spot(1000., 150.), spot(200., 500.), spot(600., 300.), spot(20., 20.), spot(880., 150.)] {
                verdicts.push(overlay(ctx, "mark", rect));
            }
        });
        drop(output);
        assert_eq!(verdicts, [false, false, true, false, true], "under the panel, under the card, free, under the top bar, beside the panel");
        let drawn: Vec<Rect> = last(&ctx).overlays().iter().map(|(_, r)| *r).collect();
        assert_eq!(drawn, [spot(600., 300.), spot(880., 150.)]);
        assert_eq!(last(&ctx).covers_declared().len(), 1);
    }
}
