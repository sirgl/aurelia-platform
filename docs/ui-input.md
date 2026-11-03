# ui-input: the own glue between winit and egui (decided: write it ourselves, minimal)

**The game no longer uses `egui-winit`: `platform/crates/ui-input` (MIT OR Apache-2.0, `publish = true` candidate) translates winit window events into egui input and applies egui's output to the window, on desktop, Android and in the browser; the patched copy `platform/third_party/egui-winit` and the `[patch]` entries are gone.** State of 21 October 2026, branch `refactor/own-egui-glue`. Live keyboard, mouse and touch input has NOT been tried on a real window or phone (rules: no game window opened); section 5 is the checklist for that.

## 1. Why

egui draws from an `egui::RawInput` and answers with an `egui::PlatformOutput`; `egui-winit` 0.36.2 was the translator. It does not build for wasm32 (its dropped-file type needs the native `bytes`), so the repository carried a patched copy plus `[patch.crates-io]` in four workspace roots. That blocked publishing `render`/`client` (a published crate cannot depend on a path-patched fork) and brought the copy's licences. Roman's decision: write the small layer ourselves.

## 2. What the game really uses (measured by reading every use site)

| Use | Where | Needed |
|---|---|---|
| `State::new(ctx, viewport, window, scale, None, None)`, `on_window_event`, `take_egui_input`, `handle_platform_output` | `launcher-core` (desktop, web page, Android host), `gallery`, `classic-desktop` visual sandbox | yes, nothing else of the API |
| `EventResponse.consumed` / `.repaint` | all launchers (consumed: a click on a panel must not move the map; repaint: ask a frame) | yes |
| pixels per point (scale factor times egui's zoom factor; the shell sets the zoom factor on the `Context`, the glue reads it every event) | pointer, wheel and screen size conversions | yes |
| clipboard (copy, paste, Ctrl+C/X/V) | text fields | yes |
| cursor icons | hover over buttons, text | yes |
| links (`OpenUrl`) | HUD links | yes |
| IME + on-screen keyboard (`set_ime_allowed`, purpose, area) | text fields (names, seeds, chat) | yes |
| key translation with the physical-key fallback | shortcuts on Russian and AZERTY layouts (`platform/docs/shortcuts.md`) | yes |
| touch to pointer; modifiers; focus; scroll units | phone/tablet mode, desktop | yes |
| dropped files, AccessKit, multi-viewport commands, window settings, custom cursor bitmaps, iOS safe area, `simulate_touch_screen`, `on_mouse_motion` | nobody | no |

The multi-touch gestures of the map (tap, long press, drag-pan, two-finger pinch zoom) are NOT in egui: the launchers feed raw winit `Touch` events to `client::shell::touch` themselves; the glue only gives egui the same fingers as `Event::Touch` and moves the pointer with the first one.

## 3. Shape

* `event` / `translate` / `output` / `clipboard`: the core. Plain structs (`InputEvent`, `KeyInput`, `TouchInput`, ...); no winit. `Translator::handle(event, &impl Wants) -> Response`, `take_input(FrameInfo) -> RawInput`, `apply_output(.., &mut dyn Host)`. `Wants` is egui's answer ("would egui take this click?"), `Host` is what the window can do (cursor, IME, link): both implemented by stubs in the tests.
* `winit_glue` (feature `winit`): `convert(&WindowEvent) -> InputEvent`, the key and cursor tables, `State` (the drop-in shape: `new(ctx, scale)`, `on_window_event`, `take_egui_input`, `handle_platform_output`).
* Clipboard: desktop `arboard` (no image data, no Wayland data-control feature: a pure-Wayland session without XWayland keeps the copy inside the program; the stock glue used `smithay-clipboard` there); Android and web keep the text inside the program; the web also writes `navigator.clipboard.writeText` (best effort). A browser page cannot read the clipboard without a paste event, which winit's web backend does not deliver, so pasting text from another tab is not supported.
* Links: the `webbrowser` crate on desktop, Android and the web (`window.open`).

## 4. Intentional differences from `egui-winit` 0.36.2

* **Key repeat is passed on** (`KeyEvent.repeat`); the stock glue always said `repeat: false` and let egui compute it. `keymap::pressed` ignores repeats, so a held shortcut key (found-city, next-city on an arrow key) now fires once; camera panning with held keys reads `key_down` and is unaffected.
* Focus loss emits a key-up for every key still held (egui also clears them itself), clears modifiers and drops the finger that drove the pointer.
* Keys that an input method swallowed (winit's `Process` key) are dropped on every platform while an input method is allowed (stock: Windows only).
* Not translated: files, rotation gesture (macOS/iOS only; a frame is requested), theme changes, iOS safe area.

## 5. Manual checklist (NOT verified: nobody has used a real keyboard, mouse or finger on this build)

Desktop (`aurelia-lands`, `aurelia-classic`):
1. Move the mouse over a button: the arrow becomes a hand; over a text field: an I-beam; leave the window and come back: it is set again.
2. Left-click a panel button (does not select a tile behind it); left-click a tile (selects); right-click a tile (secondary action); drag with the left button (map pans); wheel zooms the map; wheel over a scrolling list scrolls it and does not zoom.
3. Hold a letter key that is a shortcut (B, found city) once: one action, not repeated.
4. Switch to a Cyrillic layout: the key at the place of B founds a city, F6 opens the technology screen, Enter confirms; switch to French AZERTY: the key labelled A is A.
5. Text fields (new game seed, city rename): type Latin and Cyrillic letters, Backspace, arrows, Home/End, Ctrl+A, Ctrl+C then Ctrl+V into another field, Ctrl+X; shortcuts do not fire while typing; Alt+Tab away while holding Ctrl, come back: Ctrl is not stuck.
6. A link in the game opens the system browser. Window resize and moving between monitors of different scale keep the clicks on target.
7. An input method (fcitx/ibus Chinese or Japanese) in a text field: the preedit text shows and the candidate window is next to the field.

Web (`lands/launcher/web`, `classic/launcher/web`, built by `platform/tools/launcher/web-build.sh`): the same mouse, wheel and text checks in a browser; Ctrl+C in a field then Ctrl+V pastes the text inside the page; the browser clipboard receives the copied text.

Phone (Android APK, or the web launcher in mobile mode): tap a button, tap a tile, long press (secondary), one-finger drag pans, two fingers pinch-zoom the map; touching a panel never moves the map; the keyboard opens for a text field and typing, Backspace and Enter work; the Back button closes the open panel.

## 6. Tests

`cargo test --release -p aurelia-ui-input` (unit tests of the core and of the winit conversions). `integration/crates/ui-input-diff` compares the glue event for event with the stock `egui-winit` 0.36.2 (pointer, buttons, wheel, modifiers, touch, IME, focus, gestures); it needs a real hidden window, so it is `#[ignore]`d and opt-in (`cargo test --release -p ui-input-diff -- --ignored`, under Xvfb or on a desktop session). Keyboard events cannot be constructed outside winit (`KeyEvent` has a private field), so keys are covered by the core tests with plain `KeyInput` values and by the table tests on winit's `Key`/`KeyCode`.
