//! Spotter's window should feel like a desktop app, not a web page (ADR 0003, after Debrief's ADR 0028): no browser
//! zoom and, in release builds on Windows, none of WebView2's browser accelerator keys (reload, find, print,
//! devtools, back/forward). Debug builds keep the accelerators so developers can reload and inspect.
//!
//! The window is declared in `tauri.conf.json`, which also sets `zoomHotkeysEnabled: false`. The rest (the default
//! context menu, dragging links out, the keys the other webviews do not let the shell turn off, files dropped on the
//! window) is handled by the UI's `native-feel` guard, so the browser build behaves the same.

use tauri::{Runtime, WebviewWindow};

/// The browser features the app window's webview keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebviewFeel {
    /// WebView2's `AreBrowserAcceleratorKeysEnabled` (Ctrl+R, F5, Ctrl+F, Ctrl+P, F12, ...). Text editing and
    /// movement keys are not affected, and every key still reaches the page.
    pub browser_accelerator_keys: bool,
    /// Ctrl +/-/0, Ctrl+wheel and pinch zoom of the whole page (set in `tauri.conf.json`).
    pub zoom_hotkeys: bool,
}

impl WebviewFeel {
    /// The settings for an app window: browser accelerators only in debug builds, never page zoom.
    pub const fn app_window(release: bool) -> Self {
        Self {
            browser_accelerator_keys: !release,
            zoom_hotkeys: false,
        }
    }

    /// The settings for this build.
    pub const fn current() -> Self {
        Self::app_window(!cfg!(debug_assertions))
    }
}

/// Applies what the window configuration cannot set. A failure only leaves the browser default in place, so it is
/// reported on standard error (no window content is logged).
pub fn apply<R: Runtime>(window: &WebviewWindow<R>) {
    apply_settings(window, WebviewFeel::current());
}

fn apply_settings<R: Runtime>(window: &WebviewWindow<R>, feel: WebviewFeel) {
    if feel.browser_accelerator_keys {
        return;
    }
    #[cfg(windows)]
    {
        let result = window.with_webview(|webview| {
            if let Err(e) = disable_browser_accelerator_keys(&webview) {
                eprintln!("Spotter: could not turn off the browser keys: {e}");
            }
        });
        if let Err(e) = result {
            eprintln!("Spotter: could not reach the webview to turn off the browser keys: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// Turns off WebView2's browser accelerator keys (from the next navigation commit; the UI guard covers the keys it
/// can in between).
#[cfg(windows)]
#[allow(unsafe_code)]
fn disable_browser_accelerator_keys(webview: &tauri::webview::PlatformWebview) -> windows::core::Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows::core::Interface;
    // SAFETY: COM calls on the webview's own controller, on the thread `with_webview` runs them on (the UI thread).
    unsafe {
        let settings: ICoreWebView2Settings3 = webview.controller().CoreWebView2()?.Settings()?.cast()?;
        settings.SetAreBrowserAcceleratorKeysEnabled(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_windows_turn_the_browser_keys_off() {
        let feel = WebviewFeel::app_window(true);
        assert!(!feel.browser_accelerator_keys);
        assert!(!feel.zoom_hotkeys);
    }

    #[test]
    fn debug_windows_keep_reload_and_devtools_but_never_zoom() {
        let feel = WebviewFeel::app_window(false);
        assert!(feel.browser_accelerator_keys);
        assert!(!feel.zoom_hotkeys);
        // Tests are debug builds.
        assert_eq!(WebviewFeel::current(), feel);
    }
}
