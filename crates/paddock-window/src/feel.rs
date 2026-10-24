//! The app's own windows should feel like a desktop app, not a web page: no browser zoom, no browser extensions and,
//! in release builds, none of WebView2's browser accelerator keys (reload, find, print, devtools, back/forward). Debug
//! builds keep the accelerators so developers can reload and inspect.
//!
//! The rest (the default context menu outside text fields, dragging images and links out, the keys other webviews do
//! not let the shell turn off) is the UI's native-feel guard, so the browser build behaves the same. A window that shows
//! external pages (an internal browser) is deliberately not built with this.
//!
//! Ported from Debrief's `native_feel.rs`.

/// The browser features an app window's webview keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebviewFeel {
    /// WebView2's `AreBrowserAcceleratorKeysEnabled` (Ctrl+R, F5, Ctrl+F, Ctrl+P, F12, ...). Text editing and movement
    /// keys are not affected, and every key still reaches the page.
    pub browser_accelerator_keys: bool,
    /// Ctrl +/-/0, Ctrl+wheel and pinch zoom of the whole page.
    pub zoom_hotkeys: bool,
    /// Browser extensions in the webview environment.
    pub browser_extensions: bool,
}

impl WebviewFeel {
    /// The settings for an app window: browser accelerators only in debug builds, never page zoom or extensions.
    pub const fn app_window(release: bool) -> Self {
        Self {
            browser_accelerator_keys: !release,
            zoom_hotkeys: false,
            browser_extensions: false,
        }
    }

    /// The settings for this build.
    pub const fn current() -> Self {
        Self::app_window(!cfg!(debug_assertions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_windows_turn_the_browser_keys_off() {
        let feel = WebviewFeel::app_window(true);
        assert!(!feel.browser_accelerator_keys && !feel.zoom_hotkeys && !feel.browser_extensions);
    }

    #[test]
    fn debug_windows_keep_reload_and_devtools_but_never_zoom() {
        let feel = WebviewFeel::app_window(false);
        assert!(feel.browser_accelerator_keys && !feel.zoom_hotkeys && !feel.browser_extensions);
        // Tests are debug builds.
        assert_eq!(WebviewFeel::current(), feel);
    }
}
