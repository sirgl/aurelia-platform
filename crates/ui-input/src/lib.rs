//! `ui-input`: the glue between a winit window and egui, written for this game and small on purpose.
//!
//! egui draws a user interface from an `egui::RawInput` (pointer, keys, text, touches) and answers
//! with an `egui::PlatformOutput` (a cursor icon, text to copy, a link to open, the input method's
//! needs). Somebody has to translate between the window system and those two; for winit that was
//! the `egui-winit` crate, which does not build for wasm32 (version 0.36.2) and brings drag-and-drop,
//! accessibility, multi-window and macOS/iOS code we never use. This crate keeps the subset a
//! single-window game needs, on desktop, Android and in the browser:
//!
//! * pointer, buttons, wheel (lines and pixels) and trackpad gestures, with the display's scale;
//! * touch: every finger is an `egui::Event::Touch`, the first one also moves the pointer;
//! * keys: the key the layout produced, or the key's place where the layout has no Latin letter
//!   (a Russian keyboard still shortcuts like a US one), key repeat, Ctrl/Cmd, clipboard keys;
//! * text and the input method (preedit and commit), on-screen keyboards;
//! * focus loss releases held keys and modifiers;
//! * the cursor icon, the clipboard, links, the IME area and purpose.
//!
//! # Layers
//!
//! * [`event`]: plain structs and enums for input; no winit.
//! * [`translate`]: [`Translator`], the core: events in, `RawInput` out. [`clipboard`] and
//!   [`output`] (the `Host` trait, the answer to egui's `PlatformOutput`) complete it. Everything
//!   up to here is testable without a window.
//! * `winit_glue` (feature `winit`, default): `WindowEvent` to plain events, the cursor and key
//!   tables, and [`State`], the object a launcher holds.
//!
//! ```text
//! let mut input = ui_input::State::new(ctx.clone(), window.scale_factor() as f32);
//! // in window_event:
//! let response = input.on_window_event(&window, &event);   // response.consumed: egui's, not the game's
//! // before a frame:
//! let raw = input.take_egui_input(&window);
//! let output = ctx.run_ui(raw, |ui| { /* ... */ });
//! input.handle_platform_output(&window, output.platform_output);
//! ```
pub mod clipboard;
pub mod event;
pub mod links;
pub mod output;
pub mod translate;

#[cfg(feature = "winit")]
pub mod winit_glue;

pub use clipboard::{Clipboard, MemoryClipboard, SystemClipboard};
pub use event::{Button, Ime, InputEvent, KeyInput, ModifierState, Phase, TouchInput, WheelDelta};
pub use output::{Host, OutputState};
pub use translate::{FrameInfo, Platform, Response, Translator, Wants};
#[cfg(feature = "winit")]
pub use winit_glue::{EventResponse, State};
