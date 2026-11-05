//! Taking a native message dialog down from the outside (the page it asked about recovered). `tauri-plugin-dialog` (rfd) gives
//! no handle to close a dialog it showed, so each OS is asked for the dialog that belongs to the window: the sheet attached to it
//! (macOS), the enabled popup it owns (Windows), the message dialog transient for it (GTK). Always on the main thread. Each finds
//! only the window's own question dialog, because the watchdog calls this only while that question is on screen (a problem report's
//! save dialog is not dismissed: [`Watchdog::close_question`](crate::Watchdog::close_question) leaves a question being answered
//! alone). The dialog's own answer, which arrives after it is closed, is ignored by the watchdog's stale-answer check.

use ::tauri::{AppHandle, Manager, Runtime, WebviewWindow};

/// Closes the question dialog of window `label`; whether one was found and asked to close.
pub fn dismiss<R: Runtime>(app: &AppHandle<R>, label: &str) -> bool {
    app.get_webview_window(label)
        .is_some_and(|window| close_for(&window))
}

/// macOS: `rfd` shows the alert as a sheet of the parent window; ending the sheet runs its completion handler (a cancelled alert).
#[cfg(target_os = "macos")]
fn close_for<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    use objc2_app_kit::NSWindow;
    let Ok(ptr) = window.ns_window() else {
        return false;
    };
    if ptr.is_null() {
        return false;
    }
    // SAFETY: Tauri hands out the window's own NSWindow, which lives as long as `window`; this runs on the main thread.
    let parent: &NSWindow = unsafe { &*ptr.cast::<NSWindow>() };
    match parent.attachedSheet() {
        Some(sheet) => {
            parent.endSheet(&sheet);
            true
        }
        None => false,
    }
}

/// Windows: the dialog (a task dialog, or a message box) is an enabled popup owned by the window, which is disabled meanwhile;
/// `WM_CLOSE` is its Cancel (the question always has a Cancel or Keep Waiting button).
#[cfg(windows)]
fn close_for<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GW_ENABLEDPOPUP, GetWindow, PostMessageW, WM_CLOSE};
    let Ok(parent) = window.hwnd() else {
        return false;
    };
    let parent = parent.0 as windows_sys::Win32::Foundation::HWND;
    // SAFETY: both calls only read or post to a window handle; a stale handle makes them fail, not misbehave.
    unsafe {
        let popup = GetWindow(parent, GW_ENABLEDPOPUP);
        // With no enabled popup the answer is the window itself.
        if popup.is_null() || popup == parent {
            return false;
        }
        PostMessageW(popup, WM_CLOSE, 0, 0) != 0
    }
}

/// Linux (GTK, `rfd`'s `gtk3` backend): the question is a `GtkMessageDialog` transient for the window's GTK window; a Cancel
/// response ends it. (The xdg-desktop-portal backend, not used by Paddock's apps, cannot be closed from here.)
#[cfg(target_os = "linux")]
fn close_for<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    use gtk::prelude::*;
    let Ok(parent) = window.gtk_window() else {
        return false;
    };
    let parent: gtk::Window = parent.upcast();
    let mut found = false;
    for top in gtk::Window::list_toplevels() {
        if let Ok(dialog) = top.downcast::<gtk::MessageDialog>()
            && dialog.transient_for().as_ref() == Some(&parent)
        {
            dialog.response(gtk::ResponseType::Cancel);
            found = true;
        }
    }
    found
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn close_for<R: Runtime>(_window: &WebviewWindow<R>) -> bool {
    false
}
