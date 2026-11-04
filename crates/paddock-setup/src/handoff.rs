//! The installer-running signals (ADR 0053 section 4.1 step 4). Nothing an unprivileged account can open can be *held* by it.
//!
//! - **Exclusion between administrators' processes** is a mutex and a hand-over event inside a **private namespace** whose
//!   boundary descriptor names Administrators ([`namespace_alias`], [`LOCK_SDDL`]). Only an elevated process can create or open
//!   that namespace, so the names cannot be squatted by a standard user, and the objects have no access for anyone else.
//! - **The signal for the app** is a manual-reset event in the `Global\` namespace ([`signal_name`]), created non-signalled by
//!   `apply-update` (or by `hold-installing` when an install is started by hand) and set when the install is over
//!   ([`SIGNAL_SDDL`]: Authenticated Users get `SYNCHRONIZE | READ_CONTROL` only). An event has no owner, so waiting on it
//!   acquires nothing: no standard user can block an update or the app. The name *can* be squatted in `Global\`; the app and
//!   the installer judge the object's owner ([`signal_state`]) and a squatted signal is ignored (logged), never waited on and
//!   never a reason to refuse an update: the private mutex still excludes a second install.
//! - **No release-then-wait gap, decided only inside the private namespace** ([`admit`]). `apply-update` keeps the hand-over
//!   event, which it creates unset, until `hold-installing` has taken the lock and set it, so "the lock is held, or the
//!   hand-over event exists and is unset" is busy even in the instant the lock is free. The `Global` signal never decides
//!   admission: a standard user's handle can keep a dead install's signal alive, and a squatted signal must not open the gap.
//!   A crashed installer closes its handles and the private objects vanish with them.
//! - The app waits on the signal with a **required bound** (`wait_for_installer`); a wait that times out means "unknown", and
//!   the hand-off record settles it at the next start.

use paddock_service::privileged::trust::is_trusted_sid;

/// Descriptor of the private-namespace objects (the mutex and the hand-over event): owner Administrators, SYSTEM and
/// Administrators full access, nobody else any.
pub const LOCK_SDDL: &str = "O:BAD:(A;;GA;;;SY)(A;;GA;;;BA)";

/// Descriptor of the app-facing signal: owner Administrators, SYSTEM and Administrators full access, Authenticated Users
/// `SYNCHRONIZE | READ_CONTROL` only (to wait, and to read who owns it); never `EVENT_MODIFY_STATE`.
pub const SIGNAL_SDDL: &str = "O:BAD:(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00120000;;;AU)";

/// The private namespace's own descriptor (who may open it): SYSTEM and Administrators.
pub const NAMESPACE_SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;BA)";

/// The SID the namespace boundary is made of (`BUILTIN\Administrators`).
pub const BOUNDARY_SID: &str = "S-1-5-32-544";

/// A namespace alias made only of letters, digits, `.` and `_` from the app identifier.
pub fn namespace_alias(identifier: &str) -> String {
    let clean: String = identifier
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("PaddockSetup.{clean}")
}

/// The mutex inside the namespace.
pub fn lock_name(identifier: &str) -> String {
    format!(r"{}\lock", namespace_alias(identifier))
}

/// The hand-over event inside the namespace: `hold-installing` sets it once it holds the lock.
pub fn handoff_name(identifier: &str) -> String {
    format!(r"{}\handoff", namespace_alias(identifier))
}

/// The app-facing signal.
pub fn signal_name(identifier: &str) -> String {
    format!(r"Global\{identifier}.installing")
}

/// What a wait on a handle returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// `WAIT_OBJECT_0`.
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

/// What looking at the signal found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalProbe {
    /// It does not exist.
    Absent,
    /// It exists but no administrator made it (or its owner could not be read): a squat, to be ignored.
    Foreign,
    /// An administrator's installer is running (the event is not signalled).
    Active,
    /// The install is over (the event is signalled).
    Finished,
}

/// The probe of an event that exists: who owns it and whether it is signalled.
pub fn signal_state(owner_sid: Option<&str>, signalled: bool) -> SignalProbe {
    match (owner_sid.is_some_and(is_trusted_sid), signalled) {
        (false, _) => SignalProbe::Foreign,
        (true, false) => SignalProbe::Active,
        (true, true) => SignalProbe::Finished,
    }
}

/// The state after a wait on the signal.
pub fn after_wait(outcome: WaitOutcome) -> InstallerState {
    match outcome {
        WaitOutcome::Released | WaitOutcome::Abandoned => InstallerState::Gone,
        WaitOutcome::TimedOut | WaitOutcome::Failed => InstallerState::Installing,
    }
}

/// The state at the app's start. A squatted or finished signal is not an installer.
pub fn still_installing(probe: SignalProbe) -> InstallerState {
    match probe {
        SignalProbe::Active => InstallerState::Installing,
        SignalProbe::Absent | SignalProbe::Foreign | SignalProbe::Finished => InstallerState::Gone,
    }
}

/// Whether a new `apply-update` may go on, from what the lock try-acquire and the signal showed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    /// Go on.
    Go,
    /// An installer is running (or is about to take the lock).
    Busy,
    /// The lock could not be tested.
    Failed,
}

/// The decision, from objects inside the private namespace only (no standard user can hold or make any of them): busy when
/// the lock is held, or when the hand-over event exists and is not set (`apply-update` is waiting for `hold-installing` to
/// take the lock: the gap). The `Global` signal never decides: a user handle can keep a dead install's signal alive, and a
/// squatted one could open the gap.
pub fn admit(lock: WaitOutcome, handoff_pending: bool) -> Admit {
    match (lock, handoff_pending) {
        (WaitOutcome::Failed, _) => Admit::Failed,
        (WaitOutcome::TimedOut, _) | (_, true) => Admit::Busy,
        (WaitOutcome::Released | WaitOutcome::Abandoned, false) => Admit::Go,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_stable_and_the_lock_is_not_in_global() {
        assert_eq!(
            namespace_alias("com.decryp7.dyno"),
            "PaddockSetup.com.decryp7.dyno"
        );
        assert_eq!(
            lock_name("com.decryp7.dyno"),
            r"PaddockSetup.com.decryp7.dyno\lock"
        );
        assert_eq!(
            handoff_name("com.decryp7.dyno"),
            r"PaddockSetup.com.decryp7.dyno\handoff"
        );
        assert_eq!(
            signal_name("com.decryp7.dyno"),
            r"Global\com.decryp7.dyno.installing"
        );
        assert!(!lock_name("x").starts_with("Global"));
    }

    #[test]
    fn an_alias_cannot_contain_a_separator() {
        assert_eq!(namespace_alias(r"a\b/c d"), "PaddockSetup.a_b_c_d");
    }

    #[test]
    fn the_lock_has_no_access_for_anyone_but_system_and_administrators() {
        assert_eq!(LOCK_SDDL, "O:BAD:(A;;GA;;;SY)(A;;GA;;;BA)");
        for sddl in [LOCK_SDDL, NAMESPACE_SDDL] {
            let who: Vec<&str> = sddl
                .split('(')
                .skip(1)
                .map(|a| a.trim_end_matches(')').rsplit(';').next().unwrap())
                .collect();
            assert_eq!(who, ["SY", "BA"], "{sddl}");
        }
    }

    #[test]
    fn the_signal_gives_authenticated_users_synchronize_and_read_only() {
        let au = SIGNAL_SDDL
            .split('(')
            .find(|a| a.ends_with(";AU)"))
            .unwrap();
        assert!(au.contains("0x00120000"));
        // EVENT_MODIFY_STATE (0x0002) is the bit that would let them set or reset it.
        assert_eq!(0x0012_0000u32 & 0x0002, 0);
        assert!(!SIGNAL_SDDL.contains(";WD)") && !SIGNAL_SDDL.contains(";BU)"));
        assert!(SIGNAL_SDDL.starts_with("O:BAD:"));
    }

    #[test]
    fn the_boundary_is_administrators() {
        assert_eq!(BOUNDARY_SID, "S-1-5-32-544");
    }

    #[test]
    fn only_an_administrators_event_counts_and_its_state_decides() {
        assert_eq!(
            signal_state(Some("S-1-5-32-544"), false),
            SignalProbe::Active
        );
        assert_eq!(signal_state(Some("S-1-5-18"), false), SignalProbe::Active);
        assert_eq!(
            signal_state(Some("S-1-5-32-544"), true),
            SignalProbe::Finished
        );
        assert_eq!(
            signal_state(Some("S-1-5-21-1-2-3-1001"), false),
            SignalProbe::Foreign
        );
        assert_eq!(
            signal_state(Some("S-1-5-21-1-2-3-1001"), true),
            SignalProbe::Foreign
        );
        assert_eq!(signal_state(None, false), SignalProbe::Foreign);
    }

    #[test]
    fn waits_map_to_the_state() {
        assert_eq!(after_wait(WaitOutcome::Abandoned), InstallerState::Gone);
        assert_eq!(after_wait(WaitOutcome::Released), InstallerState::Gone);
        assert_eq!(
            after_wait(WaitOutcome::TimedOut),
            InstallerState::Installing
        );
        assert_eq!(after_wait(WaitOutcome::Failed), InstallerState::Installing);
    }

    #[test]
    fn only_an_active_signal_is_an_installer() {
        assert_eq!(
            still_installing(SignalProbe::Active),
            InstallerState::Installing
        );
        for p in [
            SignalProbe::Absent,
            SignalProbe::Foreign,
            SignalProbe::Finished,
        ] {
            assert_eq!(still_installing(p), InstallerState::Gone, "{p:?}");
        }
    }

    #[test]
    fn admission_is_decided_by_the_private_lock_and_hand_over_event_only() {
        use WaitOutcome::*;
        assert_eq!(admit(Released, false), Admit::Go);
        assert_eq!(
            admit(Abandoned, false),
            Admit::Go,
            "a dead holder's lock is free"
        );
        assert_eq!(admit(TimedOut, false), Admit::Busy);
        assert_eq!(
            admit(Released, true),
            Admit::Busy,
            "the gap between apply-update and hold-installing"
        );
        assert_eq!(admit(TimedOut, true), Admit::Busy);
        assert_eq!(admit(Failed, false), Admit::Failed);
        assert_eq!(admit(Failed, true), Admit::Failed);
    }
}
