//! Native glue: the few lines only a real desktop runs, and that tests cannot reach by rule (rule 7: no window effects on
//! the desktop) or that read OS state (the mouse button): the pointer, showing the ghost without taking the focus, the
//! frameless window and the tracker thread's loop. Never logic: each function hands over to tested code or makes one
//! platform call. This file is left out of the coverage job (`crates/README.md`) and reviewed by hand.

use super::ghost::step;
use super::{Builder, TARGET};
use crate::redock::OpacityError;
use ::tauri::{AppHandle, Manager, Runtime, Window};
use std::time::Instant;

/// Whether the primary (usually left) mouse button is held down anywhere on screen; `None` when the platform cannot tell
/// (then the host falls back to docking when the pointer rests on a guide).
#[cfg(target_os = "macos")]
pub(crate) fn primary_button_down() -> Option<bool> {
    // Bit 0 is the primary button, already mapped for left-handed setups by the system.
    Some(objc2_app_kit::NSEvent::pressedMouseButtons() & 1 == 1)
}

#[cfg(windows)]
pub(crate) fn primary_button_down() -> Option<bool> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
    // GetAsyncKeyState reports physical buttons: with swapped buttons the primary one is the right button.
    // SAFETY: plain Win32 queries without pointers or handles.
    let key = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 {
        VK_RBUTTON
    } else {
        VK_LBUTTON
    };
    // SAFETY: as above.
    let state = unsafe { GetAsyncKeyState(i32::from(key.0)) };
    Some(state < 0)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) fn primary_button_down() -> Option<bool> {
    None
}

/// Shows the ghost without activating it (it must never take the focus from the window being dragged from).
pub(super) fn show_without_focus<R: Runtime>(w: &Window<R>) {
    #[cfg(windows)]
    if let Ok(hwnd) = w.hwnd() {
        use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow};
        // SAFETY: the handle is the live ghost window's; ShowWindow only changes its visibility.
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
        return;
    }
    let _ = w.show();
}

/// Hides the ghost.
pub(super) fn hide<R: Runtime>(w: &Window<R>) {
    #[cfg(windows)]
    if let Ok(hwnd) = w.hwnd() {
        use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
        // SAFETY: as in `show_without_focus`.
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        return;
    }
    let _ = w.hide();
}

/// Pop-outs draw the app's own compact title bar (like tool windows of an IDE). macOS keeps the native traffic lights
/// (full screen, tiling, Mission Control) over the web content; elsewhere the window has no native frame (the UI draws
/// minimise / maximise / close) but keeps the OS shadow and its resize border.
#[cfg(target_os = "macos")]
pub(super) fn frameless<R: Runtime>(b: Builder<'_, R>) -> Builder<'_, R> {
    b.title_bar_style(::tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn frameless<R: Runtime>(b: Builder<'_, R>) -> Builder<'_, R> {
    b.decorations(false).shadow(true)
}

/// Follows the cursor while session `id` lasts, on a thread of its own at about 60 Hz.
pub(super) fn spawn_tracker<R: Runtime>(app: AppHandle<R>, id: u64) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("paddock-drag-ghost".into())
        .spawn(move || {
            let started = Instant::now();
            let mut up = 0u32;
            loop {
                std::thread::sleep(crate::ghost::TICK);
                up = if primary_button_down() == Some(false) {
                    up + 1
                } else {
                    0
                };
                if !step(&app, id, up, started.elapsed()) {
                    log::debug!(target: TARGET, Session = id; "Drag ghost tracker stopped");
                    return;
                }
            }
        })
        .map(|_| ())
}

/// Lets mouse events pass through the ghost window (it only shows where the pop-out will open). A platform that cannot
/// leaves the ghost clickable; it is harmless.
pub(super) fn make_click_through<R: Runtime>(w: &Window<R>) {
    let _ = w
        .set_ignore_cursor_events(true)
        .inspect_err(|e| warn("The drag ghost cannot be made click-through", e));
}

fn warn(what: &str, e: &dyn std::fmt::Display) {
    log::warn!(target: TARGET, Error = e.to_string().as_str(); "{what}");
}

/// The ghost's content could not be handed to its page (cosmetic: the UI keeps its in-window preview).
pub(super) fn content_lost(e: &::tauri::Error) {
    warn("Could not hand the drag ghost its content", e);
}

/// The ghost window could not be created; the dragging window shows its own preview.
pub(super) fn ghost_unavailable(e: &::tauri::Error) {
    warn(
        "Could not create the drag ghost; the window shows its own preview",
        e,
    );
}

/// The ghost page finished loading: it takes its content now (the event cannot be raised on the mock runtime).
pub(super) fn on_page_load<R: Runtime>(
    w: ::tauri::WebviewWindow<R>,
    payload: ::tauri::webview::PageLoadPayload<'_>,
) {
    if payload.event() == ::tauri::webview::PageLoadEvent::Finished {
        super::ghost::page_loaded(w.app_handle());
    }
}

/// Sets the window's opacity (1.0 is fully opaque). Call on the main thread.
#[cfg(target_os = "macos")]
pub(super) fn set_window_opacity<R: Runtime>(
    w: &Window<R>,
    opacity: f64,
) -> Result<(), OpacityError> {
    let ptr = w
        .ns_window()
        .map_err(|e| OpacityError::Failed(e.to_string()))?;
    if ptr.is_null() {
        return Err(OpacityError::Failed("the window has no NSWindow".into()));
    }
    // SAFETY: Tauri hands out the live NSWindow of this window; this runs on the main thread, as AppKit requires, and the
    // reference does not outlive the call.
    let ns_window = unsafe { &*ptr.cast::<objc2_app_kit::NSWindow>() };
    ns_window.setAlphaValue(opacity);
    Ok(())
}

#[cfg(windows)]
pub(super) fn set_window_opacity<R: Runtime>(
    w: &Window<R>,
    opacity: f64,
) -> Result<(), OpacityError> {
    use windows::Win32::Foundation::{COLORREF, GetLastError, HWND, SetLastError, WIN32_ERROR};
    use windows::Win32::Graphics::Gdi::{
        RDW_ALLCHILDREN, RDW_ERASE, RDW_FRAME, RDW_INVALIDATE, RedrawWindow,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongPtrW,
        WS_EX_LAYERED,
    };

    /// Sets the extended style; SetWindowLongPtrW answers the old value, so 0 is an error only when the last error is set.
    ///
    /// # Safety
    /// `hwnd` is a live window of this process.
    unsafe fn set_style(hwnd: HWND, style: isize) -> Result<(), OpacityError> {
        // SAFETY: as the function's contract says; the last-error calls take no pointers.
        unsafe {
            SetLastError(WIN32_ERROR(0));
            let old = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
            let error = GetLastError();
            if old == 0 && error.0 != 0 {
                return Err(OpacityError::Failed(format!(
                    "SetWindowLongPtrW failed with error {}",
                    error.0
                )));
            }
        }
        Ok(())
    }

    let hwnd = w.hwnd().map_err(|e| OpacityError::Failed(e.to_string()))?;
    let layered = WS_EX_LAYERED.0 as isize;
    // SAFETY: the handle is the live window's; these calls only change its extended style, alpha and repaint it.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if opacity >= 1.0 {
            if style & layered == 0 {
                return Ok(());
            }
            // "Using Layered Windows": make it fully opaque first, then drop the style, then repaint everything so no
            // stale layered surface is left (frame, children and the WebView2 child included).
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)
                .map_err(|e| OpacityError::Failed(e.to_string()))?;
            set_style(hwnd, style & !layered)?;
            let _ = RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_ERASE | RDW_INVALIDATE | RDW_FRAME | RDW_ALLCHILDREN,
            );
            return Ok(());
        }
        if style & layered == 0 {
            set_style(hwnd, style | layered)?;
        }
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA)
            .map_err(|e| OpacityError::Failed(e.to_string()))
    }
}

/// The primary mouse button as the drag follower reads it, on the main thread with the dragged pop-out `w`. macOS and
/// Windows read it system-wide; X11 asks GDK for the pointer's button state through the window. Wayland tells a client
/// nothing about the pointer outside its own surfaces: there the cursor position fails too and the feature never starts.
#[cfg(any(target_os = "macos", windows))]
pub(super) fn button_down_for<R: Runtime>(_w: &Window<R>) -> Option<bool> {
    primary_button_down()
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn button_down_for<R: Runtime>(w: &Window<R>) -> Option<bool> {
    use gtk::gdk::ModifierType;
    use gtk::gdk::prelude::SeatExt;
    use gtk::prelude::WidgetExt;
    let gdk_window = w.gtk_window().ok()?.window()?;
    let pointer = gtk::gdk::Display::default()?.default_seat()?.pointer()?;
    let (_, _, _, mask) = gdk_window.device_position(&pointer);
    Some(mask.contains(ModifierType::BUTTON1_MASK))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn set_window_opacity<R: Runtime>(
    w: &Window<R>,
    opacity: f64,
) -> Result<(), OpacityError> {
    use gtk::prelude::WidgetExt;
    let window = w
        .gtk_window()
        .map_err(|e| OpacityError::Failed(e.to_string()))?;
    // Without a compositing screen GTK ignores the opacity: then there is nothing to show, and the guides get an outline.
    if !window.screen().is_some_and(|s| s.is_composited()) {
        return Err(OpacityError::Unsupported(
            "the screen has no compositing manager".into(),
        ));
    }
    window.set_opacity(opacity);
    Ok(())
}

/// Does the user ask the system to reduce transparency? (macOS "Reduce transparency", Windows "Transparency effects"
/// off.) Other platforms expose nothing: false.
#[cfg(target_os = "macos")]
pub(super) fn reduce_transparency() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
}

#[cfg(windows)]
pub(super) fn reduce_transparency() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    use windows::core::w;
    let mut data = 1u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: `data` and `size` live through the call and `size` is its byte length.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("EnableTransparency"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut data).cast()),
            Some(&raw mut size),
        )
    };
    // A missing value means the default: transparency on.
    status.is_ok() && data == 0
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(super) fn reduce_transparency() -> bool {
    false
}
