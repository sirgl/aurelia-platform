use super::*;
use crate::clipboard::{Clipboard, MemoryClipboard};
use crate::event::InputEvent;
use crate::translate::{Platform, Wants};
use egui::output::IMEOutput;
use egui::{pos2, CursorIcon, IMEPurpose, OpenUrl, OutputCommand, PlatformOutput, Rect};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, PartialEq)]
enum Call {
    Cursor(Option<CursorIcon>),
    ImeAllowed(bool),
    ImePurpose(IMEPurpose),
    ImeArea(Rect),
    Url(String, bool),
}

#[derive(Default)]
struct Recorder {
    calls: Vec<Call>,
}

impl Host for Recorder {
    fn set_cursor(&mut self, icon: Option<CursorIcon>) {
        self.calls.push(Call::Cursor(icon));
    }

    fn set_ime_allowed(&mut self, allowed: bool) {
        self.calls.push(Call::ImeAllowed(allowed));
    }

    fn set_ime_purpose(&mut self, purpose: IMEPurpose) {
        self.calls.push(Call::ImePurpose(purpose));
    }

    fn set_ime_area(&mut self, area: Rect) {
        self.calls.push(Call::ImeArea(area));
    }

    fn open_url(&mut self, url: &str, new_tab: bool) {
        self.calls.push(Call::Url(url.to_owned(), new_tab));
    }
}

struct Nothing;

impl Wants for Nothing {
    fn pointer(&self) -> bool {
        false
    }

    fn using_pointer(&self) -> bool {
        false
    }

    fn keyboard(&self) -> bool {
        false
    }
}

/// A clipboard the test can look into after the translator owns it.
#[derive(Clone, Default)]
struct Shared(Rc<RefCell<MemoryClipboard>>);

impl Clipboard for Shared {
    fn get(&mut self) -> Option<String> {
        self.0.borrow_mut().get()
    }

    fn set(&mut self, text: String) {
        self.0.borrow_mut().set(text);
    }
}

struct Setup {
    t: Translator,
    state: OutputState,
    host: Recorder,
    clipboard: Shared,
}

fn setup() -> Setup {
    let clipboard = Shared::default();
    let t = Translator::with(Platform::default(), Box::new(clipboard.clone()), 1.0);
    Setup {
        t,
        state: OutputState::default(),
        host: Recorder::default(),
        clipboard,
    }
}

impl Setup {
    fn apply(&mut self, output: PlatformOutput, ppp: f32, had_input: bool) {
        self.t
            .apply_output(&mut self.state, output, ppp, had_input, &mut self.host);
    }

    fn calls(&mut self) -> Vec<Call> {
        std::mem::take(&mut self.host.calls)
    }

    fn pointer_inside(&mut self) {
        self.t
            .handle(InputEvent::CursorMoved { x: 1., y: 1. }, &Nothing);
    }
}

fn cursor(icon: CursorIcon) -> PlatformOutput {
    PlatformOutput {
        cursor_icon: icon,
        ..Default::default()
    }
}

fn ime(purpose: IMEPurpose, rect: Rect, interrupt: bool) -> PlatformOutput {
    PlatformOutput {
        ime: Some(IMEOutput {
            purpose,
            rect,
            cursor_rect: rect,
            should_interrupt_composition: interrupt,
        }),
        ..Default::default()
    }
}

#[test]
fn copied_text_reaches_the_clipboard_and_links_are_opened() {
    let mut s = setup();
    let output = PlatformOutput {
        commands: vec![
            OutputCommand::CopyText("hello".into()),
            OutputCommand::OpenUrl(OpenUrl {
                url: "https://example.org".into(),
                new_tab: true,
            }),
            OutputCommand::CopyImage(egui::ColorImage::example()),
        ],
        ..Default::default()
    };
    s.apply(output, 1.0, false);
    assert_eq!(s.clipboard.get().as_deref(), Some("hello"));
    assert_eq!(
        s.calls(),
        vec![Call::Url("https://example.org".into(), true)]
    );
}

#[test]
fn the_cursor_is_set_only_while_the_pointer_is_inside_and_only_when_it_changes() {
    let mut s = setup();
    s.apply(cursor(CursorIcon::PointingHand), 1.0, false);
    assert_eq!(s.calls(), vec![], "the pointer is outside the window");
    s.pointer_inside();
    s.apply(cursor(CursorIcon::PointingHand), 1.0, false);
    s.apply(cursor(CursorIcon::PointingHand), 1.0, false);
    assert_eq!(
        s.calls(),
        vec![Call::Cursor(Some(CursorIcon::PointingHand))],
        "the second identical answer is dropped"
    );
    s.apply(cursor(CursorIcon::Text), 1.0, false);
    assert_eq!(s.calls(), vec![Call::Cursor(Some(CursorIcon::Text))]);
    // Leaving forgets the icon: coming back sets it again, even though egui still says Text.
    s.t.handle(InputEvent::CursorLeft, &Nothing);
    s.apply(cursor(CursorIcon::Text), 1.0, false);
    s.pointer_inside();
    s.apply(cursor(CursorIcon::Text), 1.0, false);
    assert_eq!(s.calls(), vec![Call::Cursor(Some(CursorIcon::Text))]);
}

#[test]
fn the_none_cursor_hides_the_cursor() {
    let mut s = setup();
    s.pointer_inside();
    s.apply(cursor(CursorIcon::None), 1.0, false);
    assert_eq!(s.calls(), vec![Call::Cursor(None)]);
}

#[test]
fn the_input_method_follows_the_text_field() {
    let mut s = setup();
    let field = Rect::from_min_max(pos2(10., 20.), pos2(110., 40.));
    s.apply(ime(IMEPurpose::Normal, field, false), 2.0, false);
    assert_eq!(
        s.calls(),
        vec![
            Call::ImeAllowed(true),
            Call::ImePurpose(IMEPurpose::Normal),
            Call::ImeArea(Rect::from_min_max(pos2(20., 40.), pos2(220., 80.))),
        ],
        "the area is in physical pixels"
    );
    // Nothing changed, nothing sent, until egui saw input (the field may have moved on screen).
    s.apply(ime(IMEPurpose::Normal, field, false), 2.0, false);
    assert_eq!(s.calls(), vec![]);
    s.apply(ime(IMEPurpose::Normal, field, false), 2.0, true);
    assert_eq!(s.calls().len(), 1);
    // A password field.
    s.apply(ime(IMEPurpose::Password, field, false), 2.0, false);
    assert_eq!(s.calls(), vec![Call::ImePurpose(IMEPurpose::Password)]);
    // The field lost the focus.
    s.apply(PlatformOutput::default(), 2.0, false);
    assert_eq!(s.calls(), vec![Call::ImeAllowed(false)]);
}

#[test]
fn interrupting_a_composition_switches_the_input_method_off_and_on() {
    let mut s = setup();
    let field = Rect::from_min_max(pos2(0., 0.), pos2(10., 10.));
    s.apply(ime(IMEPurpose::Normal, field, false), 1.0, false);
    s.calls();
    s.apply(ime(IMEPurpose::Normal, field, true), 1.0, false);
    assert_eq!(
        s.calls(),
        vec![Call::ImeAllowed(false), Call::ImeAllowed(true)]
    );
}
