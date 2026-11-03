//! What the host knows about the device and the shell cannot read from the environment: a
//! browser page has no `AURELIA_*` variables, so the page's script tells the shell (once,
//! before the first frame) that it runs on a phone. Native hosts keep using the variables.
use std::sync::atomic::{AtomicBool, Ordering};

static TOUCH: AtomicBool = AtomicBool::new(false);
static PHONE_GRAPHICS: AtomicBool = AtomicBool::new(false);

/// Pixels of the 3D scene `Auto` aims at on a phone GPU (a tablet screen is 3-4 million
/// pixels; a phone's tile-based GPU is not given more than this much of them).
pub const PHONE_SCENE_PIXELS: f32 = 1.2e6;

/// A touch screen is the main input: 48 dp touch targets and the compact phone layouts.
pub fn set_touch(on: bool) {
    TOUCH.store(on, Ordering::Relaxed);
}

/// The host says touch is the main input (see `theme::touch_wanted`).
pub fn touch() -> bool {
    TOUCH.load(Ordering::Relaxed)
}

/// The GPU is a phone's: `Auto` render scale aims at `PHONE_SCENE_PIXELS` and no multisampling,
/// whatever the adapter reports (a browser hides the adapter's type).
pub fn set_phone_graphics(on: bool) {
    PHONE_GRAPHICS.store(on, Ordering::Relaxed);
}

pub fn phone_graphics() -> bool {
    PHONE_GRAPHICS.load(Ordering::Relaxed)
}

/// The page offers to change the interface mode (phone and tablet, or desktop): a hook that
/// leaves the game, which only the page can do.
#[derive(Clone, Copy)]
pub struct ModeSwitch {
    /// The page is in the phone and tablet mode now.
    pub mobile: bool,
    /// Switch to the other mode (the page reloads).
    pub switch: fn(),
}

static MODE_SWITCH: std::sync::Mutex<Option<ModeSwitch>> = std::sync::Mutex::new(None);

pub fn set_mode_switch(switch: Option<ModeSwitch>) {
    if let Ok(mut slot) = MODE_SWITCH.lock() {
        *slot = switch;
    }
}

pub fn mode_switch() -> Option<ModeSwitch> {
    MODE_SWITCH.lock().ok().and_then(|slot| *slot)
}
