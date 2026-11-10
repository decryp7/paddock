//! The ignored override is logged once, by name, never with its value (own process: it installs the global logger).

use std::sync::Mutex;

use paddock_env::dev_override::resolve;

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        LINES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(format!("{}: {}", record.level(), record.args()));
    }
    fn flush(&self) {}
}

#[test]
fn an_ignored_variable_is_logged_once_by_name_and_never_by_value() {
    log::set_logger(&Capture).expect("the only logger of this test process");
    log::set_max_level(log::LevelFilter::Warn);

    for _ in 0..3 {
        assert_eq!(
            resolve(false, "PADDOCK_LOG_TEST", Some("/secret/evil-path".into())),
            None
        );
    }
    // A debug build uses it and logs nothing; another name is logged on its own.
    assert_eq!(
        resolve(true, "PADDOCK_LOG_TEST", Some("/dev/x".into())),
        Some("/dev/x".into())
    );
    assert_eq!(
        resolve(
            false,
            "PADDOCK_LOG_TEST_2",
            Some("/secret/evil-path".into())
        ),
        None
    );

    // A caller that parses the variable itself reports through the same once-per-name list.
    paddock_env::dev_override::warn_ignored("PADDOCK_LOG_TEST_2");
    paddock_env::dev_override::warn_ignored("PADDOCK_LOG_TEST_3");

    let lines = LINES.lock().unwrap().clone();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines[0].starts_with("WARN: PADDOCK_LOG_TEST is set but ignored"),
        "{lines:?}"
    );
    assert!(lines[1].contains("PADDOCK_LOG_TEST_2"), "{lines:?}");
    assert!(lines[2].contains("PADDOCK_LOG_TEST_3"), "{lines:?}");
    assert!(
        lines
            .iter()
            .all(|l| !l.contains("evil-path") && !l.contains("/dev/x")),
        "{lines:?}"
    );
}

#[test]
fn the_macro_expands_the_callers_debug_flag() {
    // This test crate is built with the profile's debug assertions, the same flag `cfg!` sees here.
    let name = "PADDOCK_MACRO_TEST_UNSET";
    assert_eq!(paddock_env::dev_override!(name), None);
}
