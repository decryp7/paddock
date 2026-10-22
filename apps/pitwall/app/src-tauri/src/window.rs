//! The window lifecycle as pure decisions (docs/suite-lessons.md, "Tray apps"):
//!
//! - Closing the window **destroys** it (frees the webview; a hidden WKWebView came back blank). The app keeps
//!   running in the tray / menu bar.
//! - Tray "Open" and the macOS Dock icon (Reopen) **rebuild** it from the configuration, or focus it when it exists.
//! - Only an explicit Quit exits.
//! - With no tray (some Linux desktops) the window is the only way in, so closing it quits the app.
//!
//! The server keeps serving in every case; quitting the app never stops it.

/// The main window's label (`tauri.conf.json`).
pub const MAIN: &str = "main";

/// The argument the autostart entry passes: start in the tray without a window.
pub const HIDDEN_ARG: &str = "--hidden";

/// Something that happened to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// Tauri asks to exit. `code` is `None` when the last window closed and `Some` for an explicit `exit(code)`.
    ExitRequested {
        /// The exit code of an explicit exit.
        code: Option<i32>,
    },
    /// The user clicked the Dock icon of the running app (macOS "Reopen").
    DockReopen,
    /// The user chose Open in the tray menu, or clicked the tray icon (Windows, Linux).
    TrayOpen,
    /// The app was started at log-in with `--hidden`.
    StartedHidden,
}

/// What the app does about a [`Lifecycle`] event, given whether the main window exists and whether there is a tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    /// Prevent the exit: the app stays in the tray.
    KeepRunning,
    /// Let the exit happen.
    Exit,
    /// Bring the existing window to the front.
    FocusWindow,
    /// Build the window again from the configuration.
    CreateWindow,
    /// Destroy the window that was created at start (no webview at log-in).
    DestroyWindow,
    /// Nothing.
    Nothing,
}

/// The decision table.
pub fn react(event: Lifecycle, window_exists: bool, has_tray: bool) -> Reaction {
    match event {
        Lifecycle::ExitRequested { code: None } if has_tray => Reaction::KeepRunning,
        Lifecycle::ExitRequested { .. } => Reaction::Exit,
        Lifecycle::DockReopen | Lifecycle::TrayOpen if window_exists => Reaction::FocusWindow,
        Lifecycle::DockReopen | Lifecycle::TrayOpen => Reaction::CreateWindow,
        Lifecycle::StartedHidden if has_tray && window_exists => Reaction::DestroyWindow,
        // Without a tray there is no way back to a hidden app: show the window.
        Lifecycle::StartedHidden => Reaction::Nothing,
    }
}

/// Whether the arguments ask for a start in the tray.
pub fn started_hidden(args: &[String]) -> bool {
    args.iter().any(|a| a == HIDDEN_ARG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_last_window_keeps_the_app_in_the_tray() {
        assert_eq!(react(Lifecycle::ExitRequested { code: None }, false, true), Reaction::KeepRunning);
    }

    #[test]
    fn closing_the_last_window_without_a_tray_quits() {
        assert_eq!(react(Lifecycle::ExitRequested { code: None }, false, false), Reaction::Exit);
    }

    #[test]
    fn an_explicit_quit_always_exits() {
        assert_eq!(react(Lifecycle::ExitRequested { code: Some(0) }, false, true), Reaction::Exit);
        assert_eq!(react(Lifecycle::ExitRequested { code: Some(0) }, true, true), Reaction::Exit);
        assert_eq!(react(Lifecycle::ExitRequested { code: Some(1) }, false, false), Reaction::Exit);
    }

    #[test]
    fn the_tray_and_the_dock_rebuild_a_destroyed_window_and_focus_an_existing_one() {
        for ev in [Lifecycle::TrayOpen, Lifecycle::DockReopen] {
            assert_eq!(react(ev, false, true), Reaction::CreateWindow, "{ev:?}");
            assert_eq!(react(ev, true, true), Reaction::FocusWindow, "{ev:?}");
        }
    }

    #[test]
    fn a_login_start_drops_the_webview_only_when_a_tray_can_bring_it_back() {
        assert_eq!(react(Lifecycle::StartedHidden, true, true), Reaction::DestroyWindow);
        assert_eq!(react(Lifecycle::StartedHidden, true, false), Reaction::Nothing);
        assert_eq!(react(Lifecycle::StartedHidden, false, true), Reaction::Nothing);
    }

    #[test]
    fn the_hidden_argument_is_recognised() {
        assert!(started_hidden(&["pitwall-app".into(), "--hidden".into()]));
        assert!(!started_hidden(&["pitwall-app".into()]));
        assert!(!started_hidden(&["--hidden-ish".into()]));
    }
}
