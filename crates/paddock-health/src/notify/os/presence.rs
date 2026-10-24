//! Whether the user already sees a message: a window it belongs to is in front. Debrief's `seen_in_window`, as a pure
//! function over what the windows report, so it is tested without windows.

/// One top-level window as the user sees it (a dialog or pop-out counts for its host window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowPresence {
    /// The host window's label.
    pub host: String,
    /// It has the keyboard focus.
    pub focused: bool,
    /// It is on screen (not minimised, not hidden).
    pub visible: bool,
}

/// Whether the user sees a message posted for `window` (`None`: an app-wide one, seen in any window).
pub fn user_sees(window: Option<&str>, windows: &[WindowPresence]) -> bool {
    windows
        .iter()
        .any(|w| w.focused && w.visible && window.is_none_or(|target| w.host == target))
}
