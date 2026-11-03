//! The plain events the translation core understands: our own structs and enums, no winit.
//!
//! The winit adapter (`winit_glue`) turns a `WindowEvent` into one of these; a test builds them
//! directly. Positions are in physical pixels of the window, as winit reports them; the core
//! divides by the current pixels-per-point.

/// A mouse button.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Button {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    /// Any other button: egui has no name for it, the event is dropped.
    Other,
}

/// Where a wheel, touch or pan gesture is in its life.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

/// A scroll amount.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WheelDelta {
    /// Notches of a wheel: the application decides how far a line is.
    Lines { x: f32, y: f32 },
    /// Physical pixels (a trackpad).
    Pixels { x: f64, y: f64 },
}

/// One finger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchInput {
    /// The touch screen (winit's device id, hashed). Two screens never share a finger id.
    pub device: u64,
    /// The finger; stable from `Started` to `Ended` or `Cancelled`.
    pub id: u64,
    pub phase: Phase,
    /// Physical pixels.
    pub x: f64,
    pub y: f64,
    /// Pressure in 0..=1 when the screen reports it.
    pub force: Option<f32>,
}

/// Which modifier keys are held (either side counts).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModifierState {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// The Windows key, or Cmd on a Mac.
    pub super_key: bool,
}

/// A key press or release, already looked up in egui's key table by the adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyInput {
    /// The key the layout produced (`Key::B` for the key labelled B, `Key::A` on AZERTY for the
    /// key labelled A). `None` when egui has no key for it: a Cyrillic or Greek letter, a dead key.
    pub logical: Option<egui::Key>,
    /// The key at that place on a US keyboard, whatever the layout says.
    pub physical: Option<egui::Key>,
    /// The text the press types. The core only looks at it for presses.
    pub text: Option<String>,
    pub pressed: bool,
    /// The operating system's auto-repeat, not a new press.
    pub repeat: bool,
    /// The platform made this event up on a focus change; a synthetic press is no key press.
    pub synthetic: bool,
    /// The input method swallowed this key (winit's `Process` key): it is part of a composition.
    pub ime_processed: bool,
}

impl KeyInput {
    /// A real key press of `key` (logical and physical key the same), no text.
    pub fn press(key: egui::Key) -> Self {
        Self {
            logical: Some(key),
            physical: Some(key),
            text: None,
            pressed: true,
            repeat: false,
            synthetic: false,
            ime_processed: false,
        }
    }

    /// The release that belongs to a press.
    pub fn released(mut self) -> Self {
        self.pressed = false;
        self.text = None;
        self
    }
}

/// An input method (IME) event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Ime {
    Enabled,
    Disabled,
    /// Text being composed. `cursor` is the highlighted range in BYTES of `text`, as winit gives it.
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    Commit(String),
}

/// Everything the core translates.
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    /// The mouse moved to this position (physical pixels).
    CursorMoved {
        x: f64,
        y: f64,
    },
    CursorLeft,
    Button {
        button: Button,
        pressed: bool,
    },
    Wheel {
        delta: WheelDelta,
        phase: Phase,
    },
    Touch(TouchInput),
    Key(KeyInput),
    Modifiers(ModifierState),
    Ime(Ime),
    Focused(bool),
    /// The display's scale factor changed (the window moved to another monitor).
    ScaleFactor(f64),
    /// A trackpad pinch: `delta` is the change of the logarithm of the scale (positive zooms in).
    Pinch {
        delta: f64,
    },
    /// A trackpad two-finger pan, physical pixels.
    Pan {
        dx: f64,
        dy: f64,
        phase: Phase,
    },
    /// Something that needs a new frame but carries no input (resize, move, occlusion).
    Repaint,
    /// Something egui does not care about at all (no frame needed).
    Ignored,
}
