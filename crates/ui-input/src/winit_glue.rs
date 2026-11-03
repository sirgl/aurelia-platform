//! The thin winit adapter: `winit::event::WindowEvent` to [`InputEvent`], egui's output to a
//! `winit::window::Window`, and [`State`], the one object a launcher holds per window.
//!
//! Works on every target winit works on: desktop, Android (`android-native-activity`) and the
//! browser canvas (wasm32). Nothing here needs a display to be *compiled*; only
//! [`State`] needs a window to be *used*.
use egui::{CursorIcon, IMEPurpose, Key, Rect};
use winit::event::{
    ElementState, Force, Ime as WinitIme, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use winit::keyboard::{Key as WinitKey, KeyCode, NamedKey, PhysicalKey};
use winit::window::{CursorIcon as WinitCursor, ImePurpose, Window};

use crate::event::{
    Button, Ime, InputEvent, KeyInput, ModifierState, Phase, TouchInput, WheelDelta,
};
use crate::output::{Host, OutputState};
use crate::translate::{FrameInfo, Response, Translator};

/// The answer to [`State::on_window_event`].
pub type EventResponse = Response;

/// Everything the launcher of one window needs: create it once, feed it the window's events,
/// take the input before a frame, hand it the frame's output.
pub struct State {
    ctx: egui::Context,
    core: Translator,
    output: OutputState,
    start: web_time::Instant,
}

impl State {
    /// `native_pixels_per_point` is the window's scale factor.
    pub fn new(ctx: egui::Context, native_pixels_per_point: f32) -> Self {
        let start = web_time::Instant::now()
            .checked_sub(web_time::Duration::from_secs_f64(ctx.time()))
            .unwrap_or_else(web_time::Instant::now);
        Self {
            ctx,
            core: Translator::new(native_pixels_per_point),
            output: OutputState::default(),
            start,
        }
    }

    /// The translation core, for a test or an extra clipboard operation.
    pub fn core_mut(&mut self) -> &mut Translator {
        &mut self.core
    }

    /// Pixels per egui point: the window's scale factor times egui's zoom factor.
    fn pixels_per_point(&self, window: &Window) -> f32 {
        self.ctx.zoom_factor() * window.scale_factor() as f32
    }

    /// Feed every event of the window. Events for other windows must not come here.
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> EventResponse {
        self.core
            .set_pixels_per_point(self.pixels_per_point(window));
        let mut plain = convert(event);
        if let (InputEvent::Focused(focused), true) = (&mut plain, cfg!(target_os = "macos")) {
            // winit reports a stale focus flag on macOS (winit issue 4371): ask the window.
            *focused = window.has_focus();
        }
        self.core.handle(plain, &self.ctx)
    }

    /// The input of the frame about to run: the events since the last frame, the time and the
    /// screen size.
    pub fn take_egui_input(&mut self, window: &Window) -> egui::RawInput {
        let size = window.inner_size();
        let native = window.scale_factor() as f32;
        let ppp = self.pixels_per_point(window);
        self.core.set_pixels_per_point(ppp);
        self.core.take_input(FrameInfo {
            size_px: (size.width as f32, size.height as f32),
            time: self.start.elapsed().as_secs_f64(),
            pixels_per_point: ppp,
            native_pixels_per_point: native,
        })
    }

    /// Applies what egui wants from the window after a frame: clipboard, links, cursor, IME.
    pub fn handle_platform_output(&mut self, window: &Window, output: egui::PlatformOutput) {
        let ppp = self.pixels_per_point(window);
        let had_input = self.ctx.input(|i| !i.events.is_empty());
        self.core.apply_output(
            &mut self.output,
            output,
            ppp,
            had_input,
            &mut WindowHost(window),
        );
    }
}

/// A window as a [`Host`].
struct WindowHost<'a>(&'a Window);

impl Host for WindowHost<'_> {
    fn set_cursor(&mut self, icon: Option<CursorIcon>) {
        match icon.map(winit_cursor) {
            Some(cursor) => {
                self.0.set_cursor_visible(true);
                self.0.set_cursor(cursor);
            }
            None => self.0.set_cursor_visible(false),
        }
    }

    fn set_ime_allowed(&mut self, allowed: bool) {
        self.0.set_ime_allowed(allowed);
    }

    fn set_ime_purpose(&mut self, purpose: IMEPurpose) {
        self.0.set_ime_purpose(match purpose {
            IMEPurpose::Password => ImePurpose::Password,
            IMEPurpose::Terminal => ImePurpose::Terminal,
            IMEPurpose::Normal => ImePurpose::Normal,
        });
    }

    fn set_ime_area(&mut self, area: Rect) {
        self.0.set_ime_cursor_area(
            winit::dpi::PhysicalPosition::new(area.min.x, area.min.y),
            winit::dpi::PhysicalSize::new(area.width(), area.height()),
        );
    }

    fn open_url(&mut self, url: &str, new_tab: bool) {
        crate::links::open(url, new_tab);
    }
}

// ---------------------------------------------------------------------------------------------
// WindowEvent -> InputEvent

/// The plain event for a winit window event.
pub fn convert(event: &WindowEvent) -> InputEvent {
    match event {
        WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
            InputEvent::ScaleFactor(*scale_factor)
        }
        WindowEvent::MouseInput { state, button, .. } => InputEvent::Button {
            button: button_of(*button),
            pressed: *state == ElementState::Pressed,
        },
        WindowEvent::MouseWheel { delta, phase, .. } => InputEvent::Wheel {
            delta: wheel_of(*delta),
            phase: phase_of(*phase),
        },
        WindowEvent::CursorMoved { position, .. } => InputEvent::CursorMoved {
            x: position.x,
            y: position.y,
        },
        WindowEvent::CursorLeft { .. } => InputEvent::CursorLeft,
        WindowEvent::Touch(touch) => InputEvent::Touch(TouchInput {
            device: egui::epaint::util::hash(touch.device_id),
            id: touch.id,
            phase: phase_of(touch.phase),
            x: touch.location.x,
            y: touch.location.y,
            force: touch.force.map(|force| match force {
                Force::Normalized(force) => force as f32,
                Force::Calibrated {
                    force,
                    max_possible_force,
                    ..
                } => (force / max_possible_force) as f32,
            }),
        }),
        WindowEvent::Ime(ime) => InputEvent::Ime(match ime {
            WinitIme::Enabled => Ime::Enabled,
            WinitIme::Disabled => Ime::Disabled,
            WinitIme::Preedit(text, cursor) => Ime::Preedit {
                text: text.clone(),
                cursor: *cursor,
            },
            WinitIme::Commit(text) => Ime::Commit(text.clone()),
        }),
        WindowEvent::KeyboardInput {
            event,
            is_synthetic,
            ..
        } => InputEvent::Key(key_of(
            &event.logical_key,
            event.physical_key,
            event.text.as_deref(),
            event.state == ElementState::Pressed,
            event.repeat,
            *is_synthetic,
        )),
        WindowEvent::Focused(focused) => InputEvent::Focused(*focused),
        WindowEvent::ModifiersChanged(modifiers) => {
            let state = modifiers.state();
            InputEvent::Modifiers(ModifierState {
                shift: state.shift_key(),
                ctrl: state.control_key(),
                alt: state.alt_key(),
                super_key: state.super_key(),
            })
        }
        WindowEvent::PinchGesture { delta, .. } => InputEvent::Pinch { delta: *delta },
        WindowEvent::PanGesture { delta, phase, .. } => InputEvent::Pan {
            dx: delta.x as f64,
            dy: delta.y as f64,
            phase: phase_of(*phase),
        },
        WindowEvent::AxisMotion { .. }
        | WindowEvent::DoubleTapGesture { .. }
        | WindowEvent::ActivationTokenDone { .. } => InputEvent::Ignored,
        // Resized, Moved, Occluded, CursorEntered, CloseRequested, Destroyed, the rotation gesture,
        // file hovering (not supported): a frame, nothing more.
        _ => InputEvent::Repaint,
    }
}

/// The plain key event for what a `KeyEvent` carries (a `KeyEvent` cannot be built outside winit,
/// so the fields are separate: tests build them).
pub fn key_of(
    logical: &WinitKey,
    physical: PhysicalKey,
    text: Option<&str>,
    pressed: bool,
    repeat: bool,
    synthetic: bool,
) -> KeyInput {
    let physical = match physical {
        PhysicalKey::Code(code) => key_from_code(code),
        PhysicalKey::Unidentified(_) => None,
    };
    KeyInput {
        logical: key_from_logical(logical),
        physical,
        text: text.or_else(|| logical.to_text()).map(str::to_owned),
        pressed,
        repeat,
        synthetic,
        ime_processed: *logical == WinitKey::Named(NamedKey::Process),
    }
}

fn button_of(button: MouseButton) -> Button {
    match button {
        MouseButton::Left => Button::Left,
        MouseButton::Right => Button::Right,
        MouseButton::Middle => Button::Middle,
        MouseButton::Back => Button::Back,
        MouseButton::Forward => Button::Forward,
        MouseButton::Other(_) => Button::Other,
    }
}

fn phase_of(phase: TouchPhase) -> Phase {
    match phase {
        TouchPhase::Started => Phase::Started,
        TouchPhase::Moved => Phase::Moved,
        TouchPhase::Ended => Phase::Ended,
        TouchPhase::Cancelled => Phase::Cancelled,
    }
}

fn wheel_of(delta: MouseScrollDelta) -> WheelDelta {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => WheelDelta::Lines { x, y },
        MouseScrollDelta::PixelDelta(p) => WheelDelta::Pixels { x: p.x, y: p.y },
    }
}

// ---------------------------------------------------------------------------------------------
// Keys

/// egui's key for the key the layout produced: a named key, or a character egui knows by name
/// (letters, digits, punctuation). A Cyrillic letter has no egui key: `None`.
pub fn key_from_logical(key: &WinitKey) -> Option<Key> {
    match key {
        WinitKey::Named(named) => key_from_named(*named),
        WinitKey::Character(text) => Key::from_name(text.as_str()),
        WinitKey::Unidentified(_) | WinitKey::Dead(_) => None,
    }
}

fn key_from_named(key: NamedKey) -> Option<Key> {
    use NamedKey as N;
    Some(match key {
        N::Enter => Key::Enter,
        N::Tab => Key::Tab,
        N::Space => Key::Space,
        N::Escape => Key::Escape,
        N::Backspace => Key::Backspace,
        N::Delete => Key::Delete,
        N::Insert => Key::Insert,
        N::Home => Key::Home,
        N::End => Key::End,
        N::PageUp => Key::PageUp,
        N::PageDown => Key::PageDown,
        N::ArrowUp => Key::ArrowUp,
        N::ArrowDown => Key::ArrowDown,
        N::ArrowLeft => Key::ArrowLeft,
        N::ArrowRight => Key::ArrowRight,
        N::Cut => Key::Cut,
        N::Copy => Key::Copy,
        N::Paste => Key::Paste,
        N::BrowserBack => Key::BrowserBack,
        other => return function_key(other),
    })
}

/// F1 to F35 by name.
fn function_key(key: NamedKey) -> Option<Key> {
    use NamedKey as N;
    const F: [(NamedKey, Key); 35] = [
        (N::F1, Key::F1),
        (N::F2, Key::F2),
        (N::F3, Key::F3),
        (N::F4, Key::F4),
        (N::F5, Key::F5),
        (N::F6, Key::F6),
        (N::F7, Key::F7),
        (N::F8, Key::F8),
        (N::F9, Key::F9),
        (N::F10, Key::F10),
        (N::F11, Key::F11),
        (N::F12, Key::F12),
        (N::F13, Key::F13),
        (N::F14, Key::F14),
        (N::F15, Key::F15),
        (N::F16, Key::F16),
        (N::F17, Key::F17),
        (N::F18, Key::F18),
        (N::F19, Key::F19),
        (N::F20, Key::F20),
        (N::F21, Key::F21),
        (N::F22, Key::F22),
        (N::F23, Key::F23),
        (N::F24, Key::F24),
        (N::F25, Key::F25),
        (N::F26, Key::F26),
        (N::F27, Key::F27),
        (N::F28, Key::F28),
        (N::F29, Key::F29),
        (N::F30, Key::F30),
        (N::F31, Key::F31),
        (N::F32, Key::F32),
        (N::F33, Key::F33),
        (N::F34, Key::F34),
        (N::F35, Key::F35),
    ];
    F.iter()
        .find(|(named, _)| *named == key)
        .map(|&(_, key)| key)
}

/// egui's key for the key at a place on the keyboard, whatever the layout (the numpad counts as
/// the main block).
pub fn key_from_code(code: KeyCode) -> Option<Key> {
    use KeyCode as C;
    const LETTERS: [(KeyCode, Key); 26] = [
        (C::KeyA, Key::A),
        (C::KeyB, Key::B),
        (C::KeyC, Key::C),
        (C::KeyD, Key::D),
        (C::KeyE, Key::E),
        (C::KeyF, Key::F),
        (C::KeyG, Key::G),
        (C::KeyH, Key::H),
        (C::KeyI, Key::I),
        (C::KeyJ, Key::J),
        (C::KeyK, Key::K),
        (C::KeyL, Key::L),
        (C::KeyM, Key::M),
        (C::KeyN, Key::N),
        (C::KeyO, Key::O),
        (C::KeyP, Key::P),
        (C::KeyQ, Key::Q),
        (C::KeyR, Key::R),
        (C::KeyS, Key::S),
        (C::KeyT, Key::T),
        (C::KeyU, Key::U),
        (C::KeyV, Key::V),
        (C::KeyW, Key::W),
        (C::KeyX, Key::X),
        (C::KeyY, Key::Y),
        (C::KeyZ, Key::Z),
    ];
    const DIGITS: [(KeyCode, KeyCode, Key); 10] = [
        (C::Digit0, C::Numpad0, Key::Num0),
        (C::Digit1, C::Numpad1, Key::Num1),
        (C::Digit2, C::Numpad2, Key::Num2),
        (C::Digit3, C::Numpad3, Key::Num3),
        (C::Digit4, C::Numpad4, Key::Num4),
        (C::Digit5, C::Numpad5, Key::Num5),
        (C::Digit6, C::Numpad6, Key::Num6),
        (C::Digit7, C::Numpad7, Key::Num7),
        (C::Digit8, C::Numpad8, Key::Num8),
        (C::Digit9, C::Numpad9, Key::Num9),
    ];
    if let Some(&(_, key)) = LETTERS.iter().find(|(c, _)| *c == code) {
        return Some(key);
    }
    if let Some(&(_, _, key)) = DIGITS.iter().find(|(d, n, _)| *d == code || *n == code) {
        return Some(key);
    }
    if let Some(key) = function_key_code(code) {
        return Some(key);
    }
    Some(match code {
        C::ArrowUp => Key::ArrowUp,
        C::ArrowDown => Key::ArrowDown,
        C::ArrowLeft => Key::ArrowLeft,
        C::ArrowRight => Key::ArrowRight,
        C::Escape => Key::Escape,
        C::Tab => Key::Tab,
        C::Backspace => Key::Backspace,
        C::Enter | C::NumpadEnter => Key::Enter,
        C::Insert => Key::Insert,
        C::Delete => Key::Delete,
        C::Home => Key::Home,
        C::End => Key::End,
        C::PageUp => Key::PageUp,
        C::PageDown => Key::PageDown,
        C::Space => Key::Space,
        C::Comma => Key::Comma,
        C::Period => Key::Period,
        C::Semicolon => Key::Semicolon,
        C::Backslash => Key::Backslash,
        C::Slash | C::NumpadDivide => Key::Slash,
        C::BracketLeft => Key::OpenBracket,
        C::BracketRight => Key::CloseBracket,
        C::Backquote => Key::Backtick,
        C::Quote => Key::Quote,
        C::Minus | C::NumpadSubtract => Key::Minus,
        C::NumpadAdd => Key::Plus,
        C::Equal => Key::Equals,
        C::Cut => Key::Cut,
        C::Copy => Key::Copy,
        C::Paste => Key::Paste,
        C::ShiftLeft => Key::ShiftLeft,
        C::ShiftRight => Key::ShiftRight,
        C::ControlLeft => Key::ControlLeft,
        C::ControlRight => Key::ControlRight,
        C::AltLeft => Key::AltLeft,
        C::AltRight => Key::AltRight,
        C::SuperLeft => Key::SuperLeft,
        C::SuperRight => Key::SuperRight,
        // The extra key of an ISO keyboard: `<>|` on AZERTY, `\|` on a British keyboard.
        C::IntlBackslash => Key::IntlBackslash,
        _ => return None,
    })
}

fn function_key_code(code: KeyCode) -> Option<Key> {
    use KeyCode as C;
    const F: [(KeyCode, Key); 35] = [
        (C::F1, Key::F1),
        (C::F2, Key::F2),
        (C::F3, Key::F3),
        (C::F4, Key::F4),
        (C::F5, Key::F5),
        (C::F6, Key::F6),
        (C::F7, Key::F7),
        (C::F8, Key::F8),
        (C::F9, Key::F9),
        (C::F10, Key::F10),
        (C::F11, Key::F11),
        (C::F12, Key::F12),
        (C::F13, Key::F13),
        (C::F14, Key::F14),
        (C::F15, Key::F15),
        (C::F16, Key::F16),
        (C::F17, Key::F17),
        (C::F18, Key::F18),
        (C::F19, Key::F19),
        (C::F20, Key::F20),
        (C::F21, Key::F21),
        (C::F22, Key::F22),
        (C::F23, Key::F23),
        (C::F24, Key::F24),
        (C::F25, Key::F25),
        (C::F26, Key::F26),
        (C::F27, Key::F27),
        (C::F28, Key::F28),
        (C::F29, Key::F29),
        (C::F30, Key::F30),
        (C::F31, Key::F31),
        (C::F32, Key::F32),
        (C::F33, Key::F33),
        (C::F34, Key::F34),
        (C::F35, Key::F35),
    ];
    F.iter().find(|(c, _)| *c == code).map(|&(_, key)| key)
}

// ---------------------------------------------------------------------------------------------
// Cursors

/// The winit cursor for an egui cursor icon. `CursorIcon::None` has no cursor (the host hides it);
/// it maps to the default arrow here only so the function is total.
pub fn winit_cursor(icon: CursorIcon) -> WinitCursor {
    use CursorIcon as E;
    match icon {
        E::None | E::Default => WinitCursor::Default,
        E::ContextMenu => WinitCursor::ContextMenu,
        E::Help => WinitCursor::Help,
        E::PointingHand => WinitCursor::Pointer,
        E::Progress => WinitCursor::Progress,
        E::Wait => WinitCursor::Wait,
        E::Cell => WinitCursor::Cell,
        E::Crosshair => WinitCursor::Crosshair,
        E::Text => WinitCursor::Text,
        E::VerticalText => WinitCursor::VerticalText,
        E::Alias => WinitCursor::Alias,
        E::Copy => WinitCursor::Copy,
        E::Move => WinitCursor::Move,
        E::NoDrop => WinitCursor::NoDrop,
        E::NotAllowed => WinitCursor::NotAllowed,
        E::Grab => WinitCursor::Grab,
        E::Grabbing => WinitCursor::Grabbing,
        E::AllScroll => WinitCursor::AllScroll,
        E::ResizeHorizontal => WinitCursor::EwResize,
        E::ResizeNeSw => WinitCursor::NeswResize,
        E::ResizeNwSe => WinitCursor::NwseResize,
        E::ResizeVertical => WinitCursor::NsResize,
        E::ResizeEast => WinitCursor::EResize,
        E::ResizeSouthEast => WinitCursor::SeResize,
        E::ResizeSouth => WinitCursor::SResize,
        E::ResizeSouthWest => WinitCursor::SwResize,
        E::ResizeWest => WinitCursor::WResize,
        E::ResizeNorthWest => WinitCursor::NwResize,
        E::ResizeNorth => WinitCursor::NResize,
        E::ResizeNorthEast => WinitCursor::NeResize,
        E::ResizeColumn => WinitCursor::ColResize,
        E::ResizeRow => WinitCursor::RowResize,
        E::ZoomIn => WinitCursor::ZoomIn,
        E::ZoomOut => WinitCursor::ZoomOut,
    }
}

#[cfg(test)]
mod tests;
