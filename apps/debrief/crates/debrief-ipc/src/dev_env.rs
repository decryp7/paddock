//! Development-only environment overrides that choose which program Debrief starts.
//!
//! A release build must never let the environment decide which executable runs: `restart_elevated` relaunches Debrief
//! through `runas` with the user's environment, and `HKCU\Environment` is writable without elevation, so any process of
//! the user could otherwise pick the program that starts elevated (ADR 0073 DB9). Such a variable is honoured only when
//! `debug` is true (`cfg!(debug_assertions)`), the way Dyno treats `DYNO_AGENT_BIN`. In a release build a variable that
//! is set is ignored and logged once, by name only (never its value).

use std::ffi::OsString;
use std::sync::Mutex;

/// What an override variable means in this build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvOverride {
    /// Not set (or empty): use the installed location.
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

/// The value of the environment variable `name` when this is a debug build; in a release build always `None`, and
/// a set variable is logged once (its name only).
pub fn dev_override(name: &'static str) -> Option<OsString> {
    resolve(cfg!(debug_assertions), name, std::env::var_os(name))
}

/// [`dev_override`] with the build mode and the value given.
pub fn resolve(debug: bool, name: &'static str, value: Option<OsString>) -> Option<OsString> {
    match decide(debug, value) {
        EnvOverride::Use(v) => Some(v),
        EnvOverride::Unset => None,
        EnvOverride::Ignored => {
            if first_time(name) {
                log::warn!("{name} is set but ignored: a release build runs its programs from the install location only");
            }
            None
        }
    }
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
        assert_eq!(resolve(false, "DEBRIEF_TEST_A", v("/evil/worker")), None);
    }

    #[test]
    fn a_debug_build_uses_it() {
        assert_eq!(decide(true, v("/dev/worker")), EnvOverride::Use("/dev/worker".into()));
        assert_eq!(resolve(true, "DEBRIEF_TEST_B", v("/dev/worker")), Some("/dev/worker".into()));
    }

    #[test]
    fn unset_and_empty_are_unset_in_both_modes() {
        for debug in [true, false] {
            assert_eq!(decide(debug, None), EnvOverride::Unset);
            assert_eq!(decide(debug, v("")), EnvOverride::Unset);
        }
    }

    #[test]
    fn an_ignored_name_is_remembered_so_it_is_logged_once() {
        assert!(first_time("DEBRIEF_TEST_ONCE"));
        assert!(!first_time("DEBRIEF_TEST_ONCE"));
        assert!(first_time("DEBRIEF_TEST_ONCE_OTHER"));
    }
}
