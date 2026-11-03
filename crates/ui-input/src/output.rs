//! egui's answer to the window: `egui::PlatformOutput` applied through a [`Host`].
//!
//! The [`Host`] is what the window can do (change the cursor, switch the input method on, open
//! a link). The winit adapter implements it for a `winit::window::Window`; a test records the calls.
use egui::{CursorIcon, IMEPurpose, OutputCommand, PlatformOutput, Rect};

use crate::translate::Translator;

/// What the window can do for egui.
pub trait Host {
    /// Shows this cursor, or hides it (`None`: egui asked for `CursorIcon::None`).
    fn set_cursor(&mut self, icon: Option<CursorIcon>);
    /// Switches the input method (the on-screen keyboard, a CJK IME) on or off.
    fn set_ime_allowed(&mut self, allowed: bool);
    fn set_ime_purpose(&mut self, purpose: IMEPurpose);
    /// Where the text field is, in physical pixels: the IME places its candidate window there.
    fn set_ime_area(&mut self, area: Rect);
    /// Opens a link in the system browser; `new_tab` matters on the web only.
    fn open_url(&mut self, url: &str, new_tab: bool);
}

/// What the window was told last, so that an unchanged answer costs nothing (and a cursor does
/// not flicker while the window system is resizing it).
#[derive(Debug, Default)]
pub struct OutputState {
    cursor: Option<CursorIcon>,
    ime_area: Option<Rect>,
    ime_purpose: Option<IMEPurpose>,
}

impl Translator {
    /// Applies the output of a frame: copies text, opens links, sets the cursor, drives the input
    /// method. `pixels_per_point` converts egui points to the pixels the window uses;
    /// `had_input` is whether egui saw any event this frame (the IME area is sent again then).
    pub fn apply_output(
        &mut self,
        state: &mut OutputState,
        output: PlatformOutput,
        pixels_per_point: f32,
        had_input: bool,
        host: &mut dyn Host,
    ) {
        for command in output.commands {
            match command {
                OutputCommand::CopyText(text) => self.clipboard().set(text),
                OutputCommand::CopyImage(_) => {
                    log::debug!("ui-input: copying images is not supported")
                }
                OutputCommand::OpenUrl(link) => host.open_url(&link.url, link.new_tab),
            }
        }
        self.apply_cursor(state, output.cursor_icon, host);

        let allowed = output.ime.is_some();
        let toggling = self.ime_allowed != allowed;
        if toggling {
            self.ime_allowed = allowed;
            host.set_ime_allowed(allowed);
        }
        let Some(ime) = output.ime else {
            state.ime_area = None;
            return;
        };
        if !toggling && ime.should_interrupt_composition {
            // The only way to end a composition from outside: switch the input method off and on.
            host.set_ime_allowed(false);
            host.set_ime_allowed(true);
        }
        if state.ime_purpose != Some(ime.purpose) {
            state.ime_purpose = Some(ime.purpose);
            host.set_ime_purpose(ime.purpose);
        }
        let area = Rect::from_min_max(
            ime.rect.min * pixels_per_point,
            ime.rect.max * pixels_per_point,
        );
        if state.ime_area != Some(area) || had_input {
            state.ime_area = Some(area);
            host.set_ime_area(area);
        }
    }

    /// The cursor is only the window's while the pointer is inside it; leaving forgets the last
    /// icon, so the next frame inside sets it again.
    fn apply_cursor(&mut self, state: &mut OutputState, icon: CursorIcon, host: &mut dyn Host) {
        if !self.pointer_in_window() {
            state.cursor = None;
        } else if state.cursor != Some(icon) {
            state.cursor = Some(icon);
            host.set_cursor((icon != CursorIcon::None).then_some(icon));
        }
    }
}

#[cfg(test)]
mod tests;
