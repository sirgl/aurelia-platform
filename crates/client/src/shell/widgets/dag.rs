//! A graph of cards with prerequisites (technologies, laws): nodes on a grid of columns
//! and rows, column bands with jump buttons, edges routed between the cards, and the
//! research queue as numbered badges. The module places the nodes (from its content, or
//! by depth with `layered`) and says each one's progress; the widget draws, scrolls and
//! reports clicks and hovers. The words and the tooltips stay the module's.
use super::cards::{self, ImageCard};
use crate::shell::art::ArtPen;
use crate::shell::kit::{text, BG, GOLD, MUTED, RAISED, TEAL};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};

/// The size of a node card.
pub const CARD: egui::Vec2 = egui::Vec2::new(218., 62.);
/// The distance between columns and between rows.
pub const COLUMN: f32 = 282.;
pub const ROW: f32 = 72.;
const LEFT: f32 = 18.;
const TOP: f32 = 38.;

/// A node's place on the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub column: usize,
    pub row: usize,
}

impl Cell {
    /// The card's rectangle on the graph's canvas.
    pub fn rect(self) -> Rect {
        Rect::from_min_size(pos2(LEFT + self.column as f32 * COLUMN, TOP + self.row as f32 * ROW), CARD)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Done,
    /// In the queue at this place (1 = studied now).
    Queued(u32),
    Open,
}

/// The node's first unlock in the card's corner: its picture or initial, and how many more.
#[derive(Debug, Clone, PartialEq)]
pub struct Corner {
    pub art: Option<String>,
    pub initial: char,
    pub more: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub cell: Cell,
    /// Indexes of the nodes this one needs; each sits in an earlier column.
    pub requires: Vec<usize>,
    /// The band (era) its column belongs to.
    pub band: String,
    pub progress: Progress,
    /// A click acts now: the card lights up on hover.
    pub ready: bool,
    /// The picture, a title and one status line (`cards::NODE`).
    pub card: ImageCard,
    pub corner: Option<Corner>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    /// Salt of the scroll state: one per graph screen.
    pub id: &'static str,
    pub nodes: Vec<Node>,
}

/// The selected node and a scroll asked for the next frame; the module keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphState {
    pub selected: Option<usize>,
    jump: Option<usize>,
}

impl GraphState {
    /// Select a node and scroll its column into view on the next frame.
    pub fn focus(&mut self, node: usize) {
        self.selected = Some(node);
        self.jump = Some(node);
    }
}

/// A drawn card: hover it for a tooltip, mark it for a reference page.
#[derive(Debug)]
pub struct NodeResponse {
    pub node: usize,
    pub response: egui::Response,
}

#[derive(Debug, Default)]
pub struct GraphResponse {
    /// The node clicked this frame (it is selected; acting on it is the module's call).
    pub clicked: Option<usize>,
    /// The cards drawn this frame (the visible ones).
    pub cards: Vec<NodeResponse>,
}

/// The bands in column order: label, first and last column.
pub fn bands(graph: &Graph) -> Vec<(&str, usize, usize)> {
    let mut bands: Vec<(&str, usize, usize)> = Vec::new();
    for node in &graph.nodes {
        let column = node.cell.column;
        if let Some(band) = bands.iter_mut().find(|b| b.0 == node.band) {
            band.1 = band.1.min(column);
            band.2 = band.2.max(column);
        } else {
            bands.push((&node.band, column, column));
        }
    }
    bands.sort_by_key(|b| b.1);
    bands
}

/// The canvas width up to the last column.
pub fn canvas_width(last_column: usize) -> f32 {
    last_column as f32 * COLUMN + CARD.x + 36.
}

/// Places by depth for a module whose content has no grid: a node's column is one past
/// its deepest prerequisite, its row the count of nodes before it in that column.
/// `requires` must be acyclic.
pub fn layered(requires: &[Vec<usize>]) -> Vec<Cell> {
    fn depth(node: usize, requires: &[Vec<usize>], memo: &mut [Option<usize>]) -> usize {
        if let Some(column) = memo[node] {
            return column;
        }
        let column = requires[node].iter().map(|&p| depth(p, requires, memo) + 1).max().unwrap_or(0);
        memo[node] = Some(column);
        column
    }
    let mut memo = vec![None; requires.len()];
    let mut rows: Vec<usize> = Vec::new();
    (0..requires.len()).map(|node| {
        let column = depth(node, requires, &mut memo);
        if rows.len() <= column {
            rows.resize(column + 1, 0);
        }
        rows[column] += 1;
        Cell { column, row: rows[column] - 1 }
    }).collect()
}

/// The path of the edge `order` into `to` from `from`: out of the right side, along the
/// gap between rows, into the left side; never through a card.
pub fn route(from: Cell, to: Cell, order: usize) -> Vec<egui::Pos2> {
    let start = from.rect().right_center();
    let end = to.rect().left_center();
    let enter = end.x - 20. - (order % 3) as f32 * 4.;
    if to.column == from.column + 1 {
        return vec![start, pos2(enter, start.y), pos2(enter, end.y), end];
    }
    let lane = TOP + (((start.y + end.y) * 0.5 - TOP) / ROW).round() * ROW - (ROW - CARD.y) * 0.5;
    vec![start, pos2(start.x + 14., start.y), pos2(start.x + 14., lane), pos2(enter, lane), pos2(enter, end.y), end]
}

/// The band buttons on top of `area` and the scrolled graph under them.
pub fn show(ui: &mut egui::Ui, area: Rect, graph: &Graph, state: &mut GraphState, pen: &mut ArtPen) -> GraphResponse {
    let column_x = |node: usize| graph.nodes.get(node).map(|n| n.cell.column as f32 * COLUMN);
    let mut jump = state.jump.take().and_then(column_x);
    let buttons = Rect::from_min_size(area.min + vec2(4., 0.), vec2(area.width() - 8., 28.));
    ui.scope_builder(egui::UiBuilder::new().max_rect(buttons), |ui| {
        ui.horizontal_wrapped(|ui| {
            for (band, first, _) in bands(graph) {
                if ui.small_button(band).clicked() {
                    jump = Some(first as f32 * COLUMN);
                }
            }
        });
    });
    let body = Rect::from_min_max(area.min + vec2(0., 34.), area.max);
    let mut response = GraphResponse::default();
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        ui.set_clip_rect(body.intersect(ui.clip_rect()));
        let mut scroll = egui::ScrollArea::both().id_salt(graph.id).auto_shrink([false, false])
            .scroll_source(egui::scroll_area::ScrollSource::ALL);
        if let Some(x) = jump {
            scroll = scroll.horizontal_scroll_offset(x);
        }
        response = scroll.show(ui, |ui| draw_canvas(ui, graph, state, pen)).inner;
    });
    response
}

fn draw_canvas(ui: &mut egui::Ui, graph: &Graph, state: &mut GraphState, pen: &mut ArtPen) -> GraphResponse {
    let last = |f: fn(&Cell) -> usize| graph.nodes.iter().map(|n| f(&n.cell)).max().unwrap_or(0);
    let width = canvas_width(last(|c| c.column));
    let height = last(|c| c.row) as f32 * ROW + CARD.y + TOP + 14.;
    let (canvas, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    draw_bands(ui, graph, canvas);
    draw_edges(ui, graph, canvas.min.to_vec2());
    let mut response = GraphResponse::default();
    for (index, node) in graph.nodes.iter().enumerate() {
        let r = node.cell.rect().translate(canvas.min.to_vec2());
        if ui.is_rect_visible(r) {
            let card = draw_node(ui, r, graph.id, index, node, state.selected == Some(index), pen);
            if card.clicked() {
                state.selected = Some(index);
                response.clicked = Some(index);
            }
            response.cards.push(NodeResponse { node: index, response: card });
        }
    }
    response
}

fn draw_bands(ui: &egui::Ui, graph: &Graph, canvas: Rect) {
    for (index, (band, first, last)) in bands(graph).iter().enumerate() {
        let x = *first as f32 * COLUMN;
        let right = ((*last + 1) as f32 * COLUMN).min(canvas.width());
        let r = Rect::from_min_max(canvas.min + vec2(x, 0.), canvas.min + vec2(right, canvas.height()));
        let fill = if index % 2 == 0 { Color32::from_rgb(14, 33, 39) } else { Color32::from_rgb(19, 39, 45) };
        ui.painter().rect_filled(r, 0., fill);
        text(ui.painter(), r.min + vec2(18., 10.), band.to_uppercase(), 12., GOLD);
        ui.painter().vline(r.left(), r.y_range(), Stroke::new(1., Color32::from_rgb(43, 62, 66)));
    }
}

fn queued(progress: Progress) -> bool {
    matches!(progress, Progress::Queued(_))
}

/// Gold along the queue, teal out of what is done, grey elsewhere.
fn draw_edges(ui: &egui::Ui, graph: &Graph, origin: egui::Vec2) {
    for node in &graph.nodes {
        for (order, &parent) in node.requires.iter().enumerate() {
            let Some(source) = graph.nodes.get(parent) else { continue };
            let planned = queued(node.progress) && (queued(source.progress) || source.progress == Progress::Done);
            let color = if planned {
                GOLD
            } else if source.progress == Progress::Done {
                TEAL
            } else {
                Color32::from_rgb(88, 109, 112)
            };
            let points: Vec<_> = route(source.cell, node.cell, order).into_iter().map(|p| p + origin).collect();
            let tip = *points.last().expect("a route has an end");
            ui.painter().add(egui::Shape::line(points, Stroke::new(if planned { 2.3 } else { 1.3 }, color)));
            ui.painter().add(egui::Shape::convex_polygon(vec![tip, tip - vec2(6., 3.), tip - vec2(6., -3.)], color, Stroke::NONE));
        }
    }
}

fn draw_node(ui: &mut egui::Ui, r: Rect, salt: &str, index: usize, node: &Node, selected: bool, pen: &mut ArtPen) -> egui::Response {
    let response = ui.interact(r, ui.id().with((salt, index)), Sense::click());
    let planned = queued(node.progress);
    let fill = match node.progress {
        Progress::Done => Color32::from_rgb(26, 53, 45),
        Progress::Queued(_) => Color32::from_rgb(57, 54, 35),
        Progress::Open => RAISED,
    };
    let lit = planned || selected || (response.hovered() && node.ready);
    let edge = if lit { GOLD } else { Color32::from_rgb(69, 93, 96) };
    let width = if selected || response.hovered() { 1.8 } else { 1. };
    ui.painter().rect_filled(r, 5., fill);
    ui.painter().rect_stroke(r, 5., Stroke::new(width, edge), egui::StrokeKind::Inside);
    cards::paint(ui, r, &node.card, &cards::NODE, Some(pen));
    if let Some(corner) = &node.corner {
        draw_corner(ui, r, corner, pen);
    }
    if let Progress::Queued(place) = node.progress {
        let center = pos2(r.right() - 13., r.top() + 14.);
        ui.painter().circle_filled(center, 9., GOLD);
        ui.painter().text(center, Align2::CENTER_CENTER, place.to_string(), FontId::proportional(10.), BG);
    }
    response
}

fn draw_corner(ui: &egui::Ui, r: Rect, corner: &Corner, pen: &mut ArtPen) {
    let slot = Rect::from_min_size(pos2(r.right() - 49., r.bottom() - 27.), vec2(19., 19.));
    if let Some(art) = &corner.art {
        pen.paint(ui, slot, art);
    } else {
        ui.painter().rect_filled(slot, 3., Color32::from_rgb(45, 66, 70));
        ui.painter().text(slot.center(), Align2::CENTER_CENTER, corner.initial, FontId::proportional(11.), GOLD);
    }
    if corner.more > 0 {
        text(ui.painter(), pos2(r.right() - 26., r.bottom() - 24.), format!("+{}", corner.more), 10., MUTED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::art::ScreenArt;
    use crate::shell::widgets::cards::{CardLine, Picture};

    fn node(cell: Cell, requires: Vec<usize>, progress: Progress) -> Node {
        Node {
            cell,
            requires,
            band: if cell.column < 2 { "Ancient" } else { "Classical" }.into(),
            progress,
            ready: progress == Progress::Open,
            card: ImageCard { picture: Picture::Art("missing".into()), lines: vec![CardLine::new("Node", GOLD)], bar: None },
            corner: None,
        }
    }

    #[test]
    fn layered_places_each_node_one_column_past_its_deepest_prerequisite() {
        let requires = vec![vec![], vec![0], vec![], vec![1, 2], vec![0]];
        let cells = layered(&requires);
        let columns: Vec<_> = cells.iter().map(|c| c.column).collect();
        assert_eq!(columns, [0, 1, 0, 2, 1]);
        let rows: Vec<_> = cells.iter().map(|c| c.row).collect();
        assert_eq!(rows, [0, 0, 1, 0, 1]);
    }

    #[test]
    fn routes_leave_right_enter_left_and_skip_the_cards_between() {
        let requires = vec![vec![], vec![], vec![0], vec![0, 1], vec![2], vec![0, 3]];
        let cells = layered(&requires);
        for (target, parents) in requires.iter().enumerate() {
            for (order, &parent) in parents.iter().enumerate() {
                let points = route(cells[parent], cells[target], order);
                assert_eq!(points[0], cells[parent].rect().right_center());
                assert_eq!(*points.last().unwrap(), cells[target].rect().left_center());
                for segment in points.windows(2) {
                    for cell in &cells {
                        let r = cell.rect().shrink(0.01);
                        let (a, b) = (segment[0], segment[1]);
                        let vertical = a.x == b.x && r.x_range().contains(a.x) && a.y.min(b.y) < r.bottom() && a.y.max(b.y) > r.top();
                        let horizontal = a.y == b.y && r.y_range().contains(a.y) && a.x.min(b.x) < r.right() && a.x.max(b.x) > r.left();
                        assert!(!vertical && !horizontal, "{parent} -> {target} crosses {cell:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_click_selects_the_node_and_reports_it_once() {
        let graph = Graph {
            id: "test_graph",
            nodes: vec![
                node(Cell { column: 0, row: 0 }, vec![], Progress::Done),
                node(Cell { column: 1, row: 0 }, vec![0], Progress::Queued(1)),
                node(Cell { column: 2, row: 1 }, vec![1], Progress::Open),
            ],
        };
        let ctx = egui::Context::default();
        let mut state = GraphState::default();
        let mut art = ScreenArt::default();
        let area = Rect::from_min_size(pos2(0., 0.), vec2(960., 400.));
        let mut frame = |events: Vec<egui::Event>| {
            let mut result = None;
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(area), events, ..Default::default() }, |ui| {
                let mut pen = ArtPen { art: &mut art, tint: Color32::WHITE };
                let response = show(ui, area, &graph, &mut state, &mut pen);
                let rects: Vec<_> = response.cards.iter().map(|c| (c.node, c.response.rect)).collect();
                result = Some((response.clicked, rects));
            });
            output.textures_delta.clear();
            result.unwrap()
        };
        let (_, rects) = frame(vec![]);
        assert_eq!(rects.len(), 3, "all cards are visible");
        let at = rects[2].1.center();
        frame(vec![egui::Event::PointerMoved(at)]);
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame(vec![press(true)]);
        let (clicked, _) = frame(vec![press(false)]);
        assert_eq!(clicked, Some(2));
        assert_eq!(frame(vec![]).0, None, "reported once");
        assert_eq!(state.selected, Some(2));
    }

    #[test]
    fn bands_follow_columns_in_order() {
        let graph = Graph {
            id: "bands",
            nodes: vec![
                node(Cell { column: 3, row: 0 }, vec![], Progress::Open),
                node(Cell { column: 0, row: 0 }, vec![], Progress::Open),
                node(Cell { column: 1, row: 2 }, vec![], Progress::Open),
            ],
        };
        assert_eq!(bands(&graph), [("Ancient", 0, 1), ("Classical", 3, 3)]);
    }
}
