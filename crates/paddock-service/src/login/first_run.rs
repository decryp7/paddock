//! The first run of an app whose start at login is on by default: the first sight of the settings registers once, so a
//! fresh install starts at login without Settings being opened, and a switch the user turned off later is never turned on
//! again. Moved from Dyno's `login.rs`.

use super::{Autostart, FirstRunMarker, LoginService, LoginWords, apply_launch_at_login_with};
use crate::port::System;

/// What the first run did about start at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstRun {
    /// The default was applied now (the switch was on and the marker was missing).
    Applied,
    /// Nothing to do: the marker is there, or the switch is off.
    Skipped,
}

/// The first sight of the settings with the switch on (`wanted`) and no per-user `marker` registers both entries
/// ([`apply_launch_at_login_with`]) and writes the marker, so a user who later turns the switch off is never overridden; a
/// switch that is off (a file that says `false`) is respected and only leaves the marker. A failed registration leaves no
/// marker (it is tried again at the next start) and returns the message.
///
/// # Errors
/// The message to show the user: what did not register, or that the marker could not be written.
pub fn apply_first_run_with(
    marker: &FirstRunMarker,
    sys: &mut dyn System,
    wanted: bool,
    location: Result<(), String>,
    service: Option<&mut dyn LoginService>,
    autostart: &mut dyn Autostart,
    words: &LoginWords,
) -> Result<FirstRun, String> {
    if marker.is_set(sys) {
        return Ok(FirstRun::Skipped);
    }
    if !wanted {
        marker.record(sys, "off")?;
        return Ok(FirstRun::Skipped);
    }
    apply_launch_at_login_with(true, location, service, autostart, words)?;
    marker.record(sys, "applied")?;
    Ok(FirstRun::Applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::login::{Autostart, LoginService};
    use crate::testing::FakeSystem;
    use std::sync::{Arc, Mutex};

    type Log = Arc<Mutex<Vec<String>>>;

    struct Service(Log, bool);
    impl LoginService for Service {
        fn install(&mut self) -> Result<(), String> {
            self.0.lock().unwrap().push("service install".into());
            if self.1 {
                Err("refused".into())
            } else {
                Ok(())
            }
        }
        fn uninstall(&mut self) -> Result<(), String> {
            self.0.lock().unwrap().push("service uninstall".into());
            Ok(())
        }
    }

    struct Item(Log, bool);
    impl Autostart for Item {
        fn set(&mut self, on: bool) -> Result<(), String> {
            self.0.lock().unwrap().push(format!("autostart {on}"));
            if self.1 { Err("denied".into()) } else { Ok(()) }
        }
    }

    fn marker() -> FirstRunMarker {
        FirstRunMarker::new("/data/login-default-applied")
    }

    fn run(
        sys: &FakeSystem,
        wanted: bool,
        location: Result<(), String>,
        with_service: bool,
        item_fails: bool,
    ) -> (Result<FirstRun, String>, Vec<String>) {
        let log = Log::default();
        let mut s = Service(log.clone(), false);
        let mut a = Item(log.clone(), item_fails);
        let service = if with_service {
            Some(&mut s as &mut dyn LoginService)
        } else {
            None
        };
        let r = apply_first_run_with(
            &marker(),
            &mut sys.clone(),
            wanted,
            location,
            service,
            &mut a,
            &LoginWords::default(),
        );
        let calls = log.lock().unwrap().clone();
        (r, calls)
    }

    #[test]
    fn a_first_run_with_the_default_registers_both_once_and_never_again() {
        let sys = FakeSystem::new();
        let (r, log) = run(&sys, true, Ok(()), true, false);
        assert_eq!(r, Ok(FirstRun::Applied));
        assert_eq!(log, vec!["service install", "autostart true"]);
        assert_eq!(
            sys.file(marker().path()).as_deref(),
            Some("applied"),
            "the marker says what was decided"
        );
        // The user turns it off later; the next start sees the marker and leaves it off.
        let (r, log) = run(&sys, true, Ok(()), true, false);
        assert_eq!(r, Ok(FirstRun::Skipped));
        assert!(log.is_empty());
    }

    #[test]
    fn an_app_without_a_service_registers_only_its_own_item() {
        let sys = FakeSystem::new();
        let (r, log) = run(&sys, true, Ok(()), false, false);
        assert_eq!(r, Ok(FirstRun::Applied));
        assert_eq!(log, vec!["autostart true"]);
    }

    #[test]
    fn a_refused_service_leaves_no_marker_and_never_removes_anything() {
        let sys = FakeSystem::new();
        let log = Log::default();
        let mut s = Service(log.clone(), true);
        let mut a = Item(log.clone(), false);
        let r = apply_first_run_with(
            &marker(),
            &mut sys.clone(),
            true,
            Ok(()),
            Some(&mut s),
            &mut a,
            &LoginWords::default(),
        );
        assert!(r.unwrap_err().contains("could not be registered"));
        assert!(!marker().is_set(&mut sys.clone()));
        // Both were tried; a first run only ever registers.
        assert_eq!(
            *log.lock().unwrap(),
            vec!["service install", "autostart true"]
        );
        assert_eq!(s.uninstall(), Ok(()));
    }

    #[test]
    fn an_explicit_off_is_recorded_and_never_overridden() {
        let sys = FakeSystem::new();
        let (r, log) = run(&sys, false, Ok(()), true, false);
        assert_eq!(r, Ok(FirstRun::Skipped));
        assert!(log.is_empty(), "nothing registered: {log:?}");
        assert_eq!(sys.file(marker().path()).as_deref(), Some("off"));
    }

    #[test]
    fn a_failed_first_run_leaves_no_marker_so_the_next_start_tries_again() {
        let sys = FakeSystem::new();
        let (r, _) = run(&sys, true, Ok(()), true, true);
        let err = r.unwrap_err();
        assert!(
            err.contains("could not be added to the login items"),
            "{err}"
        );
        assert!(!marker().is_set(&mut sys.clone()));
    }

    #[test]
    fn an_unstable_location_registers_nothing_and_leaves_no_marker() {
        let sys = FakeSystem::new();
        let (r, log) = run(&sys, true, Err("move it".into()), true, false);
        assert!(r.unwrap_err().contains("move it"));
        assert!(log.is_empty());
        assert!(!marker().is_set(&mut sys.clone()));
    }

    #[test]
    fn a_marker_that_cannot_be_written_is_the_callers_error() {
        let sys = FakeSystem::new();
        sys.fail_writes(Some(std::io::ErrorKind::PermissionDenied));
        let (r, log) = run(&sys, true, Ok(()), true, false);
        assert!(r.unwrap_err().contains("first-run marker"));
        assert_eq!(log, vec!["service install", "autostart true"]);
        let (r, _) = run(&sys, false, Ok(()), true, false);
        assert!(r.unwrap_err().contains("first-run marker"));
    }
}
