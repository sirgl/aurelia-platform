use super::*;
use egui::Key;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{DeviceId, Modifiers, Touch};
use winit::keyboard::{ModifiersState, SmolStr};

fn character(text: &str) -> WinitKey {
    WinitKey::Character(SmolStr::new(text))
}

#[test]
fn mouse_events_convert() {
    let device_id = DeviceId::dummy();
    assert_eq!(
        convert(&WindowEvent::CursorMoved {
            device_id,
            position: PhysicalPosition::new(3.5, 4.0)
        }),
        InputEvent::CursorMoved { x: 3.5, y: 4.0 }
    );
    assert_eq!(
        convert(&WindowEvent::CursorLeft { device_id }),
        InputEvent::CursorLeft
    );
    for (winit, plain) in [
        (MouseButton::Left, Button::Left),
        (MouseButton::Right, Button::Right),
        (MouseButton::Middle, Button::Middle),
        (MouseButton::Back, Button::Back),
        (MouseButton::Forward, Button::Forward),
        (MouseButton::Other(9), Button::Other),
    ] {
        let event = WindowEvent::MouseInput {
            device_id,
            state: ElementState::Pressed,
            button: winit,
        };
        assert_eq!(
            convert(&event),
            InputEvent::Button {
                button: plain,
                pressed: true
            }
        );
    }
    let released = WindowEvent::MouseInput {
        device_id,
        state: ElementState::Released,
        button: MouseButton::Left,
    };
    assert_eq!(
        convert(&released),
        InputEvent::Button {
            button: Button::Left,
            pressed: false
        }
    );
}

#[test]
fn wheel_units_survive_the_conversion() {
    let device_id = DeviceId::dummy();
    let lines = WindowEvent::MouseWheel {
        device_id,
        delta: MouseScrollDelta::LineDelta(1.0, -2.0),
        phase: TouchPhase::Moved,
    };
    assert_eq!(
        convert(&lines),
        InputEvent::Wheel {
            delta: WheelDelta::Lines { x: 1.0, y: -2.0 },
            phase: Phase::Moved
        }
    );
    let pixels = WindowEvent::MouseWheel {
        device_id,
        delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(0.0, 12.0)),
        phase: TouchPhase::Started,
    };
    assert_eq!(
        convert(&pixels),
        InputEvent::Wheel {
            delta: WheelDelta::Pixels { x: 0.0, y: 12.0 },
            phase: Phase::Started
        }
    );
}

#[test]
fn touch_converts_with_its_force_and_a_stable_device_hash() {
    let touch = |phase, force| {
        WindowEvent::Touch(Touch {
            device_id: DeviceId::dummy(),
            phase,
            location: PhysicalPosition::new(10.0, 20.0),
            force,
            id: 5,
        })
    };
    let InputEvent::Touch(a) = convert(&touch(TouchPhase::Started, Some(Force::Normalized(0.5))))
    else {
        panic!()
    };
    let InputEvent::Touch(b) = convert(&touch(
        TouchPhase::Moved,
        Some(Force::Calibrated {
            force: 1.0,
            max_possible_force: 4.0,
            altitude_angle: None,
        }),
    )) else {
        panic!()
    };
    assert_eq!(
        (a.id, a.phase, a.x, a.y, a.force),
        (5, Phase::Started, 10.0, 20.0, Some(0.5))
    );
    assert_eq!((b.phase, b.force), (Phase::Moved, Some(0.25)));
    assert_eq!(a.device, b.device, "one screen, one device id");
    let InputEvent::Touch(c) = convert(&touch(TouchPhase::Ended, None)) else {
        panic!()
    };
    assert_eq!(c.force, None);
}

#[test]
fn modifiers_focus_and_ime_convert() {
    let mut state = ModifiersState::empty();
    state.insert(ModifiersState::CONTROL);
    state.insert(ModifiersState::SUPER);
    assert_eq!(
        convert(&WindowEvent::ModifiersChanged(Modifiers::from(state))),
        InputEvent::Modifiers(ModifierState {
            shift: false,
            ctrl: true,
            alt: false,
            super_key: true
        })
    );
    assert_eq!(
        convert(&WindowEvent::Focused(false)),
        InputEvent::Focused(false)
    );
    assert_eq!(
        convert(&WindowEvent::Ime(WinitIme::Enabled)),
        InputEvent::Ime(Ime::Enabled)
    );
    assert_eq!(
        convert(&WindowEvent::Ime(WinitIme::Preedit(
            "\u{43d}\u{438}".into(),
            Some((0, 2))
        ))),
        InputEvent::Ime(Ime::Preedit {
            text: "\u{43d}\u{438}".into(),
            cursor: Some((0, 2))
        })
    );
    assert_eq!(
        convert(&WindowEvent::Ime(WinitIme::Commit("\u{43d}\u{438}".into()))),
        InputEvent::Ime(Ime::Commit("\u{43d}\u{438}".into()))
    );
}

#[test]
fn frames_that_carry_no_input_still_ask_for_a_frame_and_a_few_things_are_ignored() {
    assert_eq!(
        convert(&WindowEvent::Resized(PhysicalSize::new(10, 10))),
        InputEvent::Repaint
    );
    assert_eq!(convert(&WindowEvent::Occluded(true)), InputEvent::Repaint);
    assert_eq!(convert(&WindowEvent::CloseRequested), InputEvent::Repaint);
    assert_eq!(
        convert(&WindowEvent::AxisMotion {
            device_id: DeviceId::dummy(),
            axis: 0,
            value: 1.0
        }),
        InputEvent::Ignored
    );
}

#[test]
fn the_layout_key_wins_and_the_place_of_the_key_covers_layouts_without_latin_letters() {
    // US: the key labelled B.
    let us = key_of(
        &character("b"),
        PhysicalKey::Code(KeyCode::KeyB),
        Some("b"),
        true,
        false,
        false,
    );
    assert_eq!((us.logical, us.physical), (Some(Key::B), Some(Key::B)));
    // Russian: the key labelled <cyrillic> sits at the place of B; egui has no key named "<cyrillic>".
    let ru = key_of(
        &character("\u{438}"),
        PhysicalKey::Code(KeyCode::KeyB),
        Some("\u{438}"),
        true,
        false,
        false,
    );
    assert_eq!(
        (ru.logical, ru.physical, ru.text.as_deref()),
        (None, Some(Key::B), Some("\u{438}"))
    );
    // AZERTY: the key labelled A sits at the place of Q.
    let fr = key_of(
        &character("a"),
        PhysicalKey::Code(KeyCode::KeyQ),
        Some("a"),
        true,
        false,
        false,
    );
    assert_eq!((fr.logical, fr.physical), (Some(Key::A), Some(Key::Q)));
    // Shift gives a capital: still the same key.
    let capital = key_of(
        &character("B"),
        PhysicalKey::Code(KeyCode::KeyB),
        Some("B"),
        true,
        false,
        false,
    );
    assert_eq!(capital.logical, Some(Key::B));
    // A dead key and an unidentified key have no logical key.
    let dead = key_of(
        &WinitKey::Dead(Some('`')),
        PhysicalKey::Code(KeyCode::Backquote),
        None,
        true,
        false,
        false,
    );
    assert_eq!((dead.logical, dead.physical), (None, Some(Key::Backtick)));
}

#[test]
fn named_keys_physical_keys_and_the_text_of_a_press() {
    let f6 = key_of(
        &WinitKey::Named(NamedKey::F6),
        PhysicalKey::Code(KeyCode::F6),
        None,
        true,
        false,
        false,
    );
    assert_eq!(
        (f6.logical, f6.physical, f6.text),
        (Some(Key::F6), Some(Key::F6), None)
    );
    // No `text` from the system: the logical key's own text is used (the space bar).
    let space = key_of(
        &WinitKey::Named(NamedKey::Space),
        PhysicalKey::Code(KeyCode::Space),
        None,
        true,
        false,
        false,
    );
    assert_eq!(space.text.as_deref(), Some(" "));
    let enter = key_of(
        &WinitKey::Named(NamedKey::Enter),
        PhysicalKey::Code(KeyCode::NumpadEnter),
        Some("\r"),
        true,
        false,
        false,
    );
    assert_eq!(
        (enter.logical, enter.physical),
        (Some(Key::Enter), Some(Key::Enter))
    );
    let repeat = key_of(
        &character("b"),
        PhysicalKey::Code(KeyCode::KeyB),
        Some("b"),
        true,
        true,
        false,
    );
    assert!(repeat.repeat && repeat.pressed && !repeat.synthetic);
    let process = key_of(
        &WinitKey::Named(NamedKey::Process),
        PhysicalKey::Code(KeyCode::KeyB),
        None,
        true,
        false,
        false,
    );
    assert!(process.ime_processed);
    assert!(!us_flag(&character("b")));
}

fn us_flag(key: &WinitKey) -> bool {
    key_of(
        key,
        PhysicalKey::Code(KeyCode::KeyB),
        None,
        true,
        false,
        false,
    )
    .ime_processed
}

#[test]
fn the_key_tables_cover_letters_digits_function_keys_and_the_numpad() {
    for (i, letter) in ('a'..='z').enumerate() {
        let code = [
            KeyCode::KeyA,
            KeyCode::KeyB,
            KeyCode::KeyC,
            KeyCode::KeyD,
            KeyCode::KeyE,
            KeyCode::KeyF,
            KeyCode::KeyG,
            KeyCode::KeyH,
            KeyCode::KeyI,
            KeyCode::KeyJ,
            KeyCode::KeyK,
            KeyCode::KeyL,
            KeyCode::KeyM,
            KeyCode::KeyN,
            KeyCode::KeyO,
            KeyCode::KeyP,
            KeyCode::KeyQ,
            KeyCode::KeyR,
            KeyCode::KeyS,
            KeyCode::KeyT,
            KeyCode::KeyU,
            KeyCode::KeyV,
            KeyCode::KeyW,
            KeyCode::KeyX,
            KeyCode::KeyY,
            KeyCode::KeyZ,
        ][i];
        let by_name = Key::from_name(&letter.to_string()).unwrap();
        assert_eq!(key_from_code(code), Some(by_name), "{letter}");
        assert_eq!(
            key_from_logical(&character(&letter.to_string())),
            Some(by_name)
        );
    }
    assert_eq!(key_from_code(KeyCode::Digit7), Some(Key::Num7));
    assert_eq!(key_from_code(KeyCode::Numpad7), Some(Key::Num7));
    assert_eq!(key_from_code(KeyCode::NumpadAdd), Some(Key::Plus));
    assert_eq!(key_from_code(KeyCode::NumpadSubtract), Some(Key::Minus));
    assert_eq!(key_from_code(KeyCode::F35), Some(Key::F35));
    assert_eq!(
        key_from_code(KeyCode::IntlBackslash),
        Some(Key::IntlBackslash)
    );
    assert_eq!(key_from_code(KeyCode::ShiftLeft), Some(Key::ShiftLeft));
    assert_eq!(key_from_code(KeyCode::CapsLock), None);
    assert_eq!(
        key_from_logical(&WinitKey::Named(NamedKey::F12)),
        Some(Key::F12)
    );
    assert_eq!(
        key_from_logical(&WinitKey::Named(NamedKey::BrowserBack)),
        Some(Key::BrowserBack)
    );
    assert_eq!(key_from_logical(&WinitKey::Named(NamedKey::Shift)), None);
    assert_eq!(
        key_from_logical(&WinitKey::Unidentified(
            winit::keyboard::NativeKey::Unidentified
        )),
        None
    );
}

#[test]
fn every_egui_cursor_has_a_winit_cursor() {
    // `winit_cursor` is total; the point of the test is that a new egui icon breaks the build of
    // the match and that the common ones are the ones people expect.
    for icon in CursorIcon::ALL {
        let _ = winit_cursor(icon);
    }
    assert_eq!(winit_cursor(CursorIcon::PointingHand), WinitCursor::Pointer);
    assert_eq!(winit_cursor(CursorIcon::Text), WinitCursor::Text);
    assert_eq!(
        winit_cursor(CursorIcon::ResizeHorizontal),
        WinitCursor::EwResize
    );
    assert_eq!(
        winit_cursor(CursorIcon::ResizeNwSe),
        WinitCursor::NwseResize
    );
}
