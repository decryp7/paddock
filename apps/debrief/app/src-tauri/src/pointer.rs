//! The state of the mouse outside any webview. A pop-out window dragged by the OS gives its host no "dropped" event,
//! so the host asks whether the primary button is still held to know when the window was released.

/// Whether the primary (usually left) mouse button is held down anywhere on screen; `None` when the platform cannot
/// tell (then the host falls back to docking when the pointer rests on a guide).
#[tauri::command]
pub async fn pointer_button_down() -> Option<bool> {
    platform_button_down()
}

/// What the host acts on: the platform's answer, or in tests the test's own (see [`hold_primary_button_for_test`]).
pub(crate) fn primary_button_down() -> Option<bool> {
    #[cfg(test)]
    return TEST_BUTTON.with(std::cell::Cell::get);
    #[cfg(not(test))]
    platform_button_down()
}

#[cfg(target_os = "macos")]
fn platform_button_down() -> Option<bool> {
    // Bit 0 is the primary button, already mapped for left-handed setups by the system.
    Some(primary_in_mask(objc2_app_kit::NSEvent::pressedMouseButtons()))
}

/// Whether bit 0 (the primary button) is set in AppKit's pressed-buttons mask.
#[cfg(any(target_os = "macos", test))]
fn primary_in_mask(mask: usize) -> bool {
    mask & 1 == 1
}

#[cfg(windows)]
fn platform_button_down() -> Option<bool> {
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
fn platform_button_down() -> Option<bool> {
    None
}

#[cfg(test)]
thread_local! {
    // The button as this test thread sets it; up unless a test holds it.
    static TEST_BUTTON: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(Some(false)) };
}

/// Tests set what the primary button reports on their thread, so no test reads the real mouse of the machine it runs on (a
/// click on the shared CI Mac failed a drag test, Debrief run 7955; ER-14).
#[cfg(test)]
pub(crate) fn hold_primary_button_for_test(down: Option<bool>) {
    TEST_BUTTON.with(|b| b.set(down));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Now as _;

    #[test]
    fn reports_the_button_where_the_platform_can() {
        // Whether the real mouse is held is the machine's, not the test's: only whether the platform can tell.
        let state = pointer_button_down().now();
        assert_eq!(state.is_some(), cfg!(any(target_os = "macos", windows)));
    }

    #[test]
    fn the_primary_button_is_bit_zero_of_the_mask() {
        assert!(!primary_in_mask(0));
        assert!(primary_in_mask(1));
        assert!(primary_in_mask(0b11));
        assert!(!primary_in_mask(0b10));
    }
}
