//! The installer-running signal (ADR 0053 section 4.1 step 4): the named mutex `Global\<identifier>.setup`, held by
//! `apply-update` and by the installer include (through `hold-installing`) while an install runs, and how the app reads it.
//!
//! - The mutex gets an explicit security descriptor ([`MUTEX_SDDL`]) that grants `SYNCHRONIZE` to Authenticated Users:
//!   without it the standard user's app could not `OpenMutex` a mutex created by an elevated process. Only SYSTEM and
//!   Administrators get full access, so a standard user can neither take nor release it. (A standard user cannot create
//!   `Global\` objects at all, which needs `SeCreateGlobalPrivilege`, so the name cannot be squatted.)
//! - `WAIT_ABANDONED` on a wait means the holder ended without releasing: the installer has gone.
//! - **The gap.** Between `apply-update` exiting (it releases the mutex) and the installer taking it in `PREINSTALL`, no
//!   process holds the mutex. The app therefore treats a hand-off record younger than [`HANDOFF_GRACE`] as "still
//!   installing" ([`still_installing`]); after that, a missing mutex means the installer is not running.

use std::time::Duration;

/// Security descriptor of the mutex: SYSTEM and Administrators full access, Authenticated Users `SYNCHRONIZE` only.
pub const MUTEX_SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00100000;;;AU)";

/// How long after a hand-off record is written the app still assumes the installer is about to take the mutex.
pub const HANDOFF_GRACE: Duration = Duration::from_secs(60);

/// The mutex name for an app identifier.
pub fn mutex_name(identifier: &str) -> String {
    format!(r"Global\{identifier}.setup")
}

/// What a wait on the mutex returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// `WAIT_OBJECT_0`: the holder released it.
    Released,
    /// `WAIT_ABANDONED`: the holder ended without releasing.
    Abandoned,
    /// `WAIT_TIMEOUT`.
    TimedOut,
    /// The wait failed.
    Failed,
}

/// Whether an installer of the app is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallerState {
    /// Yes (or about to).
    Installing,
    /// No.
    Gone,
}

/// What `OpenMutex` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutexProbe {
    /// The mutex exists (an installer holds it).
    Exists,
    /// It does not exist.
    Absent,
}

/// The state after a wait on the mutex finished.
pub fn after_wait(outcome: WaitOutcome) -> InstallerState {
    match outcome {
        WaitOutcome::Released | WaitOutcome::Abandoned => InstallerState::Gone,
        WaitOutcome::TimedOut | WaitOutcome::Failed => InstallerState::Installing,
    }
}

/// The state at the app's start: the mutex, and the age of the hand-off record (`None` when there is none).
pub fn still_installing(probe: MutexProbe, record_age: Option<Duration>) -> InstallerState {
    match (probe, record_age) {
        (MutexProbe::Exists, _) => InstallerState::Installing,
        (MutexProbe::Absent, Some(age)) if age < HANDOFF_GRACE => InstallerState::Installing,
        (MutexProbe::Absent, _) => InstallerState::Gone,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_global_and_stable() {
        assert_eq!(
            mutex_name("com.decryp7.dyno"),
            r"Global\com.decryp7.dyno.setup"
        );
    }

    #[test]
    fn sddl_gives_authenticated_users_synchronize_and_nothing_more() {
        assert_eq!(MUTEX_SDDL, "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00100000;;;AU)");
        let au = MUTEX_SDDL.split('(').find(|a| a.ends_with(";AU)")).unwrap();
        assert!(au.contains("0x00100000"), "SYNCHRONIZE only");
        assert!(
            !MUTEX_SDDL.contains(";WD)") && !MUTEX_SDDL.contains(";BU)"),
            "no Everyone, no Users"
        );
    }

    #[test]
    fn abandoned_means_the_installer_is_gone() {
        assert_eq!(after_wait(WaitOutcome::Abandoned), InstallerState::Gone);
        assert_eq!(after_wait(WaitOutcome::Released), InstallerState::Gone);
        assert_eq!(
            after_wait(WaitOutcome::TimedOut),
            InstallerState::Installing
        );
        assert_eq!(after_wait(WaitOutcome::Failed), InstallerState::Installing);
    }

    #[test]
    fn a_held_mutex_is_installing() {
        assert_eq!(
            still_installing(MutexProbe::Exists, None),
            InstallerState::Installing
        );
        assert_eq!(
            still_installing(MutexProbe::Exists, Some(Duration::from_secs(3600))),
            InstallerState::Installing
        );
    }

    #[test]
    fn the_gap_is_covered_by_a_young_record() {
        assert_eq!(
            still_installing(MutexProbe::Absent, Some(Duration::from_secs(5))),
            InstallerState::Installing
        );
        assert_eq!(
            still_installing(
                MutexProbe::Absent,
                Some(HANDOFF_GRACE - Duration::from_millis(1))
            ),
            InstallerState::Installing
        );
        assert_eq!(
            still_installing(MutexProbe::Absent, Some(HANDOFF_GRACE)),
            InstallerState::Gone
        );
        assert_eq!(
            still_installing(MutexProbe::Absent, None),
            InstallerState::Gone
        );
    }
}
