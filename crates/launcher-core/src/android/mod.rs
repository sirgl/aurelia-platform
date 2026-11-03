//! Aurelia on Android: a NativeActivity window driven by winit's Android event loop, the shell of
//! `client` on top of it. Android-specific code lives only here (and in `client::shell::touch`,
//! which is platform-neutral). The launcher crate (`lands/launcher/android`, `classic/launcher/android`)
//! has the `android_main` entry point and calls [`run`] with its module.

mod host;
mod platform;

pub use winit::platform::android::activity::AndroidApp;

use client::shell::module::ModuleUi;

use crate::Layers;

/// The activity's body (call from `android_main`): logging, folders, then the game with `module`.
pub fn run<U: ModuleUi + 'static>(app: AndroidApp, module: fn() -> (U, Layers)) {
    platform::init_logging();
    let paths = platform::prepare(&app);
    host::run(app, paths, module);
}
