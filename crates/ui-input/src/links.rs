//! Opening a link: the system browser on desktop and Android, `window.open` on the web.

/// Opens `url`. `new_tab` only matters in a browser page (the `webbrowser` crate opens a tab).
/// A failure is logged and otherwise ignored; so is a build without the `links` feature.
pub fn open(url: &str, new_tab: bool) {
    let _ = new_tab;
    #[cfg(feature = "links")]
    if let Err(error) = webbrowser::open(url) {
        log::warn!("ui-input: cannot open {url}: {error}");
    }
    #[cfg(not(feature = "links"))]
    log::warn!("ui-input: cannot open {url}: the `links` feature is off");
}
