//! The clipboard behind Ctrl+C / Ctrl+V.
//!
//! * Desktop (Windows, macOS, Linux): the system clipboard through `arboard`. When the system
//!   clipboard cannot be opened (no display server, a sandbox), text still travels inside the
//!   program: what was copied can be pasted back.
//! * Web (wasm32): copying also writes the browser clipboard (`navigator.clipboard.writeText`,
//!   fire and forget; it needs a secure context and a user gesture, otherwise it is ignored). A
//!   browser lets a page READ the clipboard only inside a paste event or after the user grants a
//!   permission, and winit's web backend delivers no paste event, so pasting from outside the page
//!   is not supported: paste returns the last text copied inside the page.
//! * Android: the text stays inside the program (no JNI to `ClipboardManager` yet).

/// Where copied text goes and pasted text comes from.
pub trait Clipboard {
    /// The text to paste, if there is any.
    fn get(&mut self) -> Option<String>;
    /// Puts `text` on the clipboard.
    fn set(&mut self, text: String);
}

/// A clipboard that lives inside the program: the fallback, and what the tests use.
#[derive(Clone, Debug, Default)]
pub struct MemoryClipboard {
    text: Option<String>,
}

impl MemoryClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clipboard for MemoryClipboard {
    fn get(&mut self) -> Option<String> {
        self.text.clone()
    }

    fn set(&mut self, text: String) {
        self.text = Some(text);
    }
}

/// The clipboard of the platform this build runs on (see the module documentation).
pub struct SystemClipboard {
    inside: MemoryClipboard,
    #[cfg(all(
        feature = "clipboard",
        not(any(target_os = "android", target_arch = "wasm32"))
    ))]
    system: Option<arboard::Clipboard>,
}

impl SystemClipboard {
    pub fn new() -> Self {
        Self {
            inside: MemoryClipboard::new(),
            #[cfg(all(
                feature = "clipboard",
                not(any(target_os = "android", target_arch = "wasm32"))
            ))]
            system: match arboard::Clipboard::new() {
                Ok(clipboard) => Some(clipboard),
                Err(error) => {
                    log::warn!("ui-input: no system clipboard ({error}); copy and paste stay inside the program");
                    None
                }
            },
        }
    }
}

impl Default for SystemClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for SystemClipboard {
    fn get(&mut self) -> Option<String> {
        #[cfg(all(
            feature = "clipboard",
            not(any(target_os = "android", target_arch = "wasm32"))
        ))]
        if let Some(system) = self.system.as_mut() {
            match system.get_text() {
                Ok(text) => return Some(text),
                // An empty or non-text clipboard is no reason to paste an older copy.
                Err(arboard::Error::ContentNotAvailable) => return None,
                Err(error) => log::warn!("ui-input: cannot read the clipboard ({error})"),
            }
        }
        self.inside.get()
    }

    fn set(&mut self, text: String) {
        #[cfg(all(
            feature = "clipboard",
            not(any(target_os = "android", target_arch = "wasm32"))
        ))]
        if let Some(system) = self.system.as_mut() {
            if let Err(error) = system.set_text(text.clone()) {
                log::warn!("ui-input: cannot write the clipboard ({error})");
            }
        }
        #[cfg(target_arch = "wasm32")]
        browser_write(&text);
        self.inside.set(text);
    }
}

/// `navigator.clipboard.writeText(text)`, called by name so that the crate needs neither the
/// unstable `web-sys` clipboard API nor a `--cfg` flag. Any failure is ignored on purpose.
#[cfg(target_arch = "wasm32")]
fn browser_write(text: &str) {
    use wasm_bindgen::{JsCast, JsValue};
    let Some(window) = web_sys::window() else {
        return;
    };
    let navigator: JsValue = window.navigator().into();
    let Ok(clipboard) = js_sys::Reflect::get(&navigator, &JsValue::from_str("clipboard")) else {
        return;
    };
    if clipboard.is_undefined() || clipboard.is_null() {
        return;
    }
    let Ok(write) = js_sys::Reflect::get(&clipboard, &JsValue::from_str("writeText")) else {
        return;
    };
    let Ok(write) = write.dyn_into::<js_sys::Function>() else {
        return;
    };
    // A rejected promise (no permission, no user gesture) is dropped with the returned value.
    let _ = write.call1(&clipboard, &JsValue::from_str(text));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_clipboard_returns_the_last_copy() {
        let mut clipboard = MemoryClipboard::new();
        assert_eq!(clipboard.get(), None);
        clipboard.set("one".into());
        clipboard.set("two".into());
        assert_eq!(clipboard.get().as_deref(), Some("two"));
    }
}
