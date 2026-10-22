//! Pitwall's window should feel like a desktop app, not a web page: no browser
//! zoom, no browser extensions and, in release builds, none of WebView2's browser accelerator keys (reload, find,
//! print, devtools, back/forward). Debug builds keep the accelerators so developers can reload and inspect.
//!
//! The rest (the default context menu outside text fields, dragging images and links out, the keys other webviews
//! do not let the shell turn off) is handled by the UI's `native-feel` guard, so the browser build behaves the same.
//! Ported exactly from Debrief's `native_feel.rs` (Debrief ADR 0028); Pitwall has no pop-out windows or internal browser.

use tauri::{Manager, Runtime, WebviewWindow, WebviewWindowBuilder};

/// The browser features an app window's webview keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebviewFeel {
    /// WebView2's `AreBrowserAcceleratorKeysEnabled` (Ctrl+R, F5, Ctrl+F, Ctrl+P, F12, ...). Text editing and
    /// movement keys are not affected, and every key still reaches the page.
    pub browser_accelerator_keys: bool,
    /// Ctrl +/-/0, Ctrl+wheel and pinch zoom of the whole page (`IsZoomControlEnabled`, `IsPinchZoomEnabled`).
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

/// Builds an app window with [`WebviewFeel::current`] applied. Use it for every Pitwall window.
pub fn build<'a, R: Runtime, M: Manager<R>>(builder: WebviewWindowBuilder<'a, R, M>) -> tauri::Result<WebviewWindow<R>> {
    let feel = WebviewFeel::current();
    let window = builder
        .zoom_hotkeys_enabled(feel.zoom_hotkeys)
        .browser_extensions_enabled(feel.browser_extensions)
        .build()?;
    apply_settings(&window, feel);
    Ok(window)
}

/// Applies the shell-side settings to a window Tauri created from `tauri.conf.json` at start (its builder options are
/// in the configuration: `zoomHotkeysEnabled: false`).
pub fn apply_to_existing<R: Runtime>(window: &WebviewWindow<R>) {
    apply_settings(window, WebviewFeel::current());
}

/// Applies what the builder cannot set. A failure only leaves the browser default in place, so it is logged.
fn apply_settings<R: Runtime>(window: &WebviewWindow<R>, feel: WebviewFeel) {
    if feel.browser_accelerator_keys {
        return;
    }
    #[cfg(windows)]
    {
        let label = window.label().to_string();
        let result = window.with_webview(move |webview| {
            if let Err(e) = disable_browser_accelerator_keys(&webview) {
                eprintln!("Pitwall: could not turn off the browser keys in window {label}: {e}");
            }
        });
        if let Err(e) = result {
            eprintln!("Pitwall: could not reach the webview to turn off the browser keys: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// Turns off WebView2's browser accelerator keys (applies from the next navigation commit: the app's first page
/// load, as this runs right after the window is built). The UI guard covers the keys it can in between.
#[cfg(windows)]
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
        assert!(!feel.browser_extensions);
    }

    #[test]
    fn debug_windows_keep_reload_and_devtools_but_never_zoom() {
        let feel = WebviewFeel::app_window(false);
        assert!(feel.browser_accelerator_keys);
        assert!(!feel.zoom_hotkeys);
        assert!(!feel.browser_extensions);
        // Tests are debug builds.
        assert_eq!(WebviewFeel::current(), feel);
    }

    #[test]
    fn windows_are_built_with_the_feel_applied() {
        let app = tauri::test::mock_app();
        let built = build(tauri::WebviewWindowBuilder::new(&app, "feel", tauri::WebviewUrl::default()));
        assert!(built.is_ok());
        // Release settings on a (mock) window are a no-op off Windows and never fail the build.
        if let Ok(w) = built {
            apply_settings(&w, WebviewFeel::app_window(true));
        }
    }
}
