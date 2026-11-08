//! Windows: the dialog is `rfd`'s task dialog, owned by the stuck window and shown on a thread of its own, so it has no handle. It is
//! found by what it is (see [`is_question_dialog`]) and checked again right before `WM_CLOSE` is posted (rule 53).

use super::identity::is_question_dialog;
use ::tauri::{AppHandle, Manager, Runtime};
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetClassNameW, GetWindow, GetWindowTextW, IsWindowVisible, PostMessageW,
    WM_CLOSE,
};

struct Search {
    parent: HWND,
    title: String,
    found: Vec<HWND>,
}

/// Whether `hwnd` is the question dialog owned by `parent` titled `title`, read now.
fn is_ours(hwnd: HWND, parent: HWND, title: &str) -> bool {
    let (mut class, mut text) = ([0u16; 64], [0u16; 256]);
    // SAFETY: the buffers are as long as the lengths passed; a window that has gone makes the calls fail and the checks false.
    let (class, text, visible, owner) = unsafe {
        let c = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
        let t = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32).max(0) as usize;
        (
            String::from_utf16_lossy(&class[..c]),
            String::from_utf16_lossy(&text[..t]),
            IsWindowVisible(hwnd) != 0,
            GetWindow(hwnd, GW_OWNER),
        )
    };
    is_question_dialog(&class, visible, owner == parent, &text, title)
}

unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> i32 {
    // SAFETY: `lparam` is the `Search` that `dismiss` passed and keeps alive for the whole `EnumWindows` call.
    let search = unsafe { &mut *(lparam as *mut Search) };
    if is_ours(hwnd, search.parent, &search.title) {
        search.found.push(hwnd);
    }
    1
}

/// Asks the question dialog of window `label` titled `title` to close; whether one was found.
pub fn dismiss<R: Runtime>(app: &AppHandle<R>, label: &str, title: &str) -> bool {
    let Some(parent) = app.get_webview_window(label).and_then(|w| w.hwnd().ok()) else {
        return false;
    };
    let mut search = Search {
        parent: parent.0 as HWND,
        title: title.to_string(),
        found: Vec::new(),
    };
    // SAFETY: `each` only reads window state and pushes into `search`, which outlives the call.
    unsafe { EnumWindows(Some(each), &mut search as *mut Search as LPARAM) };
    let mut closed = false;
    for hwnd in search.found {
        // A window handle is a reusable name: look again just before acting on it.
        if is_ours(hwnd, search.parent, &search.title) {
            // SAFETY: posting a message to a window handle that may have gone fails harmlessly.
            closed |= unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } != 0;
        }
    }
    closed
}
