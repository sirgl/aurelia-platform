//! The translation core: [`InputEvent`]s in, an `egui::RawInput` out. No winit here.
//!
//! One [`Translator`] belongs to one window. Feed it every event of the window
//! ([`Translator::handle`]), then once per frame take the accumulated input
//! ([`Translator::take_input`]) and give it to egui.
use std::collections::HashSet;

use egui::{Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

use crate::clipboard::{Clipboard, SystemClipboard};
use crate::event::{
    Button, Ime, InputEvent, KeyInput, ModifierState, Phase, TouchInput, WheelDelta,
};

/// What egui says about the last frame's pointer and keyboard, to decide whether an event is
/// egui's or the game's. `egui::Context` implements it; a test uses a stub.
pub trait Wants {
    /// A click or a wheel turn would land on egui (`Context::egui_wants_pointer_input`).
    fn pointer(&self) -> bool;
    /// egui is dragging or pressing something (`Context::egui_is_using_pointer`).
    fn using_pointer(&self) -> bool;
    /// A text field has the focus (`Context::egui_wants_keyboard_input`).
    fn keyboard(&self) -> bool;
}

impl Wants for egui::Context {
    fn pointer(&self) -> bool {
        self.egui_wants_pointer_input()
    }

    fn using_pointer(&self) -> bool {
        self.egui_is_using_pointer()
    }

    fn keyboard(&self) -> bool {
        self.egui_wants_keyboard_input()
    }
}

/// What the event did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Response {
    /// egui wants this event for itself (a click on a panel, typing into a text field): the game
    /// must not act on it.
    pub consumed: bool,
    /// A new frame is needed.
    pub repaint: bool,
}

/// Which platform's conventions apply. [`Platform::current`] is the one this build runs on.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Platform {
    /// Cmd (the Super key) is the command key; elsewhere it is Ctrl.
    pub mac: bool,
    /// Shift+Delete, Ctrl+Insert and Shift+Insert also cut, copy and paste; a buggy Windows IME
    /// reports the cursor of a composition at 0..0.
    pub windows: bool,
}

impl Platform {
    pub fn current() -> Self {
        Self {
            mac: cfg!(target_os = "macos"),
            windows: cfg!(target_os = "windows"),
        }
    }
}

/// The window's state when a frame starts.
#[derive(Clone, Copy, Debug)]
pub struct FrameInfo {
    /// The drawable area in physical pixels.
    pub size_px: (f32, f32),
    /// Seconds since the program started the UI.
    pub time: f64,
    /// Pixels per egui point now: the display's scale factor times egui's zoom factor.
    pub pixels_per_point: f32,
    /// The display's scale factor alone.
    pub native_pixels_per_point: f32,
}

/// Translates input events into `egui::RawInput`.
pub struct Translator {
    pub(crate) input: RawInput,
    pub(crate) platform: Platform,
    modifiers: Modifiers,
    pixels_per_point: f32,
    /// Where the pointer is inside the window, in points; `None` when it is outside.
    pub(crate) pointer: Option<Pos2>,
    /// The finger that moves the pointer; the other fingers only produce touch events.
    pointer_touch: Option<u64>,
    /// Keys pressed and not yet released, to release them when the focus is lost.
    held: Vec<(Key, Option<Key>)>,
    /// An input method is allowed (set by the output side); its swallowed keys are not keys.
    pub(crate) ime_allowed: bool,
    /// Physical keys whose press the input method swallowed; their release is swallowed too.
    ime_swallowed: HashSet<Option<Key>>,
    clipboard: Box<dyn Clipboard>,
}

impl Translator {
    /// A translator for the platform this build runs on, with the system clipboard.
    pub fn new(native_pixels_per_point: f32) -> Self {
        Self::with(
            Platform::current(),
            Box::new(SystemClipboard::new()),
            native_pixels_per_point,
        )
    }

    /// A translator with a chosen platform and clipboard (tests).
    pub fn with(
        platform: Platform,
        clipboard: Box<dyn Clipboard>,
        native_pixels_per_point: f32,
    ) -> Self {
        let mut input = RawInput {
            focused: false,
            ..Default::default()
        };
        input
            .viewports
            .entry(input.viewport_id)
            .or_default()
            .native_pixels_per_point = Some(native_pixels_per_point);
        Self {
            input,
            platform,
            modifiers: Modifiers::default(),
            pixels_per_point: native_pixels_per_point,
            pointer: None,
            pointer_touch: None,
            held: Vec::new(),
            ime_allowed: false,
            ime_swallowed: HashSet::new(),
            clipboard,
        }
    }

    /// Pixels per point for the events that follow (the scale factor times egui's zoom factor).
    pub fn set_pixels_per_point(&mut self, pixels_per_point: f32) {
        if pixels_per_point.is_finite() && pixels_per_point > 0. {
            self.pixels_per_point = pixels_per_point;
        }
    }

    /// The input collected so far (events are cleared by [`Self::take_input`]).
    pub fn pending(&self) -> &RawInput {
        &self.input
    }

    /// The clipboard (the window glue copies and pastes through it too).
    pub fn clipboard(&mut self) -> &mut dyn Clipboard {
        self.clipboard.as_mut()
    }

    /// Whether the pointer is inside the window.
    pub fn pointer_in_window(&self) -> bool {
        self.pointer.is_some()
    }

    /// The input of this frame: the events since the last call plus the frame's facts.
    pub fn take_input(&mut self, frame: FrameInfo) -> RawInput {
        self.input.time = Some(frame.time);
        let points =
            Vec2::new(frame.size_px.0, frame.size_px.1) / frame.pixels_per_point.max(f32::EPSILON);
        // A minimised window can have zero size: no screen, so egui keeps its window positions.
        self.input.screen_rect =
            (points.x > 0. && points.y > 0.).then(|| Rect::from_min_size(Pos2::ZERO, points));
        let id = self.input.viewport_id;
        self.input
            .viewports
            .entry(id)
            .or_default()
            .native_pixels_per_point = Some(frame.native_pixels_per_point);
        self.input.take()
    }

    fn point(&self, x: f64, y: f64) -> Pos2 {
        Pos2::new(
            x as f32 / self.pixels_per_point,
            y as f32 / self.pixels_per_point,
        )
    }

    /// Translates one event. `wants` is egui's answer from its last frame.
    pub fn handle(&mut self, event: InputEvent, wants: &dyn Wants) -> Response {
        let repaint = |consumed: bool| Response {
            consumed,
            repaint: true,
        };
        match event {
            InputEvent::ScaleFactor(scale) => {
                let id = self.input.viewport_id;
                self.input
                    .viewports
                    .entry(id)
                    .or_default()
                    .native_pixels_per_point = Some(scale as f32);
                repaint(false)
            }
            InputEvent::Button { button, pressed } => {
                self.button(button, pressed);
                repaint(wants.pointer())
            }
            InputEvent::Wheel { delta, phase } => {
                self.wheel(delta, phase);
                repaint(wants.pointer())
            }
            InputEvent::CursorMoved { x, y } => {
                let pos = self.point(x, y);
                self.pointer = Some(pos);
                self.input.events.push(Event::PointerMoved(pos));
                repaint(wants.using_pointer())
            }
            InputEvent::CursorLeft => {
                self.pointer = None;
                self.input.events.push(Event::PointerGone);
                repaint(false)
            }
            InputEvent::Touch(touch) => {
                self.touch(touch);
                repaint(match touch.phase {
                    Phase::Started | Phase::Ended | Phase::Cancelled => wants.pointer(),
                    Phase::Moved => wants.using_pointer(),
                })
            }
            InputEvent::Ime(ime) => {
                self.ime(ime);
                repaint(wants.keyboard())
            }
            InputEvent::Key(key) => self.key_event(key, wants),
            InputEvent::Focused(focused) => {
                self.focus(focused);
                repaint(false)
            }
            InputEvent::Modifiers(state) => {
                self.set_modifiers(state);
                repaint(false)
            }
            InputEvent::Pinch { delta } => {
                self.input.events.push(Event::Zoom((delta as f32).exp()));
                repaint(wants.pointer())
            }
            InputEvent::Pan { dx, dy, phase } => {
                let delta = Vec2::new(dx as f32, dy as f32) / self.pixels_per_point;
                self.input.events.push(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta,
                    phase: touch_phase(phase),
                    modifiers: self.modifiers,
                });
                repaint(wants.pointer())
            }
            InputEvent::Repaint => repaint(false),
            InputEvent::Ignored => Response::default(),
        }
    }

    fn set_modifiers(&mut self, state: ModifierState) {
        let mac = self.platform.mac;
        self.modifiers = Modifiers {
            alt: state.alt,
            ctrl: state.ctrl,
            shift: state.shift,
            mac_cmd: mac && state.super_key,
            command: if mac { state.super_key } else { state.ctrl },
        };
        self.input
            .events
            .push(Event::ModifiersChanged(self.modifiers));
    }

    fn focus(&mut self, focused: bool) {
        self.input.focused = focused;
        if !focused {
            // The release of a key never arrives when the window loses the focus in the middle
            // of a shortcut (Alt+Tab): release everything here, so nothing stays "down".
            for (key, physical_key) in std::mem::take(&mut self.held) {
                self.input.events.push(Event::Key {
                    key,
                    physical_key,
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
            }
            self.modifiers = Modifiers::default();
            self.pointer_touch = None;
            self.ime_swallowed.clear();
        }
        self.input.events.push(Event::WindowFocused(focused));
    }

    fn button(&mut self, button: Button, pressed: bool) {
        let (Some(pos), Some(button)) = (self.pointer, pointer_button(button)) else {
            return;
        };
        self.input.events.push(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: self.modifiers,
        });
    }

    fn wheel(&mut self, delta: WheelDelta, phase: Phase) {
        let (unit, delta) = match delta {
            WheelDelta::Lines { x, y } => (egui::MouseWheelUnit::Line, Vec2::new(x, y)),
            WheelDelta::Pixels { x, y } => (
                egui::MouseWheelUnit::Point,
                Vec2::new(x as f32, y as f32) / self.pixels_per_point,
            ),
        };
        self.input.events.push(Event::MouseWheel {
            unit,
            delta,
            phase: touch_phase(phase),
            modifiers: self.modifiers,
        });
    }

    /// A finger: always a touch event; the first finger also drives the pointer, so that buttons
    /// and sliders work with a finger. The pointer vanishes when the finger lifts (no hover).
    fn touch(&mut self, touch: TouchInput) {
        let pos = self.point(touch.x, touch.y);
        self.input.events.push(Event::Touch {
            device_id: egui::TouchDeviceId(touch.device),
            id: egui::TouchId::from(touch.id),
            phase: touch_phase(touch.phase),
            pos,
            force: touch.force,
        });
        if self.pointer_touch.is_some_and(|id| id != touch.id) {
            return;
        }
        match touch.phase {
            Phase::Started => {
                self.pointer_touch = Some(touch.id);
                self.pointer = Some(pos);
                self.input.events.push(Event::PointerMoved(pos));
                self.button(Button::Left, true);
            }
            Phase::Moved => {
                self.pointer = Some(pos);
                self.input.events.push(Event::PointerMoved(pos));
            }
            Phase::Ended => {
                self.pointer_touch = None;
                self.button(Button::Left, false);
                self.pointer = None;
                self.input.events.push(Event::PointerGone);
            }
            Phase::Cancelled => {
                self.pointer_touch = None;
                self.pointer = None;
                self.input.events.push(Event::PointerGone);
            }
        }
    }

    fn ime(&mut self, ime: Ime) {
        match ime {
            // Enabled and Disabled mean different things on different systems and carry nothing egui needs.
            Ime::Enabled | Ime::Disabled => {}
            Ime::Preedit { text, cursor } => {
                let active_range_chars = cursor.and_then(|(from, to)| {
                    let (Some(before), Some(inside)) = (text.get(..from), text.get(from..to)) else {
                        log::warn!("ui-input: ignoring the cursor {from}..{to} of the preedit {text:?}: not on character boundaries");
                        return None;
                    };
                    let (start, len) = (before.chars().count(), inside.chars().count());
                    // The Korean IME of Windows reports the cursor at 0..0 during composition.
                    (!(self.platform.windows && start == 0 && len == 0)).then_some(start..start + len)
                });
                self.input.events.push(Event::Ime(egui::ImeEvent::Preedit {
                    text,
                    active_range_chars,
                }));
            }
            Ime::Commit(text) => self
                .input
                .events
                .push(Event::Ime(egui::ImeEvent::Commit(text))),
        }
    }

    fn key_event(&mut self, key: KeyInput, wants: &dyn Wants) -> Response {
        if key.synthetic && key.pressed {
            // Made up by the platform when the focus changes: no key was pressed.
            return Response {
                consumed: false,
                repaint: true,
            };
        }
        let wants_keyboard = wants.keyboard();
        if self.ime_allowed {
            // While an input method composes, the keys it swallows are neither keys nor text; the
            // release of a swallowed press must not surface as a key-up either.
            let swallowed = if key.pressed {
                key.ime_processed
                    .then(|| self.ime_swallowed.insert(key.physical))
                    .is_some()
            } else {
                self.ime_swallowed.remove(&key.physical)
            };
            if swallowed {
                return Response {
                    consumed: wants_keyboard,
                    repaint: false,
                };
            }
        }
        self.key(&key);
        // Tab moves egui's focus, so it is always egui's.
        Response {
            consumed: wants_keyboard || key.logical == Some(Key::Tab),
            repaint: true,
        }
    }

    fn key(&mut self, input: &KeyInput) {
        let m = self.modifiers;
        // The key the layout made; where the layout has no Latin letter (Russian, Greek) the key at
        // that place on a US keyboard, so shortcuts and clipboard commands work on every layout.
        if let Some(key) = input.logical.or(input.physical) {
            if input.pressed {
                let windows = self.platform.windows;
                if key == Key::Cut
                    || (m.command && key == Key::X)
                    || (windows && m.shift && key == Key::Delete)
                {
                    return self.input.events.push(Event::Cut);
                }
                if key == Key::Copy
                    || (m.command && key == Key::C)
                    || (windows && m.ctrl && key == Key::Insert)
                {
                    return self.input.events.push(Event::Copy);
                }
                if key == Key::Paste
                    || (m.command && key == Key::V)
                    || (windows && m.shift && key == Key::Insert)
                {
                    if let Some(text) = self
                        .clipboard
                        .get()
                        .map(|t| t.replace("\r\n", "\n"))
                        .filter(|t| !t.is_empty())
                    {
                        self.input.events.push(Event::Paste(text));
                    }
                    return;
                }
                if !self.held.iter().any(|&(k, _)| k == key) {
                    self.held.push((key, input.physical));
                }
            } else {
                self.held.retain(|&(k, _)| k != key);
            }
            self.input.events.push(Event::Key {
                key,
                physical_key: input.physical,
                pressed: input.pressed,
                repeat: input.repeat,
                modifiers: m,
            });
        }
        if let Some(text) = input.text.as_deref() {
            // Special keys can arrive as private-use characters (Delete on a Mac) or control
            // characters (Enter, Tab): those are keys, not text. Neither is the character a
            // shortcut leaves behind (Ctrl+W).
            let typed = !text.is_empty() && text.chars().all(printable);
            let shortcut = m.ctrl || m.command || m.mac_cmd;
            if typed && input.pressed && !shortcut {
                self.input.events.push(Event::Text(text.to_owned()));
            }
        }
    }
}

fn printable(c: char) -> bool {
    let private_use = ('\u{e000}'..='\u{f8ff}').contains(&c)
        || ('\u{f0000}'..='\u{ffffd}').contains(&c)
        || ('\u{100000}'..='\u{10fffd}').contains(&c);
    !private_use && !c.is_ascii_control()
}

fn touch_phase(phase: Phase) -> egui::TouchPhase {
    match phase {
        Phase::Started => egui::TouchPhase::Start,
        Phase::Moved => egui::TouchPhase::Move,
        Phase::Ended => egui::TouchPhase::End,
        Phase::Cancelled => egui::TouchPhase::Cancel,
    }
}

fn pointer_button(button: Button) -> Option<PointerButton> {
    Some(match button {
        Button::Left => PointerButton::Primary,
        Button::Right => PointerButton::Secondary,
        Button::Middle => PointerButton::Middle,
        Button::Back => PointerButton::Extra1,
        Button::Forward => PointerButton::Extra2,
        Button::Other => return None,
    })
}

#[cfg(test)]
mod tests;
