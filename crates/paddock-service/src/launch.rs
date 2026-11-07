//! How this process was started and where the app may register itself to start again: the pure parts of "start at login"
//! that every app with a login item shares (first written in Dyno's start-at-login code, now removed from it). The macOS Apple Event itself is read by
//! [`crate::apple_event`]; nothing here touches the OS, so every OS runs these tests.

use std::path::Path;

/// What the open-application Apple Event said about this start (read in the same moment the event is handled; plain values
/// so the decision is pure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OpenEvent {
    /// The event carries `keyAELaunchedAsLogInItem` (`'lgit'`): a Login Items start.
    pub login_item: bool,
    /// The event carries `keyAERestoreAppState` (`'rsto'`) with the value yes: a "Reopen windows" relaunch.
    pub restore: bool,
    /// The event's sender is `loginwindow`, by the executable path of the sender pid read in the same moment.
    pub sender_is_loginwindow: bool,
}

impl OpenEvent {
    /// The event in words for the start log: which key the OS sent, who sent it, and whether `--hidden` was given. Fixed words only.
    pub fn describe(&self, flag: bool) -> String {
        let event = if self.login_item {
            "login item (lgit)"
        } else if self.restore {
            "restore (rsto)"
        } else {
            "none"
        };
        let sender = if self.login_item || self.restore {
            if self.sender_is_loginwindow {
                "loginwindow"
            } else {
                "another program"
            }
        } else {
            "unknown"
        };
        format!(
            "open event: {event}; sender {sender}; flag {}",
            if flag { "yes" } else { "no" }
        )
    }
}

/// Why a start counts as a start at login (the app opens in the tray instead of showing its window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchReason {
    /// The `--hidden` flag.
    Flag,
    /// A Login Items start (`lgit`).
    LoginItem,
    /// loginwindow reopened the app after a restart (`rsto=yes` from loginwindow), whatever Start at login says.
    RestoredAtLogin,
}

impl LaunchReason {
    /// The words for the log line.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::LoginItem => "login item",
            Self::RestoredAtLogin => "restored at login",
        }
    }
}

/// The executable of macOS's login window, the only sender whose restore request counts as a login start.
pub const LOGINWINDOW_PATH: &str =
    "/System/Library/CoreServices/loginwindow.app/Contents/MacOS/loginwindow";

/// Whether `path` (a sender pid's executable) is the login window.
pub fn is_loginwindow(path: &str) -> bool {
    path == LOGINWINDOW_PATH
}

/// Why a start is a start at login, if it is: the `--hidden` flag, a Login Items start, or a restore request from
/// loginwindow (a system reopen starts the app in the tray whatever Start at login says). A restore from any other sender
/// (a user's own "Resume") shows the window.
pub fn launch_reason(flag: bool, event: OpenEvent) -> Option<LaunchReason> {
    if flag {
        Some(LaunchReason::Flag)
    } else if event.login_item {
        Some(LaunchReason::LoginItem)
    } else if event.restore && event.sender_is_loginwindow {
        Some(LaunchReason::RestoredAtLogin)
    } else {
        None
    }
}

/// Refuses a registration for an executable that will not exist after the next login: Gatekeeper path randomisation
/// (`/AppTranslocation/`) or a mounted disk image or other volume (`/Volumes/`). The message names `app` and says what to do.
///
/// # Errors
/// The message to show the user.
pub fn refuse_unstable_location(exe: &Path, app: &str) -> Result<(), String> {
    let path = exe.to_string_lossy();
    if path.contains("/AppTranslocation/") || path.starts_with("/Volumes/") {
        return Err(format!(
            "{app} is running from a temporary location (a disk image, another disk or a quarantined download). Move {app} to the \
             Applications folder on this Mac's startup disk (not another disk or a disk image), open it from there, and turn on Start at login again."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(login_item: bool, restore: bool, sender_is_loginwindow: bool) -> OpenEvent {
        OpenEvent {
            login_item,
            restore,
            sender_is_loginwindow,
        }
    }

    #[test]
    fn translocated_and_mounted_paths_are_refused_with_a_next_step() {
        for bad in [
            "/private/var/folders/ab/T/AppTranslocation/1F2E/d/Dyno.app/Contents/MacOS/dyno-app",
            "/Volumes/Dyno/Dyno.app/Contents/MacOS/dyno-app",
        ] {
            let m = refuse_unstable_location(Path::new(bad), "Dyno").unwrap_err();
            assert!(
                m.contains("Dyno is running from")
                    && m.contains("Applications")
                    && m.contains("another disk"),
                "{m}"
            );
        }
        for good in [
            "/Applications/Dyno.app/Contents/MacOS/dyno-app",
            "/Users/a/Applications/Dyno.app/Contents/MacOS/dyno-app",
            "/home/a/dyno",
        ] {
            assert!(
                refuse_unstable_location(Path::new(good), "Dyno").is_ok(),
                "{good}"
            );
        }
    }

    #[test]
    fn a_volume_name_inside_a_path_is_not_a_mounted_volume() {
        assert!(
            refuse_unstable_location(
                Path::new("/Users/a/Volumes/Dyno.app/Contents/MacOS/dyno-app"),
                "Dyno"
            )
            .is_ok()
        );
    }

    #[test]
    fn what_makes_a_start_a_start_at_login() {
        let cases = [
            (true, event(false, false, false), Some(LaunchReason::Flag)),
            (
                false,
                event(true, false, true),
                Some(LaunchReason::LoginItem),
            ),
            (
                false,
                event(true, false, false),
                Some(LaunchReason::LoginItem),
            ),
            (
                false,
                event(false, true, true),
                Some(LaunchReason::RestoredAtLogin),
            ),
            (false, event(false, true, false), None),
            (false, event(false, false, true), None),
            (false, event(false, false, false), None),
            (true, event(true, true, true), Some(LaunchReason::Flag)),
            (
                false,
                event(true, true, true),
                Some(LaunchReason::LoginItem),
            ),
        ];
        for (flag, ev, want) in cases {
            assert_eq!(launch_reason(flag, ev), want, "flag {flag}, {ev:?}");
        }
    }

    #[test]
    fn the_signals_are_named_in_the_log() {
        assert_eq!(LaunchReason::Flag.describe(), "flag");
        assert_eq!(LaunchReason::LoginItem.describe(), "login item");
        assert_eq!(
            LaunchReason::RestoredAtLogin.describe(),
            "restored at login"
        );
    }

    #[test]
    fn only_the_login_window_executable_counts_as_loginwindow() {
        assert!(is_loginwindow(LOGINWINDOW_PATH));
        assert!(!is_loginwindow(
            "/Applications/Evil.app/Contents/MacOS/loginwindow"
        ));
        assert!(!is_loginwindow("/usr/bin/open"));
        assert!(!is_loginwindow(""));
    }

    #[test]
    fn the_open_event_is_told_in_words_for_the_start_log() {
        assert_eq!(
            event(true, false, true).describe(false),
            "open event: login item (lgit); sender loginwindow; flag no"
        );
        assert_eq!(
            event(false, true, true).describe(false),
            "open event: restore (rsto); sender loginwindow; flag no"
        );
        assert_eq!(
            event(false, true, false).describe(false),
            "open event: restore (rsto); sender another program; flag no"
        );
        assert_eq!(
            OpenEvent::default().describe(true),
            "open event: none; sender unknown; flag yes"
        );
    }
}
