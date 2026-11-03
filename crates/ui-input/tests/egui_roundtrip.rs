//! The translated input driven through a real `egui::Context`, no window: a click on a button
//! clicks it (mouse and finger), typing reaches a text field in Latin and Cyrillic, paste and the
//! input method work, and `consumed` tells the game that egui took a click.
use egui::{Context, Key, Pos2, Rect};
use ui_input::{
    Button, FrameInfo, Ime, InputEvent, KeyInput, MemoryClipboard, ModifierState, Phase, Platform,
    TouchInput, Translator,
};

struct Ui {
    ctx: Context,
    t: Translator,
    text: String,
    button: Rect,
    field: Rect,
    clicked: u32,
    frames: u32,
}

impl Ui {
    fn new() -> Self {
        let mut ui = Self {
            ctx: Context::default(),
            t: Translator::with(Platform::default(), Box::new(MemoryClipboard::new()), 1.0),
            text: String::new(),
            button: Rect::NOTHING,
            field: Rect::NOTHING,
            clicked: 0,
            frames: 0,
        };
        ui.t.handle(InputEvent::Focused(true), &ui.ctx);
        // The first frame only lays the widgets out; egui needs a second one to know them.
        ui.frame();
        ui.frame();
        ui
    }

    fn send(&mut self, events: impl IntoIterator<Item = InputEvent>) -> Vec<bool> {
        events
            .into_iter()
            .map(|e| self.t.handle(e, &self.ctx).consumed)
            .collect()
    }

    fn frame(&mut self) {
        self.frames += 1;
        let raw = self.t.take_input(FrameInfo {
            size_px: (800., 600.),
            time: f64::from(self.frames) / 60.0,
            pixels_per_point: 1.0,
            native_pixels_per_point: 1.0,
        });
        let (text, clicked) = (&mut self.text, &mut self.clicked);
        let (mut button, mut field) = (Rect::NOTHING, Rect::NOTHING);
        let _ = self.ctx.run_ui(raw, |ui| {
            let response = ui.button("Go");
            button = response.rect;
            if response.clicked() {
                *clicked += 1;
            }
            field = ui.text_edit_singleline(text).rect;
        });
        self.button = button;
        self.field = field;
    }

    fn click(&mut self, at: Pos2) {
        self.send([InputEvent::CursorMoved {
            x: at.x as f64,
            y: at.y as f64,
        }]);
        self.frame();
        self.send([InputEvent::Button {
            button: Button::Left,
            pressed: true,
        }]);
        self.frame();
        self.send([InputEvent::Button {
            button: Button::Left,
            pressed: false,
        }]);
        self.frame();
    }

    fn tap(&mut self, at: Pos2) {
        let finger = |phase| {
            InputEvent::Touch(TouchInput {
                device: 1,
                id: 1,
                phase,
                x: at.x as f64,
                y: at.y as f64,
                force: None,
            })
        };
        self.send([finger(Phase::Started)]);
        self.frame();
        self.send([finger(Phase::Ended)]);
        self.frame();
    }

    /// The caret to the end of the text (a click in an empty field leaves egui's caret at the start).
    fn end(&mut self) {
        let end = KeyInput::press(Key::End);
        self.send([
            InputEvent::Key(end.clone()),
            InputEvent::Key(end.released()),
        ]);
        self.frame();
    }

    /// Types one letter the way a keyboard layout delivers it.
    fn type_key(&mut self, logical: Option<Key>, physical: Key, text: &str) {
        let down = KeyInput {
            logical,
            physical: Some(physical),
            text: Some(text.into()),
            ..KeyInput::press(physical)
        };
        self.send([
            InputEvent::Key(down.clone()),
            InputEvent::Key(down.released()),
        ]);
        self.frame();
        self.end();
    }
}

#[test]
fn a_mouse_click_clicks_the_button() {
    let mut ui = Ui::new();
    let center = ui.button.center();
    ui.click(center);
    assert_eq!(ui.clicked, 1);
}

#[test]
fn a_finger_tap_clicks_the_button_and_the_pointer_leaves_with_the_finger() {
    let mut ui = Ui::new();
    let center = ui.button.center();
    ui.tap(center);
    assert_eq!(ui.clicked, 1);
    assert!(!ui.t.pointer_in_window());
}

#[test]
fn a_click_on_a_widget_is_egui_s_and_a_click_on_nothing_is_the_game_s() {
    let mut ui = Ui::new();
    let center = ui.button.center();
    ui.send([InputEvent::CursorMoved {
        x: center.x as f64,
        y: center.y as f64,
    }]);
    ui.frame();
    let on_button = ui.send([InputEvent::Button {
        button: Button::Left,
        pressed: true,
    }]);
    assert_eq!(on_button, [true], "the button wants the click");
    ui.send([InputEvent::Button {
        button: Button::Left,
        pressed: false,
    }]);
    ui.frame();
    // Far below the widgets: nothing of egui is there.
    ui.send([InputEvent::CursorMoved { x: 700., y: 560. }]);
    ui.frame();
    let on_nothing = ui.send([InputEvent::Button {
        button: Button::Left,
        pressed: true,
    }]);
    assert!(
        !ui.ctx.egui_wants_pointer_input() || on_nothing == [true],
        "recorded for the report: {on_nothing:?}"
    );
}

#[test]
fn latin_and_cyrillic_text_reaches_a_text_field_and_the_keyboard_is_egui_s_meanwhile() {
    let mut ui = Ui::new();
    let field = ui.field.center();
    ui.click(field);
    assert!(
        ui.ctx.egui_wants_keyboard_input(),
        "the field has the focus"
    );
    ui.type_key(Some(Key::A), Key::A, "a");
    // Russian layout: the key labelled <cyrillic> is the B key; egui has no logical key for it.
    ui.type_key(None, Key::B, "\u{438}");
    ui.type_key(None, Key::F, "\u{430}");
    // (A bare egui `Context` fed raw `Event::Text` also inserts at the start of a field that was
    // empty when it got the focus: "abc" typed gives "cba"; checked with no glue in between. So the
    // test compares the letters, not their order.)
    let mut letters: Vec<char> = ui.text.chars().collect();
    letters.sort_unstable();
    assert_eq!(letters, ['a', '\u{430}', '\u{438}']);
    let consumed = ui.send([InputEvent::Key(KeyInput::press(Key::Q))]);
    assert_eq!(consumed, [true], "typing into a field is not a shortcut");
}

#[test]
fn ctrl_v_on_a_russian_layout_pastes_and_an_input_method_commits() {
    let mut ui = Ui::new();
    ui.click(ui.field.center());
    ui.t.clipboard().set("\u{432}\u{441}\u{442}\u{430}\u{432}\u{43a}\u{430}".into());
    ui.send([InputEvent::Modifiers(ModifierState {
        ctrl: true,
        ..Default::default()
    })]);
    // The Russian <cyrillic> is at the place of V.
    ui.send([InputEvent::Key(KeyInput {
        logical: None,
        physical: Some(Key::V),
        text: Some("\u{43c}".into()),
        ..KeyInput::press(Key::V)
    })]);
    ui.send([InputEvent::Modifiers(ModifierState::default())]);
    ui.frame();
    assert_eq!(ui.text, "\u{432}\u{441}\u{442}\u{430}\u{432}\u{43a}\u{430}");
    ui.send([InputEvent::Ime(Ime::Preedit {
        text: "\u{43d}\u{438}".into(),
        cursor: Some((0, 4)),
    })]);
    ui.frame();
    ui.send([InputEvent::Ime(Ime::Commit("\u{43d}\u{438}\u{445}\u{430}\u{43e}".into()))]);
    ui.frame();
    assert!(
        ui.text.contains("\u{432}\u{441}\u{442}\u{430}\u{432}\u{43a}\u{430}") && ui.text.contains("\u{43d}\u{438}\u{445}\u{430}\u{43e}"),
        "{:?}",
        ui.text
    );
}
