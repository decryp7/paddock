//! What closing a window and asking to exit do, as pure decisions: closing the last window follows the user's setting
//! (quit, keep running in the tray, or ask); without a tray there is nothing to keep running in, so the app quits; only
//! an explicit Quit exits a tray app; a start in the tray (`--hidden`) creates no window when a tray can bring it back.
//!
//! Ported from Debrief's `window_manager.rs` and Pitwall's `window.rs` (the tray-app lessons: closing destroys the
//! window, which frees its webview; a hidden WKWebView came back blank).

use serde::{Deserialize, Serialize};

/// What closing the last window does (a user setting).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnClose {
    /// Quit the app.
    Quit,
    /// Keep running in the tray.
    Tray,
    /// Ask the user (the window shows a prompt and answers with `window_resolve_close`).
    #[default]
    Ask,
}

/// What keeping running in the tray does to the last window: destroy it (the default: frees the webview, and a
/// reopened window is rebuilt from the configuration) or hide it (keeps its in-memory state).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Background {
    #[default]
    Destroy,
    Hide,
}

/// What a close request does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    /// Close it (other windows remain).
    Close,
    /// The app keeps running in the tray ([`Background`] says what happens to the window).
    Background,
    /// Quit the app.
    Quit,
    /// Ask the user.
    Ask,
}

/// Closing a window while others are visible just closes it. Closing the last visible one follows the setting;
/// without a tray icon there is nothing to keep running in, so the app quits. Closing again while the question is
/// open quits (the UI may be stuck).
pub fn decide_close(
    pref: OnClose,
    other_visible: usize,
    tray_available: bool,
    already_asked: bool,
) -> CloseDecision {
    if other_visible > 0 {
        return CloseDecision::Close;
    }
    match pref {
        OnClose::Quit => CloseDecision::Quit,
        OnClose::Tray | OnClose::Ask if !tray_available => CloseDecision::Quit,
        OnClose::Tray => CloseDecision::Background,
        OnClose::Ask if already_asked => CloseDecision::Quit,
        OnClose::Ask => CloseDecision::Ask,
    }
}

/// Whether an exit request is refused so the app keeps running in the tray: Tauri asks to exit without a code when
/// the last window was destroyed, and with one for an explicit exit (Quit). Once quitting has begun, nothing is refused.
pub fn keeps_running(code: Option<i32>, tray_available: bool, quitting: bool) -> bool {
    code.is_none() && tray_available && !quitting
}

/// Whether the main window is created at start: not for a start in the tray (`--hidden`) when a tray can bring it
/// back; without a tray there is no other way in.
pub fn creates_main_at_start(hidden: bool, tray_available: bool) -> bool {
    !(hidden && tray_available)
}

/// The user's answer to the close question.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseChoice {
    Quit,
    Tray,
}

impl CloseChoice {
    /// The setting this answer becomes when the user ticks "remember".
    pub fn setting(self) -> OnClose {
        match self {
            CloseChoice::Quit => OnClose::Quit,
            CloseChoice::Tray => OnClose::Tray,
        }
    }
}

/// The user's answer to "open here or in a new window?".
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenChoice {
    /// Open the waiting files in the asking window.
    Here,
    NewWindow,
    /// The question was dismissed: the waiting files are dropped.
    Cancel,
}

/// The longest window title, in characters.
pub const MAX_TITLE: usize = 200;

/// A window title from the UI: one line, at most [`MAX_TITLE`] characters, never empty (`default` instead).
pub fn clean_title(title: &str, default: &str) -> String {
    let t: String = title
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE)
        .collect();
    let t = t.trim();
    if t.is_empty() {
        default.to_string()
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_decisions() {
        use CloseDecision::*;
        assert_eq!(
            decide_close(OnClose::Ask, 1, true, false),
            Close,
            "other windows remain"
        );
        assert_eq!(decide_close(OnClose::Quit, 0, true, false), Quit);
        assert_eq!(decide_close(OnClose::Tray, 0, true, false), Background);
        assert_eq!(
            decide_close(OnClose::Tray, 0, false, false),
            Quit,
            "no tray to keep running in"
        );
        assert_eq!(decide_close(OnClose::Ask, 0, true, false), Ask);
        assert_eq!(decide_close(OnClose::Ask, 0, false, false), Quit);
        assert_eq!(
            decide_close(OnClose::Ask, 0, true, true),
            Quit,
            "closing again while asked"
        );
        assert_eq!(OnClose::default(), OnClose::Ask);
        assert_eq!(super::Background::default(), super::Background::Destroy);
    }

    #[test]
    fn only_an_explicit_quit_exits_a_tray_app() {
        assert!(keeps_running(None, true, false), "the last window closed");
        assert!(!keeps_running(None, false, false), "no tray: closing quits");
        assert!(!keeps_running(Some(0), true, false), "Quit");
        assert!(!keeps_running(None, true, true), "quitting already");
    }

    #[test]
    fn a_login_start_creates_no_window_only_when_a_tray_can_bring_it_back() {
        assert!(!creates_main_at_start(true, true));
        assert!(creates_main_at_start(true, false));
        assert!(creates_main_at_start(false, true));
    }

    #[test]
    fn answers_parse_from_the_wire_and_become_settings() {
        let close = |s: &str| serde_json::from_value::<CloseChoice>(serde_json::json!(s));
        assert_eq!(close("quit").unwrap().setting(), OnClose::Quit);
        assert_eq!(close("tray").unwrap().setting(), OnClose::Tray);
        assert!(close("Quit").is_err());
        let open = |s: &str| serde_json::from_value::<OpenChoice>(serde_json::json!(s));
        assert_eq!(open("here").unwrap(), OpenChoice::Here);
        assert_eq!(open("newWindow").unwrap(), OpenChoice::NewWindow);
        assert_eq!(open("cancel").unwrap(), OpenChoice::Cancel);
        assert!(open("true").is_err() && open("Here").is_err());
        assert_eq!(
            serde_json::from_value::<OnClose>(serde_json::json!("tray")).unwrap(),
            OnClose::Tray
        );
    }

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(clean_title("  Incident\n — App ", "App"), "Incident — App");
        assert_eq!(clean_title("\u{7}", "App"), "App");
        assert_eq!(clean_title(&"x".repeat(500), "App").len(), MAX_TITLE);
    }
}
