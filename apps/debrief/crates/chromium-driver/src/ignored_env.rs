//! The one place a release build reports a development-only environment variable it ignored: once per name, by name only
//! (never the value), because the environment must not decide which program or switch a shipped build uses.

use std::sync::Mutex;

static WARNED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// Logs that `name` is set but ignored, the first time only.
pub(crate) fn warn_once(name: &'static str) {
    if first_time(name) {
        log::warn!("{name} is set but ignored: a release build does not let the environment choose what it starts");
    }
}

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

    #[test]
    fn a_name_is_reported_once() {
        assert!(first_time("CHROMIUM_TEST_ONCE"));
        assert!(!first_time("CHROMIUM_TEST_ONCE"));
        assert!(first_time("CHROMIUM_TEST_ONCE_OTHER"));
    }
}
