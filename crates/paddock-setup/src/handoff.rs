//! The installer-running signal (ADR 0053 section 4.1 step 4): the named mutex `Global\<identifier>.setup`, held by
//! `apply-update` and then by the installer include (through `hold-installing`) while an install runs, and how the app reads it.
//!
//! - **The name can be squatted.** Any authenticated account may create a mutex in the `Global\` namespace (only file
//!   mappings and symbolic links need `SeCreateGlobalPrivilege`), so a standard user can make the name first. The mutex is
//!   therefore judged by its **owner**: only an object owned by SYSTEM, Administrators or TrustedInstaller counts as "an
//!   installer runs" ([`classify`]); an object owned by anyone else is [`MutexProbe::Foreign`], logged, and neither waited on
//!   nor trusted. `apply-update` and `hold-installing` refuse with [`crate::apply::Refusal::Squatted`] when the name is taken by one.
//! - The mutex is created with [`MUTEX_SDDL`]: owner Administrators, full access for SYSTEM and Administrators and
//!   `SYNCHRONIZE | READ_CONTROL` for Authenticated Users, so the standard user's app can `OpenMutex` it for a wait and read
//!   its owner.
//! - `WAIT_ABANDONED` on a wait means the holder ended without releasing: the installer has gone.
//! - **No gap and no clock.** `apply-update` does not release the mutex until the installer's own `hold-installing` holds
//!   it (a second event, [`handoff_event_name`], signals that; the wait ends early if the installer process ends). The app
//!   therefore only needs the mutex: [`still_installing`].

use paddock_service::privileged::trust::is_trusted_sid;

/// Security descriptor of the mutex and the hand-over event: owner Administrators, SYSTEM and Administrators full access,
/// Authenticated Users `SYNCHRONIZE | READ_CONTROL` only (to wait, and to read who owns it).
pub const MUTEX_SDDL: &str = "O:BAD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00120000;;;AU)";

/// The mutex name for an app identifier.
pub fn mutex_name(identifier: &str) -> String {
    format!(r"Global\{identifier}.setup")
}

/// The name of the event `hold-installing` sets once it holds the mutex (`apply-update` waits on it).
pub fn handoff_event_name(identifier: &str) -> String {
    format!(r"Global\{identifier}.setup.handoff")
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
    /// Yes.
    Installing,
    /// No.
    Gone,
}

/// What looking for the mutex found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutexProbe {
    /// It does not exist.
    Absent,
    /// It exists and an administrator's process made it.
    Held,
    /// It exists but its owner is not SYSTEM, Administrators or TrustedInstaller: a squat.
    Foreign,
}

/// The probe for an object that exists and has this owner (`None` when the owner could not be read: not trusted).
pub fn classify(owner_sid: Option<&str>) -> MutexProbe {
    if owner_sid.is_some_and(is_trusted_sid) {
        MutexProbe::Held
    } else {
        MutexProbe::Foreign
    }
}

/// The state after a wait on the mutex finished.
pub fn after_wait(outcome: WaitOutcome) -> InstallerState {
    match outcome {
        WaitOutcome::Released | WaitOutcome::Abandoned => InstallerState::Gone,
        WaitOutcome::TimedOut | WaitOutcome::Failed => InstallerState::Installing,
    }
}

/// The state at the app's start. A foreign object is not an installer.
pub fn still_installing(probe: MutexProbe) -> InstallerState {
    match probe {
        MutexProbe::Held => InstallerState::Installing,
        MutexProbe::Absent | MutexProbe::Foreign => InstallerState::Gone,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_global_and_stable() {
        assert_eq!(
            mutex_name("com.decryp7.dyno"),
            r"Global\com.decryp7.dyno.setup"
        );
        assert_eq!(
            handoff_event_name("com.decryp7.dyno"),
            r"Global\com.decryp7.dyno.setup.handoff"
        );
    }

    #[test]
    fn sddl_owns_it_by_administrators_and_gives_authenticated_users_synchronize_only() {
        assert_eq!(
            MUTEX_SDDL,
            "O:BAD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00120000;;;AU)"
        );
        let au = MUTEX_SDDL.split('(').find(|a| a.ends_with(";AU)")).unwrap();
        assert!(
            au.contains("0x00120000"),
            "SYNCHRONIZE and READ_CONTROL only"
        );
        assert!(
            !MUTEX_SDDL.contains(";WD)") && !MUTEX_SDDL.contains(";BU)"),
            "no Everyone, no Users"
        );
    }

    #[test]
    fn only_an_administrators_object_counts_as_an_installer() {
        assert_eq!(classify(Some("S-1-5-32-544")), MutexProbe::Held);
        assert_eq!(classify(Some("S-1-5-18")), MutexProbe::Held);
        assert_eq!(
            classify(Some(
                "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"
            )),
            MutexProbe::Held
        );
        assert_eq!(
            classify(Some("S-1-5-21-1-2-3-1001")),
            MutexProbe::Foreign,
            "a standard user made it"
        );
        assert_eq!(classify(Some("S-1-5-32-545")), MutexProbe::Foreign);
        assert_eq!(classify(Some("S-1-1-0")), MutexProbe::Foreign);
        assert_eq!(classify(None), MutexProbe::Foreign);
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
    fn a_held_mutex_is_installing_and_a_foreign_one_is_not() {
        assert_eq!(
            still_installing(MutexProbe::Held),
            InstallerState::Installing
        );
        assert_eq!(still_installing(MutexProbe::Absent), InstallerState::Gone);
        assert_eq!(
            still_installing(MutexProbe::Foreign),
            InstallerState::Gone,
            "a squatted name must not make the app wait forever"
        );
    }
}
