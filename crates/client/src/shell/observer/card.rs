//! What the observer's tooltips and cards say, as plain data: a title, and sections of
//! lines. The module words them from its own rules (`Describe::tile_card`,
//! `Describe::city_card`: the same facts its ordinary tooltip and city screen show, with
//! no orders), the shell lays them out and paints them (`widget`). Nothing here names a
//! mechanic.
use crate::shell::hud::Icon;

/// How a line reads: the colour it is painted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Plain,
    Muted,
    Good,
    Bad,
    Warn,
    Accent,
}

/// A line of text with an optional icon in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub icon: Option<Icon>,
    pub text: String,
    pub tone: Tone,
}

impl Line {
    pub fn new(text: impl Into<String>) -> Self {
        Self { icon: None, text: text.into(), tone: Tone::Plain }
    }

    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

/// One row of a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A line of text (wraps when it is long).
    Line(Line),
    /// A label at the left and its value at the right: "Food  +3".
    Pair { label: String, value: String, tone: Tone },
    /// A label, its value and a bar `percent` full: "Growth  12 of 20".
    Bar { label: String, value: String, percent: u8 },
}

impl Row {
    pub fn line(text: impl Into<String>) -> Self {
        Row::Line(Line::new(text))
    }

    pub fn note(text: impl Into<String>) -> Self {
        Row::Line(Line::new(text).tone(Tone::Muted))
    }

    pub fn pair(label: impl Into<String>, value: impl ToString) -> Self {
        Row::Pair { label: label.into(), value: value.to_string(), tone: Tone::Plain }
    }

    pub fn toned(label: impl Into<String>, value: impl ToString, tone: Tone) -> Self {
        Row::Pair { label: label.into(), value: value.to_string(), tone }
    }

    pub fn bar(label: impl Into<String>, value: impl ToString, percent: u8) -> Self {
        Row::Bar { label: label.into(), value: value.to_string(), percent: percent.min(100) }
    }
}

/// A titled group of rows (the title may be absent: a tooltip's lines).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Section {
    pub title: Option<String>,
    pub rows: Vec<Row>,
}

impl Section {
    pub fn new(title: impl Into<String>, rows: Vec<Row>) -> Self {
        Self { title: Some(title.into()), rows }
    }

    pub fn plain(rows: Vec<Row>) -> Self {
        Self { title: None, rows }
    }
}

/// A tooltip or a card: a title, an optional line under it, and sections.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Card {
    pub title: String,
    pub subtitle: Option<String>,
    pub sections: Vec<Section>,
}

impl Card {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), subtitle: None, sections: Vec::new() }
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn section(mut self, section: Section) -> Self {
        self.sections.push(section);
        self
    }

    /// Rows of every section, in order.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.sections.iter().flat_map(|s| s.rows.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.sections.iter().all(|s| s.rows.is_empty())
    }
}
