//! The one entry point of a background program run by an OS service manager or by `serve` (ADR 0054 section 2.3): the
//! Windows Service Control Manager, systemd, launchd.
//!
//! [`service_main`] replaces the dispatcher each program wrapped itself (Dyno's `winsvc.rs`, `native::scm::run_as_service`).
//! A service has no console, and until its log file is trusted nowhere to write, so it does the following:
//!
//! - **Opens the OS log before anything else** ([`os_log::open`](crate::os_log::open): the Windows Application event log
//!   under the service's own source, the systemd journal through standard error, the macOS unified log through
//!   `syslog`). Every line before the file log opens, and every refusal, goes there ([`Context::early_sink`]).
//! - **Logs the start-up phases with their durations** and checks every status report it makes to the SCM.
//! - **Turns a failure into an exit code that names it.** The body returns `Result<(), E>` with `E:` [`ServiceExit`]. On
//!   `Err` the error with its cause chain and next step goes to the OS log, and the process stops with the error's own code:
//!   `ServiceExitCode::ServiceSpecific(code)` on Windows (the event log's 7024 entry and `sc query` carry it) and
//!   [`exit_status`] on Unix. A clean stop is 0.
//! - **Catches panics at the boundary**, logs them with their location from the first panic on any thread, and reports
//!   [`PANIC_EXIT_CODE`].
//!
//! The run itself ([`run_body`]) is pure over the [`OsLog`] port, so every path is tested on every OS; the dispatcher and the
//! signal wait are the native glue (`native.rs`).
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use crate::native::{Job, Slot};
use crate::os_log::{OsLog, cut};
use crate::scm_host::Control;
use crate::spec::ServiceSpec;
use log::Level;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

/// The service-specific exit code of a panic. A service's own table stays below it.
pub const PANIC_EXIT_CODE: u32 = 1000;
/// The code when the service manager could not be told the control handler (Windows) or the signals could not be waited
/// for (Unix): the service did not start.
pub const HOST_EXIT_CODE: u32 = 1001;
/// The longest line sent to the OS log (the event log takes about 31 000 characters; a runaway reason is cut).
pub const MAX_OS_LOG_TEXT: usize = 2000;

/// An error a service body fails with: its own exit code and what to do next (one table per service).
pub trait ServiceExit: std::error::Error {
    /// The service-specific exit code (never 0, which means a clean stop; at most [`PANIC_EXIT_CODE`] - 1).
    fn exit_code(&self) -> u32;
    /// What the person reading the log does next (`None`: the message says it).
    fn next_step(&self) -> Option<String> {
        None
    }
}

/// How the program is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hosting {
    /// By the Windows Service Control Manager (`--service`): the dispatcher runs the body and reports its state.
    ServiceManager,
    /// By systemd, launchd or a console (`serve`): the body runs on this thread, SIGTERM and SIGINT mean stop.
    Foreground,
}

/// What the body gets.
pub struct Context {
    /// Set when the service must stop (the SCM's stop or shutdown, SIGTERM, SIGINT).
    pub stop: Arc<AtomicBool>,
    /// The control events in order (a stop, a session change); a body that only polls `stop` ignores it.
    pub events: Receiver<Control>,
    /// The OS log, open from the first line.
    pub os_log: Arc<dyn OsLog>,
}

/// The sink for the lines written before the file log may open (a clone of the OS log's writer).
pub type EarlySink = Arc<dyn Fn(Level, &str) + Send + Sync>;

impl Context {
    /// The OS log as the early sink of a deferred file log.
    pub fn early_sink(&self) -> EarlySink {
        let log = self.os_log.clone();
        Arc::new(move |level, text| log.write(level, &cut(text, MAX_OS_LOG_TEXT)))
    }
}

/// Why the program could not be run as a service at all (the body never ran).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceMainError {
    /// The service manager does not own this process (started from a console, or this OS has no such manager).
    NotStartedByManager(String),
}

impl std::fmt::Display for ServiceMainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceMainError::NotStartedByManager(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ServiceMainError {}

/// The `u8`-sized status a Unix process exits with for service-specific `code`: 0 stays 0, anything else keeps its low
/// byte and is never 0 (a code of 256 would otherwise read as success).
pub fn exit_status(code: u32) -> i32 {
    match code {
        0 => 0,
        c => match i32::try_from(c & 0xFF) {
            Ok(0) | Err(_) => 1,
            Ok(low) => low,
        },
    }
}

/// The text of a failed run for the OS log: what, the cause chain, the exit code and the next step.
pub fn failure_text(
    name: &str,
    error: &dyn std::error::Error,
    code: u32,
    next_step: Option<&str>,
) -> String {
    let mut text = format!("{name} did not run (exit code {code}): {error}");
    let mut cause = error.source();
    while let Some(c) = cause {
        text.push_str(&format!("; caused by: {c}"));
        cause = c.source();
    }
    if let Some(step) = next_step.map(str::trim).filter(|s| !s.is_empty()) {
        text.push_str(&format!(" Next step: {step}"));
    }
    cut(&text, MAX_OS_LOG_TEXT)
}

/// The words for a panic: what, where and on which thread (no user content: the program's own message and source line).
pub fn panic_text(
    name: &str,
    message: &str,
    location: Option<(&str, u32)>,
    thread: &str,
) -> String {
    let at = location.map_or_else(String::new, |(file, line)| format!(" at {file}:{line}"));
    format!(
        "{name} panicked on thread '{thread}'{at}: {}",
        message.trim()
    )
}

/// Makes a panic on any thread leave a trace in the OS log (a service has no console, so on its own it leaves nothing),
/// then runs the previous hook. Installed for the life of the process; the log is the one of the run.
fn install_panic_hook(name: &str, os_log: Arc<dyn OsLog>) {
    let previous = std::panic::take_hook();
    let name = name.to_string();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let thread = std::thread::current();
        let text = panic_text(
            &name,
            &message,
            info.location().map(|l| (l.file(), l.line())),
            thread.name().unwrap_or("unnamed"),
        );
        log::error!("{text}");
        os_log.write(Level::Error, &cut(&text, MAX_OS_LOG_TEXT));
        previous(info);
    }));
}

/// What a finished run reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// 0 for a clean stop, else the failure's own code, or [`PANIC_EXIT_CODE`].
    pub code: u32,
}

/// Runs `body` with `context`: logs the phase and its duration, a failure with its cause chain and next step, a panic as
/// [`PANIC_EXIT_CODE`]. Never panics and never returns 0 for a failure.
pub fn run_body<E: ServiceExit>(
    name: &str,
    os_log: &dyn OsLog,
    context: Context,
    body: impl FnOnce(Context) -> Result<(), E>,
) -> Outcome {
    let began = Instant::now();
    os_log.write(Level::Info, &format!("{name} is starting"));
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| body(context)));
    let ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
    match result {
        Ok(Ok(())) => {
            log::info!(ms = ms; "service: stopped");
            os_log.write(
                Level::Info,
                &format!("{name} stopped cleanly after {ms} ms"),
            );
            Outcome { code: 0 }
        }
        Ok(Err(e)) => {
            let code = e.exit_code().max(1);
            let text = failure_text(name, &e, code, e.next_step().as_deref());
            log::error!(ms = ms, code = code; "service: failed");
            os_log.write(Level::Error, &text);
            Outcome { code }
        }
        // The panic hook has already written the location and message; this line says what the run did about it.
        Err(_) => {
            log::error!(ms = ms; "service: panicked");
            os_log.write(
                Level::Error,
                &format!(
                    "{name} stopped after {ms} ms because it panicked (exit code {PANIC_EXIT_CODE})"
                ),
            );
            Outcome {
                code: PANIC_EXIT_CODE,
            }
        }
    }
}

/// Runs `body` as the service `spec` under `hosting` and returns the exit code it ended with (0: clean stop). The OS log is
/// opened first; the caller exits the process with [`exit_status`] of the code (Windows reported it to the SCM already).
///
/// # Errors
/// [`ServiceMainError`] when this process is not run by a service manager and `hosting` says it is (the body never ran; the
/// reason is also in the OS log).
pub fn service_main<E>(
    spec: &ServiceSpec,
    hosting: Hosting,
    body: impl FnOnce(Context) -> Result<(), E> + Send + 'static,
) -> Result<u32, ServiceMainError>
where
    E: ServiceExit + 'static,
{
    service_main_with(spec, hosting, false, body)
}

/// [`service_main`] for a body that also wants the session-change events of the SCM (the Windows service host).
pub(crate) fn service_main_with<E>(
    spec: &ServiceSpec,
    hosting: Hosting,
    sessions: bool,
    body: impl FnOnce(Context) -> Result<(), E> + Send + 'static,
) -> Result<u32, ServiceMainError>
where
    E: ServiceExit + 'static,
{
    let os_log = crate::os_log::open(spec);
    install_panic_hook(spec.name(), os_log.clone());
    run_hosted(spec, hosting, sessions, os_log, body)
}

/// [`service_main_with`] over an already open OS log (tests pass a recording one).
pub(crate) fn run_hosted<E>(
    spec: &ServiceSpec,
    hosting: Hosting,
    sessions: bool,
    os_log: Arc<dyn OsLog>,
    body: impl FnOnce(Context) -> Result<(), E> + Send + 'static,
) -> Result<u32, ServiceMainError>
where
    E: ServiceExit + 'static,
{
    let name = spec.name().to_string();
    let run: Box<dyn FnOnce(Context) -> Outcome + Send> = {
        let (log, name) = (os_log.clone(), name.clone());
        Box::new(move |context| run_body(&name, log.as_ref(), context, body))
    };
    let slot: Slot = Arc::new(Mutex::new(Some(Job { run, sessions })));
    let result = crate::native::host(spec, hosting, os_log.clone(), slot);
    if let Err(e) = &result {
        log::error!(error:% = e; "service: could not be hosted");
        os_log.write(
            Level::Error,
            &cut(
                &format!("{name} could not run as a service: {e}"),
                MAX_OS_LOG_TEXT,
            ),
        );
    }
    result
}

/// Takes the job out of `slot` (it runs once per process; the SCM may call a service's main only once).
pub(crate) fn take_job(slot: &Mutex<Option<Job>>) -> Option<Job> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// A stop flag with the channel of control events (hosts set the flag, then send the event).
pub(crate) fn stop_pair() -> (Arc<AtomicBool>, Sender<Control>, Receiver<Control>) {
    let (tx, rx) = std::sync::mpsc::channel();
    (Arc::new(AtomicBool::new(false)), tx, rx)
}

/// Marks the stop: the flag first (a poller sees it), then the event (a waiter wakes).
pub(crate) fn signal_stop(stop: &AtomicBool, tx: &Sender<Control>) {
    stop.store(true, Ordering::SeqCst);
    // The body may have finished and dropped its receiver: nothing is left to wake.
    paddock_proc::cleanup::logged(
        "forwarding a stop after the body ended",
        tx.send(Control::Stop),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RecordingOsLog;

    #[derive(Debug)]
    struct Cause;

    impl std::fmt::Display for Cause {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("the folder is writable by Users")
        }
    }

    impl std::error::Error for Cause {}

    #[derive(Debug)]
    struct Refused(u32, Option<Cause>);

    impl std::fmt::Display for Refused {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("the data folder is not protected")
        }
    }

    impl std::error::Error for Refused {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1
                .as_ref()
                .map(|c| c as &(dyn std::error::Error + 'static))
        }
    }

    impl ServiceExit for Refused {
        fn exit_code(&self) -> u32 {
            self.0
        }
        fn next_step(&self) -> Option<String> {
            Some("Press Repair in Dyno.".into())
        }
    }

    fn context(log: &RecordingOsLog) -> Context {
        let (stop, _tx, events) = stop_pair();
        Context {
            stop,
            events,
            os_log: Arc::new(log.clone()),
        }
    }

    fn spec() -> ServiceSpec {
        ServiceSpec::builder(
            "net.decryptology.test.service",
            "TestService",
            "test-service",
        )
        .build()
        .unwrap()
    }

    #[test]
    fn a_clean_stop_is_code_zero_and_the_os_log_has_the_start_and_the_stop() {
        let log = RecordingOsLog::default();
        let outcome = run_body(
            "TestService",
            &log,
            context(&log),
            |_| Ok::<(), Refused>(()),
        );
        assert_eq!(outcome, Outcome { code: 0 });
        let lines = log.lines();
        assert_eq!(
            lines[0],
            (Level::Info, "TestService is starting".to_string())
        );
        assert!(
            lines[1].1.starts_with("TestService stopped cleanly after "),
            "{lines:?}"
        );
        assert!(lines.iter().all(|(level, _)| *level == Level::Info));
    }

    #[test]
    fn a_failure_goes_to_the_os_log_with_its_cause_chain_code_and_next_step_and_is_the_exit_code() {
        let log = RecordingOsLog::default();
        let outcome = run_body("TestService", &log, context(&log), |_| {
            Err(Refused(3, Some(Cause)))
        });
        assert_eq!(outcome.code, 3);
        let (level, text) = log.lines().pop().unwrap();
        assert_eq!(level, Level::Error);
        assert_eq!(
            text,
            "TestService did not run (exit code 3): the data folder is not protected; caused by: the folder is writable by Users Next step: Press Repair in Dyno."
        );
    }

    #[derive(Debug)]
    struct Plain;

    impl std::fmt::Display for Plain {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("plain failure")
        }
    }

    impl std::error::Error for Plain {}

    impl ServiceExit for Plain {
        fn exit_code(&self) -> u32 {
            9
        }
    }

    #[test]
    fn an_error_without_a_next_step_logs_only_what_and_why() {
        let log = RecordingOsLog::default();
        assert_eq!(run_body("T", &log, context(&log), |_| Err(Plain)).code, 9);
        assert_eq!(
            log.lines().pop().unwrap().1,
            "T did not run (exit code 9): plain failure"
        );
    }

    #[test]
    fn a_failure_never_exits_with_zero() {
        let log = RecordingOsLog::default();
        assert_eq!(
            run_body("T", &log, context(&log), |_| Err(Refused(0, None))).code,
            1
        );
    }

    #[test]
    fn a_panic_is_caught_logged_and_becomes_the_panic_exit_code() {
        let log = RecordingOsLog::default();
        let outcome = run_body("T", &log, context(&log), |_| -> Result<(), Refused> {
            panic!("probe panic")
        });
        assert_eq!(outcome.code, PANIC_EXIT_CODE);
        let text = log.lines().pop().unwrap().1;
        assert!(
            text.contains("panicked") && text.contains(&PANIC_EXIT_CODE.to_string()),
            "{text}"
        );
    }

    #[test]
    fn the_panic_hook_writes_the_thread_the_place_and_the_message_to_the_os_log_from_any_thread() {
        let log = RecordingOsLog::default();
        let before = std::panic::take_hook();
        install_panic_hook("TestService", Arc::new(log.clone()));
        let joined = std::thread::Builder::new()
            .name("probe-thread".into())
            .spawn(|| panic!("probe failure"))
            .unwrap()
            .join();
        drop(std::panic::take_hook());
        std::panic::set_hook(before);
        assert!(joined.is_err());
        let text = log.lines().pop().unwrap().1;
        assert!(
            text.contains("probe-thread")
                && text.contains("probe failure")
                && text.contains("service_main.rs"),
            "{text}"
        );
    }

    #[test]
    fn exit_codes_survive_as_unix_statuses_and_never_read_as_success() {
        assert_eq!(exit_status(0), 0);
        assert_eq!(exit_status(3), 3);
        assert_eq!(exit_status(255), 255);
        assert_eq!(exit_status(256), 1);
        assert_eq!(exit_status(PANIC_EXIT_CODE), 232);
        assert_eq!(exit_status(u32::MAX), 255);
    }

    #[test]
    fn a_runaway_failure_text_is_cut_and_a_missing_next_step_is_left_out() {
        let text = failure_text("T", &Refused(5, None), 5, None);
        assert_eq!(
            text,
            "T did not run (exit code 5): the data folder is not protected"
        );
        let long = failure_text(
            "T",
            &std::io::Error::other("é".repeat(MAX_OS_LOG_TEXT * 2)),
            1,
            Some("  "),
        );
        assert_eq!(long.chars().count(), MAX_OS_LOG_TEXT);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn the_early_sink_hands_lines_to_the_os_log_at_their_level() {
        let log = RecordingOsLog::default();
        let sink = context(&log).early_sink();
        sink(Level::Warn, "before the data folder is verified");
        assert_eq!(
            log.lines(),
            [(
                Level::Warn,
                "before the data folder is verified".to_string()
            )]
        );
    }

    #[test]
    fn a_stop_sets_the_flag_before_the_event() {
        let (stop, tx, rx) = stop_pair();
        signal_stop(&stop, &tx);
        assert!(stop.load(Ordering::SeqCst));
        assert_eq!(rx.try_recv(), Ok(Control::Stop));
        drop(rx);
        signal_stop(&stop, &tx);
    }

    #[test]
    fn a_foreground_run_returns_the_code_of_the_body_and_logs_to_the_given_os_log() {
        let log = RecordingOsLog::default();
        let ok = run_hosted(
            &spec(),
            Hosting::Foreground,
            false,
            Arc::new(log.clone()),
            |context| {
                assert!(!context.stop.load(Ordering::SeqCst));
                Ok::<(), Refused>(())
            },
        );
        assert_eq!(ok, Ok(0));
        let failed = run_hosted(
            &spec(),
            Hosting::Foreground,
            false,
            Arc::new(log.clone()),
            |_| Err(Refused(4, None)),
        );
        assert_eq!(failed, Ok(4));
        assert!(
            log.lines()
                .iter()
                .any(|(l, t)| *l == Level::Error && t.contains("exit code 4"))
        );
    }

    #[cfg(not(windows))]
    fn clean(_: Context) -> Result<(), Refused> {
        Ok(())
    }

    #[cfg(not(windows))]
    #[test]
    fn the_service_manager_is_refused_off_windows_and_the_reason_reaches_the_os_log() {
        let log = RecordingOsLog::default();
        let result = run_hosted(
            &spec(),
            Hosting::Foreground,
            false,
            Arc::new(log.clone()),
            clean,
        );
        assert_eq!(
            result,
            Ok(0),
            "the same body runs when the hosting allows it"
        );
        let result = run_hosted(
            &spec(),
            Hosting::ServiceManager,
            false,
            Arc::new(log.clone()),
            clean,
        );
        assert!(
            matches!(&result, Err(ServiceMainError::NotStartedByManager(why)) if why.contains("for Windows")),
            "{result:?}"
        );
        assert!(
            log.lines()
                .iter()
                .any(|(l, t)| *l == Level::Error && t.contains("could not run as a service"))
        );
        assert_eq!(
            ServiceMainError::NotStartedByManager("x".into()).to_string(),
            "x"
        );
    }
}
