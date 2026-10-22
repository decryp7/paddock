//! Where a runner is started (ADR 0003): in the logged-on user's session, or as the service account.
//!
//! On Windows the service runs as LocalSystem in session 0. An `interactive` runner is started in the session of the
//! user logged on at the console, with that user's token; when nobody is logged on it either waits for a log-on
//! (default) or, if the runner allows it, runs as the service account until someone logs on and it is idle again.
//! A `service` runner always runs as the service account. On macOS the service is the user's own launch agent, so
//! every runner is in the user's session.

use crate::config::SessionMode;
use serde::{Deserialize, Serialize};

/// What an `interactive` runner does while nobody is logged on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoggedOffMode {
    /// Wait for a user to log on (jobs that need a desktop never run without one).
    #[default]
    Wait,
    /// Run as the service account meanwhile (jobs run as LocalSystem without a desktop); moved into the user's
    /// session once someone logs on and the runner is idle.
    ServiceAccount,
}

/// Where to start a runner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LaunchTarget {
    /// In this Windows session, with its user's token.
    UserSession {
        /// The session's id.
        session_id: u32,
    },
    /// As the service's own account, in the service's session.
    ServiceAccount,
}

/// Why a runner is not started now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoLaunch {
    /// It needs a logged-on user and nobody is.
    WaitingForLogon,
}

/// Where a runner runs, as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionState {
    /// Not running.
    NotRunning,
    /// In the logged-on user's session.
    UserSession,
    /// As the service account (in `fallback` because nobody is logged on).
    ServiceAccount {
        /// It normally runs in the user's session.
        fallback: bool,
    },
    /// Waiting for a user to log on.
    WaitingForLogon,
    /// The service could not start it where it should run.
    Unavailable {
        /// Why, for the user.
        reason: String,
    },
}

/// A Windows session with a logged-on user (session 0, the services' session, never counts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserSession {
    /// The session's id.
    pub id: u32,
    /// Connected (at the console or over Remote Desktop); `false`: disconnected, the user is still logged on.
    pub active: bool,
}

/// The session a logged-on user's runner runs in. The physical console is only one of them: a machine used over
/// Remote Desktop has its user in another session, and the console shows the sign-in screen. Connected sessions come
/// before disconnected ones (a disconnected Remote Desktop session keeps running its programs), the console before
/// other sessions, then the lowest id, so the choice is stable.
pub fn pick_user_session(console: Option<u32>, sessions: &[UserSession]) -> Option<u32> {
    sessions
        .iter()
        .filter(|s| s.id != 0)
        .min_by_key(|s| (!s.active, Some(s.id) != console, s.id))
        .map(|s| s.id)
}

/// Where to start a runner with `mode` and `logged_off`, given the console session (`None`: nobody logged on).
pub fn launch_target(mode: SessionMode, logged_off: LoggedOffMode, console_session: Option<u32>) -> Result<LaunchTarget, NoLaunch> {
    match (mode, console_session, logged_off) {
        (SessionMode::Service, _, _) => Ok(LaunchTarget::ServiceAccount),
        (SessionMode::Interactive, Some(session_id), _) => Ok(LaunchTarget::UserSession { session_id }),
        (SessionMode::Interactive, None, LoggedOffMode::ServiceAccount) => Ok(LaunchTarget::ServiceAccount),
        (SessionMode::Interactive, None, LoggedOffMode::Wait) => Err(NoLaunch::WaitingForLogon),
    }
}

/// Whether a running runner should be moved (stopped when idle and started again) because a session appeared or
/// went away since it was started.
pub fn should_relocate(mode: SessionMode, logged_off: LoggedOffMode, running_as: LaunchTarget, console_session: Option<u32>) -> bool {
    match launch_target(mode, logged_off, console_session) {
        Ok(target) => target != running_as,
        // Nobody is logged on any more: Windows ends the user's processes itself at log-off.
        Err(NoLaunch::WaitingForLogon) => false,
    }
}

/// How a runner started at `target` shows in the window.
pub fn state_for(mode: SessionMode, target: LaunchTarget) -> SessionState {
    match target {
        LaunchTarget::UserSession { .. } => SessionState::UserSession,
        LaunchTarget::ServiceAccount => SessionState::ServiceAccount {
            fallback: mode == SessionMode::Interactive,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_desktop_user_is_found_when_the_console_shows_the_sign_in_screen() {
        let rdp = UserSession { id: 2, active: true };
        // The console (session 1) has no user, so it is not in the list.
        assert_eq!(pick_user_session(Some(1), &[rdp]), Some(2));
        assert_eq!(pick_user_session(None, &[rdp]), Some(2));
        assert_eq!(pick_user_session(Some(1), &[]), None);
    }

    #[test]
    fn connected_before_disconnected_then_the_console_then_the_lowest_id() {
        let s = |id, active| UserSession { id, active };
        assert_eq!(pick_user_session(Some(5), &[s(3, false), s(4, true)]), Some(4));
        assert_eq!(pick_user_session(Some(5), &[s(4, true), s(5, true)]), Some(5));
        assert_eq!(pick_user_session(Some(9), &[s(7, true), s(6, true)]), Some(6));
        assert_eq!(pick_user_session(Some(9), &[s(7, false), s(9, false)]), Some(9));
        assert_eq!(
            pick_user_session(None, &[s(8, false)]),
            Some(8),
            "a disconnected session still runs programs"
        );
        assert_eq!(
            pick_user_session(Some(0), &[s(0, true)]),
            None,
            "session 0 is the services' session"
        );
    }

    const I: SessionMode = SessionMode::Interactive;
    const S: SessionMode = SessionMode::Service;

    #[test]
    fn interactive_runners_go_to_the_console_user() {
        assert_eq!(
            launch_target(I, LoggedOffMode::Wait, Some(3)),
            Ok(LaunchTarget::UserSession { session_id: 3 })
        );
        assert_eq!(launch_target(I, LoggedOffMode::Wait, None), Err(NoLaunch::WaitingForLogon));
        assert_eq!(
            launch_target(I, LoggedOffMode::ServiceAccount, None),
            Ok(LaunchTarget::ServiceAccount)
        );
        assert_eq!(launch_target(S, LoggedOffMode::Wait, Some(3)), Ok(LaunchTarget::ServiceAccount));
    }

    #[test]
    fn a_fallback_runner_moves_into_the_session_once_someone_logs_on() {
        let fallback = LaunchTarget::ServiceAccount;
        assert!(should_relocate(I, LoggedOffMode::ServiceAccount, fallback, Some(1)));
        assert!(!should_relocate(I, LoggedOffMode::ServiceAccount, fallback, None));
        assert!(!should_relocate(S, LoggedOffMode::Wait, fallback, Some(1)));
        let user = LaunchTarget::UserSession { session_id: 1 };
        assert!(!should_relocate(I, LoggedOffMode::Wait, user, Some(1)));
        assert!(should_relocate(I, LoggedOffMode::Wait, user, Some(2)), "another user logged on");
        assert!(!should_relocate(I, LoggedOffMode::Wait, user, None));
    }

    #[test]
    fn states_say_whether_the_service_account_is_a_fallback() {
        assert_eq!(
            state_for(I, LaunchTarget::ServiceAccount),
            SessionState::ServiceAccount { fallback: true }
        );
        assert_eq!(
            state_for(S, LaunchTarget::ServiceAccount),
            SessionState::ServiceAccount { fallback: false }
        );
        assert_eq!(state_for(I, LaunchTarget::UserSession { session_id: 1 }), SessionState::UserSession);
    }
}
