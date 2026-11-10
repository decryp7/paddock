//! Development-only environment overrides (ADR 0073 DB1, DB9, WP-S).
//!
//! A release build must never let the environment decide which program runs or which safety switch is off: a process of
//! the user can set a variable (`HKCU\Environment` is writable without elevation), and an elevated relaunch would inherit
//! it. A variable that picks a program is therefore honoured only in a debug build. In a release build a variable that
//! is set is ignored and logged once, by name only, never its value.
//!
//! # Whose build mode counts
//!
//! The mode is decided by `cfg!(debug_assertions)` **in the crate that asks**, not in `paddock-env`. A `cfg!` here would
//! be evaluated in `paddock-env`'s own compilation; one Cargo build uses one profile for every crate, so the two agree
//! today, but a per-package profile override (`[profile.release.package.paddock-env] debug-assertions = true`) or a
//! prebuilt copy of this crate would silently make a release app honour the variable. So the decision functions take the
//! flag as an argument ([`decide`], [`resolve`]), and the one-line [`dev_override!`](crate::dev_override!) macro expands
//! `cfg!(debug_assertions)` at the call site, so a caller cannot forget it or evaluate it in the wrong crate.

use std::ffi::OsString;
use std::sync::Mutex;

/// What an override variable means in this build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvOverride {
    /// Not set (or empty): use the installed location or the default.
    Unset,
    /// Set in a debug build: use this value.
    Use(OsString),
    /// Set in a release build: ignored.
    Ignored,
}

/// The pure decision: `value` counts only when `debug`; an empty value is unset.
pub fn decide(debug: bool, value: Option<OsString>) -> EnvOverride {
    match value.filter(|v| !v.is_empty()) {
        None => EnvOverride::Unset,
        Some(v) if debug => EnvOverride::Use(v),
        Some(_) => EnvOverride::Ignored,
    }
}

/// The value of the variable `name` (as given in `value`) when `debug` is true; otherwise `None`, and a set variable is
/// logged once per name at `warn` level (the name only, never the value). A poisoned lock does not stop the answer.
pub fn resolve(debug: bool, name: &'static str, value: Option<OsString>) -> Option<OsString> {
    match decide(debug, value) {
        EnvOverride::Use(v) => Some(v),
        EnvOverride::Unset => None,
        EnvOverride::Ignored => {
            warn_ignored(name);
            None
        }
    }
}

/// Logs that `name` is set but ignored, the first time only (the name, never the value). For callers that parse the
/// variable themselves but want the same one-time report.
pub fn warn_ignored(name: &'static str) {
    if first_time(name) {
        log::warn!("{name} is set but ignored: only a development build honours it");
    }
}

/// [`resolve`] reading the variable `name` from the process environment.
///
/// Prefer the [`dev_override!`](crate::dev_override!) macro, which passes the caller's `cfg!(debug_assertions)`.
pub fn from_environment(debug: bool, name: &'static str) -> Option<OsString> {
    resolve(debug, name, std::env::var_os(name))
}

/// The value of the environment variable `$name` (a `&'static str`) in a debug build of the calling crate, else `None`
/// (logged once by name when it is set). `cfg!(debug_assertions)` is expanded in the caller's crate.
#[macro_export]
macro_rules! dev_override {
    ($name:expr) => {
        $crate::dev_override::from_environment(cfg!(debug_assertions), $name)
    };
}

static WARNED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// True the first time `name` is seen (a poisoned lock still answers).
fn first_time(name: &'static str) -> bool {
    let mut seen = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if seen.contains(&name) {
        false
    } else {
        seen.push(name);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Option<OsString> {
        Some(s.into())
    }

    #[test]
    fn a_release_build_ignores_the_variable() {
        assert_eq!(decide(false, v("/evil/worker")), EnvOverride::Ignored);
        assert_eq!(resolve(false, "PADDOCK_TEST_A", v("/evil/worker")), None);
    }

    #[test]
    fn a_debug_build_uses_it() {
        assert_eq!(
            decide(true, v("/dev/worker")),
            EnvOverride::Use("/dev/worker".into())
        );
        assert_eq!(
            resolve(true, "PADDOCK_TEST_B", v("/dev/worker")),
            Some("/dev/worker".into())
        );
    }

    #[test]
    fn unset_and_empty_are_unset_in_both_modes() {
        for debug in [true, false] {
            assert_eq!(decide(debug, None), EnvOverride::Unset);
            assert_eq!(decide(debug, v("")), EnvOverride::Unset);
            assert_eq!(resolve(debug, "PADDOCK_TEST_C", v("")), None);
        }
    }

    #[test]
    fn an_ignored_name_is_remembered_so_it_is_logged_once() {
        assert!(first_time("PADDOCK_TEST_ONCE"));
        assert!(!first_time("PADDOCK_TEST_ONCE"));
        assert!(first_time("PADDOCK_TEST_ONCE_OTHER"));
    }

    #[test]
    fn a_poisoned_lock_still_answers() {
        let _ = std::thread::spawn(|| {
            let _held = WARNED.lock().unwrap();
            panic!("poison the log-once list on purpose");
        })
        .join();
        assert!(WARNED.is_poisoned());
        assert!(first_time("PADDOCK_TEST_POISONED"));
        assert!(!first_time("PADDOCK_TEST_POISONED"));
        assert_eq!(resolve(false, "PADDOCK_TEST_POISONED_2", v("x")), None);
    }

    #[test]
    fn the_environment_is_read_by_name_and_an_unset_name_is_none() {
        assert_eq!(
            from_environment(true, "PADDOCK_DEV_OVERRIDE_SURELY_UNSET"),
            None
        );
        assert_eq!(
            crate::dev_override!("PADDOCK_DEV_OVERRIDE_SURELY_UNSET"),
            None
        );
    }
}
