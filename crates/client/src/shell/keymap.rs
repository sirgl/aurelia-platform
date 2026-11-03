//! Keyboard shortcuts: one table of actions with default keys, the player's changes on top,
//! and the one place that turns a key press into an action.
//!
//! * An action is a row (`ActionDef`): a stable id (`end-turn`, `found-city`), the situation
//!   it works in (`Scope`) and its default keys. The shell owns the rows every module shares
//!   (`SHELL_ROWS`: end turn, camera, saves, the help); a module lists its own
//!   (`ModuleUi::shortcuts`). The label of a row is the message `shortcut.<id>`.
//! * `Keymap` is the table with the player's changes applied. It resolves a key press in a
//!   `KeyContext` to the rows to try (narrowest scope first), finds conflicts for the
//!   remapping screen, and turns the changes back into `settings.cfg` lines.
//! * A chord is a key with Ctrl (Cmd on a Mac), Shift and Alt. It is written `Ctrl+Shift+B`
//!   with egui's key names (`Period`, `Plus`, `F5`). Letters are matched by the key the
//!   layout produces, and by the key's position where the layout has no Latin letter
//!   (a Russian <cyrillic> keyboard: the key labelled <cyrillic> is the B key). `ui-input` makes that
//!   choice; this module only matches what it reports.
//! * Text input is never robbed: the shell does not look for chords while a text field has
//!   the focus or an input method is composing.
use super::module::Screen;
use egui::{Key, Modifiers};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A key with its modifiers. `ctrl` is the platform's command key (Cmd on a Mac).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Keys whose symbol already needs Shift on most layouts: Shift is not part of their chord.
fn shifted_symbol(key: Key) -> bool {
    matches!(
        key,
        Key::Plus
            | Key::Questionmark
            | Key::Colon
            | Key::Pipe
            | Key::Exclamationmark
            | Key::OpenCurlyBracket
            | Key::CloseCurlyBracket
    )
}

fn is_modifier_key(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
            | Key::Copy
            | Key::Cut
            | Key::Paste
    )
}

impl Chord {
    pub const fn plain(key: Key) -> Self {
        Self { key, ctrl: false, shift: false, alt: false }
    }

    /// The chord of a key press; None for a modifier key by itself.
    pub fn of_event(key: Key, modifiers: &Modifiers) -> Option<Self> {
        if is_modifier_key(key) {
            return None;
        }
        Some(Self { key, ctrl: modifiers.command, shift: modifiers.shift && !shifted_symbol(key), alt: modifiers.alt })
    }

    /// `Ctrl+Shift+B`; None when the text is not a chord (an unknown key, a modifier alone).
    pub fn parse(text: &str) -> Option<Self> {
        let mut chord = Self::plain(Key::Escape);
        let mut key = None;
        for part in text.split('+') {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "cmd" | "command" => chord.ctrl = true,
                "shift" => chord.shift = true,
                "alt" | "option" => chord.alt = true,
                _ => {
                    if key.is_some() {
                        return None;
                    }
                    key = Some(Key::from_name(part).filter(|k| !is_modifier_key(*k))?);
                }
            }
        }
        chord.key = key?;
        chord.shift &= !shifted_symbol(chord.key);
        Some(chord)
    }

    /// The text that is saved: `Ctrl+Shift+B`, with egui's key names.
    pub fn canonical(&self) -> String {
        let mut text = String::new();
        for (on, name) in [(self.ctrl, "Ctrl"), (self.shift, "Shift"), (self.alt, "Alt")] { // i18n-ok: keycap names and file keys
            if on {
                text.push_str(name);
                text.push('+');
            }
        }
        text + self.key.name()
    }

    /// The text of a keycap for tooltips and the screens (`Ctrl+B`, `.`, `+`).
    pub fn caption(&self) -> String {
        let key = match self.key {
            Key::Period => ".",
            Key::Comma => ",",
            Key::Plus => "+",
            Key::Minus => "-",
            Key::Equals => "=",
            Key::Slash => "/",
            Key::Questionmark => "?",
            Key::Escape => "Esc", // i18n-ok: keycap names and file keys
            Key::PageUp => "PgUp",
            Key::PageDown => "PgDn",
            Key::ArrowLeft => "Left", // i18n-ok: keycap names and file keys
            Key::ArrowRight => "Right", // i18n-ok: keycap names and file keys
            Key::ArrowUp => "Up",
            Key::ArrowDown => "Down", // i18n-ok: keycap names and file keys
            other => other.name(),
        };
        let mut text = String::new();
        for (on, name) in [(self.ctrl, "Ctrl"), (self.shift, "Shift"), (self.alt, "Alt")] { // i18n-ok: keycap names and file keys
            if on {
                text.push_str(name);
                text.push('+');
            }
        }
        text + key
    }
}

/// The key presses among a frame's events that are shortcut candidates, in order: key-downs
/// that are not repeats, and not while an input method is composing (those keys are text).
/// The key is the one the layout produced, or the key's position on a layout without Latin
/// letters (`ui-input` decides; `physical_key` is not looked at), so on a Russian layout the
/// key labelled <cyrillic> is the `B` key, and on an AZERTY layout the key labelled A is `A`.
pub fn pressed(events: &[egui::Event]) -> Vec<(Key, Modifiers)> {
    let mut composing = false;
    let mut found = Vec::new();
    for event in events {
        match event {
            egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => composing = !text.is_empty(),
            egui::Event::Ime(_) => composing = false,
            egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } if !composing => found.push((*key, *modifiers)),
            _ => {}
        }
    }
    found
}

/// Where an action works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Everywhere outside text fields, also in the menu before a match.
    Anywhere,
    /// While a match runs, over any screen.
    Game,
    /// Over the map of a running match.
    Map,
    /// On one screen of the module (its `ScreenId`).
    Screen(u8),
}

/// Whether an action also needs a unit not to be selected (the letters that move the camera
/// are the unit's orders while a unit is selected).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Always,
    NoUnit,
}

/// One action: its id, group in the help, scope and default keys (chords separated by spaces).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionDef {
    pub id: &'static str,
    pub group: &'static str,
    pub scope: Scope,
    pub when: When,
    pub keys: &'static str,
}

impl ActionDef {
    pub const fn new(id: &'static str, group: &'static str, scope: Scope, keys: &'static str) -> Self {
        Self { id, group, scope, when: When::Always, keys }
    }

    /// Works only while no unit is selected.
    pub const fn no_unit(mut self) -> Self {
        self.when = When::NoUnit;
        self
    }

    /// The label message of the action.
    pub fn label_key(&self) -> String {
        format!("shortcut.{}", self.id)
    }

    /// The label in the active language (the English text of the catalogue when it has none).
    pub fn label(&self) -> String {
        // i18n-keys: shortcut.*
        i18n::tr_dynamic(&self.label_key(), &[]).unwrap_or_else(|| self.id.to_owned())
    }
}

/// The rows of a module with its name (the prefix of its lines in `settings.cfg`).
#[derive(Debug, Clone, Copy)]
pub struct ShortcutSet {
    pub name: &'static str,
    pub rows: &'static [ActionDef],
}

impl ShortcutSet {
    pub const NONE: ShortcutSet = ShortcutSet { name: "none", rows: &[] };
}

/// The name under which the shell's own rows are saved.
pub const SHELL_NAME: &str = "shell";

/// Rows every module shares. A module may not reuse these ids.
pub const SHELL_ROWS: &[ActionDef] = &[
    ActionDef::new("end-turn", "turn", Scope::Map, "Enter Ctrl+Space"), // i18n-ok: keycap names and file keys
    ActionDef::new("center-selection", "map", Scope::Map, "Home"), // i18n-ok: keycap names and file keys
    ActionDef::new("toggle-grid", "map", Scope::Map, "G"),
    ActionDef::new("toggle-yields", "map", Scope::Map, "Ctrl+Y"),
    ActionDef::new("zoom-in", "map", Scope::Map, "Plus Equals PageUp"), // i18n-ok: keycap names and file keys
    ActionDef::new("zoom-out", "map", Scope::Map, "Minus PageDown"), // i18n-ok: keycap names and file keys
    ActionDef::new("pan-left", "camera", Scope::Map, "ArrowLeft"),
    ActionDef::new("pan-right", "camera", Scope::Map, "ArrowRight"),
    ActionDef::new("pan-up", "camera", Scope::Map, "ArrowUp"),
    ActionDef::new("pan-down", "camera", Scope::Map, "ArrowDown"),
    ActionDef::new("pan-left-alt", "camera", Scope::Map, "A").no_unit(),
    ActionDef::new("pan-right-alt", "camera", Scope::Map, "D").no_unit(),
    ActionDef::new("pan-up-alt", "camera", Scope::Map, "W").no_unit(),
    ActionDef::new("pan-down-alt", "camera", Scope::Map, "S").no_unit(),
    ActionDef::new("quick-save", "game", Scope::Game, "F11 Ctrl+S"), // i18n-ok: keycaps
    ActionDef::new("quick-load", "game", Scope::Game, "F12 Ctrl+L"), // i18n-ok: keycaps
    ActionDef::new("settings", "game", Scope::Anywhere, "Ctrl+O"),
    ActionDef::new("shortcuts", "game", Scope::Anywhere, "Ctrl+H"),
];

/// What the shell knows when a key is pressed.
#[derive(Debug, Clone, Copy)]
pub struct KeyContext {
    /// A match runs.
    pub in_game: bool,
    pub screen: Screen,
    /// An own unit is selected.
    pub unit_selected: bool,
    /// The shortcuts overlay is open: only the help key works.
    pub overlay: bool,
}

impl Scope {
    fn rank(self) -> u8 {
        match self {
            Scope::Anywhere => 0,
            Scope::Game => 1,
            Scope::Map => 2,
            Scope::Screen(_) => 3,
        }
    }

    /// Whether the scope is in force in `context`.
    pub fn active(self, context: &KeyContext) -> bool {
        match self {
            Scope::Anywhere => true,
            Scope::Game => context.in_game,
            Scope::Map => context.in_game && context.screen == Screen::Map,
            Scope::Screen(id) => context.in_game && context.screen == Screen::Module(super::module::ScreenId(id)),
        }
    }

    /// Whether one key press could mean both scopes at once.
    fn overlaps(self, other: Scope) -> bool {
        use Scope::*;
        match (self, other) {
            (Anywhere, _) | (_, Anywhere) => true,
            (Game, _) | (_, Game) => true,
            (Map, Map) => true,
            (Screen(a), Screen(b)) => a == b,
            (Map, Screen(_)) | (Screen(_), Map) => false,
        }
    }
}

/// A row with its keys now.
#[derive(Debug, Clone)]
pub struct Binding {
    pub def: &'static ActionDef,
    pub chords: Vec<Chord>,
    /// Handled by the shell itself (else by the module).
    pub shell: bool,
}

/// The player's changes: `<set>.<id>` -> chords as text (`B Ctrl+B`), `none` for no key.
pub type Overrides = BTreeMap<String, String>;

pub fn parse_chords(text: &str) -> Option<Vec<Chord>> {
    if text.trim().eq_ignore_ascii_case("none") {
        return Some(Vec::new());
    }
    let chords: Option<Vec<Chord>> = text.split_whitespace().map(Chord::parse).collect();
    chords.filter(|c| !c.is_empty())
}

/// The text of a list of chords for the settings file.
pub fn chords_text(chords: &[Chord]) -> String {
    if chords.is_empty() {
        "none".to_owned()
    } else {
        chords.iter().map(Chord::canonical).collect::<Vec<_>>().join(" ")
    }
}

#[derive(Debug, Clone)]
pub struct Keymap {
    set: &'static str,
    bindings: Vec<Binding>,
}

/// A chord that another action already has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub id: &'static str,
}

impl Keymap {
    /// The shell's rows and the module's, with the player's changes applied.
    pub fn build(module: ShortcutSet, overrides: &Overrides) -> Self {
        let mut bindings = Vec::new();
        for (set, rows, shell) in [(SHELL_NAME, SHELL_ROWS, true), (module.name, module.rows, false)] {
            for def in rows {
                let chords = overrides
                    .get(&format!("{set}.{}", def.id))
                    .and_then(|text| parse_chords(text))
                    .unwrap_or_else(|| default_chords(def));
                bindings.push(Binding { def, chords, shell });
            }
        }
        Self { set: module.name, bindings }
    }

    pub fn defaults(module: ShortcutSet) -> Self {
        Self::build(module, &Overrides::new())
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    pub fn binding(&self, id: &str) -> Option<&Binding> {
        self.bindings.iter().find(|b| b.def.id == id)
    }

    pub fn chords(&self, id: &str) -> &[Chord] {
        self.binding(id).map_or(&[], |b| &b.chords)
    }

    /// The keycap text of the first key of an action (`Enter`), for tooltips.
    pub fn hint(&self, id: &str) -> Option<String> {
        self.chords(id).first().map(Chord::caption)
    }

    /// The groups in table order.
    pub fn groups(&self) -> Vec<&'static str> {
        let mut groups: Vec<&'static str> = Vec::new();
        for b in &self.bindings {
            if !groups.contains(&b.def.group) {
                groups.push(b.def.group);
            }
        }
        groups
    }

    /// The rows to try for a key press, narrowest scope first. A row whose action the
    /// module cannot do now (a unit without that order) hands the key to the next.
    pub fn resolve(&self, chord: Chord, context: &KeyContext) -> Vec<&Binding> {
        let mut found: Vec<&Binding> = self
            .bindings
            .iter()
            .filter(|b| b.chords.contains(&chord))
            .filter(|b| if context.overlay { b.def.id == "shortcuts" } else { b.def.scope.active(context) })
            .filter(|b| b.def.when == When::Always || !context.unit_selected)
            .collect();
        found.sort_by_key(|b| std::cmp::Reverse(b.def.scope.rank()));
        found
    }

    /// Other actions that already have `chord` where `id` works.
    pub fn conflicts(&self, id: &str, chord: Chord) -> Vec<Conflict> {
        let Some(own) = self.binding(id) else { return Vec::new() };
        self.bindings
            .iter()
            .filter(|b| b.def.id != id && b.chords.contains(&chord) && b.def.scope.overlaps(own.def.scope))
            .filter(|b| (b.def.when == When::NoUnit) == (own.def.when == When::NoUnit))
            .map(|b| Conflict { id: b.def.id })
            .collect()
    }

    /// Every chord used twice where the scopes overlap (a table error; the defaults have none).
    pub fn duplicates(&self) -> Vec<(Chord, &'static str, &'static str)> {
        let mut found = Vec::new();
        for (i, a) in self.bindings.iter().enumerate() {
            for b in &self.bindings[i + 1..] {
                if !a.def.scope.overlaps(b.def.scope) {
                    continue;
                }
                // An alternate camera key that waits for "no unit" does not clash with an order.
                let split = (a.def.when == When::NoUnit) != (b.def.when == When::NoUnit);
                for chord in &a.chords {
                    if b.chords.contains(chord) && !split {
                        found.push((*chord, a.def.id, b.def.id));
                    }
                }
            }
        }
        found
    }

    /// Gives `id` exactly these keys; other actions that had one of them lose it.
    pub fn assign(&mut self, id: &str, chord: Chord, replace_others: bool) {
        let conflicts = self.conflicts(id, chord);
        if replace_others {
            for conflict in &conflicts {
                if let Some(other) = self.bindings.iter_mut().find(|b| b.def.id == conflict.id) {
                    other.chords.retain(|c| *c != chord);
                }
            }
        }
        if let Some(own) = self.bindings.iter_mut().find(|b| b.def.id == id) {
            if !own.chords.contains(&chord) {
                own.chords.push(chord);
            }
        }
    }

    /// Replaces the keys of `id` by the one chord.
    pub fn rebind(&mut self, id: &str, chord: Chord, replace_others: bool) {
        if let Some(own) = self.bindings.iter_mut().find(|b| b.def.id == id) {
            own.chords.clear();
        }
        self.assign(id, chord, replace_others);
    }

    pub fn unbind(&mut self, id: &str) {
        if let Some(own) = self.bindings.iter_mut().find(|b| b.def.id == id) {
            own.chords.clear();
        }
    }

    pub fn reset(&mut self, id: &str) {
        if let Some(own) = self.bindings.iter_mut().find(|b| b.def.id == id) {
            own.chords = default_chords(own.def);
        }
    }

    pub fn reset_all(&mut self) {
        for b in &mut self.bindings {
            b.chords = default_chords(b.def);
        }
    }

    pub fn is_default(&self, id: &str) -> bool {
        self.binding(id).is_none_or(|b| b.chords == default_chords(b.def))
    }

    /// The settings lines for the rows that differ from the defaults, merged into `saved`
    /// (the lines of other modules stay).
    pub fn write_overrides(&self, saved: &mut Overrides) {
        for b in &self.bindings {
            let set = if b.shell { SHELL_NAME } else { self.set };
            let key = format!("{set}.{}", b.def.id);
            if b.chords == default_chords(b.def) {
                saved.remove(&key);
            } else {
                saved.insert(key, chords_text(&b.chords));
            }
        }
    }

    /// Camera direction (-1..1 per axis) from the keys held down. Letters count only while
    /// no unit is selected.
    pub fn pan(&self, down: impl Fn(Key) -> bool, unit_selected: bool) -> (f32, f32) {
        let held = |ids: [&str; 2]| {
            ids.iter().any(|id| {
                self.binding(id).is_some_and(|b| {
                    (b.def.when == When::Always || !unit_selected) && b.chords.iter().any(|c| !c.ctrl && !c.alt && down(c.key))
                })
            })
        };
        let axis = |negative: bool, positive: bool| f32::from(u8::from(positive)) - f32::from(u8::from(negative));
        (
            axis(held(["pan-left", "pan-left-alt"]), held(["pan-right", "pan-right-alt"])),
            axis(held(["pan-up", "pan-up-alt"]), held(["pan-down", "pan-down-alt"])),
        )
    }
}

fn default_chords(def: &ActionDef) -> Vec<Chord> {
    def.keys.split_whitespace().filter_map(Chord::parse).collect()
}

// --- The keymap in the egui context, for the tooltips of buttons --------------------------

#[derive(Clone)]
struct Published(Arc<Keymap>);

/// Makes the keymap of this frame readable by the widgets (`hint`).
pub fn publish(ctx: &egui::Context, keymap: &Arc<Keymap>) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("shell_keymap"), Published(keymap.clone())));
}

/// The first key of an action in the published keymap (`Enter`).
pub fn hint(ctx: &egui::Context, id: &str) -> Option<String> {
    ctx.data(|data| data.get_temp::<Published>(egui::Id::new("shell_keymap"))).and_then(|k| k.0.hint(id))
}

/// `text (key)`: a tooltip or a label with the action's key.
pub fn with_hint(ctx: &egui::Context, text: &str, id: &str) -> String {
    match hint(ctx, id) {
        Some(key) => format!("{text} ({key})"),
        None => text.to_owned(),
    }
}

/// A key shown for a screen or action: the keymap's key of the action `name`, else `name`
/// itself (a fixed key such as `Esc`).
pub fn key_text(ctx: &egui::Context, name: &str) -> String {
    hint(ctx, name).unwrap_or_else(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS: &[ActionDef] = &[
        ActionDef::new("found-city", "units", Scope::Map, "B"),
        ActionDef::new("order-fortify", "units", Scope::Map, "F"),
        ActionDef::new("technologies", "screens", Scope::Game, "F6"),
        ActionDef::new("next-city", "cities", Scope::Screen(2), "ArrowRight"),
    ];
    const SET: ShortcutSet = ShortcutSet { name: "test", rows: ROWS };

    fn map_context() -> KeyContext {
        KeyContext { in_game: true, screen: Screen::Map, unit_selected: false, overlay: false }
    }

    fn key_down(key: Key, physical_key: Option<Key>) -> egui::Event {
        egui::Event::Key { key, physical_key, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    #[test]
    fn the_key_is_the_one_the_layout_made_or_its_place_without_latin_letters() {
        // Russian layout: ui-input reports the key at the place of B (it has no Latin B to give).
        let russian = [key_down(Key::B, Some(Key::B))];
        assert_eq!(pressed(&russian).len(), 1);
        let found = pressed(&russian)[0];
        assert_eq!(Chord::of_event(found.0, &found.1), Chord::parse("B"));
        // AZERTY: the key labelled A sits where Q is on a US keyboard; the label counts.
        let azerty = [key_down(Key::A, Some(Key::Q))];
        let found = pressed(&azerty)[0];
        assert_eq!(Chord::of_event(found.0, &found.1), Chord::parse("A"));
    }

    #[test]
    fn keys_of_an_input_method_and_repeats_are_not_shortcuts() {
        let preedit = |text: &str| egui::Event::Ime(egui::ImeEvent::Preedit { text: text.to_owned(), active_range_chars: None });
        let repeat = egui::Event::Key { key: Key::B, physical_key: None, pressed: true, repeat: true, modifiers: Modifiers::NONE };
        let up = egui::Event::Key { key: Key::B, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE };
        assert!(pressed(&[preedit("\u{43d}\u{438}"), key_down(Key::B, None)]).is_empty(), "composing");
        assert_eq!(pressed(&[preedit("\u{43d}\u{438}"), preedit(""), key_down(Key::B, None)]).len(), 1, "composition ended");
        assert_eq!(pressed(&[preedit("\u{43d}\u{438}"), egui::Event::Ime(egui::ImeEvent::Commit("\u{43d}\u{438}".into())), key_down(Key::B, None)]).len(), 1);
        assert!(pressed(&[repeat, up]).is_empty());
    }

    fn event(key: Key) -> Chord {
        Chord::of_event(key, &Modifiers::NONE).unwrap()
    }

    #[test]
    fn chords_round_trip_through_their_text() {
        for text in ["B", "Ctrl+B", "Ctrl+Shift+Space", "Alt+C", "F11", "Period", "Plus", "ArrowLeft"] {
            let chord = Chord::parse(text).unwrap_or_else(|| panic!("{text}"));
            let again = Chord::parse(&chord.canonical()).unwrap();
            assert_eq!(chord, again, "{text}");
        }
        assert_eq!(Chord::parse("cmd+b"), Chord::parse("Ctrl+B"));
        assert_eq!(Chord::parse("Ctrl"), None, "a modifier alone is no chord");
        assert_eq!(Chord::parse("Ctrl+Bogus"), None);
        assert_eq!(Chord::parse("A+B"), None, "two keys");
        assert_eq!(Chord::parse("").map(|c| c.key), None);
    }

    #[test]
    fn captions_are_the_keycaps() {
        assert_eq!(Chord::parse("Period").unwrap().caption(), ".");
        assert_eq!(Chord::parse("Ctrl+Space").unwrap().caption(), "Ctrl+Space");
        assert_eq!(Chord::parse("Escape").unwrap().caption(), "Esc");
    }

    #[test]
    fn shift_is_not_part_of_a_symbol_that_needs_it() {
        let shifted = Modifiers { shift: true, ..Modifiers::NONE };
        assert_eq!(Chord::of_event(Key::Plus, &shifted), Chord::parse("Plus"), "+ on a US keyboard is Shift+=");
        assert_eq!(Chord::of_event(Key::Questionmark, &shifted), Chord::parse("Questionmark"));
        assert_eq!(Chord::of_event(Key::B, &shifted), Chord::parse("Shift+B"));
        assert_eq!(Chord::of_event(Key::ShiftLeft, &Modifiers::NONE), None);
    }

    #[test]
    fn the_shell_table_and_a_module_have_no_duplicates() {
        let map = Keymap::defaults(SET);
        assert_eq!(map.duplicates(), vec![]);
        let mut seen = std::collections::HashSet::new();
        for b in map.bindings() {
            assert!(seen.insert(b.def.id), "{} is listed twice", b.def.id);
            assert!(!b.chords.is_empty(), "{} has no default key", b.def.id);
            assert_eq!(default_chords(b.def).len(), b.def.keys.split_whitespace().count(), "{}: a default key is not a chord", b.def.id);
        }
    }

    #[test]
    fn every_shell_action_has_an_english_label() {
        for def in SHELL_ROWS {
            assert!(i18n::tr_dynamic(&def.label_key(), &[]).is_some(), "{} has no message", def.label_key());
        }
    }

    #[test]
    fn a_key_resolves_by_scope_and_the_narrowest_comes_first() {
        let map = Keymap::defaults(SET);
        let on_map = map_context();
        let ids = |chord, context: &KeyContext| map.resolve(chord, context).iter().map(|b| b.def.id).collect::<Vec<_>>();
        assert_eq!(ids(event(Key::B), &on_map), ["found-city"]);
        assert_eq!(ids(event(Key::F6), &on_map), ["technologies"]);
        let screen = KeyContext { screen: Screen::Menu, ..on_map };
        assert_eq!(ids(event(Key::B), &screen), Vec::<&str>::new(), "orders only over the map");
        assert_eq!(ids(event(Key::F6), &screen), ["technologies"], "screens from any screen of the match");
        let before_match = KeyContext { in_game: false, ..screen };
        assert_eq!(ids(event(Key::F6), &before_match), Vec::<&str>::new());
        assert_eq!(ids(Chord::parse("Ctrl+H").unwrap(), &before_match), ["shortcuts"], "the help works before a match");
        let city = KeyContext { screen: Screen::Module(super::super::module::ScreenId(2)), ..on_map };
        assert_eq!(ids(event(Key::ArrowRight), &city), ["next-city"]);
        assert_eq!(ids(event(Key::ArrowRight), &on_map), ["pan-right"]);
    }

    #[test]
    fn the_camera_letters_wait_while_a_unit_is_selected() {
        let map = Keymap::defaults(SET);
        let mut context = map_context();
        assert_eq!(map.resolve(event(Key::A), &context).len(), 1);
        context.unit_selected = true;
        assert_eq!(map.resolve(event(Key::A), &context).len(), 0);
        let held = |keys: &'static [Key]| move |k: Key| keys.contains(&k);
        assert_eq!(map.pan(held(&[Key::ArrowLeft, Key::ArrowUp]), true), (-1., -1.));
        assert_eq!(map.pan(held(&[Key::D, Key::S]), false), (1., 1.));
        assert_eq!(map.pan(held(&[Key::D, Key::S]), true), (0., 0.));
        assert_eq!(map.pan(held(&[Key::A, Key::D]), false), (0., 0.));
    }

    #[test]
    fn with_the_overlay_open_only_the_help_key_works() {
        let map = Keymap::defaults(SET);
        let context = KeyContext { overlay: true, ..map_context() };
        assert!(map.resolve(event(Key::B), &context).is_empty());
        assert_eq!(map.resolve(Chord::parse("Ctrl+H").unwrap(), &context).len(), 1);
    }

    #[test]
    fn a_remap_finds_conflicts_replaces_and_resets() {
        let mut map = Keymap::defaults(SET);
        let f = event(Key::F);
        assert_eq!(map.conflicts("found-city", f), vec![Conflict { id: "order-fortify" }]);
        assert!(map.conflicts("found-city", event(Key::J)).is_empty());
        assert!(!map.conflicts("technologies", f).is_empty(), "a Game key meets a Map key");
        assert!(map.conflicts("next-city", event(Key::B)).is_empty(), "a city screen key does not meet a map key");
        map.rebind("found-city", f, true);
        assert_eq!(map.chords("found-city"), &[f]);
        assert!(map.chords("order-fortify").is_empty(), "the other action lost the key");
        assert!(!map.is_default("found-city"));
        map.reset_all();
        assert_eq!(map.chords("found-city"), &[event(Key::B)]);
        assert_eq!(map.chords("order-fortify"), &[f]);
    }

    #[test]
    fn changes_are_saved_as_lines_and_come_back() {
        let mut map = Keymap::defaults(SET);
        map.rebind("found-city", Chord::parse("Ctrl+B").unwrap(), true);
        map.unbind("order-fortify");
        map.rebind("zoom-in", Chord::parse("Plus").unwrap(), true);
        let mut saved = Overrides::new();
        saved.insert("other.thing".to_owned(), "X".to_owned());
        map.write_overrides(&mut saved);
        assert_eq!(saved["test.found-city"], "Ctrl+B");
        assert_eq!(saved["test.order-fortify"], "none");
        assert_eq!(saved["shell.zoom-in"], "Plus");
        assert_eq!(saved["other.thing"], "X", "other modules' lines stay");
        assert!(!saved.contains_key("test.technologies"), "the defaults are not written");
        let again = Keymap::build(SET, &saved);
        assert_eq!(again.chords("found-city"), map.chords("found-city"));
        assert!(again.chords("order-fortify").is_empty());
        // Putting the default back removes the line.
        map.reset("found-city");
        map.write_overrides(&mut saved);
        assert!(!saved.contains_key("test.found-city"));
    }

    #[test]
    fn bad_lines_keep_the_defaults() {
        let mut saved = Overrides::new();
        saved.insert("test.found-city".to_owned(), "Ctrl+Bogus".to_owned());
        saved.insert("test.gone-action".to_owned(), "B".to_owned());
        let map = Keymap::build(SET, &saved);
        assert_eq!(map.chords("found-city"), &[event(Key::B)]);
        assert!(map.binding("gone-action").is_none());
    }
}
