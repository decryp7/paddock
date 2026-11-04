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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// By systemd, launchd or a console (`serve`): the body runs on this thread, SIGTERM and SIGINT mean stop (the handlers
    /// are installed once per process).
    Foreground,
    /// On this thread like [`Foreground`](Hosting::Foreground), with no signal handler installed (tests, and programs that
    /// stop the body themselves).
    Embedded,
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
        Arc::new(move |level, text| {
            EARLY_LINES.fetch_add(1, Ordering::SeqCst);
            log.write(level, &cut(text, MAX_OS_LOG_TEXT));
        })
    }
}

/// How many lines the early sinks have written to the OS log: the panic hook compares it before and after it logs, so a line
/// that already reached the OS log through the file log's early sink is not written a second time.
static EARLY_LINES: AtomicU64 = AtomicU64::new(0);

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

/// The words for a panic: what, where and on which thread. The panic's own message is left out (rule 56: it may carry user
/// content); the program's source line says where to look, and standard error keeps the full message.
pub fn panic_text(name: &str, location: Option<(&str, u32)>, thread: &str) -> String {
    let at = location.map_or_else(String::new, |(file, line)| format!(" at {file}:{line}"));
    format!("{name} panicked on thread '{thread}'{at}")
}

/// Makes a panic on any thread leave a trace in the OS log (a service has no console, so on its own it leaves nothing),
/// then runs the previous hook, which prints the panic's own message (one line, no user content in the OS log) to standard
/// error. The line goes through the program's logger first (its file log, or its early sink before the file is trusted); the
/// OS log is written directly only when the early sink did not already carry it and the OS log is not standard error itself
/// (the previous hook already writes there). Installed for the life of the process.
fn install_panic_hook(name: &str, os_log: Arc<dyn OsLog>) {
    let previous = std::panic::take_hook();
    let name = name.to_string();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let text = cut(
            &panic_text(
                &name,
                info.location().map(|l| (l.file(), l.line())),
                thread.name().unwrap_or("unnamed"),
            ),
            MAX_OS_LOG_TEXT,
        );
        let heard = EARLY_LINES.load(Ordering::SeqCst);
        log::error!("{text}");
        if EARLY_LINES.load(Ordering::SeqCst) == heard && !os_log.is_standard_error() {
            os_log.write(Level::Error, &text);
        }
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
            log::error!(ms = ms, code = code, error = text.as_str(); "service: failed");
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

#[cfg(any(windows, test))]
mod scm_status {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use std::time::Duration;
    /// What the Service Control Manager is told about the service's state.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum ScmState {
        /// `SERVICE_RUNNING`.
        Running,
        /// `SERVICE_STOP_PENDING`.
        StopPending,
        /// `SERVICE_STOPPED`.
        Stopped,
    }

    /// One status report: the state, the exit code it carries (0, or the service-specific code), the checkpoint and the wait
    /// hint that tell the SCM a stop in progress has not hung.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct ScmReport {
        pub(crate) state: ScmState,
        /// The host also takes session-change controls while running.
        pub(crate) accepts_sessions: bool,
        pub(crate) exit_code: u32,
        pub(crate) checkpoint: u32,
        pub(crate) wait_hint: Duration,
    }

    /// The SCM's `SetServiceStatus` (the Windows adapter; tests use a fake).
    pub(crate) trait StatusPort: Send + Sync {
        /// Reports `report`.
        ///
        /// # Errors
        /// The reason the SCM refused it, in words.
        fn report(&self, report: &ScmReport) -> Result<(), String>;
    }

    /// How long the SCM waits for a stop before it counts the service hung; each report of the stop raises the checkpoint and
    /// starts the wait again.
    pub(crate) const STOP_WAIT_HINT: Duration = Duration::from_secs(30);
    /// How often a stop in progress is reported again (a third of the hint).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) const STOP_TICK: Duration = Duration::from_secs(10);

    /// The status reports of one service run. Every report is checked: a refusal is logged to the app log and the OS log (the
    /// SCM then times the service out or keeps the old state).
    pub(crate) struct ScmStatus {
        port: Arc<dyn StatusPort>,
        os_log: Arc<dyn OsLog>,
        sessions: bool,
        checkpoint: AtomicU32,
    }

    impl ScmStatus {
        pub(crate) fn new(
            port: Arc<dyn StatusPort>,
            os_log: Arc<dyn OsLog>,
            sessions: bool,
        ) -> Self {
            Self {
                port,
                os_log,
                sessions,
                checkpoint: AtomicU32::new(0),
            }
        }

        fn send(
            &self,
            state: ScmState,
            exit_code: u32,
            checkpoint: u32,
            wait_hint: Duration,
        ) -> bool {
            let report = ScmReport {
                state,
                accepts_sessions: self.sessions,
                exit_code,
                checkpoint,
                wait_hint,
            };
            match self.port.report(&report) {
                Ok(()) => true,
                Err(why) => {
                    log::error!(state:? = state, error = why.as_str(); "the service status could not be reported to the Service Control Manager");
                    self.os_log.write(
                        Level::Error,
                        &format!("The Service Control Manager refused the state {state:?}: {why}"),
                    );
                    false
                }
            }
        }

        /// The service is running.
        pub(crate) fn running(&self) -> bool {
            self.send(ScmState::Running, 0, 0, Duration::ZERO)
        }

        /// A stop is in progress: the first report starts the wait, every later one raises the checkpoint.
        pub(crate) fn stop_pending(&self) -> bool {
            let checkpoint = self.checkpoint.fetch_add(1, Ordering::SeqCst) + 1;
            self.send(ScmState::StopPending, 0, checkpoint, STOP_WAIT_HINT)
        }

        /// The service has stopped with `code` (0: cleanly). A stop that began on its own (the body ended or failed) is reported
        /// as pending first.
        pub(crate) fn stopped(&self, code: u32) -> bool {
            if self.checkpoint.load(Ordering::SeqCst) == 0 {
                self.stop_pending();
            }
            self.send(ScmState::Stopped, code, 0, Duration::ZERO)
        }
    }

    /// Runs `job` as the SCM's service: reports Running, runs the body, keeps reporting the stop while the body winds down
    /// (every `tick` once `stop` is set), then reports Stopped with the body's code.
    pub(crate) fn run_under_scm(
        status: &Arc<ScmStatus>,
        job: Job,
        context: Context,
        tick: Duration,
    ) -> Outcome {
        let began = Instant::now();
        let running = status.running();
        log::info!(running_reported = running; "service started");
        let stop = context.stop.clone();
        let (done, finished) = std::sync::mpsc::channel::<()>();
        let ticker = {
            let status = status.clone();
            std::thread::Builder::new()
                .name("scm-stop-ticker".into())
                .spawn(move || {
                    // A stop in progress is reported again every tick until the body is done: the wait is the channel.
                    while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                        finished.recv_timeout(tick)
                    {
                        if stop.load(Ordering::SeqCst) {
                            status.stop_pending();
                        }
                    }
                })
        };
        let outcome = (job.run)(context);
        drop(done);
        match ticker {
            Ok(handle) => {
                if handle.join().is_err() {
                    log::error!("service: the stop ticker panicked");
                }
            }
            Err(e) => {
                log::error!(error:% = e; "service: the stop ticker could not start; a long stop is reported once")
            }
        }
        status.stopped(outcome.code);
        log::info!(code = outcome.code, ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX); "service stopped");
        outcome
    }
}

#[cfg(any(windows, test))]
pub(crate) use scm_status::*;

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
    use std::time::Duration;

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
    fn the_panic_hook_writes_the_thread_and_the_place_but_not_the_message_to_the_os_log_from_any_thread()
     {
        let log = RecordingOsLog::default();
        let before = std::panic::take_hook();
        install_panic_hook("TestService", Arc::new(log.clone()));
        let joined = std::thread::Builder::new()
            .name("probe-thread".into())
            .spawn(|| panic!("probe failure with user content"))
            .unwrap()
            .join();
        drop(std::panic::take_hook());
        std::panic::set_hook(before);
        assert!(joined.is_err());
        // Other tests of this process may panic meanwhile: find this one's line.
        let lines = log.lines();
        let ours = lines
            .iter()
            .find(|(_, t)| t.contains("probe-thread"))
            .unwrap_or_else(|| panic!("{lines:?}"));
        assert!(ours.1.contains("service_main.rs"), "{ours:?}");
        assert!(
            !ours.1.contains("user content"),
            "rule 56: the message may hold user content: {ours:?}"
        );
        assert_eq!(ours.0, Level::Error);
        // With a logger whose early sink is the same OS log, the panic reaches the OS log once, not twice.
        struct ToSink(EarlySink);
        impl log::Log for ToSink {
            fn enabled(&self, _: &log::Metadata<'_>) -> bool {
                true
            }
            fn log(&self, record: &log::Record<'_>) {
                (self.0)(record.level(), &record.args().to_string());
            }
            fn flush(&self) {}
        }
        let once = RecordingOsLog::default();
        let sink = context(&once).early_sink();
        // This is the only test of the crate that installs a logger; a failure here must not skip the check (ER-12).
        let installed = log::set_boxed_logger(Box::new(ToSink(sink)));
        assert!(
            installed.is_ok(),
            "the test logger could not be installed: {installed:?}"
        );
        {
            log::set_max_level(log::LevelFilter::Info);
            let before = std::panic::take_hook();
            install_panic_hook("TestService", Arc::new(once.clone()));
            let joined = std::thread::Builder::new()
                .name("dedupe-thread".into())
                .spawn(|| panic!("second probe"))
                .unwrap()
                .join();
            drop(std::panic::take_hook());
            std::panic::set_hook(before);
            assert!(joined.is_err());
            let heard = once
                .lines()
                .iter()
                .filter(|(_, t)| t.contains("dedupe-thread"))
                .count();
            assert_eq!(heard, 1, "{:?}", once.lines());
        }
    }

    #[test]
    fn a_panic_text_has_the_name_the_thread_and_the_place_only() {
        assert_eq!(
            panic_text("T", Some(("src/a.rs", 42)), "worker"),
            "T panicked on thread 'worker' at src/a.rs:42"
        );
        assert_eq!(panic_text("T", None, "main"), "T panicked on thread 'main'");
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
            Hosting::Embedded,
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
            Hosting::Embedded,
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

    /// A fake SCM: keeps every report, hands each to a channel too (a test waits on the condition, not on the clock), and can
    /// refuse a state.
    struct FakePort {
        seen: Mutex<Vec<ScmReport>>,
        feed: Mutex<std::sync::mpsc::Sender<ScmReport>>,
        refuse: Option<ScmState>,
    }

    impl FakePort {
        fn new(refuse: Option<ScmState>) -> (Arc<FakePort>, Receiver<ScmReport>) {
            let (tx, rx) = std::sync::mpsc::channel();
            (
                Arc::new(FakePort {
                    seen: Mutex::new(Vec::new()),
                    feed: Mutex::new(tx),
                    refuse,
                }),
                rx,
            )
        }

        fn reports(&self) -> Vec<ScmReport> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl StatusPort for FakePort {
        fn report(&self, report: &ScmReport) -> Result<(), String> {
            if self.refuse == Some(report.state) {
                return Err("access denied".into());
            }
            self.seen.lock().unwrap().push(report.clone());
            self.feed.lock().unwrap().send(report.clone()).unwrap();
            Ok(())
        }
    }

    fn job(
        body: impl FnOnce(Context) -> Result<(), Refused> + Send + 'static,
        log: &RecordingOsLog,
    ) -> Job {
        let log: Arc<dyn OsLog> = Arc::new(log.clone());
        Job {
            run: Box::new(move |context| run_body("T", log.as_ref(), context, body)),
            sessions: false,
        }
    }

    fn state_of(r: &ScmReport) -> (ScmState, u32, u32) {
        (r.state, r.exit_code, r.checkpoint)
    }

    #[test]
    fn a_clean_service_run_tells_the_scm_running_then_the_stop_pending_then_stopped_with_zero() {
        let (port, _rx) = FakePort::new(None);
        let log = RecordingOsLog::default();
        let status = Arc::new(ScmStatus::new(port.clone(), Arc::new(log.clone()), false));
        let outcome = run_under_scm(
            &status,
            job(|_| Ok(()), &log),
            context(&log),
            Duration::from_secs(3600),
        );
        assert_eq!(outcome.code, 0);
        let reports = port.reports();
        assert_eq!(
            reports.iter().map(state_of).collect::<Vec<_>>(),
            [
                (ScmState::Running, 0, 0),
                (ScmState::StopPending, 0, 1),
                (ScmState::Stopped, 0, 0)
            ]
        );
        assert_eq!(
            reports[1].wait_hint, STOP_WAIT_HINT,
            "the SCM is told how long a stop may take"
        );
        assert_eq!(reports[0].wait_hint, Duration::ZERO);
    }

    #[test]
    fn a_failed_body_stops_the_service_with_its_service_specific_code() {
        let (port, _rx) = FakePort::new(None);
        let log = RecordingOsLog::default();
        let status = Arc::new(ScmStatus::new(port.clone(), Arc::new(log.clone()), true));
        let outcome = run_under_scm(
            &status,
            job(|_| Err(Refused(3, None)), &log),
            context(&log),
            Duration::from_secs(3600),
        );
        assert_eq!(outcome.code, 3);
        let last = port.reports().pop().unwrap();
        assert_eq!((last.state, last.exit_code), (ScmState::Stopped, 3));
        assert!(last.accepts_sessions, "the host's flag is carried");
    }

    #[test]
    fn a_stop_in_progress_is_reported_again_with_a_rising_checkpoint_until_the_body_is_done() {
        let (port, rx) = FakePort::new(None);
        let log = RecordingOsLog::default();
        let status = Arc::new(ScmStatus::new(port.clone(), Arc::new(log.clone()), false));
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let (tx, events) = std::sync::mpsc::channel();
        let (stop, _unused, _events) = stop_pair();
        let context = Context {
            stop: stop.clone(),
            events,
            os_log: Arc::new(log.clone()),
        };
        // The body winds down slowly: it waits until the test has seen the repeated reports.
        let job = job(
            move |_| {
                wait.recv().unwrap();
                Ok(())
            },
            &log,
        );
        let runner = {
            let status = status.clone();
            std::thread::spawn(move || {
                run_under_scm(&status, job, context, Duration::from_millis(5))
            })
        };
        // The control handler's part: the stop begins (checkpoint 1) and the flag is set.
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(30)).unwrap().state,
            ScmState::Running
        );
        status.stop_pending();
        stop.store(true, Ordering::SeqCst);
        let mut checkpoints = Vec::new();
        while checkpoints.len() < 3 {
            let report = rx
                .recv_timeout(Duration::from_secs(30))
                .expect("the stop is reported again");
            assert_eq!(report.state, ScmState::StopPending);
            checkpoints.push(report.checkpoint);
        }
        assert!(
            checkpoints.windows(2).all(|w| w[0] < w[1]),
            "{checkpoints:?}"
        );
        go.send(()).unwrap();
        drop(tx);
        assert_eq!(runner.join().unwrap().code, 0);
        let last = port.reports().pop().unwrap();
        assert_eq!(
            (last.state, last.exit_code, last.checkpoint),
            (ScmState::Stopped, 0, 0)
        );
    }

    #[test]
    fn a_state_the_scm_refuses_is_logged_to_the_os_log_and_the_run_goes_on() {
        let (port, _rx) = FakePort::new(Some(ScmState::Running));
        let log = RecordingOsLog::default();
        let status = Arc::new(ScmStatus::new(port.clone(), Arc::new(log.clone()), false));
        let outcome = run_under_scm(
            &status,
            job(|_| Ok(()), &log),
            context(&log),
            Duration::from_secs(3600),
        );
        assert_eq!(outcome.code, 0);
        let lines = log.lines();
        assert!(
            lines.iter().any(|(l, t)| *l == Level::Error
                && t == "The Service Control Manager refused the state Running: access denied"),
            "{lines:?}"
        );
        assert_eq!(port.reports().last().unwrap().state, ScmState::Stopped);
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
            Hosting::Embedded,
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
