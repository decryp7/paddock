//! Registering the Pitwall server with the OS from the app. Elevation and approval are always the OS's own: UAC on
//! Windows (the installer does this once; the app can repeat it), the Login Items approval on macOS (`SMAppService`
//! with the launch agent inside the app bundle); on Linux nothing is elevated (a systemd user unit). All three go through
//! the shared manager. Pitwall never asks for or handles a password.

use crate::commands::SharedClient;
use paddock_service::{RepairOutcome, ServiceStatus, Started};
use pitwall_core::mgmt::{CommandError, ErrorCode};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// What the UI shows about the repair (the shared tracker's notice without its step).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairNotice {
    /// Where the repair is.
    pub state: RepairState,
    /// The words to show.
    pub message: String,
}

/// Where the repair is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RepairState {
    /// Working.
    Working,
    /// Done, the service runs.
    Succeeded,
    /// Did not work; the message has next steps.
    Failed,
}

impl From<paddock_service::RepairNotice> for RepairNotice {
    fn from(n: paddock_service::RepairNotice) -> Self {
        RepairNotice {
            state: match n.state {
                paddock_service::RepairState::Working => RepairState::Working,
                paddock_service::RepairState::Succeeded => RepairState::Succeeded,
                paddock_service::RepairState::Failed => RepairState::Failed,
            },
            message: n.message,
        }
    }
}

/// The service's state as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInstall {
    /// `windows`, `macos` or `linux`.
    pub platform: &'static str,
    /// Registered with the OS.
    pub installed: bool,
    /// The server answers.
    pub running: bool,
    /// The OS integration can be switched on from the app.
    pub can_install: bool,
    /// The user has not been asked yet: show the "run Pitwall's service?" question.
    pub ask_consent: bool,
    /// What to do next, when something is off.
    pub message: Option<String>,
    /// The background repair of a registration macOS refused, while it works and after (macOS only).
    pub repair: Option<RepairNotice>,
    /// The user switched the background item off in System Settings > General > Login Items (macOS only).
    pub disabled_in_login_items: bool,
    /// The service is registered or known to launchd but cannot start, and the server does not answer (macOS): offer Repair.
    pub start_failed: bool,
    /// launchd still has the job but System Settings > Login Items does not list Pitwall (macOS): offer Repair.
    pub not_in_login_items: bool,
}

/// The server program's file name.
pub const SERVER_EXE: &str = if cfg!(windows) { "pitwall-server.exe" } else { "pitwall-server" };

/// The server program in `dir` (Tauri puts sidecars next to the app's executable), if it is really there.
pub fn server_exe_in(dir: &Path) -> Result<PathBuf, CommandError> {
    let p = dir.join(SERVER_EXE);
    if p.metadata().is_ok_and(|m| m.len() > 0) {
        Ok(p)
    } else {
        Err(CommandError::new(
            ErrorCode::NotAvailable,
            format!(
                "{} is missing; reinstall Pitwall (development builds: build pitwall-server and copy it there)",
                p.display()
            ),
        ))
    }
}

/// The server program next to this app.
pub fn server_exe() -> Result<PathBuf, CommandError> {
    let exe =
        std::env::current_exe().map_err(|e| CommandError::new(ErrorCode::Internal, format!("the app's folder cannot be found: {e}")))?;
    let dir = exe
        .parent()
        .ok_or_else(|| CommandError::new(ErrorCode::Internal, "the app's folder cannot be found"))?;
    server_exe_in(dir)
}

/// What the OS says about the registration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OsState {
    /// Registered.
    pub installed: bool,
    /// What to do next.
    pub message: Option<String>,
    /// The macOS repair's progress, if one ran.
    pub repair: Option<RepairNotice>,
    /// The user switched the item off in Login Items (macOS).
    pub disabled_in_login_items: bool,
    /// The service is registered or known to the OS but cannot start, and the server does not answer (Repair applies).
    pub job_stale: bool,
    /// The OS has the job but the Login Items list does not show Pitwall (macOS). The shared manager no longer tells
    /// this apart from a service that cannot start, so it is `false` there and `start_failed` covers both.
    pub job_unlisted: bool,
}

/// Puts the OS state, the server's answer and the first-run decision together.
pub fn compose(os: OsState, running: bool, ask_consent: bool) -> ServiceInstall {
    let message = os.message.or_else(|| match (os.installed, running) {
        (true, false) => {
            Some("The service is installed but the server does not answer yet; give it a moment, or restart the service.".into())
        }
        (false, true) => Some("The server is running outside the service (started by hand); it stops when you log out.".into()),
        _ => None,
    });
    let repairing = os.repair.as_ref().is_some_and(|r| r.state == RepairState::Working);
    let broken = !running && !os.disabled_in_login_items && !repairing;
    let start_failed = broken && os.job_stale;
    let not_in_login_items = broken && os.job_unlisted;
    ServiceInstall {
        platform: PLATFORM,
        installed: os.installed,
        running,
        can_install: true,
        ask_consent: ask_consent && !os.installed && !running,
        message,
        repair: os.repair,
        disabled_in_login_items: os.disabled_in_login_items,
        start_failed,
        not_in_login_items,
    }
}

/// The platform name the UI sees.
pub const PLATFORM: &str = if cfg!(windows) {
    "windows"
} else if cfg!(target_os = "macos") {
    "macos"
} else {
    "linux"
};

/// The words `pitwall-server check-install` refused with (its standard error), as the UI shows them: trimmed, at most 600
/// characters, and a plain sentence when it said nothing.
pub fn refusal_words(stderr: &str) -> String {
    // The refusal is the last line; anything before it is not for the user (the OS-log fallback's lines).
    let t = stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("");
    if t.is_empty() {
        return "Pitwall's service cannot be registered for this copy of Pitwall. Uninstall Pitwall, then install it again into Program Files.".into();
    }
    let mut out: String = t.chars().take(600).collect();
    if t.chars().count() > 600 {
        out.push('…');
    }
    out
}

/// The OS state from the shared manager's status.
pub fn os_state_of(s: &ServiceStatus) -> OsState {
    OsState {
        installed: s.installed,
        message: s.message.clone(),
        repair: s.repair.clone().map(RepairNotice::from),
        disabled_in_login_items: s.condition == paddock_service::Condition::SwitchedOff,
        job_stale: s.can_repair && !s.running,
        job_unlisted: false,
    }
}

mod imp {
    //! Every OS goes through the shared manager ([`crate::service_manager`], ADR 0056): the launch agent inside the app bundle
    //! (`SMAppService`) on macOS, a systemd user unit on Linux and the Windows service, changed through Windows' own prompt
    //! (the elevated child is `pitwall-server install` or `uninstall`, UAC, never a password), on Windows. Status, install,
    //! repair, removal and the check at every start are the manager's on all three.
    use super::*;
    use crate::service_manager::{command_error, manager};
    use paddock_service::Timings;

    /// What the OS says about the registration, with the repair's progress and (macOS) the Login Items switch.
    pub fn os_state(client: &SharedClient) -> OsState {
        match manager(client, Timings::default()) {
            Ok(mut m) => os_state_of(&m.status()),
            Err(e) => OsState {
                message: Some(e.message),
                ..OsState::default()
            },
        }
    }

    /// Opens System Settings > General > Login Items. The user switches the item; Pitwall never does it for them.
    #[cfg(target_os = "macos")]
    pub fn open_login_items(client: &SharedClient) -> Result<(), CommandError> {
        manager(client, Timings::default())?.open_login_items();
        Ok(())
    }

    /// Login Items exist on macOS only.
    #[cfg(not(target_os = "macos"))]
    pub fn open_login_items(_client: &SharedClient) -> Result<(), CommandError> {
        Err(CommandError::new(ErrorCode::NotAvailable, "Login Items exist on macOS only"))
    }

    /// On Windows the elevated `pitwall-server install` refuses a program outside Program Files, a folder or program other
    /// accounts can change, and another copy's registration (plan B6), but the elevated run cannot hand its words back. The
    /// same checks run first without elevation (`pitwall-server check-install`), so a refusal is shown in its own words before
    /// Windows asks for permission. Elsewhere there is nothing to check.
    #[cfg(windows)]
    fn check_install_first() -> Result<(), CommandError> {
        use std::time::Duration;
        let mut check = paddock_proc::command(&server_exe()?);
        check.arg("check-install");
        let out = paddock_proc::run_bounded(check, paddock_proc::Limits::new(Duration::from_secs(30)).max_output(8 * 1024))
            .map_err(|e| CommandError::new(ErrorCode::Internal, format!("the server program could not be run: {e}")))?;
        if out.success() {
            Ok(())
        } else {
            Err(CommandError::new(ErrorCode::Forbidden, refusal_words(&out.stderr_text())))
        }
    }

    #[cfg(not(windows))]
    fn check_install_first() -> Result<(), CommandError> {
        Ok(())
    }

    /// Registers the service (replacing what is there) and waits for it to answer; the OS asks the user where it must.
    pub fn install(client: &SharedClient) -> Result<(), CommandError> {
        check_install_first()?;
        let cancel = AtomicBool::new(false);
        manager(client, Timings::default())?
            .install(&cancel)
            .map(drop)
            .map_err(command_error)
    }

    /// Stops and removes the service.
    pub fn uninstall(client: &SharedClient) -> Result<(), CommandError> {
        manager(client, Timings::default())?.uninstall().map_err(command_error)
    }

    /// The Repair the user pressed, as a job body: runs on the calling (blocking) thread; `cancel` is asked between steps
    /// and while waiting. Refused when the user switched the item off in Login Items, and while another repair works.
    pub fn repair_blocking(client: &SharedClient, cancel: &AtomicBool) -> Result<RepairOutcome, CommandError> {
        // On Windows a repair is the elevated `install` again: the same check first, in its own words.
        check_install_first()?;
        manager(client, Timings::default())?.repair(cancel).map_err(command_error)
    }

    /// The check at every start (rule 24): replaces a stale registration while the service is down and once after every
    /// version change; never installs unasked, never touches what the user switched off, and never opens a prompt (a
    /// mechanism that is not automatic, the Windows service, only offers).
    pub fn on_start(client: &SharedClient, cancel: &AtomicBool) -> Result<Option<Started>, CommandError> {
        Ok(Some(manager(client, Timings::default())?.on_start(cancel)))
    }
}

pub use imp::{install, on_start, open_login_items, os_state, repair_blocking, uninstall};

pub use crate::service_manager::repair_step;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_is_shown_in_its_own_words_bounded_with_a_plain_fallback() {
        let words = "The Pitwall service runs as the system, so Pitwall must be installed in C:\\Program Files; it is in D:\\x.";
        assert_eq!(refusal_words(&format!("  {words}\r\n")), words);
        assert!(refusal_words("").contains("Uninstall Pitwall, then install it again"));
        // B6 re-review P2-3: lines the OS-log fallback wrote before the refusal are not shown, only the refusal.
        let with_fallback = format!(
            "the OS log could not be opened (RegisterEventSourceW failed); lines go to standard error\ninstall refused: x\n{words}\n"
        );
        assert_eq!(refusal_words(&with_fallback), words);
        let long = refusal_words(&"y".repeat(700));
        assert_eq!(long.chars().count(), 601);
        assert!(long.ends_with('…'));
    }
    use paddock_service::{Condition, Mechanism};

    fn status(condition: Condition, running: bool, can_repair: bool) -> ServiceStatus {
        ServiceStatus {
            mechanism: Mechanism::LaunchdBundled,
            condition,
            installed: true,
            running,
            message: Some("words".into()),
            can_repair,
            can_install: false,
            can_open_login_items: false,
            repair: None,
        }
    }

    /// The shared manager's status reaches the UI's state: a refused or silent service offers Repair, the user's switch
    /// is kept apart, and a running service offers nothing.
    #[test]
    fn the_shared_status_maps_onto_what_the_ui_shows() {
        let refused = os_state_of(&status(Condition::Refused, false, true));
        assert!(refused.installed && refused.job_stale && !refused.disabled_in_login_items);
        assert_eq!(refused.message.as_deref(), Some("words"));
        let off = os_state_of(&status(Condition::SwitchedOff, false, false));
        assert!(off.disabled_in_login_items && !off.job_stale);
        let up = os_state_of(&status(Condition::Running, true, false));
        assert!(!up.job_stale && up.repair.is_none());
        let mut working = status(Condition::Repairing, false, false);
        working.repair = Some(paddock_service::RepairNotice {
            state: paddock_service::RepairState::Working,
            message: "m".into(),
            step: Some("Removing the old registration".into()),
        });
        let notice = os_state_of(&working).repair.unwrap();
        assert_eq!((notice.state, notice.message.as_str()), (RepairState::Working, "m"));
    }

    #[test]
    fn the_repair_and_the_login_item_switch_reach_the_ui() {
        let os = OsState {
            installed: true,
            repair: Some(RepairNotice {
                state: RepairState::Working,
                message: "m".into(),
            }),
            disabled_in_login_items: true,
            ..OsState::default()
        };
        let s = compose(os, false, false);
        assert!(s.disabled_in_login_items && s.repair.is_some());
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["disabledInLoginItems"], true);
        assert_eq!(json["repair"]["state"], "working");
    }

    #[test]
    fn repair_is_offered_when_the_job_cannot_start_or_is_missing_from_login_items() {
        let stale = OsState {
            job_stale: true,
            job_unlisted: true,
            ..OsState::default()
        };
        let s = compose(stale.clone(), false, false);
        assert!(s.start_failed && s.not_in_login_items);
        // Running, repairing or switched off by the user: no Repair offer.
        assert!(!compose(stale.clone(), true, false).start_failed);
        let mut working = stale.clone();
        working.repair = Some(RepairNotice {
            state: RepairState::Working,
            message: "m".into(),
        });
        assert!(!compose(working, false, false).start_failed);
        let mut off = stale;
        off.disabled_in_login_items = true;
        let s = compose(off, false, false);
        assert!(!s.start_failed && !s.not_in_login_items);
        let json = serde_json::to_value(compose(OsState::default(), false, false)).unwrap();
        assert_eq!(
            (json["startFailed"].clone(), json["notInLoginItems"].clone()),
            (false.into(), false.into())
        );
    }

    #[test]
    fn the_server_program_must_exist_and_not_be_an_empty_placeholder() {
        let d = tempfile::tempdir().unwrap();
        let e = server_exe_in(d.path()).unwrap_err();
        assert_eq!(e.code, ErrorCode::NotAvailable);
        assert!(e.message.contains("reinstall Pitwall"));
        std::fs::write(d.path().join(SERVER_EXE), b"").unwrap();
        assert!(server_exe_in(d.path()).is_err(), "the build placeholder is empty");
        std::fs::write(d.path().join(SERVER_EXE), b"binary").unwrap();
        assert_eq!(server_exe_in(d.path()).unwrap(), d.path().join(SERVER_EXE));
    }

    #[test]
    fn the_state_tells_the_user_what_is_off() {
        let none = OsState::default();
        let yes = OsState {
            installed: true,
            ..OsState::default()
        };
        // Fresh machine: ask.
        let s = compose(none.clone(), false, true);
        assert!(s.ask_consent && !s.installed && !s.running && s.message.is_none());
        // The question is asked once.
        assert!(!compose(none.clone(), false, false).ask_consent);
        // Never asked when something already runs the server.
        assert!(!compose(none.clone(), true, true).ask_consent);
        assert!(!compose(yes.clone(), false, true).ask_consent);
        // Hints.
        assert!(compose(yes.clone(), false, false).message.unwrap().contains("restart the service"));
        assert!(compose(none, true, false).message.unwrap().contains("started by hand"));
        assert!(compose(yes.clone(), true, false).message.is_none());
        // The OS's own message wins.
        let os = OsState {
            installed: true,
            message: Some("allow it".into()),
            ..OsState::default()
        };
        assert_eq!(compose(os, false, false).message.as_deref(), Some("allow it"));
        assert!(["windows", "macos", "linux"].contains(&PLATFORM));
    }
}
