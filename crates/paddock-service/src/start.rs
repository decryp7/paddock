//! Starting a registered service without elevation (the Windows SCM lets interactive users start a service whose
//! security descriptor says so, [`crate::hardening`]): after a restart a delayed or failed service is brought up by the
//! app that needs it instead of asking the user to Repair (and answer UAC) first.
//!
//! The logic is one pure function over the [`System`] port (`sc.exe query` and `sc.exe start`, both run with the
//! user's token) and a [`Pace`] (time and waiting, injected so tests never sleep). macOS and Linux have no unelevated
//! start (a launch daemon or a system unit needs root or polkit), so there it reports [`Revival::NotSupported`].
//! Every step is logged with its duration (ER-27).

use crate::parse::{has_last_exit, SC_RUNNING, SC_STOPPED, sc_exit, sc_state};
use crate::paths::SC;
use crate::port::System;
use paddock_env::Os;
use std::time::Duration;

/// `SERVICE_START_PENDING`.
pub const SC_START_PENDING: u32 = 2;
/// `SERVICE_STOP_PENDING`.
pub const SC_STOP_PENDING: u32 = 3;
/// `ERROR_SERVICE_ALREADY_RUNNING`: `sc start` on a service that is starting or running.
const ALREADY_RUNNING: i32 = 1056;
/// How often the state and the endpoint are looked at while waiting.
pub const POLL: Duration = Duration::from_millis(500);
/// How long a start is waited for by default (a service start is seconds; a pending start can take longer after a boot).
pub const DEFAULT_BUDGET: Duration = Duration::from_secs(15);

/// What the SCM says about the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceRun {
    /// Stopped. `win32` is the exit code it stopped with (0 or 1077 (never started since boot): not a failure; 1066: it ended with its own
    /// code, `specific`).
    Stopped {
        /// `WIN32_EXIT_CODE`.
        win32: u32,
        /// `SERVICE_EXIT_CODE`.
        specific: u32,
    },
    /// Starting.
    StartPending,
    /// Stopping.
    StopPending,
    /// Running.
    Running,
    /// Paused or any other state number.
    Other(u32),
}

impl ServiceRun {
    /// Stopped with an error rather than cleanly.
    pub fn failed(self) -> bool {
        matches!(self, ServiceRun::Stopped { win32, .. } if win32 != 0 && has_last_exit(win32))
    }
}

/// Time and waiting, injected: the real one sleeps, a test's advances a fake clock.
pub trait Pace {
    /// Time since the wait began.
    fn elapsed(&mut self) -> Duration;
    /// Waits `how_long`.
    fn pause(&mut self, how_long: Duration);
}

/// How the attempt ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revival {
    /// The endpoint answers.
    Up,
    /// This system has no unelevated start (or the service is not a Windows service).
    NotSupported,
    /// The service is not registered.
    NotInstalled,
    /// It is stopped with an error, or the start failed: the words say why (a person has to repair it).
    Down(String),
    /// It was starting (or running) but the endpoint did not answer in time; a later look may find it up.
    Waiting(String),
}

/// Reads the service's state with `sc query` (no rights needed beyond the user's own).
pub fn query(sys: &mut dyn System, name: &str) -> Result<Option<ServiceRun>, String> {
    match sys.run(SC, &["query", name]) {
        Ok(o) if o.code == Some(sc_exit::DOES_NOT_EXIST) => Ok(None),
        Ok(o) if o.success() => Ok(Some(parse_run(&o.stdout))),
        Ok(o) => Err(format!("sc query: {}", o.reason())),
        Err(e) => Err(format!("sc.exe {e}")),
    }
}

/// The state in `sc query` output.
pub fn parse_run(text: &str) -> ServiceRun {
    let number = |field: &str| -> u32 {
        text.lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                (k.trim() == field).then(|| v.split_whitespace().next()?.parse().ok())?
            })
            .unwrap_or(0)
    };
    match sc_state(text) {
        Some(SC_STOPPED) => ServiceRun::Stopped {
            win32: number("WIN32_EXIT_CODE"),
            specific: number("SERVICE_EXIT_CODE"),
        },
        Some(SC_START_PENDING) => ServiceRun::StartPending,
        Some(SC_STOP_PENDING) => ServiceRun::StopPending,
        Some(SC_RUNNING) => ServiceRun::Running,
        Some(other) => ServiceRun::Other(other),
        None => ServiceRun::Other(0),
    }
}

/// Words for a stopped service's exit codes. `explain` turns a service-specific code into the program's own words.
fn stopped_words(
    win32: u32,
    specific: u32,
    explain: &dyn Fn(u32) -> Option<&'static str>,
) -> String {
    match (win32, specific) {
        (1066, s) => match explain(s) {
            Some(why) => format!("the service stopped with its own exit code {s}: {why}"),
            None => format!("the service stopped with its own exit code {s}"),
        },
        (w, _) => format!("the service stopped with Windows error {w}"),
    }
}

/// Brings the service `name` up without elevation and waits up to `budget` for `ready` (the endpoint answers).
///
/// - the endpoint answers: [`Revival::Up`], nothing is started;
/// - starting or running: it is waited for (a service that is starting is not started again);
/// - stopped cleanly (never started since boot, or stopped): one `sc start`;
/// - stopped with an error: not started, [`Revival::Down`] with the exit codes (a failing service is repaired, not looped);
/// - the start fails or the service stops again right after it: [`Revival::Down`].
pub fn revive(
    sys: &mut dyn System,
    os: Os,
    name: &str,
    budget: Duration,
    pace: &mut dyn Pace,
    explain: &dyn Fn(u32) -> Option<&'static str>,
    ready: &mut dyn FnMut() -> bool,
) -> Revival {
    if os != Os::Windows {
        return Revival::NotSupported;
    }
    let began = pace.elapsed();
    let at = |pace: &mut dyn Pace| pace.elapsed().saturating_sub(began).as_millis();
    let mut started = false;
    loop {
        if ready() {
            log::info!(service = name, ms = at(pace), started; "the helper endpoint answers");
            return Revival::Up;
        }
        let state = match query(sys, name) {
            Ok(Some(s)) => s,
            Ok(None) => {
                log::warn!(service = name; "the service is not registered");
                return Revival::NotInstalled;
            }
            Err(e) => {
                log::warn!(service = name, error = e.as_str(); "the service state could not be read");
                return Revival::Down(format!("the service state could not be read ({e})"));
            }
        };
        log::debug!(service = name, state = format!("{state:?}"), ms = at(pace); "service state");
        match state {
            ServiceRun::Stopped { win32, specific } if started => {
                let why = format!(
                    "it stopped right after it was started: {}",
                    stopped_words(win32, specific, explain)
                );
                log::warn!(service = name, ms = at(pace); "{why}");
                return Revival::Down(why);
            }
            ServiceRun::Stopped { win32, specific } if state.failed() => {
                let why = stopped_words(win32, specific, explain);
                log::warn!(service = name, ms = at(pace); "not started, it is stopped with an error: {why}");
                return Revival::Down(why);
            }
            ServiceRun::Stopped { .. } => match sys.run(SC, &["start", name]) {
                Ok(o) if o.success() || o.code == Some(ALREADY_RUNNING) => {
                    started = true;
                    log::info!(service = name, ms = at(pace); "started the stopped service without elevation");
                }
                Ok(o) => {
                    let why = format!(
                        "the service could not be started without elevation (sc start: {})",
                        o.reason()
                    );
                    log::warn!(service = name, code = o.code.unwrap_or(-1), ms = at(pace); "{why}");
                    return Revival::Down(why);
                }
                Err(e) => {
                    let why = format!("the service could not be started ({e})");
                    log::warn!(service = name, ms = at(pace); "{why}");
                    return Revival::Down(why);
                }
            },
            ServiceRun::StartPending
            | ServiceRun::Running
            | ServiceRun::StopPending
            | ServiceRun::Other(_) => {}
        }
        if pace.elapsed().saturating_sub(began) >= budget {
            let why = format!(
                "the service is {state:?} and its endpoint did not answer within {} s",
                budget.as_secs()
            );
            log::warn!(service = name, ms = at(pace); "{why}");
            return Revival::Waiting(why);
        }
        pace.pause(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port::Output;
    use crate::testing::FakeSystem;
    use std::sync::{Arc, Mutex};

    const NAME: &str = "DynoHelper";

    /// A fake clock over the fake system's virtual time.
    struct Fake(FakeSystem);
    impl Pace for Fake {
        fn elapsed(&mut self) -> Duration {
            self.0.now()
        }
        fn pause(&mut self, how_long: Duration) {
            let mut s = self.0.clone();
            System::pause(&mut s, how_long);
        }
    }

    fn query_text(state: &str, win32: u32, specific: u32) -> Output {
        Output::ok(&format!(
            "\r\nSERVICE_NAME: {NAME}\r\n        TYPE               : 10  WIN32_OWN_PROCESS\r\n        STATE              : {state}\r\n        WIN32_EXIT_CODE    : {win32}  (0x0)\r\n        SERVICE_EXIT_CODE  : {specific}  (0x0)\r\n        CHECKPOINT         : 0x0\r\n"
        ))
    }

    /// A service that follows `script` one `sc query` at a time (the last entry repeats); `sc start` is counted.
    fn service(script: Vec<Output>, start: Output) -> (FakeSystem, Arc<Mutex<u32>>) {
        let sys = FakeSystem::new();
        let at = Arc::new(Mutex::new(0usize));
        let starts = Arc::new(Mutex::new(0u32));
        sys.on("sc query", move |_, _| {
            let mut i = at.lock().unwrap();
            let out = script[(*i).min(script.len() - 1)].clone();
            *i += 1;
            Ok(out)
        });
        let n = starts.clone();
        sys.on("sc start", move |_, _| {
            *n.lock().unwrap() += 1;
            Ok(start.clone())
        });
        (sys, starts)
    }

    fn run(sys: &FakeSystem, mut ready: impl FnMut(Duration) -> bool, budget: Duration) -> Revival {
        let mut pace = Fake(sys.clone());
        let probe = sys.clone();
        let mut sys = sys.clone();
        revive(
            &mut sys,
            Os::Windows,
            NAME,
            budget,
            &mut pace,
            &|c| (c == 3).then_some("the data folder is not protected"),
            &mut || ready(probe.now()),
        )
    }

    #[test]
    fn a_stopped_service_is_started_once_and_the_endpoint_answers() {
        let (sys, starts) = service(
            vec![
                query_text("1  STOPPED", 0, 0),
                query_text("2  START_PENDING", 0, 0),
                query_text("4  RUNNING", 0, 0),
            ],
            Output::ok("SERVICE_NAME: DynoHelper\r\n STATE : 2  START_PENDING"),
        );
        let out = run(&sys, |t| t >= Duration::from_secs(2), DEFAULT_BUDGET);
        assert_eq!(out, Revival::Up);
        assert_eq!(*starts.lock().unwrap(), 1, "exactly one start");
        assert!(
            sys.commands()
                .iter()
                .any(|c| c.ends_with("sc start DynoHelper")),
            "{:?}",
            sys.commands()
        );
    }

    #[test]
    fn a_service_that_is_starting_is_waited_for_not_started_again() {
        let (sys, starts) = service(
            vec![query_text("2  START_PENDING", 0, 0)],
            Output::code(1056),
        );
        let out = run(&sys, |t| t >= Duration::from_secs(4), DEFAULT_BUDGET);
        assert_eq!(out, Revival::Up);
        assert_eq!(*starts.lock().unwrap(), 0);
    }

    #[test]
    fn a_service_stopped_with_an_error_is_not_started_and_says_why() {
        let (sys, starts) = service(vec![query_text("1  STOPPED", 1066, 3)], Output::ok(""));
        let out = run(&sys, |_| false, DEFAULT_BUDGET);
        match out {
            Revival::Down(why) => {
                assert!(
                    why.contains("exit code 3") && why.contains("data folder is not protected"),
                    "{why}"
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            *starts.lock().unwrap(),
            0,
            "a failing service is repaired, not looped"
        );
        let (sys, _) = service(vec![query_text("1  STOPPED", 1, 0)], Output::ok(""));
        assert!(
            matches!(run(&sys, |_| false, DEFAULT_BUDGET), Revival::Down(w) if w.contains("Windows error 1"))
        );
    }

    #[test]
    fn a_failed_start_and_a_service_that_stops_again_are_down() {
        let (sys, _) = service(vec![query_text("1  STOPPED", 0, 0)], Output::code(5));
        assert!(
            matches!(run(&sys, |_| false, DEFAULT_BUDGET), Revival::Down(w) if w.contains("sc start"))
        );
        let (sys, starts) = service(
            vec![
                query_text("1  STOPPED", 0, 0),
                query_text("1  STOPPED", 1066, 3),
            ],
            Output::ok(""),
        );
        let out = run(&sys, |_| false, DEFAULT_BUDGET);
        assert!(
            matches!(out, Revival::Down(ref w) if w.contains("right after it was started")),
            "{out:?}"
        );
        assert_eq!(*starts.lock().unwrap(), 1);
    }

    #[test]
    fn a_service_that_never_answers_is_waiting_after_the_budget_not_down() {
        let (sys, _) = service(vec![query_text("2  START_PENDING", 0, 0)], Output::ok(""));
        let out = run(&sys, |_| false, Duration::from_secs(10));
        assert!(matches!(out, Revival::Waiting(_)), "{out:?}");
        assert!(
            sys.now() >= Duration::from_secs(10) && sys.now() < Duration::from_secs(12),
            "{:?}",
            sys.now()
        );
    }

    #[test]
    fn an_answering_endpoint_starts_nothing_and_other_systems_and_missing_services_are_reported() {
        let (sys, starts) = service(vec![query_text("1  STOPPED", 0, 0)], Output::ok(""));
        assert_eq!(run(&sys, |_| true, DEFAULT_BUDGET), Revival::Up);
        assert_eq!(*starts.lock().unwrap(), 0);
        let mut pace = Fake(sys.clone());
        let mut s = sys.clone();
        for os in [Os::MacOs, Os::Linux] {
            assert_eq!(
                revive(
                    &mut s,
                    os,
                    NAME,
                    DEFAULT_BUDGET,
                    &mut pace,
                    &|_| None,
                    &mut || false
                ),
                Revival::NotSupported
            );
        }
        let gone = FakeSystem::new();
        gone.answer("sc query", Output::code(1060));
        assert_eq!(run(&gone, |_| false, DEFAULT_BUDGET), Revival::NotInstalled);
        let broken = FakeSystem::new();
        broken.answer("sc query", Output::code(5));
        assert!(
            matches!(run(&broken, |_| false, DEFAULT_BUDGET), Revival::Down(w) if w.contains("could not be read"))
        );
    }

    #[test]
    fn query_output_is_read_into_states() {
        assert_eq!(
            parse_run(&query_text("4  RUNNING", 0, 0).stdout),
            ServiceRun::Running
        );
        assert_eq!(
            parse_run(&query_text("3  STOP_PENDING", 0, 0).stdout),
            ServiceRun::StopPending
        );
        assert_eq!(
            parse_run(&query_text("7  PAUSED", 0, 0).stdout),
            ServiceRun::Other(7)
        );
        assert_eq!(parse_run("nothing"), ServiceRun::Other(0));
        assert!(parse_run(&query_text("1  STOPPED", 1066, 3).stdout).failed());
        assert!(!parse_run(&query_text("1  STOPPED", 0, 0).stdout).failed());
    }
}

#[cfg(test)]
mod research_never_started {
    use super::*;
    use crate::port::Output;
    use crate::testing::FakeSystem;

    // ERROR_SERVICE_NEVER_STARTED: what `sc query` prints for a service that has not started since boot (parse.rs SC_NEVER_STARTED).
    #[test]
    fn a_service_that_never_started_since_boot_is_not_failed() {
        let text = "SERVICE_NAME: DynoHelper\r\n        STATE              : 1  STOPPED\r\n        WIN32_EXIT_CODE    : 1077  (0x435)\r\n        SERVICE_EXIT_CODE  : 0  (0x0)\r\n";
        assert!(!parse_run(text).failed());
    }

    struct Pause(FakeSystem);
    impl Pace for Pause {
        fn elapsed(&mut self) -> Duration {
            self.0.now()
        }
        fn pause(&mut self, how_long: Duration) {
            let mut s = self.0.clone();
            System::pause(&mut s, how_long);
        }
    }

    #[test]
    fn a_service_that_never_started_since_boot_is_started_without_elevation() {
        let sys = FakeSystem::new();
        sys.answer(
            "sc query",
            Output::ok("SERVICE_NAME: DynoHelper\r\n        STATE              : 1  STOPPED\r\n        WIN32_EXIT_CODE    : 1077  (0x435)\r\n        SERVICE_EXIT_CODE  : 0  (0x0)\r\n"),
        );
        sys.answer("sc start", Output::ok("STATE : 2  START_PENDING"));
        let mut pace = Pause(sys.clone());
        let mut s = sys.clone();
        let probe = sys.clone();
        let out = revive(&mut s, Os::Windows, "DynoHelper", Duration::from_secs(4), &mut pace, &|_| None, &mut || {
            probe.commands().iter().any(|c| c.ends_with("sc start DynoHelper"))
        });
        assert_eq!(out, Revival::Up, "{:?}", sys.commands());
    }
}
