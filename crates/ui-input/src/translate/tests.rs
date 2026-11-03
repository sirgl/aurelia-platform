use super::*;
use crate::clipboard::MemoryClipboard;
use egui::{Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, Vec2};

#[derive(Clone, Copy, Default)]
struct Stub {
    pointer: bool,
    using_pointer: bool,
    keyboard: bool,
}

impl Wants for Stub {
    fn pointer(&self) -> bool {
        self.pointer
    }

    fn using_pointer(&self) -> bool {
        self.using_pointer
    }

    fn keyboard(&self) -> bool {
        self.keyboard
    }
}

const FREE: Stub = Stub {
    pointer: false,
    using_pointer: false,
    keyboard: false,
};

fn platform() -> Platform {
    Platform {
        mac: false,
        windows: false,
    }
}

fn translator(platform: Platform, scale: f32) -> Translator {
    Translator::with(platform, Box::new(MemoryClipboard::new()), scale)
}

fn frame(scale: f32) -> FrameInfo {
    FrameInfo {
        size_px: (1000., 500.),
        time: 1.5,
        pixels_per_point: scale,
        native_pixels_per_point: scale,
    }
}

/// Feeds the events and returns what egui would see.
fn run(t: &mut Translator, events: impl IntoIterator<Item = InputEvent>) -> Vec<Event> {
    run_with(t, events, FREE)
}

fn run_with(
    t: &mut Translator,
    events: impl IntoIterator<Item = InputEvent>,
    wants: Stub,
) -> Vec<Event> {
    for event in events {
        t.handle(event, &wants);
    }
    t.take_input(frame(t.pixels_per_point)).events
}

fn mods(shift: bool, ctrl: bool, alt: bool, super_key: bool) -> InputEvent {
    InputEvent::Modifiers(ModifierState {
        shift,
        ctrl,
        alt,
        super_key,
    })
}

fn typed(key: Key, text: &str) -> KeyInput {
    KeyInput {
        text: Some(text.to_owned()),
        ..KeyInput::press(key)
    }
}

fn touch(id: u64, phase: Phase, x: f64, y: f64) -> InputEvent {
    InputEvent::Touch(TouchInput {
        device: 7,
        id,
        phase,
        x,
        y,
        force: None,
    })
}

fn key_events(events: &[Event]) -> Vec<(Key, Option<Key>, bool, bool)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Key {
                key,
                physical_key,
                pressed,
                repeat,
                ..
            } => Some((*key, *physical_key, *pressed, *repeat)),
            _ => None,
        })
        .collect()
}

// ---- pointer --------------------------------------------------------------------------------

#[test]
fn pointer_positions_are_divided_by_the_dpi_scale() {
    let mut t = translator(platform(), 2.0);
    let events = run(&mut t, [InputEvent::CursorMoved { x: 200., y: 100. }]);
    assert_eq!(events, vec![Event::PointerMoved(Pos2::new(100., 50.))]);
    // egui's zoom factor changes the scale between events.
    t.set_pixels_per_point(4.0);
    let events = run(&mut t, [InputEvent::CursorMoved { x: 200., y: 100. }]);
    assert_eq!(events, vec![Event::PointerMoved(Pos2::new(50., 25.))]);
}

#[test]
fn a_button_press_carries_the_pointer_position_and_modifiers() {
    let mut t = translator(platform(), 2.0);
    let events = run(
        &mut t,
        [
            mods(true, false, false, false),
            InputEvent::CursorMoved { x: 40., y: 60. },
            InputEvent::Button {
                button: Button::Left,
                pressed: true,
            },
            InputEvent::Button {
                button: Button::Left,
                pressed: false,
            },
        ],
    );
    let shift = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    let at = Pos2::new(20., 30.);
    assert_eq!(
        events[2..],
        [
            Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: shift
            },
            Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: shift
            },
        ]
    );
}

#[test]
fn every_mouse_button_has_its_egui_name_and_unknown_ones_are_dropped() {
    let mut t = translator(platform(), 1.0);
    let buttons = [
        (Button::Left, PointerButton::Primary),
        (Button::Right, PointerButton::Secondary),
        (Button::Middle, PointerButton::Middle),
        (Button::Back, PointerButton::Extra1),
        (Button::Forward, PointerButton::Extra2),
    ];
    let mut input = vec![InputEvent::CursorMoved { x: 1., y: 1. }];
    input.extend(buttons.iter().map(|&(button, _)| InputEvent::Button {
        button,
        pressed: true,
    }));
    input.push(InputEvent::Button {
        button: Button::Other,
        pressed: true,
    });
    let events = run(&mut t, input);
    let seen: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::PointerButton { button, .. } => Some(*button),
            _ => None,
        })
        .collect();
    assert_eq!(seen, buttons.iter().map(|&(_, b)| b).collect::<Vec<_>>());
}

#[test]
fn a_button_before_any_pointer_position_is_dropped() {
    let mut t = translator(platform(), 1.0);
    assert!(run(
        &mut t,
        [InputEvent::Button {
            button: Button::Left,
            pressed: true
        }]
    )
    .is_empty());
}

#[test]
fn leaving_the_window_removes_the_pointer() {
    let mut t = translator(platform(), 1.0);
    let events = run(
        &mut t,
        [
            InputEvent::CursorMoved { x: 5., y: 5. },
            InputEvent::CursorLeft,
        ],
    );
    assert_eq!(events.last(), Some(&Event::PointerGone));
    assert!(!t.pointer_in_window());
    assert!(
        run(
            &mut t,
            [InputEvent::Button {
                button: Button::Left,
                pressed: true
            }]
        )
        .is_empty(),
        "no position, no click"
    );
}

#[test]
fn wheel_lines_stay_lines_and_pixels_become_points() {
    let mut t = translator(platform(), 2.0);
    let events = run(
        &mut t,
        [
            InputEvent::Wheel {
                delta: WheelDelta::Lines { x: 0., y: -3. },
                phase: Phase::Moved,
            },
            InputEvent::Wheel {
                delta: WheelDelta::Pixels { x: 20., y: -60. },
                phase: Phase::Started,
            },
        ],
    );
    assert_eq!(
        events,
        vec![
            Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: Vec2::new(0., -3.),
                phase: egui::TouchPhase::Move,
                modifiers: Modifiers::NONE
            },
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: Vec2::new(10., -30.),
                phase: egui::TouchPhase::Start,
                modifiers: Modifiers::NONE
            },
        ]
    );
}

#[test]
fn pinch_and_two_finger_pan_gestures() {
    let mut t = translator(platform(), 2.0);
    let events = run(
        &mut t,
        [
            InputEvent::Pinch { delta: 0.5 },
            InputEvent::Pan {
                dx: 10.,
                dy: -20.,
                phase: Phase::Moved,
            },
        ],
    );
    assert_eq!(events[0], Event::Zoom(0.5f32.exp()));
    assert_eq!(
        events[1],
        Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: Vec2::new(5., -10.),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE
        }
    );
}

// ---- modifiers ------------------------------------------------------------------------------

#[test]
fn ctrl_is_the_command_key_except_on_a_mac_where_cmd_is() {
    let mut other = translator(platform(), 1.0);
    let events = run(&mut other, [mods(false, true, false, true)]);
    assert_eq!(
        events,
        vec![Event::ModifiersChanged(Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        })]
    );

    let mut mac = translator(
        Platform {
            mac: true,
            windows: false,
        },
        1.0,
    );
    let events = run(&mut mac, [mods(false, true, false, true)]);
    assert_eq!(
        events,
        vec![Event::ModifiersChanged(Modifiers {
            ctrl: true,
            command: true,
            mac_cmd: true,
            ..Modifiers::NONE
        })]
    );
    let events = run(&mut mac, [mods(false, true, false, false)]);
    assert_eq!(
        events,
        vec![Event::ModifiersChanged(Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        })],
        "Ctrl alone is not the command key on a Mac"
    );
}

#[test]
fn later_events_carry_the_modifiers_held_at_the_time() {
    let mut t = translator(platform(), 1.0);
    let events = run(
        &mut t,
        [
            mods(true, false, true, false),
            InputEvent::Key(KeyInput::press(Key::F6)),
            mods(false, false, false, false),
            InputEvent::Key(KeyInput::press(Key::F6)),
        ],
    );
    let modifiers: Vec<Modifiers> = events
        .iter()
        .filter_map(|e| match e {
            Event::Key { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .collect();
    assert_eq!(
        modifiers,
        vec![
            Modifiers {
                shift: true,
                alt: true,
                ..Modifiers::NONE
            },
            Modifiers::NONE
        ]
    );
}

// ---- keys and text --------------------------------------------------------------------------

#[test]
fn a_typed_letter_is_a_key_event_and_a_text_event_and_its_release_only_a_key() {
    let mut t = translator(platform(), 1.0);
    let down = typed(Key::B, "b");
    let events = run(
        &mut t,
        [
            InputEvent::Key(down.clone()),
            InputEvent::Key(down.released()),
        ],
    );
    assert_eq!(
        events,
        vec![
            Event::Key {
                key: Key::B,
                physical_key: Some(Key::B),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE
            },
            Event::Text("b".into()),
            Event::Key {
                key: Key::B,
                physical_key: Some(Key::B),
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE
            },
        ]
    );
}

#[test]
fn control_characters_and_private_use_characters_are_keys_not_text() {
    let mut t = translator(platform(), 1.0);
    let events = run(
        &mut t,
        [
            InputEvent::Key(typed(Key::Enter, "\r")),
            InputEvent::Key(typed(Key::Tab, "\t")),
            InputEvent::Key(typed(Key::Delete, "\u{f728}")),
            InputEvent::Key(typed(Key::Backspace, "\u{8}")),
        ],
    );
    assert!(
        events.iter().all(|e| !matches!(e, Event::Text(_))),
        "{events:?}"
    );
    assert_eq!(key_events(&events).len(), 4);
}

#[test]
fn a_shortcut_does_not_type_its_letter() {
    let mut t = translator(platform(), 1.0);
    let events = run(
        &mut t,
        [
            mods(false, true, false, false),
            InputEvent::Key(typed(Key::W, "w")),
        ],
    );
    assert!(events.iter().all(|e| !matches!(e, Event::Text(_))));
    assert_eq!(
        key_events(&events),
        vec![(Key::W, Some(Key::W), true, false)]
    );
    // Alt-modified letters still type (AltGr produces letters on many layouts).
    let events = run(
        &mut t,
        [
            mods(false, false, true, false),
            InputEvent::Key(typed(Key::E, "é")),
        ],
    );
    assert!(events.contains(&Event::Text("é".into())));
}

#[test]
fn a_russian_letter_is_the_key_at_its_place_and_still_types() {
    // The key labelled <cyrillic> sits where B is on a US keyboard.
    let russian = KeyInput {
        logical: None,
        physical: Some(Key::B),
        text: Some("\u{438}".into()),
        ..KeyInput::press(Key::B)
    };
    let mut t = translator(platform(), 1.0);
    let events = run(
        &mut t,
        [
            InputEvent::Key(russian.clone()),
            InputEvent::Key(russian.released()),
        ],
    );
    assert_eq!(
        events,
        vec![
            Event::Key {
                key: Key::B,
                physical_key: Some(Key::B),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE
            },
            Event::Text("\u{438}".into()),
            Event::Key {
                key: Key::B,
                physical_key: Some(Key::B),
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE
            },
        ]
    );
}

#[test]
fn a_key_without_logical_and_physical_key_gives_only_text() {
    let odd = KeyInput {
        logical: None,
        physical: None,
        text: Some("€".into()),
        ..KeyInput::press(Key::A)
    };
    let mut t = translator(platform(), 1.0);
    assert_eq!(
        run(&mut t, [InputEvent::Key(odd)]),
        vec![Event::Text("€".into())]
    );
}

#[test]
fn azerty_reports_the_key_the_label_says_and_the_place_beside_it() {
    // The key labelled A sits where Q is on a US keyboard.
    let azerty = KeyInput {
        logical: Some(Key::A),
        physical: Some(Key::Q),
        ..KeyInput::press(Key::A)
    };
    let mut t = translator(platform(), 1.0);
    let events = run(&mut t, [InputEvent::Key(azerty)]);
    assert_eq!(
        key_events(&events),
        vec![(Key::A, Some(Key::Q), true, false)]
    );
}

#[test]
fn key_repeat_is_passed_on() {
    let mut t = translator(platform(), 1.0);
    let repeat = KeyInput {
        repeat: true,
        ..KeyInput::press(Key::ArrowRight)
    };
    let events = run(
        &mut t,
        [
            InputEvent::Key(KeyInput::press(Key::ArrowRight)),
            InputEvent::Key(repeat.clone()),
            InputEvent::Key(repeat),
        ],
    );
    let flags: Vec<bool> = key_events(&events).iter().map(|k| k.3).collect();
    assert_eq!(flags, [false, true, true]);
}

#[test]
fn a_synthetic_press_is_ignored_but_a_synthetic_release_is_not() {
    let mut t = translator(platform(), 1.0);
    let synthetic = KeyInput {
        synthetic: true,
        ..KeyInput::press(Key::A)
    };
    let response = t.handle(InputEvent::Key(synthetic.clone()), &FREE);
    assert_eq!(
        response,
        Response {
            consumed: false,
            repaint: true
        }
    );
    t.handle(InputEvent::Key(synthetic.released()), &FREE);
    let events = t.take_input(frame(1.0)).events;
    assert_eq!(
        key_events(&events),
        vec![(Key::A, Some(Key::A), false, false)]
    );
}

#[test]
fn clipboard_keys_work_on_a_russian_layout_through_the_physical_key() {
    let clipboard = MemoryClipboard::new();
    let mut t = Translator::with(platform(), Box::new(clipboard), 1.0);
    let russian = |physical: Key| KeyInput {
        logical: None,
        physical: Some(physical),
        text: Some("\u{441}".into()),
        ..KeyInput::press(physical)
    };
    // Ctrl+<cyrillic> (the key at the place of C) copies; Ctrl+<cyrillic> (X) cuts; Ctrl+<cyrillic> (V) pastes.
    let events = run(
        &mut t,
        [
            mods(false, true, false, false),
            InputEvent::Key(russian(Key::C)),
            InputEvent::Key(russian(Key::X)),
        ],
    );
    assert_eq!(events[1..], [Event::Copy, Event::Cut]);
    t.clipboard().set("a\r\nb".into());
    let events = run(&mut t, [InputEvent::Key(russian(Key::V))]);
    assert_eq!(
        events,
        vec![Event::Paste("a\nb".into())],
        "CRLF becomes LF, and no key or text event is left behind"
    );
}

#[test]
fn pasting_an_empty_clipboard_does_nothing_and_command_keys_need_command() {
    let mut t = translator(platform(), 1.0);
    let events = run(
        &mut t,
        [
            mods(false, true, false, false),
            InputEvent::Key(KeyInput::press(Key::V)),
        ],
    );
    assert_eq!(events.len(), 1, "only the modifiers event: {events:?}");
    // Plain C is a key, not a copy.
    let events = run(
        &mut t,
        [
            mods(false, false, false, false),
            InputEvent::Key(KeyInput::press(Key::C)),
        ],
    );
    assert_eq!(key_events(&events).len(), 1);
    assert!(!events.contains(&Event::Copy));
    // The dedicated Copy key copies without a modifier.
    let events = run(&mut t, [InputEvent::Key(KeyInput::press(Key::Copy))]);
    assert_eq!(events, vec![Event::Copy]);
}

#[test]
fn the_windows_clipboard_keys_exist_on_windows_only() {
    let shift_insert = |t: &mut Translator| {
        t.clipboard().set("x".into());
        run(
            t,
            [
                mods(true, false, false, false),
                InputEvent::Key(KeyInput::press(Key::Insert)),
            ],
        )
    };
    let mut windows = translator(
        Platform {
            mac: false,
            windows: true,
        },
        1.0,
    );
    assert_eq!(shift_insert(&mut windows)[1], Event::Paste("x".into()));
    let mut linux = translator(platform(), 1.0);
    assert!(matches!(
        shift_insert(&mut linux)[1],
        Event::Key {
            key: Key::Insert,
            ..
        }
    ));
}

#[test]
fn on_a_mac_cmd_c_copies_and_ctrl_c_does_not() {
    let mut t = translator(
        Platform {
            mac: true,
            windows: false,
        },
        1.0,
    );
    let events = run(
        &mut t,
        [
            mods(false, false, false, true),
            InputEvent::Key(KeyInput::press(Key::C)),
        ],
    );
    assert_eq!(events[1], Event::Copy);
    let events = run(
        &mut t,
        [
            mods(false, true, false, false),
            InputEvent::Key(KeyInput::press(Key::C)),
        ],
    );
    assert!(matches!(events[1], Event::Key { key: Key::C, .. }));
}

// ---- input method ---------------------------------------------------------------------------

fn preedit(text: &str, cursor: Option<(usize, usize)>) -> InputEvent {
    InputEvent::Ime(Ime::Preedit {
        text: text.into(),
        cursor,
    })
}

#[test]
fn ime_preedit_and_commit_reach_egui_and_the_cursor_is_counted_in_characters() {
    let mut t = translator(platform(), 1.0);
    // "<cyrillic>" is six bytes, three characters; the highlighted part is "<cyrillic>" (bytes 4..6).
    let events = run(
        &mut t,
        [
            InputEvent::Ime(Ime::Enabled),
            preedit("\u{43f}\u{440}\u{438}", Some((4, 6))),
            preedit("", None),
            InputEvent::Ime(Ime::Commit("\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442}".into())),
            InputEvent::Ime(Ime::Disabled),
        ],
    );
    assert_eq!(
        events,
        vec![
            Event::Ime(egui::ImeEvent::Preedit {
                text: "\u{43f}\u{440}\u{438}".into(),
                active_range_chars: Some(2..3)
            }),
            Event::Ime(egui::ImeEvent::Preedit {
                text: String::new(),
                active_range_chars: None
            }),
            Event::Ime(egui::ImeEvent::Commit("\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442}".into())),
        ]
    );
}

#[test]
fn an_ime_cursor_inside_a_character_is_dropped() {
    let mut t = translator(platform(), 1.0);
    let events = run(&mut t, [preedit("\u{43f}\u{440}\u{438}", Some((1, 3)))]);
    assert_eq!(
        events,
        vec![Event::Ime(egui::ImeEvent::Preedit {
            text: "\u{43f}\u{440}\u{438}".into(),
            active_range_chars: None
        })]
    );
}

#[test]
fn a_zero_width_cursor_at_the_start_is_a_windows_korean_ime_bug() {
    let mut windows = translator(
        Platform {
            mac: false,
            windows: true,
        },
        1.0,
    );
    let events = run(&mut windows, [preedit("한", Some((0, 0)))]);
    assert_eq!(
        events,
        vec![Event::Ime(egui::ImeEvent::Preedit {
            text: "한".into(),
            active_range_chars: None
        })]
    );
    let mut other = translator(platform(), 1.0);
    let events = run(&mut other, [preedit("한", Some((0, 0)))]);
    assert_eq!(
        events,
        vec![Event::Ime(egui::ImeEvent::Preedit {
            text: "한".into(),
            active_range_chars: Some(0..0)
        })]
    );
}

#[test]
fn keys_the_input_method_swallowed_vanish_with_their_release() {
    let mut t = translator(platform(), 1.0);
    t.ime_allowed = true;
    let swallowed = KeyInput {
        logical: None,
        physical: Some(Key::B),
        ime_processed: true,
        ..KeyInput::press(Key::B)
    };
    let stub = Stub {
        keyboard: true,
        ..Default::default()
    };
    let response = t.handle(InputEvent::Key(swallowed.clone()), &stub);
    assert_eq!(
        response,
        Response {
            consumed: true,
            repaint: false
        }
    );
    // The release comes back with the original logical key, not "Process".
    let release = KeyInput::press(Key::B).released();
    assert_eq!(
        t.handle(InputEvent::Key(release.clone()), &FREE),
        Response {
            consumed: false,
            repaint: false
        }
    );
    // A second release is a real one.
    t.handle(InputEvent::Key(release), &FREE);
    assert_eq!(
        key_events(&t.take_input(frame(1.0)).events),
        vec![(Key::B, Some(Key::B), false, false)]
    );
}

#[test]
fn without_an_input_method_a_process_key_is_just_a_key() {
    let mut t = translator(platform(), 1.0);
    let key = KeyInput {
        logical: None,
        physical: Some(Key::B),
        ime_processed: true,
        ..KeyInput::press(Key::B)
    };
    let events = run(&mut t, [InputEvent::Key(key)]);
    assert_eq!(key_events(&events).len(), 1);
}

// ---- touch ----------------------------------------------------------------------------------

#[test]
fn a_tap_is_a_touch_a_press_and_a_release_and_the_pointer_vanishes() {
    let mut t = translator(platform(), 2.0);
    let events = run(
        &mut t,
        [
            touch(1, Phase::Started, 100., 60.),
            touch(1, Phase::Ended, 100., 60.),
        ],
    );
    let at = Pos2::new(50., 30.);
    let touch_event = |phase| Event::Touch {
        device_id: egui::TouchDeviceId(7),
        id: egui::TouchId::from(1u64),
        phase,
        pos: at,
        force: None,
    };
    let button = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    assert_eq!(
        events,
        vec![
            touch_event(egui::TouchPhase::Start),
            Event::PointerMoved(at),
            button(true),
            touch_event(egui::TouchPhase::End),
            button(false),
            Event::PointerGone,
        ]
    );
    assert!(!t.pointer_in_window(), "no hover after the finger lifts");
}

#[test]
fn a_drag_moves_the_pointer_and_a_cancel_releases_nothing() {
    let mut t = translator(platform(), 1.0);
    let events = run(
        &mut t,
        [
            touch(1, Phase::Started, 10., 10.),
            touch(1, Phase::Moved, 30., 20.),
            touch(1, Phase::Cancelled, 30., 20.),
        ],
    );
    assert!(events.contains(&Event::PointerMoved(Pos2::new(30., 20.))));
    assert_eq!(events.last(), Some(&Event::PointerGone));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::PointerButton { pressed: false, .. }))
            .count(),
        0,
        "a cancelled touch is not a click"
    );
}

#[test]
fn two_fingers_both_produce_touch_events_but_only_the_first_drives_the_pointer() {
    let mut t = translator(platform(), 1.0);
    let events = run(
        &mut t,
        [
            touch(1, Phase::Started, 100., 100.),
            touch(2, Phase::Started, 200., 100.),
            touch(1, Phase::Moved, 90., 100.),
            touch(2, Phase::Moved, 210., 100.),
        ],
    );
    let touches: Vec<(u64, egui::TouchPhase, Pos2)> = events
        .iter()
        .filter_map(|e| match e {
            Event::Touch { id, phase, pos, .. } => Some((id.0, *phase, *pos)),
            _ => None,
        })
        .collect();
    assert_eq!(
        touches.len(),
        4,
        "the pinch egui multi-touch needs: both fingers' positions"
    );
    assert_eq!(
        touches[1],
        (2, egui::TouchPhase::Start, Pos2::new(200., 100.))
    );
    let moves: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::PointerMoved(_)))
        .collect();
    assert_eq!(
        moves,
        [
            &Event::PointerMoved(Pos2::new(100., 100.)),
            &Event::PointerMoved(Pos2::new(90., 100.))
        ],
        "the second finger never moves the pointer"
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::PointerButton { .. }))
            .count(),
        1
    );
}

#[test]
fn touch_force_and_device_are_passed_on() {
    let mut t = translator(platform(), 1.0);
    let finger = InputEvent::Touch(TouchInput {
        device: 9,
        id: 3,
        phase: Phase::Started,
        x: 1.,
        y: 2.,
        force: Some(0.25),
    });
    let events = run(&mut t, [finger]);
    assert_eq!(
        events[0],
        Event::Touch {
            device_id: egui::TouchDeviceId(9),
            id: egui::TouchId::from(3u64),
            phase: egui::TouchPhase::Start,
            pos: Pos2::new(1., 2.),
            force: Some(0.25)
        }
    );
}

// ---- focus ----------------------------------------------------------------------------------

#[test]
fn losing_the_focus_releases_the_held_keys_and_modifiers() {
    let mut t = translator(platform(), 1.0);
    run(
        &mut t,
        [
            InputEvent::Focused(true),
            mods(true, true, false, false),
            InputEvent::Key(KeyInput::press(Key::S)),
            InputEvent::Key(KeyInput::press(Key::ArrowLeft)),
        ],
    );
    let events = run(&mut t, [InputEvent::Focused(false)]);
    assert_eq!(
        events,
        vec![
            Event::Key {
                key: Key::S,
                physical_key: Some(Key::S),
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE
            },
            Event::Key {
                key: Key::ArrowLeft,
                physical_key: Some(Key::ArrowLeft),
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE
            },
            Event::WindowFocused(false),
        ]
    );
    // Control was held when the window lost the focus; coming back, a key press has no modifiers.
    let events = run(
        &mut t,
        [
            InputEvent::Focused(true),
            InputEvent::Key(KeyInput::press(Key::B)),
        ],
    );
    assert_eq!(
        events[1..],
        [Event::Key {
            key: Key::B,
            physical_key: Some(Key::B),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE
        }]
    );
    assert!(!t
        .take_input(frame(1.0))
        .events
        .contains(&Event::WindowFocused(false)));
}

#[test]
fn the_focus_flag_follows_the_window_and_starts_false() {
    let mut t = translator(platform(), 1.0);
    assert!(
        !t.take_input(frame(1.0)).focused,
        "winit tells us when we have the focus"
    );
    t.handle(InputEvent::Focused(true), &FREE);
    assert!(t.take_input(frame(1.0)).focused);
    t.handle(InputEvent::Focused(false), &FREE);
    assert!(!t.take_input(frame(1.0)).focused);
}

#[test]
fn losing_the_focus_drops_the_finger_that_drove_the_pointer() {
    let mut t = translator(platform(), 1.0);
    run(
        &mut t,
        [touch(1, Phase::Started, 5., 5.), InputEvent::Focused(false)],
    );
    // Another finger can now take the pointer.
    let events = run(&mut t, [touch(2, Phase::Started, 9., 9.)]);
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::PointerButton { pressed: true, .. })));
}

// ---- the frame ------------------------------------------------------------------------------

#[test]
fn the_frame_input_has_the_screen_in_points_the_time_and_the_scale() {
    let mut t = translator(platform(), 2.0);
    let raw = t.take_input(FrameInfo {
        size_px: (2880., 1800.),
        time: 12.5,
        pixels_per_point: 2.0,
        native_pixels_per_point: 2.0,
    });
    assert_eq!(
        raw.screen_rect,
        Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            Vec2::new(1440., 900.)
        ))
    );
    assert_eq!(raw.time, Some(12.5));
    assert_eq!(raw.viewport().native_pixels_per_point, Some(2.0));
    // With egui's zoom factor 1.5 the same window is fewer points.
    let raw = t.take_input(FrameInfo {
        size_px: (2880., 1800.),
        time: 13.,
        pixels_per_point: 3.0,
        native_pixels_per_point: 2.0,
    });
    assert_eq!(
        raw.screen_rect.map(|r| r.size()),
        Some(Vec2::new(960., 600.))
    );
    assert_eq!(raw.viewport().native_pixels_per_point, Some(2.0));
}

#[test]
fn a_minimised_window_has_no_screen_rect_and_events_are_taken_once() {
    let mut t = translator(platform(), 1.0);
    t.handle(InputEvent::CursorMoved { x: 1., y: 1. }, &FREE);
    let raw = t.take_input(FrameInfo {
        size_px: (0., 0.),
        time: 0.,
        pixels_per_point: 1.,
        native_pixels_per_point: 1.,
    });
    assert_eq!(raw.screen_rect, None);
    assert_eq!(raw.events.len(), 1);
    assert!(t.take_input(frame(1.0)).events.is_empty());
}

#[test]
fn a_scale_factor_change_reaches_the_viewport_info() {
    let mut t = translator(platform(), 1.0);
    let response = t.handle(InputEvent::ScaleFactor(2.5), &FREE);
    assert!(response.repaint);
    assert_eq!(t.pending().viewport().native_pixels_per_point, Some(2.5));
}

// ---- what egui keeps for itself --------------------------------------------------------------

#[test]
fn consumed_follows_what_egui_wants() {
    let mut t = translator(platform(), 1.0);
    let all = Stub {
        pointer: true,
        using_pointer: true,
        keyboard: true,
    };
    let none = FREE;
    let pointer_only = Stub {
        pointer: true,
        ..Default::default()
    };
    let using_only = Stub {
        using_pointer: true,
        ..Default::default()
    };

    let ask = |t: &mut Translator, event: InputEvent, wants: Stub| t.handle(event, &wants).consumed;
    let click = InputEvent::Button {
        button: Button::Left,
        pressed: true,
    };
    assert!(ask(&mut t, click.clone(), pointer_only));
    assert!(!ask(&mut t, click, using_only));
    let wheel = InputEvent::Wheel {
        delta: WheelDelta::Lines { x: 0., y: 1. },
        phase: Phase::Moved,
    };
    assert!(ask(&mut t, wheel.clone(), pointer_only));
    assert!(!ask(&mut t, wheel, none));
    // Hovering counts only while egui is using the pointer (a drag in progress).
    let moved = InputEvent::CursorMoved { x: 1., y: 1. };
    assert!(ask(&mut t, moved.clone(), using_only));
    assert!(!ask(&mut t, moved, pointer_only));
    // A touch start/end asks "would egui take a click", a touch move "is egui dragging".
    assert!(ask(&mut t, touch(1, Phase::Started, 1., 1.), pointer_only));
    assert!(!ask(&mut t, touch(1, Phase::Moved, 1., 1.), pointer_only));
    assert!(ask(&mut t, touch(1, Phase::Moved, 1., 1.), using_only));
    assert!(ask(&mut t, touch(1, Phase::Ended, 1., 1.), pointer_only));
    // Keys and IME: a text field has the focus; Tab is always egui's.
    assert!(ask(&mut t, InputEvent::Key(KeyInput::press(Key::A)), all));
    assert!(!ask(&mut t, InputEvent::Key(KeyInput::press(Key::A)), none));
    assert!(ask(
        &mut t,
        InputEvent::Key(KeyInput::press(Key::Tab)),
        none
    ));
    assert!(ask(&mut t, InputEvent::Ime(Ime::Commit("x".into())), all));
    assert!(!ask(&mut t, InputEvent::Ime(Ime::Commit("x".into())), none));
    // Focus, modifiers and the rest are never consumed.
    assert!(!ask(&mut t, InputEvent::Focused(true), all));
    assert!(!ask(&mut t, mods(true, false, false, false), all));
    assert!(!ask(&mut t, InputEvent::CursorLeft, all));
}

#[test]
fn which_events_ask_for_a_new_frame() {
    let mut t = translator(platform(), 1.0);
    for event in [
        InputEvent::CursorMoved { x: 1., y: 1. },
        InputEvent::CursorLeft,
        InputEvent::Focused(true),
        InputEvent::Repaint,
        InputEvent::ScaleFactor(1.0),
    ] {
        assert!(t.handle(event.clone(), &FREE).repaint, "{event:?}");
    }
    assert_eq!(t.handle(InputEvent::Ignored, &FREE), Response::default());
}
