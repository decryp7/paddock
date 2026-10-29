//! Native glue: the few lines only a real desktop runs, and that tests cannot reach by rule (rule 7: no window effects on
//! the desktop) or that read OS state (the mouse button): the pointer, showing the ghost without taking the focus, the
//! frameless window and the tracker thread's loop. Never logic: each function hands over to tested code or makes one
//! platform call. This file is left out of the coverage job (`crates/README.md`) and reviewed by hand.

use super::ghost::step;
use super::{Builder, TARGET};
use ::tauri::{AppHandle, Manager, Runtime, Window};
use std::time::Instant;

/// How often a test process asked the OS for the mouse button: tests never may (rule 7), their app's pointer answers.
#[cfg(test)]
pub(crate) static OS_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn count_os_read() {
    #[cfg(test)]
    OS_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// Whether the primary (usually left) mouse button is held down anywhere on screen; `None` when the platform cannot tell
/// (then the host falls back to docking when the pointer rests on a guide).
#[cfg(target_os = "macos")]
pub(crate) fn primary_button_down() -> Option<bool> {
    count_os_read();
    // Bit 0 is the primary button, already mapped for left-handed setups by the system.
    Some(objc2_app_kit::NSEvent::pressedMouseButtons() & 1 == 1)
}

#[cfg(windows)]
pub(crate) fn primary_button_down() -> Option<bool> {
    count_os_read();
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
    count_os_read();
    None
}

/// The OS's mouse button, the default [`super::Pointer`] (`NSEvent` on macOS, `GetAsyncKeyState` on Windows; elsewhere
/// it cannot tell).
pub(super) struct OsPointer;

impl super::Pointer for OsPointer {
    fn primary_button_down(&self) -> Option<bool> {
        primary_button_down()
    }
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
pub(super) fn spawn_tracker<R: Runtime>(
    app: AppHandle<R>,
    id: u64,
    pointer: std::sync::Arc<dyn super::Pointer>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("paddock-drag-ghost".into())
        .spawn(move || {
            let started = Instant::now();
            let mut up = 0u32;
            loop {
                std::thread::sleep(crate::ghost::TICK);
                up = if pointer.primary_button_down() == Some(false) {
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
