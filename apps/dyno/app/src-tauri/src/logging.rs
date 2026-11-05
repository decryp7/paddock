//! Dyno's own lines (connecting, starting the agent, window and tray events) go to the same dated paddock-log file as the shared
//! crates' lines (`dyno.<day>.log`, installed first in `run`): one CLEF file per process with microsecond stamps, the session
//! and process ids and the redaction layer, which the Report-a-problem bundle reads. The former `logs/app.log` writer is gone
//! (rule 55: one implementation). Names, codes and millisecond counts only; never file contents or command lines.

static HANDLE: std::sync::OnceLock<paddock_log::LogHandle> = std::sync::OnceLock::new();

/// Keeps the installed log's handle for Report a problem (its Logging section and the level).
pub fn set_handle(handle: paddock_log::LogHandle) {
    let _ = HANDLE.set(handle);
}

/// The installed log's handle, when there is one.
pub fn handle() -> Option<paddock_log::LogHandle> {
    HANDLE.get().cloned()
}

static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Milliseconds since the app began (the first call, made first thing in `run`): the `UptimeMs` of the start lines, so the gap
/// from process start to the login decision and to each later step reads straight off the log.
pub fn uptime_ms() -> u64 {
    u64::try_from(STARTED.get_or_init(std::time::Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Logs a line at info (to standard error too, as before, for a development run).
pub fn info(message: &str) {
    eprintln!("dyno: {message}");
    log::info!(target: "dyno", "{message}");
}

/// Logs a line at warn: something failed that the app carries on from (a reading that could not be made, a registration that was
/// refused). A failure message belongs here, not at info, so a log read at Warn shows what went wrong.
pub fn warn(message: &str) {
    eprintln!("dyno: {message}");
    log::warn!(target: "dyno", "{message}");
}

/// Logs a line at debug (the log file only: it is for following one flow, such as the tray's image sets per tick).
pub fn debug(message: &str) {
    log::debug!(target: "dyno", "{message}");
}

/// The lines the tests of any module logged (one global capture: the `log` crate allows one logger). Assert on lines with a
/// unique id of your own, since tests run in parallel.
#[cfg(test)]
pub(crate) mod testlog {
    use std::sync::{Mutex, OnceLock};

    struct Capture(Mutex<Vec<String>>);

    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }
        fn log(&self, r: &log::Record) {
            self.0.lock().unwrap().push(format!("{} {}: {}", r.level(), r.target(), r.args()));
        }
        fn flush(&self) {}
    }

    fn capture() -> &'static Capture {
        static C: OnceLock<&'static Capture> = OnceLock::new();
        C.get_or_init(|| {
            let c: &'static Capture = Box::leak(Box::new(Capture(Mutex::new(Vec::new()))));
            let _ = log::set_logger(c);
            log::set_max_level(log::LevelFilter::Trace);
            c
        })
    }

    /// Every line logged so far, as `LEVEL target: message`.
    pub fn lines() -> Vec<String> {
        capture().0.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn uptime_counts_from_the_first_call_and_never_goes_back() {
        let a = super::uptime_ms();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(super::uptime_ms() >= a);
    }

    #[test]
    fn dynos_lines_go_to_the_shared_log_not_to_a_file_of_their_own() {
        super::info("connected to the agent");
        let lines = super::testlog::lines();
        assert!(lines.iter().any(|l| l == "INFO dyno: connected to the agent"), "{lines:?}");
    }

    #[test]
    fn a_failure_is_logged_at_warn() {
        // The capture installs itself on the first read, so read once before logging (a line logged before it exists is lost).
        let _ = super::testlog::lines();
        super::warn("could not read the power state: the agent did not answer");
        let lines = super::testlog::lines();
        assert!(
            lines
                .iter()
                .any(|l| l == "WARN dyno: could not read the power state: the agent did not answer"),
            "{lines:?}"
        );
    }
}
