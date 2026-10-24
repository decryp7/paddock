//! What the UI sends and gets back (camelCase JSON), and the window layer's events.

use serde::Serialize;

/// To every window: the list of top-level windows changed. Payload: `WindowInfo[]`.
pub const WINDOWS_EVENT: &str = "paddock://windows";
/// To one window: the user closed it and the setting is Ask; it shows the question and answers with
/// `window_resolve_close`. No payload.
pub const CLOSE_REQUESTED_EVENT: &str = "paddock://window-close-requested";
/// To one window: files from another app wait for "open here or in a new window?"; it answers with
/// `window_resolve_open`. Payload: [`OpenAsk`].
pub const OPEN_ASK_EVENT: &str = "paddock://window-open-ask";
/// To one window: files or links wait for it; it takes them with `window_take_pending`. No payload.
pub const OPEN_EVENT: &str = "paddock://window-open";

/// A top-level window as the UI and the tray see it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub label: String,
    pub title: String,
    pub focused: bool,
    pub visible: bool,
}

/// The question "open here or in a new window?": how many files and their names (the paths stay in Rust).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAsk {
    pub count: usize,
    pub names: Vec<String>,
}

/// The kind of a [`WindowError`].
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowErrorCode {
    /// The app did not set the window layer up (a wiring bug).
    NotReady,
    /// The window is gone.
    Closed,
    /// The most windows are open already.
    TooMany,
    /// Not for this window or app (a single-window app's New Window, a label outside the scheme).
    NotAllowed,
    /// The platform refused (the message says how).
    Failed,
}

/// Why a window command failed (the command's error).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowError {
    pub code: WindowErrorCode,
    /// For the user, as is.
    pub message: String,
}

impl WindowError {
    pub fn new(code: WindowErrorCode, message: impl Into<String>) -> WindowError {
        WindowError {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for WindowError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_have_the_shape_of_a_command_error() {
        let e = WindowError::new(WindowErrorCode::TooMany, "close one first");
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({ "code": "tooMany", "message": "close one first" })
        );
        assert_eq!(e.to_string(), "close one first");
        let _: &dyn std::error::Error = &e;
    }
}
