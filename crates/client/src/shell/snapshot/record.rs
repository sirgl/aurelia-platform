//! The text of one frame. Three parts, each in a stable order:
//!
//! - `widgets`: what a player can act on (egui's accessibility tree): role, label (or the
//!   texts painted inside it), place and state, nested as the widgets are;
//! - `painted`: every visible shape in paint order under its clip rectangle — texts with
//!   color and size, rectangles, pictures by texture name. Vector drawings (consecutive
//!   circles, lines and paths: icons, ornaments) read as one `figure` line with their box
//!   and colors, and a long run of rectangles (a minimap, a grid of cells) as one `rects`
//!   line with a count per color;
//! - `art`: the pictures the frame asked for while art stays unloaded (`art::stay_offline`).
//!
//! Places are in points, rounded to whole points (`x,y wxh`); colors are `#rrggbb`
//! (`#rrggbbaa` when translucent). Rounding keeps sub-point layout noise out; anything a
//! player could see move, resize or recolor still changes a line.
use egui::accesskit::{self, Action, Node, NodeId, Role, Toggled};
use egui::epaint::{ClippedShape, ColorMode, Mesh, PathStroke, RectShape, TextShape};
use egui::{Color32, Pos2, Rect, Shape, Stroke, TextureId};
use std::collections::HashMap;
use std::fmt::Write;

/// Characters of a widget's name at most (longer names are cut with `…`).
const NAME_LIMIT: usize = 80;
/// Consecutive rectangles from which they read as one `rects` line.
const RECT_RUN: usize = 8;
/// Colors a summary line lists at most.
const COLOR_LIMIT: usize = 6;

/// The snapshot text of a frame's output (`ctx` names its textures and holds the art
/// requests, which this call takes).
pub fn describe(ctx: &egui::Context, output: &egui::FullOutput) -> String {
    let painted = painted(ctx, &output.shapes);
    let mut out = String::new();
    if let Some(tree) = &output.platform_output.accesskit_update {
        out.push_str("widgets\n");
        Widgets::new(tree, &painted).write(&mut out);
    }
    out.push_str("painted\n");
    let mut clip = None;
    for run in runs(&painted) {
        if clip != Some(run[0].clip) {
            clip = Some(run[0].clip);
            writeln!(out, "  clip {}", place(run[0].clip)).unwrap();
        }
        writeln!(out, "    {}", run_line(run)).unwrap();
    }
    let art = super::super::art::take_offline_requests(ctx);
    if !art.is_empty() {
        out.push_str("art\n");
        art.iter().for_each(|name| writeln!(out, "  {name}").unwrap());
    }
    out
}

/// What a visible shape is to the snapshot.
#[derive(Debug, Clone, PartialEq)]
enum Kind {
    /// A text and where it reads (widgets take their names from the texts inside them).
    Text(Rect, String),
    Rect,
    Picture,
    /// A circle, line, path or curve: a stroke of a vector drawing.
    Figure,
}

/// One visible shape: its line, clip, kind, box and main color.
struct Painted {
    clip: Rect,
    line: String,
    kind: Kind,
    bounds: Rect,
    color: Color32,
}

/// The painted shapes cut into the runs that print as one line each: a whole vector
/// drawing, a long run of rectangles, else a single shape.
fn runs(painted: &[Painted]) -> Vec<&[Painted]> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < painted.len() {
        let first = &painted[start];
        let same = |item: &Painted| item.clip == first.clip && item.kind == first.kind;
        let length = painted[start..].iter().take_while(|item| same(item)).count();
        let grouped = match first.kind {
            Kind::Figure => length,
            Kind::Rect if length >= RECT_RUN => length,
            _ => 1,
        };
        runs.push(&painted[start..start + grouped]);
        start += grouped;
    }
    runs
}

fn run_line(run: &[Painted]) -> String {
    if run.len() == 1 {
        return run[0].line.clone();
    }
    let bounds = run.iter().fold(Rect::NOTHING, |all, item| all.union(item.bounds));
    let mut counts: Vec<(Color32, usize)> = Vec::new();
    for item in run {
        match counts.iter_mut().find(|(color, _)| *color == item.color) {
            Some((_, count)) => *count += 1,
            None => counts.push((item.color, 1)),
        }
    }
    if run[0].kind == Kind::Figure {
        let colors: Vec<_> = counts.iter().take(COLOR_LIMIT).map(|(color, _)| color_text(*color)).collect();
        return format!("figure {} shapes {} {}", run.len(), place(bounds), colors.join(" "));
    }
    // Most frequent first; equal counts by color, so the line does not depend on order.
    counts.sort_by_key(|(color, count)| (std::cmp::Reverse(*count), color.to_array()));
    let colors: Vec<_> = counts.iter().take(COLOR_LIMIT).map(|(color, count)| format!("{count} {}", color_text(*color))).collect();
    let more = counts.len().saturating_sub(COLOR_LIMIT);
    let rest = if more > 0 { format!(" +{more} colors") } else { String::new() };
    format!("rects {} {}: {}{rest}", run.len(), place(bounds), colors.join(", "))
}

fn painted(ctx: &egui::Context, shapes: &[ClippedShape]) -> Vec<Painted> {
    let textures = ctx.tex_manager();
    let textures = textures.read();
    let name = |id: TextureId| textures.meta(id).map_or_else(|| format!("{id:?}"), |meta| meta.name.clone());
    let mut items = Vec::new();
    for clipped in shapes {
        let clip = rounded(clipped.clip_rect);
        flatten(&clipped.shape, &mut |shape| {
            if let Some(item) = paint_line(shape, clip, &name) {
                items.push(item);
            }
        });
    }
    items
}

fn flatten(shape: &Shape, visit: &mut dyn FnMut(&Shape)) {
    match shape {
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| flatten(shape, visit)),
        shape => visit(shape),
    }
}

/// The line of a shape; None when nothing of it shows (transparent, empty or clipped away).
fn paint_line(shape: &Shape, clip: Rect, texture: &dyn Fn(TextureId) -> String) -> Option<Painted> {
    let bounds = shape.visual_bounding_rect();
    if !bounds.is_finite() || !bounds.intersects(clip) {
        return None;
    }
    let (line, kind, color) = match shape {
        Shape::Noop | Shape::Vec(_) | Shape::Callback(_) => return None,
        Shape::Text(shape) => {
            let (line, color, read) = text_line(shape)?;
            (line, read, color)
        }
        Shape::Rect(rect) => rect_line(rect, texture)?,
        Shape::Mesh(mesh) => (mesh_line(mesh, bounds, texture)?, Kind::Picture, Color32::WHITE),
        shape => {
            let (line, color) = figure_line(shape, bounds)?;
            (line, Kind::Figure, color)
        }
    };
    Some(Painted { clip, line, kind, bounds, color })
}

/// A circle, ellipse, line, path or curve; None when it does not show.
fn figure_line(shape: &Shape, bounds: Rect) -> Option<(String, Color32)> {
    let (line, fill, stroke) = match shape {
        Shape::Circle(circle) => {
            let look = look(circle.fill, circle.stroke)?;
            (format!("circle {} r{}{look}", point(circle.center), number(circle.radius)), circle.fill, circle.stroke.color)
        }
        Shape::Ellipse(ellipse) => (format!("ellipse {}{}", place(bounds), look(ellipse.fill, ellipse.stroke)?), ellipse.fill, ellipse.stroke.color),
        Shape::LineSegment { points, stroke } => {
            let look = look(Color32::TRANSPARENT, *stroke)?;
            (format!("line {} -> {}{look}", point(points[0]), point(points[1])), Color32::TRANSPARENT, stroke.color)
        }
        Shape::Path(path) => {
            let look = path_look(path.fill, &path.stroke)?;
            (format!("path {}pt {}{look}", path.points.len(), place(bounds)), path.fill, solid(&path.stroke))
        }
        Shape::QuadraticBezier(curve) => (format!("curve {}{}", place(bounds), path_look(curve.fill, &curve.stroke)?), curve.fill, solid(&curve.stroke)),
        Shape::CubicBezier(curve) => (format!("curve {}{}", place(bounds), path_look(curve.fill, &curve.stroke)?), curve.fill, solid(&curve.stroke)),
        _ => return None,
    };
    Some((line, if fill.a() > 0 { fill } else { stroke }))
}

fn solid(stroke: &PathStroke) -> Color32 {
    match stroke.color {
        ColorMode::Solid(color) => color,
        ColorMode::UV(_) => Color32::TRANSPARENT,
    }
}

fn text_line(shape: &TextShape) -> Option<(String, Color32, Kind)> {
    let galley = &shape.galley;
    let content = galley.text();
    let format = galley.job.sections.first().map(|section| &section.format);
    let written = format.map_or(Color32::PLACEHOLDER, |format| format.color);
    let color = match shape.override_text_color {
        Some(color) => color,
        None if written == Color32::PLACEHOLDER => shape.fallback_color,
        None => written,
    };
    if content.trim().is_empty() || color.a() == 0 || shape.opacity_factor <= 0.0 {
        return None;
    }
    let rect = galley.rect.translate(shape.pos.to_vec2());
    let mut line = format!("text {:?} {} {}", content, place(rect), color_text(color));
    if let Some(font) = format.map(|format| &format.font_id) {
        write!(line, " {}px", number(font.size)).unwrap();
        match &font.family {
            egui::FontFamily::Proportional => {}
            egui::FontFamily::Monospace => line.push_str(" mono"),
            egui::FontFamily::Name(name) => write!(line, " {name}").unwrap(),
        }
    }
    if shape.opacity_factor < 1.0 {
        write!(line, " opacity {}", number(shape.opacity_factor)).unwrap();
    }
    if shape.angle != 0.0 {
        write!(line, " angle {}", number(shape.angle)).unwrap();
    }
    Some((line, color, Kind::Text(rect, content.to_owned())))
}

fn rect_line(rect: &RectShape, texture: &dyn Fn(TextureId) -> String) -> Option<(String, Kind, Color32)> {
    let image = rect.brush.as_ref().map(|brush| texture(brush.fill_texture_id));
    let look = match &image {
        Some(_) => String::new(),
        None => look(rect.fill, rect.stroke)?,
    };
    let color = if rect.fill.a() > 0 { rect.fill } else { rect.stroke.color };
    let (mut line, kind) = match image {
        Some(name) => (format!("image {name:?} {}{}", place(rect.rect), tint(rect.fill)), Kind::Picture),
        None => (format!("rect {}{look}", place(rect.rect)), Kind::Rect),
    };
    let corners = rect.corner_radius;
    if corners.is_same() && corners.nw > 0 {
        write!(line, " r{}", corners.nw).unwrap();
    } else if !corners.is_same() {
        write!(line, " r{}/{}/{}/{}", corners.nw, corners.ne, corners.se, corners.sw).unwrap();
    }
    Some((line, kind, color))
}

fn mesh_line(mesh: &Mesh, bounds: Rect, texture: &dyn Fn(TextureId) -> String) -> Option<String> {
    if mesh.indices.is_empty() {
        return None;
    }
    if mesh.texture_id == TextureId::default() {
        return Some(format!("mesh {}tri {}", mesh.indices.len() / 3, place(bounds)));
    }
    let color = mesh.vertices.first().map_or(Color32::WHITE, |vertex| vertex.color);
    Some(format!("image {:?} {}{}", texture(mesh.texture_id), place(bounds), tint(color)))
}

/// ` fill #.. stroke w #..`; None when neither shows.
fn look(fill: Color32, stroke: Stroke) -> Option<String> {
    let stroke = (stroke.width > 0.0 && stroke.color.a() > 0).then(|| format!(" stroke {} {}", number(stroke.width), color_text(stroke.color)));
    let fill = (fill.a() > 0).then(|| format!(" fill {}", color_text(fill)));
    (fill.is_some() || stroke.is_some()).then(|| fill.unwrap_or_default() + &stroke.unwrap_or_default())
}

fn path_look(fill: Color32, stroke: &PathStroke) -> Option<String> {
    match stroke.color {
        ColorMode::Solid(color) => look(fill, Stroke::new(stroke.width, color)),
        ColorMode::UV(_) => Some(format!("{} stroke {} gradient", look(fill, Stroke::NONE).unwrap_or_default(), number(stroke.width))),
    }
}

fn tint(color: Color32) -> String {
    if color == Color32::WHITE {
        String::new()
    } else {
        format!(" tint {}", color_text(color))
    }
}

/// The widgets of the accessibility tree a player acts on, with their names.
struct Widgets<'a> {
    nodes: HashMap<NodeId, &'a Node>,
    root: Option<NodeId>,
    texts: Vec<(Rect, &'a str)>,
}

impl<'a> Widgets<'a> {
    fn new(tree: &'a accesskit::TreeUpdate, painted: &'a [Painted]) -> Self {
        let nodes = tree.nodes.iter().map(|(id, node)| (*id, node)).collect();
        let root = tree.tree.as_ref().map(|tree| tree.root);
        let texts = painted
            .iter()
            .filter_map(|item| match &item.kind {
                Kind::Text(rect, text) => Some((*rect, text.as_str())),
                _ => None,
            })
            .collect();
        Self { nodes, root, texts }
    }

    fn write(&self, out: &mut String) {
        if let Some(root) = self.root {
            self.write_node(out, root, 1);
        }
    }

    /// A kept widget's line, then its children one level deeper; a container's children
    /// stay at its level.
    fn write_node(&self, out: &mut String, id: NodeId, depth: usize) {
        let Some(node) = self.nodes.get(&id) else { return };
        let kept = self.line(node);
        if let Some(line) = &kept {
            writeln!(out, "{}{line}", "  ".repeat(depth)).unwrap();
        }
        let depth = depth + usize::from(kept.is_some());
        for child in node.children() {
            self.write_node(out, *child, depth);
        }
    }

    fn line(&self, node: &Node) -> Option<String> {
        let role = node.role();
        let acts = node.supports_action(Action::Click) || node.supports_action(Action::Focus);
        let kind = match role {
            Role::Unknown | Role::GenericContainer if acts => "click".to_owned(),
            Role::Unknown | Role::GenericContainer | Role::Window | Role::Label | Role::TextRun | Role::Pane | Role::Image => {
                return None
            }
            role => format!("{role:?}").to_lowercase(),
        };
        let bounds = node.bounds().map(|b| Rect::from_min_max(Pos2::new(b.x0 as f32, b.y0 as f32), Pos2::new(b.x1 as f32, b.y1 as f32)));
        let mut line = format!("{kind} {:?}", self.name(node, bounds));
        if let Some(bounds) = bounds {
            write!(line, " {}", place(bounds)).unwrap();
        }
        if node.is_disabled() {
            line.push_str(" disabled");
        }
        match node.toggled() {
            Some(Toggled::True) => line.push_str(" on"),
            Some(Toggled::False) => line.push_str(" off"),
            Some(Toggled::Mixed) => line.push_str(" mixed"),
            None => {}
        }
        if node.is_selected() == Some(true) {
            line.push_str(" selected");
        }
        if let Some(value) = node.numeric_value() {
            write!(line, " value {}", number(value as f32)).unwrap();
        }
        Some(line)
    }

    /// The widget's label, else its value, else the texts painted inside it.
    fn name(&self, node: &Node, bounds: Option<Rect>) -> String {
        let own = node.label().or(node.value()).filter(|name| !name.trim().is_empty());
        let name = match (own, bounds) {
            (Some(name), _) => name.to_owned(),
            (None, Some(bounds)) => {
                let inside = self.texts.iter().filter(|(rect, _)| bounds.expand(1.0).contains_rect(*rect));
                inside.map(|(_, text)| *text).collect::<Vec<_>>().join(" / ")
            }
            (None, None) => String::new(),
        };
        cut(name.replace('\n', " "))
    }
}

fn cut(name: String) -> String {
    match name.char_indices().nth(NAME_LIMIT) {
        Some((at, _)) => format!("{}…", &name[..at]),
        None => name,
    }
}

fn rounded(rect: Rect) -> Rect {
    Rect::from_min_max(rect.min.round(), rect.max.round())
}

/// `x,y wxh` in whole points.
fn place(rect: Rect) -> String {
    let rect = rounded(rect);
    format!("{},{} {}x{}", whole(rect.min.x), whole(rect.min.y), whole(rect.width()), whole(rect.height()))
}

fn point(at: Pos2) -> String {
    format!("{},{}", whole(at.x), whole(at.y))
}

fn whole(value: f32) -> i64 {
    // `+ 0.0` turns -0 into 0.
    (value.round() + 0.0) as i64
}

/// A size or factor with at most one decimal (`2`, `1.5`).
fn number(value: f32) -> String {
    let tenths = (value * 10.0).round() / 10.0 + 0.0;
    format!("{tenths}")
}

fn color_text(color: Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}
